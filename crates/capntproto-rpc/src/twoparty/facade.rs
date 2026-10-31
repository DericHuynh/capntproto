//! Executor-neutral conveniences for a single peer and a collection of peers.
mod borrowed;
mod driver;
pub use driver::RpcDriver;
use driver::Running;
mod notify;
use notify::WakeDriven;

use super::{OutputSnapshot, QueueSnapshot, VatId, VatNetwork};
use crate::{task_set, RpcSystem};
use capnp::{capability::Client, capability::Promise, message::ReaderOptions, Error};
use capntproto_rpc_engine::completion::shared as oneshot;
use futures::{future::Shared, AsyncRead, AsyncReadExt, AsyncWrite, FutureExt};
use std::{cell::RefCell, future::Future, pin::Pin, rc::Rc, task::Context, task::Poll};

/// Snapshot provider independent of the stream or network's lifetime. Providers
/// must retain only diagnostic state, not connections, messages or descriptors.
#[derive(Clone)]
pub struct QueueDiagnostics {
    queued: std::sync::Arc<dyn Fn() -> QueueSnapshot + Send + Sync>,
    output: Option<std::sync::Arc<dyn Fn() -> OutputSnapshot + Send + Sync>>,
}
impl QueueDiagnostics {
    pub fn new(snapshot: impl Fn() -> QueueSnapshot + Send + Sync + 'static) -> Self {
        Self {
            queued: std::sync::Arc::new(snapshot),
            output: None,
        }
    }
    /// Provide both pending-only and complete byte-stream output diagnostics.
    pub fn with_output(snapshot: impl Fn() -> OutputSnapshot + Send + Sync + 'static) -> Self {
        let output = std::sync::Arc::new(snapshot);
        let queued = output.clone();
        Self {
            queued: std::sync::Arc::new(move || queued().queued),
            output: Some(output),
        }
    }
    pub fn snapshot(&self) -> QueueSnapshot {
        (self.queued)()
    }
    /// None means this transport only supplies pending-queue diagnostics.
    pub fn output_snapshot(&self) -> Option<OutputSnapshot> {
        self.output.as_ref().map(|snapshot| snapshot())
    }
}

/// A bilateral transport usable by the common client/server facade. Descriptor
/// transports implement this without passing ancillary data through byte IO.
pub trait TwoPartyNetwork: crate::VatNetwork<VatId> {
    fn side(&self) -> VatId;
    fn outgoing_queue(&self) -> QueueDiagnostics;
    /// Local output closure, including write errors independently of input EOF.
    /// The future must not retain the transport after the network is dropped.
    fn output_closed(&self) -> Shared<Promise<(), Error>>;
    /// Optional earlier notification of write queue completion. An error must
    /// be reported without waiting for transport closure. Does not retain IO.
    fn when_write_finished(&self) -> Option<Shared<Promise<(), Error>>> {
        None
    }
}
impl<T: AsyncRead + Unpin + 'static> TwoPartyNetwork for VatNetwork<T> {
    fn side(&self) -> VatId {
        self.side
    }
    fn outgoing_queue(&self) -> QueueDiagnostics {
        let queue = self.outgoing_queue();
        QueueDiagnostics::with_output(move || queue.output_snapshot())
    }
    fn output_closed(&self) -> Shared<Promise<(), Error>> {
        self.output_closed()
    }
    fn when_write_finished(&self) -> Option<Shared<Promise<(), Error>>> {
        Some(self.when_write_finished())
    }
}

/// One bilateral RPC connection. Obtain capabilities and observation handles,
/// then poll this future on a local executor. Dropping it cancels RPC immediately.
/// Disconnect observers and queue metrics do not retain the connection.
///
/// Owned streams use the ordinary network directly. Borrowed byte streams use a
/// safe, bounded copying adapter. Other transports can supply a scoped network;
/// neither mechanism places borrowed IO in the runtime's `'static` tasks.
#[must_use = "the client must be polled to drive RPC"]
pub struct TwoPartyClient<'a> {
    system: Option<Running>,
    side: VatId,
    queue: QueueDiagnostics,
    output_closed: Option<WakeDriven<Shared<Promise<(), Error>>>>,
    write_finished: Option<WakeDriven<Shared<Promise<(), Error>>>>,
    disconnected: Shared<Promise<(), Error>>,
    completion: Option<oneshot::Sender<capnp::Result<()>>>,
    borrowed: Option<Box<dyn borrowed::Pump + 'a>>,
    scope: Option<Box<dyn FnOnce() + 'a>>,
}

impl TwoPartyClient<'static> {
    /// Connect without a local bootstrap capability, using default reader limits.
    pub fn new(stream: impl AsyncRead + AsyncWrite + Unpin + 'static) -> Self {
        Self::with_bootstrap(stream, None, VatId::Client)
    }

    /// Connect on either side, optionally exposing a local bootstrap capability.
    pub fn with_bootstrap(
        stream: impl AsyncRead + AsyncWrite + Unpin + 'static,
        bootstrap: Option<Client>,
        side: VatId,
    ) -> Self {
        let (read, write) = stream.split();
        Self::from_network(
            VatNetwork::new(read, write, side, ReaderOptions::new()),
            bootstrap,
        )
    }

    /// Use a network configured with custom reader limits, a clock or flow policy.
    pub fn from_network<N: TwoPartyNetwork + 'static>(
        network: N,
        bootstrap: Option<Client>,
    ) -> Self {
        Self::from_network_with_driver(network, bootstrap, RpcDriver::Legacy)
            .expect("legacy construction is infallible")
    }

    /// Select the protocol driver explicitly. Portable storage limits are
    /// validated before the network is transferred to its runtime.
    pub fn from_network_with_driver<N: TwoPartyNetwork + 'static>(
        network: N,
        bootstrap: Option<Client>,
        driver: RpcDriver,
    ) -> capnp::Result<Self> {
        let side = network.side();
        let queue = network.outgoing_queue();
        let output_closed = network.output_closed();
        let write_finished = network.when_write_finished();
        let (completion, receiver) = oneshot::channel();
        Ok(Self {
            system: Some(Running::new(Box::new(network), bootstrap, driver)?),
            side,
            queue,
            output_closed: Some(WakeDriven::new(output_closed)),
            write_finished: write_finished.map(WakeDriven::new),
            disconnected: Promise::from_future(async move {
                receiver
                    .await
                    .map_err(|_| Error::disconnected("RPC driver canceled".into()))?
            })
            .shared(),
            completion: Some(completion),
            borrowed: None,
            scope: None,
        })
    }

    /// Connect an owned byte stream using the selected protocol driver.
    pub fn with_driver(
        stream: impl AsyncRead + AsyncWrite + Unpin + 'static,
        bootstrap: Option<Client>,
        side: VatId,
        driver: RpcDriver,
    ) -> capnp::Result<Self> {
        let (read, write) = stream.split();
        Self::from_network_with_driver(
            VatNetwork::new(read, write, side, driver.reader_options()),
            bootstrap,
            driver,
        )
    }
}

impl<'a> TwoPartyClient<'a> {
    /// Associate an owned transport adapter with a borrowed resource. Useful for
    /// adapters which own a duplicated OS handle. The resource is released only
    /// after the RPC system and transport are destroyed, even on cancellation.
    pub fn from_scoped_network<N: TwoPartyNetwork + 'static, T: ?Sized>(
        network: N,
        bootstrap: Option<Client>,
        resource: &'a mut T,
    ) -> Self {
        let mut client: Self = TwoPartyClient::from_network(network, bootstrap);
        client.scope = Some(Box::new(move || {
            let _resource = resource;
        }));
        client
    }
    /// Scoped-network variant with explicit driver selection. The resource
    /// remains borrowed until protocol and transport ownership are released.
    pub fn from_scoped_network_with_driver<N: TwoPartyNetwork + 'static, T: ?Sized>(
        network: N,
        bootstrap: Option<Client>,
        resource: &'a mut T,
        driver: RpcDriver,
    ) -> capnp::Result<Self> {
        let mut client: Self =
            TwoPartyClient::from_network_with_driver(network, bootstrap, driver)?;
        client.scope = Some(Box::new(move || {
            let _resource = resource;
        }));
        Ok(client)
    }
    /// Borrow a stream until this driver completes or is dropped. Cancellation
    /// releases the borrow without dropping or closing the caller's stream.
    /// Normal protocol shutdown may close its write half. Buffered input already
    /// consumed by RPC is not returned to the caller on cancellation.
    ///
    /// ```compile_fail
    /// let mut io = futures::io::Cursor::new(Vec::<u8>::new());
    /// let client = capnp_rpc::twoparty::TwoPartyClient::new_borrowed(
    ///     &mut io, None, capnp_rpc::rpc_twoparty_capnp::Side::Client, Default::default());
    /// drop(io); // the stream must outlive its RPC driver
    /// drop(client);
    /// ```
    pub fn new_borrowed<T: AsyncRead + AsyncWrite + Unpin + 'a>(
        stream: &'a mut T,
        bootstrap: Option<Client>,
        side: VatId,
        options: ReaderOptions,
    ) -> Self {
        Self::new_borrowed_with_driver(stream, bootstrap, side, options, RpcDriver::Legacy)
            .expect("legacy construction is infallible")
    }

    /// Borrowed-stream variant with explicit driver selection. Both drivers
    /// use the same bounded IO pump and release the caller's borrow on Drop.
    /// The explicit reader options apply to both framing and portable admission.
    pub fn new_borrowed_with_driver<T: AsyncRead + AsyncWrite + Unpin + 'a>(
        stream: &'a mut T,
        bootstrap: Option<Client>,
        side: VatId,
        options: ReaderOptions,
        driver: RpcDriver,
    ) -> capnp::Result<Self> {
        let (proxy, pump) = borrowed::new(stream);
        let (read, write) = proxy.split();
        let mut client: Self = TwoPartyClient::from_network_with_driver(
            VatNetwork::new(read, write, side, options),
            bootstrap,
            driver.with_reader_options(options),
        )?;
        client.borrowed = Some(Box::new(pump));
        Ok(client)
    }

    /// The remote bootstrap, irrespective of this endpoint's side.
    pub fn bootstrap<T: capnp::capability::FromClientHook>(&mut self) -> T {
        let peer = match self.side {
            VatId::Client => VatId::Server,
            VatId::Server => VatId::Client,
        };
        match self.system.as_mut() {
            Some(system) => system.bootstrap(peer),
            None => T::new(crate::broken::new_cap(Error::disconnected(
                "RPC driver already completed".into(),
            ))),
        }
    }

    /// Completes after this driver's shutdown and cleanup. Multiple observers
    /// receive the same result. Cancellation is reported as Disconnected.
    pub fn on_disconnect(&self) -> Shared<Promise<(), Error>> {
        self.disconnected.clone()
    }

    /// Close the connection while continuing to poll this driver. Already
    /// completed drivers return a ready future, making shutdown idempotent.
    pub fn get_disconnector(&self) -> Promise<(), Error> {
        match &self.system {
            Some(system) => system.get_disconnector(),
            None => Promise::ok(()),
        }
    }

    pub fn set_trace_encoder(&mut self, encoder: impl Fn(&Error) -> String + 'static) {
        if let Some(system) = &mut self.system {
            system.set_trace_encoder(encoder);
        }
    }

    pub fn clear_trace_encoder(&mut self) {
        if let Some(system) = &mut self.system {
            system.clear_trace_encoder();
        }
    }

    pub fn outgoing_queue(&self) -> QueueDiagnostics {
        self.queue.clone()
    }

    /// Configure per-connection outgoing Call admission before spawning the driver.
    /// See [`RpcSystem::set_outgoing_call_limit`] for credit lifetimes and scope.
    pub fn set_outgoing_call_limit(&mut self, calls: usize) {
        if let Some(system) = &mut self.system {
            system.set_outgoing_call_limit(calls);
        }
    }

    pub fn diagnostics(&self) -> crate::RpcDiagnostics {
        self.system.as_ref().map_or_else(
            || crate::RpcDiagnostics(Rc::new(Vec::new)),
            Running::diagnostics,
        )
    }
    pub fn get_current_queue_size(&self) -> usize {
        self.queue.snapshot().bytes
    }
    pub fn get_current_queue_count(&self) -> usize {
        self.queue.snapshot().message_count
    }
    pub fn get_outgoing_message_wait_time(&self) -> std::time::Duration {
        self.queue.snapshot().wait_time
    }

    fn finish(&mut self, result: capnp::Result<()>) {
        // Release runtime ownership before waking disconnect observers.
        drop(self.system.take());
        drop(self.borrowed.take());
        drop(self.scope.take());
        if let Some(completion) = self.completion.take() {
            let _ = completion.send(result);
        }
    }
}

impl Future for TwoPartyClient<'_> {
    type Output = capnp::Result<()>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        for _ in 0..16 {
            match Pin::new(this.system.as_mut().expect("RPC driver already completed")).poll(cx) {
                Poll::Ready(result) => {
                    if let Some(pump) = &mut this.borrowed {
                        pump.poll_pending_close(cx);
                    }
                    this.finish(result.clone());
                    return Poll::Ready(result);
                }
                Poll::Pending => {
                    // A write failure can occur while the peer keeps input
                    // open forever. Propagate it without waiting for input EOF
                    // or for the network's last connection handle to disappear.
                    let written = this
                        .write_finished
                        .as_mut()
                        .map_or(Poll::Pending, |written| Pin::new(written).poll(cx));
                    if written.is_ready() {
                        this.write_finished = None;
                    }
                    let closed = this
                        .output_closed
                        .as_mut()
                        .map_or(Poll::Pending, |closed| Pin::new(closed).poll(cx));
                    if closed.is_ready() {
                        // Explicit shutdown can finish the physical close
                        // before the RPC task set consumes its registrations.
                        // Shared receivers cannot be polled after completion.
                        this.output_closed = None;
                    }
                    let failure = match written {
                        Poll::Ready(Err(error)) => Some(error),
                        _ => match closed {
                            Poll::Ready(Err(error)) => Some(error),
                            _ => None,
                        },
                    };
                    // An explicit disconnect terminates the write queue with
                    // Disconnected before its physical close completes. Keep
                    // driving the system/pump so its registrations can finish;
                    // dropping it here would report false task cancellation.
                    let deliberate = failure.as_ref().is_some_and(|error| {
                        error.kind == capnp::ErrorKind::Disconnected
                            && this
                                .system
                                .as_ref()
                                .is_some_and(|system| system.is_closing())
                    });
                    if let Some(error) = failure.filter(|_| !deliberate) {
                        let result = if error.kind == capnp::ErrorKind::Disconnected {
                            Ok(())
                        } else {
                            Err(error)
                        };
                        if let Some(pump) = &mut this.borrowed {
                            pump.poll_pending_close(cx);
                        }
                        this.finish(result.clone());
                        return Poll::Ready(result);
                    }
                    if !this.borrowed.as_mut().is_some_and(|pump| pump.poll_io(cx)) {
                        return Poll::Pending;
                    }
                }
            }
        }
        // Bound work per poll even when both IO directions remain ready.
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

impl Drop for TwoPartyClient<'_> {
    fn drop(&mut self) {
        self.finish(Err(Error::disconnected("RPC driver canceled".into())));
    }
}

type TraceEncoder = Rc<dyn Fn(&Error) -> String>;

/// Accepts independent connections exposing the same bootstrap capability.
/// Poll the associated [`ServerDriver`] concurrently. Its lifetime owns the
/// accepted streams; dropping it cancels all owned connections. Borrowed accepts
/// have their own driver and are excluded from [`Self::drain`].
///
/// ```no_run
/// # async fn serve_one<T: futures::AsyncRead + futures::AsyncWrite + Unpin + 'static>(
/// #     io: T, bootstrap: capnp::capability::Client) -> capnp::Result<()> {
/// let (server, driver) = capnp_rpc::twoparty::TwoPartyServer::new(bootstrap);
/// server.accept(io)?;
/// let drained = server.drain();
/// drop(server); // stop accepting; the driver retains the accepted connection
/// futures::future::try_join(driver, drained).await?;
/// # Ok(()) }
/// ```
pub struct TwoPartyServer {
    bootstrap: Client,
    options: ReaderOptions,
    driver: RpcDriver,
    trace_encoder: Option<TraceEncoder>,
    tasks: task_set::TaskSetHandle<Error>,
    drain: Rc<RefCell<Drain>>,
}

/// Drives all owned server connections. Unlike the C++ event-loop-owned task
/// set, Rust requires this future to be explicitly polled. Dropping the accept
/// handle stops further acceptance; this driver then finishes after draining.
#[must_use = "the server driver must be polled"]
pub struct ServerDriver {
    // Field order matters: release every accepted connection before the drain
    // guard publishes cancellation to user-supplied wakers.
    tasks: task_set::TaskSet<Error>,
    drain: ServerDrain,
}

impl Future for ServerDriver {
    type Output = capnp::Result<()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.tasks).poll(cx)
    }
}

impl Drop for ServerDriver {
    fn drop(&mut self) {
        // Close admission before any connection destructor can call user code.
        self.drain.0.borrow_mut().alive = false;
    }
}

struct ServerDrain(Rc<RefCell<Drain>>);
impl Drop for ServerDrain {
    fn drop(&mut self) {
        let waiting = {
            let mut drain = self.0.borrow_mut();
            std::mem::take(&mut drain.waiting)
        };
        for waiter in waiting {
            let _ = waiter.send(Err(Error::disconnected("server driver canceled".into())));
        }
    }
}

struct Drain {
    alive: bool,
    count: usize,
    waiting: Vec<oneshot::Sender<capnp::Result<()>>>,
}

// Count acceptance synchronously, including tasks not yet polled. Notify only
// after dropping the connection, and capture empty drain at the time of the call.
struct OwnedConnection {
    client: Option<TwoPartyClient<'static>>,
    drain: Rc<RefCell<Drain>>,
}
impl Future for OwnedConnection {
    type Output = capnp::Result<()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(self.client.as_mut().unwrap()).poll(cx)
    }
}
impl Drop for OwnedConnection {
    fn drop(&mut self) {
        drop(self.client.take());
        let waiting = {
            let mut drain = self.drain.borrow_mut();
            drain.count -= 1;
            if drain.alive && drain.count == 0 {
                std::mem::take(&mut drain.waiting)
            } else {
                Vec::new()
            }
        };
        for waiter in waiting {
            let _ = waiter.send(Ok(()));
        }
    }
}

struct Reaper<F>(F);
impl<F: FnMut(Error) + 'static> task_set::TaskReaper<Error> for Reaper<F> {
    fn task_failed(&mut self, error: Error) {
        (self.0)(error);
    }
}

impl TwoPartyServer {
    /// Creates an accept handle and its connection owner/driver. Individual
    /// connection failures are isolated; use [`Self::with_error_handler`] to
    /// observe them instead of ignoring them.
    pub fn new(bootstrap: Client) -> (Self, ServerDriver) {
        Self::with_error_handler(bootstrap, |_| {})
    }

    pub fn with_error_handler(
        bootstrap: Client,
        on_error: impl FnMut(Error) + 'static,
    ) -> (Self, ServerDriver) {
        Self::with_driver(bootstrap, RpcDriver::Legacy, on_error)
    }

    /// All accepted owned, borrowed and scoped connections use this driver.
    /// Portable options are checked for each acceptance; no legacy fallback is used.
    pub fn with_driver(
        bootstrap: Client,
        protocol: RpcDriver,
        on_error: impl FnMut(Error) + 'static,
    ) -> (Self, ServerDriver) {
        let (tasks, driver) = task_set::TaskSet::new(Box::new(Reaper(on_error)));
        let drain = Rc::new(RefCell::new(Drain {
            alive: true,
            count: 0,
            waiting: Vec::new(),
        }));
        (
            Self {
                bootstrap,
                options: protocol.reader_options(),
                driver: protocol,
                trace_encoder: None,
                tasks,
                drain: drain.clone(),
            },
            ServerDriver {
                tasks: driver,
                drain: ServerDrain(drain),
            },
        )
    }

    /// Applies to subsequently accepted connections.
    pub fn set_reader_options(&mut self, options: ReaderOptions) {
        self.options = options;
        self.driver = self.driver.with_reader_options(options);
    }

    /// Applies to subsequently accepted connections; existing connections keep
    /// their encoder, as in the C++ server constructor configuration.
    pub fn set_trace_encoder(&mut self, encoder: impl Fn(&Error) -> String + 'static) {
        self.trace_encoder = Some(Rc::new(encoder));
    }

    fn configure(&self, client: &mut TwoPartyClient<'_>) {
        if let Some(encoder) = &self.trace_encoder {
            let encoder = encoder.clone();
            client.set_trace_encoder(move |error| encoder(error));
        }
    }

    /// Transfers ownership to the server driver. Fails if that driver is gone.
    pub fn accept(
        &self,
        stream: impl AsyncRead + AsyncWrite + Unpin + 'static,
    ) -> capnp::Result<()> {
        let (read, write) = stream.split();
        self.accept_network(VatNetwork::new(read, write, VatId::Server, self.options))
    }

    /// Accepts a configured server-side network (e.g. with a custom flow policy).
    pub fn accept_network<N: TwoPartyNetwork + 'static>(&self, network: N) -> capnp::Result<()> {
        if !self.drain.borrow().alive {
            return Err(Error::disconnected("server driver canceled".into()));
        }
        if network.side() != VatId::Server {
            return Err(Error::failed("server network must use Side::Server".into()));
        }
        let mut client = TwoPartyClient::from_network_with_driver(
            network,
            Some(self.bootstrap.clone()),
            self.driver,
        )?;
        self.configure(&mut client);
        self.drain.borrow_mut().count += 1;
        self.tasks
            .try_add(OwnedConnection {
                client: Some(client),
                drain: self.drain.clone(),
            })
            .map_err(|()| Error::disconnected("server driver canceled".into()))
    }

    /// Like a borrowed byte-stream accept, but for an owned transport adapter
    /// tied to a caller-owned resource (e.g. a duplicated descriptor socket).
    pub fn accept_scoped_network<'a, N: TwoPartyNetwork + 'static, T: ?Sized>(
        &self,
        network: N,
        resource: &'a mut T,
    ) -> capnp::Result<TwoPartyClient<'a>> {
        if network.side() != VatId::Server {
            return Err(Error::failed("server network must use Side::Server".into()));
        }
        let mut client = TwoPartyClient::from_scoped_network_with_driver(
            network,
            Some(self.bootstrap.clone()),
            resource,
            self.driver,
        )?;
        self.configure(&mut client);
        Ok(client)
    }

    /// The returned driver owns only the borrow, is not counted by drain, and
    /// can outlive this accept handle. Dropping it cancels this RPC connection.
    pub fn accept_borrowed<'a, T: AsyncRead + AsyncWrite + Unpin + 'a>(
        &self,
        stream: &'a mut T,
    ) -> capnp::Result<TwoPartyClient<'a>> {
        let mut client = TwoPartyClient::new_borrowed_with_driver(
            stream,
            Some(self.bootstrap.clone()),
            VatId::Server,
            self.options,
            self.driver,
        )?;
        self.configure(&mut client);
        Ok(client)
    }

    /// Waits for the next point at which all owned connections have completed.
    /// Acceptance may continue while this is pending. Does not stop a listener.
    pub fn drain(&self) -> Promise<(), Error> {
        let mut drain = self.drain.borrow_mut();
        if !drain.alive {
            return Promise::err(Error::disconnected("server driver canceled".into()));
        }
        if drain.count == 0 {
            return Promise::ok(());
        }
        let (sender, receiver) = oneshot::channel();
        drain.waiting.retain(|waiter| !waiter.is_closed());
        drain.waiting.push(sender);
        Promise::from_future(async move {
            receiver
                .await
                .map_err(|_| Error::disconnected("server driver canceled".into()))?
        })
    }

    /// Accepts a stream of IO results. Listener errors propagate; EOF completes.
    /// Dropping this future stops listening while owned connections keep running
    /// on the server driver. A socket listener can be adapted with `try_unfold`.
    pub async fn listen<S, T>(&self, listener: S) -> capnp::Result<()>
    where
        S: futures::TryStream<Ok = T, Error = std::io::Error>,
        T: AsyncRead + AsyncWrite + Unpin + 'static,
    {
        use futures::TryStreamExt;
        let networks = listener.map_ok(|stream| {
            let (read, write) = stream.split();
            VatNetwork::new(read, write, VatId::Server, self.options)
        });
        self.listen_networks(networks).await
    }

    /// Accept configured bilateral networks. This preserves descriptor-aware
    /// framing and limits supplied by a transport-specific listener adapter.
    pub async fn listen_networks<S, N>(&self, listener: S) -> capnp::Result<()>
    where
        S: futures::TryStream<Ok = N, Error = std::io::Error>,
        N: TwoPartyNetwork + 'static,
    {
        use futures::{StreamExt, TryStreamExt};
        let listener = listener.into_stream();
        futures::pin_mut!(listener);
        while let Some(stream) = listener.next().await {
            self.accept_network(stream?)?;
        }
        Ok(())
    }
}
