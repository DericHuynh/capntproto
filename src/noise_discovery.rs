//! Recipient-scoped, expiring capability discovery and explicit name/key rotation.
//! The directory's owner authorizes publications; lookups never grant publication
//! rights. Native vat IDs remain immutable Noise keys, including across rotation.
use crate::{
    noise_discovery_capnp::{directory, record},
    noise_provisioning::{relay, ProvisioningConnector},
    noise_provisioning_capnp::provisioner,
    noise_rpc::Connector,
    transport::{AuthenticatedSession, Identity},
};
use capnp::{capability::Promise, Error};
use std::{cell::RefCell, collections::BTreeMap, net::SocketAddr, rc::Rc, time::Duration};
use tokio::time::Instant;

mod generation;
pub use generation::DiscoveryGeneration;
mod resolved;
pub use resolved::Resolved;
mod publication;
pub use publication::{Publication, PublicationStatus};
mod readers;
pub use readers::{Discovery, DiscoveryOptions};
mod advertisement;
pub use advertisement::{
    Advertisement, AdvertisementOptions, AdvertisementStatus, AdvertisementStop, MappedService,
};

fn failed(message: &str) -> Error {
    Error::failed(message.into())
}
const MAX_ENTRIES: usize = 64;
const MAX_LIFETIME: Duration = Duration::from_secs(60);

/// An authorized binding. The provider must be scoped to this recipient and
/// kept on an independent control route (use noise_provisioning::relay).
#[derive(Clone)]
pub struct Binding {
    pub host: [u8; 32],
    pub recipient: [u8; 32],
    pub address: SocketAddr,
    pub context: Vec<u8>,
    pub provider: provisioner::Client,
}
struct Entry {
    binding: Rc<Binding>,
    generation: DiscoveryGeneration,
    expires: Instant,
    visible: bool,
}
// Commit ownership bookkeeping before releasing old hooks or waking observers.
struct Change {
    old: Option<Rc<Entry>>,
    generation: DiscoveryGeneration,
    changed: Rc<tokio::sync::Notify>,
}
impl Drop for Change {
    fn drop(&mut self) {
        drop(self.old.take());
        self.changed.notify_waiters();
    }
}
#[derive(Default)]
struct State {
    entries: BTreeMap<(String, [u8; 32]), Rc<Entry>>,
    next: u64,
    closed: bool,
    changed: Rc<tokio::sync::Notify>,
}

/// Administrative publication handle. Export client(recipient), not this owner.
#[derive(Clone, Default)]
pub struct Directory(Rc<RefCell<State>>);
impl Directory {
    /// Compare-and-replace a named binding. expected=None creates a new entry;
    /// renewal/rotation requires its current generation, including after expiry.
    /// A rotation changes future name resolutions, never an existing vat's key.
    pub fn publish(
        &self,
        name: &str,
        binding: Binding,
        expected: Option<DiscoveryGeneration>,
        lifetime: Duration,
    ) -> capnp::Result<DiscoveryGeneration> {
        Ok(self.prepare(name, binding, expected, lifetime)?.generation)
    }
    fn prepare(
        &self,
        name: &str,
        binding: Binding,
        expected: Option<DiscoveryGeneration>,
        lifetime: Duration,
    ) -> capnp::Result<Change> {
        if !name_ok(name)
            || binding.host == [0; 32]
            || binding.recipient == [0; 32]
            || binding.host == binding.recipient
            || !crate::nat::address_ok(binding.address)
            || binding.context.len() > 1024
            || lifetime.is_zero()
            || lifetime > MAX_LIFETIME
        {
            return Err(failed("invalid Noise discovery publication"));
        }
        let key = (name.to_owned(), binding.recipient);
        let (old, generation, changed) = {
            let mut state = self.0.borrow_mut();
            if state.closed {
                return Err(Error::disconnected("Noise directory closed".into()));
            }
            if state.entries.get(&key).map(|entry| entry.generation) != expected {
                return Err(failed("stale Noise publication generation"));
            }
            if !state.entries.contains_key(&key) && state.entries.len() >= MAX_ENTRIES {
                return Err(Error::overloaded("Noise directory capacity".into()));
            }
            // One host may have several service names only if dialing is
            // unambiguous. Keep the directory's host lookup one-to-one.
            if state.entries.iter().any(|(k, e)| {
                k != &key && k.1 == binding.recipient && e.binding.host == binding.host
            }) {
                return Err(failed("Noise host already published for recipient"));
            }
            let generation = DiscoveryGeneration::after(state.next)?;
            state.next = generation.get();
            let old = state.entries.insert(
                key,
                Rc::new(Entry {
                    binding: Rc::new(binding),
                    generation,
                    expires: Instant::now() + lifetime,
                    visible: true,
                }),
            );
            (old, generation, state.changed.clone())
        };
        Ok(Change {
            old,
            generation,
            changed,
        })
    }
    /// Revocation requires the current generation. A stale publisher cannot
    /// remove a replacement. Recreated names receive a fresh global generation.
    pub fn revoke(&self, name: &str, recipient: [u8; 32], generation: DiscoveryGeneration) -> bool {
        let key = (name.to_owned(), recipient);
        let (old, changed) = {
            let mut state = self.0.borrow_mut();
            if state
                .entries
                .get(&key)
                .is_none_or(|e| e.generation != generation)
            {
                return false;
            }
            (state.entries.remove(&key), state.changed.clone())
        };
        drop(old);
        changed.notify_waiters();
        true
    }
    pub fn close(&self) {
        let (entries, changed) = {
            let mut state = self.0.borrow_mut();
            state.closed = true;
            (std::mem::take(&mut state.entries), state.changed.clone())
        };
        drop(entries);
        changed.notify_waiters();
    }
    pub fn client(&self, recipient: [u8; 32]) -> directory::Client {
        capnp_rpc::new_client(Reader {
            directory: self.clone(),
            recipient,
        })
    }
}
fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-/".contains(&b))
}
struct Reader {
    directory: Directory,
    recipient: [u8; 32],
}
impl Reader {
    fn read(
        &self,
        predicate: impl Fn(&(String, [u8; 32]), &Entry) -> bool,
        mut out: record::Builder<'_>,
    ) -> capnp::Result<()> {
        let entry = {
            let state = self.directory.0.borrow();
            if state.closed {
                return Err(Error::disconnected("Noise directory closed".into()));
            }
            let now = Instant::now();
            state
                .entries
                .iter()
                .find(|(k, e)| {
                    k.1 == self.recipient && e.visible && e.expires > now && predicate(k, e)
                })
                .map(|(_, e)| e.clone())
                .ok_or_else(|| failed("Noise discovery binding absent or expired"))?
        };
        // ClientHook::add_ref() is user code. Clone the provider only after
        // releasing the registry borrow, just as for capability destruction.
        let binding = entry.binding.as_ref().clone();
        let generation = entry.generation;
        let remaining = entry
            .expires
            .saturating_duration_since(Instant::now())
            .as_millis() as u32;
        if remaining == 0 {
            return Err(failed("Noise discovery binding expired"));
        }
        out.set_host(&binding.host);
        out.set_recipient(&binding.recipient);
        out.set_address(binding.address.to_string());
        out.set_context(&binding.context);
        out.set_generation(generation.get());
        out.set_remaining_millis(remaining);
        // Directory lookup must not shorten the provisioning control route into
        // the very host connection that the connector is trying to establish.
        out.set_provider(relay(binding.provider));
        Ok(())
    }
}
impl directory::Server for Reader {
    async fn lookup(
        self: Rc<Self>,
        params: directory::LookupParams,
        mut results: directory::LookupResults,
    ) -> capnp::Result<()> {
        let host = params.get()?.get_host()?;
        if host.len() != 32 {
            return Err(failed("invalid discovery host"));
        }
        self.read(|_, e| e.binding.host == host, results.get().init_record())
    }
    async fn resolve(
        self: Rc<Self>,
        params: directory::ResolveParams,
        mut results: directory::ResolveResults,
    ) -> capnp::Result<()> {
        let name = params.get()?.get_name()?.to_str()?;
        if !name_ok(name) {
            return Err(failed("invalid discovery name"));
        }
        self.read(|k, _| k.0 == name, results.get().init_record())
    }
}

fn decode(
    record: record::Reader<'_>,
    recipient: [u8; 32],
    started: Instant,
) -> capnp::Result<Resolved> {
    let host = record
        .get_host()?
        .try_into()
        .map_err(|_| failed("invalid discovered host"))?;
    let actual = record
        .get_recipient()?
        .try_into()
        .map_err(|_| failed("invalid discovered recipient"))?;
    let text = record.get_address()?;
    let context = record.get_context()?;
    let remaining = record.get_remaining_millis();
    let generation = DiscoveryGeneration::new(record.get_generation())
        .ok_or_else(|| failed("invalid discovery generation"))?;
    if actual != recipient
        || host == [0; 32]
        || host == recipient
        || text.len() > 128
        || context.len() > 1024
        || remaining == 0
        || remaining > 60_000
    {
        return Err(failed("invalid discovery binding"));
    }
    let address = text
        .to_str()?
        .parse()
        .map_err(|_| failed("invalid discovery address"))?;
    if !crate::nat::address_ok(address) {
        return Err(failed("invalid discovery address"));
    }
    // Starting at request time is deliberately conservative about network delay.
    let expires = started + Duration::from_millis(remaining.into());
    if Instant::now() >= expires {
        return Err(failed("discovery response expired in transit"));
    }
    Ok(Resolved::new(
        Binding {
            host,
            recipient: actual,
            address,
            context: context.to_vec(),
            provider: record.get_provider()?,
        },
        generation,
        expires,
    ))
}
pub async fn resolve(
    directory: &directory::Client,
    recipient: [u8; 32],
    name: &str,
) -> capnp::Result<Resolved> {
    Discovery::from(directory.clone())
        .resolve(recipient, name)
        .await
}

/// Looks up a pinned host on every dial through a delegated reader or Discovery
/// reader set. Retries perform a new lookup and get a fresh provisioning
/// reservation. Optional STUN enables rendezvous.
pub struct DiscoveryConnector {
    identity: Rc<Identity>,
    bind: SocketAddr,
    directory: Discovery,
    stun: Option<SocketAddr>,
}
impl DiscoveryConnector {
    /// Resolve the directory's current key and endpoint for a service name on
    /// each new session. Attach using session.peer(); never relabel a vat ID.
    pub async fn connect_name(&self, name: &str) -> capnp::Result<AuthenticatedSession> {
        let resolved = self
            .directory
            .resolve(self.identity.public_key(), name)
            .await?;
        self.connect_resolved(resolved).await
    }
    pub fn new(
        identity: Rc<Identity>,
        bind: SocketAddr,
        directory: impl Into<Discovery>,
        stun: Option<SocketAddr>,
    ) -> capnp::Result<Self> {
        if stun.is_some_and(|a| !crate::nat::address_ok(a)) {
            return Err(failed("invalid STUN server"));
        }
        Ok(Self {
            identity,
            bind,
            directory: directory.into(),
            stun,
        })
    }
    pub async fn connect_resolved(
        &self,
        resolved: Resolved,
    ) -> capnp::Result<AuthenticatedSession> {
        let expires = resolved.expires();
        if resolved.binding().recipient != self.identity.public_key() || Instant::now() >= expires {
            return Err(failed("expired or foreign discovery binding"));
        }
        let connector = match self.stun {
            Some(server) => {
                ProvisioningConnector::with_stun(self.identity.clone(), self.bind, server)?
            }
            None => ProvisioningConnector::new(self.identity.clone(), self.bind),
        };
        let (b, _) = resolved.into_parts();
        connector.insert(b.host, b.provider, b.address, &b.context)?;
        tokio::select! {
            biased;
            _=tokio::time::sleep_until(expires)=>Err(Error::disconnected("Noise discovery lease expired during setup".into())),
            result=connector.connect(b.host)=>{
                let session=result?;
                if Instant::now()>=expires { return Err(Error::disconnected("Noise discovery lease expired during setup".into())); }
                Ok(session)
            },
        }
    }
}
impl Connector for DiscoveryConnector {
    fn connect(&self, peer: [u8; 32]) -> Promise<AuthenticatedSession, Error> {
        let this = Self {
            identity: self.identity.clone(),
            bind: self.bind,
            directory: self.directory.clone(),
            stun: self.stun,
        };
        Promise::from_future(async move {
            let resolved = this
                .directory
                .lookup(this.identity.public_key(), peer)
                .await?;
            this.connect_resolved(resolved).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use capnp::capability::FromClientHook;
    use capnp::private::capability::{ClientHook, ParamsHook, ResultsHook};
    use capnp::traits::ImbueMut;
    use std::{cell::Cell, rc::Weak};
    struct Reentrant {
        registry: Weak<RefCell<State>>,
        cloned: Rc<Cell<usize>>,
    }
    impl ClientHook for Reentrant {
        fn add_ref(&self) -> Box<dyn ClientHook> {
            let registry = self.registry.upgrade().unwrap();
            assert!(
                registry.try_borrow_mut().is_ok(),
                "provider clone reentered a borrowed registry"
            );
            self.cloned.set(self.cloned.get() + 1);
            Box::new(Self {
                registry: self.registry.clone(),
                cloned: self.cloned.clone(),
            })
        }
        fn new_call(
            &self,
            _: u64,
            _: u16,
            _: Option<capnp::MessageSize>,
        ) -> capnp::capability::Request<capnp::any_pointer::Owned, capnp::any_pointer::Owned>
        {
            panic!("no invocation expected")
        }
        fn call(
            &self,
            _: u64,
            _: u16,
            _: Box<dyn ParamsHook>,
            _: Box<dyn ResultsHook>,
        ) -> Promise<(), Error> {
            Promise::err(failed("no invocation expected"))
        }
        fn get_brand(&self) -> usize {
            0
        }
        fn get_ptr(&self) -> usize {
            self as *const Self as usize
        }
        fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
            None
        }
        fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, Error>> {
            None
        }
        fn when_resolved(&self) -> Promise<(), Error> {
            Promise::ok(())
        }
    }
    #[test]
    fn lookup_clones_capabilities_outside_the_registry_borrow() {
        let directory = Directory::default();
        let cloned = Rc::new(Cell::new(0));
        let provider = provisioner::Client::new(Box::new(Reentrant {
            registry: Rc::downgrade(&directory.0),
            cloned: cloned.clone(),
        }));
        let _ = directory
            .publish(
                "service",
                Binding {
                    host: [1; 32],
                    recipient: [2; 32],
                    address: "127.0.0.1:12345".parse().unwrap(),
                    context: vec![],
                    provider,
                },
                None,
                Duration::from_secs(1),
            )
            .unwrap();
        let mut message = capnp::message::Builder::new_default();
        let mut caps = vec![];
        let mut out = message.init_root::<record::Builder>();
        out.imbue_mut(&mut caps);
        Reader {
            directory: directory.clone(),
            recipient: [2; 32],
        }
        .read(|_, _| true, out)
        .unwrap();
        assert!(cloned.get() > 0);
    }
}
