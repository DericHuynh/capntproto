//! Durable, owner-sealed references for a single application realm. This realm
//! stores descriptors, not serialized live capabilities. Application factories
//! rebuild the described authority after restart.
use crate::{
    authority::{ObjectGeneration, ObjectId, Rights},
    persistence_capnp as wire,
    storage::{ObjectKey, Store},
};
use capnp::{
    capability::{Client, FromClientHook, Promise},
    Error,
};
use ring::{
    digest::{digest, SHA256},
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::Path,
    rc::{Rc, Weak},
};

mod descriptor;
mod hooks;
mod orm;
pub use crate::object_ids::ObjectKind;
pub use descriptor::Descriptor;
pub use orm::ObjectFactory;
pub type OwnerId = [u8; 16];
pub type PeerKey = [u8; 32];
pub type Persistent =
    capnp_rpc::persistent_capnp::persistent::Client<wire::sturdy_ref::Owned, wire::owner::Owned>;
const LEDGER_OBJECT: ObjectKey = ObjectKey::new(0x52505245414c4d01);
const FORMAT: &str = "reproto-realm/2";

fn fail(message: impl std::fmt::Display) -> Error {
    Error::failed(message.to_string())
}
fn unavailable() -> Error {
    fail("sturdy reference unavailable")
}
fn random<const N: usize>() -> capnp::Result<[u8; N]> {
    let mut bytes = [0; N];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| fail("random source failed"))?;
    Ok(bytes)
}
fn hash(token: &[u8; 32]) -> [u8; 32] {
    digest(&SHA256, token).as_ref().try_into().unwrap()
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub owners: usize,
    pub references: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            owners: 64,
            references: 1024,
        }
    }
}
impl Limits {
    fn check(self) -> capnp::Result<()> {
        if !(1..=256).contains(&self.owners) || !(1..=4096).contains(&self.references) {
            return Err(fail("realm limits out of range"));
        }
        Ok(())
    }
}

/// A reusable reference, sealed to the owner recorded in the realm ledger.
/// Debug deliberately excludes its token. Wire encoding is explicit.
#[derive(Clone, PartialEq, Eq)]
pub struct SturdyRef {
    realm: [u8; 16],
    token: [u8; 32],
}
impl std::fmt::Debug for SturdyRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SturdyRef")
            .field("realm", &self.realm)
            .finish_non_exhaustive()
    }
}
impl SturdyRef {
    pub fn read(value: wire::sturdy_ref::Reader<'_>) -> capnp::Result<Self> {
        Ok(Self {
            realm: value.get_realm()?.try_into().map_err(|_| unavailable())?,
            token: value.get_token()?.try_into().map_err(|_| unavailable())?,
        })
    }
    pub fn write(&self, mut value: wire::sturdy_ref::Builder<'_>) {
        value.set_realm(&self.realm);
        value.set_token(&self.token);
    }
    pub fn realm(&self) -> [u8; 16] {
        self.realm
    }
}

/// Factories run outside ledger borrows and may perform asynchronous work.
/// Only the host registers factories. A canceled restore drops its future.
pub trait Factory {
    fn restore(
        &self,
        descriptor: &Descriptor,
        authenticated_peer: PeerKey,
    ) -> Promise<Client, Error>;
}
impl<F: Fn(&Descriptor, PeerKey) -> Promise<Client, Error>> Factory for F {
    fn restore(&self, descriptor: &Descriptor, peer: PeerKey) -> Promise<Client, Error> {
        self(descriptor, peer)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    id: OwnerId,
    key: PeerKey,
    epoch: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    hash: [u8; 32],
    owner: OwnerId,
    descriptor: Descriptor,
    revoked: bool,
    #[serde(deserialize_with = "required_expiry")]
    expires_at: Option<u64>,
    serial: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    format: String,
    realm: [u8; 16],
    owners: Vec<Owner>,
    references: Vec<Reference>,
    clock: u64,
    last_epoch: u64,
    last_token: u64,
}
fn required_expiry<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u64>, D::Error> {
    Option::<u64>::deserialize(deserializer)
}
impl Ledger {
    fn decode(bytes: &[u8]) -> capnp::Result<Self> {
        // No compatibility defaults: duplicates, unknown and missing fields fail.
        serde_json::from_slice(bytes).map_err(fail)
    }
    fn next_epoch(&mut self) -> capnp::Result<u64> {
        self.last_epoch = self
            .last_epoch
            .checked_add(1)
            .ok_or_else(|| fail("owner epoch exhausted"))?;
        Ok(self.last_epoch)
    }
    fn validate(&self, limits: Limits) -> capnp::Result<()> {
        if self.format != FORMAT
            || self.realm == [0; 16]
            || self.owners.len() > limits.owners
            || self.references.len() > limits.references
        {
            return Err(fail("invalid realm ledger format or quota"));
        }
        let mut owners = std::collections::BTreeSet::new();
        let mut keys = std::collections::BTreeSet::new();
        let mut hashes = std::collections::BTreeSet::new();
        for o in &self.owners {
            if o.id == [0; 16]
                || o.key == [0; 32]
                || o.epoch == 0
                || o.epoch > self.last_epoch
                || !owners.insert(o.id)
                || !keys.insert(o.key)
            {
                return Err(fail("invalid realm owner record"));
            }
        }
        let mut serials = std::collections::BTreeSet::new();
        for r in &self.references {
            if !owners.contains(&r.owner)
                || !hashes.insert(r.hash)
                || r.serial > self.last_token
                || (r.serial == 0 || !serials.insert(r.serial))
            {
                return Err(fail("invalid realm reference record"));
            }
        }
        Ok(())
    }
    fn authorized(&self, reference: &SturdyRef, peer: PeerKey) -> capnp::Result<(&Reference, u64)> {
        if reference.realm != self.realm {
            return Err(unavailable());
        }
        let key = hash(&reference.token);
        let r = self
            .references
            .iter()
            .find(|r| r.hash == key && !r.revoked && r.expires_at.is_none_or(|t| self.clock < t))
            .ok_or_else(unavailable)?;
        let owner = self
            .owners
            .iter()
            .find(|o| o.id == r.owner && o.key == peer)
            .ok_or_else(unavailable)?;
        Ok((r, owner.epoch))
    }
}

struct Core {
    store: Option<Store>,
    ledger: Ledger,
    limits: Limits,
    failed: bool,
}
impl Core {
    fn available(&self) -> capnp::Result<()> {
        if self.failed || self.store.is_none() {
            Err(fail("realm is closed or requires recovery"))
        } else {
            Ok(())
        }
    }
    fn commit(&mut self, next: Ledger) -> capnp::Result<()> {
        self.available()?;
        next.validate(self.limits)?;
        let bytes = serde_json::to_vec(&next).map_err(fail)?;
        let store = self.store.as_mut().unwrap();
        let head = store.head(LEDGER_OBJECT);
        match store.put(LEDGER_OBJECT, head, &bytes) {
            Ok(_) => {
                self.ledger = next;
                Ok(())
            }
            Err(error) => {
                self.failed = true;
                Err(fail(error))
            }
        }
    }
}
struct State {
    core: RefCell<Core>,
    factories: RefCell<BTreeMap<ObjectKind, Rc<dyn Factory>>>,
    executor: Option<Rc<dyn capnp::capability::CallExecutor>>,
}
/// Local administrative authority. RPC facets retain weak references, so
/// dropping the last Realm closes persistence even when clients survive.
#[derive(Clone)]
pub struct Realm(Rc<State>);
impl Realm {
    /// Open a realm for incoming RPC calls, which obtain their task owner from
    /// RpcSystem. For direct local Persistent.save calls use open_with_executor.
    pub fn open(path: impl AsRef<Path>, limits: Limits) -> capnp::Result<Self> {
        Self::open_inner(path.as_ref(), limits, None)
    }
    /// Standard Persistent.save does not permit cancellation after dispatch.
    /// Keep the supplied local call executor's driver running while serving it.
    pub fn open_with_executor(
        path: impl AsRef<Path>,
        limits: Limits,
        executor: Rc<dyn capnp::capability::CallExecutor>,
    ) -> capnp::Result<Self> {
        Self::open_inner(path.as_ref(), limits, Some(executor))
    }
    fn open_inner(
        path: &Path,
        limits: Limits,
        executor: Option<Rc<dyn capnp::capability::CallExecutor>>,
    ) -> capnp::Result<Self> {
        limits.check()?;
        let store = Store::open(path).map_err(fail)?;
        Self::from_store(store, limits, executor)
    }
    /// Open a dedicated ledger in a host-configured Store (including larger
    /// storage quotas). Pass an executor for protected local Persistent.save.
    pub fn from_store(
        store: Store,
        limits: Limits,
        executor: Option<Rc<dyn capnp::capability::CallExecutor>>,
    ) -> capnp::Result<Self> {
        limits.check()?;
        store.healthy().map_err(fail)?;
        if store.objects().any(|id| id != LEDGER_OBJECT)
            || store.published(LEDGER_OBJECT) != crate::storage::Revision::INITIAL
        {
            return Err(fail("file is not a realm ledger"));
        }
        let head = store.head(LEDGER_OBJECT);
        let ledger = if head == crate::storage::Revision::INITIAL {
            Ledger {
                format: FORMAT.into(),
                realm: random()?,
                owners: Vec::new(),
                references: Vec::new(),
                clock: 0,
                last_epoch: 0,
                last_token: 0,
            }
        } else {
            Ledger::decode(store.revision(LEDGER_OBJECT, head).map_err(fail)?.bytes())?
        };
        ledger.validate(limits)?;
        let mut core = Core {
            store: Some(store),
            ledger,
            limits,
            failed: false,
        };
        if head == crate::storage::Revision::INITIAL {
            core.commit(core.ledger.clone())?;
        }
        Ok(Self(Rc::new(State {
            core: RefCell::new(core),
            factories: RefCell::new(BTreeMap::new()),
            executor,
        })))
    }
    pub fn id(&self) -> [u8; 16] {
        self.0.core.borrow().ledger.realm
    }
    pub fn close(&self) {
        let store = self.0.core.borrow_mut().store.take();
        drop(store);
    }
    /// Reclaim obsolete ledger snapshots without changing authority, deadlines,
    /// counters or pending factories. A failed replacement requires recovery.
    pub fn compact(&self) -> capnp::Result<crate::storage::Compaction> {
        let mut core = self.0.core.borrow_mut();
        core.available()?;
        match core
            .store
            .as_mut()
            .unwrap()
            .compact(crate::storage::Retention::Latest)
        {
            Ok(result) => Ok(result),
            Err(error) => {
                core.failed = true;
                Err(fail(error))
            }
        }
    }
    /// Change storage quota policy before a ledger commit exhausts capacity.
    pub fn set_storage_limits(&self, limits: crate::storage::Limits) -> capnp::Result<()> {
        let mut core = self.0.core.borrow_mut();
        core.available()?;
        core.store
            .as_mut()
            .unwrap()
            .set_limits(limits)
            .map_err(fail)
    }
    pub fn register_owner(&self, id: OwnerId, key: PeerKey) -> capnp::Result<u64> {
        let mut core = self.0.core.borrow_mut();
        core.available()?;
        if core.ledger.owners.len() >= core.limits.owners
            || core.ledger.owners.iter().any(|o| o.id == id)
        {
            return Err(fail("owner exists or quota exceeded"));
        }
        let mut next = core.ledger.clone();
        let epoch = next.next_epoch()?;
        next.owners.push(Owner { id, key, epoch });
        core.commit(next)?;
        Ok(epoch)
    }
    pub fn rotate_owner(
        &self,
        id: OwnerId,
        expected_epoch: u64,
        key: PeerKey,
    ) -> capnp::Result<u64> {
        let mut core = self.0.core.borrow_mut();
        core.available()?;
        let mut next = core.ledger.clone();
        let epoch = next.next_epoch()?;
        let owner = next
            .owners
            .iter_mut()
            .find(|o| o.id == id && o.epoch == expected_epoch)
            .ok_or_else(|| fail("owner epoch conflict"))?;
        owner.epoch = epoch;
        owner.key = key;
        let epoch = owner.epoch;
        core.commit(next)?;
        Ok(epoch)
    }
    /// Delete this incarnation of an owner and every reference sealed to it in
    /// one durable commit. Returned live capabilities remain independent.
    pub fn delete_owner(&self, id: OwnerId, expected_epoch: u64) -> capnp::Result<usize> {
        let mut core = self.0.core.borrow_mut();
        core.available()?;
        let mut next = core.ledger.clone();
        let position = next
            .owners
            .iter()
            .position(|o| o.id == id && o.epoch == expected_epoch)
            .ok_or_else(|| fail("owner epoch conflict"))?;
        next.owners.remove(position);
        let before = next.references.len();
        next.references.retain(|r| r.owner != id);
        let removed = before - next.references.len();
        core.commit(next)?;
        Ok(removed)
    }
    /// Durable logical time. Hosts choose the unit and advance it explicitly;
    /// this is not a wall clock or a background expiration timer.
    pub fn clock(&self) -> u64 {
        self.0.core.borrow().ledger.clock
    }
    /// Advance durable time and reclaim revoked/expired reference slots.
    /// Equal time performs collection; backwards time is rejected. Historical
    /// file bytes remain until a separate storage compaction is implemented.
    pub fn advance_clock(&self, now: u64) -> capnp::Result<usize> {
        let mut core = self.0.core.borrow_mut();
        core.available()?;
        if now < core.ledger.clock {
            return Err(fail("realm clock cannot move backwards"));
        }
        let mut next = core.ledger.clone();
        next.clock = now;
        let before = next.references.len();
        next.references
            .retain(|r| !r.revoked && r.expires_at.is_none_or(|t| now < t));
        let removed = before - next.references.len();
        if now != core.ledger.clock || removed != 0 {
            core.commit(next)?;
        }
        Ok(removed)
    }
    /// Add or shorten one reference's durable deadline in realm-clock units.
    /// Expiration cannot be removed or extended. Already issued descendants
    /// remain independent; subsequent saves inherit the current deadline.
    pub fn expire_at(&self, reference: &SturdyRef, deadline: u64) -> capnp::Result<()> {
        let mut core = self.0.core.borrow_mut();
        core.available()?;
        if reference.realm != core.ledger.realm {
            return Err(unavailable());
        }
        let mut next = core.ledger.clone();
        let position = next
            .references
            .iter()
            .position(|r| r.hash == hash(&reference.token) && !r.revoked)
            .ok_or_else(unavailable)?;
        let r = &mut next.references[position];
        if r.expires_at.is_some_and(|t| deadline > t) {
            return Err(fail("reference expiration cannot be extended"));
        }
        if deadline <= next.clock {
            next.references.remove(position);
        } else {
            if r.expires_at == Some(deadline) {
                return Ok(());
            }
            r.expires_at = Some(deadline);
        }
        core.commit(next)
    }
    pub fn register_factory(
        &self,
        kind: ObjectKind,
        factory: Rc<dyn Factory>,
    ) -> capnp::Result<()> {
        self.0.core.borrow().available()?;
        let mut factories = self.0.factories.borrow_mut();
        if factories.len() >= 64 || factories.contains_key(&kind) {
            return Err(fail("factory exists or quota exceeded"));
        }
        factories.insert(kind, factory);
        Ok(())
    }
    /// Trusted host binding. The descriptor must describe exactly this client's
    /// authority. authorize_save must enforce its current revocation/delegation
    /// policy; it runs for every save outside all internal borrows.
    pub fn persistent<C: FromClientHook>(
        &self,
        client: C,
        descriptor: Descriptor,
        authorize_save: impl Fn(OwnerId) -> capnp::Result<()> + 'static,
    ) -> capnp::Result<C> {
        self.bind_persistent(
            client,
            descriptor,
            Rc::new(move |owner| authorize_save(owner).map(|()| None)),
        )
    }
    /// Bind a facet whose saved references all expire at the given absolute
    /// realm-clock deadline. The standard Persistent.save wire API is unchanged.
    pub fn persistent_until<C: FromClientHook>(
        &self,
        client: C,
        descriptor: Descriptor,
        deadline: u64,
        authorize_save: impl Fn(OwnerId) -> capnp::Result<()> + 'static,
    ) -> capnp::Result<C> {
        if deadline <= self.clock() {
            return Err(fail("reference deadline has passed"));
        }
        self.bind_persistent(
            client,
            descriptor,
            Rc::new(move |owner| authorize_save(owner).map(|()| Some(deadline))),
        )
    }
    fn bind_persistent<C: FromClientHook>(
        &self,
        client: C,
        descriptor: Descriptor,
        authorize_save: Rc<dyn Fn(OwnerId) -> capnp::Result<Option<u64>>>,
    ) -> capnp::Result<C> {
        self.0.core.borrow().available()?;
        if !self.0.factories.borrow().contains_key(&descriptor.kind()) {
            return Err(fail("restore factory not registered"));
        }
        Ok(hooks::bind(
            client,
            Rc::downgrade(&self.0),
            descriptor,
            authorize_save,
        ))
    }
    fn issue(
        &self,
        owner: OwnerId,
        descriptor: &Descriptor,
        expires_at: Option<u64>,
    ) -> capnp::Result<SturdyRef> {
        let mut core = self.0.core.borrow_mut();
        core.available()?;
        if !descriptor.rights().contains(Rights::DELEGATE) {
            return Err(fail("persistent save requires delegation authority"));
        }
        if expires_at.is_some_and(|t| t <= core.ledger.clock) {
            return Err(fail("reference deadline has passed"));
        }
        if !core.ledger.owners.iter().any(|o| o.id == owner)
            || core.ledger.references.len() >= core.limits.references
        {
            return Err(fail("unknown owner or reference quota exceeded"));
        }
        let serial = core
            .ledger
            .last_token
            .checked_add(1)
            .ok_or_else(|| fail("reference serial exhausted"))?;
        let token = (0..8)
            .map(|_| {
                let mut token: [u8; 32] = random()?;
                token[..8].copy_from_slice(&serial.to_be_bytes());
                Ok::<_, Error>(token)
            })
            .find_map(|token| match token {
                Ok(token)
                    if core
                        .ledger
                        .references
                        .iter()
                        .any(|r| r.hash == hash(&token)) =>
                {
                    None
                }
                result => Some(result),
            })
            .ok_or_else(|| fail("reference token collision"))??;
        let reference = SturdyRef {
            realm: core.ledger.realm,
            token,
        };
        let mut next = core.ledger.clone();
        next.last_token = serial;
        next.references.push(Reference {
            hash: hash(&token),
            owner,
            descriptor: descriptor.clone(),
            revoked: false,
            expires_at,
            serial,
        });
        core.commit(next)?;
        Ok(reference)
    }
    /// Revoke future restoration, including pending factories. Already returned
    /// live capabilities retain their independent authority. advance_clock can
    /// reclaim the tombstone without reusing its issuance serial.
    pub fn revoke(&self, reference: &SturdyRef) -> capnp::Result<()> {
        let mut core = self.0.core.borrow_mut();
        core.available()?;
        if reference.realm != core.ledger.realm {
            return Err(unavailable());
        }
        let mut next = core.ledger.clone();
        let key = hash(&reference.token);
        let r = next
            .references
            .iter_mut()
            .find(|r| r.hash == key)
            .ok_or_else(unavailable)?;
        if r.revoked {
            return Ok(());
        }
        r.revoked = true;
        core.commit(next)
    }
    /// The peer argument is trusted transport context, never an RPC parameter.
    /// Prefer bootstrap_factory() on an authenticated Noise network.
    pub fn restore(
        &self,
        reference: SturdyRef,
        authenticated_peer: PeerKey,
    ) -> Promise<Client, Error> {
        let selected = (|| {
            let core = self.0.core.borrow();
            core.available()?;
            let (r, epoch) = core.ledger.authorized(&reference, authenticated_peer)?;
            let factory = self
                .0
                .factories
                .borrow()
                .get(&r.descriptor.kind())
                .cloned()
                .ok_or_else(unavailable)?;
            Ok((r.descriptor.clone(), r.owner, epoch, factory))
        })();
        let (descriptor, owner, epoch, factory) = match selected {
            Ok(x) => x,
            Err(e) => return Promise::err(e),
        };
        let state = Rc::downgrade(&self.0);
        // No core/factory borrow crosses application code or an await.
        let pending = factory.restore(&descriptor, authenticated_peer);
        Promise::from_future(async move {
            let client = pending.await?;
            let state = state.upgrade().ok_or_else(unavailable)?;
            {
                let core = state.core.borrow();
                core.available()?;
                let (current, current_epoch) =
                    core.ledger.authorized(&reference, authenticated_peer)?;
                if current_epoch != epoch
                    || current.owner != owner
                    || current.descriptor != descriptor
                {
                    return Err(unavailable());
                }
            }
            // Re-saving a restored facet is permitted only while the source
            // reference and this owner's authentication epoch remain valid.
            let guard_state = Rc::downgrade(&state);
            Ok(hooks::bind(
                client,
                Rc::downgrade(&state),
                descriptor,
                Rc::new(move |seal_for| {
                    if seal_for != owner {
                        return Err(unavailable());
                    }
                    let state = guard_state.upgrade().ok_or_else(unavailable)?;
                    let core = state.core.borrow();
                    core.available()?;
                    let (current, current_epoch) =
                        core.ledger.authorized(&reference, authenticated_peer)?;
                    if current_epoch == epoch {
                        Ok(current.expires_at)
                    } else {
                        Err(unavailable())
                    }
                }),
            ))
        })
    }
    pub fn bootstrap_factory(&self) -> Rc<dyn capnp_rpc::BootstrapFactory<PeerKey>> {
        Rc::new(RestoreBootstrap(Rc::downgrade(&self.0)))
    }
}

struct RestoreBootstrap(Weak<State>);
impl capnp_rpc::BootstrapFactory<PeerKey> for RestoreBootstrap {
    fn create_for(&self, peer: &PeerKey) -> capnp::Result<Client> {
        let state = self.0.upgrade().ok_or_else(unavailable)?;
        state.core.borrow().available()?;
        let cap: wire::restorer::Client = capnp_rpc::new_client(RestoreServer {
            realm: self.0.clone(),
            peer: *peer,
        });
        Ok(cap.client)
    }
}
struct RestoreServer {
    realm: Weak<State>,
    peer: PeerKey,
}
impl wire::restorer::Server for RestoreServer {
    async fn restore(
        self: Rc<Self>,
        params: wire::restorer::RestoreParams,
        mut results: wire::restorer::RestoreResults,
    ) -> capnp::Result<()> {
        let reference = SturdyRef::read(params.get()?.get_reference()?)?;
        let realm = Realm(self.realm.upgrade().ok_or_else(unavailable)?);
        let pending = realm.restore(reference, self.peer);
        drop(realm);
        drop(params);
        results
            .get()
            .get_cap()
            .set_as_capability(pending.await?.hook);
        Ok(())
    }
}
