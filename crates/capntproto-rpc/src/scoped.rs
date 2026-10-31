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
//! transfer and Join integrate through the bounded wire driver. Caller-supplied endpoint
//! metadata preserves attached resources and hosted diagnostics;
//! this bridge does not replace `RpcSystem` yet.
#![forbid(unsafe_code)]

use capnp::{
    any_pointer,
    capability::{self, CallHints, Promise},
    private::capability::{ClientHook, MetadataChanges, MetadataWatch},
    Error,
};
use capntproto_rpc_engine::{
    clients::{dispatch, Capabilities, Client},
    completion::shared,
};
use futures::{future::Shared, FutureExt};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::{Rc, Weak},
    task::{Context, Poll, Waker},
};

// Registry and endpoint identity stay here. The request owner and ClientHook
// adapter depend on that registry; neither owns an executor or wire progress.
mod admission;
mod bindings;
mod call;
pub use call::Call;
use call::{CallCompletion, CallKind, CallParams, CallPermit};
mod hook;
#[cfg(test)]
use hook::Reply;
pub(crate) use hook::{call_third_party, tail_target, Hook};
mod metadata;
pub use metadata::MetadataSource;
pub(crate) mod response;
mod shutdown;
mod streaming;
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
    request_cache: call::Cache,
    call_templates: [std::cell::OnceCell<Box<CallTemplate>>; 4],
    message_pool: Option<capnp::message::SegmentPool>,
    admission: Option<Rc<crate::admission::Admission>>,
    stream_factory: Option<Rc<streaming::Factory>>,
    joins: Option<wire::coordinator::multiparty::Handle>,
    native_tail: bool,
    queue: RefCell<VecDeque<Box<Call>>>,
    owners: Ownership,

    wake: RefCell<Option<Waker>>,
    enqueue_wake: RefCell<Option<Rc<wire::coordinator::EnqueueWake>>>,
    shutdown: Shutdown,
    closed: Cell<bool>,
    live_calls: Cell<usize>,
    maximum_calls: usize,
    maximum_clients: usize,
    direct_clients: Cell<usize>,
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
            // Combine both differences before branching; OR cannot hide a
            // mismatch in either the interface ID or the method ID.
            let mismatch = (entry.interface ^ interface) | u64::from(entry.method ^ method);
            if mismatch == 0 {
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
        self.request_cache.clear();
        self.notify();
    }
}
struct Endpoint {
    streaming: std::cell::OnceCell<Rc<streaming::Flow>>,
    introduction: RefCell<Option<Rc<capnp::message::Builder<capnp::message::HeapAllocator>>>>,
    introduced: RefCell<Option<Rc<dyn ClientHook>>>,
    remote: Cell<bool>,
    inbox: Weak<Inbox>,
    index: Cell<Option<usize>>,
    binding_wakes: RefCell<Vec<Waker>>,
    direct: Cell<Option<usize>>,
    direct_parent: RefCell<Weak<wire::PipelineState>>,
    settled: Cell<bool>,
    pending_calls: Cell<usize>,
    local_deferred: Cell<bool>,
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
            streaming: std::cell::OnceCell::new(),
            introduction: RefCell::default(),
            introduced: RefCell::default(),
            remote: Cell::new(true),
            inbox,
            index: Cell::new(None),
            binding_wakes: RefCell::default(),
            direct: Cell::new(None),
            direct_parent: RefCell::default(),
            settled: Cell::new(false),
            pending_calls: Cell::new(0),
            local_deferred: Cell::new(false),
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
    fn request_materialization(&self) {
        if self.direct.get().is_some() {
            let parent = self.direct_parent.borrow().upgrade();
            if let Some(parent) = parent {
                parent.request_materialization();
            }
        }
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
                let previous = self.introduced.replace(Some(Rc::from(hook)));
                drop(previous);
                let resolution = self.resolution.borrow_mut().take();
                if let Some(resolution) = resolution {
                    let _ = resolution.sender.send(Ok(()));
                }
            }
            Err(error) => {
                let previous = self.failure.replace(Some(error.clone()));
                drop(previous);
                let resolution = self.resolution.borrow_mut().take();
                if let Some(resolution) = resolution {
                    let _ = resolution.sender.send(Err(error));
                }
            }
        }
        if let Some(changes) = self.metadata_changes.get() {
            changes.notify();
        }
        self.notify_binding();
        if let Some(inbox) = self.inbox.upgrade() {
            inbox.notify_ownership();
        }
    }

    fn accept_local_resolution(&self, hook: Option<Rc<dyn ClientHook>>) -> bool {
        if self.introduced.borrow().is_some() {
            return false;
        }
        let Some(hook) = hook else {
            return false;
        };
        if hook
            .as_any()
            .and_then(|any| any.downcast_ref::<Hook>())
            .is_some_and(|hook| core::ptr::eq(&*hook.0, self))
        {
            return false;
        }
        *self.introduced.borrow_mut() = Some(hook);
        // Like legacy's queued client, forward older calls before exposing the
        // original client to resolution observers. Waiting for their replies
        // would deadlock methods which themselves await this resolution.
        let deferred = self.pending_calls.get() != 0;
        self.local_deferred.set(deferred);
        self.settled.set(!deferred);
        !deferred
    }

    fn call_dispatched(&self) {
        let remaining = self.pending_calls.get() - 1;
        self.pending_calls.set(remaining);
        if remaining == 0 && self.local_deferred.replace(false) {
            self.settled.set(true);
            let resolution = self.resolution.borrow_mut().take();
            if let Some(changes) = self.metadata_changes.get() {
                changes.notify();
            }
            if let Some(resolution) = resolution {
                let _ = resolution.sender.send(Ok(()));
            }
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
    pub(crate) fn set_native_tail(&mut self, enabled: bool) {
        Rc::get_mut(&mut self.inbox)
            .expect("unpublished hooks")
            .native_tail = enabled;
    }
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
                request_cache: call::Cache::new(maximum_calls),
                call_templates: std::array::from_fn(|_| std::cell::OnceCell::new()),
                message_pool: None,
                admission: None,
                stream_factory: None,
                joins: None,
                native_tail: false,
                queue: RefCell::new(VecDeque::new()),
                owners: Ownership::default(),
                wake: RefCell::new(None),
                enqueue_wake: RefCell::new(None),
                shutdown: shutdown::Receiver::new(),
                closed: Cell::new(false),
                live_calls: Cell::new(0),
                maximum_calls,
                maximum_clients,
                direct_clients: Cell::new(0),
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
    pub(crate) fn with_stream_factory(mut self, factory: Rc<streaming::Factory>) -> Self {
        Rc::get_mut(&mut self.inbox)
            .expect("unpublished hook inbox")
            .stream_factory = Some(factory);
        self
    }
    /// Reuse released application request/result segments. The supplied pool
    /// bounds cached storage independently of live messages; retained readers
    /// and builders keep exclusive ownership until their last owner drops.
    /// Configure this before exporting clients or attaching the coordinator.
    /// Panics if the hook inbox has already been shared.
    pub fn with_message_pool(mut self, pool: capnp::message::SegmentPool) -> Self {
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

    fn fence_projection(
        &mut self,
        graph: &mut impl Capabilities<'id>,
        endpoint: Rc<Endpoint>,
        adopted: Box<dyn ClientHook>,
        fence: &mut impl FnMut(Box<dyn ClientHook>, Box<dyn ClientHook>) -> Option<Box<dyn ClientHook>>,
    ) -> capnp::Result<()> {
        let (client, pending) = graph
            .snapshot_for_export(self.lookup(&endpoint)?)
            .map_err(|e| Error::failed(e.to_string()))?;
        if pending {
            graph.release(client);
            return Ok(());
        }
        // Freeze into a distinct endpoint: replacing the public projection must
        // never redirect the old Join share back through its own fence.
        let frozen = Endpoint::projection(Rc::downgrade(&self.inbox));
        let old = self
            .bind_retained(graph, client, false, true, Some(frozen))?
            .hook;
        let next = fence(old.add_ref(), adopted).unwrap_or(old);
        endpoint.forward_projection(Ok(next));
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
        if self.entries.len() - self.free.len() + self.inbox.direct_clients.get()
            >= self.maximum_clients
        {
            graph.release(client);
            return Err(Error::overloaded("scoped client capacity exhausted".into()));
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
                streaming: std::cell::OnceCell::new(),
                introduction: RefCell::default(),
                introduced: RefCell::default(),
                remote: Cell::new(remote),
                inbox: Rc::downgrade(&self.inbox),
                index: Cell::new(None),
                binding_wakes: RefCell::default(),
                direct: Cell::new(None),
                direct_parent: RefCell::default(),
                settled: Cell::new(!pending),
                pending_calls: Cell::new(0),
                local_deferred: Cell::new(false),
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
            same_key_next: None,
            route_subscription,
            metadata_work: capntproto_rpc_engine::work_queue::Link::EMPTY,
            metadata_watch: None,
        });
        self.link_binding(key, index);
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
        self.synchronize(graph, |_| None, |_| None, |_| None, |_| None, false, false);
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
        self.synchronize(graph, metadata, |_| None, |_| None, |_| None, true, false);
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
            |_| None,
            true,
            true,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn synchronize<G: Capabilities<'id>>(
        &mut self,
        graph: &mut G,
        mut metadata: impl FnMut(&G::Owner) -> Option<EndpointMetadata>,
        mut subscribe: impl FnMut(&G::Owner) -> Option<Rc<MetadataSource>>,
        mut changes: impl FnMut(&G::Owner) -> Option<Rc<MetadataChanges>>,
        mut local: impl FnMut(&G::Owner) -> Option<Rc<dyn ClientHook>>,
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
                    &mut local,
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
                    &mut local,
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
        local: &mut impl FnMut(&G::Owner) -> Option<Rc<dyn ClientHook>>,
        project_metadata: bool,
        bind_sources: bool,
    ) {
        let entry = self.entries[index].as_ref().expect("notified endpoint");
        let Some(endpoint) = entry.endpoint.upgrade() else {
            return;
        };
        let old_key = entry.client.key();
        let destination = graph.inspect(&entry.client);
        // A newly settled promise can reach an import already represented by
        // another public hook. Legacy redirects that promise to its canonical
        // import client rather than creating a second public identity.
        let canonical = if entry.pending {
            if let Ok(capntproto_rpc_engine::clients::Destination::Endpoint { key, .. }) =
                &destination
            {
                let mut matching = self.bindings.get(key).copied();
                let mut canonical = None;
                while let Some(other) = matching {
                    let candidate = self.entries[other].as_ref().expect("indexed endpoint");
                    if other != index {
                        if let Some(endpoint) = candidate.endpoint.upgrade() {
                            canonical = Some(endpoint);
                            break;
                        }
                    }
                    matching = candidate.same_key_next;
                }
                canonical
            } else {
                None
            }
        } else {
            None
        };
        let entry = self.entries[index].as_mut().expect("notified endpoint");
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
            Self::settle_endpoint(
                graph,
                entry,
                &endpoint,
                &mut self.unsettled,
                local,
                canonical,
            );
        } else if project_metadata && bind_sources && !endpoint.remote.get() {
            // A generic collect can observe settlement before the hosted
            // ownership refresh. That later refresh must still expose the
            // original local authority, without revisiting stable imports.
            if let Ok(capntproto_rpc_engine::clients::Destination::Endpoint { owner, .. }) =
                graph.inspect(&entry.client)
            {
                if endpoint.accept_local_resolution(local(owner)) {
                    if let Some(changes) = endpoint.metadata_changes.get() {
                        changes.notify();
                    }
                }
            }
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
            self.link_binding(new_key, index);
        }
    }

    fn link_binding(&mut self, key: capntproto_rpc_engine::clients::Key<'id>, index: usize) {
        if let Some(head) = self.bindings.get(&key).copied() {
            // Keep the first live endpoint canonical. Inserting later aliases
            // after it prevents a chain of promise-to-promise forwarding.
            let next = self.entries[head]
                .as_mut()
                .unwrap()
                .same_key_next
                .replace(index);
            self.entries[index].as_mut().unwrap().same_key_next = next;
        } else {
            self.bindings.insert(key, index);
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
        local: &mut impl FnMut(&G::Owner) -> Option<Rc<dyn ClientHook>>,
        canonical: Option<Rc<Endpoint>>,
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
            // Only the settled graph route grants authority to expose its
            // original local hook. Metadata and public hook brands cannot do so.
            // Install it before waking resolution observers, preserving opaque
            // membranes and allowing local authority to outlive this connection.
            let resolved = if result.is_ok() && endpoint.introduced.borrow().is_none() {
                match graph.inspect(&entry.client) {
                    Ok(capntproto_rpc_engine::clients::Destination::Endpoint { owner, .. }) => {
                        local(owner).or_else(|| {
                            canonical
                                .map(|endpoint| Rc::new(Hook(endpoint, None)) as Rc<dyn ClientHook>)
                        })
                    }
                    _ => None,
                }
            } else {
                None
            };
            let redirected = endpoint.accept_local_resolution(resolved);
            *endpoint.failure.borrow_mut() = result.as_ref().err().cloned();

            endpoint.settled.set(!endpoint.local_deferred.get());
            entry.pending = false;
            *unsettled -= 1;
            // Detach observer state before notification can invoke a waker.
            let resolution = if endpoint.local_deferred.get() {
                None
            } else {
                endpoint.resolution.borrow_mut().take()
            };
            if redirected {
                if let Some(changes) = endpoint.metadata_changes.get() {
                    changes.notify();
                }
            }
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
        let hook = wire::native_hook(hook);
        hook.is_some_and(|hook| {
            if hook.0.inbox.as_ptr() == Rc::as_ptr(&self.inbox) && hook.0.needs_binding() {
                hook.0.request_materialization();
                true
            } else {
                false
            }
        })
    }
    fn projection_pending(&self, hook: &dyn ClientHook, cx: &mut Context<'_>) -> bool {
        let hook = wire::native_hook(hook);
        hook.is_some_and(|hook| {
            if hook.0.inbox.as_ptr() == Rc::as_ptr(&self.inbox) {
                hook.0.request_materialization();
                hook.0.binding_pending(cx)
            } else {
                false
            }
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
        // A waiter may already exist on the unbound projection. Even a
        // terminal target must pass through settlement after metadata refresh
        // so that waiter receives its completion (including a broken error).
        let pending = pending || endpoint.resolution.borrow().is_some();
        self.bind_retained(graph, client, pending, false, Some(endpoint))
            .map(|client| client.hook)
    }
    /// Recover the scoped authority for this call. The private Rc identity is
    /// checked as well as the slot: another bridge or a recycled slot cannot
    /// retarget a call. No branded capability is stored in the owned hook.
    pub fn target(&self, call: &Call) -> capnp::Result<&Client<'id>> {
        self.lookup(call.endpoint())
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
                let hook = wire::native_hook(&*forwarded);
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

    /// Borrow an already-bound private hook from this bridge. The registry and
    /// caller keep its authority alive; no public foreign resolution is polled.
    pub(crate) fn try_borrow_native(
        &self,
        hook: &dyn ClientHook,
    ) -> capnp::Result<Option<&Client<'id>>> {
        match self.native_endpoint(hook) {
            Some(endpoint) => self.lookup(endpoint).map(Some),
            None => Ok(None),
        }
    }
    /// Eligibility only. The actual borrow must still validate the current
    /// registry entry against the private endpoint identity.
    pub(crate) fn can_borrow_native(&self, hook: &dyn ClientHook) -> bool {
        self.native_endpoint(hook).is_some()
    }
    fn native_endpoint<'a>(&self, hook: &'a dyn ClientHook) -> Option<&'a Rc<Endpoint>> {
        let hook = wire::native_hook(hook)?;
        (hook.0.inbox.as_ptr() == Rc::as_ptr(&self.inbox)
            && hook.0.index.get().is_some()
            && (hook.0.local_deferred.get() || hook.0.introduced.borrow().is_none()))
        .then_some(&hook.0)
    }

    /// Recognize one of this bridge's parameter/result hooks and retain its
    /// graph authority, following already-available public resolutions first.
    /// Unresolved foreign and opaque hooks return None, so the caller can
    /// export them through its ordinary policy-preserving local-server path.
    /// Never unwrap a membrane or trust its public pointer/brand values.
    pub fn try_retain(
        &self,
        graph: &mut impl Capabilities<'id>,
        hook: &dyn ClientHook,
    ) -> capnp::Result<Option<Client<'id>>> {
        let mut resolved = None::<Box<dyn ClientHook>>;
        let hook = loop {
            let current = resolved.as_deref().unwrap_or(hook);
            if let Some(scoped) = wire::native_hook(current) {
                // An accepted introduction/local handoff has a new authority.
                // Preserve an older route only while its ordering fence is shut.
                if scoped.0.local_deferred.get() || scoped.0.introduced.borrow().is_none() {
                    break scoped;
                }
            }
            // Legacy normalizes public resolutions before recognizing a
            // same-connection capability. Opaque wrappers keep returning None.
            let Some(next) = current.get_resolved() else {
                return Ok(None);
            };
            resolved = Some(next);
        };
        if hook.0.inbox.as_ptr() != Rc::as_ptr(&self.inbox) {
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
            .lookup(call.endpoint())
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

#[cfg(test)]
mod tests;
