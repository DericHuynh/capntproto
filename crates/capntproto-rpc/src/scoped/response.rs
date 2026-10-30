//! One completion for both operation status and result ownership. A successful
//! operation can finish before its ResultsHook is dropped; both are required.
//! This is the hosted request adapter, not a second wire ledger.
use super::*;
use crate::local::ResultsDoneHook;

struct Connection {
    shutdown: Shutdown,
    inbox: Weak<Inbox>,
}

#[derive(Default)]
pub(super) struct State {
    connection: std::cell::OnceCell<Connection>,
    // One weak shutdown subscription follows this completion allocation through
    // reuse; cached states carry no task waker or application result.
    close_watch: RefCell<Option<Shutdown>>,
    status: Cell<Option<Result<(), Box<Error>>>>,
    results: Cell<Option<ResultOwner>>,
    observed: Cell<bool>,
    ready: Cell<bool>,
    receive: Cell<Option<Waker>>,
    canceled: Cell<Option<Waker>>,
    response_interest: Cell<bool>,
    // An unsent child must observe a locally failed projection even before the
    // coordinator admits it. Publication transfers that duty to its question.
    projection: Cell<Option<Weak<Endpoint>>>,
    pipeline_interest: Cell<usize>,
    pipeline: Cell<Option<Weak<wire::PipelineState>>>,
}

pub(super) fn notify_closed(state: &Rc<State>) {
    if let Some(pipeline) = state
        .pipeline
        .take()
        .and_then(|pipeline| pipeline.upgrade())
    {
        let error = state
            .close_watch
            .borrow()
            .as_ref()
            .and_then(Shutdown::reason)
            .unwrap_or_else(|| Error::disconnected("scoped connection closed".into()));
        pipeline.break_pending(error);
    }
    let wake = state.receive.take();
    if let Some(wake) = wake {
        wake.wake();
    }
}

/// Reuse empty completions up to the inbox's existing call limit. The common
/// serial/parent-child slots stay inline; a concurrent high-water mark grows
/// the overflow lazily. Only the final owner may recycle a completed state.
pub(super) struct Cache {
    inline: [Cell<Option<Rc<State>>>; 2],
    overflow: RefCell<Vec<Rc<State>>>,
    maximum: usize,
}
impl Default for Cache {
    fn default() -> Self {
        Self::new(2)
    }
}
impl Cache {
    pub(super) fn new(maximum: usize) -> Self {
        Self {
            inline: Default::default(),
            overflow: RefCell::new(Vec::new()),
            maximum,
        }
    }
    fn acquire(&self) -> Rc<State> {
        let cached = self.inline[0]
            .take()
            .or_else(|| self.inline[1].take())
            .or_else(|| self.overflow.borrow_mut().pop());
        match cached {
            Some(state) => {
                debug_assert_eq!(Rc::strong_count(&state), 1, "exclusive cached completion");
                debug_assert!(
                    !has_value(&state.status)
                        && !has_value(&state.results)
                        && !has_value(&state.receive)
                        && !has_value(&state.canceled)
                        && !has_value(&state.projection)
                );
                state.observed.set(false);
                state.ready.set(false);
                state
            }
            None => Rc::default(),
        }
    }
    fn recycle(&self, state: &Rc<State>) {
        if Rc::strong_count(state) != 1 {
            return;
        }
        // No callbacks occur while cached ownership is temporarily removed.
        for slot in self.inline.iter().take(self.maximum) {
            let cached = slot.take();
            let occupied = cached.is_some();
            slot.set(Some(cached.unwrap_or_else(|| state.clone())));
            if !occupied {
                return;
            }
        }
        let mut overflow = self.overflow.borrow_mut();
        if overflow.len() < self.maximum.saturating_sub(self.inline.len()) {
            overflow.push(state.clone());
        }
    }
}
enum ResultOwner {
    Done(Box<dyn ResultsDoneHook>),
    Response(Box<dyn capnp::private::capability::ResponseHook>),
}
// These inspections never invoke application code: the temporary owner returns
// to its cell before any waker or payload can be cloned, dropped or polled.
#[inline(always)]
fn has_value<T>(slot: &Cell<Option<T>>) -> bool {
    let value = slot.take();
    let present = value.is_some();
    slot.set(value);
    present
}
#[inline(always)]
fn registered(slot: &Cell<Option<Waker>>, wake: &Waker) -> bool {
    let value = slot.take();
    let same = value.as_ref().is_some_and(|old| old.will_wake(wake));
    slot.set(value);
    same
}
impl State {
    fn watch_close(self: &Rc<Self>, shutdown: &Shutdown) {
        let mut subscription = self.close_watch.borrow_mut();
        // A cache belongs to one inbox and one terminal shutdown state. The
        // weak observer does not keep a completed response or its task alive.
        let subscription = subscription.get_or_insert_with(|| shutdown.clone());
        subscription.watch_response(self);
    }
    fn ready(&self) -> bool {
        self.ready.get()
    }
}

pub(crate) struct Completion(Rc<State>);
pub(crate) struct ResultsSender(Rc<State>);

/// A public pipeline keeps the operation alive without polling a second future.
/// The wire owner only holds PipelineState, never this application interest.
pub(super) struct PipelineInterest(Rc<State>);
impl Clone for PipelineInterest {
    fn clone(&self) -> Self {
        self.0.pipeline_interest.set(
            self.0
                .pipeline_interest
                .get()
                .checked_add(1)
                .expect("pipeline interest overflow"),
        );
        Self(self.0.clone())
    }
}
impl Drop for PipelineInterest {
    fn drop(&mut self) {
        let remaining = self.0.pipeline_interest.get() - 1;
        self.0.pipeline_interest.set(remaining);
        if remaining == 0 && !self.0.response_interest.get() {
            let inbox = self
                .0
                .connection
                .get()
                .map(|connection| connection.inbox.clone())
                .unwrap_or_default();
            self.0.cancel_response(&inbox);
        }
    }
}
impl State {
    fn retain_pipeline(self: &Rc<Self>, pipeline: &Rc<wire::PipelineState>) -> PipelineInterest {
        assert_eq!(self.pipeline_interest.replace(1), 0);
        self.pipeline.set(Some(Rc::downgrade(pipeline)));
        PipelineInterest(self.clone())
    }

    /// Complete projections at the producer, before waking the response consumer.
    /// Remote wire completion normally already installed its admitted pipeline;
    /// the fallback handles locally forwarded results and admission failures.
    fn notify_ready(&self) {
        if !self.ready.get() {
            return;
        }
        if let Some(pipeline) = self.pipeline.take().and_then(|pipeline| pipeline.upgrade()) {
            if pipeline.unresolved() {
                let status = self.status.take().expect("ready operation status");
                let error = status.as_ref().err().map(|error| (**error).clone());
                self.status.set(Some(status));
                match error {
                    Some(error) => pipeline.break_pending(error),
                    None => {
                        let response = match self.results.take().expect("ready operation results") {
                            ResultOwner::Done(results) => {
                                Box::new(crate::local::Response::new(results))
                                    as Box<dyn capnp::private::capability::ResponseHook>
                            }
                            ResultOwner::Response(response) => response,
                        };
                        let (response, completed) = crate::local::Pipeline::share_response(
                            capability::Response::new(response),
                        );
                        // Restore the response before resolving any user hooks.
                        self.results.set(Some(ResultOwner::Response(response.hook)));
                        pipeline.finish_pending(|| completed);
                    }
                }
            }
        }
        if let Some(wake) = self.receive.take() {
            wake.wake();
        }
    }
}

// Most remote no-pipeline calls only transfer an admitted payload. Build the
// general ResultsHook lazily, if a local producer or public accessor needs it.
pub(super) enum Results {
    Unsent,
    Hook(Box<dyn ResultsHook>),
    Direct(DirectResults),
}
pub(super) struct DirectResults {
    sender: Option<ResultsSender>,
    context: Option<Rc<dyn std::any::Any>>,
    pipeline: Option<Weak<wire::PipelineState>>,
}
impl DirectResults {
    fn take_hook(&mut self) -> crate::local::Results {
        let sender = self.sender.take().expect("unconsumed direct results");
        let context = self.context.take();
        match self.pipeline.take().and_then(|pipeline| pipeline.upgrade()) {
            Some(pipeline) => crate::local::Results::direct_pipelined(
                sender,
                context,
                crate::queued::PipelineInnerSender::wire(&pipeline),
            ),
            None => crate::local::Results::direct(sender, context),
        }
    }
    fn into_hook(mut self) -> Box<dyn ResultsHook> {
        Box::new(self.take_hook())
    }
}
impl Drop for DirectResults {
    fn drop(&mut self) {
        if self.sender.is_some() {
            // An untouched local result still completes with empty content.
            drop(self.take_hook());
        }
    }
}

struct SharedPayloadResponse {
    payload: Rc<wire::Payload>,
    _context: Option<Rc<dyn std::any::Any>>,
}
impl capnp::private::capability::ResponseHook for SharedPayloadResponse {
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        capnp::private::capability::ResponseHook::get(&*self.payload)
    }
}

impl Results {
    /// The private direct request owns both halves of one completion. Transfer
    /// the already admitted reader and successful status under one borrow;
    /// arbitrary ResultsHook implementations retain the ordinary two-phase path.
    pub(super) fn complete_plain(
        &mut self,
        completion: &CallCompletion,
        mut message: Box<capntproto_rpc_wire::owned::PlainResults>,
    ) -> Result<(), Box<capntproto_rpc_wire::owned::PlainResults>> {
        let (Self::Direct(direct), CallCompletion::Response(completion)) = (self, completion)
        else {
            return Err(message);
        };
        let Some(sender) = direct.sender.as_ref() else {
            return Err(message);
        };
        if !Rc::ptr_eq(&sender.0, &completion.0) {
            return Err(message);
        }
        let sender = direct.sender.take().expect("checked direct sender");
        if let Some(context) = direct.context.take() {
            message.retain_owner(context);
        }
        let (wake, canceled) = {
            let state = &sender.0;
            if state.observed.get() {
                return Ok(());
            }
            debug_assert!(!has_value(&state.status) && !has_value(&state.results));
            state.results.set(Some(ResultOwner::Response(message)));
            state.status.set(Some(Ok(())));
            state.ready.set(true);
            (state.receive.take(), state.canceled.take())
        };
        drop(canceled);
        if let Some(wake) = wake {
            wake.wake();
        }
        Ok(())
    }
    fn direct(sender: ResultsSender, context: Option<Rc<dyn std::any::Any>>) -> Self {
        Self::Direct(DirectResults {
            sender: Some(sender),
            context,
            pipeline: None,
        })
    }
    pub(super) fn take_wire_pipeline(&mut self) -> Option<Option<Rc<wire::PipelineState>>> {
        match self {
            Self::Direct(direct) => direct.pipeline.take().map(|pipeline| pipeline.upgrade()),
            Self::Hook(hook) => hook
                .as_any_mut()
                .and_then(|hook| hook.downcast_mut::<crate::local::Results>())
                .and_then(crate::local::Results::take_wire_pipeline),
            Self::Unsent => None,
        }
    }
    /// A wire reply owns both halves of a direct request. Finish projections
    /// before making the response ready, then publish payload and status once.
    #[allow(clippy::result_large_err)]
    pub(super) fn complete_payload(
        &mut self,
        completion: &CallCompletion,
        payload: wire::Payload,
        pipeline: Option<&Rc<wire::PipelineState>>,
        early_pipeline: impl FnOnce() -> Option<Box<dyn capnp::private::capability::PipelineHook>>,
    ) -> Result<(), wire::Payload> {
        let (Self::Direct(direct), CallCompletion::Response(completion)) = (self, completion)
        else {
            return Err(payload);
        };
        let Some(sender) = direct.sender.as_ref() else {
            return Err(payload);
        };
        if !Rc::ptr_eq(&sender.0, &completion.0) {
            return Err(payload);
        }
        let sender = direct.sender.take().expect("checked direct sender");
        let response = if let Some(pipeline) = pipeline {
            let payload = Rc::new(payload);
            let completed = early_pipeline();
            let response = Box::new(SharedPayloadResponse {
                payload: payload.clone(),
                _context: direct.context.take(),
            });
            match completed {
                Some(completed) => pipeline.finish(completed),
                None => pipeline.finish_payload(payload),
            }
            response as Box<dyn capnp::private::capability::ResponseHook>
        } else {
            owned_response(payload, direct.context.take())
        };
        let state = &sender.0;
        if state.observed.get() {
            return Ok(());
        }
        debug_assert!(!has_value(&state.status) && !has_value(&state.results));
        state.results.set(Some(ResultOwner::Response(response)));
        state.status.set(Some(Ok(())));
        state.ready.set(true);
        // The wire producer above completed this exact request's projections.
        drop(state.pipeline.take());
        drop(state.canceled.take());
        if let Some(wake) = state.receive.take() {
            wake.wake();
        }
        Ok(())
    }
    pub(super) fn into_hook(self) -> Box<dyn ResultsHook> {
        match self {
            Self::Unsent => unreachable!("unadmitted request"),
            Self::Hook(hook) => hook,
            Self::Direct(direct) => direct.into_hook(),
        }
    }
    pub(super) fn hook(&mut self) -> &mut dyn ResultsHook {
        if let Self::Direct(direct) = self {
            let hook = direct.take_hook();
            *self = Self::Hook(Box::new(hook));
        }
        match self {
            Self::Unsent => unreachable!("unadmitted request"),
            Self::Hook(hook) => &mut **hook,
            Self::Direct(_) => unreachable!(),
        }
    }
    pub(super) fn retain_context(&mut self, context: Rc<dyn std::any::Any>) {
        match self {
            Self::Unsent => unreachable!("unadmitted request"),
            Self::Hook(hook) => hook.retain_response_context(context),
            Self::Direct(direct) => crate::admission::retain_context(&mut direct.context, context),
        }
    }
    // Return the intact payload to the fallback; boxing it adds an allocation.
    #[allow(clippy::result_large_err)]
    pub(super) fn finish_admitted(&mut self, payload: wire::Payload) -> Result<(), wire::Payload> {
        match self {
            Self::Unsent => unreachable!("unadmitted request"),
            Self::Direct(direct) => {
                direct
                    .sender
                    .take()
                    .expect("unconsumed direct results")
                    .send_payload(payload, direct.context.take());
                Ok(())
            }
            Self::Hook(hook) => match hook
                .as_any_mut()
                .and_then(|hook| hook.downcast_mut::<crate::local::Results>())
            {
                Some(hook) => hook.finish_admitted_response(payload),
                None => Err(payload),
            },
        }
    }

    // A plain reply has no capability imports or pipeline publication to
    // coordinate. Exact local sinks can retain its admission proof; opaque
    // result hooks still receive their ordinary validating set_response call.
    pub(super) fn finish_plain(&mut self, payload: wire::Payload) -> capnp::Result<()> {
        let payload = match self.finish_admitted(payload) {
            Ok(()) => return Ok(()),
            Err(payload) => Rc::new(payload),
        };
        let results = self.hook();
        if let Some(local) = results
            .as_any_mut()
            .and_then(|sink| sink.downcast_mut::<crate::local::Results>())
        {
            local.set_admitted_response(payload);
            Ok(())
        } else {
            results.set_response(payload)
        }
    }
}
#[cfg(test)]
struct Response {
    state: Rc<State>,
    shutdown: Shutdown,
    inbox: Weak<Inbox>,
}

impl Completion {
    pub(super) fn published(&self) {
        self.0.projection.take();
    }

    pub(super) fn tail_acknowledged(&self) {
        // ResultsSentElsewhere completes execution, not the promised results.
        // The wire owner retains that pipeline through the original Finish.
        self.0.pipeline.take();
    }

    pub(super) fn is_observed(&self) -> bool {
        self.0.observed.get()
    }
    pub(super) fn poll_observed(&self, cx: &mut Context<'_>) -> Poll<()> {
        {
            let state = &self.0;
            if state.observed.get() {
                return Poll::Ready(());
            }
            if registered(&state.canceled, cx.waker()) {
                return Poll::Pending;
            }
        }
        let wake = cx.waker().clone();
        let old = self.0.canceled.replace(Some(wake));
        drop(old);
        if self.0.observed.get() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
    pub(super) fn complete(&self, result: capnp::Result<()>) {
        let state = &self.0;
        if has_value(&state.status) || state.observed.get() {
            return;
        }
        state
            .ready
            .set(result.is_err() || has_value(&state.results));
        state.status.set(Some(result.map_err(Box::new)));
        // This operation is complete; later observation needs no cancellation poll.
        drop(state.canceled.take());
        state.notify_ready();
    }
}
impl Drop for Completion {
    fn drop(&mut self) {
        if !has_value(&self.0.status) && !self.is_observed() {
            self.complete(Err(Error::disconnected("scoped request canceled".into())));
        }
    }
}
impl ResultsSender {
    pub(crate) fn send(self, result: Box<dyn ResultsDoneHook>) {
        self.finish(ResultOwner::Done(result));
    }
    pub(crate) fn send_response(self, result: Box<dyn capnp::private::capability::ResponseHook>) {
        self.finish(ResultOwner::Response(result));
    }
    fn finish(self, result: ResultOwner) {
        let state = &self.0;
        if state.observed.get() {
            return;
        }
        debug_assert!(!has_value(&state.results));
        state.results.set(Some(result));
        state.ready.set(has_value(&state.status));
        state.notify_ready();
    }
}
#[cold]
#[inline(never)]
fn closed_reason(shutdown: &Shutdown) -> Error {
    shutdown.reason().expect("closed connection")
}
impl State {
    fn promise(
        self: Rc<Self>,
        inbox: Rc<Inbox>,
    ) -> capability::ResponsePromise<any_pointer::Owned> {
        self.connection.get_or_init(|| {
            // The weak subscription follows this allocation through cache reuse.
            // Its terminal connection never needs another registration.
            let shutdown = inbox.shutdown.clone();
            self.watch_close(&shutdown);
            Connection {
                shutdown,
                inbox: Rc::downgrade(&inbox),
            }
        });
        self.response_interest.set(true);
        capability::ResponsePromise::from_shared(self)
    }
    fn poll_response(
        &self,
        shutdown: &Shutdown,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Box<dyn capnp::private::capability::ResponseHook>, Box<Error>>> {
        if shutdown.is_closed() {
            return Poll::Ready(Err(Box::new(closed_reason(shutdown))));
        }
        if !self.ready() {
            if has_value(&self.projection) {
                if let Some(error) = self.projection_failure(cx) {
                    return Poll::Ready(Err(Box::new(error)));
                }
            }
            let registered = registered(&self.receive, cx.waker());
            if !registered {
                let wake = cx.waker().clone();
                let old = self.receive.replace(Some(wake));
                drop(old);
            }
            // A custom Waker clone/drop may close the connection while the
            // completion notification is being installed.
            if shutdown.is_closed() {
                return Poll::Ready(Err(Box::new(closed_reason(shutdown))));
            }
        }
        let (status, results, canceled) = {
            let state = &self;
            if !state.ready() {
                return Poll::Pending;
            }
            state.observed.set(true);
            state.ready.set(false);
            (
                state.status.take().unwrap(),
                state.results.take(),
                state.canceled.take(),
            )
        };
        if let Some(wake) = canceled {
            wake.wake();
        }
        Poll::Ready(status.map(
            |()| match results.expect("successful operation owns results") {
                ResultOwner::Done(results) => Box::new(crate::local::Response::new(results)),
                ResultOwner::Response(response) => response,
            },
        ))
    }
    #[cold]
    #[inline(never)]
    fn projection_failure(&self, cx: &mut Context<'_>) -> Option<Error> {
        let projection = self.projection.take();
        let endpoint = projection.as_ref().and_then(Weak::upgrade);
        self.projection.set(projection);
        let endpoint = endpoint?;
        endpoint.binding_pending(cx);
        if endpoint.index.get().is_none() {
            Hook(endpoint, None).get_failure()
        } else {
            None
        }
    }
    fn cancel_response(self: &Rc<Self>, inbox: &Weak<Inbox>) {
        self.response_interest.set(false);
        self.projection.take();
        if self.pipeline_interest.get() != 0 && !self.observed.get() {
            // The response future is gone, but an owned projection still needs
            // the operation. Completion publishes its pipeline without polling.
            drop(self.receive.take());
            return;
        }
        self.pipeline.take();
        let (status, results, receive, canceled, abandoned) = {
            let state = &self;
            let abandoned = !state.observed.get() && !has_value(&state.status);
            state.observed.set(true);
            state.ready.set(false);
            (
                state.status.take(),
                state.results.take(),
                state.receive.take(),
                state.canceled.take(),
                abandoned,
            )
        };
        drop((status, results, receive));
        if let Some(wake) = canceled {
            wake.wake();
        }
        if abandoned {
            if let Some(inbox) = inbox.upgrade() {
                inbox.notify();
            }
        }
        if let Some(inbox) = inbox.upgrade().filter(|inbox| !inbox.closed.get()) {
            inbox.completion_cache.recycle(self);
        }
    }
}
impl
    capnp::private::capability::SharedPromise<
        Box<dyn capnp::private::capability::ResponseHook>,
        Error,
    > for State
{
    fn poll(
        &self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Box<dyn capnp::private::capability::ResponseHook>, Error>> {
        self.poll_response(
            &self.connection.get().expect("admitted completion").shutdown,
            cx,
        )
        .map(|result| result.map_err(|error| *error))
    }
    fn cancel(self: Rc<Self>) {
        self.cancel_response(&self.connection.get().expect("admitted completion").inbox);
    }
}
#[cfg(test)]
impl Future for Response {
    type Output = capnp::Result<capability::Response<any_pointer::Owned>>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if !self.state.ready() {
            self.state.watch_close(&self.shutdown);
        }
        self.state.poll_response(&self.shutdown, cx).map(|result| {
            result
                .map(capability::Response::new)
                .map_err(|error| *error)
        })
    }
}
#[cfg(test)]
impl Drop for Response {
    fn drop(&mut self) {
        self.state.cancel_response(&self.inbox);
    }
}

pub(crate) fn available(client: &dyn ClientHook) -> bool {
    client
        .as_any()
        .and_then(|client| client.downcast_ref::<Hook>())
        .or_else(|| wire::direct_response_hook(client))
        .is_some_and(|hook| hook.introduced().is_none())
}
pub(crate) fn request(
    client: &dyn ClientHook,
    interface: u64,
    method: u16,
    hints: CallHints,
    params: Box<crate::local::Request>,
    context: Option<Rc<dyn std::any::Any>>,
) -> capability::RemotePromise<any_pointer::Owned> {
    let hook = client
        .as_any()
        .and_then(|client| client.downcast_ref::<Hook>());
    match hook {
        Some(hook) => request_scoped(hook, interface, method, hints, params, context),
        None => wire::request_projected(client, interface, method, hints, params, context),
    }
}

pub(crate) fn request_scoped(
    hook: &Hook,
    interface: u64,
    method: u16,
    hints: CallHints,
    params: Box<crate::local::Request>,
    context: Option<Rc<dyn std::any::Any>>,
) -> capability::RemotePromise<any_pointer::Owned> {
    let state = hook
        .0
        .inbox
        .upgrade()
        .map_or_else(Rc::default, |inbox| inbox.completion_cache.acquire());
    if hook.0.index.get().is_none() {
        state.projection.set(Some(Rc::downgrade(&hook.0)));
    }
    let results = Results::direct(ResultsSender(state.clone()), context);
    let completion = CallCompletion::Response(Completion(state.clone()));
    let admitted = hook.enqueue(
        interface,
        method,
        CallParams::owned_request(params),
        results,
        hints,
        None,
        completion,
    );
    let promise = match admitted {
        Ok(inbox) => state.promise(inbox),
        Err(error) => capability::ResponsePromise::new(Promise::err(*error)),
    };
    capability::RemotePromise {
        promise,
        pipeline: any_pointer::Pipeline::new(Box::new(crate::broken::DisabledPipeline)),
    }
}

pub(super) fn request_prepared(
    mut call: Box<Call>,
) -> capability::RemotePromise<any_pointer::Owned> {
    let inbox = call.endpoint().inbox.upgrade();
    let state = inbox
        .as_ref()
        .map_or_else(Rc::default, |inbox| inbox.completion_cache.acquire());
    // Admission transfers the request credit to its results exactly once.
    // This private request has no independent application context to merge.
    call.results = Results::direct(ResultsSender(state.clone()), None);
    call.completion = CallCompletion::Response(Completion(state.clone()));
    let promise = match call.enqueue_owned(inbox) {
        Ok(inbox) => state.promise(inbox),
        Err(error) => capability::ResponsePromise::new(Promise::err(*error)),
    };
    capability::RemotePromise {
        promise,
        pipeline: any_pointer::Pipeline::new(Box::new(crate::broken::DisabledPipeline)),
    }
}

// Keep the ordinary split/pipeline interests and early set_pipeline semantics,
// but join operation status and result ownership in the same direct completion
// as non-pipelined requests. The prepared request also keeps its wire envelope.
pub(super) fn request_pipelined_prepared(
    mut call: Box<Call>,
) -> capability::RemotePromise<any_pointer::Owned> {
    let inbox = call.endpoint().inbox.upgrade();
    let state = inbox
        .as_ref()
        .map_or_else(Rc::default, |inbox| inbox.completion_cache.acquire());
    let pipeline = wire::PipelineState::prepared(inbox.as_ref());
    let interest = state.retain_pipeline(&pipeline);
    call.results = Results::Direct(DirectResults {
        sender: Some(ResultsSender(state.clone())),
        context: None,
        pipeline: Some(Rc::downgrade(&pipeline)),
    });
    call.completion = CallCompletion::Response(Completion(state.clone()));
    let promise = match call.enqueue_owned(inbox) {
        Ok(inbox) => state.promise(inbox),
        Err(error) => {
            pipeline.break_pending((*error).clone());
            capability::ResponsePromise::new(Promise::err(*error))
        }
    };
    capability::RemotePromise {
        promise,
        pipeline: any_pointer::Pipeline::new(pipeline.owned(interest)),
    }
}

struct OwnedResponse {
    payload: wire::Payload,
    _context: Option<Rc<dyn std::any::Any>>,
}
impl capnp::private::capability::ResponseHook for OwnedResponse {
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        capnp::private::capability::ResponseHook::get(&self.payload)
    }
}
fn owned_response(
    mut payload: wire::Payload,
    context: Option<Rc<dyn std::any::Any>>,
) -> Box<dyn capnp::private::capability::ResponseHook> {
    let context = context.and_then(|context| payload.retain_plain_context(context).err());
    if context.is_none() {
        payload.into_response()
    } else {
        Box::new(OwnedResponse {
            payload,
            _context: context,
        })
    }
}
impl ResultsSender {
    pub(crate) fn send_payload(
        self,
        payload: wire::Payload,
        context: Option<Rc<dyn std::any::Any>>,
    ) {
        self.send_response(owned_response(payload, context));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    struct WakeCount(AtomicUsize);
    impl std::task::Wake for WakeCount {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[test]
    fn projection_failure_is_observed_before_publication_and_does_not_replace_child_results() {
        for published in [false, true] {
            let hooks = ClientHooks::new(4, 4);
            let state = Rc::new(State::default());
            let completion = Completion(state.clone());
            let sender = ResultsSender(state.clone());
            let endpoint = Endpoint::projection(Rc::downgrade(&hooks.inbox));
            state.projection.set(Some(Rc::downgrade(&endpoint)));
            let mut promise = state.clone().promise(hooks.inbox.clone());
            let wake = Arc::new(WakeCount(AtomicUsize::new(0)));
            let waker = Waker::from(wake.clone());
            let mut cx = Context::from_waker(&waker);
            assert!(Pin::new(&mut promise).poll(&mut cx).is_pending());
            if published {
                completion.published();
            }
            endpoint.forward_projection(Err(Error::overloaded("parent failed".into())));
            assert!(wake.0.load(Ordering::SeqCst) != 0);
            let result = Pin::new(&mut promise).poll(&mut cx);
            if published {
                assert!(result.is_pending(), "a published child owns its response");
                sender.send(results("child response"));
                completion.complete(Ok(()));
                let Poll::Ready(Ok(response)) = Pin::new(&mut promise).poll(&mut cx) else {
                    panic!("child response")
                };
                assert_eq!(
                    response
                        .get()
                        .unwrap()
                        .get_as::<capnp::text::Reader>()
                        .unwrap(),
                    "child response"
                );
            } else {
                let Poll::Ready(Err(error)) = result else {
                    panic!("unsent child must observe the parent's original failure")
                };
                assert_eq!(error.kind, capnp::ErrorKind::Overloaded);
                assert_eq!(error.extra, "parent failed");
                drop(sender);
            }
            drop(promise);
            assert!(
                !has_value(&state.projection),
                "recycled completion retained a prior target"
            );
        }
    }

    #[test]
    fn fused_reply_finishes_pipeline_and_retains_response_context() {
        for early in [false, true] {
            let hooks = ClientHooks::new(8, 8);
            let state = Rc::new(State::default());
            let pipeline = wire::PipelineState::prepared(Some(&hooks.inbox));
            let interest = state.retain_pipeline(&pipeline);
            let completion = CallCompletion::Response(Completion(state.clone()));
            let context = Rc::new(());
            let weak = Rc::downgrade(&context);
            let mut results = Results::direct(ResultsSender(state.clone()), Some(context));
            let mut promise = state.promise(hooks.inbox.clone());
            let wake = Arc::new(WakeCount(AtomicUsize::new(0)));
            let waker = Waker::from(wake.clone());
            assert!(Pin::new(&mut promise)
                .poll(&mut Context::from_waker(&waker))
                .is_pending());
            let mut message = capnp::message::Builder::new_default();
            message
                .set_root::<capnp::text::Owned>("fused reply")
                .unwrap();
            let payload = wire::Payload::copy(message.get_root_as_reader().unwrap()).unwrap();
            assert!(results
                .complete_payload(&completion, payload, Some(&pipeline), || {
                    early.then(|| {
                        Box::new(crate::broken::Pipeline::new(Error::failed("early".into())))
                            as Box<dyn capnp::private::capability::PipelineHook>
                    })
                })
                .is_ok());
            assert!(!pipeline.unresolved());
            assert_eq!(wake.0.load(Ordering::SeqCst), 1);
            if early {
                assert_eq!(
                    pipeline
                        .owned(interest)
                        .get_pipelined_cap(&[])
                        .get_failure()
                        .unwrap()
                        .extra,
                    "early"
                );
            } else {
                drop(interest);
            }
            let Poll::Ready(Ok(response)) =
                Pin::new(&mut promise).poll(&mut Context::from_waker(&waker))
            else {
                panic!("ready response")
            };
            drop((completion, results, promise));
            assert_eq!(
                response
                    .get()
                    .unwrap()
                    .get_as::<capnp::text::Reader>()
                    .unwrap(),
                "fused reply"
            );
            assert!(weak.upgrade().is_some());
            drop(response);
            assert!(weak.upgrade().is_none());
        }
    }

    #[test]
    fn producer_completes_unpolled_pipeline_and_pipeline_only_interest() {
        for drop_response in [false, true] {
            let hooks = ClientHooks::new(8, 8);
            let state = hooks.inbox.completion_cache.acquire();
            let pipeline = wire::PipelineState::prepared(Some(&hooks.inbox));
            let interest = state.retain_pipeline(&pipeline);
            let completion = Completion(state.clone());
            let sender = ResultsSender(state.clone());
            let mut promise = Some(state.clone().promise(hooks.inbox.clone()));
            if drop_response {
                drop(promise.take());
                assert!(!completion.is_observed(), "pipeline retains the operation");
            }
            completion.complete(Ok(()));
            assert!(pipeline.unresolved(), "results ownership has not arrived");
            sender.send(results("completed without polling"));
            assert!(
                !pipeline.unresolved(),
                "producer must publish the pipeline directly"
            );
            if let Some(promise) = &mut promise {
                let Poll::Ready(Ok(response)) =
                    Pin::new(promise).poll(&mut Context::from_waker(Waker::noop()))
                else {
                    panic!("ready response")
                };
                assert_eq!(
                    response
                        .get()
                        .unwrap()
                        .get_as::<capnp::text::Reader>()
                        .unwrap(),
                    "completed without polling"
                );
            }
            drop(promise);
            drop(interest);
            assert!(completion.is_observed());
        }
    }

    #[test]
    fn final_pipeline_interest_cancels_without_a_response_future() {
        let hooks = ClientHooks::new(8, 8);
        let state = hooks.inbox.completion_cache.acquire();
        let pipeline = wire::PipelineState::prepared(Some(&hooks.inbox));
        let interest = state.retain_pipeline(&pipeline);
        let completion = Completion(state.clone());
        let promise = state.clone().promise(hooks.inbox.clone());
        drop(promise);
        assert!(!completion.is_observed());
        let retained = interest.clone();
        drop(interest);
        assert!(!completion.is_observed());
        drop(retained);
        assert!(completion.is_observed());
    }

    #[test]
    fn shutdown_breaks_unpolled_pipeline_and_preserves_an_early_pipeline() {
        for early in [false, true] {
            let hooks = ClientHooks::new(8, 8);
            let state = hooks.inbox.completion_cache.acquire();
            let pipeline = wire::PipelineState::prepared(Some(&hooks.inbox));
            let interest = state.retain_pipeline(&pipeline);
            let _promise = state.promise(hooks.inbox.clone());
            if early {
                pipeline.publish(Box::new(crate::broken::Pipeline::new(Error::failed(
                    "early".into(),
                ))));
            }
            hooks.inbox.close(Error::disconnected("terminal".into()));
            assert!(!pipeline.unresolved());
            let public = pipeline.owned(interest);
            assert_eq!(
                public.get_pipelined_cap(&[]).get_failure().unwrap().extra,
                if early { "early" } else { "terminal" }
            );
        }
    }

    #[test]
    fn direct_response_recycles_completion_and_shutdown_wakes_the_next_use() {
        let hooks = ClientHooks::new(8, 8);
        let mut address = None;
        for value in ["first", "second"] {
            let state = hooks.inbox.completion_cache.acquire();
            let pointer = Rc::as_ptr(&state);
            assert_eq!(*address.get_or_insert(pointer), pointer);
            let completion = Completion(state.clone());
            let sender = ResultsSender(state.clone());
            let mut promise = state.promise(hooks.inbox.clone());
            assert!(Pin::new(&mut promise)
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending());
            completion.complete(Ok(()));
            drop(completion);
            sender.send(results(value));
            let Poll::Ready(Ok(response)) =
                Pin::new(&mut promise).poll(&mut Context::from_waker(Waker::noop()))
            else {
                panic!("completed")
            };
            drop(promise);
            assert_eq!(
                response
                    .get()
                    .unwrap()
                    .get_as::<capnp::text::Reader>()
                    .unwrap(),
                value
            );
        }
        let state = hooks.inbox.completion_cache.acquire();
        assert_eq!(Some(Rc::as_ptr(&state)), address);
        let completion = Completion(state.clone());
        let mut promise = state.promise(hooks.inbox.clone());
        let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
        let wake = Waker::from(wakes.clone());
        assert!(Pin::new(&mut promise)
            .poll(&mut Context::from_waker(&wake))
            .is_pending());
        hooks.inbox.close(Error::disconnected("terminal".into()));
        assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
        assert!(
            matches!(Pin::new(&mut promise).poll(&mut Context::from_waker(&wake)), Poll::Ready(Err(error)) if error.extra == "terminal")
        );
        drop(promise);
        assert!(completion.is_observed());
        assert!(hooks
            .inbox
            .completion_cache
            .inline
            .iter()
            .all(|slot| !has_value(slot)));
    }
    #[test]
    fn completion_cache_reuses_only_exclusive_empty_state() {
        let cache = Cache::default();
        let state = cache.acquire();
        let address = Rc::as_ptr(&state);
        state.observed.set(true);
        let pending_owner = state.clone();
        cache.recycle(&state);
        assert!(cache.inline.iter().all(|slot| !has_value(slot)));
        drop(pending_owner);
        cache.recycle(&state);
        drop(state);
        let reused = cache.acquire();
        assert_eq!(Rc::as_ptr(&reused), address);
        assert!(!reused.observed.get());
        assert!(!reused.ready());
        let other = cache.acquire();
        assert!(!Rc::ptr_eq(&other, &reused));
        cache.recycle(&reused);
        cache.recycle(&other);
        drop(reused);
        let first = cache.acquire();
        assert_eq!(Rc::as_ptr(&first), address);
        assert!(!Rc::ptr_eq(&first, &other));
    }

    #[test]
    fn completion_cache_reuses_two_states_and_releases_overflow() {
        let cache = Cache::default();
        let first = cache.acquire();
        let second = cache.acquire();
        let overflow = cache.acquire();
        let addresses = [Rc::as_ptr(&first), Rc::as_ptr(&second)];
        let overflow_owner = Rc::downgrade(&overflow);
        cache.recycle(&first);
        cache.recycle(&second);
        cache.recycle(&overflow);
        drop((first, second, overflow));
        assert!(overflow_owner.upgrade().is_none());
        for _ in 0..4 {
            let first = cache.acquire();
            let second = cache.acquire();
            assert_eq!([Rc::as_ptr(&first), Rc::as_ptr(&second)], addresses);
            assert!(!Rc::ptr_eq(&first, &second));
            // A second interest prevents either slot from recycling this
            // completion, including when the other slot remains available.
            let retained = second.clone();
            cache.recycle(&first);
            cache.recycle(&second);
            assert!(!has_value(&cache.inline[1]));
            drop(retained);
            cache.recycle(&second);
        }
    }

    #[test]
    fn concurrent_completion_cache_is_bounded_and_reuses_its_high_water_mark() {
        for maximum in [0, 1, 2, 9, 37] {
            let cache = Cache::new(maximum);
            let mut states = (0..maximum + 1)
                .map(|_| cache.acquire())
                .collect::<Vec<_>>();
            let overflow = Rc::downgrade(states.last().unwrap());
            for state in &states {
                cache.recycle(state);
            }
            states.clear();
            assert!(
                overflow.upgrade().is_none(),
                "cache exceeded its call limit"
            );
            for _ in 0..3 {
                let measured = allocation_counter::measure(|| {
                    states.extend((0..maximum).map(|_| cache.acquire()));
                    for state in &states {
                        cache.recycle(state);
                    }
                    states.clear();
                });
                assert_eq!(measured.count_total, 0);
            }
        }
    }

    #[test]
    fn recycled_completion_keeps_one_weak_shutdown_subscription() {
        let cache = Cache::default();
        let shutdown = Shutdown::new();
        let state = cache.acquire();
        let address = Rc::as_ptr(&state);
        let first_wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
        let first_waker = Waker::from(first_wakes.clone());
        let mut first = Response {
            state: state.clone(),
            shutdown: shutdown.clone(),
            inbox: Weak::new(),
        };
        assert!(Pin::new(&mut first)
            .poll(&mut Context::from_waker(&first_waker))
            .is_pending());
        assert_eq!(Rc::weak_count(&state), 1);
        ResultsSender(state.clone()).send(results("first"));
        let completion = Completion(state.clone());
        completion.complete(Ok(()));
        drop(completion);
        assert!(Pin::new(&mut first)
            .poll(&mut Context::from_waker(&first_waker))
            .is_ready());
        drop(first);
        assert_eq!(first_wakes.0.load(Ordering::SeqCst), 1);
        cache.recycle(&state);
        drop(state);

        let state = cache.acquire();
        assert_eq!(Rc::as_ptr(&state), address);
        let second_wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
        let second_waker = Waker::from(second_wakes.clone());
        let mut second = Response {
            state: state.clone(),
            shutdown: shutdown.clone(),
            inbox: Weak::new(),
        };
        assert!(Pin::new(&mut second)
            .poll(&mut Context::from_waker(&second_waker))
            .is_pending());
        assert_eq!(
            Rc::weak_count(&state),
            1,
            "reuse must not append another observer"
        );
        shutdown.close(Error::disconnected("connection stopped".into()));
        assert_eq!(first_wakes.0.load(Ordering::SeqCst), 1);
        assert_eq!(second_wakes.0.load(Ordering::SeqCst), 1);
        let Poll::Ready(Err(error)) =
            Pin::new(&mut second).poll(&mut Context::from_waker(&second_waker))
        else {
            panic!("shutdown must wake the reused completion")
        };
        assert_eq!(error.kind, capnp::ErrorKind::Disconnected);
        assert_eq!(Rc::weak_count(&state), 0);
    }

    fn pair() -> (Completion, ResultsSender, Response) {
        let state = Rc::new(State::default());
        (
            Completion(state.clone()),
            ResultsSender(state.clone()),
            Response {
                state,
                shutdown: Shutdown::new(),
                inbox: Weak::new(),
            },
        )
    }
    fn results(text: &str) -> Box<dyn ResultsDoneHook> {
        let mut builder = capnp::message::Builder::new_default();
        builder.set_root(text).unwrap();
        Box::new(crate::local::ResultsDone::new(builder, Vec::new()))
    }
    #[test]
    fn success_waits_for_both_status_and_retained_results_in_either_order() {
        for status_first in [false, true] {
            let (completion, sender, mut response) = pair();
            let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
            let waker = Waker::from(wakes.clone());
            let mut cx = Context::from_waker(&waker);
            assert!(Pin::new(&mut response).poll(&mut cx).is_pending());
            if status_first {
                completion.complete(Ok(()));
                drop(completion);
                assert!(Pin::new(&mut response).poll(&mut cx).is_pending());
                sender.send(results("retained"));
            } else {
                sender.send(results("retained"));
                assert!(Pin::new(&mut response).poll(&mut cx).is_pending());
                completion.complete(Ok(()));
                drop(completion);
            }
            assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
            let Poll::Ready(Ok(result)) = Pin::new(&mut response).poll(&mut cx) else {
                panic!()
            };
            drop(response);
            assert_eq!(
                result
                    .get()
                    .unwrap()
                    .get_as::<capnp::text::Reader>()
                    .unwrap(),
                "retained"
            );
        }
    }
    #[test]
    fn error_is_immediate_and_abandonment_notifies_the_call_owner() {
        let (completion, sender, mut response) = pair();
        completion.complete(Err(Error::failed("method failed".into())));
        let Poll::Ready(Err(error)) =
            Pin::new(&mut response).poll(&mut Context::from_waker(Waker::noop()))
        else {
            panic!()
        };
        assert_eq!(error.extra, "method failed");
        assert!(completion.is_observed());
        sender.send(results("late results"));
        assert!(!has_value(&response.state.results));

        let (completion, sender, response) = pair();
        let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
        let waker = Waker::from(wakes.clone());
        assert!(completion
            .poll_observed(&mut Context::from_waker(&waker))
            .is_pending());
        drop(response);
        assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
        assert!(completion
            .poll_observed(&mut Context::from_waker(&waker))
            .is_ready());
        sender.send(results("canceled results"));
        assert!(!has_value(&completion.0.results));
    }
    #[test]
    fn observing_completed_response_does_not_wake_cancellation() {
        for status_first in [false, true] {
            let (completion, sender, mut response) = pair();
            let canceled = Arc::new(WakeCount(AtomicUsize::new(0)));
            let wake = Waker::from(canceled.clone());
            assert!(completion
                .poll_observed(&mut Context::from_waker(&wake))
                .is_pending());
            if status_first {
                completion.complete(Ok(()));
                sender.send(results("ready"));
            } else {
                sender.send(results("ready"));
                completion.complete(Ok(()));
            }
            assert!(Pin::new(&mut response)
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_ready());
            drop(response);
            assert_eq!(canceled.0.load(Ordering::SeqCst), 0);
            assert!(completion.is_observed());
        }
    }
    #[test]
    fn shutdown_follows_the_current_completion_waker_without_retaining_its_state() {
        let (completion, sender, mut response) = pair();
        let shutdown = response.shutdown.clone();
        let first = Arc::new(WakeCount(AtomicUsize::new(0)));
        let second = Arc::new(WakeCount(AtomicUsize::new(0)));
        let first_waker = Waker::from(first.clone());
        let second_waker = Waker::from(second.clone());
        assert!(Pin::new(&mut response)
            .poll(&mut Context::from_waker(&first_waker))
            .is_pending());
        assert!(Pin::new(&mut response)
            .poll(&mut Context::from_waker(&second_waker))
            .is_pending());
        assert_eq!(Rc::strong_count(&response.state), 3);
        assert_eq!(Rc::weak_count(&response.state), 1);
        shutdown.close(Error::disconnected(
            "closed before producer completion".into(),
        ));
        assert_eq!(first.0.load(Ordering::SeqCst), 0);
        assert_eq!(second.0.load(Ordering::SeqCst), 1);
        assert_eq!(Rc::weak_count(&response.state), 0);
        let Poll::Ready(Err(error)) =
            Pin::new(&mut response).poll(&mut Context::from_waker(&second_waker))
        else {
            panic!()
        };
        assert_eq!(error.extra, "closed before producer completion");
        drop(response);
        completion.complete(Ok(()));
        sender.send(results("late result"));
        assert!(!has_value(&completion.0.results));
        assert_eq!(second.0.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn dropped_completion_and_shutdown_both_finish_the_receiver() {
        let (completion, _sender, mut response) = pair();
        drop(completion);
        assert!(matches!(
            Pin::new(&mut response).poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Err(_))
        ));
        let (_completion, _sender, mut response) = pair();
        response
            .shutdown
            .close(Error::disconnected("closed".into()));
        let Poll::Ready(Err(error)) =
            Pin::new(&mut response).poll(&mut Context::from_waker(Waker::noop()))
        else {
            panic!()
        };
        assert_eq!(error.extra, "closed");
    }

    #[test]
    fn direct_payload_keeps_context_and_retained_guards_in_both_completion_orders() {
        for retained in [false, true] {
            for status_first in [false, true] {
                let (completion, sender, mut response) = pair();
                let context = Rc::new(());
                let weak = Rc::downgrade(&context);
                let mut results = crate::local::Results::direct(sender, Some(context));
                let guard = retained.then(|| results.cancellation_guard().unwrap());
                let mut message = capnp::message::Builder::new_default();
                message.set_root("direct payload").unwrap();
                let payload =
                    super::super::wire::Payload::copy(message.get_root_as_reader().unwrap())
                        .unwrap();
                if status_first {
                    completion.complete(Ok(()));
                }
                match results.finish_admitted_response(payload) {
                    Ok(()) => assert!(!retained),
                    Err(payload) => {
                        assert!(retained);
                        results.set_admitted_response(Rc::new(payload));
                    }
                }
                drop(results);
                if !status_first {
                    assert!(Pin::new(&mut response)
                        .poll(&mut Context::from_waker(Waker::noop()))
                        .is_pending());
                    completion.complete(Ok(()));
                }
                let Poll::Ready(Ok(result)) =
                    Pin::new(&mut response).poll(&mut Context::from_waker(Waker::noop()))
                else {
                    panic!()
                };
                drop((completion, response, message));
                assert_eq!(
                    result
                        .get()
                        .unwrap()
                        .get_as::<capnp::text::Reader>()
                        .unwrap(),
                    "direct payload"
                );
                assert!(weak.upgrade().is_some());
                drop(result);
                assert_eq!(weak.upgrade().is_some(), retained);
                drop(guard);
                assert!(weak.upgrade().is_none());
            }
        }
    }

    #[test]
    fn lazy_results_preserve_empty_completion_and_materialized_guards() {
        for materialize in [false, true] {
            let (completion, sender, mut response) = pair();
            let context = Rc::new(());
            let weak = Rc::downgrade(&context);
            let mut results = Results::direct(sender, Some(context));
            let guard = materialize.then(|| results.hook().cancellation_guard().unwrap());
            completion.complete(Ok(()));
            assert!(Pin::new(&mut response)
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending());
            drop(results);
            let Poll::Ready(Ok(result)) =
                Pin::new(&mut response).poll(&mut Context::from_waker(Waker::noop()))
            else {
                panic!()
            };
            assert!(result.get().unwrap().is_null());
            drop(result);
            assert_eq!(weak.upgrade().is_some(), materialize);
            drop(guard);
            assert!(weak.upgrade().is_none());
        }
    }

    #[test]
    fn abandoned_lazy_results_release_their_context() {
        let (completion, sender, response) = pair();
        let context = Rc::new(());
        let weak = Rc::downgrade(&context);
        let results = Results::direct(sender, Some(context));
        drop(response);
        drop(results);
        assert!(completion.is_observed());
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn materialized_direct_results_keep_context_and_first_completion() {
        let (completion, sender, mut response) = pair();
        let context = Rc::new(());
        let weak = Rc::downgrade(&context);
        let mut hook = Results::direct(sender, Some(context)).into_hook();
        hook.get().unwrap().set_as("materialized").unwrap();
        completion.complete(Ok(()));
        completion.complete(Err(Error::failed("late duplicate".into())));
        drop(hook);
        let result = Pin::new(&mut response)
            .poll(&mut Context::from_waker(Waker::noop()))
            .map(|result| result.unwrap());
        assert!(result.is_ready());
        assert!(weak.upgrade().is_some());
        drop(result);
        assert!(weak.upgrade().is_none());
        completion.complete(Err(Error::failed("already observed".into())));
        assert!(!has_value(&completion.0.status));
    }

    #[test]
    fn observation_wakes_a_cancellation_waiter_registered_after_completion() {
        let (completion, sender, mut response) = pair();
        sender.send(results("complete"));
        completion.complete(Ok(()));
        let count = Arc::new(WakeCount(AtomicUsize::new(0)));
        let wake = Waker::from(count.clone());
        assert!(completion
            .poll_observed(&mut Context::from_waker(&wake))
            .is_pending());
        assert!(Pin::new(&mut response)
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_ready());
        assert_eq!(count.0.load(Ordering::SeqCst), 1);
        assert!(completion
            .poll_observed(&mut Context::from_waker(&wake))
            .is_ready());
        drop(response);
        assert_eq!(count.0.load(Ordering::SeqCst), 1);
    }
    thread_local! {
        static ON_WAKER_DROP: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    }
    struct CompleteOnDrop;
    #[allow(clippy::manual_noop_waker)]
    impl std::task::Wake for CompleteOnDrop {
        fn wake(self: Arc<Self>) {}
    }
    impl Drop for CompleteOnDrop {
        fn drop(&mut self) {
            let action = ON_WAKER_DROP.with(|slot| slot.borrow_mut().take());
            if let Some(action) = action {
                action();
            }
        }
    }
    #[test]
    fn replacing_wakers_can_complete_or_cancel_without_losing_the_transition() {
        let (completion, sender, mut response) = pair();
        {
            let wake = Waker::from(Arc::new(CompleteOnDrop));
            assert!(Pin::new(&mut response)
                .poll(&mut Context::from_waker(&wake))
                .is_pending());
        }
        ON_WAKER_DROP.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || {
                completion.complete(Ok(()));
                sender.send(results("reentrant completion"));
            }))
        });
        let Poll::Ready(Ok(result)) =
            Pin::new(&mut response).poll(&mut Context::from_waker(Waker::noop()))
        else {
            panic!("completion during old waker destruction must be observed")
        };
        assert_eq!(
            result
                .get()
                .unwrap()
                .get_as::<capnp::text::Reader>()
                .unwrap(),
            "reentrant completion"
        );
        drop(response);

        let (completion, sender, response) = pair();
        {
            let wake = Waker::from(Arc::new(CompleteOnDrop));
            assert!(completion
                .poll_observed(&mut Context::from_waker(&wake))
                .is_pending());
        }
        ON_WAKER_DROP.with(|slot| *slot.borrow_mut() = Some(Box::new(move || drop(response))));
        assert!(completion
            .poll_observed(&mut Context::from_waker(Waker::noop()))
            .is_ready());
        sender.send(results("discarded after cancellation"));
        assert!(!has_value(&completion.0.results));
    }
}
