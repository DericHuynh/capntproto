//! Typed ORM reconstruction; durable references carry whole-object rights.
use super::*;
use crate::{
    authority::Grant,
    orm::{ObjectServer, ObjectState},
};
use capnp::traits::{HasTypeId, Owned, SetterInput};
use std::marker::PhantomData;

pub struct ObjectFactory<T> {
    store: Rc<RefCell<Store>>,
    generation: ObjectGeneration,
    states: RefCell<BTreeMap<ObjectId, Weak<ObjectState>>>,
    marker: PhantomData<T>,
}
impl<T> ObjectFactory<T> {
    pub fn new(store: Rc<RefCell<Store>>, generation: ObjectGeneration) -> Self {
        Self {
            store,
            generation,
            states: RefCell::new(BTreeMap::new()),
            marker: PhantomData,
        }
    }
    /// Reuse one state per object so independently restored clients share the
    /// schema binding and publication notifications.
    pub fn state(&self, object: ObjectId) -> Rc<ObjectState> {
        let mut states = self.states.borrow_mut();
        if let Some(state) = states.get(&object).and_then(Weak::upgrade) {
            return state;
        }
        states.retain(|_, state| state.strong_count() != 0);
        let state = ObjectState::new(self.store.clone(), object);
        states.insert(object, Rc::downgrade(&state));
        state
    }
}
impl<T: Owned + Unpin + 'static> Factory for ObjectFactory<T>
where
    for<'a> T::Reader<'a>: SetterInput<T> + HasTypeId,
{
    fn restore(&self, descriptor: &Descriptor, peer: PeerKey) -> Promise<Client, Error> {
        if descriptor.generation() != self.generation {
            return Promise::err(unavailable());
        }
        let grant = Grant::root(
            descriptor.object(),
            descriptor.generation(),
            peer,
            descriptor.rights(),
        );
        match ObjectServer::<T>::client(self.state(descriptor.object()), grant) {
            Ok(client) => Promise::ok(client.client),
            Err(error) => Promise::err(error),
        }
    }
}
impl Realm {
    /// Bind the existing ORM facet. Saving requires its DELEGATE right and live
    /// lineage. Each successful save creates an independent durable grant;
    /// revoke that reference explicitly to prevent its future restoration.
    pub fn persistent_object<T: Owned + Unpin + 'static>(
        &self,
        state: Rc<ObjectState>,
        grant: Grant,
        kind: ObjectKind,
    ) -> capnp::Result<crate::store_capnp::object::Client<T>>
    where
        for<'a> T::Reader<'a>: SetterInput<T> + HasTypeId,
    {
        let descriptor = Descriptor::new(kind, grant.object(), grant.generation(), grant.rights());
        let client = ObjectServer::<T>::client(state, grant.clone())?;
        self.persistent(client, descriptor, move |_| {
            if grant.allows(Rights::DELEGATE) {
                Ok(())
            } else {
                Err(fail("persistent save requires live delegation authority"))
            }
        })
    }
}
