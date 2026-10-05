// Copyright (c) 2013-2017 Sandstorm Development Group, Inc. and contributors
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

use capnp::capability::CallHints;
use capnp::capability::{self, Promise};
use capnp::private::capability::{
    ClientHook, ParamsHook, PipelineHook, PipelineOp, RequestHook, ResponseHook, ResultsHook,
};
use capnp::traits::{Imbue, ImbueMut};
use capnp::Error;
use capnp::{any_pointer, message};

use futures::channel::oneshot;
use futures::FutureExt;
use futures::TryFutureExt;

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll};

pub trait ResultsDoneHook {
    fn add_ref(&self) -> Box<dyn ResultsDoneHook>;
    fn get(&self) -> ::capnp::Result<any_pointer::Reader<'_>>;
}

impl Clone for Box<dyn ResultsDoneHook> {
    fn clone(&self) -> Self {
        self.add_ref()
    }
}

pub(crate) struct Response {
    results: Box<dyn ResultsDoneHook>,
}

impl Response {
    fn new(results: Box<dyn ResultsDoneHook>) -> Self {
        Self { results }
    }
}

impl ResponseHook for Response {
    fn get(&self) -> ::capnp::Result<any_pointer::Reader<'_>> {
        self.results.get()
    }
}

struct Params {
    request: message::Builder<message::HeapAllocator>,
    cap_table: Vec<Option<Box<dyn ClientHook>>>,
}

impl Params {
    fn new(
        request: message::Builder<message::HeapAllocator>,
        cap_table: Vec<Option<Box<dyn ClientHook>>>,
    ) -> Self {
        Self { request, cap_table }
    }
}

impl ParamsHook for Params {
    fn get(&self) -> ::capnp::Result<any_pointer::Reader<'_>> {
        let mut result: any_pointer::Reader = self.request.get_root_as_reader()?;
        result.imbue(&self.cap_table);
        Ok(result)
    }
}

struct Results {
    retained_results: Rc<RefCell<Option<Box<dyn ResultsDoneHook>>>>,
    only_pipeline: bool,
    is_streaming: bool,
    message: Option<message::Builder<message::HeapAllocator>>,
    cap_table: Vec<Option<Box<dyn ClientHook>>>,
    results_done_fulfiller: Option<oneshot::Sender<Box<dyn ResultsDoneHook>>>,
    pipeline_sender: Option<crate::queued::PipelineInnerSender>,
}

impl Results {
    fn new(
        fulfiller: oneshot::Sender<Box<dyn ResultsDoneHook>>,
        pipeline_sender: crate::queued::PipelineInnerSender,
    ) -> Self {
        Self {
            retained_results: Rc::new(RefCell::new(None)),
            only_pipeline: false,
            is_streaming: false,
            message: None,
            cap_table: Vec::new(),
            results_done_fulfiller: Some(fulfiller),
            pipeline_sender: Some(pipeline_sender),
        }
    }
}

impl Drop for Results {
    fn drop(&mut self) {
        let message = self.message.take().unwrap_or_default();
        let fulfiller = self
            .results_done_fulfiller
            .take()
            .expect("results completed once");
        let cap_table = ::std::mem::take(&mut self.cap_table);
        let result = Box::new(ResultsDone::new(message, cap_table));
        if Rc::strong_count(&self.retained_results) > 1 {
            *self.retained_results.borrow_mut() = Some(result.add_ref());
        }
        let _ = fulfiller.send(result);
    }
}

impl ResultsHook for Results {
    fn cancellation_guard(&self) -> Option<Box<dyn std::any::Any>> {
        Some(Box::new(self.retained_results.clone()))
    }

    fn get_with_size_hint(
        &mut self,
        size_hint: Option<capnp::MessageSize>,
    ) -> ::capnp::Result<any_pointer::Builder<'_>> {
        let message = self
            .message
            .get_or_insert_with(|| message::Builder::new(result_allocator(size_hint)));
        let mut result: any_pointer::Builder = message.get_root()?;
        result.imbue_mut(&mut self.cap_table);
        Ok(result)
    }

    fn set_pipeline(&mut self) -> capnp::Result<()> {
        use ::capnp::traits::ImbueMut;
        let root = self.get()?;
        let size = root.target_size()?;
        let mut message2 = capnp::message::Builder::new(
            capnp::message::HeapAllocator::new().first_segment_words(size.word_count as u32 + 1),
        );
        let mut root2: capnp::any_pointer::Builder = message2.init_root();
        let mut cap_table2 = vec![];
        root2.imbue_mut(&mut cap_table2);
        root2.set_as(root.into_reader())?;
        let hook = Box::new(ResultsDone::new(message2, cap_table2)) as Box<dyn ResultsDoneHook>;
        self.set_pipeline_from(Box::new(Pipeline::new(hook)))
    }

    fn set_pipeline_from(&mut self, pipeline: Box<dyn PipelineHook>) -> capnp::Result<()> {
        let Some(sender) = self.pipeline_sender.take() else {
            return Err(Error::failed("set_pipeline() called twice".into()));
        };
        sender.complete(pipeline);
        Ok(())
    }

    fn tail_call(self: Box<Self>, request: Box<dyn RequestHook>) -> Promise<(), Error> {
        self.direct_tail_call(request).0
    }

    fn direct_tail_call(
        mut self: Box<Self>,
        request: Box<dyn RequestHook>,
    ) -> (Promise<(), Error>, Box<dyn PipelineHook>) {
        if self.only_pipeline {
            let pipeline = request.send_for_pipeline();
            if let Some(sender) = self.pipeline_sender.take() {
                sender.complete(pipeline.hook.clone());
            }
            return (
                Promise::from_future(async move {
                    let _context = self;
                    futures::future::pending::<capnp::Result<()>>().await
                }),
                pipeline.hook,
            );
        }
        if self.is_streaming {
            let promise = request.send_streaming();
            let pipeline = Box::new(crate::broken::Pipeline::new(Error::failed(
                "streaming calls have no result pipeline".into(),
            )));
            if let Some(sender) = self.pipeline_sender.take() {
                sender.complete(pipeline.add_ref());
            }
            return (
                Promise::from_future(async move {
                    let result = promise.await;
                    drop(self);
                    result
                }),
                pipeline,
            );
        }
        let capnp::capability::RemotePromise { promise, pipeline } = request.send();
        if let Some(sender) = self.pipeline_sender.take() {
            sender.complete(pipeline.hook.clone());
        }
        let completion = Promise::from_future(async move {
            let response = promise.await?;
            self.get()?.set_as(response.get()?)?;
            Ok(())
        });
        (completion, pipeline.hook)
    }

    fn allow_cancellation(&self) {
        // This runtime already permits cancellation when the response and all
        // dependent pipelines are dropped. No additional opt-in is required.
    }
}

// Large hints are advisory. Bound preallocation as the C++ RPC implementation
// does; payloads can still grow beyond this bound through ordinary allocation.
pub(crate) fn result_allocator(size_hint: Option<capnp::MessageSize>) -> message::HeapAllocator {
    let allocator = message::HeapAllocator::new();
    match size_hint {
        Some(size) => allocator.first_segment_words(size.word_count.min(1 << 20) as u32),
        None => allocator,
    }
}

struct ResultsDoneInner {
    message: ::capnp::message::Builder<::capnp::message::HeapAllocator>,
    cap_table: Vec<Option<Box<dyn ClientHook>>>,
}

pub(crate) struct ResultsDone {
    inner: Rc<ResultsDoneInner>,
}

impl ResultsDone {
    pub(crate) fn new(
        message: message::Builder<message::HeapAllocator>,
        cap_table: Vec<Option<Box<dyn ClientHook>>>,
    ) -> Self {
        Self {
            inner: Rc::new(ResultsDoneInner { message, cap_table }),
        }
    }
}

impl ResultsDoneHook for ResultsDone {
    fn add_ref(&self) -> Box<dyn ResultsDoneHook> {
        Box::new(Self {
            inner: self.inner.clone(),
        })
    }
    fn get(&self) -> ::capnp::Result<any_pointer::Reader<'_>> {
        let mut result: any_pointer::Reader = self.inner.message.get_root_as_reader()?;
        result.imbue(&self.inner.cap_table);
        Ok(result)
    }
}

pub(crate) struct Request {
    message: message::Builder<::capnp::message::HeapAllocator>,
    cap_table: Vec<Option<Box<dyn ClientHook>>>,
    interface_id: u64,
    method_id: u16,
    client: Box<dyn ClientHook>,
    hints: CallHints,
    is_streaming: bool,
    pipeline: crate::queued::Pipeline,
    pipeline_sender: crate::queued::PipelineInnerSender,
}

impl Request {
    pub(crate) fn new(
        interface_id: u64,
        method_id: u16,
        _size_hint: Option<::capnp::MessageSize>,
        client: Box<dyn ClientHook>,
    ) -> Self {
        let (pipeline_sender, pipeline) = crate::queued::Pipeline::new();
        Self {
            message: message::Builder::new_default(),
            cap_table: Vec::new(),
            interface_id,
            method_id,
            client,
            hints: CallHints::default(),
            is_streaming: false,
            pipeline,
            pipeline_sender,
        }
    }
}

impl RequestHook for Request {
    fn set_hints(&mut self, hints: CallHints) {
        self.hints = hints;
    }
    fn send_for_pipeline(mut self: Box<Self>) -> any_pointer::Pipeline {
        self.hints.only_promise_pipeline = true;
        self.hints.no_promise_pipelining = false;
        self.send().pipeline
    }

    fn get(&mut self) -> any_pointer::Builder<'_> {
        let mut result: any_pointer::Builder = self.message.get_root().unwrap();
        result.imbue_mut(&mut self.cap_table);
        result
    }
    fn get_brand(&self) -> usize {
        0
    }
    fn send(self: Box<Self>) -> capability::RemotePromise<any_pointer::Owned> {
        let tmp = *self;
        let Self {
            message,
            cap_table,
            interface_id,
            method_id,
            client,
            hints,
            is_streaming,
            mut pipeline,
            pipeline_sender,
        } = tmp;
        let params = Params::new(message, cap_table);

        let (results_done_fulfiller, results_done_promise) =
            oneshot::channel::<Box<dyn ResultsDoneHook>>();
        let results_done_promise = results_done_promise.map_err(crate::canceled_to_error);
        let mut results = Results::new(results_done_fulfiller, pipeline_sender.weak_clone());
        results.only_pipeline = hints.only_promise_pipeline;
        results.is_streaming = is_streaming;
        let promise = client.call_with_hints(
            interface_id,
            method_id,
            Box::new(params),
            Box::new(results),
            hints,
        );

        let p = futures::future::try_join(promise, results_done_promise).and_then(
            move |((), results_done_hook)| {
                pipeline_sender
                    .complete(Box::new(Pipeline::new(results_done_hook.add_ref()))
                        as Box<dyn PipelineHook>);
                Promise::ok((
                    capability::Response::new(Box::new(Response::new(results_done_hook))),
                    (),
                ))
            },
        );

        let (left, right) = crate::split::split(p);

        pipeline.drive(right);
        let pipeline = if hints.no_promise_pipelining {
            any_pointer::Pipeline::new(Box::new(crate::broken::DisabledPipeline))
        } else {
            any_pointer::Pipeline::new(Box::new(pipeline))
        };

        capability::RemotePromise {
            promise: Promise::from_future(left),
            pipeline,
        }
    }
    fn send_streaming(mut self: Box<Self>) -> Promise<(), Error> {
        // Local servers complete normally. If this capability resolves remotely,
        // direct_tail_call must preserve streaming credit-based readiness.
        self.is_streaming = true;
        Promise::from_future(async {
            let _ = self.send().promise.await?;
            Ok(())
        })
    }
    fn tail_send(self: Box<Self>) -> Option<(u32, Promise<(), Error>, Box<dyn PipelineHook>)> {
        None // A local request has no wire question ID to transfer.
    }
}

struct PipelineInner {
    results: Box<dyn ResultsDoneHook>,
}

pub(crate) struct Pipeline {
    inner: Rc<RefCell<PipelineInner>>,
}

impl Pipeline {
    pub(crate) fn new(results: Box<dyn ResultsDoneHook>) -> Self {
        Self {
            inner: Rc::new(RefCell::new(PipelineInner { results })),
        }
    }
}

impl Clone for Pipeline {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl PipelineHook for Pipeline {
    fn add_ref(&self) -> Box<dyn PipelineHook> {
        Box::new(self.clone())
    }
    fn get_pipelined_cap(&self, ops: &[PipelineOp]) -> Box<dyn ClientHook> {
        match self
            .inner
            .borrow_mut()
            .results
            .get()
            .unwrap()
            .get_pipelined_cap(ops)
        {
            Ok(v) => v,
            Err(e) => Box::new(crate::broken::Client::new(e, true, 0)) as Box<dyn ClientHook>,
        }
    }
}

pub(crate) struct Client<S>
where
    S: capability::Server + Clone,
{
    state: Rc<RefCell<ClientState<S>>>,
}

type LocalKey = (TypeId, usize);
struct LocalRegistration {
    state: Weak<dyn Any>,
    live: Weak<Cell<bool>>,
    revocable: bool,
}
impl LocalRegistration {
    fn is_live(&self) -> bool {
        self.live.upgrade().is_some_and(|live| live.get())
    }
}
thread_local! {
    // Weak entries do not keep application servers alive. Different generated
    // interface views of a Rust server have distinct dispatch implementations.
    static LOCAL_CLIENTS: RefCell<HashMap<usize, HashMap<TypeId, LocalRegistration>>> = RefCell::default();
}

struct ClientState<S>
where
    S: capability::Server + Clone,
{
    inner: Option<S>,
    identity: usize,
    registration: Option<LocalKey>,
    live: Rc<Cell<bool>>,
    resolved: Option<Box<dyn ClientHook>>,
    resolve_task: Option<futures::future::Shared<Promise<(), Error>>>,
    resolve_canceler: Rc<crate::revocable::Canceler>,
    revocation: Option<Rc<crate::revocable::Canceler>>,
    executor: Option<Rc<dyn capability::CallExecutor>>,
    #[cfg(unix)]
    fd: Option<std::rc::Rc<std::os::fd::OwnedFd>>,

    /// If a streaming call on this capability has returned an error,
    /// this contains a copy of that error.
    broken_error: Option<Error>,

    /// True while a streaming call is in flight. Later calls must wait here so
    /// non-streaming calls, like EOF methods, cannot overtake streaming writes.
    blocked: bool,
    blocked_calls: VecDeque<BlockedCall>,
}

impl<S: capability::Server + Clone> Drop for ClientState<S> {
    fn drop(&mut self) {
        if let Some(key) = self.registration {
            let _ = LOCAL_CLIENTS.try_with(|clients| {
                if let Ok(mut clients) = clients.try_borrow_mut() {
                    if let Some(views) = clients.get_mut(&key.1) {
                        if views
                            .get(&key.0)
                            .is_some_and(|entry| entry.state.strong_count() == 0)
                        {
                            views.remove(&key.0);
                        }
                        if views.is_empty() {
                            clients.remove(&key.1);
                        }
                    }
                }
            });
        }
        self.resolve_canceler
            .cancel(Error::failed("local client dropped".into()));
    }
}

pub(crate) struct Revoker {
    revoke: Box<dyn Fn(Error)>,
    in_use: Box<dyn Fn() -> bool>,
}
impl Revoker {
    pub(crate) fn revoke(&self, error: Error) {
        (self.revoke)(error);
    }
    pub(crate) fn is_in_use(&self) -> bool {
        (self.in_use)()
    }
}

enum BlockedCall {
    Call {
        interface_id: u64,
        method_id: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
        fulfiller: oneshot::Sender<Promise<(), Error>>,
    },
    Barrier(oneshot::Sender<()>),
    Redirect(ShorteningBarrier),
}

struct ShorteningBarrier {
    client: Weak<RefCell<crate::queued::ClientInner>>,
    target: Option<Box<dyn ClientHook>>,
}
impl ShorteningBarrier {
    fn complete(mut self) {
        let target = self.target.take().unwrap();
        if let Some(client) = self.client.upgrade() {
            crate::queued::ClientInner::resolve(&client, Ok(target));
        }
    }
}
impl Drop for ShorteningBarrier {
    fn drop(&mut self) {
        if self.target.is_some() {
            if let Some(client) = self.client.upgrade() {
                crate::queued::ClientInner::resolve(
                    &client,
                    Err(Error::failed("shortening barrier dropped".into())),
                );
            }
        }
    }
}

struct StreamingCall<S>
where
    S: capability::Server + Clone + 'static,
{
    state: Rc<RefCell<ClientState<S>>>,
    promise: Promise<(), Error>,
    completed: bool,
}

impl<S> Unpin for StreamingCall<S> where S: capability::Server + Clone + 'static {}

impl<S> Future for StreamingCall<S>
where
    S: capability::Server + Clone + 'static,
{
    type Output = Result<(), Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match Pin::new(&mut this.promise).poll(cx) {
            Poll::Ready(result) => {
                if let Err(e) = &result {
                    this.state.borrow_mut().broken_error = Some(e.clone());
                }
                this.completed = true;
                ClientState::unblock(this.state.clone());
                Poll::Ready(result)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<S> Drop for StreamingCall<S>
where
    S: capability::Server + Clone + 'static,
{
    fn drop(&mut self) {
        if !self.completed {
            ClientState::unblock(self.state.clone());
        }
    }
}

impl<S> ClientState<S>
where
    S: capability::Server + Clone + 'static,
{
    fn revoke(state: &Rc<RefCell<Self>>, error: Error) {
        let (server, revocation, resolution, queued) = {
            let mut state = state.borrow_mut();
            let Some(server) = state.inner.take() else {
                return;
            };
            state.live.set(false);
            state.broken_error = Some(error.clone());
            state.blocked = false;
            #[cfg(unix)]
            {
                state.fd = None;
            }
            (
                server,
                state.revocation.clone(),
                state.resolve_canceler.clone(),
                std::mem::take(&mut state.blocked_calls),
            )
        };
        if let Some(revocation) = revocation {
            revocation.cancel(error.clone());
        }
        resolution.cancel(error.clone());
        for call in queued {
            match call {
                BlockedCall::Call { fulfiller, .. } => {
                    let _ = fulfiller.send(Promise::err(error.clone()));
                }
                BlockedCall::Barrier(fulfiller) => {
                    let _ = fulfiller.send(());
                }
                BlockedCall::Redirect(barrier) => barrier.complete(),
            }
        }
        drop(server);
    }

    fn dispatch_or_queue(
        state: Rc<RefCell<Self>>,
        interface_id: u64,
        method_id: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
    ) -> Promise<(), Error> {
        {
            let mut state_ref = state.borrow_mut();
            if let Some(e) = &state_ref.broken_error {
                return Promise::err(e.clone());
            }

            if state_ref.blocked {
                let (fulfiller, promise) = oneshot::channel();
                state_ref.blocked_calls.push_back(BlockedCall::Call {
                    interface_id,
                    method_id,
                    params,
                    results,
                    fulfiller,
                });

                // The queued call has not started yet. Resolve this promise
                // with the call's real promise once it reaches the front.
                return Promise::from_future(async move {
                    match promise.await {
                        Ok(p) => p.await,
                        Err(e) => Err(crate::canceled_to_error(e)),
                    }
                });
            }
        }

        Self::dispatch_call(state, interface_id, method_id, params, results)
    }

    fn dispatch_call(
        state: Rc<RefCell<Self>>,
        interface_id: u64,
        method_id: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
    ) -> Promise<(), Error> {
        let inner = {
            let state_ref = state.borrow();
            if let Some(e) = &state_ref.broken_error {
                return Promise::err(e.clone());
            }
            state_ref.inner.as_ref().expect("live server").clone()
        };

        let guard = results.cancellation_guard();
        let executor = state
            .borrow()
            .executor
            .clone()
            .or_else(|| results.cancellation_executor());
        let mut f = inner.dispatch_call(
            interface_id,
            method_id,
            ::capnp::capability::Params::new(params),
            ::capnp::capability::Results::new(results),
        );

        let revocation = state.borrow().revocation.clone();
        if let Some(revocation) = revocation {
            f.promise = revocation.wrap(f.promise);
        }

        if !f.allow_cancellation {
            let Some(executor) = executor else {
                return Promise::err(Error::failed(
                    "non-cancellable local call requires new_client_with_executor()".into(),
                ));
            };
            let retained_server = state.clone();
            let task = Promise::from_future(async move {
                let result = f.promise.await;
                drop(guard);
                drop(retained_server);
                result
            })
            .shared();
            if let Err(error) = executor.spawn(Promise::from_future(task.clone())) {
                return Promise::err(error);
            }
            f.promise = Promise::from_future(task);
        }

        if f.is_streaming {
            // A streaming call serializes later calls on the same local
            // capability until its server-side promise completes.
            state.borrow_mut().blocked = true;
            Promise::from_future(StreamingCall {
                state,
                promise: f.promise,
                completed: false,
            })
        } else {
            f.promise
        }
    }

    fn unblock(state: Rc<RefCell<Self>>) {
        loop {
            let blocked_call = {
                let mut state_ref = state.borrow_mut();
                state_ref.blocked = false;
                state_ref.blocked_calls.pop_front()
            };

            let Some(blocked_call) = blocked_call else {
                return;
            };

            let BlockedCall::Call {
                interface_id,
                method_id,
                params,
                results,
                fulfiller,
            } = blocked_call
            else {
                if let BlockedCall::Barrier(fulfiller) = blocked_call {
                    let _ = fulfiller.send(());
                } else if let BlockedCall::Redirect(barrier) = blocked_call {
                    barrier.complete();
                }
                continue;
            };
            if fulfiller.is_canceled() {
                continue;
            }
            let promise =
                Self::dispatch_call(state.clone(), interface_id, method_id, params, results);
            let _ = fulfiller.send(promise);

            // If the unblocked call was itself streaming, dispatch_call() set
            // blocked again; leave remaining calls queued behind it.
            if state.borrow().blocked {
                return;
            }
        }
    }
}

impl<S> Client<S>
where
    S: capability::Server + Clone,
{
    pub(crate) fn is_registered(server: usize, identity: usize) -> bool
    where
        S: 'static,
    {
        let key = (TypeId::of::<S>(), server);
        let existing = LOCAL_CLIENTS.with(|clients| {
            clients
                .borrow()
                .get(&key.1)
                .and_then(|views| views.get(&key.0))
                .and_then(|entry| entry.state.upgrade())
        });
        existing
            .and_then(|state| state.downcast::<RefCell<ClientState<S>>>().ok())
            .is_some_and(|state| {
                let state = state.borrow();
                state.identity == identity && state.inner.is_some()
            })
    }

    pub(crate) fn into_hook(self) -> Box<dyn ClientHook>
    where
        S: 'static,
    {
        let key = (
            TypeId::of::<S>(),
            self.state.borrow().inner.as_ref().unwrap().as_ptr(),
        );
        let revocable = self.state.borrow().revocation.is_some();
        let (existing, conflict) = LOCAL_CLIENTS.with(|clients| {
            let clients = clients.borrow();
            match clients.get(&key.1) {
                Some(views) => (
                    views.get(&key.0).and_then(|entry| entry.state.upgrade()),
                    views
                        .values()
                        .any(|entry| entry.is_live() && (revocable || entry.revocable)),
                ),
                None => (None, false),
            }
        });
        if conflict {
            return Box::new(crate::broken::Client::new(
                Error::failed(
                    "cannot create another client for a server with a revocable client".into(),
                ),
                true,
                0,
            ));
        }
        if let Some(existing) = existing {
            let state = existing.downcast::<RefCell<ClientState<S>>>().ok().unwrap();
            if state.borrow().inner.is_some() {
                return Box::new(Self { state });
            }
        }
        self.state.borrow_mut().registration = Some(key);
        let erased: Rc<dyn Any> = self.state.clone();
        let live = Rc::downgrade(&self.state.borrow().live);
        LOCAL_CLIENTS.with(|clients| {
            clients.borrow_mut().entry(key.1).or_default().insert(
                key.0,
                LocalRegistration {
                    state: Rc::downgrade(&erased),
                    live,
                    revocable,
                },
            )
        });
        drop(erased);

        // No internal borrow spans a user hook. In particular shorten_path() may
        // call this server's self capability or construct another client alias.
        let server = self.state.borrow().inner.as_ref().unwrap().clone();
        if let Some(hooks) = server.get_hooks() {
            if let Some(slot) = hooks.self_cap() {
                let weak = Rc::downgrade(&self.state);
                slot.bind(Rc::new(move || {
                    let state = weak.upgrade()?;
                    state.borrow().inner.as_ref()?;
                    Some(Box::new(Self { state }))
                }));
            }
            if let Some(promise) = hooks.shorten_path() {
                self.start_shortening(promise);
            }
        }
        Box::new(self)
    }

    fn start_shortening(&self, promise: Promise<capability::Client, Error>)
    where
        S: 'static,
    {
        let weak = Rc::downgrade(&self.state);
        let task = Promise::from_future(async move {
            let target = promise.await?;
            let Some(state) = weak.upgrade() else {
                return Ok(());
            };
            // A direct self-resolution would otherwise form a reference cycle
            // and make when_resolved() loop forever.
            if target.hook.get_ptr() == state.borrow().identity && target.hook.get_brand() == 0 {
                return Err(Error::failed("shorten_path resolved to itself".into()));
            }
            let mut state = state.borrow_mut();
            if state.inner.is_none() {
                return Err(state.broken_error.clone().unwrap());
            }
            let target = if state.blocked {
                let queued = crate::queued::Client::new(None);
                state
                    .blocked_calls
                    .push_back(BlockedCall::Redirect(ShorteningBarrier {
                        client: Rc::downgrade(&queued.inner),
                        target: Some(target.hook),
                    }));
                // Resolve as soon as this barrier leaves the old queue, even if
                // no observer happens to be polling the replacement capability.
                Box::new(queued) as Box<dyn ClientHook>
            } else {
                target.hook
            };
            state.resolved = Some(target);
            Ok(())
        });
        let (canceler, executor) = {
            let state = self.state.borrow();
            (state.resolve_canceler.clone(), state.executor.clone())
        };
        let task = canceler.wrap(task).shared();
        self.state.borrow_mut().resolve_task = Some(task.clone());
        if let Some(executor) = executor {
            // Failure to drive eagerly does not break old-route calls. Resolution
            // waiters and live calls can also drive this shared future.
            let _ = executor.spawn(Promise::from_future(task));
        }
    }

    pub(crate) fn into_revocable(self) -> (Self, Revoker)
    where
        S: 'static,
    {
        {
            let mut state = self.state.borrow_mut();
            state.revocation = Some(Rc::new(crate::revocable::Canceler::default()));
        }
        let weak = Rc::downgrade(&self.state);
        let usage = weak.clone();
        let revoker = Revoker {
            revoke: Box::new(move |error| {
                if let Some(state) = weak.upgrade() {
                    ClientState::revoke(&state, error);
                }
            }),
            in_use: Box::new(move || usage.strong_count() > 1),
        };
        (self, revoker)
    }

    pub(crate) fn new_with_executor(server: S, executor: Rc<dyn capability::CallExecutor>) -> Self {
        Self::new(server).with_executor(executor)
    }

    pub(crate) fn with_executor(self, executor: Rc<dyn capability::CallExecutor>) -> Self {
        self.state.borrow_mut().executor = Some(executor);
        self
    }

    #[cfg(unix)]
    pub(crate) fn new_with_fd(server: S, fd: std::rc::Rc<std::os::fd::OwnedFd>) -> Self {
        let client = Self::new(server);
        client.state.borrow_mut().fd = Some(fd);
        client
    }

    pub(crate) fn new(server: S) -> Self {
        let client = Self {
            state: Rc::new(RefCell::new(ClientState {
                identity: 0,
                registration: None,
                live: Rc::new(Cell::new(true)),
                resolved: None,
                resolve_task: None,
                resolve_canceler: Rc::new(crate::revocable::Canceler::default()),
                inner: Some(server),
                revocation: None,
                executor: None,
                #[cfg(unix)]
                fd: None,
                broken_error: None,
                blocked: false,
                blocked_calls: VecDeque::new(),
            })),
        };
        client.state.borrow_mut().identity = Rc::as_ptr(&client.state) as usize;
        client
    }
}

impl<S> Clone for Client<S>
where
    S: capability::Server + Clone,
{
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl<S> ClientHook for Client<S>
where
    S: capability::Server + Clone + 'static,
{
    fn debug_info(&self, chain: &mut capnp::private::capability::DebugInfo) {
        let Ok(state) = self.state.try_borrow() else {
            chain.push("local(busy)");
            return;
        };
        if let Some(error) = &state.broken_error {
            chain.push("broken");
            chain.push(error.to_string());
        } else if let Some(resolved) = state.resolved.clone() {
            drop(state);
            chain.push("shortened");
            chain.follow(&*resolved);
        } else if state.inner.is_some() {
            chain.push("local");
            chain.push(std::any::type_name::<S>());
        } else {
            chain.push("revoked");
        }
    }

    fn is_local_server_ready(&self) -> bool {
        let state = self.state.borrow();
        !state.blocked && state.inner.is_some()
    }
    fn when_local_server_ready(&self) -> Promise<(), Error> {
        let promise = {
            let mut state = self.state.borrow_mut();
            if !state.blocked {
                return Promise::ok(());
            }
            let (fulfiller, promise) = oneshot::channel();
            state
                .blocked_calls
                .push_back(BlockedCall::Barrier(fulfiller));
            promise
        };
        let state = self.state.clone();
        Promise::from_future(async move {
            let result = promise.await.map_err(crate::canceled_to_error);
            drop(state);
            result
        })
    }
    #[cfg(unix)]
    fn get_fd(&self) -> Option<std::rc::Rc<std::os::fd::OwnedFd>> {
        self.state.borrow().fd.clone()
    }
    fn add_ref(&self) -> Box<dyn ClientHook> {
        Box::new(self.clone())
    }
    fn new_call(
        &self,
        interface_id: u64,
        method_id: u16,
        size_hint: Option<::capnp::MessageSize>,
    ) -> capability::Request<any_pointer::Owned, any_pointer::Owned> {
        if let Some(target) = self.get_resolved() {
            return target.new_call(interface_id, method_id, size_hint);
        }
        capability::Request::new(Box::new(Request::new(
            interface_id,
            method_id,
            size_hint,
            self.add_ref(),
        )))
    }

    fn call(
        &self,
        interface_id: u64,
        method_id: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
    ) -> Promise<(), Error> {
        self.call_with_hints(
            interface_id,
            method_id,
            params,
            results,
            CallHints::default(),
        )
    }

    fn call_with_hints(
        &self,
        interface_id: u64,
        method_id: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
        hints: CallHints,
    ) -> Promise<(), Error> {
        if let Some(target) = self.get_resolved() {
            return target.call_with_hints(interface_id, method_id, params, results, hints);
        }
        // We don't want to actually dispatch the call synchronously, because we don't want the callee
        // to have any side effects before the promise is returned to the caller.  This helps avoid
        // race conditions.
        let state = self.state.clone();
        let task = state.borrow().resolve_task.clone();
        Promise::from_future(async move {
            let call =
                ClientState::dispatch_or_queue(state, interface_id, method_id, params, results);
            if let Some(task) = task {
                match futures::future::select(call, task).await {
                    futures::future::Either::Left((result, _)) => result,
                    futures::future::Either::Right((_, call)) => call.await,
                }
            } else {
                call.await
            }
        })
    }

    fn get_ptr(&self) -> usize {
        self.state.borrow().identity
    }

    fn get_brand(&self) -> usize {
        0
    }

    fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
        self.state.borrow().resolved.clone()
    }

    fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, Error>> {
        if let Some(target) = self.get_resolved() {
            return Some(Promise::ok(target));
        }
        let task = self.state.borrow().resolve_task.clone()?;
        let state = self.state.clone();
        Some(Promise::from_future(async move {
            task.await?;
            Ok(state.borrow().resolved.as_ref().unwrap().add_ref())
        }))
    }

    fn when_resolved(&self) -> Promise<(), Error> {
        crate::rpc::default_when_resolved_impl(self)
    }
}
