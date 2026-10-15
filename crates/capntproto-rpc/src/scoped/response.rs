//! One completion for both operation status and result ownership. A successful
//! operation can finish before its ResultsHook is dropped; both are required.
//! This is the hosted no-pipeline request adapter, not a second wire ledger.
use super::*;
use crate::local::ResultsDoneHook;

#[derive(Default)]
pub(super) struct State {
    status: Option<Result<(), Box<Error>>>,
    results: Option<ResultOwner>,
    observed: bool,
    ready: bool,
    receive: Option<Waker>,
    canceled: Option<Waker>,
}

pub(super) fn notify_closed(state: &Rc<RefCell<State>>) {
    let wake = state.borrow_mut().receive.take();
    if let Some(wake) = wake {
        wake.wake();
    }
}

/// One empty completion allocation per hook inbox. Only the last owner may
/// recycle it, so a canceled or retained operation can never observe reuse.
#[derive(Default)]
pub(super) struct Cache(RefCell<Option<Rc<RefCell<State>>>>);
impl Cache {
    fn acquire(&self) -> Rc<RefCell<State>> {
        let cached = self.0.borrow_mut().take();
        match cached {
            Some(mut state) => {
                let state_mut = Rc::get_mut(&mut state)
                    .expect("exclusive cached completion")
                    .get_mut();
                debug_assert!(
                    state_mut.status.is_none()
                        && state_mut.results.is_none()
                        && state_mut.receive.is_none()
                        && state_mut.canceled.is_none()
                );
                state_mut.observed = false;
                state_mut.ready = false;
                state
            }
            None => Rc::default(),
        }
    }
    fn recycle(&self, state: &Rc<RefCell<State>>) {
        if Rc::strong_count(state) == 1 {
            let mut cached = self.0.borrow_mut();
            if cached.is_none() {
                *cached = Some(state.clone());
            }
        }
    }
}
enum ResultOwner {
    Done(Box<dyn ResultsDoneHook>),
    Response(Box<dyn capnp::private::capability::ResponseHook>),
}
impl State {
    fn ready(&self) -> bool {
        self.ready
    }
}

pub(crate) struct Completion(Rc<RefCell<State>>);
pub(crate) struct ResultsSender(Rc<RefCell<State>>);

// Most remote no-pipeline calls only transfer an admitted payload. Build the
// general ResultsHook lazily, if a local producer or public accessor needs it.
pub(super) enum Results {
    Hook(Box<dyn ResultsHook>),
    Direct(DirectResults),
}
pub(super) struct DirectResults {
    sender: Option<ResultsSender>,
    context: Option<Rc<dyn std::any::Any>>,
}
impl DirectResults {
    fn into_hook(mut self) -> Box<dyn ResultsHook> {
        Box::new(crate::local::Results::direct(
            self.sender.take().expect("unconsumed direct results"),
            self.context.take(),
        ))
    }
}
impl Drop for DirectResults {
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            // An untouched hook still completes with empty results. This also
            // preserves completion-before-results and cancellation semantics.
            drop(crate::local::Results::direct(sender, self.context.take()));
        }
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
            let mut state = sender.0.borrow_mut();
            if state.observed {
                return Ok(());
            }
            debug_assert!(state.status.is_none() && state.results.is_none());
            state.results = Some(ResultOwner::Response(message));
            state.status = Some(Ok(()));
            state.ready = true;
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
        })
    }
    pub(super) fn into_hook(self) -> Box<dyn ResultsHook> {
        match self {
            Self::Hook(hook) => hook,
            Self::Direct(direct) => direct.into_hook(),
        }
    }
    pub(super) fn hook(&mut self) -> &mut dyn ResultsHook {
        if let Self::Direct(direct) = self {
            let hook = crate::local::Results::direct(
                direct.sender.take().expect("unconsumed direct results"),
                direct.context.take(),
            );
            *self = Self::Hook(Box::new(hook));
        }
        match self {
            Self::Hook(hook) => &mut **hook,
            Self::Direct(_) => unreachable!(),
        }
    }
    pub(super) fn retain_context(&mut self, context: Rc<dyn std::any::Any>) {
        match self {
            Self::Hook(hook) => hook.retain_response_context(context),
            Self::Direct(direct) => crate::admission::retain_context(&mut direct.context, context),
        }
    }
    // Return the intact payload to the fallback; boxing it adds an allocation.
    #[allow(clippy::result_large_err)]
    pub(super) fn finish_admitted(&mut self, payload: wire::Payload) -> Result<(), wire::Payload> {
        match self {
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
}
struct Response {
    state: Rc<RefCell<State>>,
    shutdown: Shutdown,
    inbox: Weak<Inbox>,
}

impl Completion {
    pub(super) fn is_observed(&self) -> bool {
        self.0.borrow().observed
    }
    pub(super) fn poll_observed(&self, cx: &mut Context<'_>) -> Poll<()> {
        {
            let state = self.0.borrow();
            if state.observed {
                return Poll::Ready(());
            }
            if state
                .canceled
                .as_ref()
                .is_some_and(|wake| wake.will_wake(cx.waker()))
            {
                return Poll::Pending;
            }
        }
        let wake = cx.waker().clone();
        let old = self.0.borrow_mut().canceled.replace(wake);
        drop(old);
        if self.0.borrow().observed {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
    pub(super) fn complete(&self, result: capnp::Result<()>) {
        let (wake, canceled) = {
            let mut state = self.0.borrow_mut();
            if state.status.is_some() || state.observed {
                return;
            }
            state.ready = result.is_err() || state.results.is_some();
            state.status = Some(result.map_err(Box::new));
            (
                if state.ready() {
                    state.receive.take()
                } else {
                    None
                },
                state.canceled.take(),
            )
        };
        // The operation owner is completing now. Observing its response later
        // must not wake a canceled-operation poll that no longer exists.
        drop(canceled);
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}
impl Drop for Completion {
    fn drop(&mut self) {
        if self.0.borrow().status.is_none() && !self.is_observed() {
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
        let wake = {
            let mut state = self.0.borrow_mut();
            if state.observed {
                return;
            }
            debug_assert!(state.results.is_none());
            state.results = Some(result);
            state.ready = state.status.is_some();
            if state.ready() {
                state.receive.take()
            } else {
                None
            }
        };
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}
#[cold]
#[inline(never)]
fn closed_reason(shutdown: &Shutdown) -> Error {
    shutdown.reason().expect("closed connection")
}
impl Future for Response {
    type Output = capnp::Result<capability::Response<any_pointer::Owned>>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.shutdown.is_closed() {
            return Poll::Ready(Err(closed_reason(&self.shutdown)));
        }
        if !self.state.borrow().ready() {
            let state = self.state.clone();
            self.shutdown.watch_response(&state);
            let registered = self
                .state
                .borrow()
                .receive
                .as_ref()
                .is_some_and(|wake| wake.will_wake(cx.waker()));
            if !registered {
                let wake = cx.waker().clone();
                let old = self.state.borrow_mut().receive.replace(wake);
                drop(old);
            }
            // A custom Waker clone/drop may close the connection while the
            // completion notification is being installed.
            if self.shutdown.is_closed() {
                return Poll::Ready(Err(closed_reason(&self.shutdown)));
            }
        }
        let (status, results, canceled) = {
            let mut state = self.state.borrow_mut();
            if !state.ready() {
                return Poll::Pending;
            }
            state.observed = true;
            state.ready = false;
            (
                state.status.take().unwrap(),
                state.results.take(),
                state.canceled.take(),
            )
        };
        if let Some(wake) = canceled {
            wake.wake();
        }
        Poll::Ready(status.map_err(|error| *error).map(|()| {
            capability::Response::new(match results.expect("successful operation owns results") {
                ResultOwner::Done(results) => Box::new(crate::local::Response::new(results)),
                ResultOwner::Response(response) => response,
            })
        }))
    }
}
impl Drop for Response {
    fn drop(&mut self) {
        let (status, results, receive, canceled, abandoned) = {
            let mut state = self.state.borrow_mut();
            let abandoned = !state.observed && state.status.is_none();
            state.observed = true;
            state.ready = false;
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
            if let Some(inbox) = self.inbox.upgrade() {
                inbox.notify();
            }
        }
        if let Some(inbox) = self.inbox.upgrade().filter(|inbox| !inbox.closed.get()) {
            inbox.completion_cache.recycle(&self.state);
        }
    }
}

pub(crate) fn available(client: &dyn ClientHook) -> bool {
    client
        .as_any()
        .and_then(|client| client.downcast_ref::<Hook>())
        .is_some_and(|hook| hook.introduced().is_none())
}
pub(crate) fn request(
    client: &dyn ClientHook,
    interface: u64,
    method: u16,
    hints: CallHints,
    params: Box<dyn ParamsHook>,
    context: Option<Rc<dyn std::any::Any>>,
) -> capability::RemotePromise<any_pointer::Owned> {
    let hook = client
        .as_any()
        .and_then(|client| client.downcast_ref::<Hook>())
        .expect("private scoped request hook");
    request_scoped(hook, interface, method, hints, params, context)
}

pub(crate) fn request_scoped(
    hook: &Hook,
    interface: u64,
    method: u16,
    hints: CallHints,
    params: Box<dyn ParamsHook>,
    context: Option<Rc<dyn std::any::Any>>,
) -> capability::RemotePromise<any_pointer::Owned> {
    let state = hook
        .0
        .inbox
        .upgrade()
        .map_or_else(Rc::default, |inbox| inbox.completion_cache.acquire());
    let results = Results::direct(ResultsSender(state.clone()), context);
    let completion = CallCompletion::Response(Completion(state.clone()));
    let admitted = hook.enqueue(interface, method, params, results, hints, None, completion);
    let promise = match admitted {
        Ok((shutdown, inbox)) => Promise::from_future(Response {
            state,
            shutdown,
            inbox,
        }),
        Err(error) => Promise::err(*error),
    };
    capability::RemotePromise {
        promise,
        pipeline: any_pointer::Pipeline::new(Box::new(crate::broken::DisabledPipeline)),
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
    fn completion_cache_reuses_only_exclusive_empty_state() {
        let cache = Cache::default();
        let state = cache.acquire();
        let address = Rc::as_ptr(&state);
        state.borrow_mut().observed = true;
        let pending_owner = state.clone();
        cache.recycle(&state);
        assert!(cache.0.borrow().is_none());
        drop(pending_owner);
        cache.recycle(&state);
        drop(state);
        let reused = cache.acquire();
        assert_eq!(Rc::as_ptr(&reused), address);
        assert!(!reused.borrow().observed);
        assert!(!reused.borrow().ready());
        let other = cache.acquire();
        assert!(!Rc::ptr_eq(&other, &reused));
        cache.recycle(&reused);
        cache.recycle(&other);
        drop(reused);
        let first = cache.acquire();
        assert_eq!(Rc::as_ptr(&first), address);
        assert!(!Rc::ptr_eq(&first, &other));
    }

    fn pair() -> (Completion, ResultsSender, Response) {
        let state = Rc::new(RefCell::new(State {
            status: None,
            results: None,
            observed: false,
            ready: false,
            receive: None,
            canceled: None,
        }));
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
        assert!(response.state.borrow().results.is_none());

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
        assert!(completion.0.borrow().results.is_none());
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
        assert!(completion.0.borrow().results.is_none());
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
        assert!(completion.0.borrow().status.is_none());
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
}
