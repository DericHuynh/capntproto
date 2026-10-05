// Copyright (c) 2015 Sandstorm Development Group, Inc. and contributors
// Licensed under the MIT License:
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

//! An implementation of [`VatNetwork`](crate::VatNetwork) for the common case
//! of a client-server connection.

use capnp::capability::Promise;
use capnp::message::ReaderOptions;
use futures::channel::oneshot;
use futures::{AsyncRead, AsyncWrite, FutureExt, TryFutureExt};

use std::cell::RefCell;
use std::rc::{Rc, Weak};

pub use capnp_futures::{OutgoingQueue, OutputSnapshot, QueueSnapshot};

mod facade;
mod send_buffer;
pub use facade::{QueueDiagnostics, ServerDriver, TwoPartyClient, TwoPartyNetwork, TwoPartyServer};
pub use send_buffer::SendBufferWindow;

pub type VatId = crate::rpc_twoparty_capnp::Side;

struct IncomingMessage {
    message: ::capnp::message::Reader<capnp_futures::BufferedSegments>,
}

impl IncomingMessage {
    fn new(message: ::capnp::message::Reader<capnp_futures::BufferedSegments>) -> Self {
        Self { message }
    }
}

impl crate::IncomingMessage for IncomingMessage {
    fn size_in_words(&self) -> usize {
        self.message.size_in_words()
    }
    fn get_body(&self) -> ::capnp::Result<::capnp::any_pointer::Reader<'_>> {
        self.message.get_root()
    }
}

struct QueuedMessage {
    body: Rc<capnp::message::Builder<capnp::message::HeapAllocator>>,
    _guard: Option<Rc<dyn std::any::Any>>,
}
impl capnp_futures::serialize::AsOutputSegments for QueuedMessage {
    fn as_output_segments(&self) -> capnp::OutputSegments<'_> {
        self.body.get_segments_for_output()
    }
}

struct OutgoingMessage {
    message: ::capnp::message::Builder<::capnp::message::HeapAllocator>,
    sender: capnp_futures::Sender<QueuedMessage>,
    guard: Option<Rc<dyn std::any::Any>>,
}

impl crate::OutgoingMessage for OutgoingMessage {
    fn get_body(&mut self) -> ::capnp::Result<::capnp::any_pointer::Builder<'_>> {
        self.message.get_root()
    }

    fn get_body_as_reader(&self) -> ::capnp::Result<::capnp::any_pointer::Reader<'_>> {
        self.message.get_root_as_reader()
    }

    fn send(
        self: Box<Self>,
    ) -> (
        Promise<(), ::capnp::Error>,
        Rc<::capnp::message::Builder<::capnp::message::HeapAllocator>>,
    ) {
        let tmp = *self;
        let Self {
            message,
            mut sender,
            guard,
        } = tmp;
        let m = Rc::new(message);
        (
            Promise::from_future(
                sender
                    .send(QueuedMessage {
                        body: m.clone(),
                        _guard: guard,
                    })
                    .map_ok(|_| ()),
            ),
            m,
        )
    }

    fn send_detached(
        self: Box<Self>,
    ) -> Rc<capnp::message::Builder<capnp::message::HeapAllocator>> {
        let Self {
            message,
            mut sender,
            guard,
        } = *self;
        let body = Rc::new(message);
        sender.send_detached(QueuedMessage {
            body: body.clone(),
            _guard: guard,
        });
        body
    }

    fn retain_until_sent(
        &mut self,
        guard: Rc<dyn std::any::Any>,
    ) -> Result<(), Rc<dyn std::any::Any>> {
        if self.guard.is_some() {
            return Err(guard);
        }
        self.guard = Some(guard);
        Ok(())
    }

    fn take(self: Box<Self>) -> ::capnp::message::Builder<::capnp::message::HeapAllocator> {
        self.message
    }

    fn size_in_words(&self) -> usize {
        self.message.size_in_words()
    }
}

struct ConnectionInner<T>
where
    T: AsyncRead + 'static,
{
    // Move only this owner into each receive future; the framing state and
    // partial message stay in place across calls. Canceling a receive still
    // drops the input and prevents another read from a partial frame.
    input_stream: Rc<RefCell<Option<Box<capnp_futures::BufferedRead<T>>>>>,
    sender: capnp_futures::Sender<QueuedMessage>,
    side: crate::rpc_twoparty_capnp::Side,
    on_disconnect_fulfiller: Option<oneshot::Sender<()>>,
    flow_control: crate::flow_control::Policy,
    write_finished: futures::future::Shared<Promise<(), capnp::Error>>,
    segments: capnp::message::SegmentPool,
}

struct Connection<T>
where
    T: AsyncRead + 'static,
{
    inner: Rc<RefCell<ConnectionInner<T>>>,
}

impl<T> Drop for ConnectionInner<T>
where
    T: AsyncRead,
{
    fn drop(&mut self) {
        match self.on_disconnect_fulfiller.take() {
            Some(fulfiller) => {
                let _ = fulfiller.send(());
            }
            None => unreachable!(),
        }
    }
}

impl<T> Connection<T>
where
    T: AsyncRead + Unpin,
{
    fn new(
        input_stream: T,
        sender: capnp_futures::Sender<QueuedMessage>,
        side: crate::rpc_twoparty_capnp::Side,
        receive_options: ReaderOptions,
        on_disconnect_fulfiller: oneshot::Sender<()>,
        flow_control: crate::flow_control::Policy,
        write_finished: futures::future::Shared<Promise<(), capnp::Error>>,
    ) -> Self {
        Self {
            inner: Rc::new(RefCell::new(ConnectionInner {
                input_stream: Rc::new(RefCell::new(Some(Box::new(
                    // Keep ordinary control frames buffered, but spill larger
                    // frames into their final storage after a bounded prefix.
                    capnp_futures::BufferedRead::with_buffer_size(
                        input_stream,
                        receive_options,
                        1024,
                    )
                    .expect("8 KiB holds every legal RPC segment table"),
                )))),
                sender,
                side,
                on_disconnect_fulfiller: Some(on_disconnect_fulfiller),
                flow_control,
                write_finished,
                // Includes a 64 KiB body plus framing/envelope growth, while
                // bounding retained storage independently of live messages.
                segments: capnp::message::SegmentPool::new(16 * 1024, 16),
            })),
        }
    }
}

impl<T> crate::Connection<crate::rpc_twoparty_capnp::Side> for Connection<T>
where
    T: AsyncRead + Unpin,
{
    fn get_peer_vat_id(&self) -> crate::rpc_twoparty_capnp::Side {
        self.inner.borrow().side
    }

    fn connection_id(&self) -> usize {
        Rc::as_ptr(&self.inner) as usize
    }

    fn supports_two_party_join(&self) -> bool {
        true
    }

    fn new_outgoing_message(
        &mut self,
        first_segment_word_size: u32,
    ) -> Box<dyn crate::OutgoingMessage> {
        // Zero means no hint, not a zero-word first segment. A bounded 2 KiB
        // default fits small results without clearing an 8 KiB arena each time.
        // Larger bodies still grow normally; explicit hints remain authoritative.
        let inner = self.inner.borrow();
        let allocator = ::capnp::message::HeapAllocator::new()
            .segment_pool(inner.segments.clone())
            .first_segment_words(if first_segment_word_size == 0 {
                256
            } else {
                first_segment_word_size
            });
        let message = ::capnp::message::Builder::new(allocator);
        Box::new(OutgoingMessage {
            message,
            sender: inner.sender.clone(),
            guard: None,
        })
    }

    fn receive_incoming_message(
        &mut self,
    ) -> Promise<Option<Box<dyn crate::IncomingMessage + 'static>>, ::capnp::Error> {
        let inner = self.inner.borrow_mut();

        let maybe_input_stream = inner.input_stream.borrow_mut().take();
        let return_it_here = inner.input_stream.clone();
        match maybe_input_stream {
            Some(mut s) => Promise::from_future(async move {
                let result = s
                    .try_read_message(|message| {
                        crate::is_short_lived_rpc_message(message.get_root()?)
                    })
                    .await;
                *return_it_here.borrow_mut() = Some(s);
                Ok(result?.map(|message| {
                    Box::new(IncomingMessage::new(message)) as Box<dyn crate::IncomingMessage>
                }))
            }),
            None => Promise::err(::capnp::Error::failed(
                "incoming read already in progress or canceled".to_string(),
            )),
        }
    }

    fn new_stream(&mut self) -> (Box<dyn crate::FlowController>, Promise<(), capnp::Error>) {
        let policy = self.inner.borrow().flow_control.clone();
        policy.controller()
    }

    fn when_write_finished(&self) -> Option<Promise<(), capnp::Error>> {
        Some(Promise::from_future(
            self.inner.borrow().write_finished.clone(),
        ))
    }

    fn shutdown(&mut self, result: ::capnp::Result<()>) -> Promise<(), ::capnp::Error> {
        Promise::from_future(self.inner.borrow_mut().sender.terminate(result))
    }
}

/// A vat network with two parties, the client and the server.
pub struct VatNetwork<T>
where
    T: AsyncRead + 'static + Unpin,
{
    // connection handle that we will return on accept()
    connection: Option<Connection<T>>,

    // connection handle that we will return on connect()
    weak_connection_inner: Weak<RefCell<ConnectionInner<T>>>,

    execution_driver: futures::future::Shared<Promise<(), ::capnp::Error>>,
    output_closed: futures::future::Shared<Promise<(), ::capnp::Error>>,
    write_finished: futures::future::Shared<Promise<(), ::capnp::Error>>,
    outgoing_queue: OutgoingQueue,
    side: crate::rpc_twoparty_capnp::Side,
}

/// A two-party vat `VatNetwork` implementation.
impl<T> VatNetwork<T>
where
    T: AsyncRead + Unpin,
{
    /// Resolves when the output queue terminates and the write half is closed.
    /// Poll the network driver concurrently. Unlike drive_until_shutdown(), this
    /// fence does not wait for the read half or connection handles to disappear.
    /// It confirms local output closure, never peer receipt or RPC execution.
    pub fn output_closed(&self) -> futures::future::Shared<Promise<(), capnp::Error>> {
        self.output_closed.clone()
    }

    /// Resolves when queued writes finish, before closing the transport. A write
    /// failure is observable even if close blocks. This is not a closure fence
    /// or a peer acknowledgement. Poll the network driver concurrently.
    pub fn when_write_finished(&self) -> futures::future::Shared<Promise<(), capnp::Error>> {
        self.write_finished.clone()
    }

    /// Observe queued messages, excluding the active write batch. This handle
    /// remains usable after moving the network into RpcSystem and does not keep
    /// the connection alive. Byte counts exclude stream framing.
    pub fn outgoing_queue(&self) -> OutgoingQueue {
        self.outgoing_queue.clone()
    }

    pub fn get_current_queue_size(&self) -> usize {
        self.outgoing_queue.snapshot().bytes
    }

    pub fn get_current_queue_count(&self) -> usize {
        self.outgoing_queue.snapshot().message_count
    }

    pub fn get_outgoing_message_wait_time(&self) -> std::time::Duration {
        self.outgoing_queue.snapshot().wait_time
    }

    /// Creates a new two-party vat network that will receive data on `input_stream` and send data on
    /// `output_stream`. (Typically, performance is best if these streams are buffered, possibly via
    /// `futures::io::BufReader` and `futures::io::BufWriter`.)
    ///
    /// `side` indicates whether this is the client or the server side of the connection. This has no
    /// effect on the data sent over the connection; it merely exists so that `RpcNetwork::bootstrap` knows
    /// whether to return the local or the remote bootstrap capability. `VatId` parameters like this one
    /// will make more sense once we have vat networks with more than two parties.
    ///
    /// The options in `receive_options` will be used when reading the messages that come in on `input_stream`.
    pub fn new<U>(
        input_stream: T,
        output_stream: U,
        side: crate::rpc_twoparty_capnp::Side,
        receive_options: ReaderOptions,
    ) -> Self
    where
        U: AsyncWrite + 'static + Unpin,
    {
        let start = std::time::Instant::now();
        Self::new_with_clock(
            input_stream,
            output_stream,
            side,
            receive_options,
            move || start.elapsed(),
        )
    }

    /// Creates a network using a monotonic elapsed-time clock for queue metrics.
    pub fn new_with_clock<U>(
        input_stream: T,
        output_stream: U,
        side: crate::rpc_twoparty_capnp::Side,
        receive_options: ReaderOptions,
        clock: impl Fn() -> std::time::Duration + Send + Sync + 'static,
    ) -> Self
    where
        U: AsyncWrite + 'static + Unpin,
    {
        Self::with_send_buffer_and_clock(
            input_stream,
            output_stream,
            side,
            receive_options,
            |_| None,
            clock,
        )
    }

    /// Creates a transport-informed variable window. The query samples the live
    /// output stream on sends and successful acknowledgements when credit needs
    /// a window (in-flight bytes exceed the largest message). Return `None` if
    /// unavailable or on error; the connection then permanently uses 64 KiB.
    /// The query must be synchronous and must not reenter the output stream.
    /// It should inspect its argument rather than capture transport ownership.
    /// Controllers hold only a weak reference to output, so pending acks cannot
    /// keep the socket alive after its network driver and connections are gone.
    pub fn new_with_send_buffer<U>(
        input_stream: T,
        output_stream: U,
        side: VatId,
        receive_options: ReaderOptions,
        get_send_buffer: impl Fn(&U) -> Option<usize> + 'static,
    ) -> Self
    where
        U: AsyncWrite + 'static + Unpin,
    {
        let start = std::time::Instant::now();
        Self::with_send_buffer_and_clock(
            input_stream,
            output_stream,
            side,
            receive_options,
            get_send_buffer,
            move || start.elapsed(),
        )
    }

    fn with_send_buffer_and_clock<U>(
        input_stream: T,
        output_stream: U,
        side: VatId,
        receive_options: ReaderOptions,
        get_send_buffer: impl Fn(&U) -> Option<usize> + 'static,
        clock: impl Fn() -> std::time::Duration + Send + Sync + 'static,
    ) -> Self
    where
        U: AsyncWrite + 'static + Unpin,
    {
        let (fulfiller, disconnect_promise) = oneshot::channel();
        let disconnect_promise =
            disconnect_promise.map_err(|_| ::capnp::Error::disconnected("disconnected".into()));

        let output = SharedOutput(Rc::new(RefCell::new(output_stream)));
        let weak_output = Rc::downgrade(&output.0);
        let window = SendBufferWindow::new(move || {
            let output = weak_output.upgrade()?;
            let output = output.borrow();
            get_send_buffer(&output)
        });
        let mut closer = output.clone();
        let (written_tx, written_rx) = oneshot::channel();
        let write_finished = Promise::from_future(async move {
            written_rx
                .await
                .map_err(|_| capnp::Error::disconnected("output driver canceled".into()))?
        })
        .shared();
        let (closed_tx, closed_rx) = oneshot::channel();
        let output_closed = Promise::from_future(async move {
            closed_rx
                .await
                .map_err(|_| capnp::Error::disconnected("output driver canceled".into()))?
        })
        .shared();
        let (sender, write_queue) = ::capnp_futures::write_queue_with_clock(output, clock);
        let outgoing_queue = sender.outgoing_queue();
        let execution_driver = Promise::from_future(async move {
            // Report write failure promptly, while preserving the distinct
            // physical closure fence and connection lifetime below.
            let written = write_queue.await;
            let _ = written_tx.send(written.clone());
            let closed = futures::AsyncWriteExt::close(&mut closer)
                .await
                .map_err(capnp::Error::from);
            let result = written.and(closed);
            let _ = closed_tx.send(result.clone());
            let _ = disconnect_promise.await;
            result
        })
        .shared();

        let connection = Connection::new(
            input_stream,
            sender,
            side,
            receive_options,
            fulfiller,
            crate::flow_control::Policy::Variable(Rc::new(move || window.get())),
            write_finished.clone(),
        );
        let weak_inner = Rc::downgrade(&connection.inner);
        Self {
            connection: Some(connection),
            weak_connection_inner: weak_inner,
            execution_driver,
            output_closed,
            write_finished,
            outgoing_queue,
            side,
        }
    }

    /// Select a fixed byte window for subsequently created streams. Existing
    /// streams retain their policy. This also works after connect/accept.
    pub fn set_window_size(&mut self, window_size: usize) {
        self.set_flow_control(crate::flow_control::Policy::Fixed(window_size));
    }

    /// Select a shared window getter for subsequently created streams. Each
    /// running stream samples it on sends and successful acknowledgements when
    /// in-flight bytes exceed its largest message. The generic
    /// byte-stream adapter can instead use `new_with_send_buffer` for a transport
    /// query with cached fallback. Existing streams retain their policy.
    pub fn set_variable_window(&mut self, get_window: impl Fn() -> usize + 'static) {
        self.set_flow_control(crate::flow_control::Policy::Variable(Rc::new(get_window)));
    }

    /// Select an independent adaptive controller for each subsequently created
    /// stream. Uses the system monotonic clock; existing streams keep their policy.
    pub fn set_adaptive_window(&mut self, initial_window: usize) {
        self.set_flow_control(crate::flow_control::Policy::Adaptive(initial_window));
    }

    fn set_flow_control(&mut self, policy: crate::flow_control::Policy) {
        if let Some(inner) = self.weak_connection_inner.upgrade() {
            inner.borrow_mut().flow_control = policy;
        }
    }
}

impl<T> crate::VatNetwork<VatId> for VatNetwork<T>
where
    T: AsyncRead + Unpin,
{
    fn connect(&mut self, host_id: VatId) -> Option<Box<dyn crate::Connection<VatId>>> {
        if host_id == self.side {
            None
        } else {
            match self.weak_connection_inner.upgrade() {
                Some(connection_inner) => Some(Box::new(Connection {
                    inner: connection_inner,
                })),
                None => {
                    panic!("tried to reconnect a disconnected twoparty vat network.")
                }
            }
        }
    }

    fn accept(&mut self) -> Promise<Box<dyn crate::Connection<VatId>>, ::capnp::Error> {
        match self.connection.take() {
            Some(c) => Promise::ok(Box::new(c) as Box<dyn crate::Connection<VatId>>),
            None => Promise::from_future(::futures::future::pending()),
        }
    }

    fn drive_until_shutdown(&mut self) -> Promise<(), ::capnp::Error> {
        Promise::from_future(self.execution_driver.clone())
    }
}

// Owned by the queue and its driver only. No external handle can write past the
// queue terminator. The driver closes this half before waiting for disconnect.
struct SharedOutput<W>(Rc<RefCell<W>>);
impl<W> Clone for SharedOutput<W> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<W: AsyncWrite + Unpin> AsyncWrite for SharedOutput<W> {
    fn poll_write_vectored(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buffers: &[std::io::IoSlice<'_>],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut *self.0.borrow_mut()).poll_write_vectored(cx, buffers)
    }

    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut *self.0.borrow_mut()).poll_write(cx, bytes)
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut *self.0.borrow_mut()).poll_flush(cx)
    }
    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut *self.0.borrow_mut()).poll_close(cx)
    }
}
