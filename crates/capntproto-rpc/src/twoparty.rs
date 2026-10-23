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
use capntproto_rpc_engine::completion::shared as oneshot;
use futures::{AsyncRead, AsyncWrite, Future, FutureExt, TryFutureExt};

use std::cell::RefCell;
use std::rc::{Rc, Weak};

pub use capnp_futures::{OutgoingQueue, OutputSnapshot, QueueSnapshot};

mod facade;
#[cfg(test)]
mod output_driver_tests;
mod send_buffer;
pub use facade::{QueueDiagnostics, ServerDriver, TwoPartyClient, TwoPartyNetwork, TwoPartyServer};
pub use send_buffer::SendBufferWindow;

pub type VatId = crate::rpc_twoparty_capnp::Side;

struct IncomingMessage {
    message: ::capnp::message::StructReaderCache<capnp_futures::BufferedSegments>,
    selection: core::cell::Cell<Option<IncomingPayload>>,
    caps: Vec<Option<Box<dyn capnp::private::capability::ClientHook>>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum IncomingPayload {
    Params,
    Results,
}

struct IncomingStream<T> {
    _connection: Rc<dyn std::any::Any>,
    input: Box<capnp_futures::BufferedRead<T>>,
}

struct OwnedIncomingStream<T> {
    input: Option<Box<capnp_futures::BufferedRead<T>>>,
    return_to: Rc<RefCell<Option<Box<capnp_futures::BufferedRead<T>>>>>,
}
impl<T> Drop for OwnedIncomingStream<T> {
    fn drop(&mut self) {
        // Exclusive ownership leaves this slot empty until we return it.
        *self.return_to.borrow_mut() = self.input.take();
    }
}
impl<T: AsyncRead + Unpin> crate::IncomingMessageStream for OwnedIncomingStream<T> {
    fn poll_receive(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<capnp::Result<Option<Box<dyn crate::IncomingMessage>>>> {
        self.input
            .as_mut()
            .expect("live input owner")
            .poll_read_message(cx, |message| {
                crate::is_short_lived_rpc_message(message.get_root()?)
            })
            .map_ok(|message| {
                message.map(|message| {
                    Box::new(IncomingMessage::new(message)) as Box<dyn crate::IncomingMessage>
                })
            })
    }
}
impl<T: AsyncRead + Unpin> crate::IncomingFrameStream for IncomingStream<T> {
    fn has_buffered_next_frame(&self) -> bool {
        self.input.has_buffered_next_frame()
    }
    fn poll_frame_size(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<capnp::Result<Option<usize>>> {
        self.input.poll_stream_frame_size(cx)
    }
    fn poll_read_frame(
        &mut self,
        cx: &mut std::task::Context<'_>,
        bytes: &mut [u8],
    ) -> std::task::Poll<capnp::Result<usize>> {
        self.input.poll_stream_frame(cx, bytes)
    }
    fn poll_frame(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<capnp::Result<Option<&[u8]>>> {
        self.input.poll_borrowed_frame(cx)
    }
    fn frame(&self) -> Option<&[u8]> {
        self.input.borrowed_frame()
    }
    fn share_frame(&mut self) -> Option<(std::sync::Arc<Vec<u8>>, core::ops::Range<usize>)> {
        self.input.share_borrowed_frame()
    }
    fn finish_frame(&mut self) {
        self.input.finish_borrowed_frame();
    }
}
impl<T: AsyncRead + Unpin> crate::IncomingMessageStream for IncomingStream<T> {
    fn poll_receive(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<capnp::Result<Option<Box<dyn crate::IncomingMessage>>>> {
        self.input
            .poll_read_short_lived_message(cx)
            .map_ok(|message| {
                message.map(|message| {
                    Box::new(IncomingMessage::new(message)) as Box<dyn crate::IncomingMessage>
                })
            })
    }
}

impl IncomingMessage {
    fn new(message: ::capnp::message::Reader<capnp_futures::BufferedSegments>) -> Self {
        Self {
            message: capnp::message::StructReaderCache::new(message),
            selection: core::cell::Cell::new(None),
            caps: Vec::new(),
        }
    }
}

impl capnp::private::capability::ParamsHook for IncomingMessage {
    fn get(&self) -> capnp::Result<capnp::any_pointer::Reader<'_>> {
        use capnp::traits::Imbue;
        let mut content = crate::IncomingMessage::get_params(self)?;
        content.imbue(&self.caps);
        Ok(content)
    }
}
impl crate::IncomingMessage for IncomingMessage {
    fn into_params(
        mut self: Box<Self>,
        caps: Vec<Option<Box<dyn capnp::private::capability::ClientHook>>>,
    ) -> Box<dyn capnp::private::capability::ParamsHook> {
        self.caps = caps;
        self
    }

    fn cache_params(&self, payload: crate::rpc_capnp::payload::Reader<'_>) -> capnp::Result<()> {
        use capnp::traits::IntoInternalStructReader;
        self.message.select(payload.into_internal_struct_reader())?;
        self.selection.set(Some(IncomingPayload::Params));
        Ok(())
    }
    fn get_params(&self) -> capnp::Result<capnp::any_pointer::Reader<'_>> {
        if self.selection.get() == Some(IncomingPayload::Params) {
            if let Some(root) = self.message.get() {
                return Ok(crate::rpc_capnp::payload::Reader::from(root).get_content());
            }
        }
        let root = self
            .get_body()?
            .get_as::<crate::rpc_capnp::message::Reader>()?;
        let crate::rpc_capnp::message::Call(call) = root.which()? else {
            return Err(capnp::Error::failed("request does not contain Call".into()));
        };
        Ok(call?.get_params()?.get_content())
    }
    fn cache_results(&self, payload: crate::rpc_capnp::payload::Reader<'_>) -> capnp::Result<()> {
        use capnp::traits::IntoInternalStructReader;
        self.message.select(payload.into_internal_struct_reader())?;
        self.selection.set(Some(IncomingPayload::Results));
        Ok(())
    }
    fn get_results(&self) -> capnp::Result<capnp::any_pointer::Reader<'_>> {
        if self.selection.get() == Some(IncomingPayload::Results) {
            if let Some(root) = self.message.get() {
                return Ok(crate::rpc_capnp::payload::Reader::from(root).get_content());
            }
        }
        let root = self
            .get_body()?
            .get_as::<crate::rpc_capnp::message::Reader>()?;
        let crate::rpc_capnp::message::Return(ret) = root.which()? else {
            return Err(capnp::Error::failed(
                "response does not contain Return".into(),
            ));
        };
        let crate::rpc_capnp::return_::Results(payload) = ret?.which()? else {
            return Err(capnp::Error::failed(
                "response does not contain Results".into(),
            ));
        };
        Ok(payload?.get_content())
    }
    fn framed_bytes(&self) -> Option<&[u8]> {
        Some(self.message.message().get_segments().framed_bytes())
    }
    fn get_segments(&self) -> Option<&dyn capnp::message::ReaderSegments> {
        Some(self.message.message().get_segments())
    }
    fn size_in_words(&self) -> usize {
        self.message.message().size_in_words()
    }
    fn get_body(&self) -> ::capnp::Result<::capnp::any_pointer::Reader<'_>> {
        self.message.message().get_root()
    }
}

struct BuiltMessage {
    body: Rc<capnp::message::Builder<capnp::message::HeapAllocator>>,
    _guard: Option<Rc<dyn std::any::Any>>,
}
enum QueuedMessage {
    Built(BuiltMessage),
    Owned(QueuedOutgoing),
    Frame(crate::OutgoingFrame),
}
// Recycle only the empty envelope after the physical writer releases it.
// Payloads, admission guards and the sender never enter the cache.
struct QueuedOutgoing(Option<Box<OutgoingMessage>>);
impl std::ops::Deref for QueuedOutgoing {
    type Target = OutgoingMessage;
    fn deref(&self) -> &Self::Target {
        self.0.as_ref().unwrap()
    }
}
impl Drop for QueuedOutgoing {
    fn drop(&mut self) {
        self.0.take().unwrap().recycle();
    }
}
impl capnp_futures::serialize::AsOutputSegments for QueuedMessage {
    fn framed_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Frame(frame) => frame.framed_bytes(),
            _ => None,
        }
    }
    fn size_in_bytes(&self) -> usize {
        match self {
            Self::Built(message) => message.body.size_in_words() * 8,
            Self::Owned(message) => message.message.message().size_in_words() * 8,
            Self::Frame(frame) => frame.size_in_words() * 8,
        }
    }
    fn as_output_segments(&self) -> capnp::OutputSegments<'_> {
        match self {
            Self::Built(message) => message.body.get_segments_for_output(),
            Self::Owned(message) => message.message.message().get_segments_for_output(),
            Self::Frame(frame) => frame.segments(),
        }
    }
}

enum MessageBuilder {
    Root(capnp::message::Builder<capnp::message::HeapAllocator>),
    Call(capnp::message::SelectedStructBuilder<capnp::message::HeapAllocator>),
    Return(capnp::message::SelectedStructBuilder<capnp::message::HeapAllocator>),
}
impl MessageBuilder {
    #[inline]
    fn message(&self) -> &capnp::message::Builder<capnp::message::HeapAllocator> {
        match self {
            Self::Root(message) => message,
            Self::Call(call) | Self::Return(call) => call.message(),
        }
    }
    fn into_inner(self) -> capnp::message::Builder<capnp::message::HeapAllocator> {
        match self {
            Self::Root(message) => message,
            Self::Call(call) | Self::Return(call) => call.into_inner(),
        }
    }
    fn take(&mut self) -> capnp::message::Builder<capnp::message::HeapAllocator> {
        std::mem::replace(self, Self::Root(capnp::message::Builder::new_default())).into_inner()
    }
    fn root(&mut self) -> &mut capnp::message::Builder<capnp::message::HeapAllocator> {
        if !matches!(self, Self::Root(_)) {
            *self = Self::Root(self.take());
        }
        let Self::Root(message) = self else {
            unreachable!()
        };
        message
    }
    #[cold]
    fn select_call(&mut self, initialize: bool) -> capnp::Result<()> {
        let selected = self.take().try_select_struct(|root| {
            let message = root.get_as::<crate::rpc_capnp::message::Builder<'_>>()?;
            let call = if initialize {
                message.init_call()
            } else {
                match message.which()? {
                    crate::rpc_capnp::message::Call(call) => call?,
                    _ => return Err(capnp::Error::failed("request does not contain Call".into())),
                }
            };
            capnp::any_struct::Builder::from_builder(call)
        });
        match selected {
            Ok(call) => *self = Self::Call(call),
            Err((message, error)) => {
                *self = Self::Root(message);
                return Err(error);
            }
        }
        Ok(())
    }

    #[inline(always)]
    fn call(&mut self, initialize: bool) -> capnp::Result<crate::rpc_capnp::call::Builder<'_>> {
        if initialize || !matches!(self, Self::Call(_)) {
            self.select_call(initialize)?;
        }
        let Self::Call(call) = self else {
            unreachable!()
        };
        call.get_as::<crate::rpc_capnp::call::Owned>()
    }

    fn returned(&mut self) -> capnp::Result<crate::rpc_capnp::return_::Builder<'_>> {
        if !matches!(self, Self::Return(_)) {
            let selected = self.take().try_select_struct(|root| {
                let message = root.get_as::<crate::rpc_capnp::message::Builder>()?;
                let crate::rpc_capnp::message::Return(returned) = message.which()? else {
                    return Err(capnp::Error::failed(
                        "response does not contain Return".into(),
                    ));
                };
                capnp::any_struct::Builder::from_builder(returned?)
            });
            match selected {
                Ok(returned) => *self = Self::Return(returned),
                Err((message, error)) => {
                    *self = Self::Root(message);
                    return Err(error);
                }
            }
        }
        let Self::Return(returned) = self else {
            unreachable!()
        };
        returned.get_as::<crate::rpc_capnp::return_::Owned>()
    }
}

type MessageCache = Rc<RefCell<Vec<Box<OutgoingMessage>>>>;
const CACHED_ENVELOPES: usize = 64;
struct OutgoingMessage {
    // Preserve the allocation consumed by the boxed public message hook.
    #[allow(clippy::vec_box)]
    cache: Weak<RefCell<Vec<Box<OutgoingMessage>>>>,
    message: MessageBuilder,
    sender: Option<Rc<capnp_futures::LocalSender<QueuedMessage>>>,
    guard: Option<Rc<dyn std::any::Any>>,
}

impl OutgoingMessage {
    fn acquire(
        cache: &MessageCache,
        message: MessageBuilder,
        sender: Rc<capnp_futures::LocalSender<QueuedMessage>>,
    ) -> Box<Self> {
        let cached = cache.borrow_mut().pop();
        if let Some(mut cached) = cached {
            cached.message = message;
            cached.sender = Some(sender);
            cached
        } else {
            Box::new(Self {
                cache: Rc::downgrade(cache),
                message,
                sender: Some(sender),
                guard: None,
            })
        }
    }
    fn recycle(mut self: Box<Self>) {
        drop(self.message.take());
        drop(self.sender.take());
        drop(self.guard.take());
        if let Some(cache) = self.cache.upgrade() {
            let mut cache = cache.borrow_mut();
            if cache.len() < CACHED_ENVELOPES {
                cache.push(self);
            }
        }
    }
    fn into_parts(
        mut self: Box<Self>,
    ) -> (
        capnp::message::Builder<capnp::message::HeapAllocator>,
        Rc<capnp_futures::LocalSender<QueuedMessage>>,
        Option<Rc<dyn std::any::Any>>,
    ) {
        let body = self.message.take();
        let sender = self.sender.take().expect("unsent message has sender");
        let guard = self.guard.take();
        self.recycle();
        (body, sender, guard)
    }
}

impl crate::OutgoingMessage for OutgoingMessage {
    fn get_body(&mut self) -> ::capnp::Result<::capnp::any_pointer::Builder<'_>> {
        self.message.root().get_root()
    }

    fn get_body_as_reader(&self) -> ::capnp::Result<::capnp::any_pointer::Reader<'_>> {
        self.message.message().get_root_as_reader()
    }

    fn init_call(&mut self) -> capnp::Result<crate::rpc_capnp::call::Builder<'_>> {
        self.message.call(true)
    }
    fn get_call(&mut self) -> capnp::Result<crate::rpc_capnp::call::Builder<'_>> {
        self.message.call(false)
    }
    fn get_return(&mut self) -> capnp::Result<crate::rpc_capnp::return_::Builder<'_>> {
        self.message.returned()
    }

    fn send(
        self: Box<Self>,
    ) -> (
        Promise<(), ::capnp::Error>,
        Rc<::capnp::message::Builder<::capnp::message::HeapAllocator>>,
    ) {
        let (message, sender, guard) = self.into_parts();
        let m = Rc::new(message);
        (
            Promise::from_future(
                sender
                    .send(QueuedMessage::Built(BuiltMessage {
                        body: m.clone(),
                        _guard: guard,
                    }))
                    .map_ok(|_| ()),
            ),
            m,
        )
    }

    fn send_detached(
        self: Box<Self>,
    ) -> Rc<capnp::message::Builder<capnp::message::HeapAllocator>> {
        let (message, sender, guard) = self.into_parts();
        let body = Rc::new(message);
        sender.send_detached(QueuedMessage::Built(BuiltMessage {
            body: body.clone(),
            _guard: guard,
        }));
        body
    }

    fn send_detached_and_drive(mut self: Box<Self>) {
        // Like C++ evalLast(), publish into the FIFO and let the network turn
        // write related messages together. Remove the sender before enqueueing
        // so queued output cannot retain its own queue in a cycle.
        let sender = self.sender.take().expect("unsent message has sender");
        sender.send_detached(QueuedMessage::Owned(QueuedOutgoing(Some(self))));
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
        self.into_parts().0
    }

    fn size_in_words(&self) -> usize {
        self.message.message().size_in_words()
    }
}

// Only the network task owns this future. Cooperative writers may poll it
// through a weak handle, using the network task's registered IO waker. The
// externally shared network completion remains independent of per-frame flush.
struct OutputDriver {
    task: RefCell<Option<Promise<(), capnp::Error>>>,
    result: RefCell<Option<capnp::Result<()>>>,
    network: RefCell<Option<std::task::Waker>>,
    extracted: std::cell::Cell<bool>,
    completion: RefCell<Vec<std::task::Waker>>,
}
impl OutputDriver {
    fn poll_completion(
        &self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<capnp::Result<()>> {
        if let Some(result) = self.result.borrow().as_ref() {
            return std::task::Poll::Ready(result.clone());
        }
        if !self
            .completion
            .borrow()
            .iter()
            .any(|wake| wake.will_wake(cx.waker()))
        {
            self.completion.borrow_mut().push(cx.waker().clone());
        }
        std::task::Poll::Pending
    }
    fn poll_network(&self, cx: &mut std::task::Context<'_>) -> std::task::Poll<capnp::Result<()>> {
        if self
            .network
            .borrow()
            .as_ref()
            .is_none_or(|wake| !wake.will_wake(cx.waker()))
        {
            let wake = cx.waker().clone();
            let old = self.network.replace(Some(wake));
            drop(old);
        }
        self.poll_inner(cx)
    }
    fn poll_inner(&self, cx: &mut std::task::Context<'_>) -> std::task::Poll<capnp::Result<()>> {
        if let Some(result) = self.result.borrow().as_ref() {
            return std::task::Poll::Ready(result.clone());
        }
        let Ok(mut slot) = self.task.try_borrow_mut() else {
            cx.waker().wake_by_ref();
            return std::task::Poll::Pending;
        };
        let result = std::pin::Pin::new(slot.as_mut().expect("live output future")).poll(cx);
        if let std::task::Poll::Ready(result) = &result {
            let finished = slot.take();
            *self.result.borrow_mut() = Some(result.clone());
            drop(slot);
            drop(finished);
            let wake = self.network.borrow_mut().take();
            if let Some(wake) = wake {
                wake.wake();
            }
            let wakes = std::mem::take(&mut *self.completion.borrow_mut());
            for wake in wakes {
                wake.wake();
            }
        }
        result
    }
    fn poll_cooperative(
        &self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<capnp::Result<()>> {
        if self.extracted.get() {
            return self.poll_completion(cx);
        }
        let wake = self.network.borrow().clone();
        match wake {
            Some(wake) => self.poll_inner(&mut std::task::Context::from_waker(&wake)),
            None => self.poll_inner(cx),
        }
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
    sender: Rc<capnp_futures::LocalSender<QueuedMessage>>,
    side: crate::rpc_twoparty_capnp::Side,
    on_disconnect_fulfiller: Option<oneshot::Sender<()>>,
    flow_control: crate::flow_control::Policy,
    tail_call_optimization: bool,
    write_finished: futures::future::Shared<Promise<(), capnp::Error>>,
    output_closed: futures::future::Shared<Promise<(), capnp::Error>>,
    output_driver: Weak<OutputDriver>,
    segments: capnp::message::SegmentPool,
    message_cache: MessageCache,
    call_template:
        Option<capnp::message::SelectedStructBuilderTemplate<capnp::message::HeapAllocator>>,
    return_template:
        Option<capnp::message::SelectedStructBuilderTemplate<capnp::message::HeapAllocator>>,
    finish_template:
        Option<capnp::message::SelectedStructBuilderTemplate<capnp::message::HeapAllocator>>,
}

struct OutgoingStream {
    _connection: Rc<dyn std::any::Any>,
    sender: capnp_futures::LocalObservedSender<QueuedMessage>,
    driver: Weak<OutputDriver>,
    closed: futures::future::Shared<Promise<(), capnp::Error>>,
}
impl crate::OutgoingMessageStream for OutgoingStream {
    fn reserve(&mut self) -> capnp::Result<()> {
        self.sender.reserve()
    }
    fn cancel_reservation(&mut self) {
        self.sender.cancel_reservation();
    }

    fn shutdown(&mut self, result: capnp::Result<()>) -> Promise<(), capnp::Error> {
        let drained = self.sender.terminate(result);
        let closed = self.closed.clone();
        Promise::from_future(async move {
            drained.await?;
            closed.await
        })
    }
    fn publish_reserved(&mut self, frame: crate::OutgoingFrame) -> capnp::Result<()> {
        #[cfg(unix)]
        let frame = {
            let mut frame = frame;
            drop(frame.take_fds());
            frame
        };
        self.sender.publish_reserved(QueuedMessage::Frame(frame))
    }
    fn poll_flush(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<capnp::Result<()>> {
        if let Some(driver) = self.driver.upgrade() {
            if !driver.extracted.get() {
                if let std::task::Poll::Ready(Err(error)) = driver.poll_cooperative(cx) {
                    return std::task::Poll::Ready(Err(error));
                }
            }
        }
        self.sender.poll_flush(cx)
    }
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
    #[allow(clippy::too_many_arguments)]
    fn new(
        input_stream: T,
        sender: capnp_futures::LocalSender<QueuedMessage>,
        side: crate::rpc_twoparty_capnp::Side,
        receive_options: ReaderOptions,
        on_disconnect_fulfiller: oneshot::Sender<()>,
        flow_control: crate::flow_control::Policy,
        write_finished: futures::future::Shared<Promise<(), capnp::Error>>,
        output_closed: futures::future::Shared<Promise<(), capnp::Error>>,
        output_driver: Weak<OutputDriver>,
    ) -> Self {
        Self {
            inner: Rc::new(RefCell::new(ConnectionInner {
                input_stream: Rc::new(RefCell::new(Some(Box::new(
                    // Match C++'s 64 KiB receive buffer so a ready burst is not
                    // split into 8 KiB reads. Larger frames still spill into
                    // their final storage after this bounded prefix.
                    capnp_futures::BufferedRead::with_buffer_size(
                        input_stream,
                        receive_options,
                        8192,
                    )
                    .expect("64 KiB holds every legal RPC segment table"),
                )))),
                sender: Rc::new(sender),
                side,
                on_disconnect_fulfiller: Some(on_disconnect_fulfiller),
                flow_control,
                tail_call_optimization: false,
                write_finished,
                output_closed,
                output_driver,
                // Includes a 64 KiB body plus framing/envelope growth, while
                // bounding retained storage independently of live messages.
                segments: capnp::message::SegmentPool::new(16 * 1024, 64),
                message_cache: Rc::default(),
                call_template: None,
                return_template: None,
                finish_template: None,
            })),
        }
    }
}

impl<T: AsyncRead + Unpin> Connection<T> {
    fn take_outgoing_stream(&mut self) -> Option<Box<dyn crate::OutgoingMessageStream>> {
        let inner = self.inner.borrow();
        inner.sender.observe().map(|sender| {
            Box::new(OutgoingStream {
                _connection: self.inner.clone(),
                sender,
                driver: inner.output_driver.clone(),
                closed: inner.output_closed.clone(),
            }) as Box<dyn crate::OutgoingMessageStream>
        })
    }
    #[cfg(test)]
    fn take_incoming_stream(&mut self) -> Option<Box<dyn crate::IncomingMessageStream>> {
        let input = self.inner.borrow().input_stream.borrow_mut().take()?;
        Some(Box::new(IncomingStream {
            input,
            _connection: self.inner.clone(),
        }))
    }

    fn take_incoming_frames(&mut self) -> Option<Box<dyn crate::IncomingFrameStream>> {
        let input = self.inner.borrow().input_stream.borrow_mut().take()?;
        Some(Box::new(IncomingStream {
            input,
            _connection: self.inner.clone(),
        }))
    }
}

impl<T> crate::Connection<crate::rpc_twoparty_capnp::Side> for Connection<T>
where
    T: AsyncRead + Unpin,
{
    fn control_mode(&self) -> crate::ControlMode {
        crate::ControlMode::TwoParty
    }
    fn take_frame_io(&mut self) -> capnp::Result<crate::FrameIo> {
        let outgoing = self
            .take_outgoing_stream()
            .ok_or_else(|| capnp::Error::failed("output already owned".into()))?;
        let incoming = self
            .take_incoming_frames()
            .ok_or_else(|| capnp::Error::failed("input already owned".into()))?;
        Ok(crate::FrameIo {
            incoming,
            outgoing,
            write_finished: self.when_write_finished(),
        })
    }
    fn get_peer_vat_id(&self) -> crate::rpc_twoparty_capnp::Side {
        match self.inner.borrow().side {
            VatId::Client => VatId::Server,
            VatId::Server => VatId::Client,
        }
    }

    fn connection_id(&self) -> usize {
        Rc::as_ptr(&self.inner) as usize
    }

    fn supports_two_party_join(&self) -> bool {
        true
    }

    fn supports_tail_call_transfer(&self) -> bool {
        self.inner.borrow().tail_call_optimization
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
        OutgoingMessage::acquire(
            &inner.message_cache,
            MessageBuilder::Root(message),
            inner.sender.clone(),
        )
    }

    fn send_frame(&mut self, frame: crate::OutgoingFrame) -> Promise<(), capnp::Error> {
        // Ordinary byte streams do not transport ancillary resources.
        #[cfg(unix)]
        let frame = {
            let mut frame = frame;
            drop(frame.take_fds());
            frame
        };
        let sender = self.inner.borrow().sender.clone();
        Promise::from_future(sender.send(QueuedMessage::Frame(frame)).map_ok(|_| ()))
    }

    fn new_outgoing_call(
        &mut self,
        first_segment_word_size: u32,
    ) -> capnp::Result<Box<dyn crate::OutgoingMessage>> {
        let mut inner = self.inner.borrow_mut();
        if inner.call_template.is_none() {
            let message = capnp::message::Builder::new(
                capnp::message::HeapAllocator::new().first_segment_words(64),
            );
            let selected = message
                .try_select_struct(|root| {
                    let mut call = root
                        .init_as::<crate::rpc_capnp::message::Builder>()
                        .init_call();
                    let _ = call.reborrow().get_target()?;
                    let _ = call.reborrow().get_params()?;
                    capnp::any_struct::Builder::from_builder(call)
                })
                .map_err(|(_, error)| error)?;
            inner.call_template = Some(selected.into_template()?);
        }
        let allocator = capnp::message::HeapAllocator::new()
            .segment_pool(inner.segments.clone())
            .first_segment_words(if first_segment_word_size == 0 {
                256
            } else {
                first_segment_word_size
            });
        let message = inner.call_template.as_ref().unwrap().instantiate(allocator);
        Ok(OutgoingMessage::acquire(
            &inner.message_cache,
            MessageBuilder::Call(message),
            inner.sender.clone(),
        ))
    }

    fn new_outgoing_return(
        &mut self,
        first_segment_word_size: u32,
    ) -> capnp::Result<Box<dyn crate::OutgoingMessage>> {
        let mut inner = self.inner.borrow_mut();
        if inner.return_template.is_none() {
            let message = capnp::message::Builder::new(
                capnp::message::HeapAllocator::new().first_segment_words(16),
            );
            let selected = message
                .try_select_struct(|root| {
                    let mut returned = root
                        .init_as::<crate::rpc_capnp::message::Builder>()
                        .init_return();
                    let _ = returned.reborrow().init_results();
                    capnp::any_struct::Builder::from_builder(returned)
                })
                .map_err(|(_, error)| error)?;
            inner.return_template = Some(selected.into_template()?);
        }
        let allocator = capnp::message::HeapAllocator::new()
            .segment_pool(inner.segments.clone())
            .first_segment_words(if first_segment_word_size == 0 {
                256
            } else {
                first_segment_word_size
            });
        let message = inner
            .return_template
            .as_ref()
            .unwrap()
            .instantiate(allocator);
        Ok(OutgoingMessage::acquire(
            &inner.message_cache,
            MessageBuilder::Return(message),
            inner.sender.clone(),
        ))
    }

    fn new_outgoing_finish(
        &mut self,
        question_id: u32,
        release_result_caps: bool,
    ) -> capnp::Result<Box<dyn crate::OutgoingMessage>> {
        let mut inner = self.inner.borrow_mut();
        if inner.finish_template.is_none() {
            let message = capnp::message::Builder::new(
                capnp::message::HeapAllocator::new().first_segment_words(5),
            );
            let selected = message
                .try_select_struct(|root| {
                    let mut finish = root
                        .init_as::<crate::rpc_capnp::message::Builder>()
                        .init_finish();
                    finish.set_require_early_cancellation_workaround(false);
                    capnp::any_struct::Builder::from_builder(finish)
                })
                .map_err(|(_, error)| error)?;
            inner.finish_template = Some(selected.into_template()?);
        }
        let allocator = capnp::message::HeapAllocator::new()
            .segment_pool(inner.segments.clone())
            .first_segment_words(5);
        let mut message = inner
            .finish_template
            .as_ref()
            .unwrap()
            .instantiate(allocator);
        let mut finish = message.get_as::<crate::rpc_capnp::finish::Owned>()?;
        finish.set_question_id(question_id);
        finish.set_release_result_caps(release_result_caps);
        Ok(OutgoingMessage::acquire(
            &inner.message_cache,
            MessageBuilder::Root(message.into_inner()),
            inner.sender.clone(),
        ))
    }

    fn receive_incoming_message(
        &mut self,
    ) -> Promise<Option<Box<dyn crate::IncomingMessage + 'static>>, ::capnp::Error> {
        self.receive(false)
    }

    fn receive_incoming_message_short_lived(
        &mut self,
    ) -> Promise<Option<Box<dyn crate::IncomingMessage>>, capnp::Error> {
        self.receive(true)
    }

    fn take_owned_incoming_stream(&mut self) -> Option<Box<dyn crate::IncomingMessageStream>> {
        let return_to = self.inner.borrow().input_stream.clone();
        let input = return_to.borrow_mut().take()?;
        Some(Box::new(OwnedIncomingStream {
            input: Some(input),
            return_to,
        }))
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

    fn output_driver(&self) -> Option<Promise<(), capnp::Error>> {
        let driver = self.inner.borrow().output_driver.clone();
        if driver
            .upgrade()
            .is_some_and(|driver| driver.extracted.get())
        {
            return None;
        }
        Some(Promise::from_future(std::future::poll_fn(move |cx| {
            let Some(driver) = driver.upgrade() else {
                return std::task::Poll::Ready(Err(capnp::Error::disconnected(
                    "network driver dropped".into(),
                )));
            };
            driver.poll_cooperative(cx)
        })))
    }

    fn shutdown(&mut self, result: ::capnp::Result<()>) -> Promise<(), ::capnp::Error> {
        let (sender, closed) = {
            let inner = self.inner.borrow();
            (inner.sender.clone(), inner.output_closed.clone())
        };
        let drained = sender.terminate(result);
        Promise::from_future(async move {
            drained.await?;
            closed.await
        })
    }
}

impl<T: AsyncRead + Unpin> Connection<T> {
    fn receive(
        &mut self,
        short_lived: bool,
    ) -> Promise<Option<Box<dyn crate::IncomingMessage>>, capnp::Error> {
        let inner = self.inner.borrow_mut();

        let maybe_input_stream = inner.input_stream.borrow_mut().take();
        let return_it_here = inner.input_stream.clone();
        match maybe_input_stream {
            Some(mut s) => Promise::from_future(async move {
                let result = s
                    .try_read_message(|message| {
                        if short_lived {
                            Ok(true)
                        } else {
                            crate::is_short_lived_rpc_message(message.get_root()?)
                        }
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
    _output_driver: Rc<OutputDriver>,
    output_closed: futures::future::Shared<Promise<(), ::capnp::Error>>,
    write_finished: futures::future::Shared<Promise<(), ::capnp::Error>>,
    outgoing_queue: OutgoingQueue,
    side: crate::rpc_twoparty_capnp::Side,
    task_yield: fn(&mut std::task::Context<'_>),
}

/// A two-party vat `VatNetwork` implementation.
impl<T> VatNetwork<T>
where
    T: AsyncRead + Unpin,
{
    /// Use the host executor's deferred wake queue for RPC output turns.
    /// Configure before constructing RpcSystem. See [`crate::VatNetwork::task_yield`].
    pub fn set_task_yield(&mut self, task_yield: fn(&mut std::task::Context<'_>)) {
        self.task_yield = task_yield;
    }

    /// Enable optimized tail-result transfer for a peer known to support it
    /// (for example C++ or capntproto). Disabled by default for interoperability
    /// with capnp-rpc 0.27.0, whose redirected answers can remain unpolled.
    /// Tail calls still work when disabled, using ordinary call/reply forwarding.
    /// Incoming transfers remain supported in either mode. This is not negotiated
    /// on the wire and must reflect the actual peer's implementation.
    pub fn set_tail_call_optimization(&mut self, enabled: bool) {
        if let Some(inner) = self.weak_connection_inner.upgrade() {
            inner.borrow_mut().tail_call_optimization = enabled;
        }
    }

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

    /// Configure exact-body read hints before the RPC system takes input.
    /// The transport may use the remaining byte count for readiness, but must
    /// not wait for more bytes than requested. Ordinary reads use one.
    pub fn set_read_hint(
        &mut self,
        hint: fn(&mut T, usize) -> std::io::Result<()>,
    ) -> capnp::Result<()> {
        let inner = self
            .weak_connection_inner
            .upgrade()
            .ok_or_else(|| capnp::Error::disconnected("connection no longer exists".into()))?;
        let input = inner.borrow().input_stream.clone();
        let mut input = input.borrow_mut();
        let input = input
            .as_mut()
            .ok_or_else(|| capnp::Error::failed("receive stream already extracted".into()))?;
        input.set_read_hint(hint);
        Ok(())
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
        let (sender, write_queue) = ::capnp_futures::write_queue_local_with_clock(output, clock);
        let outgoing_queue = sender.outgoing_queue();
        let execution = Promise::from_future(async move {
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
        });
        let output_driver = Rc::new(OutputDriver {
            task: RefCell::new(Some(execution)),
            result: RefCell::new(None),
            network: RefCell::new(None),
            extracted: std::cell::Cell::new(false),
            completion: RefCell::new(Vec::new()),
        });
        let network_driver = output_driver.clone();
        let execution_driver = Promise::from_future(std::future::poll_fn(move |cx| {
            network_driver.poll_network(cx)
        }))
        .shared();
        let connection = Connection::new(
            input_stream,
            sender,
            side,
            receive_options,
            fulfiller,
            crate::flow_control::Policy::Variable(Rc::new(move || window.get())),
            write_finished.clone(),
            output_closed.clone(),
            Rc::downgrade(&output_driver),
        );
        let weak_inner = Rc::downgrade(&connection.inner);
        Self {
            connection: Some(connection),
            weak_connection_inner: weak_inner,
            execution_driver,
            _output_driver: output_driver,
            output_closed,
            write_finished,
            outgoing_queue,
            side,
            task_yield: |cx| cx.waker().wake_by_ref(),
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
    fn take_output_driver(&mut self) -> Option<Promise<(), capnp::Error>> {
        if self._output_driver.extracted.replace(true) {
            return None;
        }
        Some(Promise::from_future(self.execution_driver.clone()))
    }

    fn task_yield(&self) -> fn(&mut std::task::Context<'_>) {
        self.task_yield
    }

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
        if self._output_driver.extracted.get() {
            let output = self._output_driver.clone();
            return Promise::from_future(std::future::poll_fn(move |cx| {
                output.poll_completion(cx)
            }));
        }
        let driver = self.execution_driver.clone();
        let output = self._output_driver.clone();
        Promise::from_future(async move {
            // Callers may drop VatNetwork after extracting its task and
            // connection. The task then owns cooperative write progress too.
            let _output = output;
            driver.await
        })
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

#[cfg(test)]
mod receive_lifetime_tests {
    use super::*;
    use crate::Connection as _;

    #[test]
    fn lifetime_classifier_matches_generated_union_tags() {
        for tag in 0..=20u16 {
            let mut message = capnp::message::Builder::new_default();
            let mut root = message
                .init_root::<capnp::any_pointer::Builder>()
                .init_as_any_struct(1, 1);
            root.get_data_section()[..2].copy_from_slice(&tag.to_le_bytes());
            let body = message
                .get_root_as_reader::<capnp::any_pointer::Reader>()
                .unwrap();
            let expected = !matches!(
                body.get_as::<crate::rpc_capnp::message::Reader>()
                    .unwrap()
                    .which(),
                Ok(crate::rpc_capnp::message::Call(_) | crate::rpc_capnp::message::Return(_))
            );
            assert_eq!(
                crate::is_short_lived_rpc_message(body).unwrap(),
                expected,
                "tag {tag}"
            );
        }
    }

    #[test]
    fn owned_stream_returns_partial_input_and_retained_calls_survive_later_reads() {
        use std::{
            cell::Cell,
            pin::Pin,
            task::{Context, Poll, Waker},
        };
        struct Input {
            bytes: Vec<u8>,
            position: usize,
            available: Rc<Cell<usize>>,
            dropped: Rc<Cell<bool>>,
        }
        impl Drop for Input {
            fn drop(&mut self) {
                self.dropped.set(true);
            }
        }
        impl AsyncRead for Input {
            fn poll_read(
                mut self: Pin<&mut Self>,
                _: &mut Context<'_>,
                out: &mut [u8],
            ) -> Poll<std::io::Result<usize>> {
                let end = self.bytes.len().min(self.available.get());
                if self.position == end && end < self.bytes.len() {
                    return Poll::Pending;
                }
                let count = out.len().min(end - self.position);
                out[..count].copy_from_slice(&self.bytes[self.position..self.position + count]);
                self.position += count;
                Poll::Ready(Ok(count))
            }
        }
        let mut bytes = Vec::new();
        for id in [17, 29, 41] {
            let mut message = capnp::message::Builder::new_default();
            message
                .init_root::<crate::rpc_capnp::message::Builder<'_>>()
                .init_call()
                .set_question_id(id);
            capnp::serialize::write_message(&mut bytes, &message).unwrap();
        }
        let available = Rc::new(Cell::new(5));
        let dropped = Rc::new(Cell::new(false));
        let mut network = VatNetwork::new(
            Input {
                bytes,
                position: 0,
                available: available.clone(),
                dropped: dropped.clone(),
            },
            futures::io::sink(),
            VatId::Server,
            Default::default(),
        );
        let mut connection = network.connection.take().unwrap();
        let mut stream = connection.take_owned_incoming_stream().unwrap();
        assert!(connection.take_owned_incoming_stream().is_none());
        assert!(connection.take_incoming_stream().is_none());
        assert!(connection.take_incoming_frames().is_none());
        assert!(futures::executor::block_on(connection.receive(false)).is_err());
        let mut cx = Context::from_waker(Waker::noop());
        assert!(stream.poll_receive(&mut cx).is_pending());
        drop(stream);
        assert!(
            !dropped.get(),
            "input must survive cancellation until output drain"
        );
        available.set(usize::MAX);
        let first = futures::executor::block_on(connection.receive(false))
            .unwrap()
            .unwrap();
        let mut stream = connection.take_owned_incoming_stream().unwrap();
        let mut retained = vec![first];
        for _ in 0..2 {
            let Poll::Ready(Ok(Some(message))) = stream.poll_receive(&mut cx) else {
                panic!()
            };
            retained.push(message);
        }
        assert!(matches!(
            stream.poll_receive(&mut cx),
            Poll::Ready(Ok(None))
        ));
        drop(stream);
        assert!(futures::executor::block_on(connection.receive(false))
            .unwrap()
            .is_none());
        drop((connection, network));
        assert!(dropped.get());
        for (message, id) in retained.iter().zip([17, 29, 41]) {
            let root = message
                .get_body()
                .unwrap()
                .get_as::<crate::rpc_capnp::message::Reader<'_>>()
                .unwrap();
            let crate::rpc_capnp::message::Call(call) = root.which().unwrap() else {
                panic!()
            };
            assert_eq!(call.unwrap().get_question_id(), id);
        }
    }

    #[test]
    fn reusable_stream_exclusively_owns_input_and_messages_survive_its_drop() {
        let mut builder = capnp::message::Builder::new_default();
        builder.set_root("retained input").unwrap();
        let bytes = capnp::serialize::write_message_to_words(&builder);
        let mut network = VatNetwork::new(
            futures::io::Cursor::new(bytes.clone()),
            futures::io::sink(),
            VatId::Server,
            Default::default(),
        );
        let mut connection = network.connection.take().unwrap();
        let mut stream = connection.take_incoming_stream().unwrap();
        assert!(connection.take_incoming_stream().is_none());
        assert!(futures::executor::block_on(connection.receive(true)).is_err());
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        let std::task::Poll::Ready(Ok(Some(message))) = stream.poll_receive(&mut cx) else {
            panic!()
        };
        assert_eq!(message.framed_bytes().unwrap(), bytes);
        assert!(matches!(
            stream.poll_receive(&mut cx),
            std::task::Poll::Ready(Err(_))
        ));
        drop((stream, connection, network));
        assert_eq!(
            message
                .get_body()
                .unwrap()
                .get_as::<capnp::text::Reader>()
                .unwrap(),
            "retained input"
        );
    }

    #[test]
    fn transient_calls_lend_storage_and_reject_reuse_until_released() {
        check(true);
    }

    #[test]
    fn ordinary_calls_remain_independent_of_subsequent_receives() {
        check(false);
    }

    fn check(short_lived: bool) {
        futures::executor::block_on(async {
            let mut bytes = Vec::new();
            for id in [17, 29] {
                let mut message = capnp::message::Builder::new_default();
                message
                    .init_root::<crate::rpc_capnp::message::Builder<'_>>()
                    .init_call()
                    .set_question_id(id);
                capnp::serialize::write_message(&mut bytes, &message).unwrap();
            }
            let mut network = VatNetwork::new(
                futures::io::Cursor::new(bytes),
                futures::io::Cursor::new(Vec::<u8>::new()),
                VatId::Server,
                Default::default(),
            );
            let mut connection = network.connection.take().unwrap();
            let first = connection.receive(short_lived).await.unwrap().unwrap();
            assert_eq!(
                connection
                    .inner
                    .borrow()
                    .input_stream
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .has_outstanding_short_lived_message(),
                short_lived,
            );
            let question = |message: &dyn crate::IncomingMessage| {
                let root = message
                    .get_body()
                    .unwrap()
                    .get_as::<crate::rpc_capnp::message::Reader<'_>>()
                    .unwrap();
                let crate::rpc_capnp::message::Call(call) = root.which().unwrap() else {
                    panic!("expected Call")
                };
                call.unwrap().get_question_id()
            };
            assert_eq!(question(&*first), 17);
            if short_lived {
                assert!(connection
                    .receive_incoming_message_short_lived()
                    .await
                    .is_err());
                assert_eq!(question(&*first), 17, "rejected reuse preserves storage");
                drop(first);
                let second = connection
                    .receive_incoming_message_short_lived()
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(question(&*second), 29);
                drop(second);
            } else {
                let second = connection
                    .receive_incoming_message()
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(question(&*second), 29);
                assert_eq!(
                    question(&*first),
                    17,
                    "owned Call survives the next receive"
                );
            }
            assert!(connection
                .receive_incoming_message()
                .await
                .unwrap()
                .is_none());
        });
    }
}
