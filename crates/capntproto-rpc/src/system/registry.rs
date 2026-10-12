//! Hosted connection ownership, independent of a protocol driver or transport.
//! Registrations cover construction, live lookup and final transport shutdown.
use capnp::{capability::Promise, Error};
use capntproto_rpc_engine::completion::shared;
use futures::{future::Shared, FutureExt};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::{Rc, Weak},
};

pub(crate) trait Connection {
    fn disconnect(&self, error: Error);
}

struct Entry<S> {
    identity: Rc<()>,
    state: Option<Rc<S>>,
}
struct Completion {
    remaining: Cell<usize>,
    closing: Cell<bool>,
    error: RefCell<Option<Error>>,
    sender: RefCell<Option<shared::Sender<capnp::Result<()>>>>,
}
impl Completion {
    fn finish(&self, result: capnp::Result<()>) {
        if let Err(error) = result {
            self.error.borrow_mut().get_or_insert(error);
        }
        self.remaining.set(self.remaining.get() - 1);
        self.notify();
    }
    fn notify(&self) {
        if self.closing.get() && self.remaining.get() == 0 {
            let sender = self.sender.borrow_mut().take();
            let result = self.error.borrow().clone().map_or(Ok(()), Err);
            // Completion may synchronously re-enter lookup or poll another
            // disconnector. All registry and completion borrows have ended.
            if let Some(sender) = sender {
                let _ = sender.send(result);
            }
        }
    }
}
struct Inner<S> {
    entries: RefCell<HashMap<usize, Entry<S>>>,
    completion: Rc<Completion>,
    closed: Shared<Promise<(), Error>>,
}

/// Clones share one registry. Protocol owners are returned/dropped only after
/// releasing the map borrow; no application or transport callback runs inside it.
pub(crate) struct Connections<S>(Rc<Inner<S>>);
impl<S> Clone for Connections<S> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
pub(crate) enum Acquired<S: Connection> {
    Existing(Rc<S>),
    New(Registration<S>),
}
impl<S: Connection> Connections<S> {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = shared::channel();
        Self(Rc::new(Inner {
            entries: RefCell::default(),
            completion: Rc::new(Completion {
                remaining: Cell::new(0),
                closing: Cell::new(false),
                error: RefCell::new(None),
                sender: RefCell::new(Some(sender)),
            }),
            closed: Promise::from_future(async move {
                receiver
                    .await
                    .map_err(|_| Error::disconnected("connection registry ended".into()))?
            })
            .shared(),
        }))
    }

    /// Reserve before constructing a driver: construction can invoke a custom
    /// transport. Reentrant construction cannot create a second protocol ledger.
    pub(crate) fn acquire(&self, id: usize) -> capnp::Result<Acquired<S>> {
        self.acquire_bounded(id, usize::MAX)
    }
    /// Count initializing and draining tasks, not only published live entries.
    /// Reusing a live connection remains possible at the configured limit.
    pub(crate) fn acquire_bounded(&self, id: usize, maximum: usize) -> capnp::Result<Acquired<S>> {
        if self.is_closing() {
            return Err(closing());
        }
        let mut entries = self.0.entries.borrow_mut();
        if let Some(entry) = entries.get(&id) {
            return entry
                .state
                .clone()
                .map(Acquired::Existing)
                .ok_or_else(|| Error::overloaded("RPC connection is being initialized".into()));
        }
        if self.0.completion.remaining.get() >= maximum {
            return Err(Error::overloaded(
                "RPC connection capacity exhausted".into(),
            ));
        }
        let next = self
            .0
            .completion
            .remaining
            .get()
            .checked_add(1)
            .ok_or_else(|| Error::overloaded("RPC connection capacity exhausted".into()))?;
        entries
            .try_reserve(1)
            .map_err(|_| Error::overloaded("RPC connection allocation failed".into()))?;
        let identity = Rc::new(());
        entries.insert(
            id,
            Entry {
                identity: identity.clone(),
                state: None,
            },
        );
        self.0.completion.remaining.set(next);
        Ok(Acquired::New(Registration {
            registry: Rc::downgrade(&self.0),
            completion: Some(self.0.completion.clone()),
            id,
            identity,
        }))
    }
    pub(crate) fn is_closing(&self) -> bool {
        self.0.completion.closing.get()
    }
    pub(crate) fn get(&self, id: usize) -> Option<Rc<S>> {
        self.0.entries.borrow().get(&id)?.state.clone()
    }
    pub(crate) fn states(&self) -> Vec<Rc<S>> {
        self.0
            .entries
            .borrow()
            .values()
            .filter_map(|entry| entry.state.clone())
            .collect()
    }
    /// An old shutdown never removes a replacement with the same transport ID.
    /// Detaching lookup does not complete the registration's IO lifetime.
    pub(crate) fn detach(&self, id: usize, state: &S) {
        let removed = {
            let mut entries = self.0.entries.borrow_mut();
            if entries
                .get(&id)
                .and_then(|entry| entry.state.as_ref())
                .is_some_and(|current| std::ptr::eq(&**current, state))
            {
                entries.remove(&id)
            } else {
                None
            }
        };
        drop(removed);
    }
    pub(crate) fn begin_close(&self) -> Vec<Rc<S>> {
        if self.0.completion.closing.replace(true) {
            return Vec::new();
        }
        let states = self.states();
        self.0.completion.notify();
        states
    }
    /// Preserve an explicit abort even while a constructor has reserved its
    /// identity but has not published a state that can receive the abort yet.
    pub(crate) fn begin_abort(&self, error: Error) -> Vec<Rc<S>> {
        self.0.completion.error.borrow_mut().get_or_insert(error);
        self.0.completion.closing.set(true);
        let states = self.states();
        self.0.completion.notify();
        states
    }
    pub(crate) fn closed(&self) -> Shared<Promise<(), Error>> {
        self.0.closed.clone()
    }
}

fn closing() -> Error {
    Error::disconnected("RPC system is closing".into())
}

/// Affine completion owner. Create outside the async body so cancellation before
/// its first poll also retires it. Dropping it reports cancellation, never a
/// successful transport close. It does not retain the registry or protocol state.
#[must_use = "retain registration until the transport has finished shutting down"]
pub(crate) struct Registration<S: Connection> {
    registry: Weak<Inner<S>>,
    completion: Option<Rc<Completion>>,
    id: usize,
    identity: Rc<()>,
}
impl<S: Connection> Registration<S> {
    pub(crate) fn publish(&self, state: Rc<S>) -> capnp::Result<()> {
        let registry = self.registry.upgrade().ok_or_else(closing)?;
        if registry.completion.closing.get() {
            return Err(closing());
        }
        let mut entries = registry.entries.borrow_mut();
        let entry = entries
            .get_mut(&self.id)
            .filter(|entry| Rc::ptr_eq(&entry.identity, &self.identity) && entry.state.is_none())
            .ok_or_else(|| Error::failed("RPC registration is no longer vacant".into()))?;
        entry.state = Some(state);
        Ok(())
    }
    /// Stop lookup while retaining ownership of pending transport completion.
    pub(crate) fn detach(&self) {
        drop(self.remove());
    }
    fn remove(&self) -> Option<Rc<S>> {
        let registry = self.registry.upgrade()?;
        let entry = {
            let mut entries = registry.entries.borrow_mut();
            if entries
                .get(&self.id)
                .is_some_and(|entry| Rc::ptr_eq(&entry.identity, &self.identity))
            {
                entries.remove(&self.id)
            } else {
                None
            }
        };
        entry.and_then(|entry| entry.state)
    }
    pub(crate) fn complete(mut self, mut result: capnp::Result<()>) {
        let state = self.remove();
        // A normally completed connection was detached by its protocol driver.
        // Ensure canceled/unexpected live owners cannot survive a finished task.
        if let Some(state) = state {
            let error =
                Error::disconnected("RPC connection task ended while still registered".into());
            state.disconnect(error.clone());
            if result.is_ok() {
                result = Err(error);
            }
        }
        self.completion.take().unwrap().finish(result);
    }
}
impl<S: Connection> Drop for Registration<S> {
    fn drop(&mut self) {
        let Some(completion) = self.completion.take() else {
            return;
        };
        let error =
            Error::disconnected("RPC connection task canceled before shutdown completed".into());
        if let Some(state) = self.remove() {
            state.disconnect(error.clone());
        }
        completion.finish(Err(error));
    }
}

#[cfg(test)]
mod tests;
