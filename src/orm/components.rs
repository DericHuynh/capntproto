//! Independent typed Cap'n Proto messages sharing one atomic object revision.
//! Split frequently changed fields from large immutable data at schema boundaries.
//! Cross-component links must be application IDs, not Cap'n Proto pointers.
use super::{
    failure, Grant, HasTypeId, ObjectId, Owned, PhantomData, Rc, ReaderOptions, RefCell, Rights,
    SetterInput,
};
use crate::{
    storage::{ComponentId, ComponentSnapshot, ComponentUpdate, ObjectKey, Revision, Store},
    store_capnp::component,
};
use std::collections::BTreeMap;

/// An owned, capability-free typed replacement for one component.
/// Several replacements can be applied atomically with [`ComponentState::edit`].
pub struct Edit {
    id: ComponentId,
    bytes: Vec<u8>,
}
impl Edit {
    pub fn set<T: Owned>(id: ComponentId, value: T::Reader<'_>) -> capnp::Result<Self>
    where
        for<'a> T::Reader<'a>: SetterInput<T> + HasTypeId,
    {
        let bytes = super::encode_value::<T>(value)?;
        Ok(Self { id, bytes })
    }
}

fn schema(bytes: &[u8]) -> capnp::Result<u64> {
    Ok(u64::from_le_bytes(
        bytes
            .get(..8)
            .ok_or_else(|| failure("missing component schema"))?
            .try_into()
            .unwrap(),
    ))
}

/// A mapped component and its containing root revision. Cloning shares storage.
pub struct TypedComponent<T> {
    snapshot: ComponentSnapshot,
    id: ComponentId,
    marker: PhantomData<T>,
}
impl<T> Clone for TypedComponent<T> {
    fn clone(&self) -> Self {
        Self {
            snapshot: self.snapshot.clone(),
            id: self.id,
            marker: PhantomData,
        }
    }
}
impl<T: Owned> TypedComponent<T>
where
    for<'a> T::Reader<'a>: HasTypeId,
{
    pub fn new(snapshot: ComponentSnapshot, id: ComponentId) -> capnp::Result<Self> {
        let bytes = snapshot
            .get(id)
            .ok_or_else(|| failure("component not found"))?;
        if schema(bytes)? != <T::Reader<'static> as HasTypeId>::TYPE_ID {
            return Err(failure("component schema mismatch"));
        }
        Ok(Self {
            snapshot,
            id,
            marker: PhantomData,
        })
    }
    pub fn revision(&self) -> Revision {
        self.snapshot.revision()
    }
    /// Read directly from the mapping within the closure. Reader traversal and
    /// nesting limits still apply; no full object or component copy is needed.
    pub fn with_reader<R>(
        &self,
        read: impl for<'a> FnOnce(T::Reader<'a>) -> capnp::Result<R>,
    ) -> capnp::Result<R> {
        let mut bytes = &self.snapshot.get(self.id).unwrap()[8..];
        let message =
            capnp::serialize::read_message_from_flat_slice(&mut bytes, ReaderOptions::new())?;
        if !bytes.is_empty() {
            return Err(failure("trailing component bytes"));
        }
        let reader = message.get_root::<T::Reader<'_>>()?;
        read(reader)
    }
}

/// Trusted local host binding for a component object. Use one shared state per
/// object for facet schema reservations. Persisted types are rechecked on writes.
/// Raw Store access is privileged: it can remove components or bypass ORM types.
pub struct ComponentState {
    store: Rc<RefCell<Store>>,
    object: ObjectId,
    schemas: RefCell<BTreeMap<ComponentId, u64>>,
}
impl ComponentState {
    pub fn new(store: Rc<RefCell<Store>>, object: ObjectId) -> capnp::Result<Rc<Self>> {
        {
            let store = store.borrow();
            if store.format_version() != 5 {
                return Err(failure("component ORM requires a version-5 store"));
            }
            let head = store.head(ObjectKey::from(object));
            if head != Revision::INITIAL {
                store
                    .component_revision(ObjectKey::from(object), head)
                    .map_err(failure)?;
            }
        }
        Ok(Rc::new(Self {
            store,
            object,
            schemas: RefCell::new(BTreeMap::new()),
        }))
    }
    pub fn object(&self) -> ObjectId {
        self.object
    }
    /// Stage replacements, optionally publishing the whole new root atomically.
    /// Both CAS arguments refer to root revisions, even when edits are disjoint.
    pub fn edit(
        &self,
        expected_head: Revision,
        expected_published: Option<Revision>,
        edits: &[Edit],
    ) -> capnp::Result<Revision> {
        let mut store = self.store.borrow_mut();
        let object = ObjectKey::from(self.object);
        let head = store.head(object);
        let previous = if head == Revision::INITIAL {
            None
        } else {
            Some(store.component_revision(object, head).map_err(failure)?)
        };
        let mut schemas = self.schemas.borrow().clone();
        for edit in edits {
            let ty = schema(&edit.bytes)?;
            if schemas.get(&edit.id).is_some_and(|old| *old != ty) {
                return Err(failure("component schema mismatch"));
            }
            if let Some(old) = previous.as_ref().and_then(|s| s.get(edit.id)) {
                if schema(old)? != ty {
                    return Err(failure("component schema mismatch"));
                }
            }
            schemas.insert(edit.id, ty);
        }
        let updates: Vec<_> = edits
            .iter()
            .map(|e| ComponentUpdate {
                id: e.id,
                value: Some(e.bytes.as_slice()),
            })
            .collect();
        let revision = store
            .edit_components(object, expected_head, expected_published, &updates)
            .map_err(failure)?;
        *self.schemas.borrow_mut() = schemas;
        Ok(revision)
    }
    pub fn get<T: Owned>(&self, id: ComponentId) -> capnp::Result<TypedComponent<T>>
    where
        for<'a> T::Reader<'a>: HasTypeId,
    {
        TypedComponent::new(
            self.store
                .borrow()
                .get_components(ObjectKey::from(self.object))
                .map_err(failure)?,
            id,
        )
    }
}

/// Capability RPC facet restricted to one component ID and schema.
pub struct ComponentServer<T> {
    state: Rc<ComponentState>,
    grant: Grant,
    id: ComponentId,
    marker: PhantomData<T>,
}
impl<T: Owned + Unpin + 'static> ComponentServer<T>
where
    for<'a> T::Reader<'a>: SetterInput<T> + HasTypeId,
{
    pub fn client(
        state: Rc<ComponentState>,
        grant: Grant,
        id: ComponentId,
    ) -> capnp::Result<component::Client<T>> {
        if state.object != grant.object() {
            return Err(failure("grant/object mismatch"));
        }
        let ty = <T::Reader<'static> as HasTypeId>::TYPE_ID;
        if state
            .schemas
            .borrow()
            .get(&id)
            .is_some_and(|old| *old != ty)
        {
            return Err(failure("component schema mismatch"));
        }
        {
            let store = state.store.borrow();
            let object = ObjectKey::from(state.object);
            let head = store.head(object);
            if head != Revision::INITIAL {
                let snapshot = store.component_revision(object, head).map_err(failure)?;
                if let Some(bytes) = snapshot.get(id) {
                    if schema(bytes)? != ty {
                        return Err(failure("component schema mismatch"));
                    }
                }
            }
        }
        state.schemas.borrow_mut().insert(id, ty);
        Ok(capnp_rpc::new_client(Self {
            state,
            grant,
            id,
            marker: PhantomData,
        }))
    }
    fn require(&self, rights: Rights) -> capnp::Result<()> {
        if self.grant.allows(rights) {
            Ok(())
        } else {
            Err(failure("capability revoked or operation not permitted"))
        }
    }
}
impl<T: Owned + Unpin + 'static> component::Server<T> for ComponentServer<T>
where
    for<'a> T::Reader<'a>: SetterInput<T> + HasTypeId,
{
    async fn get(
        self: Rc<Self>,
        _: component::GetParams<T>,
        mut results: component::GetResults<T>,
    ) -> capnp::Result<()> {
        self.require(Rights::GET)?;
        let value = self.state.get::<T>(self.id)?;
        value.with_reader(|reader| results.get().set_value(reader))?;
        results.get().set_revision(value.revision().get());
        Ok(())
    }
    async fn put(
        self: Rc<Self>,
        params: component::PutParams<T>,
        mut results: component::PutResults<T>,
    ) -> capnp::Result<()> {
        self.require(Rights::PUT)?;
        let p = params.get()?;
        let edit = Edit::set::<T>(self.id, p.get_value()?)?;
        let revision = self
            .state
            .edit(Revision::new(p.get_expected_head()), None, &[edit])?;
        results.get().set_revision(revision.get());
        Ok(())
    }
    async fn commit(
        self: Rc<Self>,
        params: component::CommitParams<T>,
        mut results: component::CommitResults<T>,
    ) -> capnp::Result<()> {
        self.require(Rights::PUT)?;
        self.require(Rights::PUBLISH)?;
        let p = params.get()?;
        let edit = Edit::set::<T>(self.id, p.get_value()?)?;
        let revision = self.state.edit(
            Revision::new(p.get_expected_head()),
            Some(Revision::new(p.get_expected_published())),
            &[edit],
        )?;
        results.get().set_revision(revision.get());
        Ok(())
    }
}
