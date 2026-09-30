//! Cap'n Proto two-party RPC over Linux or macOS Unix streams, including SCM_RIGHTS.
//!
//! Uses the standard uncompressed segment framing, with ancillary descriptors
//! attached to the first bytes of each message. The caller supplies a connected
//! socket and establishes its peer authorization before constructing the network.
//! Linux input prefetch preserves per-message descriptor ownership and limits.
//! macOS reads one frame at a time and returns owned message bodies; caller FD
//! slots still avoid a descriptor Vec allocation. Received macOS descriptors use
//! fcntl close-on-exec, which is not atomic with concurrent process creation. Short-lived
//! RPC control messages share the receive buffer and must be dropped before the
//! next read; Calls and Returns own independent storage for retained capabilities.
use capnp::{
    capability::Promise,
    message::{Builder, HeapAllocator, ReaderOptions},
    Error,
};
use capnp_rpc::{rpc_twoparty_capnp::Side, Connection, IncomingMessage, OutgoingMessage};
use futures::{
    channel::{mpsc, oneshot},
    future::{LocalBoxFuture, Shared},
    FutureExt, StreamExt,
};
use std::{
    cell::{Cell, RefCell},
    io, mem,
    os::fd::{AsRawFd, OwnedFd},
    rc::Rc,
};
use tokio::{io::Interest, net::UnixStream};

mod ancillary;
use ancillary::{recv_once, send_once};
mod facade;
mod queue;
pub use facade::{client, client_borrowed, TwoPartyServer};

type Body = Rc<Builder<HeapAllocator>>;
fn io_error(error: io::Error) -> Error {
    Error::disconnected(error.to_string())
}
fn stopped() -> Error {
    Error::disconnected("Unix RPC connection closed".into())
}

/// Resource limits for incoming messages. Excess descriptors are closed; an
/// oversized or invalid segment table terminates the connection before allocation.
#[derive(Clone, Copy)]
pub struct Options {
    pub reader: ReaderOptions,
    pub max_message_words: usize,
    /// Maximum descriptors received per message. Zero also disables sending
    /// descriptors, matching C++ TwoPartyVatNetwork's byte-only mode. Positive
    /// limits do not restrict outgoing messages (the transport caps those at 253).
    pub max_fds: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            reader: ReaderOptions::new(),
            max_message_words: 8 * 1024 * 1024,
            max_fds: 16,
        }
    }
}
struct Input {
    body: capnp::message::Reader<capnp_futures::BufferedSegments>,
    fds: Vec<OwnedFd>,
}
impl IncomingMessage for Input {
    fn size_in_words(&self) -> usize {
        self.body.size_in_words()
    }
    fn get_body(&self) -> capnp::Result<capnp::any_pointer::Reader<'_>> {
        self.body.get_root()
    }
    fn take_fds(&mut self) -> Vec<OwnedFd> {
        mem::take(&mut self.fds)
    }
}
enum Write {
    Message(
        Body,
        Vec<Rc<OwnedFd>>,
        oneshot::Sender<capnp::Result<()>>,
        queue::Pending,
    ),
    Shutdown(oneshot::Sender<capnp::Result<()>>),
}
struct Output {
    body: Builder<HeapAllocator>,
    fds: Vec<Rc<OwnedFd>>,
    sender: mpsc::UnboundedSender<Write>,
    metrics: queue::Metrics,
    send_fds: bool,
}
impl OutgoingMessage for Output {
    fn get_body(&mut self) -> capnp::Result<capnp::any_pointer::Builder<'_>> {
        self.body.get_root()
    }
    fn get_body_as_reader(&self) -> capnp::Result<capnp::any_pointer::Reader<'_>> {
        self.body.get_root_as_reader()
    }
    fn set_fds(&mut self, fds: Vec<Rc<OwnedFd>>) {
        self.fds = if self.send_fds {
            fds.into_iter().take(253).collect()
        } else {
            Vec::new()
        };
    }
    fn send(self: Box<Self>) -> (Promise<(), Error>, Body) {
        let body = Rc::new(self.body);
        let (tx, rx) = oneshot::channel();
        let pending = match self.metrics.enqueue(body.size_in_words() * 8) {
            Ok(pending) => pending,
            Err(error) => return (Promise::err(error), body),
        };
        let queued =
            self.sender
                .unbounded_send(Write::Message(body.clone(), self.fds, tx, pending));
        let promise = match queued {
            Ok(()) => Promise::from_future(async move { rx.await.map_err(|_| stopped())? }),
            Err(_) => Promise::err(stopped()),
        };
        (promise, body)
    }
    fn take(self: Box<Self>) -> Builder<HeapAllocator> {
        self.body
    }
    fn size_in_words(&self) -> usize {
        self.body.size_in_words()
    }
}
struct Inner {
    socket: Rc<UnixStream>,
    sender: mpsc::UnboundedSender<Write>,
    input: RefCell<Option<FdReader<Rc<UnixStream>>>>,
    peer: Side,
    reading: Cell<bool>,
    closed: Cell<bool>,
    borrowed: bool,
    stop_writer: RefCell<Option<oneshot::Sender<()>>>,
    metrics: queue::Metrics,
    send_fds: bool,
    window: capnp_rpc::twoparty::SendBufferWindow,
}
impl Inner {
    fn close(&self) {
        if !self.closed.replace(true) {
            // SAFETY: socket owns this descriptor. shutdown does not close or
            // transfer it; pending reads/writes wake and observe the shutdown.
            if !self.borrowed {
                unsafe {
                    libc::shutdown(self.socket.as_raw_fd(), libc::SHUT_RDWR);
                }
            }
            let stop = self.stop_writer.borrow_mut().take();
            if let Some(stop) = stop {
                let _ = stop.send(());
            }
            self.sender.close_channel();
            // Release prefetched descriptors even while connection handles live.
            let input = self.input.borrow_mut().take();
            drop(input);
        }
    }
}
struct ReadGuard {
    inner: Rc<Inner>,
    input: Option<FdReader<Rc<UnixStream>>>,
    complete: bool,
}
impl Drop for ReadGuard {
    fn drop(&mut self) {
        self.inner.reading.set(false);
        // Cancellation after consuming a frame prefix must never allow the next
        // read to interpret its remainder as a new message. Close the connection.
        if !self.complete {
            self.inner.close();
        } else if !self.inner.closed.get() {
            *self.inner.input.borrow_mut() = self.input.take();
        }
    }
}
struct Endpoint(Rc<Inner>);
impl Connection<Side> for Endpoint {
    fn new_stream(&mut self) -> (Box<dyn capnp_rpc::FlowController>, Promise<(), Error>) {
        self.0.window.new_stream()
    }
    fn get_peer_vat_id(&self) -> Side {
        self.0.peer
    }
    fn connection_id(&self) -> usize {
        Rc::as_ptr(&self.0) as usize
    }
    fn new_outgoing_message(&mut self, words: u32) -> Box<dyn OutgoingMessage> {
        let allocator = if words == 0 {
            HeapAllocator::new()
        } else {
            HeapAllocator::new().first_segment_words(words)
        };
        Box::new(Output {
            body: Builder::new(allocator),
            fds: vec![],
            sender: self.0.sender.clone(),
            metrics: self.0.metrics.clone(),
            send_fds: self.0.send_fds,
        })
    }
    fn receive_incoming_message(&mut self) -> Promise<Option<Box<dyn IncomingMessage>>, Error> {
        if self.0.closed.get() {
            return Promise::err(stopped());
        }
        if self.0.reading.get() {
            return Promise::err(Error::failed("concurrent Unix RPC read".into()));
        }
        let mut slot = self.0.input.borrow_mut();
        if slot
            .as_ref()
            .is_some_and(|r| r.reader.has_outstanding_short_lived_message())
        {
            return Promise::err(Error::failed(
                "previous short-lived message is still alive".into(),
            ));
        }
        let input = slot.take();
        drop(slot);
        self.0.reading.set(true);
        let mut guard = ReadGuard {
            inner: self.0.clone(),
            input,
            complete: false,
        };
        Promise::from_future(async move {
            let result = if guard.inner.closed.get() {
                Err(stopped())
            } else {
                guard.input.as_mut().ok_or_else(stopped)?.read().await
            };
            guard.complete = result.as_ref().is_ok_and(Option::is_some);
            drop(guard);
            result.map(|m| m.map(|m| Box::new(m) as Box<dyn IncomingMessage>))
        })
    }
    fn shutdown(&mut self, _: capnp::Result<()>) -> Promise<(), Error> {
        let (tx, rx) = oneshot::channel();
        if self.0.sender.unbounded_send(Write::Shutdown(tx)).is_err() {
            return Promise::err(stopped());
        }
        self.0.sender.close_channel();
        Promise::from_future(async move { rx.await.map_err(|_| stopped())? })
    }
}

/// A two-party VatNetwork with descriptor passing. It runs on a Tokio LocalSet;
/// `RpcSystem` drives its queued writes. No detached background task is required.
/// Streaming credit uses the socket's live SO_SNDBUF, with a connection-wide
/// 64 KiB fallback after the first unavailable query, matching C++ two-party RPC.
pub struct VatNetwork {
    inner: Rc<Inner>,
    side: Side,
    accepted: bool,
    driver: Shared<LocalBoxFuture<'static, capnp::Result<()>>>,
    output_closed: Shared<Promise<(), Error>>,
}
impl VatNetwork {
    pub fn new(socket: UnixStream, side: Side, options: Options) -> Self {
        Self::with_ownership(socket, side, options, false)
    }
    fn with_ownership(
        socket: UnixStream,
        side: Side,
        mut options: Options,
        borrowed: bool,
    ) -> Self {
        options.max_fds = options.max_fds.min(253);
        let (sender, mut receiver) = mpsc::unbounded();
        let (stop_writer, stopped_writer) = oneshot::channel();
        let (closed_tx, closed_rx) = oneshot::channel();
        let socket = Rc::new(socket);
        let weak_socket = Rc::downgrade(&socket);
        let window = capnp_rpc::twoparty::SendBufferWindow::new(move || {
            let socket = weak_socket.upgrade()?;
            socket2::SockRef::from(&*socket).send_buffer_size().ok()
        });
        let inner = Rc::new(Inner {
            socket: socket.clone(),
            sender,
            input: RefCell::new(Some(FdReader::new(socket.clone(), options))),
            peer: if side == Side::Client {
                Side::Server
            } else {
                Side::Client
            },
            reading: Cell::new(false),
            closed: Cell::new(false),
            borrowed,
            stop_writer: RefCell::new(Some(stop_writer)),
            metrics: queue::Metrics::default(),
            send_fds: options.max_fds > 0,
            window,
        });
        // Only retain the socket in the writer, so connection handles do not
        // form a cycle through the queue that their destruction must close.
        let weak = Rc::downgrade(&inner);
        let writer = async move {
            let mut bytes = Vec::new();
            while let Some(command) = receiver.next().await {
                match command {
                    Write::Message(body, fds, done, pending) => {
                        drop(pending); // active writes are excluded from queue metrics
                        bytes.clear();
                        let result = async {
                            capnp::serialize::write_message(&mut bytes, &*body)?;
                            write_message(&socket, &bytes, &fds).await.map_err(io_error)
                        }
                        .await;
                        // Reuse ordinary message storage without pinning an unusually
                        // large allocation for the rest of the connection's lifetime.
                        if bytes.capacity() > 64 * 1024 {
                            bytes = Vec::new();
                        }
                        let _ = done.send(result.clone());
                        if result.is_err() {
                            if let Some(inner) = weak.upgrade() {
                                inner.close();
                            }
                            return result;
                        }
                    }
                    Write::Shutdown(done) => {
                        // SAFETY: this stream owns the descriptor, and all writes
                        // before this command have completed in queue order.
                        let result =
                            if unsafe { libc::shutdown(socket.as_raw_fd(), libc::SHUT_WR) } == 0 {
                                Ok(())
                            } else {
                                Err(io_error(io::Error::last_os_error()))
                            };
                        let _ = done.send(result.clone());
                        return result;
                    }
                }
            }
            Ok(())
        };
        let driver = async move {
            let result = match futures::future::select(stopped_writer, Box::pin(writer)).await {
                futures::future::Either::Right((result, _)) => result,
                futures::future::Either::Left(_) => Err(stopped()),
            };
            let _ = closed_tx.send(result.clone());
            result
        }
        .boxed_local()
        .shared();
        Self {
            inner,
            side,
            accepted: false,
            driver,
            output_closed: Promise::from_future(
                async move { closed_rx.await.map_err(|_| stopped())? },
            )
            .shared(),
        }
    }
}
impl capnp_rpc::twoparty::TwoPartyNetwork for VatNetwork {
    fn side(&self) -> Side {
        self.side
    }
    fn outgoing_queue(&self) -> capnp_rpc::twoparty::QueueDiagnostics {
        self.inner.metrics.observer()
    }
    fn output_closed(&self) -> Shared<Promise<(), Error>> {
        self.output_closed.clone()
    }
}
impl capnp_rpc::VatNetwork<Side> for VatNetwork {
    fn connect(&mut self, peer: Side) -> Option<Box<dyn Connection<Side>>> {
        (peer != self.side)
            .then(|| Box::new(Endpoint(self.inner.clone())) as Box<dyn Connection<Side>>)
    }
    fn accept(&mut self) -> Promise<Box<dyn Connection<Side>>, Error> {
        if mem::replace(&mut self.accepted, true) {
            Promise::from_future(futures::future::pending())
        } else {
            Promise::ok(Box::new(Endpoint(self.inner.clone())))
        }
    }
    fn drive_until_shutdown(&mut self) -> Promise<(), Error> {
        Promise::from_future(self.driver.clone())
    }
}

async fn write_message(socket: &UnixStream, bytes: &[u8], fds: &[Rc<OwnedFd>]) -> io::Result<()> {
    let mut written = 0;
    while written < bytes.len() {
        socket.writable().await?;
        match socket.try_io(Interest::WRITABLE, || {
            send_once(
                socket,
                &bytes[written..],
                if written == 0 { fds } else { &[] },
            )
        }) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(count) => written += count,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                continue
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
// Partial reads and prefetch must own descriptors independently of an individual
// read future's borrowed output slots. Bounded staging survives cancellation and
// closes everything on stream drop without allocating a descriptor Vec.
struct PendingFds {
    slots: [Option<OwnedFd>; 253],
    len: usize,
}
impl Default for PendingFds {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| None),
            len: 0,
        }
    }
}
impl PendingFds {
    fn len(&self) -> usize {
        self.len
    }
    fn is_empty(&self) -> bool {
        self.len == 0
    }
    fn push(&mut self, fd: OwnedFd) {
        self.slots[self.len] = Some(fd);
        self.len += 1;
    }
    fn drain(&mut self) -> impl Iterator<Item = OwnedFd> + '_ {
        let len = mem::take(&mut self.len);
        self.slots[..len].iter_mut().filter_map(Option::take)
    }
    fn clear(&mut self) {
        self.drain().for_each(drop);
    }
}
#[derive(Default)]
struct Ancillary {
    received: u64,
    end: u64,
    fds: PendingFds,
    #[cfg(test)]
    reads: usize,
}
struct AncillarySource<S> {
    socket: S,
    limit: Cell<usize>,
    ancillary: RefCell<Ancillary>,
}
impl<S: std::borrow::Borrow<UnixStream> + Unpin> futures::AsyncRead for AncillarySource<S> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bytes: &mut [u8],
    ) -> std::task::Poll<io::Result<usize>> {
        use std::task::{ready, Poll};
        let socket = self.socket.borrow();
        loop {
            ready!(socket.poll_read_ready(cx))?;
            let mut ancillary = self.ancillary.borrow_mut();
            let result = socket.try_io(Interest::READABLE, || {
                recv_once(socket, bytes, &mut ancillary.fds, self.limit.get())
            });
            match result {
                Ok(count) => {
                    ancillary.received = ancillary
                        .received
                        .checked_add(count as u64)
                        .ok_or_else(|| io::Error::other("Unix input offset overflow"))?;
                    if !ancillary.fds.is_empty() {
                        // Linux stops at the ancillary-data barrier. macOS
                        // reads are bounded to one frame instead. In both cases,
                        // these FDs belong to the frame containing the last byte.
                        ancillary.end = ancillary.received;
                    }
                    #[cfg(test)]
                    {
                        ancillary.reads += 1;
                    }
                    return Poll::Ready(Ok(count));
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Poll::Ready(Err(error)),
            }
        }
    }
}
/// A framed message and the initialized prefix of the caller's descriptor slots.
/// Descriptors belong to the slots, not this view: use `Option::take` to transfer
/// them or clear the slots to close them before reusing that descriptor storage.
pub struct ScratchMessage<'a> {
    pub body: capnp::message::Reader<capnp_futures::BufferedScratchSegments<'a>>,
    pub fds: &'a mut [Option<OwnedFd>],
}

/// Buffered Unix message input with caller-supplied word and FD storage.
/// Use an owned socket, `Rc`/`Box`, or a borrowed `&UnixStream`; do not read the socket
/// separately while this reader may hold prefetched bytes or descriptors.
/// Canceling a read retains partial input and descriptors for the next read.
/// Dropping the reader closes staged descriptors. The RPC network separately
/// treats cancellation as connection shutdown and continues using owned results.
pub struct FdReader<S> {
    reader: capnp_futures::BufferedRead<AncillarySource<S>>,
    max_fds: usize,
}
impl<S: std::borrow::Borrow<UnixStream> + Unpin> FdReader<S> {
    pub fn new(socket: S, options: Options) -> Self {
        Self::with_buffer_size(socket, options, 8192).unwrap()
    }

    /// Configure receive-buffer words (at least 256), independently of per-read
    /// scratch capacity. Message and descriptor limits still come from `options`.
    pub fn with_buffer_size(socket: S, options: Options, words: usize) -> capnp::Result<Self> {
        let mut reader_options = options.reader;
        reader_options.traversal_limit_in_words = Some(
            reader_options
                .traversal_limit_in_words
                .unwrap_or(usize::MAX)
                .min(options.max_message_words),
        );
        let mut reader = capnp_futures::BufferedRead::with_buffer_size(
            AncillarySource {
                socket,
                limit: Cell::new(options.max_fds.min(253)),
                ancillary: RefCell::new(Ancillary::default()),
            },
            reader_options,
            words,
        )?;
        // Once descriptors arrive, finish only their frame. This keeps retained
        // descriptors bounded per message and avoids truncating the next frame's
        // descriptors against this frame's already-spent budget.
        // Darwin's stream receive boundaries differ from Linux. Read exactly
        // one frame there so ancillary descriptors cannot slide to a later
        // bare frame. Linux retains its qualified coalescing fast path.
        reader.set_read_ahead_policy(|source| {
            cfg!(target_os = "linux") && source.ancillary.borrow().fds.is_empty()
        });
        Ok(Self {
            reader,
            max_fds: options.max_fds.min(253),
        })
    }

    /// Read into reusable output storage. Word scratch follows
    /// `BufferedRead::try_read_message_with_scratch`, including framing words.
    /// All FD slots must initially be `None`; occupied slots or a live shared
    /// message reject the call before consuming input or changing pending FDs.
    ///
    /// Each read receives at most min(slot count, configured max_fds, 253).
    /// Excess descriptors are closed, including previously prefetched ones that
    /// do not fit this call. A previous call with smaller slots may already have
    /// discarded descriptors while prefetching. Errors/EOF/cancellation leave
    /// caller slots empty; partial-read cancellation retains FDs inside the
    /// reader until retry or drop. No descriptor Vec is allocated on this path.
    ///
    /// ```compile_fail
    /// # async fn example(socket: &tokio::net::UnixStream) -> capnp::Result<()> {
    /// let mut stream = reproto::unix_rpc::FdReader::new(socket, Default::default());
    /// let mut words = capnp::Word::allocate_zeroed_vec(256);
    /// let mut slots = [None];
    /// let message = stream.try_read_message_with_scratch(
    ///     &mut words, &mut slots, |_| Ok(false)).await?;
    /// drop(slots); // The returned descriptor view still borrows these slots.
    /// drop(message);
    /// # Ok(()) }
    /// ```
    pub async fn try_read_message_with_scratch<'a>(
        &mut self,
        scratch: &'a mut [capnp::Word],
        fd_space: &'a mut [Option<OwnedFd>],
        is_short_lived: impl FnOnce(
            &capnp::message::Reader<capnp_futures::BufferedSegments>,
        ) -> capnp::Result<bool>,
    ) -> capnp::Result<Option<ScratchMessage<'a>>> {
        if self.reader.has_outstanding_short_lived_message() {
            return Err(Error::failed(
                "previous short-lived message is still alive".into(),
            ));
        }
        if fd_space.iter().any(Option::is_some) {
            return Err(Error::failed(
                "descriptor scratch slots must be empty".into(),
            ));
        }
        let limit = self.max_fds.min(fd_space.len());
        self.reader.get_ref().limit.set(limit);
        let result = self
            .reader
            .try_read_message_with_scratch(scratch, is_short_lived)
            .await;
        let end = self.reader.consumed_bytes();
        let mut ancillary = self.reader.get_ref().ancillary.borrow_mut();
        match result {
            Ok(Some(body)) => {
                let mut count = 0;
                if ancillary.end <= end {
                    for fd in ancillary.fds.drain() {
                        if count < limit {
                            fd_space[count] = Some(fd);
                            count += 1;
                        }
                        // Remaining received descriptors close here.
                    }
                }
                Ok(Some(ScratchMessage {
                    body,
                    fds: &mut fd_space[..count],
                }))
            }
            other => {
                ancillary.fds.clear();
                other.map(|_| None)
            }
        }
    }

    async fn read(&mut self) -> capnp::Result<Option<Input>> {
        if self.reader.has_outstanding_short_lived_message() {
            return Err(Error::failed(
                "previous short-lived message is still alive".into(),
            ));
        }
        self.reader.get_ref().limit.set(self.max_fds);
        let result = self
            .reader
            .try_read_message(|message| {
                // Unrecognized payload shapes are conservatively owned. RPC's
                // dispatcher still validates them before acting on any descriptor.
                Ok(message
                    .get_root()
                    .and_then(capnp_rpc::is_short_lived_rpc_message)
                    .unwrap_or(false))
            })
            .await;
        let end = self.reader.consumed_bytes();
        let mut ancillary = self.reader.get_ref().ancillary.borrow_mut();
        match result {
            Ok(Some(body)) => {
                let fds = if ancillary.end <= end {
                    ancillary.fds.drain().collect()
                } else {
                    vec![]
                };
                Ok(Some(Input { body, fds }))
            }
            other => {
                ancillary.fds.clear();
                other.map(|_| None)
            }
        }
    }
}

// Single-frame fixtures only. Production retains FdReader across messages.
#[cfg(test)]
async fn read_message(socket: &UnixStream, options: Options) -> capnp::Result<Option<Input>> {
    FdReader::new(socket, options).read().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    pub(super) fn descriptor(byte: u8) -> Rc<OwnedFd> {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(&[byte]).unwrap();
        Rc::new(file.into())
    }
    pub(super) fn body(size: u32) -> Body {
        let mut message = Builder::new(HeapAllocator::new().first_segment_words(1));
        message.initn_root::<capnp::data::Builder>(size).fill(19);
        Rc::new(message)
    }
    #[tokio::test(flavor = "current_thread")]
    async fn partial_writes_and_message_boundaries_keep_fds_once() {
        let (sender, receiver) = UnixStream::pair().unwrap();
        let size: libc::c_int = 4096;
        // SAFETY: the option pointer references a live integer of the supplied size.
        assert_eq!(
            unsafe {
                libc::setsockopt(
                    sender.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_SNDBUF,
                    (&size as *const libc::c_int).cast(),
                    mem::size_of_val(&size) as _,
                )
            },
            0
        );
        let message = body(256 * 1024);
        let bytes = capnp::serialize::write_message_to_words(&*message);
        let small = capnp::serialize::write_message_to_words(&*body(8));
        let writer = async {
            // The first message necessarily exceeds the send buffer. The writer
            // must resume without re-attaching its descriptors to later bytes.
            write_message(&sender, &bytes, &[descriptor(1)])
                .await
                .unwrap();
            // Deliberately split the segment header down to a single byte.
            write_message(&sender, &small[..1], &[descriptor(2), descriptor(3)])
                .await
                .unwrap();
            write_message(&sender, &small[1..], &[]).await.unwrap();
        };
        let reader = async {
            let first = read_message(&receiver, Options::default())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(first.fds.len(), 1);
            assert_eq!(
                first.body.get_root::<capnp::data::Reader>().unwrap().len(),
                256 * 1024
            );
            // A one-FD limit also exercises cmsghdr padding which can fit two
            // ints: any extra received descriptor must still be closed.
            let second = read_message(
                &receiver,
                Options {
                    max_fds: 1,
                    ..Options::default()
                },
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(second.fds.len(), 1);
            assert_eq!(
                second.body.get_root::<capnp::data::Reader>().unwrap(),
                [19; 8]
            );
            for fd in first.fds.iter().chain(&second.fds) {
                // SAFETY: descriptor remains owned and open during fcntl.
                assert_ne!(
                    unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
                    0
                );
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            futures::join!(writer, reader);
        })
        .await
        .unwrap();
    }
    #[tokio::test(flavor = "current_thread")]
    async fn queued_writer_reuses_storage_without_repeating_bytes_or_descriptors() {
        use capnp_rpc::VatNetwork as _;
        use std::os::unix::fs::FileExt;
        let (sender, receiver) = UnixStream::pair().unwrap();
        let mut network = VatNetwork::new(sender, Side::Client, Options::default());
        let mut connection = network.connect(Side::Server).unwrap();
        let inputs = [(64, 11), (32, 22), (256 * 1024, 33), (16, 44)];
        let mut replies = Vec::new();
        for (size, byte) in inputs {
            let mut message = connection.new_outgoing_message(1);
            message
                .get_body()
                .unwrap()
                .set_as::<capnp::data::Owned>(&vec![byte; size][..])
                .unwrap();
            message.set_fds(vec![descriptor(byte)]);
            replies.push(message.send().0);
        }
        let shutdown = connection.shutdown(Ok(()));
        let reader = async {
            for (size, byte) in inputs {
                let mut input = read_message(&receiver, Options::default())
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    input.body.get_root::<capnp::data::Reader>().unwrap(),
                    vec![byte; size]
                );
                assert_eq!(input.fds.len(), 1);
                let file = std::fs::File::from(input.fds.remove(0));
                let mut value = [0];
                file.read_exact_at(&mut value, 0).unwrap();
                assert_eq!(value, [byte]);
            }
            assert!(read_message(&receiver, Options::default())
                .await
                .unwrap()
                .is_none());
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let (driven, (), ()) = futures::join!(
                network.drive_until_shutdown(),
                async {
                    for reply in replies {
                        reply.await.unwrap();
                    }
                    shutdown.await.unwrap();
                },
                reader
            );
            driven.unwrap();
        })
        .await
        .unwrap();
    }
    #[tokio::test(flavor = "current_thread")]
    async fn queued_message_owns_fd_after_capability_and_send_promise_drop() {
        use capnp_rpc::VatNetwork as _;
        let (sender, receiver) = UnixStream::pair().unwrap();
        let mut network = VatNetwork::new(sender, Side::Client, Options::default());
        let mut connection = network.connect(Side::Server).unwrap();
        let mut message = connection.new_outgoing_message(1);
        message
            .get_body()
            .unwrap()
            .set_as::<capnp::data::Owned>(&[7][..])
            .unwrap();
        let fd = descriptor(5);
        let weak = Rc::downgrade(&fd);
        message.set_fds(vec![fd]);
        drop(message.send());
        assert!(
            weak.upgrade().is_some(),
            "queued write must retain descriptor ownership"
        );
        let shutdown = connection.shutdown(Ok(()));
        let driver = network.drive_until_shutdown();
        let (sent, drained, received) = futures::join!(
            shutdown,
            driver,
            read_message(&receiver, Options::default())
        );
        sent.unwrap();
        drained.unwrap();
        assert_eq!(received.unwrap().unwrap().fds.len(), 1);
        assert!(
            weak.upgrade().is_none(),
            "completed write must release descriptor ownership"
        );
    }
    #[tokio::test(flavor = "current_thread")]
    async fn canceled_partial_read_closes_connection_and_received_fds() {
        use capnp_rpc::VatNetwork as _;
        let (sender, receiver) = UnixStream::pair().unwrap();
        let mut network = VatNetwork::new(receiver, Side::Server, Options::default());
        let mut connection = network.connect(Side::Client).unwrap();
        let (passed, mut witness) = std::os::unix::net::UnixStream::pair().unwrap();
        witness.set_nonblocking(true).unwrap();
        let passed = Rc::new(OwnedFd::from(passed));
        let bytes = capnp::serialize::write_message_to_words(&*body(16));
        write_message(&sender, &bytes[..8], std::slice::from_ref(&passed))
            .await
            .unwrap();
        drop(passed);
        network.inner.socket.readable().await.unwrap();
        let mut reading = Box::pin(connection.receive_incoming_message());
        assert!(futures::poll!(&mut reading).is_pending());
        drop(reading);
        assert!(connection.receive_incoming_message().await.is_err());
        use std::io::Read;
        assert_eq!(
            witness.read(&mut [0]).unwrap(),
            0,
            "canceled read leaked received descriptor"
        );
    }
    #[tokio::test(flavor = "current_thread")]
    async fn oversized_message_rejected_before_body_allocation() {
        let (sender, receiver) = UnixStream::pair().unwrap();
        let mut header = [0u8; 8];
        header[4..].copy_from_slice(&u32::MAX.to_le_bytes());
        write_message(&sender, &header, &[descriptor(1)])
            .await
            .unwrap();
        assert!(read_message(
            &receiver,
            Options {
                max_message_words: 4,
                ..Options::default()
            }
        )
        .await
        .is_err());
    }
    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "current_thread")]
    async fn replay_tlc_fd_framing_traces() {
        use capnp_rpc::VatNetwork as _;
        use std::io::Read;
        let path = reproto_test_support::verification::input("REPROTO_FD_FRAMING_TRACES")
            .expect("run this test through its verification driver");
        let cases: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        for case in cases {
            let keep = case["keep"].as_bool().unwrap();
            let (sender, receiver) = UnixStream::pair().unwrap();
            let mut sender = Some(sender);
            let mut network = VatNetwork::new(
                receiver,
                Side::Server,
                Options {
                    max_fds: usize::from(keep),
                    ..Options::default()
                },
            );
            let mut connection = network.connect(Side::Client).unwrap();
            let bytes = capnp::serialize::write_message_to_words(&*body(16));
            let (passed, mut witness) = std::os::unix::net::UnixStream::pair().unwrap();
            witness.set_nonblocking(true).unwrap();
            let mut passed = Some(Rc::new(OwnedFd::from(passed)));
            let mut read = None;
            let mut received = vec![];
            for step in case["steps"].as_array().unwrap() {
                match step["action"].as_str().unwrap() {
                    "first" => write_message(
                        sender.as_ref().unwrap(),
                        &bytes[..8],
                        &[passed.take().unwrap()],
                    )
                    .await
                    .unwrap(),
                    "rest" => write_message(sender.as_ref().unwrap(), &bytes[8..], &[])
                        .await
                        .unwrap(),
                    "start" => read = Some(Box::pin(connection.receive_incoming_message())),
                    "prefix" => {
                        network.inner.socket.readable().await.unwrap();
                        assert!(futures::poll!(read.as_mut().unwrap()).is_pending());
                    }
                    "complete" => {
                        let mut message = read.take().unwrap().await.unwrap().unwrap();
                        received = message.take_fds();
                        assert_eq!(
                            message
                                .get_body()
                                .unwrap()
                                .get_as::<capnp::data::Reader>()
                                .unwrap(),
                            [19; 16]
                        );
                    }
                    "cancel" => {
                        drop(read.take().unwrap());
                    }
                    "eof" => {
                        drop(sender.take());
                        let result = read.take().unwrap().await;
                        if step["state"][0].as_u64().unwrap() == 0 {
                            assert!(result.unwrap().is_none());
                        } else {
                            assert!(result.is_err());
                        }
                    }
                    _ => panic!(),
                }
                let state: Vec<u32> = serde_json::from_value(step["state"].clone()).unwrap();
                assert_eq!(network.inner.closed.get(), state[4] != 0);
                assert_eq!(received.len(), state[6] as usize);
                if state[2] != 0 && state[4] != 0 {
                    assert_eq!(
                        witness.read(&mut [0]).unwrap(),
                        0,
                        "canceled prefix retained an FD"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod buffered_tests;
#[cfg(test)]
mod facade_tests;
