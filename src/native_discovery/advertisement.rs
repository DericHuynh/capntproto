//! Coordinate an owned name, its recipient-bound provider and observed mapping.
use super::*;
use crate::{
    nat::{Mapping, MappingObserver, MappingStatus},
    native_listener::Listener,
    native_provisioning::Provisioner,
    native_rpc::Handle,
};
use std::cell::Cell;
use tokio::sync::watch;

#[derive(Clone, Copy, Debug)]
pub struct AdvertisementOptions {
    pub expected_generation: Option<DiscoveryGeneration>,
    /// Renewed at half this lifetime, including while suspended: 1–60s.
    pub lifetime: Duration,
    /// Explicitly delegate bounded UDP punching to the recipient.
    pub rendezvous: bool,
}
impl Default for AdvertisementOptions {
    fn default() -> Self {
        Self {
            expected_generation: None,
            lifetime: Duration::from_secs(30),
            rendezvous: false,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdvertisementStop {
    Owner,
    Mapping,
    Listener,
    Network,
    Driver,
    Publication(PublicationStatus),
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdvertisementStatus {
    Published {
        address: SocketAddr,
        generation: DiscoveryGeneration,
    },
    Suspended {
        generation: DiscoveryGeneration,
    },
    Stopped(AdvertisementStop),
}

/// One listener/network/mapping pair can publish several recipient-scoped
/// services. This does not own the mapping; keep its Mapping owner alive.
#[derive(Clone)]
pub struct MappedService {
    listener: Listener,
    network: Handle,
    mapping: MappingObserver,
}
impl MappedService {
    pub fn new(listener: Listener, network: Handle, mapping: &Mapping) -> capnp::Result<Self> {
        if listener.identity() != network.identity()
            || !listener.owns_mapping(mapping)
            || mapping.status() == MappingStatus::Stopped
            || !listener.stats().accepting
            || network.is_closed()
        {
            return Err(failed("invalid mapped Native service"));
        }
        Ok(Self {
            listener,
            network,
            mapping: mapping.subscribe(),
        })
    }
    /// Requires a current mapping observation. Publishes a matching provider and
    /// address together, then follows mapping changes on the Tokio LocalSet.
    pub fn advertise(
        &self,
        directory: &Directory,
        name: &str,
        recipient: [u8; 32],
        context: &[u8],
        options: AdvertisementOptions,
    ) -> capnp::Result<Advertisement> {
        if !self.listener.stats().accepting || self.network.is_closed() {
            return Err(failed("mapped Native service stopped"));
        }
        let MappingStatus::Observed { address, .. } = self.mapping.status() else {
            return Err(failed("mapped Native service has no fresh address"));
        };
        let state = Managed::new(
            self.clone(),
            directory,
            name,
            recipient,
            context,
            options,
            address,
        )?;
        state.reconcile(self.mapping.status());
        if state.terminal.get().is_some() {
            return Err(failed("mapped Native publication retired during setup"));
        }
        let changes = state.changes.subscribe();
        let task = state.start(directory);
        Ok(Advertisement {
            state,
            task,
            changes,
        })
    }
    fn provider(
        &self,
        recipient: [u8; 32],
        context: &[u8],
        rendezvous: bool,
        address: SocketAddr,
    ) -> capnp::Result<OwnedProvider> {
        let create = if rendezvous {
            Provisioner::with_rendezvous
        } else {
            Provisioner::new
        };
        create(
            self.listener.clone(),
            self.network.clone(),
            recipient,
            address,
            context,
        )
        .map(OwnedProvider)
    }
    fn binding(
        &self,
        recipient: [u8; 32],
        context: &[u8],
        address: SocketAddr,
        provider: &OwnedProvider,
    ) -> Binding {
        Binding {
            host: self.listener.identity(),
            recipient,
            address,
            context: context.to_vec(),
            provider: provider.0.client(),
        }
    }
}
struct OwnedProvider(Provisioner);
impl Drop for OwnedProvider {
    fn drop(&mut self) {
        self.0.close();
    }
}
// Capture outside the future so cancellation before its first poll also retires
// authority. The public owner can outlive the LocalSet that drove it.
struct Running(Rc<Managed>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.stop(AdvertisementStop::Driver);
    }
}
struct Managed {
    service: MappedService,
    recipient: [u8; 32],
    context: Vec<u8>,
    rendezvous: bool,
    publication: Publication,
    provider: RefCell<Option<OwnedProvider>>,
    address: Cell<SocketAddr>,
    terminal: Cell<Option<AdvertisementStop>>,
    failure: RefCell<Option<Error>>,
    changes: watch::Sender<AdvertisementStatus>,
}
impl Managed {
    fn start(self: &Rc<Self>, directory: &Directory) -> tokio::task::JoinHandle<()> {
        let running = Running(self.clone());
        let mut mapping = self.service.mapping.clone();
        let directory_changed = directory.0.borrow().changed.clone();
        let listener_changed = self.service.listener.admission_changed();
        let network_changed = self.service.network.admission_changed();
        tokio::task::spawn_local(async move {
            let state = &running.0;
            loop {
                let directory = directory_changed.notified();
                let listener = listener_changed.notified();
                let network = network_changed.notified();
                tokio::pin!(directory, listener, network);
                directory.as_mut().enable();
                listener.as_mut().enable();
                network.as_mut().enable();
                state.reconcile(mapping.status());
                if state.terminal.get().is_some() {
                    break;
                }
                tokio::select! {
                    biased;
                    _ = directory => {},
                    _ = listener => {},
                    _ = network => {},
                    _ = mapping.changed() => {},
                }
            }
        })
    }
    fn new(
        service: MappedService,
        directory: &Directory,
        name: &str,
        recipient: [u8; 32],
        context: &[u8],
        options: AdvertisementOptions,
        address: SocketAddr,
    ) -> capnp::Result<Rc<Self>> {
        let provider = service.provider(recipient, context, options.rendezvous, address)?;
        let binding = service.binding(recipient, context, address, &provider);
        let publication =
            directory.maintain(name, binding, options.expected_generation, options.lifetime)?;
        let generation = publication.generation();
        Ok(Rc::new(Self {
            service,
            recipient,
            context: context.to_vec(),
            rendezvous: options.rendezvous,
            publication,
            provider: RefCell::new(Some(provider)),
            address: Cell::new(address),
            terminal: Cell::new(None),
            failure: RefCell::new(None),
            changes: watch::channel(AdvertisementStatus::Published {
                address,
                generation,
            })
            .0,
        }))
    }
    fn status(&self) -> AdvertisementStatus {
        if let Some(reason) = self.terminal.get() {
            return AdvertisementStatus::Stopped(reason);
        }
        let generation = self.publication.generation();
        if self.provider.borrow().is_some() {
            AdvertisementStatus::Published {
                address: self.address.get(),
                generation,
            }
        } else {
            AdvertisementStatus::Suspended { generation }
        }
    }
    fn notify(&self) {
        let status = self.status();
        self.changes.send_if_modified(|old| {
            if *old == status {
                false
            } else {
                *old = status;
                true
            }
        });
    }
    fn stop(&self, reason: AdvertisementStop) {
        if self.terminal.get().is_some() {
            return;
        }
        self.terminal.set(Some(reason));
        let provider = self.provider.borrow_mut().take();
        drop(provider); // Lease cancellation can reenter this owner.
        self.publication.stop();
        self.notify();
    }
    fn reconcile(&self, mapping: MappingStatus) {
        if self.terminal.get().is_some() {
            return;
        }
        if !self.service.listener.stats().accepting {
            self.stop(AdvertisementStop::Listener);
            return;
        }
        if self.service.network.is_closed() {
            self.stop(AdvertisementStop::Network);
            return;
        }
        let publication = self.publication.status();
        if !matches!(publication, PublicationStatus::Active { .. }) {
            self.stop(AdvertisementStop::Publication(publication));
            return;
        }
        let result = match mapping {
            MappingStatus::Stopped => {
                self.stop(AdvertisementStop::Mapping);
                return;
            }
            MappingStatus::Unavailable | MappingStatus::Discovering => {
                let provider = self.provider.borrow_mut().take();
                if provider.is_some() {
                    drop(provider);
                    if self.terminal.get().is_some() {
                        return;
                    }
                    self.publication.suspend()
                } else {
                    Ok(())
                }
            }
            MappingStatus::Observed { address, .. } => {
                if self.provider.borrow().is_some() && self.address.get() == address {
                    self.notify();
                    return;
                }
                self.replace(address)
            }
        };
        if let Err(error) = result {
            *self.failure.borrow_mut() = Some(error);
            let status = self.publication.status();
            self.stop(if matches!(status, PublicationStatus::Active { .. }) {
                AdvertisementStop::Failed
            } else {
                AdvertisementStop::Publication(status)
            });
        }
        self.notify();
    }
    fn replace(&self, address: SocketAddr) -> capnp::Result<()> {
        let provider =
            self.service
                .provider(self.recipient, &self.context, self.rendezvous, address)?;
        let binding = self
            .service
            .binding(self.recipient, &self.context, address, &provider);
        // Install the owner before publication can wake callbacks. Reentrant
        // stop must close the new provider as well as revoke its generation.
        let old = self.provider.borrow_mut().replace(provider);
        self.address.set(address);
        drop(old);
        if self.terminal.get().is_some() {
            return Ok(());
        }
        self.publication.replace(binding)
    }
}
impl Drop for Managed {
    fn drop(&mut self) {
        self.stop(AdvertisementStop::Owner);
    }
}

/// Owns publication and all its issued provider generations. Stop/drop removes
/// only its own name generation and cancels pending reservations. Authenticated
/// sessions, the shared mapping and other recipients' advertisements survive.
#[must_use = "dropping the advertisement withdraws it"]
pub struct Advertisement {
    state: Rc<Managed>,
    task: tokio::task::JoinHandle<()>,
    changes: watch::Receiver<AdvertisementStatus>,
}
impl Advertisement {
    pub fn status(&self) -> AdvertisementStatus {
        self.state.status()
    }
    pub fn generation(&self) -> DiscoveryGeneration {
        self.state.publication.generation()
    }
    pub fn failure(&self) -> Option<Error> {
        self.state.failure.borrow().clone()
    }
    pub async fn changed(&mut self) -> AdvertisementStatus {
        if self.state.terminal.get().is_none() {
            let _ = self.changes.changed().await;
        }
        self.status()
    }
    pub fn stop(&self) {
        self.state.stop(AdvertisementStop::Owner);
        self.task.abort();
    }
}
impl Drop for Advertisement {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{nat::MappingOptions, native_listener::Limits, native_rpc::Network};
    async fn setup(directory: &Directory) -> (Rc<Managed>, Mapping, Network) {
        let identity = Rc::new(Identity::generate());
        let listener = Listener::bind(
            "127.0.0.1:0".parse().unwrap(),
            identity.clone(),
            Limits::default(),
        )
        .await
        .unwrap();
        let mapping = listener
            .maintain_mapping(
                "127.0.0.1:12345".parse().unwrap(),
                MappingOptions::default(),
            )
            .unwrap();
        let (network, handle) = Network::new(identity.public_key());
        let service = MappedService::new(listener, handle, &mapping).unwrap();
        let state = Managed::new(
            service,
            directory,
            "service",
            [3; 32],
            b"managed",
            AdvertisementOptions {
                lifetime: Duration::from_secs(60),
                ..AdvertisementOptions::default()
            },
            "127.0.0.1:10001".parse().unwrap(),
        )
        .unwrap();
        (state, mapping, network)
    }
    fn observation() -> MappingStatus {
        MappingStatus::Observed {
            address: "127.0.0.1:10002".parse().unwrap(),
            observed_at: Instant::now(),
        }
    }
    async fn check_provider(provider: &provisioner::Client, open: bool, port: Option<u16>) {
        let response = provider.reserve_request().send().promise.await;
        assert_eq!(response.is_ok(), open);
        if let Ok(response) = response {
            let result = response.get().unwrap();
            if let Some(port) = port {
                assert_eq!(
                    result
                        .get_ticket()
                        .unwrap()
                        .get_address()
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .parse::<SocketAddr>()
                        .unwrap()
                        .port(),
                    port
                );
            }
            let lease = result.get_lease().unwrap();
            lease.cancel_request().send().promise.await.unwrap();
            assert!(lease.ready_request().send().promise.await.is_err());
        }
    }
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn replay_tlc_mapped_advertisement() {
        use capntproto_test_support::verification::exploration;
        let config = include_str!("../../verification/NativeAdvertisement.cfg");
        let live = config.replace("SPECIFICATION Spec", "SPECIFICATION LiveSpec")
            + "\nPROPERTY OwnerStops\n";
        exploration::controls(
            "verification/NativeAdvertisement.tla",
            "native-advertisement",
            config,
            &[
                ("splitAddress", "AddressMatches"),
                ("staleProvider", "Suspended"),
                ("resurrect", "NoResurrection"),
                ("staleDrop", "SuccessorSurvives"),
            ],
            Some(&live),
        )
        .unwrap();
        let traces = exploration::traces(
            "verification/NativeAdvertisement.tla",
            "native-advertisement",
            config,
        )
        .unwrap();
        tokio::task::LocalSet::new()
            .run_until(async {
                for trace in traces {
                    let directory = Directory::default();
                    let (state, _mapping, network) = setup(&directory).await;
                    let mut network = Some(network);
                    let old = state.provider.borrow().as_ref().unwrap().0.client();
                    let mut changed = observation();
                    for model in trace {
                        match model["event"] {
                            1 | 3 => {
                                changed = observation();
                                state.reconcile(changed);
                            }
                            2 => {
                                changed = MappingStatus::Unavailable;
                                state.reconcile(changed);
                            }
                            4 => {
                                struct Other;
                                impl provisioner::Server for Other {}
                                let _ = directory
                                    .publish(
                                        "service",
                                        Binding {
                                            host: [9; 32],
                                            recipient: [3; 32],
                                            address: "127.0.0.1:10003".parse().unwrap(),
                                            context: vec![],
                                            provider: capnp_rpc::new_client(Other),
                                        },
                                        Some(state.publication.generation()),
                                        Duration::from_secs(60),
                                    )
                                    .unwrap();
                                state.reconcile(changed);
                            }
                            5 => {
                                assert!(directory.revoke(
                                    "service",
                                    [3; 32],
                                    state.publication.generation()
                                ));
                                state.reconcile(changed);
                            }
                            6 => state.stop(AdvertisementStop::Owner),
                            7 => state.reconcile(MappingStatus::Stopped),
                            8 => {
                                state.service.listener.stop_accepting();
                                state.reconcile(changed);
                            }
                            9 => {
                                network.take();
                                state.reconcile(changed);
                            }
                            10 => {
                                tokio::time::advance(Duration::from_secs(60)).await;
                                state.reconcile(changed);
                            }
                            11 => {
                                directory.close();
                                state.reconcile(changed);
                            }
                            12 => {
                                let task = state.start(&directory);
                                task.abort();
                                assert!(task.await.unwrap_err().is_cancelled());
                                assert_eq!(
                                    state.status(),
                                    AdvertisementStatus::Stopped(AdvertisementStop::Driver)
                                );
                            }
                            _ => panic!("unknown advertisement action"),
                        }
                        assert_eq!(state.publication.generation().get(), model["owned"]);
                        assert_eq!(directory.0.borrow().next, model["generation"]);
                        assert_eq!(
                            directory.0.borrow().entries.len(),
                            usize::from(model["current"] != 0)
                        );
                        assert_eq!(
                            match state.status() {
                                AdvertisementStatus::Published { .. } => 0,
                                AdvertisementStatus::Suspended { .. } => 1,
                                AdvertisementStatus::Stopped(_) => 2,
                            },
                            model["phase"]
                        );
                        let found = resolve(&directory.client([3; 32]), [3; 32], "service").await;
                        assert_eq!(found.is_ok(), model["address"] != 0);
                        if let Ok(found) = found {
                            assert_eq!(
                                found.binding().address.port(),
                                10000 + model["address"] as u16
                            );
                            if model["current"] == 1 {
                                check_provider(
                                    &found.binding().provider,
                                    true,
                                    Some(10000 + model["provider"] as u16),
                                )
                                .await;
                            }
                        }
                        check_provider(&old, model["oldLive"] == 1, None).await;
                    }
                }
            })
            .await;
    }
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn suspended_name_retains_claim_and_recovery_uses_fresh_authority() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let directory = Directory::default();
                let (state, _mapping, _network) = setup(&directory).await;
                let old = state.provider.borrow().as_ref().unwrap().0.client();
                let pending = old.reserve_request().send().promise.await.unwrap();
                let pending = pending.get().unwrap().get_lease().unwrap();
                state.reconcile(MappingStatus::Unavailable);
                assert!(pending.ready_request().send().promise.await.is_err());
                assert!(matches!(
                    state.status(),
                    AdvertisementStatus::Suspended { .. }
                ));
                let address = "127.0.0.1:10002".parse().unwrap();
                let contender = state
                    .service
                    .provider([3; 32], b"managed", false, address)
                    .unwrap();
                assert!(directory
                    .publish(
                        "service",
                        state
                            .service
                            .binding([3; 32], b"managed", address, &contender),
                        None,
                        Duration::from_secs(60)
                    )
                    .is_err());
                assert!(resolve(&directory.client([3; 32]), [3; 32], "service")
                    .await
                    .is_err());
                // A suspended owner keeps renewing its hidden claim.
                let previous = state.publication.generation();
                for _ in 0..8 {
                    tokio::task::yield_now().await;
                }
                tokio::time::advance(Duration::from_secs(30)).await;
                for _ in 0..8 {
                    tokio::task::yield_now().await;
                }
                assert!(state.publication.generation() > previous);
                assert!(resolve(&directory.client([3; 32]), [3; 32], "service")
                    .await
                    .is_err());
                state.reconcile(observation());
                let fresh = resolve(&directory.client([3; 32]), [3; 32], "service")
                    .await
                    .unwrap();
                check_provider(&old, false, None).await;
                check_provider(&fresh.binding().provider, true, Some(10002)).await;
                // Exhaustion retires the entry and both provider generations.
                directory.0.borrow_mut().next = u64::MAX;
                state.reconcile(MappingStatus::Unavailable);
                assert!(matches!(state.status(), AdvertisementStatus::Stopped(_)));
                assert!(state.failure.borrow().is_some());
                assert!(directory.0.borrow().entries.is_empty());
                check_provider(&fresh.binding().provider, false, None).await;
            })
            .await;
    }
    #[tokio::test(flavor = "current_thread")]
    async fn reentrant_stop_closes_new_authority_and_cannot_republish() {
        use futures::FutureExt;
        use std::{
            future::Future,
            rc::Weak,
            sync::{
                atomic::{AtomicUsize, Ordering},
                Arc,
            },
            task::{Context, Wake, Waker},
        };
        thread_local! {
            static OWNER: RefCell<Weak<Managed>> = const { RefCell::new(Weak::new()) };
            static CAPTURED: RefCell<Option<provisioner::Client>> = const { RefCell::new(None) };
        }
        struct StopWake(AtomicUsize);
        impl Wake for StopWake {
            fn wake(self: Arc<Self>) {
                self.wake_by_ref();
            }
            fn wake_by_ref(self: &Arc<Self>) {
                if let Some(owner) = OWNER.with(|slot| slot.borrow().upgrade()) {
                    let provider = owner.provider.borrow().as_ref().map(|p| p.0.clone());
                    if let Some(provider) = provider {
                        CAPTURED.with(|slot| *slot.borrow_mut() = Some(provider.client()));
                    }
                    owner.stop(AdvertisementStop::Owner);
                    self.0.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
        tokio::task::LocalSet::new()
            .run_until(async {
                for from_publication in [false, true] {
                    let directory = Directory::default();
                    let (state, _mapping, _network) = setup(&directory).await;
                    let original = state.provider.borrow().as_ref().unwrap().0.client();
                    let pending = original.reserve_request().send().promise.await.unwrap();
                    let lease = pending.get().unwrap().get_lease().unwrap();
                    let signal = directory.0.borrow().changed.clone();
                    let mut wait: std::pin::Pin<Box<dyn Future<Output = ()> + '_>> =
                        if from_publication {
                            Box::pin(signal.notified())
                        } else {
                            Box::pin(
                                lease
                                    .ready_request()
                                    .send()
                                    .promise
                                    .map(|result| assert!(result.is_err())),
                            )
                        };
                    let wake = Arc::new(StopWake(AtomicUsize::new(0)));
                    let waker = Waker::from(wake.clone());
                    let mut context = Context::from_waker(&waker);
                    assert!(wait.as_mut().poll(&mut context).is_pending());
                    OWNER.with(|slot| *slot.borrow_mut() = Rc::downgrade(&state));
                    state.reconcile(observation());
                    assert!(wake.0.load(Ordering::SeqCst) > 0);
                    assert_eq!(
                        state.status(),
                        AdvertisementStatus::Stopped(AdvertisementStop::Owner)
                    );
                    assert!(directory.0.borrow().entries.is_empty());
                    wait.await;
                    let captured = CAPTURED.with(|slot| slot.borrow_mut().take().unwrap());
                    check_provider(&captured, false, None).await;
                    check_provider(&original, false, None).await;
                    OWNER.with(|slot| *slot.borrow_mut() = Weak::new());
                }
            })
            .await;
    }
}
