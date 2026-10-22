//! Shared synchronous access to a caller-polled connection. Mutable state cannot
//! escape an access closure; the returned reply future borrows only its slot.
use super::*;
use capnp::{
    field_api::{FixedStruct, Schema},
    wire_capability::{Interface, Method, StreamingMethod},
};
use capntproto_rpc_engine::completion;
use core::{
    cell::{Cell, RefCell, RefMut},
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

type ReplyPair<'wait, 'pool, 'id, I, const CAPS: usize> = (
    completion::Sender<'wait, CallResult<'pool, 'id, I, CAPS>>,
    completion::Receiver<'wait, CallResult<'pool, 'id, I, CAPS>>,
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallError {
    Busy,
    Connection(Error),
    Completion(completion::Error),
    Failed(capnp::ErrorKind),
    Output(super::super::Error),
    Redirected,
    Capability(crate::reception::Error),
    Bind(binding::BindError),
}
impl core::fmt::Display for CallError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "RPC call: {self:?}")
    }
}
impl core::error::Error for CallError {}

/// Fixed storage stays with its caller. A session owns the driver/bindings and
/// borrows the pool; it creates no executor, transport, clock or allocation.
/// `access` is the synchronous boundary for pumping IO and dispatch. Recursive
/// access from a waker returns Busy instead of panicking or blocking.
pub struct Session<D, M, Q> {
    state: RefCell<(D, M, Q)>,
    driving: Cell<bool>,
    wake: RefCell<Option<Waker>>,
    minimum: usize,
    options: capnp::message::ReaderOptions,
}
/// A busy session has not invoked or dropped the operation. Keep this closure
/// for retry, including any running calls or completed replies it owns.
#[must_use = "retry the operation; its captured RPC owners are still live"]
pub struct AccessRejected<F> {
    pub error: CallError,
    pub operation: F,
}
impl<F> core::fmt::Debug for AccessRejected<F> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AccessRejected")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}
impl<D, M, Q> Session<D, M, Q> {
    pub fn new(
        driver: D,
        messages: M,
        invocations: Q,
        minimum: usize,
        options: capnp::message::ReaderOptions,
    ) -> Self {
        Self {
            state: RefCell::new((driver, messages, invocations)),
            driving: Cell::new(false),
            wake: RefCell::new(None),
            minimum,
            options,
        }
    }
    /// Mutable state cannot be retained by a service future.
    ///
    /// ```compile_fail
    /// use capntproto_rpc_wire::owned::connection::Session;
    /// let session = Session::new(1u32, (), (), 16, Default::default());
    /// let suspended = session.access(|driver, _, _| async move { *driver += 1 });
    /// drop(suspended);
    /// ```
    pub fn access<R, F: FnOnce(&mut D, &mut M, &mut Q) -> R>(
        &self,
        f: F,
    ) -> Result<R, AccessRejected<F>> {
        let Ok(mut state) = self.borrow_mut() else {
            return Err(AccessRejected {
                error: CallError::Busy,
                operation: f,
            });
        };
        let (driver, messages, calls) = &mut *state;
        Ok(f(driver, messages, calls))
    }
    /// Recover the original owners for explicit bounded termination. Consuming
    /// the session prevents further calls through this shared access boundary.
    pub fn into_parts(self) -> (D, M, Q) {
        self.state.into_inner()
    }

    /// Claim the sole caller-polled driver registration. The handle borrows the
    /// session, so teardown cannot race a retained driver. Dropping the handle
    /// unregisters its waker and permits a replacement driver.
    ///
    /// ```compile_fail
    /// use capntproto_rpc_wire::owned::connection::Session;
    /// let session = Session::new(0u32, (), (), 16, Default::default());
    /// let driver = session.drive().unwrap();
    /// let parts = session.into_parts();
    /// drop(driver);
    /// ```
    pub fn drive(&self) -> Result<Drive<'_, D, M, Q>, CallError> {
        if self.driving.replace(true) {
            return Err(CallError::Busy);
        }
        Ok(Drive { session: self })
    }

    fn notify(&self) {
        let wake = self.wake.borrow_mut().take();
        if let Some(wake) = wake {
            wake.wake();
        }
    }

    fn borrow_mut(&self) -> Result<Mutation<'_, D, M, Q>, CallError> {
        Ok(Mutation {
            state: Some(self.state.try_borrow_mut().map_err(|_| CallError::Busy)?),
            session: self,
        })
    }
}

// The borrow ends before any waker callback, including early errors and unwind.
// A callback may immediately reenter access(), enqueue, or poll the driver.
struct Mutation<'a, D, M, Q> {
    state: Option<RefMut<'a, (D, M, Q)>>,
    session: &'a Session<D, M, Q>,
}
impl<D, M, Q> core::ops::Deref for Mutation<'_, D, M, Q> {
    type Target = (D, M, Q);
    fn deref(&self) -> &Self::Target {
        self.state.as_deref().expect("live mutation")
    }
}
impl<D, M, Q> core::ops::DerefMut for Mutation<'_, D, M, Q> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.state.as_deref_mut().expect("live mutation")
    }
}
impl<D, M, Q> Drop for Mutation<'_, D, M, Q> {
    fn drop(&mut self) {
        drop(self.state.take());
        self.session.notify();
    }
}

/// One executor-independent driver registration. Application mutation wakes a
/// parked driver; polling itself does not wake an otherwise idle connection.
/// Poll IO, dispatch and completion bindings in `poll_with`, registering their
/// notifications with the supplied context. No mutable session borrow survives
/// the poll. Resource owners can be returned for service outside this boundary.
#[must_use = "poll the driver or drop its wake registration"]
pub struct Drive<'session, D, M, Q> {
    session: &'session Session<D, M, Q>,
}
impl<D, M, Q> Drive<'_, D, M, Q> {
    /// Pending keeps the latest waker; Ready unregisters it until the next poll.
    /// Busy leaves the operation intact, including any captured resource owners.
    /// A driver called reentrantly from inside access must retry after that access
    /// ends. The existing registration is deliberately preserved in that case.
    ///
    /// ```
    /// use core::task::{Context, Poll, Waker};
    /// use capntproto_rpc_wire::owned::connection::Session;
    /// let session = Session::new(0u32, (), (), 16, Default::default());
    /// let mut drive = session.drive().unwrap();
    /// let mut cx = Context::from_waker(Waker::noop());
    /// // In a real driver, poll IO and completion bindings with this context.
    /// assert!(drive.poll_with(&mut cx, |_, _, _, _| Poll::<u32>::Pending)
    ///     .unwrap().is_pending());
    /// session.access(|state, _, _| *state = 7).unwrap();
    /// assert_eq!(drive.poll_with(&mut cx, |state, _, _, _| Poll::Ready(*state))
    ///     .unwrap(), Poll::Ready(7));
    /// ```
    pub fn poll_with<R, F>(
        &mut self,
        cx: &mut Context<'_>,
        operation: F,
    ) -> Result<Poll<R>, AccessRejected<F>>
    where
        F: FnOnce(&mut D, &mut M, &mut Q, &mut Context<'_>) -> Poll<R>,
    {
        // Clone before borrowing: clone/drop/wake are all caller code.
        let wake = cx.waker().clone();
        let Ok(mut state) = self.session.state.try_borrow_mut() else {
            return Err(AccessRejected {
                error: CallError::Busy,
                operation,
            });
        };
        let (driver, messages, calls) = &mut *state;
        let result = operation(driver, messages, calls, cx);
        // Install the registration while state is still borrowed. No session
        // mutation can slip between observing Pending and registering its wake.
        let (next, unused) = if result.is_pending() {
            (Some(wake), None)
        } else {
            (None, Some(wake))
        };
        let old = self.session.wake.replace(next);
        drop(state);
        drop(old);
        drop(unused);
        Ok(result)
    }
}
impl<D, M, Q> Drop for Drive<'_, D, M, Q> {
    fn drop(&mut self) {
        let old = self.session.wake.borrow_mut().take();
        self.session.driving.set(false);
        drop(old);
    }
}

/// A schema-typed future over a caller-owned reply slot. Cancellation is observed
/// by the invocation registry, including abandoned accepted capability results.
#[must_use = "poll the reply or drop it to request cancellation"]
pub struct PendingReply<
    'wait,
    'pool,
    'id,
    I,
    IF: Interface,
    P: Schema,
    R: Schema,
    const CAPS: usize,
> {
    receiver: completion::Receiver<'wait, CallResult<'pool, 'id, I, CAPS>>,
    task: InvocationKey<'id>,
    method: Method<IF, P, R>,
}
// Neither the receiver (a shared slot reference) nor the method metadata has any
// address-sensitive state; schema parameters are only phantom type identities.
impl<I, IF: Interface, P: Schema, R: Schema, const CAPS: usize> Unpin
    for PendingReply<'_, '_, '_, I, IF, P, R, CAPS>
{
}
impl<'id, I, IF: Interface, P: Schema, R: Schema, const CAPS: usize>
    PendingReply<'_, '_, 'id, I, IF, P, R, CAPS>
{
    pub fn task(&self) -> InvocationKey<'id> {
        self.task
    }
}
impl<'pool, 'id, I, IF: Interface, P: Schema, R: Schema, const CAPS: usize> Future
    for PendingReply<'_, 'pool, 'id, I, IF, P, R, CAPS>
{
    type Output = Result<TypedReply<'pool, 'id, I, IF, P, R, CAPS>, CallError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        Pin::new(&mut this.receiver).poll(cx).map(|result| {
            result
                .map_err(CallError::Completion)?
                .map(|output| TypedReply {
                    output,
                    method: this.method,
                })
                .map_err(CallError::Failed)
        })
    }
}

/// The generated method used for sending determines the only available result
/// schema. Keep the owner until all borrowed readers are gone, then release it
/// through the session. Raw remote exception details remain available via output.
#[must_use = "release this reply through its originating session or driver"]
pub struct TypedReply<'pool, 'id, I, IF: Interface, P: Schema, R: Schema, const CAPS: usize> {
    output: CallOutput<'pool, 'id, I, CAPS>,
    method: Method<IF, P, R>,
}
/// A generated reader with an explicit borrow lifetime for capability selectors.
/// Dereferencing exposes the same borrowed fields as the generated reader.
pub struct ResultReader<'a, R: Schema> {
    reader: R::Ref<'a>,
    lifetime: core::marker::PhantomData<&'a ()>,
}
impl<'a, R: Schema> core::ops::Deref for ResultReader<'a, R> {
    type Target = R::Ref<'a>;
    fn deref(&self) -> &Self::Target {
        &self.reader
    }
}
impl<'a, R: Schema> ResultReader<'a, R> {
    fn new(reader: R::Ref<'a>) -> Self {
        Self {
            reader,
            lifetime: core::marker::PhantomData,
        }
    }
}
impl<'pool, 'id, I, IF: Interface, P: Schema, R: Schema, const CAPS: usize>
    TypedReply<'pool, 'id, I, IF, P, R, CAPS>
{
    pub fn output(&self) -> &CallOutput<'pool, 'id, I, CAPS> {
        &self.output
    }
    pub fn into_output(self) -> CallOutput<'pool, 'id, I, CAPS> {
        self.output
    }
    pub fn read<T>(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
        f: impl FnOnce(R::Ref<'_>) -> T,
    ) -> Result<T, CallError> {
        match &self.output {
            CallOutput::Local(result) => result
                .with(messages, |payload| {
                    payload
                        .results(self.method)
                        .map(f)
                        .map_err(|e| CallError::Failed(e.kind))
                })
                .map_err(CallError::Output)?,
            CallOutput::Remote(response) => response
                .with(messages, |reply| match reply {
                    super::super::Reply::Results(payload) => self
                        .method
                        .read_results(payload.content())
                        .map(f)
                        .map_err(|e| CallError::Failed(e.kind)),
                    super::super::Reply::Exception(exception) => {
                        Err(CallError::Failed(exception_kind(exception)))
                    }
                    _ => Err(CallError::Redirected),
                })
                .map_err(CallError::Output)?,
        }
    }
}
#[must_use = "retry release; the rejected value still owns its RPC resources"]
pub struct ReleaseRejected<T> {
    pub error: CallError,
    pub value: T,
}

/// Streaming completion has no result readers or generated pipeline surface.
#[must_use = "poll the acknowledgment or drop it to request cancellation"]
pub struct PendingAck<'wait, 'pool, 'id, I, const CAPS: usize> {
    receiver: completion::Receiver<'wait, CallResult<'pool, 'id, I, CAPS>>,
    task: InvocationKey<'id>,
}
impl<'id, I, const CAPS: usize> PendingAck<'_, '_, 'id, I, CAPS> {
    pub fn task(&self) -> InvocationKey<'id> {
        self.task
    }
}
impl<'pool, 'id, I, const CAPS: usize> Future for PendingAck<'_, 'pool, 'id, I, CAPS> {
    type Output = Result<StreamingAck<'pool, 'id, I, CAPS>, CallError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.get_mut().receiver)
            .poll(cx)
            .map(|result| {
                result
                    .map_err(CallError::Completion)?
                    .map(|output| StreamingAck { output })
                    .map_err(CallError::Failed)
            })
    }
}
#[must_use = "check and release the streaming acknowledgment through its session"]
pub struct StreamingAck<'pool, 'id, I, const CAPS: usize> {
    output: CallOutput<'pool, 'id, I, CAPS>,
}
impl<'pool, 'id, I, const CAPS: usize> StreamingAck<'pool, 'id, I, CAPS> {
    pub fn output(&self) -> &CallOutput<'pool, 'id, I, CAPS> {
        &self.output
    }
    pub fn into_output(self) -> CallOutput<'pool, 'id, I, CAPS> {
        self.output
    }
    pub fn check(&self, messages: &messages::Messages<'pool, '_, '_>) -> Result<(), CallError> {
        match &self.output {
            CallOutput::Local(_) => Ok(()),
            CallOutput::Remote(response) => response
                .with(messages, |reply| match reply {
                    super::super::Reply::Results(_) => Ok(()),
                    super::super::Reply::Exception(exception) => {
                        Err(CallError::Failed(exception_kind(exception)))
                    }
                    _ => Err(CallError::Redirected),
                })
                .map_err(CallError::Output)?,
        }
    }
}
fn exception_kind(exception: rpc_capnp::exception::Reader<'_>) -> capnp::ErrorKind {
    match exception.get_type() {
        Ok(rpc_capnp::exception::Type::Overloaded) => capnp::ErrorKind::Overloaded,
        Ok(rpc_capnp::exception::Type::Disconnected) => capnp::ErrorKind::Disconnected,
        Ok(rpc_capnp::exception::Type::Unimplemented) => capnp::ErrorKind::Unimplemented,
        _ => capnp::ErrorKind::Failed,
    }
}

impl<
        'pool,
        'id,
        's,
        'm,
        'buffers,
        'bytes,
        'q,
        'wait,
        I,
        A,
        P: Copy + Eq,
        S,
        const CAPS: usize,
        const PATH: usize,
    >
    Session<
        Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>,
        &'m mut messages::Messages<'pool, 'buffers, 'bytes>,
        Invocations<'q, 'wait, 'pool, 'id, I, CAPS>,
    >
{
    /// Reserve the reply binding and enqueue one generated call. On any
    /// rejection, both fresh slot endpoints are canceled and no owner escapes.
    /// In particular, a busy context or slot never invokes the parameter filler.
    pub fn call<IF: Interface, Params: Schema, Results: Schema>(
        &self,
        slot: &'wait completion::Slot<CallResult<'pool, 'id, I, CAPS>>,
        target: &impl binding::CallTarget<'id, IF>,
        method: Method<IF, Params, Results>,
        capabilities: &[&clients::Client<'id>],
        fill: impl FnOnce(FixedStruct<'_, Params>) -> capnp::Result<()>,
    ) -> Result<PendingReply<'wait, 'pool, 'id, I, IF, Params, Results, CAPS>, CallError> {
        self.call_with_reply(|| slot.try_pair(), target, method, capabilities, fill)
    }
    /// Allocate one reply slot and use the same tracked invocation path as
    /// `call`. Dropping the future cancels interest; the polled driver recovers
    /// any unclaimed message and capability receipts. No caller slot is needed.
    /// The reply and its resources remain bounded by this connection's scope.
    #[cfg(feature = "alloc")]
    pub fn call_owned<IF: Interface, Params: Schema, Results: Schema>(
        &self,
        target: &impl binding::CallTarget<'id, IF>,
        method: Method<IF, Params, Results>,
        capabilities: &[&clients::Client<'id>],
        fill: impl FnOnce(FixedStruct<'_, Params>) -> capnp::Result<()>,
    ) -> Result<PendingReply<'wait, 'pool, 'id, I, IF, Params, Results, CAPS>, CallError> {
        self.call_with_reply(
            || {
                let (sender, receiver) = completion::shared::channel();
                Ok((sender.into(), receiver.into()))
            },
            target,
            method,
            capabilities,
            fill,
        )
    }
    fn call_with_reply<IF: Interface, Params: Schema, Results: Schema>(
        &self,
        pair: impl FnOnce() -> Result<ReplyPair<'wait, 'pool, 'id, I, CAPS>, completion::Error>,
        target: &impl binding::CallTarget<'id, IF>,
        method: Method<IF, Params, Results>,
        capabilities: &[&clients::Client<'id>],
        fill: impl FnOnce(FixedStruct<'_, Params>) -> capnp::Result<()>,
    ) -> Result<PendingReply<'wait, 'pool, 'id, I, IF, Params, Results, CAPS>, CallError> {
        let mut state = self.borrow_mut()?;
        let (driver, messages, calls) = &mut *state;
        let (sender, receiver) = pair().map_err(CallError::Completion)?;
        let task = match calls.enqueue(
            driver,
            sender,
            target.as_call_target(),
            method,
            capabilities,
            messages,
            self.minimum,
            self.options,
            fill,
        ) {
            Ok(task) => task,
            Err(rejected) => return Err(CallError::Connection(rejected.error)),
        };
        Ok(PendingReply {
            receiver,
            task,
            method,
        })
    }
    pub fn read<IF: Interface, Params: Schema, Results: Schema, T>(
        &self,
        reply: &TypedReply<'pool, 'id, I, IF, Params, Results, CAPS>,
        f: impl FnOnce(Results::Ref<'_>) -> T,
    ) -> Result<T, CallError> {
        let state = self.state.try_borrow().map_err(|_| CallError::Busy)?;
        reply.read(state.1, f)
    }
    pub fn streaming_call<IF: Interface, Params: Schema>(
        &self,
        slot: &'wait completion::Slot<CallResult<'pool, 'id, I, CAPS>>,
        target: &impl binding::CallTarget<'id, IF>,
        method: StreamingMethod<IF, Params>,
        capabilities: &[&clients::Client<'id>],
        fill: impl FnOnce(FixedStruct<'_, Params>) -> capnp::Result<()>,
    ) -> Result<PendingAck<'wait, 'pool, 'id, I, CAPS>, CallError> {
        self.streaming_call_with_reply(|| slot.try_pair(), target, method, capabilities, fill)
    }
    /// Streaming acknowledgment backed by an owned reply slot, with the same
    /// cancellation and explicit receipt cleanup as caller-backed streaming.
    #[cfg(feature = "alloc")]
    pub fn streaming_call_owned<IF: Interface, Params: Schema>(
        &self,
        target: &impl binding::CallTarget<'id, IF>,
        method: StreamingMethod<IF, Params>,
        capabilities: &[&clients::Client<'id>],
        fill: impl FnOnce(FixedStruct<'_, Params>) -> capnp::Result<()>,
    ) -> Result<PendingAck<'wait, 'pool, 'id, I, CAPS>, CallError> {
        self.streaming_call_with_reply(
            || {
                let (sender, receiver) = completion::shared::channel();
                Ok((sender.into(), receiver.into()))
            },
            target,
            method,
            capabilities,
            fill,
        )
    }
    fn streaming_call_with_reply<IF: Interface, Params: Schema>(
        &self,
        pair: impl FnOnce() -> Result<ReplyPair<'wait, 'pool, 'id, I, CAPS>, completion::Error>,
        target: &impl binding::CallTarget<'id, IF>,
        method: StreamingMethod<IF, Params>,
        capabilities: &[&clients::Client<'id>],
        fill: impl FnOnce(FixedStruct<'_, Params>) -> capnp::Result<()>,
    ) -> Result<PendingAck<'wait, 'pool, 'id, I, CAPS>, CallError> {
        let mut state = self.borrow_mut()?;
        let (driver, messages, calls) = &mut *state;
        let (sender, receiver) = pair().map_err(CallError::Completion)?;
        let task = match calls.enqueue_streaming(
            driver,
            sender,
            target.as_call_target(),
            method,
            capabilities,
            messages,
            self.minimum,
            self.options,
            fill,
        ) {
            Ok(task) => task,
            Err(rejected) => return Err(CallError::Connection(rejected.error)),
        };
        Ok(PendingAck { receiver, task })
    }
    pub fn check_ack(&self, ack: &StreamingAck<'pool, 'id, I, CAPS>) -> Result<(), CallError> {
        let state = self.state.try_borrow().map_err(|_| CallError::Busy)?;
        ack.check(state.1)
    }
    /// Retain a capability selected from these exact generated results. Remote
    /// receipts are bound with the same reflected/promise policy as the driver;
    /// the returned client remains live after releasing the result frame.
    pub fn capability<IF: Interface, Params: Schema, Results: Schema, Target>(
        &self,
        reply: &TypedReply<'pool, 'id, I, IF, Params, Results, CAPS>,
        select: impl for<'view> FnOnce(
            ResultReader<'view, Results>,
        )
            -> capnp::Result<capnp::wire_capability::Reader<'view, Target>>,
    ) -> Result<binding::TypedClient<'id, Target>, CallError> {
        let mut state = self.borrow_mut()?;
        let (driver, messages, _) = &mut *state;
        let client = match &reply.output {
            CallOutput::Local(result) => result
                .with(messages, |payload| {
                    let value = payload
                        .results(reply.method)
                        .map(ResultReader::new)
                        .and_then(select)
                        .map_err(|e| CallError::Failed(e.kind))?;
                    let client = payload.capability(value).map_err(CallError::Output)?;
                    driver
                        .runtime
                        .retain(client)
                        .map_err(|e| CallError::Connection(Error::Graph(e)))
                })
                .map_err(CallError::Output)??,
            CallOutput::Remote(response) => response
                .with(messages, |reply_value| {
                    let payload = match reply_value {
                        super::super::Reply::Results(payload) => payload,
                        super::super::Reply::Exception(exception) => {
                            return Err(CallError::Failed(exception_kind(exception)))
                        }
                        _ => return Err(CallError::Redirected),
                    };
                    let value = reply
                        .method
                        .read_results(payload.content())
                        .map(ResultReader::new)
                        .and_then(select)
                        .map_err(|e| CallError::Failed(e.kind))?;
                    let capability = payload.capability(value).map_err(CallError::Capability)?;
                    driver
                        .imports
                        .bind_capability(
                            capability,
                            &mut driver.runtime,
                            &mut driver.ledgers.imports,
                            &driver.ledgers.exports,
                            &mut driver.ledgers.answers,
                            &mut driver.pipelines,
                        )
                        .map(binding::Bound::into_client)
                        .map_err(CallError::Bind)
                })
                .map_err(CallError::Output)??,
        };
        Ok(binding::TypedClient::from_client(client))
    }
    /// The path's root schema must match this reply's generated result schema.
    /// Call before dispatch; a bounded observer retains the original ordering.
    pub fn pipeline<IF: Interface, Params: Schema, Results: Schema, Target>(
        &self,
        reply: &PendingReply<'wait, 'pool, 'id, I, IF, Params, Results, CAPS>,
        path: capnp::wire_capability::CapabilityPath<'_, Results, Target>,
    ) -> Result<binding::TypedClient<'id, Target>, CallError> {
        let mut state = self.borrow_mut()?;
        let (driver, messages, _) = &mut *state;
        driver
            .invocation_pipeline(reply.task, pipeline::Path::new(path.fields()), messages)
            .map(binding::TypedClient::from_client)
            .map_err(CallError::Connection)
    }
    pub fn retain_client<T>(
        &self,
        client: &binding::TypedClient<'id, T>,
    ) -> Result<binding::TypedClient<'id, T>, CallError> {
        let mut state = self.borrow_mut()?;
        state
            .0
            .runtime
            .retain(client.as_client())
            .map(binding::TypedClient::from_client)
            .map_err(|e| CallError::Connection(Error::Graph(e)))
    }
    pub fn release_client<T>(
        &self,
        client: binding::TypedClient<'id, T>,
    ) -> Result<(), ReleaseRejected<binding::TypedClient<'id, T>>> {
        let Ok(mut state) = self.borrow_mut() else {
            return Err(ReleaseRejected {
                error: CallError::Busy,
                value: client,
            });
        };
        state.0.runtime.release(client.into_client());
        Ok(())
    }
    #[allow(clippy::result_large_err)]
    pub fn release_ack(
        &self,
        ack: StreamingAck<'pool, 'id, I, CAPS>,
    ) -> Result<(), ReleaseRejected<StreamingAck<'pool, 'id, I, CAPS>>> {
        self.release_owned(ack, StreamingAck::into_output)
    }
    #[allow(clippy::result_large_err, clippy::type_complexity)]
    pub fn release<IF: Interface, Params: Schema, Results: Schema>(
        &self,
        reply: TypedReply<'pool, 'id, I, IF, Params, Results, CAPS>,
    ) -> Result<(), ReleaseRejected<TypedReply<'pool, 'id, I, IF, Params, Results, CAPS>>> {
        self.release_owned(reply, TypedReply::into_output)
    }
    fn release_owned<T>(
        &self,
        value: T,
        into_output: impl FnOnce(T) -> CallOutput<'pool, 'id, I, CAPS>,
    ) -> Result<(), ReleaseRejected<T>> {
        let Ok(mut state) = self.borrow_mut() else {
            return Err(ReleaseRejected {
                error: CallError::Busy,
                value,
            });
        };
        let (driver, messages, _) = &mut *state;
        driver.release_output(into_output(value), messages);
        Ok(())
    }
}

mod streaming;
pub use streaming::{Streaming, StreamingSlot};
#[cfg(test)]
mod tests;
