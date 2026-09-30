//! A cursor belongs to the consumer, not to a connection or a server task.
use super::{failure, typed_bytes, ObjectState};
use crate::storage::ObjectKey;
use crate::{
    authority::{Grant, Rights},
    storage::{Error as StorageError, Snapshot},
    store_capnp::history,
};
use capnp::{
    message::ReaderOptions,
    traits::{HasTypeId, Owned, SetterInput},
};
use std::{cell::Cell, marker::PhantomData, rc::Rc};

pub(super) fn client<T: Owned + Unpin + 'static>(
    state: Rc<ObjectState>,
    grant: Grant,
) -> capnp::Result<history::Client<T>>
where
    for<'a> T::Reader<'a>: SetterInput<T> + HasTypeId,
{
    if state.subscribers.get() >= 64 {
        return Err(failure("subscription capacity exhausted"));
    }
    state.subscribers.set(state.subscribers.get() + 1);
    Ok(capnp_rpc::new_client(History::<T> {
        state,
        grant,
        closed: tokio::sync::watch::channel(false).0,
        reserved: Cell::new(true),
        busy: Cell::new(false),
        marker: PhantomData,
    }))
}

struct History<T> {
    state: Rc<ObjectState>,
    grant: Grant,
    closed: tokio::sync::watch::Sender<bool>,
    reserved: Cell<bool>,
    busy: Cell<bool>,
    marker: PhantomData<T>,
}
impl<T> History<T> {
    fn close(&self) {
        self.closed.send_replace(true);
        if self.reserved.replace(false) {
            self.state.subscribers.set(self.state.subscribers.get() - 1);
        }
    }
    fn check(&self) -> capnp::Result<()> {
        if *self.closed.borrow() {
            return Err(failure("history stream canceled"));
        }
        if !self.grant.allows(Rights::SUBSCRIBE) {
            self.close();
            return Err(failure("capability revoked or operation not permitted"));
        }
        Ok(())
    }
}
impl<T> Drop for History<T> {
    fn drop(&mut self) {
        self.close();
    }
}
struct Pending<'a>(&'a Cell<bool>);
impl Drop for Pending<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl<T: Owned + Unpin + 'static> history::Server<T> for History<T>
where
    for<'a> T::Reader<'a>: SetterInput<T> + HasTypeId,
{
    async fn next(
        self: Rc<Self>,
        params: history::NextParams<T>,
        mut results: history::NextResults<T>,
    ) -> capnp::Result<()> {
        self.check()?;
        let after = params.get()?.get_after();
        if self.busy.replace(true) {
            return Err(failure("history stream already has a pending next call"));
        }
        let _pending = Pending(&self.busy);
        // Subscribe before reading to avoid losing a publication between the
        // lookup and the wait. Store notifications also cover other ObjectStates.
        let mut changes = self.state.store.borrow().watch_publications();
        let mut closed = self.closed.subscribe();
        let cursor = match self
            .state
            .store
            .borrow()
            .publication_cursor(ObjectKey::from(self.state.object), after)
        {
            Ok(cursor) => cursor,
            Err(StorageError::HistoryExpired { floor }) => {
                let mut result = results.get().init_result();
                result.set_floor(floor.get());
                result.set_gap(());
                return self.check();
            }
            Err(error) => return Err(failure(error)),
        };
        loop {
            self.check()?;
            changes.borrow_and_update();
            let next = self.state.store.borrow().publication_after(&cursor);
            match next {
                Ok(Some(publication)) => {
                    write_event::<T>(publication.snapshot(), &mut results)?;
                    results.get().get_result()?.set_floor(
                        (self
                            .state
                            .store
                            .borrow()
                            .history_bounds(ObjectKey::from(self.state.object))
                            .map_err(failure)?
                            .0)
                            .get(),
                    );
                    self.check()?;
                    return Ok(());
                }
                Err(StorageError::HistoryExpired { floor }) => {
                    let mut result = results.get().init_result();
                    result.set_floor(floor.get());
                    result.set_gap(());
                    return self.check();
                }
                Err(error) => return Err(failure(error)),
                Ok(None) => (),
            }
            tokio::select! {
                _ = self.grant.when_revoked() => { self.close(); return Err(failure("capability revoked")); }
                _ = closed.changed() => return Err(failure("history stream canceled")),
                result = changes.changed() => { result.map_err(failure)?; }
            }
        }
    }
    async fn cancel(
        self: Rc<Self>,
        _: history::CancelParams<T>,
        _: history::CancelResults<T>,
    ) -> capnp::Result<()> {
        self.close();
        Ok(())
    }
}
fn write_event<T: Owned>(
    snapshot: &Snapshot,
    results: &mut history::NextResults<T>,
) -> capnp::Result<()>
where
    for<'a> T::Reader<'a>: SetterInput<T> + HasTypeId,
{
    let mut bytes = typed_bytes::<T>(snapshot)?;
    let message = capnp::serialize::read_message_from_flat_slice(&mut bytes, ReaderOptions::new())?;
    let mut event = results.get().init_result().init_event();
    event.set_revision(snapshot.revision().get());
    event.set_value(message.get_root::<T::Reader<'_>>()?)?;
    Ok(())
}
