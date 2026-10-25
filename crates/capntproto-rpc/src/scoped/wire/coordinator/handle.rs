//! Owned commands contain no graph brands, question IDs or protocol tables.
use super::*;
use alloc::boxed::Box;

pub(super) struct Completion(Weak<RefCell<crate::queued::ClientInner>>);
impl Completion {
    pub(super) fn is_closed(&self) -> bool {
        self.0.strong_count() == 0
    }
    pub(super) fn send(self, result: capnp::Result<capability::Client>) {
        if let Some(client) = self.0.upgrade() {
            crate::queued::ClientInner::resolve(&client, result.map(|client| client.hook));
        }
    }
}
pub(crate) struct Commands {
    queue: RefCell<VecDeque<Completion>>,
    wake: RefCell<Option<Waker>>,
    closed: RefCell<Option<Error>>,
    maximum: NonZeroUsize,
}
impl Commands {
    pub(super) fn is_empty(&self) -> bool {
        self.queue.borrow().is_empty()
    }
    pub(crate) fn new(maximum: NonZeroUsize) -> Self {
        Self {
            queue: RefCell::default(),
            wake: RefCell::default(),
            closed: RefCell::default(),
            maximum,
        }
    }
    pub(super) fn register(&self, waker: Waker) {
        let previous = self.wake.replace(Some(waker));
        drop(previous);
    }
    pub(super) fn take(&self) -> Option<Completion> {
        self.queue.borrow_mut().pop_front()
    }
    pub(crate) fn close(&self, error: Error) {
        if self.closed.borrow().is_some() {
            return;
        }
        *self.closed.borrow_mut() = Some(error.clone());
        let queue = core::mem::take(&mut *self.queue.borrow_mut());
        let wake = self.wake.borrow_mut().take();
        drop(wake);
        // A completion may wake reentrant application code. All inbox borrows
        // end before invoking it, and new requests already see the close reason.
        for sender in queue {
            sender.send(Err(error.clone()));
        }
    }
}
impl Drop for Commands {
    fn drop(&mut self) {
        self.close(Error::disconnected("connection coordinator dropped".into()));
    }
}

/// Cloneable weak access to a running scoped connection. Does not drive IO or
/// extend its lifetime. Commands are bounded by the coordinator's concurrency
/// limit. A rejected command returns a broken typed client; calls can pipeline
/// immediately on an accepted command, even before its Bootstrap is written.
#[derive(Clone)]
pub struct Handle {
    commands: Weak<Commands>,
}
impl Handle {
    pub(crate) fn is_closed(&self) -> bool {
        self.commands
            .upgrade()
            .is_none_or(|commands| commands.closed.borrow().is_some())
    }
    pub(crate) fn close(&self, error: Error) {
        if let Some(commands) = self.commands.upgrade() {
            commands.close(error);
        }
    }
    pub(crate) fn new(commands: &Rc<Commands>) -> Self {
        Self {
            commands: Rc::downgrade(commands),
        }
    }
    pub fn bootstrap<T: capability::FromClientHook>(&self) -> T {
        let prepare = || {
            let commands = self
                .commands
                .upgrade()
                .ok_or_else(|| Error::disconnected("connection coordinator dropped".into()))?;
            if let Some(error) = commands.closed.borrow().clone() {
                return Err(error);
            }
            let mut queue = commands.queue.borrow_mut();
            if queue.len() == commands.maximum.get() {
                return Err(Error::overloaded(
                    "bootstrap command capacity exhausted".into(),
                ));
            }
            queue.try_reserve(1).map_err(wire_error)?;
            let client = crate::queued::Client::new(None);
            queue.push_back(Completion(Rc::downgrade(&client.inner)));
            drop(queue);
            // The stable coordinator notifier remains registered across
            // commands. End the borrow before invoking a reentrant task waker.
            let wake = commands.wake.borrow().clone();
            if let Some(wake) = wake {
                wake.wake();
            }
            Ok(client)
        };
        match prepare() {
            Err(error) => T::new(crate::broken::new_cap(error)),
            Ok(client) => T::new(Box::new(client)),
        }
    }
}
