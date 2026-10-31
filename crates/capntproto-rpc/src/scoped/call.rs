//! Request storage, admission and completion for the hosted graph bridge.
//!
//! Construction is unsent. Admission reserves queue/response credit before
//! entering caller-owned hooks; publication releases destination/parameter
//! owners while the response keeps its own credit. `CallPermit` is deliberately the
//! last field: callback-bearing owners must drop before admission is released.
use super::{admission, response, streaming, tail_target, wire, Endpoint, Hook, Inbox, Shutdown};
use capnp::{
    any_pointer,
    capability::{self, CallHints, Promise},
    private::capability::{ClientHook, ParamsHook, ResultsHook},
    Error,
};
use capntproto_rpc_engine::completion;
use std::{
    cell::{Cell, RefCell},
    future::Future,
    pin::Pin,
    rc::{Rc, Weak},
    task::{Context, Poll},
};

/// Empty request metadata only. Response/pipeline owners live independently;
/// clearing a completed request releases all callback-bearing fields first.
/// Retention follows the inbox's existing concurrent-call bound.
pub(super) struct Cache {
    first: Cell<Option<Box<Call>>>,
    #[allow(clippy::vec_box)] // Reuse the existing RequestHook allocation and address.
    rest: RefCell<Vec<Box<Call>>>,
    maximum: usize,
}
impl Cache {
    pub(super) fn new(maximum: usize) -> Self {
        Self {
            first: Cell::new(None),
            rest: RefCell::new(Vec::new()),
            maximum,
        }
    }
    fn take(&self) -> Option<Box<Call>> {
        self.first.take().or_else(|| self.rest.borrow_mut().pop())
    }
    fn put(&self, call: Box<Call>) {
        if self.maximum == 0 {
            return;
        }
        match self.first.take() {
            None => {
                self.first.set(Some(call));
                return;
            }
            Some(first) => self.first.set(Some(first)),
        }
        let mut rest = self.rest.borrow_mut();
        if rest.len() < self.maximum - 1 && rest.try_reserve(1).is_ok() {
            rest.push(call);
        }
    }
    pub(super) fn clear(&self) {
        drop(self.first.take());
        let rest = std::mem::take(&mut *self.rest.borrow_mut());
        drop(rest);
    }
}

/// Owned hosted call awaiting engine admission/execution. The result hook can
/// publish a pipeline before the eventual result, and retains capabilities in
/// both directions. Dropping an unfinished call fails its completion.
#[must_use = "complete the call, or drop it to report cancellation"]
pub struct Call {
    pub(super) kind: CallKind,
    pub(super) credit: Option<Rc<admission::Credit>>,
    pub(super) interface: u64,
    pub(super) method: u16,
    pub(super) hints: CallHints,
    pub(super) params: CallParams,
    pub(super) results: response::Results,
    pub(super) completion: CallCompletion,
    pub(super) shutdown: Shutdown,
    // Last: release admission after destroying callback-bearing call owners.
    pub(super) permit: CallPermit,
}
// These publication modes are mutually exclusive. Keep their cold owners in
// the same storage instead of enlarging every ordinary request for streaming.
pub(super) enum CallKind {
    Ordinary,
    Streaming(Option<Rc<streaming::Publication>>),
    NativeTail(Rc<wire::server::NativeTail>),
    ThirdParty(Rc<capnp::message::Builder<capnp::message::HeapAllocator>>),
}
impl CallKind {
    pub(super) fn third_party(
        &self,
    ) -> Option<&Rc<capnp::message::Builder<capnp::message::HeapAllocator>>> {
        match self {
            Self::ThirdParty(contact) => Some(contact),
            _ => None,
        }
    }
    pub(super) fn native_tail(&self) -> Option<&Rc<wire::server::NativeTail>> {
        match self {
            Self::NativeTail(tail) => Some(tail),
            _ => None,
        }
    }
    pub(super) fn stream(&self) -> Option<&Rc<streaming::Publication>> {
        match self {
            Self::Streaming(publication) => publication.as_ref(),
            _ => None,
        }
    }
    pub(super) fn is_streaming(&self) -> bool {
        matches!(self, Self::Streaming(_))
    }
    pub(super) fn ordinary_destination(&self) -> bool {
        matches!(self, Self::Ordinary | Self::Streaming(_))
    }
    pub(super) fn release_stream(&mut self) {
        if let Self::Streaming(publication) = self {
            drop(publication.take());
        }
    }
}

pub(super) enum CallParams {
    Request(crate::local::Params),
    OwnedRequest(Box<crate::local::Request>),
    Hook(Box<dyn ParamsHook>),
    Published,
}
impl CallParams {
    pub(super) fn owned_request(mut request: Box<crate::local::Request>) -> Self {
        // This private request keeps its original parameter allocation through
        // forwarding. Retain its concrete type for admission and wire encoding.
        request.freeze_large_params();
        Self::OwnedRequest(request)
    }
    pub(super) fn hook(params: Box<dyn ParamsHook>) -> Self {
        Self::Hook(params)
    }
    pub(super) fn capability_table(&self) -> Option<&[Option<Box<dyn ClientHook>>]> {
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
            Self::Published => None,
        }
    }
    #[inline(always)]
    pub(super) fn prepared_call(
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
            Self::Published => None,
        }
    }
    pub(super) fn prepared_envelope(
        &self,
    ) -> Option<&capntproto_rpc_wire::owned::connection::PreparedCall<capnp::message::HeapAllocator>>
    {
        match self {
            Self::Request(request) => request.prepared_envelope(),
            Self::OwnedRequest(request) => request.prepared_envelope(),
            Self::Hook(hook) => hook.as_any().and_then(|params| {
                params
                    .downcast_ref::<crate::local::Request>()
                    .and_then(crate::local::Request::prepared_envelope)
                    .or_else(|| {
                        params
                            .downcast_ref::<crate::local::Params>()
                            .and_then(crate::local::Params::prepared_envelope)
                    })
            }),
            Self::Published => None,
        }
    }
    pub(super) fn into_hook(self) -> Box<dyn ParamsHook> {
        match self {
            Self::Request(request) => Box::new(request),
            Self::OwnedRequest(request) => request,
            Self::Hook(hook) => hook,
            Self::Published => unreachable!("published parameters cannot be forwarded"),
        }
    }
    fn request_mut(&mut self) -> &mut crate::local::Params {
        match self {
            Self::Request(request) => request,
            _ => unreachable!("unsent request"),
        }
    }
}
impl ParamsHook for CallParams {
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        match self {
            Self::Request(request) => request.get(),
            Self::OwnedRequest(request) => ParamsHook::get(&**request),
            Self::Hook(hook) => hook.get(),
            Self::Published => Err(Error::failed(
                "parameters released after wire publication".into(),
            )),
        }
    }
}
impl capnp::private::capability::RequestHook for Call {
    fn prepare_tail_call(&mut self, context: Rc<dyn std::any::Any>) -> bool {
        let Ok(context) = context.downcast::<wire::server::NativeTail>() else {
            return false;
        };
        if !self.kind.ordinary_destination()
            || !context.accepts(&self.endpoint().inbox)
            || !self.endpoint().remote.get()
            || self.endpoint().introduced.borrow().is_some()
            || self.endpoint().introduction.borrow().is_some()
            || self.endpoint().failure.borrow().is_some()
        {
            return false;
        }
        self.kind = CallKind::NativeTail(context);
        true
    }
    fn get(&mut self) -> any_pointer::Builder<'_> {
        self.params.request_mut().builder()
    }
    fn get_brand(&self) -> usize {
        if self.kind.third_party().is_none()
            && self.endpoint().remote.get()
            && self.endpoint().introduced.borrow().is_none()
            && self.endpoint().introduction.borrow().is_none()
            && self
                .endpoint()
                .inbox
                .upgrade()
                .is_some_and(|inbox| inbox.native_tail)
        {
            self.endpoint().inbox.as_ptr() as usize
        } else {
            0
        }
    }
    fn set_hints(&mut self, hints: CallHints) {
        self.hints = hints;
    }
    fn third_party_tail_target(&self) -> Option<Box<dyn std::any::Any>> {
        tail_target(&Hook(self.endpoint().clone(), self.credit.clone()))
    }
    fn set_third_party_tail_target(
        &mut self,
        contact: any_pointer::Reader<'_>,
    ) -> capnp::Result<()> {
        if self.kind.third_party().is_some() || self.third_party_tail_target().is_none() {
            return Err(Error::failed("ineligible tail transfer".into()));
        }
        contact.validate_capability_indices(0)?;
        let mut body = capnp::message::Builder::new_default();
        body.set_root(contact)?;
        self.kind = CallKind::ThirdParty(Rc::new(body));
        Ok(())
    }
    fn send(mut self: Box<Self>) -> capability::RemotePromise<any_pointer::Owned> {
        // Like legacy, only send_for_pipeline() may suppress response interest.
        self.hints.only_promise_pipeline = false;
        if self.can_send_directly() {
            return if self.hints.no_promise_pipelining {
                response::request_prepared(self)
            } else {
                response::request_pipelined_prepared(self)
            };
        }
        self.into_request().send()
    }
    fn send_for_pipeline(mut self: Box<Self>) -> any_pointer::Pipeline {
        self.hints.only_promise_pipeline = true;
        self.hints.no_promise_pipelining = false;
        if self.can_send_directly() {
            // The pipeline retains the direct completion's interest. Publishing
            // it must not drop a queued adapter's only reply receiver and cancel
            // the call before its wire publication.
            return response::request_pipelined_prepared(self).pipeline;
        }
        self.into_request().send_for_pipeline()
    }
    fn send_streaming(mut self: Box<Self>) -> Promise<(), Error> {
        self.hints.only_promise_pipeline = false;
        if self.kind.third_party().is_some()
            || self.endpoint().introduced.borrow().is_some()
            || self.endpoint().introduction.borrow().is_some()
        {
            return self.into_request().send_streaming();
        }
        // Bootstrap and unresolved local promises can acquire a remote route
        // after send(). Only actual wire publication charges the controller;
        // an initial routing hint cannot decide streaming readiness.
        let stream = self
            .endpoint()
            .inbox
            .upgrade()
            .and_then(|inbox| inbox.stream_factory.clone())
            .map(|factory| {
                let flow = self
                    .endpoint()
                    .streaming
                    .get_or_init(|| streaming::Flow::new(factory))
                    .clone();
                streaming::Publication::new(flow)
            });
        if let Some(tail) = self.kind.native_tail() {
            tail.forwarded();
        }
        self.kind = CallKind::Streaming(stream.clone());
        // Streaming exposes no result pipeline, just like legacy's dedicated
        // send. Keep the native completion without allocating an unobservable
        // pipeline or changing the caller's wire hints. All indirect routes
        // were forwarded above; a local ResultsHook here would recurse.
        let response = response::request_prepared(self).promise;
        let ack = Promise::from_future(async move {
            response.await?;
            Ok(())
        });
        match stream {
            Some(stream) => stream.bind(ack),
            None => ack,
        }
    }
    fn tail_send(
        mut self: Box<Self>,
    ) -> Option<(
        u32,
        Promise<(), Error>,
        Box<dyn capnp::private::capability::PipelineHook>,
    )> {
        self.hints.only_promise_pipeline = false;
        self.into_request().tail_send()
    }
}
impl Call {
    #[inline(always)]
    pub(super) fn boxed(inbox: Option<&Inbox>, make: impl FnOnce() -> Self) -> Box<Self> {
        match inbox.and_then(|inbox| inbox.request_cache.take()) {
            Some(mut call) => {
                *call = make();
                call
            }
            None => {
                let storage = Box::<Self>::new_uninit();
                Box::write(storage, make())
            }
        }
    }

    pub(super) fn recycle_boxed(mut self: Box<Self>) {
        let Some(inbox) = self.permit.inbox.as_ref().and_then(Weak::upgrade) else {
            return;
        };
        // Match field destruction order, including admission being last. No
        // request-cache borrow spans user payload/waker destruction. Reentrant
        // teardown may close the inbox, so check again before retaining storage.
        drop(core::mem::replace(&mut self.kind, CallKind::Ordinary));
        drop(self.credit.take());
        drop(core::mem::replace(&mut self.params, CallParams::Published));
        drop(core::mem::replace(
            &mut self.results,
            response::Results::Unsent,
        ));
        drop(core::mem::replace(
            &mut self.completion,
            CallCompletion::Unsent,
        ));
        drop(core::mem::replace(
            &mut self.shutdown,
            inbox.shutdown.clone(),
        ));
        drop(core::mem::replace(
            &mut self.permit,
            CallPermit {
                inbox: None,
                target: None,
            },
        ));
        if !inbox.closed.get() {
            inbox.request_cache.put(self);
        }
    }

    pub(super) fn endpoint(&self) -> &Rc<Endpoint> {
        self.permit
            .target
            .as_ref()
            .expect("target consumed after publication")
    }
    pub(super) fn dispatch_committed(&mut self) {
        self.permit.release_target();
    }
    pub(super) fn publication_committed(&mut self) {
        self.kind.release_stream();
        if let CallCompletion::Response(completion) = &self.completion {
            completion.published();
        }
        if let CallParams::OwnedRequest(request) = &mut self.params {
            // The question and FIFO now protect the child's route. Clear the
            // response guard before releasing parent interest: its destructor
            // may synchronously wake and poll the child.
            request.publication_committed();
        }
        // The FIFO owns the bytes and the question owns descriptor grants.
        // Like legacy RpcRequest::send_data, neither the destination nor the
        // original parameter builder belongs to the pending reply.
        self.dispatch_committed();
        drop(core::mem::replace(&mut self.params, CallParams::Published));
    }
    #[inline(always)]
    pub(super) fn can_send_directly(&self) -> bool {
        self.kind.third_party().is_none() && {
            let unintroduced = self.endpoint().introduced.borrow().is_none()
                && self.endpoint().introduction.borrow().is_none();
            unintroduced
                || Hook(self.endpoint().clone(), self.credit.clone()).direct_response_available()
        }
    }

    #[inline(always)]
    pub(super) fn enqueue_owned(
        mut self: Box<Self>,
        inbox: Option<Rc<Inbox>>,
    ) -> Result<Rc<Inbox>, Box<Error>> {
        let failure = self.endpoint().failure.borrow().clone();
        if let Some(error) = failure {
            return Err(self.reject_owned(error));
        }
        let Some(inbox) = inbox.filter(|inbox| !inbox.closed.get()) else {
            return Err(self.reject_owned(self.shutdown.reason().unwrap_or_else(|| {
                Error::disconnected("scoped ClientHook driver closed".into())
            })));
        };
        if inbox.live_calls.get() == inbox.maximum_calls {
            return Err(
                self.reject_owned(Error::overloaded("scoped call capacity exhausted".into()))
            );
        }
        let credit = self
            .credit
            .take()
            .or_else(|| inbox.admission.clone().map(admission::Credit::new));
        if self.endpoint().remote.get() {
            if let Some(credit) = &credit {
                credit.ensure().map_err(|error| self.reject_owned(error))?;
            }
        }
        if inbox.queue.borrow_mut().try_reserve(1).is_err() {
            return Err(self.reject_owned(Error::overloaded("scoped call queue exhausted".into())));
        }
        // Reserve before invoking a custom results hook, which may reenter or
        // close this connection. The guard rolls back even during unwinding.
        inbox.live_calls.set(inbox.live_calls.get() + 1);
        self.endpoint()
            .pending_calls
            .set(self.endpoint().pending_calls.get() + 1);
        self.permit.inbox = Some(Rc::downgrade(&inbox));
        if let Some(credit) = &credit {
            self.results.retain_context(credit.clone());
        }
        if inbox.closed.get() {
            return Err(
                self.reject_owned(inbox.shutdown.reason().unwrap_or_else(|| {
                    Error::disconnected("scoped ClientHook driver closed".into())
                })),
            );
        }
        self.credit = credit;
        // Requests built by this hook already retain this exact shutdown state.
        // General incoming ParamsHook calls acquire it only at admission.
        if !matches!(self.params, CallParams::Request(_)) {
            self.shutdown = inbox.shutdown.clone();
        }
        if let CallParams::Request(params) = &mut self.params {
            params.freeze_large_params();
        }
        inbox.queue.borrow_mut().push_back(self);
        inbox.notify_enqueue();
        Ok(inbox)
    }

    #[cold]
    pub(super) fn reject_owned(&mut self, error: Error) -> Box<Error> {
        // Publish the actual rejection before Drop's cancellation fallback can
        // settle a retained pipeline with a generic error.
        self.completion.complete(Err(error.clone()));
        Box::new(error)
    }

    pub(super) fn into_request(mut self: Box<Self>) -> Box<crate::local::Request> {
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
            Hook(self.permit.target.take().unwrap(), self.credit),
            context,
            match self.kind {
                CallKind::ThirdParty(contact) => Some(contact),
                _ => None,
            },
        ))
    }
}
pub(super) enum CallCompletion {
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
    pub(super) fn poll_observed(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        match self {
            Self::Unsent => unreachable!("unadmitted request"),
            Self::Promise(delivery) => delivery.poll_observed(cx),
            Self::Response(completion) => completion.poll_observed(cx),
        }
    }
    pub(super) fn complete(&mut self, result: capnp::Result<()>) {
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
    /// Read parameters before execution or wire publication. A published wire
    /// call releases the original application storage independently of its reply.
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
        if let Some(tail) = self.kind.native_tail() {
            tail.forwarded();
        }
        self.completion.complete(result);
    }
    /// Complete in place, then recycle only empty metadata. Callback-bearing
    /// fields drop in declaration order without moving the full call to stack.
    #[allow(clippy::boxed_local)] // Preserve the existing allocation through field destruction.
    pub(super) fn complete_boxed(mut self: Box<Self>, result: capnp::Result<()>) {
        if let Some(tail) = self.kind.native_tail() {
            tail.forwarded();
        }
        self.completion.complete(result);
        self.recycle_boxed();
    }

    /// Execute an admitted call through an ordinary generated server hook.
    /// Params/results (including early pipeline and cancellation policy hooks)
    /// move intact; the adapter does not rebuild the application's context.
    pub fn forward(self, target: &dyn ClientHook) -> Promise<(), Error> {
        if let Some(tail) = self.kind.native_tail() {
            tail.forwarded();
        }
        let streaming = self.kind.is_streaming();
        let Self {
            kind: _,
            credit,
            interface,
            method,
            hints,
            params,
            results,
            mut completion,
            shutdown: _,
            mut permit,
        } = self;
        let mut operation = target.call_with_hints(
            interface,
            method,
            params.into_hook(),
            results.into_streaming_hook(streaming),
            hints,
        );
        Promise::from_future(async move {
            let result = futures::future::poll_fn(|cx| {
                let result = Pin::new(&mut operation).poll(cx);
                permit.release_target();
                result
            })
            .await;
            completion.complete(result.clone());
            drop((permit, credit));
            result
        })
    }
}
pub(super) struct CallPermit {
    inbox: Option<Weak<Inbox>>,
    target: Option<Rc<Endpoint>>,
}
impl CallPermit {
    pub(super) fn new(target: Rc<Endpoint>) -> Self {
        Self {
            inbox: None,
            target: Some(target),
        }
    }
    fn release_target(&mut self) {
        if let Some(target) = self.target.take() {
            if self.inbox.is_some() {
                target.call_dispatched();
            }
        }
    }
}
impl Drop for CallPermit {
    fn drop(&mut self) {
        self.release_target();
        if let Some(inbox) = self.inbox.as_ref().and_then(Weak::upgrade) {
            inbox.live_calls.set(inbox.live_calls.get() - 1);
            // Admission refuses an exhausted quota synchronously; it has no
            // pending waiter to resume. The call's result/cancellation and the
            // last endpoint reference each notify their own actual observers.
        }
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use crate::scoped::ClientHooks;

    fn empty_call(shutdown: Shutdown) -> Box<Call> {
        Box::new(Call {
            kind: CallKind::Ordinary,
            credit: None,
            interface: 0,
            method: 0,
            hints: CallHints::default(),
            params: CallParams::Published,
            results: response::Results::Unsent,
            completion: CallCompletion::Unsent,
            shutdown,
            permit: CallPermit {
                inbox: None,
                target: None,
            },
        })
    }

    #[test]
    fn empty_request_cache_is_bounded_and_close_releases_all_storage() {
        for maximum in [0, 1, 4] {
            let hooks = ClientHooks::new(1, maximum);
            for _ in 0..8 {
                hooks
                    .inbox
                    .request_cache
                    .put(empty_call(hooks.inbox.shutdown.clone()));
            }
            let retained: Vec<_> =
                std::iter::from_fn(|| hooks.inbox.request_cache.take()).collect();
            assert_eq!(retained.len(), maximum);
            for call in retained {
                hooks.inbox.request_cache.put(call);
            }
            hooks.inbox.close(Error::disconnected("closed".into()));
            assert!(hooks.inbox.request_cache.take().is_none());
            assert_eq!(hooks.inbox.request_cache.rest.borrow().capacity(), 0);
            let weak = Rc::downgrade(&hooks.inbox);
            drop(hooks);
            assert!(
                weak.upgrade().is_none(),
                "cached metadata must not own its inbox"
            );
        }
    }

    #[test]
    fn recycling_drops_parameters_before_admission_and_allows_reentrant_close() {
        struct Params(Weak<Inbox>, Rc<Cell<bool>>);
        impl ParamsHook for Params {
            fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
                unreachable!()
            }
        }
        impl Drop for Params {
            fn drop(&mut self) {
                let inbox = self.0.upgrade().unwrap();
                assert_eq!(inbox.live_calls.get(), 1, "admission must still be held");
                assert!(inbox.request_cache.take().is_none());
                inbox.close(Error::disconnected("parameter destructor".into()));
                self.1.set(true);
            }
        }
        let hooks = ClientHooks::new(1, 1);
        let dropped = Rc::new(Cell::new(false));
        let mut call = empty_call(hooks.inbox.shutdown.clone());
        call.params = CallParams::Hook(Box::new(Params(
            Rc::downgrade(&hooks.inbox),
            dropped.clone(),
        )));
        call.permit.inbox = Some(Rc::downgrade(&hooks.inbox));
        hooks.inbox.live_calls.set(1);
        call.recycle_boxed();
        assert!(dropped.get());
        assert_eq!(
            Rc::strong_count(&dropped),
            1,
            "application owner must be released"
        );
        assert_eq!(hooks.inbox.live_calls.get(), 0);
        assert!(
            hooks.inbox.request_cache.take().is_none(),
            "close during clearing forbids recycling"
        );
    }
}
