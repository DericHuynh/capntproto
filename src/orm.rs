//! Typed whole-entry objects, coalescing notifications and resumable publication history.
mod history;
use crate::{
    authority::{Grant, ObjectId, Rights},
    storage::{ObjectKey, Revision, Store},
    store_capnp::{object, subscription},
};
use capnp::{
    message::ReaderOptions,
    traits::{HasTypeId, Owned, SetterInput},
};
use std::{
    cell::{Cell, RefCell},
    marker::PhantomData,
    rc::Rc,
};
fn failure(e: impl std::fmt::Display) -> capnp::Error {
    capnp::Error::failed(e.to_string())
}
/// Shared schema and subscription state for one immutable object/store binding.
/// Store access is trusted local administration; the ID is not a store brand.
pub struct ObjectState {
    store: Rc<RefCell<Store>>,
    object: ObjectId,
    changed: tokio::sync::watch::Sender<Revision>,
    subscribers: Cell<usize>,
    schema: Cell<Option<u64>>,
}
impl ObjectState {
    #[cfg(feature = "services")]
    pub(crate) fn notify_publication(&self, revision: Revision) {
        self.changed.send_replace(revision);
    }
    pub fn new(store: Rc<RefCell<Store>>, object: ObjectId) -> Rc<Self> {
        let revision = store.borrow().published(ObjectKey::from(object));
        let (changed, _) = tokio::sync::watch::channel(revision);
        Rc::new(Self {
            store,
            object,
            changed,
            subscribers: Cell::new(0),
            schema: Cell::new(None),
        })
    }
    #[must_use]
    pub fn object(&self) -> ObjectId {
        self.object
    }
    /// Access the bound store for trusted host operations, without rebinding it.
    #[must_use]
    pub fn store(&self) -> &Rc<RefCell<Store>> {
        &self.store
    }
}
pub struct ObjectServer<T> {
    state: Rc<ObjectState>,
    grant: Grant,
    _type: PhantomData<T>,
}
impl<T: Owned + Unpin + 'static> ObjectServer<T>
where
    for<'a> T::Reader<'a>: SetterInput<T> + HasTypeId,
{
    pub fn client(state: Rc<ObjectState>, grant: Grant) -> capnp::Result<object::Client<T>> {
        if state.object != grant.object() {
            return Err(failure("grant/object mismatch"));
        }
        let id = <T::Reader<'static> as HasTypeId>::TYPE_ID;
        if state.schema.get().is_some_and(|old| old != id) {
            return Err(failure("object schema mismatch"));
        }
        {
            let store = state.store.borrow();
            let head = store.head(ObjectKey::from(state.object));
            if head > Revision::INITIAL {
                let snapshot = store
                    .revision(ObjectKey::from(state.object), head)
                    .map_err(failure)?;
                typed_bytes::<T>(&snapshot)?;
            }
        }
        state.schema.set(Some(id));
        Ok(capnp_rpc::new_client(Self {
            state,
            grant,
            _type: PhantomData,
        }))
    }
    fn require(&self, r: Rights) -> capnp::Result<()> {
        if self.grant.allows(r) {
            Ok(())
        } else {
            Err(failure("capability revoked or operation not permitted"))
        }
    }
}
impl<T: Owned + Unpin + 'static> object::Server<T> for ObjectServer<T>
where
    for<'a> T::Reader<'a>: SetterInput<T> + HasTypeId,
{
    async fn history(
        self: Rc<Self>,
        _: object::HistoryParams<T>,
        mut results: object::HistoryResults<T>,
    ) -> capnp::Result<()> {
        self.require(Rights::SUBSCRIBE)?;
        let (floor, published) = self
            .state
            .store
            .borrow()
            .history_bounds(ObjectKey::from(self.state.object))
            .map_err(failure)?;
        let client = history::client::<T>(self.state.clone(), self.grant.clone())?;
        let mut result = results.get();
        result.set_history(client);
        result.set_floor(floor.get());
        result.set_published(published.get());
        Ok(())
    }

    async fn get(
        self: Rc<Self>,
        _: object::GetParams<T>,
        mut results: object::GetResults<T>,
    ) -> capnp::Result<()> {
        self.require(Rights::GET)?;
        let snapshot = self
            .state
            .store
            .borrow()
            .get(ObjectKey::from(self.state.object))
            .map_err(failure)?;
        let mut bytes = typed_bytes::<T>(&snapshot)?;
        let message =
            capnp::serialize::read_message_from_flat_slice(&mut bytes, ReaderOptions::new())?;
        results
            .get()
            .set_value(message.get_root::<T::Reader<'_>>()?)?;
        results.get().set_revision(snapshot.revision().get());
        Ok(())
    }
    async fn put(
        self: Rc<Self>,
        params: object::PutParams<T>,
        mut results: object::PutResults<T>,
    ) -> capnp::Result<()> {
        self.require(Rights::PUT)?;
        let p = params.get()?;
        let mut message = capnp::message::Builder::new_default();
        message.set_root::<T>(p.get_value()?)?;
        if message
            .get_root_as_reader::<capnp::any_pointer::Reader<'_>>()?
            .target_size()?
            .cap_count
            != 0
        {
            return Err(failure(
                "persistent entries cannot contain live connection capabilities",
            ));
        }
        let mut bytes = <T::Reader<'static> as HasTypeId>::TYPE_ID
            .to_le_bytes()
            .to_vec();
        bytes.extend_from_slice(&capnp::serialize::write_message_to_words(&message));
        {
            let store = self.state.store.borrow();
            let head = store.head(ObjectKey::from(self.state.object));
            if head > Revision::INITIAL {
                let snapshot = store
                    .revision(ObjectKey::from(self.state.object), head)
                    .map_err(failure)?;
                typed_bytes::<T>(&snapshot)?;
            }
        }
        let rev = self
            .state
            .store
            .borrow_mut()
            .put(
                ObjectKey::from(self.state.object),
                Revision::new(p.get_expected_head()),
                &bytes,
            )
            .map_err(failure)?;
        results.get().set_revision(rev.get());
        Ok(())
    }
    async fn publish(
        self: Rc<Self>,
        params: object::PublishParams<T>,
        mut results: object::PublishResults<T>,
    ) -> capnp::Result<()> {
        self.require(Rights::PUBLISH)?;
        let p = params.get()?;
        let rev = self
            .state
            .store
            .borrow_mut()
            .publish(
                ObjectKey::from(self.state.object),
                Revision::new(p.get_revision()),
                Revision::new(p.get_expected_published()),
            )
            .map_err(failure)?;
        self.state.changed.send_replace(rev);
        results.get().set_revision(rev.get());
        Ok(())
    }
    async fn subscribe(
        self: Rc<Self>,
        params: object::SubscribeParams<T>,
        mut results: object::SubscribeResults<T>,
    ) -> capnp::Result<()> {
        self.require(Rights::SUBSCRIBE)?;
        let p = params.get()?;
        let observer = p.get_observer()?;
        let mut after = Revision::new(p.get_after());
        if after
            > self
                .state
                .store
                .borrow()
                .published(ObjectKey::from(self.state.object))
        {
            return Err(failure("subscription cursor ahead of published revision"));
        }
        if self.state.subscribers.get() >= 64 {
            return Err(failure("subscription capacity exhausted"));
        }
        self.state.subscribers.set(self.state.subscribers.get() + 1);
        let mut updates = self.state.changed.subscribe();
        let state = self.state.clone();
        let grant = self.grant.clone();
        let task = tokio::task::spawn_local(async move {
            loop {
                if !grant.allows(Rights::SUBSCRIBE) {
                    return;
                }
                let revision = *updates.borrow_and_update();
                if revision > after {
                    let snapshot = match state
                        .store
                        .borrow()
                        .revision(ObjectKey::from(state.object), revision)
                    {
                        Ok(v) => v,
                        Err(_) => return,
                    };
                    let result = (|| -> capnp::Result<_> {
                        let mut bytes = typed_bytes::<T>(&snapshot)?;
                        let message = capnp::serialize::read_message_from_flat_slice(
                            &mut bytes,
                            ReaderOptions::new(),
                        )?;
                        let mut call = observer.changed_request();
                        call.get().set_revision(revision.get());
                        call.get().set_value(message.get_root::<T::Reader<'_>>()?)?;
                        Ok(call.send().promise)
                    })();
                    match result {
                        Ok(p) => {
                            if p.await.is_err() {
                                return;
                            }
                        }
                        Err(_) => return,
                    };
                    after = revision;
                }
                if updates.changed().await.is_err() {
                    return;
                }
            }
        });
        results
            .get()
            .set_subscription(capnp_rpc::new_client(Subscription {
                task: RefCell::new(Some(task)),
                state: self.state.clone(),
            }));
        Ok(())
    }
}
struct Subscription {
    task: RefCell<Option<tokio::task::JoinHandle<()>>>,
    state: Rc<ObjectState>,
}
impl Subscription {
    fn stop(&self) {
        if let Some(t) = self.task.borrow_mut().take() {
            t.abort();
            self.state.subscribers.set(self.state.subscribers.get() - 1);
        }
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        self.stop();
    }
}
impl subscription::Server for Subscription {
    async fn cancel(
        self: Rc<Self>,
        _: subscription::CancelParams,
        _: subscription::CancelResults,
    ) -> capnp::Result<()> {
        self.stop();
        Ok(())
    }
}

fn typed_bytes<T: Owned>(snapshot: &crate::storage::Snapshot) -> capnp::Result<&[u8]>
where
    for<'a> T::Reader<'a>: HasTypeId,
{
    let bytes = snapshot.bytes();
    if bytes.len() < 8
        || u64::from_le_bytes(bytes[..8].try_into().unwrap())
            != <T::Reader<'static> as HasTypeId>::TYPE_ID
    {
        return Err(failure("stored schema type mismatch"));
    }
    Ok(&bytes[8..])
}
