//! Owned `ClientHook` endpoints for a caller-polled, scoped capability graph.
//!
//! This is the hosted migration boundary, not a second network runtime. Hooks
//! contain only owned inbox identities; branded graph clients never escape the
//! scope. The caller admits each [`Call`] to its engine driver in receive order,
//! publishes pipelines through its results hook, and completes or cancels it.
//! Calls, parameter capabilities and result capabilities use the existing hook
//! API. No executor, IO, clock or background task is created here.
//!
//! Pending graph promises retain their route until the caller observes graph
//! progress with [`ClientHooks::collect`]. Network promise resolution, native tail
//! transfer and Join still belong to the hosted runtime. Caller-supplied endpoint
//! metadata preserves attached resources and hosted diagnostics;
//! this bridge does not replace `RpcSystem` yet.
#![forbid(unsafe_code)]

use capnp::{
    any_pointer,
    capability::{self, CallHints, Promise},
    private::capability::{
        ClientHook, DebugInfo, MetadataChanges, MetadataWatch, ParamsHook, ResultsHook,
    },
    Error,
};
use capntproto_rpc_engine::{
    clients::{dispatch, Capabilities, Client},
    completion::{self, shared},
};
use futures::{future::Shared, FutureExt};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    future::Future,
    pin::Pin,
    rc::{Rc, Weak},
    task::{Context, Poll, Waker},
};

mod admission;
mod bindings;
mod metadata;
pub use metadata::MetadataSource;
pub(crate) mod response;
mod shutdown;
pub mod wire;

/// Owned metadata projected from a scoped endpoint by its hosting adapter.
/// This carries no capability authority. Introduction contacts must come from
/// this endpoint's admitted descriptor and are authenticated by its network
/// before redirecting calls. Descriptor ownership remains shared with the import.
#[derive(Clone, Default)]
pub struct EndpointMetadata {
    pub failure: Option<Error>,
    pub introduction: Option<Rc<capnp::message::Builder<capnp::message::HeapAllocator>>>,
    #[cfg(unix)]
    pub fd: Option<Rc<std::os::fd::OwnedFd>>,
}

type Shutdown = shutdown::Receiver;
struct Inbox {
    completion_cache: response::Cache,
    call_templates: [std::cell::OnceCell<Box<CallTemplate>>; 4],
    message_pool: Option<capnp::message::SegmentPool>,
    admission: Option<Rc<crate::admission::Admission>>,
    joins: Option<wire::coordinator::multiparty::Handle>,
    queue: RefCell<VecDeque<Box<Call>>>,
    owners: Ownership,

    wake: RefCell<Option<Waker>>,
    enqueue_wake: RefCell<Option<Rc<wire::coordinator::EnqueueWake>>>,
    shutdown: Shutdown,
    closed: Cell<bool>,
    live_calls: Cell<usize>,
    maximum_calls: usize,
    maximum_clients: usize,
}
/// One shared work owner for endpoint destruction and metadata notifications.
/// Hooks retain this through their inbox; the scoped registry consumes the same
/// queue identities. A notification never scans or borrows the capability graph.
#[derive(Default)]
struct Ownership {
    // Pre-reserved once per registry slot: final endpoint Drop cannot allocate.
    released: RefCell<VecDeque<usize>>,
    metadata: wire::ready::Queue,
    wake: RefCell<Option<Waker>>,
}
impl Ownership {
    fn register(&self) {
        // AtomicWaker consumes its registration on wake. Rearm before observing
        // or consuming work, including stale events from recycled slots.
        if let Some(wake) = self.wake.borrow().as_ref() {
            self.metadata.register(wake);
        }
    }
}
struct CallTemplate {
    interface: u64,
    method: u16,
    template: capntproto_rpc_wire::owned::connection::PreparedCallTemplate,
}
impl Inbox {
    fn call_template(
        &self,
        interface: u64,
        method: u16,
    ) -> &capntproto_rpc_wire::owned::connection::PreparedCallTemplate {
        // A connection commonly alternates methods. Retain a small, bounded
        // set of headers so those requests need no builder traversal merely
        // to change their method. Further methods use the ordinary fallback.
        for slot in &self.call_templates {
            let entry = slot.get_or_init(|| {
                Box::new(CallTemplate {
                    interface,
                    method,
                    template: capntproto_rpc_wire::owned::connection::PreparedCallTemplate::new(
                        interface, method,
                    ),
                })
            });
            if entry.interface == interface && entry.method == method {
                return &entry.template;
            }
        }
        &self.call_templates[0]
            .get()
            .expect("initialized template")
            .template
    }
    fn notify_enqueue(&self) {
        let wake = self.enqueue_wake.borrow().clone();
        match wake {
            Some(wake) => wake.notify(),
            None => self.notify(),
        }
    }
    fn notify_ownership(&self) {
        let signal = self.owners.wake.borrow().clone();
        match signal {
            Some(signal) => signal.wake(),
            None => self.notify(),
        }
    }
    fn notify(&self) {
        let wake = self.wake.borrow_mut().take();
        if let Some(wake) = wake {
            wake.wake();
        }
    }
    fn close(&self, error: Error) {
        if self.closed.replace(true) {
            return;
        }
        // Detach every callback-bearing owner before waking or destroying it.
        let queue = std::mem::take(&mut *self.queue.borrow_mut());
        self.shutdown.close(error);
        drop(queue);
        self.notify();
    }
}
struct Endpoint {
    introduction: RefCell<Option<Rc<capnp::message::Builder<capnp::message::HeapAllocator>>>>,
    introduced: RefCell<Option<Rc<dyn ClientHook>>>,
    remote: Cell<bool>,
    inbox: Weak<Inbox>,
    index: Cell<Option<usize>>,
    binding_wakes: RefCell<Vec<Waker>>,
    settled: Cell<bool>,
    metadata_generation: Cell<u64>,
    metadata_changes: std::cell::OnceCell<Rc<MetadataChanges>>,
    failure: RefCell<Option<Error>>,
    resolution: RefCell<Option<Resolution>>,
    #[cfg(unix)]
    fd: RefCell<Option<Rc<std::os::fd::OwnedFd>>>,
}
impl Endpoint {
    fn projection(inbox: Weak<Inbox>) -> Rc<Self> {
        Rc::new(Self {
            introduction: RefCell::default(),
            introduced: RefCell::default(),
            remote: Cell::new(true),
            inbox,
            index: Cell::new(None),
            binding_wakes: RefCell::default(),
            settled: Cell::new(false),
            metadata_generation: Cell::new(0),
            metadata_changes: std::cell::OnceCell::new(),
            failure: RefCell::default(),
            resolution: RefCell::default(),
            #[cfg(unix)]
            fd: RefCell::default(),
        })
    }
    fn needs_binding(&self) -> bool {
        self.index.get().is_none()
            && self.introduced.borrow().is_none()
            && self.failure.borrow().is_none()
    }
    fn binding_pending(&self, cx: &mut Context<'_>) -> bool {
        if !self.needs_binding() {
            return false;
        }
        let wake = cx.waker().clone();
        if !self.needs_binding() {
            return false;
        }
        let mut wakes = self.binding_wakes.borrow_mut();
        if !wakes.iter().any(|old| old.will_wake(&wake)) {
            wakes.push(wake);
        } else {
            drop(wakes);
            drop(wake);
        }
        true
    }
    fn notify_binding(&self) {
        let wakes = core::mem::take(&mut *self.binding_wakes.borrow_mut());
        for wake in wakes {
            wake.wake();
        }
    }
    fn forward_projection(&self, result: capnp::Result<Box<dyn ClientHook>>) {
        match result {
            Ok(hook) => {
                *self.introduced.borrow_mut() = Some(Rc::from(hook));
                let resolution = self.resolution.borrow_mut().take();
                if let Some(resolution) = resolution {
                    let _ = resolution.sender.send(Ok(()));
                }
            }
            Err(error) => {
                *self.failure.borrow_mut() = Some(error.clone());
                let resolution = self.resolution.borrow_mut().take();
                if let Some(resolution) = resolution {
                    let _ = resolution.sender.send(Err(error));
                }
            }
        }
        self.notify_binding();
        if let Some(inbox) = self.inbox.upgrade() {
            inbox.notify_ownership();
        }
    }
}
struct Resolution {
    sender: shared::Sender<capnp::Result<()>>,
    receiver: Shared<shared::Receiver<capnp::Result<()>>>,
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        if let Some(inbox) = self.inbox.upgrade() {
            let mut released = inbox.owners.released.borrow_mut();
            if let Some(index) = self.index.get() {
                debug_assert!(released.len() < released.capacity());
                released.push_back(index);
            }
            drop(released);
            inbox.notify_ownership();
        }
    }
}
struct Entry<'id> {
    endpoint: Weak<Endpoint>,
    client: Client<'id>,
    pending: bool,
    metadata: Option<Rc<MetadataSource>>,
    // Aliases can converge on the same terminal key after promise resolution.
    same_key_next: Option<usize>,
    route_subscription: Option<capntproto_rpc_engine::clients::Subscription<'id>>,
    metadata_work: capntproto_rpc_engine::work_queue::Link,
    metadata_watch: Option<(MetadataWatch, Waker)>,
}

/// Scoped graph ownership paired with ordinary owned generated clients.
/// Limits cover exported routes and all queued/running calls, respectively.
/// Drop notifications require no queue space. Call [`Self::collect`] while
/// driving and [`Self::close`] before normal teardown; dropping the bridge is
/// terminal disconnection and leaves graph reclamation to the enclosing scope.
#[must_use = "drive calls and collect released endpoints, or close the bridge"]
pub struct ClientHooks<'id> {
    inbox: Rc<Inbox>,
    entries: Vec<Option<Entry<'id>>>,
    unsettled: usize,
    bindings: bindings::Bindings<'id>,
    free: Vec<usize>,
    sources_bound: bool,
    listener: Option<capntproto_rpc_engine::clients::Listener<'id>>,
    metadata_work: capntproto_rpc_engine::work_queue::Queue,
    maximum_clients: usize,
}
impl<'id> ClientHooks<'id> {
    pub(crate) fn with_joins(mut self, joins: wire::coordinator::multiparty::Handle) -> Self {
        Rc::get_mut(&mut self.inbox)
            .expect("unpublished hooks")
            .joins = Some(joins);
        self
    }
    pub fn new(maximum_clients: usize, maximum_calls: usize) -> Self {
        Self {
            inbox: Rc::new(Inbox {
                completion_cache: response::Cache::new(maximum_calls),
                call_templates: std::array::from_fn(|_| std::cell::OnceCell::new()),
                message_pool: None,
                admission: None,
                joins: None,
                queue: RefCell::new(VecDeque::new()),
                owners: Ownership::default(),
                wake: RefCell::new(None),
                enqueue_wake: RefCell::new(None),
                shutdown: shutdown::Receiver::new(),
                closed: Cell::new(false),
                live_calls: Cell::new(0),
                maximum_calls,
                maximum_clients,
            }),
            entries: Vec::new(),
            unsettled: 0,
            bindings: bindings::Bindings::default(),
            free: Vec::new(),
            sources_bound: false,
            listener: None,
            metadata_work: capntproto_rpc_engine::work_queue::Queue::EMPTY,
            maximum_clients,
        }
    }
    pub(crate) fn with_admission(mut self, admission: Rc<crate::admission::Admission>) -> Self {
        Rc::get_mut(&mut self.inbox)
            .expect("unpublished hook inbox")
            .admission = Some(admission);
        self
    }
    /// Reuse released application request/result segments. The supplied pool
    /// bounds cached storage independently of live messages; retained readers
    /// and builders keep exclusive ownership until their last owner drops.
    /// Configure this before exporting clients or attaching the coordinator.
    /// Panics if the hook inbox has already been shared.
    pub(crate) fn with_message_pool(mut self, pool: capnp::message::SegmentPool) -> Self {
        Rc::get_mut(&mut self.inbox)
            .expect("unpublished hook inbox")
            .message_pool = Some(pool);
        self
    }

    /// Freeze the current advertised route, retaining it in this graph. No raw
    /// wire ID, pointer, lifetime cast or user-provided hook brand is authority.
    /// Repeated exports of the same route share one identity and one graph ref.
    pub fn export_snapshot(
        &mut self,
        graph: &mut impl Capabilities<'id>,
        client: &Client<'id>,
    ) -> capnp::Result<capability::Client> {
        let (client, pending) = graph
            .snapshot_for_export(client)
            .map_err(|e| Error::failed(e.to_string()))?;
        self.export_retained(graph, client, pending, true)
    }

    // Authentication may make a pipeline's existing route final without
    // changing where its already-ordered calls go. Keep local barriers pending.
    fn freeze_projection(
        &mut self,
        graph: &mut impl Capabilities<'id>,
        endpoint: Rc<Endpoint>,
    ) -> capnp::Result<()> {
        let client = self.lookup(&endpoint)?;
        if graph
            .is_settled(client)
            .map_err(|e| Error::failed(e.to_string()))?
        {
            self.collect(graph);
            return Ok(());
        }
        let (client, pending) = graph
            .snapshot_for_export(client)
            .map_err(|e| Error::failed(e.to_string()))?;
        if pending {
            graph.release(client);
            return Ok(());
        }
        let frozen = self.export_retained(graph, client, false, true)?;
        endpoint.forward_projection(Ok(frozen.hook));
        Ok(())
    }

    /// Export a live application client, following imported/pipelined promise
    /// resolution in this graph. Use `export_snapshot` only for an advertised
    /// route that must stay frozen even when its original promise redirects.
    pub fn export_client(
        &mut self,
        graph: &mut impl Capabilities<'id>,
        client: &Client<'id>,
    ) -> capnp::Result<capability::Client> {
        self.collect(graph);
        self.export_after_collect(graph, client)
    }

    /// Transfer an owned graph client into a public client. The endpoint takes
    /// over its graph reference; failures release it before returning the error.
    pub fn import_client(
        &mut self,
        graph: &mut impl Capabilities<'id>,
        client: Client<'id>,
    ) -> capnp::Result<capability::Client> {
        self.collect(graph);
        self.import_after_collect(graph, client)
    }

    fn import_after_collect(
        &mut self,
        graph: &mut impl Capabilities<'id>,
        client: Client<'id>,
    ) -> capnp::Result<capability::Client> {
        let pending = match graph.is_settled(&client) {
            Ok(settled) => !settled,
            Err(error) => {
                graph.release(client);
                return Err(Error::failed(error.to_string()));
            }
        };
        self.bind_retained(graph, client, pending, false, None)
    }

    /// Export and publish hosted diagnostics before any resolution waiter can
    /// observe the endpoint. Use the same projection on subsequent collections.
    pub fn export_client_with_metadata<G: Capabilities<'id>>(
        &mut self,
        graph: &mut G,
        client: &Client<'id>,
        mut metadata: impl FnMut(&G::Owner) -> EndpointMetadata,
    ) -> capnp::Result<capability::Client> {
        self.collect_with_metadata(graph, &mut metadata);
        let exported = self.export_after_collect(graph, client)?;
        self.collect_with_metadata(graph, metadata);
        Ok(exported)
    }

    // Wire completion refreshes metadata before publishing capabilities to
    // application observers. Reclaim dead entries once before reserving the
    // endpoint; its caller performs the following metadata-aware refresh.
    fn export_before_wire_refresh(
        &mut self,
        graph: &mut impl Capabilities<'id>,
        client: &Client<'id>,
    ) -> capnp::Result<capability::Client> {
        self.collect(graph);
        self.export_after_collect(graph, client)
    }

    fn export_after_collect(
        &mut self,
        graph: &mut impl Capabilities<'id>,
        client: &Client<'id>,
    ) -> capnp::Result<capability::Client> {
        let pending = !graph
            .is_settled(client)
            .map_err(|e| Error::failed(e.to_string()))?;
        let client = graph
            .retain_key(client.key())
            .map_err(|e| Error::failed(e.to_string()))?;
        self.export_retained(graph, client, pending, false)
    }

    fn export_retained(
        &mut self,
        graph: &mut impl Capabilities<'id>,
        client: Client<'id>,
        pending: bool,
        refresh: bool,
    ) -> capnp::Result<capability::Client> {
        self.bind_retained(graph, client, pending, refresh, None)
    }
    fn bind_retained(
        &mut self,
        graph: &mut impl Capabilities<'id>,
        client: Client<'id>,
        pending: bool,
        refresh: bool,
        existing: Option<Rc<Endpoint>>,
    ) -> capnp::Result<capability::Client> {
        if self.inbox.closed.get() {
            graph.release(client);
            return Err(Error::disconnected(
                "scoped ClientHook driver closed".into(),
            ));
        }
        if refresh {
            self.collect(graph);
        }
        let key = client.key();
        let mut matching = if existing.is_none() {
            self.bindings.get(&key).copied()
        } else {
            None
        };
        while let Some(index) = matching {
            let entry = self.entries[index].as_ref().expect("indexed endpoint");
            if let Some(endpoint) = entry.endpoint.upgrade() {
                graph.release(client);
                return Ok(capability::Client::new(Box::new(Hook(endpoint, None))));
            }
            matching = entry.same_key_next;
        }
        if self
            .bindings
            .try_reserve(self.entries.len() - self.free.len() + 1 - self.bindings.len())
            .is_err()
        {
            graph.release(client);
            return Err(Error::overloaded(
                "scoped binding capacity exhausted".into(),
            ));
        }
        let index = self.free.last().copied().unwrap_or(self.entries.len());
        if index == self.entries.len() {
            if index == self.maximum_clients
                || self.entries.try_reserve(1).is_err()
                || self.free.try_reserve(index + 1 - self.free.len()).is_err()
            {
                graph.release(client);
                return Err(Error::overloaded("scoped client capacity exhausted".into()));
            }
            // Pending records plus live endpoints can never exceed the slot
            // count. Reserve before publication so Drop cannot reject a release.
            let mut released = self.inbox.owners.released.borrow_mut();
            let additional = index + 1 - released.len();
            if released.try_reserve(additional).is_err() {
                graph.release(client);
                return Err(Error::overloaded(
                    "scoped release capacity exhausted".into(),
                ));
            }
            self.entries.push(None);
        } else {
            self.free.pop();
        }
        let subscribed = (|| {
            let Some(key) = graph.pending_resolution(&client)? else {
                return Ok(None);
            };
            if self.listener.is_none() {
                self.listener = Some(graph.listen()?);
            }
            graph
                .subscribe(
                    self.listener.as_ref().unwrap(),
                    key,
                    index,
                    capntproto_rpc_engine::clients::Watch::Changes,
                )
                .map(Some)
        })();
        let route_subscription = match subscribed {
            Ok(value) => value,
            Err(error) => {
                self.free.push(index);
                graph.release(client);
                return Err(Error::overloaded(error.to_string()));
            }
        };
        // Inspect the route once while the graph is exclusively borrowed.
        let destination = graph.inspect(&client);
        let remote = matches!(
            destination,
            Ok(capntproto_rpc_engine::clients::Destination::Endpoint {
                location: capntproto_rpc_engine::clients::Location::Remote(_),
                ..
            })
        );
        let broken = matches!(
            destination,
            Ok(capntproto_rpc_engine::clients::Destination::Endpoint {
                location: capntproto_rpc_engine::clients::Location::Broken,
                ..
            })
        );
        let endpoint = existing.unwrap_or_else(|| {
            Rc::new(Endpoint {
                introduction: RefCell::default(),
                introduced: RefCell::default(),
                remote: Cell::new(remote),
                inbox: Rc::downgrade(&self.inbox),
                index: Cell::new(None),
                binding_wakes: RefCell::default(),
                settled: Cell::new(!pending),
                metadata_generation: Cell::new(0),
                metadata_changes: std::cell::OnceCell::new(),
                failure: RefCell::new(
                    broken
                        .then(|| Error::failed("capability resolved to a broken endpoint".into())),
                ),
                resolution: RefCell::default(),
                #[cfg(unix)]
                fd: RefCell::new(None),
            })
        });
        assert!(
            endpoint.index.replace(Some(index)).is_none(),
            "projection bound once"
        );
        endpoint.remote.set(remote);
        endpoint.settled.set(!pending);
        self.entries[index] = Some(Entry {
            endpoint: Rc::downgrade(&endpoint),
            client,
            pending,
            metadata: None,
            same_key_next: self.bindings.insert(key, index),
            route_subscription,
            metadata_work: capntproto_rpc_engine::work_queue::Link::EMPTY,
            metadata_watch: None,
        });
        self.queue_metadata(index);
        self.unsettled += usize::from(pending);
        endpoint.notify_binding();
        Ok(capability::Client::new(Box::new(Hook(endpoint, None))))
    }

    /// Reclaim graph refs after the final hook, request and running call drops.
    /// Publish promise readiness only after graph ordering barriers have opened.
    /// A retained response/pipeline may still own hooks and deliberately keeps
    /// those entries alive. No fallible release command is enqueued by Drop.
    pub fn collect(&mut self, graph: &mut impl Capabilities<'id>) {
        self.synchronize(graph, |_| None, |_| None, |_| None, false, false);
    }

    /// Release precisely the graph references whose final hosted owner dropped.
    /// This does not inspect live routes, publish promise readiness, refresh
    /// metadata, or send protocol messages. The driver's ordinary maintenance
    /// still owns Finish/Release ordering and backpressure.
    #[inline(always)]
    pub fn reclaim_released(&mut self, graph: &mut impl Capabilities<'id>) {
        // Most synchronization points have no releases. Keep that test at the
        // call site instead of entering the graph-release loop on every turn.
        if !self.inbox.owners.released.borrow().is_empty() {
            self.drain_released(graph);
        }
    }

    #[inline(never)]
    fn drain_released(&mut self, graph: &mut impl Capabilities<'id>) {
        loop {
            // Release the queue borrow before touching graph ownership.
            let index = self.inbox.owners.released.borrow_mut().pop_front();
            let Some(index) = index else { break };
            let key = self.entries[index]
                .as_ref()
                .expect("unreclaimed endpoint")
                .client
                .key();
            self.unlink_binding(key, index);
            self.metadata_work
                .remove(&mut self.entries, index, |entry| {
                    &mut entry.as_mut().unwrap().metadata_work
                });
            let entry = self.entries[index].take().expect("unreclaimed endpoint");
            self.free.push(index);
            debug_assert_eq!(entry.endpoint.strong_count(), 0);
            self.unsettled -= usize::from(entry.pending);
            if let Some(subscription) = entry.route_subscription {
                graph.unsubscribe(subscription);
            }
            if entry.metadata_watch.is_some() {
                self.inbox.owners.metadata.remove(index);
            }
            graph.release(entry.client);
        }
    }

    /// Project immutable metadata at admission and after route changes, before
    /// publishing resolution. Mutable metadata must use an explicit source via
    /// `collect_with_sources`; changing this callback does not rescan live hooks.
    pub fn collect_with_metadata<G: Capabilities<'id>>(
        &mut self,
        graph: &mut G,
        mut metadata: impl FnMut(&G::Owner) -> EndpointMetadata,
    ) {
        self.collect_with_optional_metadata(graph, |owner| Some(metadata(owner)));
    }

    #[inline(always)]
    fn collect_with_optional_metadata<G: Capabilities<'id>>(
        &mut self,
        graph: &mut G,
        metadata: impl FnMut(&G::Owner) -> Option<EndpointMetadata>,
    ) {
        self.synchronize(graph, metadata, |_| None, |_| None, true, false);
    }

    fn unlink_binding(&mut self, key: capntproto_rpc_engine::clients::Key<'id>, index: usize) {
        let next = self.entries[index].as_ref().unwrap().same_key_next;
        let head = self.bindings[&key];
        if head == index {
            if let Some(next) = next {
                self.bindings.insert(key, next);
            } else {
                self.bindings.remove(&key);
            }
        } else {
            let mut previous = head;
            loop {
                let entry = self.entries[previous].as_mut().unwrap();
                if entry.same_key_next == Some(index) {
                    entry.same_key_next = next;
                    break;
                }
                previous = entry.same_key_next.expect("indexed alias");
            }
        }
    }

    fn queue_metadata(&mut self, index: usize) {
        self.metadata_work.push(&mut self.entries, index, |entry| {
            &mut entry.as_mut().unwrap().metadata_work
        });
    }
    /// Subscribe to mutable metadata. Sources publish late FD/diagnostic changes
    /// directly; stable endpoints and unrelated graph owners are never revisited.
    pub fn collect_with_sources<G: Capabilities<'id>>(
        &mut self,
        graph: &mut G,
        metadata: impl FnMut(&G::Owner) -> EndpointMetadata,
        source: impl FnMut(&G::Owner) -> Option<Rc<MetadataSource>>,
        changes: impl FnMut(&G::Owner) -> Option<Rc<MetadataChanges>>,
    ) {
        let mut metadata = metadata;
        self.synchronize(
            graph,
            |owner| Some(metadata(owner)),
            source,
            changes,
            true,
            true,
        );
    }

    fn synchronize<G: Capabilities<'id>>(
        &mut self,
        graph: &mut G,
        mut metadata: impl FnMut(&G::Owner) -> Option<EndpointMetadata>,
        mut subscribe: impl FnMut(&G::Owner) -> Option<Rc<MetadataSource>>,
        mut changes: impl FnMut(&G::Owner) -> Option<Rc<MetadataChanges>>,
        project_metadata: bool,
        bind_sources: bool,
    ) {
        self.inbox.owners.register();
        self.reclaim_released(graph);
        if bind_sources && !self.sources_bound {
            // One-time activation of the source contract. Admission thereafter
            // queues each new owner itself; stable turns never walk this table.
            self.sources_bound = true;
            for index in 0..self.entries.len() {
                if self.entries[index].is_some() {
                    self.queue_metadata(index);
                }
            }
        }
        while let Some(event) = self
            .listener
            .as_ref()
            .and_then(|listener| graph.next_event(listener))
        {
            let index = event.owner;
            if !self
                .entries
                .get(index)
                .and_then(Option::as_ref)
                .is_some_and(|entry| {
                    entry
                        .route_subscription
                        .as_ref()
                        .is_some_and(|s| s.key() == event.subscription)
                })
            {
                continue;
            }
            self.queue_metadata(index);
            if !project_metadata || (self.sources_bound && !bind_sources) {
                self.synchronize_one(
                    index,
                    graph,
                    &mut metadata,
                    &mut subscribe,
                    &mut changes,
                    project_metadata,
                    false,
                );
            }
        }
        if project_metadata && (!self.sources_bound || bind_sources) {
            while let Some(ready) = self.inbox.owners.metadata.pop() {
                let index = ready.index();
                let Some(entry) = self.entries.get_mut(index).and_then(Option::as_mut) else {
                    continue;
                };
                if let Some((watch, wake)) = &mut entry.metadata_watch {
                    if watch
                        .poll_changed(&mut Context::from_waker(wake))
                        .is_ready()
                    {
                        self.queue_metadata(index);
                    }
                }
            }
            while let Some(index) = self.metadata_work.pop(&mut self.entries, |entry| {
                &mut entry.as_mut().unwrap().metadata_work
            }) {
                self.synchronize_one(
                    index,
                    graph,
                    &mut metadata,
                    &mut subscribe,
                    &mut changes,
                    true,
                    bind_sources,
                );
            }
        }
        self.reclaim_released(graph);
    }
    #[allow(clippy::too_many_arguments)]
    fn synchronize_one<G: Capabilities<'id>>(
        &mut self,
        index: usize,
        graph: &mut G,
        metadata: &mut impl FnMut(&G::Owner) -> Option<EndpointMetadata>,
        subscribe: &mut impl FnMut(&G::Owner) -> Option<Rc<MetadataSource>>,
        changes: &mut impl FnMut(&G::Owner) -> Option<Rc<MetadataChanges>>,
        project_metadata: bool,
        bind_sources: bool,
    ) {
        let entry = self.entries[index].as_mut().expect("notified endpoint");
        let Some(endpoint) = entry.endpoint.upgrade() else {
            return;
        };
        let old_key = entry.client.key();
        let destination = graph.inspect(&entry.client);
        endpoint.remote.set(matches!(
            destination,
            Ok(capntproto_rpc_engine::clients::Destination::Endpoint {
                location: capntproto_rpc_engine::clients::Location::Remote(_),
                ..
            })
        ));
        if project_metadata {
            if let Ok(capntproto_rpc_engine::clients::Destination::Endpoint { owner, .. }) =
                destination
            {
                if bind_sources {
                    let changed = changes(owner);
                    let same = match (&entry.metadata_watch, &changed) {
                        (Some((watch, _)), Some(source)) => Rc::ptr_eq(watch.source(), source),
                        (None, None) => true,
                        _ => false,
                    };
                    if !same {
                        if entry.metadata_watch.take().is_some() {
                            self.inbox.owners.metadata.remove(index);
                        }
                        if let Some(source) = changed {
                            // This watch is polled below; only a subsequent notification
                            // should enqueue it. A pre-queued slot suppresses that wake.
                            let wake = self.inbox.owners.metadata.insert(index, false);
                            let mut watch = source.subscribe();
                            let _ = watch.poll_changed(&mut Context::from_waker(&wake));
                            entry.metadata_watch = Some((watch, wake));
                        }
                    }
                    let source = subscribe(owner);
                    let same = match (&entry.metadata, &source) {
                        (Some(old), Some(new)) => Rc::ptr_eq(old, new),
                        (None, None) => true,
                        _ => false,
                    };
                    if !same {
                        endpoint.next_metadata_generation();
                        if let Some(source) = &source {
                            source.subscribe(&endpoint);
                        }
                        entry.metadata = source;
                    }
                }
                if let Some(value) = entry
                    .metadata
                    .as_ref()
                    .and_then(|source| source.snapshot())
                    .or_else(|| metadata(owner))
                {
                    endpoint.apply_metadata(value);
                }
            }
        }
        if entry.pending {
            Self::settle_endpoint(graph, entry, &endpoint, &mut self.unsettled);
        }
        if let Some(subscription) = &entry.route_subscription {
            match graph
                .pending_resolution(&entry.client)
                .expect("retained endpoint")
            {
                Some(key) => graph
                    .rebind_subscription(subscription, key)
                    .expect("live route subscription"),
                None => graph.unsubscribe(entry.route_subscription.take().unwrap()),
            }
        }
        let new_key = entry.client.key();
        if new_key != old_key {
            self.unlink_binding(old_key, index);
            let next = self.bindings.insert(new_key, index);
            self.entries[index].as_mut().unwrap().same_key_next = next;
        }
    }

    // Resolve only pending endpoints. Settled import metadata arrives through
    // its source subscription without revisiting the graph.
    #[inline(never)]
    fn settle_endpoint<G: Capabilities<'id>>(
        graph: &mut G,
        entry: &mut Entry<'id>,
        endpoint: &Endpoint,
        unsettled: &mut usize,
    ) {
        // Export inspection does not accept a call or mark the promise used.
        // A reflected resolution waits for its original-route loopback.
        if let Ok(true) = graph.is_settled(&entry.client) {
            // A settled terminal endpoint is immutable. Retain it directly so
            // completed promise chains do not remain pinned by public hooks.
            if let Ok((client, pending)) = graph.snapshot_for_export(&entry.client) {
                if pending {
                    graph.release(client);
                } else {
                    graph.release(core::mem::replace(&mut entry.client, client));
                }
            }
            let result = match graph.inspect(&entry.client) {
                Ok(capntproto_rpc_engine::clients::Destination::Endpoint {
                    location: capntproto_rpc_engine::clients::Location::Broken,
                    ..
                }) => Err(endpoint.failure.borrow().clone().unwrap_or_else(|| {
                    Error::failed("capability resolved to a broken endpoint".into())
                })),
                _ => Ok(()),
            };
            *endpoint.failure.borrow_mut() = result.as_ref().err().cloned();
            endpoint.settled.set(true);
            entry.pending = false;
            *unsettled -= 1;
            // Detach observer state before notification can invoke a waker.
            let resolution = endpoint.resolution.borrow_mut().take();
            if let Some(resolution) = resolution {
                let _ = resolution.sender.send(result);
            }
        }
    }

    pub(super) fn ownership_ready(&self, graph: &impl Capabilities<'id>) -> bool {
        self.inbox.owners.register();
        !self.sources_bound
            || !self.inbox.owners.released.borrow().is_empty()
            || !self.metadata_work.is_empty()
            || self
                .listener
                .as_ref()
                .is_some_and(|listener| graph.has_events(listener))
            || !self.inbox.owners.metadata.is_empty()
    }

    /// Poll one owned call. Keep it on downstream capacity rejection before
    /// polling another call, so admission preserves the original order.
    /// The bridge has one consumer; the latest poll registers its wake.
    pub(super) fn register_next(&self, waker: &Waker) {
        // Standalone scoped users retain the combined polling contract. Hosted
        // connections bind ownership to its own producer once, independently of
        // application call futures and their wake registrations.
        if self.inbox.owners.wake.borrow().is_none() {
            self.inbox.owners.metadata.register(waker);
        }
        if self
            .inbox
            .wake
            .borrow()
            .as_ref()
            .is_some_and(|old| old.will_wake(waker))
        {
            return;
        }
        // Clone may call user code; do so before observing/registering state.
        let wake = waker.clone();
        let old = self.inbox.wake.replace(Some(wake));
        drop(old);
    }
    pub fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<Option<Call>> {
        self.poll_owned_next(cx).map(|call| call.map(|call| *call))
    }
    fn poll_owned_next(&mut self, cx: &mut Context<'_>) -> Poll<Option<Box<Call>>> {
        self.register_next(cx.waker());
        // Dropping canceled application owners can enqueue more work. Bound
        // this poll to the observed queue, then schedule another turn if needed.
        let budget = self.inbox.queue.borrow().len();
        for _ in 0..budget {
            let call = self.inbox.queue.borrow_mut().pop_front();
            match call {
                Some(call) if call.is_canceled() => drop(call),
                Some(call) => return Poll::Ready(Some(call)),
                None => break,
            }
        }
        if self.inbox.closed.get() {
            Poll::Ready(None)
        } else {
            let more = !self.inbox.queue.borrow().is_empty();
            if more {
                self.inbox.notify();
            }
            Poll::Pending
        }
    }

    fn has_pending_projection(&self, hook: &dyn ClientHook) -> bool {
        let hook = wire::direct_response_hook(hook)
            .or_else(|| hook.as_any().and_then(|value| value.downcast_ref::<Hook>()));
        hook.is_some_and(|hook| {
            hook.0.inbox.as_ptr() == Rc::as_ptr(&self.inbox) && hook.0.needs_binding()
        })
    }
    fn projection_pending(&self, hook: &dyn ClientHook, cx: &mut Context<'_>) -> bool {
        let hook = wire::direct_response_hook(hook)
            .or_else(|| hook.as_any().and_then(|value| value.downcast_ref::<Hook>()));
        hook.is_some_and(|hook| {
            hook.0.inbox.as_ptr() == Rc::as_ptr(&self.inbox) && hook.0.binding_pending(cx)
        })
    }
    fn bind_projection(
        &mut self,
        graph: &mut impl Capabilities<'id>,
        client: Client<'id>,
        endpoint: Rc<Endpoint>,
    ) -> capnp::Result<Box<dyn ClientHook>> {
        let pending = match graph.is_settled(&client) {
            Ok(settled) => !settled,
            Err(error) => {
                graph.release(client);
                return Err(Error::failed(error.to_string()));
            }
        };
        self.bind_retained(graph, client, pending, false, Some(endpoint))
            .map(|client| client.hook)
    }
    /// Recover the scoped authority for this call. The private Rc identity is
    /// checked as well as the slot: another bridge or a recycled slot cannot
    /// retarget a call. No branded capability is stored in the owned hook.
    pub fn target(&self, call: &Call) -> capnp::Result<&Client<'id>> {
        self.lookup(&call.endpoint)
    }
    fn lookup(&self, endpoint: &Rc<Endpoint>) -> capnp::Result<&Client<'id>> {
        if endpoint.inbox.as_ptr() != Rc::as_ptr(&self.inbox) {
            return Err(Error::disconnected("call belongs to another scope".into()));
        }
        if endpoint.index.get().is_none() {
            // Local early publication can forward through another private
            // projection. Keep this rare path iterative and reject cycles;
            // ordinary bound endpoints perform no allocation or traversal.
            let mut seen = vec![Rc::as_ptr(endpoint)];
            let mut next = endpoint.introduced.borrow().clone();
            while let Some(forwarded) = next {
                let hook = wire::direct_response_hook(&*forwarded).or_else(|| {
                    forwarded
                        .as_any()
                        .and_then(|value| value.downcast_ref::<Hook>())
                });
                let Some(hook) = hook else {
                    break;
                };
                if hook.0.inbox.as_ptr() != Rc::as_ptr(&self.inbox) {
                    break;
                }
                if hook.0.index.get().is_some() {
                    return self.lookup(&hook.0);
                }
                let identity = Rc::as_ptr(&hook.0);
                if seen.contains(&identity) {
                    return Err(Error::failed("cyclic projection forwarding".into()));
                }
                seen.push(identity);
                next = hook.0.introduced.borrow().clone();
            }
        }
        let same_inbox = endpoint.inbox.as_ptr() == Rc::as_ptr(&self.inbox);
        endpoint
            .index
            .get()
            .and_then(|index| self.entries.get(index))
            .and_then(Option::as_ref)
            .filter(|entry| same_inbox && entry.endpoint.as_ptr() == Rc::as_ptr(endpoint))
            .map(|entry| &entry.client)
            .ok_or_else(|| Error::disconnected("call belongs to another or closed scope".into()))
    }

    /// Recognize one of this bridge's parameter/result hooks and retain its
    /// graph authority. Foreign and opaque hooks return None, so the caller can
    /// export them through its ordinary policy-preserving local-server path.
    /// Never unwrap a membrane or trust its public pointer/brand values.
    pub fn try_retain(
        &self,
        graph: &mut impl Capabilities<'id>,
        hook: &dyn ClientHook,
    ) -> capnp::Result<Option<Client<'id>>> {
        let Some(hook) = wire::direct_response_hook(hook)
            .or_else(|| hook.as_any().and_then(|value| value.downcast_ref::<Hook>()))
        else {
            return Ok(None);
        };
        if !hook.0.inbox.ptr_eq(&Rc::downgrade(&self.inbox)) {
            return Ok(None);
        }
        // An unbound foreign/local early projection uses the ordinary adapter.
        // Hosted call/result admission waits for its direct binding separately.
        if hook.0.index.get().is_none() {
            return Ok(None);
        }
        let client = self.lookup(&hook.0)?;
        graph
            .retain_key(client.key())
            .map(Some)
            .map_err(|e| Error::failed(e.to_string()))
    }

    /// Admit the oldest hosted call to the shared engine's ordered dispatcher.
    /// On capacity rejection it stays at the front of the inbox, ahead of newer
    /// calls. The runtime owns cancellation and target retention after success.
    pub fn poll_dispatch<'nodes, T: 'nodes, P: Copy + Eq + 'nodes, G>(
        &mut self,
        runtime: &mut dispatch::Runtime<'id, 'nodes, '_, T, P, Call, G>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<dispatch::TaskKey<'id>>, dispatch::DispatchError>>
    where
        G: dispatch::GraphStorage<'id, 'nodes, T, P>,
    {
        let call = match self.poll_next(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(None) => return Poll::Ready(Ok(None)),
            Poll::Ready(Some(call)) => call,
        };
        let target = self
            .lookup(&call.endpoint)
            .expect("queued call has its retained route");
        let mut call = Some(call);
        let outcome = runtime
            .prepare_call(target, &mut call)
            .map(|pending| pending.accept());
        match outcome {
            Ok(key) => Poll::Ready(Ok(Some(key))),
            Err(error) => {
                self.inbox
                    .queue
                    .borrow_mut()
                    .push_front(Box::new(call.expect("rejected admission retains owner")));
                Poll::Ready(Err(error))
            }
        }
    }

    /// Disconnect queued and running replies, then release all retained graph
    /// refs. Existing owned clients become broken without extending the scope.
    pub(crate) fn disconnect(&self, error: Error) {
        let mut reason = error;
        reason.kind = capnp::ErrorKind::Disconnected;
        // Retained endpoints outlive this registry. Record the complete reason
        // before callbacks run, rather than synthesizing a generic Drop error.
        for entry in self.entries.iter().flatten() {
            if let Some(endpoint) = entry.endpoint.upgrade() {
                endpoint
                    .failure
                    .borrow_mut()
                    .get_or_insert_with(|| reason.clone());
            }
        }
        self.inbox.close(reason);
    }
    pub fn close(mut self, graph: &mut impl Capabilities<'id>, error: Error) {
        self.disconnect(error);
        if let Some(listener) = self.listener.take() {
            graph.close_listener(listener);
        }
        for entry in self.entries.drain(..).flatten() {
            graph.release(entry.client);
        }
    }
}
impl Drop for ClientHooks<'_> {
    fn drop(&mut self) {
        self.inbox.close(Error::disconnected(
            "scoped ClientHook driver dropped".into(),
        ));
    }
}

/// Owned hosted call awaiting engine admission/execution. The result hook can
/// publish a pipeline before the eventual result, and retains capabilities in
/// both directions. Dropping an unfinished call fails its completion.
#[must_use = "complete the call, or drop it to report cancellation"]
pub struct Call {
    third_party: Option<Rc<capnp::message::Builder<capnp::message::HeapAllocator>>>,
    credit: Option<Rc<admission::Credit>>,
    endpoint: Rc<Endpoint>,
    interface: u64,
    method: u16,
    hints: CallHints,
    params: CallParams,
    results: response::Results,
    completion: CallCompletion,
    shutdown: Shutdown,
    // Last: release admission after destroying callback-bearing call owners.
    _permit: Permit,
}
enum CallParams {
    Request(crate::local::Params),
    OwnedRequest(Box<crate::local::Request>),
    Hook(Box<dyn ParamsHook>),
}
impl CallParams {
    fn owned_request(mut request: Box<crate::local::Request>) -> Self {
        // This private request keeps its original parameter allocation through
        // forwarding. Retain its concrete type for admission and wire encoding.
        request.freeze_large_params();
        Self::OwnedRequest(request)
    }
    fn hook(params: Box<dyn ParamsHook>) -> Self {
        Self::Hook(params)
    }
    fn capability_table(&self) -> Option<&[Option<Box<dyn ClientHook>>]> {
        match self {
            Self::Request(request) => Some(request.capability_table()),
            Self::OwnedRequest(request) => Some(request.capability_table()),
            Self::Hook(hook) => hook.as_any().and_then(|params| {
                params
                    .downcast_ref::<crate::local::Request>()
                    .map(crate::local::Request::capability_table)
                    .or_else(|| {
                        params
                            .downcast_ref::<crate::local::Params>()
                            .map(crate::local::Params::capability_table)
                    })
            }),
        }
    }
    fn single_capability_table(&self) -> Option<&[Option<Box<dyn ClientHook>>]> {
        match self {
            Self::Request(request) => request.single_capability_table(),
            Self::OwnedRequest(request) => request.single_capability_table(),
            Self::Hook(hook) => hook.as_any().and_then(|params| {
                params
                    .downcast_ref::<crate::local::Request>()
                    .and_then(crate::local::Request::single_capability_table)
                    .or_else(|| {
                        params
                            .downcast_ref::<crate::local::Params>()
                            .and_then(crate::local::Params::single_capability_table)
                    })
            }),
        }
    }

    #[inline(always)]
    fn prepared_call(
        &self,
    ) -> Option<&capntproto_rpc_wire::owned::connection::PreparedCall<capnp::message::HeapAllocator>>
    {
        match self {
            Self::Request(request) => request.prepared_call(),
            Self::OwnedRequest(request) => request.prepared_call(),
            Self::Hook(hook) => hook.as_any().and_then(|params| {
                params
                    .downcast_ref::<crate::local::Request>()
                    .and_then(crate::local::Request::prepared_call)
                    .or_else(|| {
                        params
                            .downcast_ref::<crate::local::Params>()
                            .and_then(crate::local::Params::prepared_call)
                    })
            }),
        }
    }
    fn into_hook(self) -> Box<dyn ParamsHook> {
        match self {
            Self::Request(request) => Box::new(request),
            Self::OwnedRequest(request) => request,
            Self::Hook(hook) => hook,
        }
    }
    fn request_mut(&mut self) -> &mut crate::local::Params {
        match self {
            Self::Request(request) => request,
            _ => unreachable!("unsent request"),
        }
    }
}
impl std::ops::Deref for CallParams {
    type Target = dyn ParamsHook;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Request(request) => request,
            Self::OwnedRequest(request) => &**request,
            Self::Hook(hook) => &**hook,
        }
    }
}
impl capnp::private::capability::RequestHook for Call {
    fn get(&mut self) -> any_pointer::Builder<'_> {
        self.params.request_mut().builder()
    }
    fn get_brand(&self) -> usize {
        0
    }
    fn set_hints(&mut self, hints: CallHints) {
        self.hints = hints;
    }
    fn third_party_tail_target(&self) -> Option<Box<dyn std::any::Any>> {
        tail_target(&Hook(self.endpoint.clone(), self.credit.clone()))
    }
    fn set_third_party_tail_target(
        &mut self,
        contact: any_pointer::Reader<'_>,
    ) -> capnp::Result<()> {
        if self.third_party.is_some() || self.third_party_tail_target().is_none() {
            return Err(Error::failed("ineligible tail transfer".into()));
        }
        contact.validate_capability_indices(0)?;
        let mut body = capnp::message::Builder::new_default();
        body.set_root(contact)?;
        self.third_party = Some(Rc::new(body));
        Ok(())
    }
    fn send(self: Box<Self>) -> capability::RemotePromise<any_pointer::Owned> {
        if !self.hints.only_promise_pipeline && self.third_party.is_none() && {
            let unintroduced = self.endpoint.introduced.borrow().is_none()
                && self.endpoint.introduction.borrow().is_none();
            unintroduced
                || Hook(self.endpoint.clone(), self.credit.clone()).direct_response_available()
        } {
            return if self.hints.no_promise_pipelining {
                response::request_prepared(self)
            } else {
                response::request_pipelined_prepared(self)
            };
        }
        self.into_request().send()
    }
    fn send_for_pipeline(self: Box<Self>) -> any_pointer::Pipeline {
        self.into_request().send_for_pipeline()
    }
    fn send_streaming(self: Box<Self>) -> Promise<(), Error> {
        self.into_request().send_streaming()
    }
    fn tail_send(
        self: Box<Self>,
    ) -> Option<(
        u32,
        Promise<(), Error>,
        Box<dyn capnp::private::capability::PipelineHook>,
    )> {
        self.into_request().tail_send()
    }
}
impl Call {
    #[inline(always)]
    fn enqueue_owned(
        mut self: Box<Self>,
        inbox: Option<Rc<Inbox>>,
    ) -> Result<Rc<Inbox>, Box<Error>> {
        if let Some(error) = self.endpoint.failure.borrow().clone() {
            return Err(Box::new(error));
        }
        let Some(inbox) = inbox.filter(|inbox| !inbox.closed.get()) else {
            return Err(Box::new(Error::disconnected(
                "scoped ClientHook driver closed".into(),
            )));
        };
        if inbox.live_calls.get() == inbox.maximum_calls {
            return Err(Box::new(Error::overloaded(
                "scoped call capacity exhausted".into(),
            )));
        }
        let credit = self
            .credit
            .take()
            .or_else(|| inbox.admission.clone().map(admission::Credit::new));
        if self.endpoint.remote.get() {
            if let Some(credit) = &credit {
                credit.ensure()?;
            }
        }
        if inbox.queue.borrow_mut().try_reserve(1).is_err() {
            return Err(Box::new(Error::overloaded(
                "scoped call queue exhausted".into(),
            )));
        }
        // Reserve before invoking a custom results hook, which may reenter or
        // close this connection. The guard rolls back even during unwinding.
        inbox.live_calls.set(inbox.live_calls.get() + 1);
        let permit = Permit(Rc::downgrade(&inbox));
        if let Some(credit) = &credit {
            self.results.retain_context(credit.clone());
        }
        if inbox.closed.get() {
            return Err(Box::new(Error::disconnected(
                "scoped ClientHook driver closed".into(),
            )));
        }
        self.credit = credit;
        // Requests built by this hook already retain this exact shutdown state.
        // General incoming ParamsHook calls acquire it only at admission.
        if !matches!(self.params, CallParams::Request(_)) {
            self.shutdown = inbox.shutdown.clone();
        }
        self._permit = permit;
        if let CallParams::Request(params) = &mut self.params {
            params.freeze_large_params();
        }
        inbox.queue.borrow_mut().push_back(self);
        inbox.notify_enqueue();
        Ok(inbox)
    }

    fn into_request(self: Box<Self>) -> Box<crate::local::Request> {
        let CallParams::Request(request) = self.params else {
            unreachable!("unsent request")
        };
        let context = self
            .credit
            .clone()
            .map(|credit| credit as Rc<dyn std::any::Any>);
        Box::new(request.into_request(
            self.interface,
            self.method,
            self.hints,
            Hook(self.endpoint, self.credit),
            context,
            self.third_party,
        ))
    }
}
enum CallCompletion {
    Unsent,
    Promise(completion::Delivery<'static, capnp::Result<()>>),
    Response(response::Completion),
}
impl CallCompletion {
    fn is_observed(&self) -> bool {
        match self {
            Self::Unsent => unreachable!("unadmitted request"),
            Self::Promise(delivery) => delivery.is_observed(),
            Self::Response(completion) => completion.is_observed(),
        }
    }
    fn poll_observed(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        match self {
            Self::Unsent => unreachable!("unadmitted request"),
            Self::Promise(delivery) => delivery.poll_observed(cx),
            Self::Response(completion) => completion.poll_observed(cx),
        }
    }
    fn complete(&mut self, result: capnp::Result<()>) {
        match self {
            Self::Unsent => unreachable!("unadmitted request"),
            Self::Promise(delivery) => {
                let _ = delivery.try_send(result);
            }
            Self::Response(completion) => completion.complete(result),
        }
    }
}
impl Call {
    pub fn interface_id(&self) -> u64 {
        self.interface
    }
    pub fn method_id(&self) -> u16 {
        self.method
    }
    pub fn hints(&self) -> CallHints {
        self.hints
    }
    pub fn params(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        self.params.get()
    }
    pub fn results(&mut self) -> &mut dyn ResultsHook {
        self.results.hook()
    }
    /// Loss of reply/pipeline interest; the service's cancellation policy still
    /// determines whether an already running operation can be stopped.
    pub fn is_canceled(&self) -> bool {
        self.shutdown.is_closed() || self.completion.is_observed()
    }
    pub fn poll_canceled(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if Pin::new(&mut self.shutdown).poll(cx).is_ready() {
            return Poll::Ready(());
        }
        self.completion.poll_observed(cx)
    }
    pub fn complete(mut self, result: capnp::Result<()>) {
        self.completion.complete(result);
    }
    /// Hosted queues already own a box. Complete and drop its fields in place,
    /// preserving field destruction order without moving the full call to stack.
    #[allow(clippy::boxed_local)] // Preserve the existing allocation through field destruction.
    fn complete_boxed(mut self: Box<Self>, result: capnp::Result<()>) {
        self.completion.complete(result);
    }

    /// Execute an admitted call through an ordinary generated server hook.
    /// Params/results (including early pipeline and cancellation policy hooks)
    /// move intact; the adapter does not rebuild the application's context.
    pub fn forward(self, target: &dyn ClientHook) -> Promise<(), Error> {
        let Self {
            credit,
            third_party: _,
            endpoint,
            interface,
            method,
            hints,
            params,
            results,
            mut completion,
            shutdown: _,
            _permit,
        } = self;
        let operation = target.call_with_hints(
            interface,
            method,
            params.into_hook(),
            results.into_hook(),
            hints,
        );
        Promise::from_future(async move {
            let result = operation.await;
            completion.complete(result.clone());
            drop((endpoint, _permit, credit));
            result
        })
    }
}
struct Permit(Weak<Inbox>);
impl Drop for Permit {
    fn drop(&mut self) {
        if let Some(inbox) = self.0.upgrade() {
            inbox.live_calls.set(inbox.live_calls.get() - 1);
            // Admission refuses an exhausted quota synchronously; it has no
            // pending waiter to resume. The call's result/cancellation and the
            // last endpoint reference each notify their own actual observers.
        }
    }
}

pub(crate) struct Hook(Rc<Endpoint>, Option<Rc<admission::Credit>>);
impl Hook {
    fn call_to(
        &self,
        interface: u64,
        method: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
        hints: CallHints,
        third_party: Option<Rc<capnp::message::Builder<capnp::message::HeapAllocator>>>,
    ) -> Promise<(), Error> {
        if let Some(error) = self.0.failure.borrow().clone() {
            return Promise::err(error);
        }
        if third_party.is_none() {
            if let Some(client) = self.introduced() {
                return client.call_with_hints(interface, method, params, results, hints);
            }
        }
        let (sender, receiver) = shared::channel();
        let sender: completion::Sender<'static, capnp::Result<()>> = sender.into();
        match self.enqueue(
            interface,
            method,
            CallParams::hook(params),
            response::Results::Hook(results),
            hints,
            third_party,
            CallCompletion::Promise(sender.track()),
        ) {
            Ok(inbox) => Promise::from_future(Reply {
                receiver: Some(receiver),
                shutdown: inbox.shutdown.clone(),
                inbox: Rc::downgrade(&inbox),
            }),
            Err(error) => Promise::err(*error),
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    fn enqueue(
        &self,
        interface: u64,
        method: u16,
        params: CallParams,
        results: response::Results,
        hints: CallHints,
        third_party: Option<Rc<capnp::message::Builder<capnp::message::HeapAllocator>>>,
        completion: CallCompletion,
    ) -> Result<Rc<Inbox>, Box<Error>> {
        let call = Box::new(Call {
            credit: self.1.clone(),
            third_party,
            endpoint: self.0.clone(),
            interface,
            method,
            hints,
            params,
            results,
            completion,
            shutdown: Shutdown::new(),
            _permit: Permit(Weak::new()),
        });
        call.enqueue_owned(self.0.inbox.upgrade())
    }

    // A projection can bind to another unresolved promise. Resolution observers
    // must see that first step; only when_resolved follows the remaining chain.
    fn wait_for_resolution(&self, follow: bool) -> Promise<(), Error> {
        let Some(inbox) = self.0.inbox.upgrade() else {
            return Promise::err(Error::disconnected(
                "scoped ClientHook driver dropped".into(),
            ));
        };
        let endpoint = self.0.clone();
        let shutdown = inbox.shutdown.clone();
        let resolved = endpoint
            .resolution
            .borrow_mut()
            .get_or_insert_with(|| {
                let (sender, receiver) = shared::channel();
                Resolution {
                    sender,
                    receiver: receiver.shared(),
                }
            })
            .receiver
            .clone();
        Promise::from_future(async move {
            // The waiter retains its graph route. Bridge close/drop wakes it
            // through shutdown even though notification state is endpoint-owned.
            let result = match futures::future::select(shutdown, resolved).await {
                futures::future::Either::Right((result, _)) => result
                    .map_err(crate::canceled_to_error)
                    .and_then(core::convert::identity),
                futures::future::Either::Left((reason, _)) => Err(reason.unwrap_or_else(|_| {
                    Error::disconnected("scoped ClientHook driver dropped".into())
                })),
            };
            result?;
            let forwarded = endpoint.introduced.borrow().clone();
            if follow {
                if let Some(forwarded) = forwarded {
                    forwarded.when_resolved().await?;
                }
            }
            Ok(())
        })
    }

    pub(crate) fn direct_response_available(&self) -> bool {
        self.introduced().is_none()
    }
    #[inline(always)]
    fn introduced(&self) -> Option<Box<dyn ClientHook>> {
        if let Some(client) = self.0.introduced.borrow().as_ref() {
            return Some(client.add_ref());
        }
        let contact = self.0.introduction.borrow().clone()?;
        let result = (|| {
            let inbox = self
                .0
                .inbox
                .upgrade()
                .ok_or_else(|| Error::disconnected("introduction driver closed".into()))?;
            inbox
                .joins
                .as_ref()
                .ok_or_else(|| Error::unimplemented("introduction network unavailable".into()))?
                .introduced(self.add_ref(), contact.get_root_as_reader()?)
        })();
        let client: Rc<dyn ClientHook> = Rc::from(result.unwrap_or_else(crate::broken::new_cap));
        let result = client.add_ref();
        *self.0.introduced.borrow_mut() = Some(client);
        Some(result)
    }
}
impl ClientHook for Hook {
    fn metadata_changes(&self) -> Option<Rc<MetadataChanges>> {
        Some(
            self.0
                .metadata_changes
                .get_or_init(|| Rc::new(MetadataChanges::default()))
                .clone(),
        )
    }

    fn get_failure(&self) -> Option<Error> {
        // Inspect an already accepted route without triggering an introduction.
        let introduced = self.0.introduced.borrow().clone();
        if let Some(client) = introduced {
            return client.get_failure();
        }
        self.0
            .failure
            .borrow()
            .clone()
            .or_else(|| match self.0.inbox.upgrade() {
                Some(inbox) => inbox.shutdown.reason(),
                None => Some(Error::disconnected(
                    "scoped ClientHook driver dropped".into(),
                )),
            })
    }
    fn forward_join(
        &self,
        part: any_pointer::Reader<'_>,
    ) -> Option<crate::multiparty::PartResponse> {
        if let Some(client) = self.introduced() {
            return client.forward_join(part);
        }
        let inbox = self.0.inbox.upgrade()?;
        inbox
            .joins
            .as_ref()
            .map(|joins| joins.join(self.add_ref(), part))
    }
    #[cfg(unix)]
    fn get_fd(&self) -> Option<Rc<std::os::fd::OwnedFd>> {
        self.0.fd.borrow().clone()
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn debug_info(&self, info: &mut DebugInfo) {
        info.push("scopedEndpoint");
    }
    fn add_ref(&self) -> Box<dyn ClientHook> {
        Box::new(Self(self.0.clone(), self.1.clone()))
    }
    fn new_call(
        &self,
        interface: u64,
        method: u16,
        size: Option<capnp::MessageSize>,
    ) -> capability::Request<any_pointer::Owned, any_pointer::Owned> {
        self.new_call_with_hints(interface, method, size, CallHints::default())
    }
    fn new_call_with_hints(
        &self,
        interface: u64,
        method: u16,
        size: Option<capnp::MessageSize>,
        hints: CallHints,
    ) -> capability::Request<any_pointer::Owned, any_pointer::Owned> {
        if let Some(client) = self.introduced() {
            return client.new_call_with_hints(interface, method, size, hints);
        }
        let inbox = self.0.inbox.upgrade();
        let credit = inbox
            .as_ref()
            .and_then(|inbox| inbox.admission.clone())
            .map(admission::Credit::new);
        if self.0.remote.get() {
            if let Some(credit) = &credit {
                if let Err(error) = credit.ensure() {
                    return capability::Request::new(Box::new(crate::broken::Request::new(
                        error, size,
                    )));
                }
            }
        }
        let allocator = crate::local::result_allocator(size);
        let allocator = match inbox.as_ref().and_then(|inbox| inbox.message_pool.as_ref()) {
            Some(pool) => allocator.segment_pool(pool.clone()),
            None => allocator,
        };
        let template = self.0.remote.get().then(|| {
            inbox
                .as_ref()
                .map(|inbox| inbox.call_template(interface, method))
        });
        // Allocate before constructing the arena, allowing the compiler to write
        // the large request directly into its final box without a second owner.
        let call = Box::<Call>::new_uninit();
        let request = crate::local::Params::scoped(interface, method, allocator, template);
        // Keep initialization concrete before coercing to RequestHook. The
        // pinned coverage compiler otherwise infers an unsized Box::write.
        let call: Box<Call> = Box::write(
            call,
            Call {
                third_party: None,
                credit,
                endpoint: self.0.clone(),
                interface,
                method,
                hints,
                params: CallParams::Request(request),
                results: response::Results::Unsent,
                completion: CallCompletion::Unsent,
                shutdown: inbox
                    .as_ref()
                    .map_or_else(Shutdown::new, |inbox| inbox.shutdown.clone()),
                _permit: Permit(Weak::new()),
            },
        );
        capability::Request::new(call)
    }
    fn call(
        &self,
        interface: u64,
        method: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
    ) -> Promise<(), Error> {
        self.call_with_hints(interface, method, params, results, CallHints::default())
    }
    fn call_with_hints(
        &self,
        interface: u64,
        method: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
        hints: CallHints,
    ) -> Promise<(), Error> {
        self.call_to(interface, method, params, results, hints, None)
    }
    // Opaque local brand: the legacy network must not treat this as one of its
    // wire clients or infer Join/tail-call/FD authority from graph identity.
    fn get_brand(&self) -> usize {
        self.0
            .inbox
            .upgrade()
            .filter(|inbox| inbox.joins.is_some())
            .map_or(0, |inbox| Rc::as_ptr(&inbox) as usize)
    }
    fn get_ptr(&self) -> usize {
        Rc::as_ptr(&self.0) as usize
    }
    fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
        self.introduced()
            .map(|client| client.get_resolved().unwrap_or(client))
    }
    fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, Error>> {
        if let Some(client) = self.introduced() {
            return Some(
                client
                    .when_more_resolved()
                    .unwrap_or_else(|| Promise::ok(client)),
            );
        }
        if let Some(error) = self.0.failure.borrow().clone() {
            return Some(Promise::err(error));
        }
        if self.0.settled.get() {
            None
        } else {
            let endpoint = self.0.clone();
            let ready = self.wait_for_resolution(false);
            Some(Promise::from_future(async move {
                ready.await?;
                Ok(Box::new(Hook(endpoint, None)) as Box<dyn ClientHook>)
            }))
        }
    }
    fn when_resolved(&self) -> Promise<(), Error> {
        if let Some(client) = self.introduced() {
            return client.when_resolved();
        }
        if let Some(error) = self.0.failure.borrow().clone() {
            return Promise::err(error);
        }
        if self.0.settled.get() {
            return Promise::ok(());
        }
        self.wait_for_resolution(true)
    }
}

struct Reply {
    receiver: Option<shared::Receiver<capnp::Result<()>>>,
    shutdown: Shutdown,
    inbox: Weak<Inbox>,
}
impl Future for Reply {
    type Output = capnp::Result<()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Poll::Ready(reason) = Pin::new(&mut self.shutdown).poll(cx) {
            drop(self.receiver.take());
            return Poll::Ready(Err(reason.unwrap_or_else(|_| {
                Error::disconnected("scoped ClientHook driver dropped".into())
            })));
        }
        let result = Pin::new(
            self.receiver
                .as_mut()
                .expect("reply polled after completion"),
        )
        .poll(cx);
        if result.is_ready() {
            // Completion already notified its tracked observer. A later drop
            // cannot cancel this call or enable another coordinator scan.
            drop(self.receiver.take());
        }
        result.map(|result| {
            result
                .map_err(crate::canceled_to_error)
                .and_then(|result| result)
        })
    }
}
impl Drop for Reply {
    fn drop(&mut self) {
        // First close the receiver, then wake the dispatcher to prune queued
        // cancellation even when no IO or application operation is ready.
        if let Some(receiver) = self.receiver.take() {
            drop(receiver);
            if let Some(inbox) = self.inbox.upgrade() {
                inbox.notify();
            }
        }
    }
}

#[cfg(test)]
mod tests;

// Exact private hook types carry authority; public brands and wrappers do not.
pub(crate) fn tail_target(client: &dyn ClientHook) -> Option<Box<dyn std::any::Any>> {
    let hook = client.as_any()?.downcast_ref::<Hook>()?;
    // A settled remote endpoint cannot resolve into a different connection
    // after its authenticated destination metadata has been committed.
    if !hook.0.settled.get() || !hook.0.remote.get() || hook.0.introduction.borrow().is_some() {
        return None;
    }
    let handle = hook.0.inbox.upgrade()?.joins.clone()?;
    handle.tail_target()
}
pub(crate) fn call_third_party(
    client: &dyn ClientHook,
    interface: u64,
    method: u16,
    params: Box<dyn ParamsHook>,
    results: Box<dyn ResultsHook>,
    hints: CallHints,
    contact: Rc<capnp::message::Builder<capnp::message::HeapAllocator>>,
) -> Promise<(), Error> {
    let Some(hook) = client.as_any().and_then(|any| any.downcast_ref::<Hook>()) else {
        return Promise::err(Error::failed("foreign tail transfer hook".into()));
    };
    hook.call_to(interface, method, params, results, hints, Some(contact))
}
