//! Completion bindings established before an invocation has a wire question.
//! All message buffers, completion slots and binding storage remain caller-owned.
use super::*;
use capnp::{
    field_api::{FixedStruct, Schema},
    wire_capability::{Interface, Method, StreamingMethod},
};
use capntproto_rpc_engine::clients::Capabilities;
use capntproto_rpc_engine::completion::{Delivery, Sender};
use core::task::Context;

// Completed responses retain affine capability and adoption owners inline.
// Boxing this variant would require an allocator in the fixed-storage profile.
#[allow(clippy::large_enum_variant)]
enum Stage<'pool, 'id, I, const CAPS: usize> {
    Queued,
    Sent(questions::Key<'id>),
    Running,
    Completed(CallResult<'pool, 'id, I, CAPS>),
    Delivered,
}
struct Entry<'wait, 'pool, 'id, I, const CAPS: usize> {
    task: dispatch::TaskKey<'id>,
    delivery: Delivery<'wait, CallResult<'pool, 'id, I, CAPS>>,
    stage: Stage<'pool, 'id, I, CAPS>,
    pipeline: usize,
    pipeline_remaining: usize,
    allows_pipeline: bool,
}
pub struct InvocationSlot<'wait, 'pool, 'id, I, const CAPS: usize> {
    entry: Option<Entry<'wait, 'pool, 'id, I, CAPS>>,
}
impl<I, const CAPS: usize> InvocationSlot<'_, '_, '_, I, CAPS> {
    pub const EMPTY: Self = Self { entry: None };
}
#[must_use = "retry enqueue or cancel the rejected sender"]
pub struct RejectedInvocation<'wait, 'pool, 'id, I, const CAPS: usize> {
    pub error: Error,
    pub sender: Sender<'wait, CallResult<'pool, 'id, I, CAPS>>,
}
/// Return this running operation before terminating its driver. The opaque task
/// identity ties completion to its original binding, even after other slot reuse.
#[must_use = "finish or explicitly release this local invocation"]
pub struct LocalCall<'pool, 'id, I, const CAPS: usize, const PATH: usize> {
    task: dispatch::TaskKey<'id>,
    running: dispatch::Running<'id, Task<'pool, 'id, I, CAPS, PATH>>,
}
impl<'pool, 'id, I, const CAPS: usize, const PATH: usize> LocalCall<'pool, 'id, I, CAPS, PATH> {
    pub fn task(&self) -> dispatch::TaskKey<'id> {
        self.task
    }
    /// The destination frozen by acceptance, borrowed without another retain.
    pub fn target(&self) -> &clients::Client<'id> {
        self.running.target()
    }
    pub fn params<R>(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
        f: impl FnOnce(InvocationParams<'_, '_, 'id, CAPS>) -> R,
    ) -> Result<R, super::super::Error> {
        let Task::Invoke(invocation) = self.running.owner() else {
            unreachable!()
        };
        invocation.with(messages, f)
    }
    pub fn release<A, P: Copy + Eq, S>(
        self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) {
        let Task::Invoke(invocation) = driver.runtime.complete(self.running) else {
            unreachable!()
        };
        invocation.release(messages, &mut driver.runtime);
    }
    pub fn params_async<'call, 'storage: 'call, R, F>(
        &'call self,
        messages: &messages::Messages<'pool, 'storage, '_>,
        f: F,
    ) -> impl core::future::Future<Output = Result<R, super::super::Error>>
           + 'call
           + use<'call, 'pool, 'id, I, R, F, CAPS, PATH>
    where
        F: for<'m> core::ops::AsyncFnOnce(InvocationParams<'m, 'call, 'id, CAPS>) -> R + 'call,
    {
        let Task::Invoke(invocation) = self.running.owner() else {
            unreachable!()
        };
        invocation.with_async(messages, f)
    }
}
#[must_use = "retry completion or release its call and result owners"]
pub struct RejectedLocal<'pool, 'id, I, const CAPS: usize, const PATH: usize> {
    pub error: Error,
    pub call: LocalCall<'pool, 'id, I, CAPS, PATH>,
    pub result: Result<LocalResult<'pool, 'id, CAPS>, capnp::ErrorKind>,
}
/// This is the normal completion path for enqueued generated calls. Submit
/// output through `send_one`, accept local work through `accept_local`, and poll
/// bindings alongside the driver. Lower-level dispatcher methods remain for
/// incoming calls and protocol controls, not registered invocations.
#[must_use = "drain or terminate invocation bindings before terminating the driver"]
pub struct Invocations<'s, 'wait, 'pool, 'id, I, const CAPS: usize> {
    slots: SlotStorage<'s, InvocationSlot<'wait, 'pool, 'id, I, CAPS>>,
}
impl<'s, 'wait, 'pool, 'id, I, const CAPS: usize> Invocations<'s, 'wait, 'pool, 'id, I, CAPS> {
    pub fn new(slots: &'s mut [InvocationSlot<'wait, 'pool, 'id, I, CAPS>]) -> Result<Self, Error> {
        if slots.iter().any(|slot| slot.entry.is_some()) {
            return Err(Error::NonemptyStorage);
        }
        Ok(Self {
            slots: SlotStorage::borrowed(slots),
        })
    }
    /// Grow completion bindings independently of dispatch. Reply slots and the
    /// futures borrowing them still belong to the caller and never relocate.
    #[cfg(feature = "alloc")]
    pub fn new_growing(initial: usize, maximum: usize) -> Result<Self, Error> {
        Ok(Self {
            slots: SlotStorage::growing(initial, maximum, || InvocationSlot::EMPTY)
                .ok_or(Error::Input(super::super::Error::Capacity))?,
        })
    }
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|slot| slot.entry.is_none())
    }
    /// Whether another completion binding fits the configured bound. A
    /// delivered reply retains its slot until consumption/cancellation is
    /// observed; hosted request capacity alone is not sufficient for admission.
    /// Growing storage may still reject allocation at the actual reservation.
    pub fn has_capacity(&self) -> bool {
        self.slots.len() < self.slots.maximum()
            || self.slots.iter().any(|slot| slot.entry.is_none())
    }
    pub fn has_queued(&self) -> bool {
        self.slots
            .iter()
            .filter_map(|slot| slot.entry.as_ref())
            .any(|entry| matches!(entry.stage, Stage::Queued))
    }
    /// Match a live invocation to its own sent question generation.
    pub fn question_for(&self, task: dispatch::TaskKey<'id>) -> Option<questions::Key<'id>> {
        self.slots
            .iter()
            .filter_map(|slot| slot.entry.as_ref())
            .find_map(|entry| {
                if entry.task != task {
                    return None;
                }
                match entry.stage {
                    Stage::Sent(key) => Some(key),
                    _ => None,
                }
            })
    }
    /// Bind a hosted dynamic call before driver admission. The completion
    /// owner is returned unchanged on rejection, including resource pressure.
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn enqueue_dynamic<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        sender: Sender<'wait, CallResult<'pool, 'id, I, CAPS>>,
        target: &clients::Client<'id>,
        method: calls::DynamicCall,
        capabilities: &[Option<&clients::Client<'id>>],
        messages: &mut messages::Messages<'pool, '_, '_>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        fill: impl FnOnce(
            capnp::field_api::FixedField<'_, capnp::field_api::Generic<capnp::any_pointer::Owned>>,
        ) -> capnp::Result<()>,
    ) -> Result<dispatch::TaskKey<'id>, RejectedInvocation<'wait, 'pool, 'id, I, CAPS>> {
        self.enqueue_dynamic_to(
            driver,
            sender,
            target,
            method,
            capabilities,
            messages,
            minimum,
            options,
            None,
            fill,
        )
    }
    /// Track a dynamic call whose result is delivered through an authenticated
    /// third-party rendezvous. Completion still owns the original wire question.
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn enqueue_dynamic_to<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        sender: Sender<'wait, CallResult<'pool, 'id, I, CAPS>>,
        target: &clients::Client<'id>,
        method: calls::DynamicCall,
        capabilities: &[Option<&clients::Client<'id>>],
        messages: &mut messages::Messages<'pool, '_, '_>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        destination: Option<capnp::any_pointer::Reader<'_>>,
        fill: impl FnOnce(
            capnp::field_api::FixedField<'_, capnp::field_api::Generic<capnp::any_pointer::Owned>>,
        ) -> capnp::Result<()>,
    ) -> Result<dispatch::TaskKey<'id>, RejectedInvocation<'wait, 'pool, 'id, I, CAPS>> {
        self.enqueue_with(sender, !method.no_promise_pipelining, || {
            driver.enqueue_dynamic_to(
                target,
                method,
                capabilities,
                messages,
                minimum,
                options,
                destination,
                fill,
            )
        })
    }
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn enqueue<
        A,
        P: Copy + Eq,
        S,
        const PATH: usize,
        InterfaceType: Interface,
        Params: Schema,
        Results: Schema,
    >(
        &mut self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        sender: Sender<'wait, CallResult<'pool, 'id, I, CAPS>>,
        target: &clients::Client<'id>,
        method: Method<InterfaceType, Params, Results>,
        capabilities: &[&clients::Client<'id>],
        messages: &mut messages::Messages<'pool, '_, '_>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        fill: impl FnOnce(FixedStruct<'_, Params>) -> capnp::Result<()>,
    ) -> Result<dispatch::TaskKey<'id>, RejectedInvocation<'wait, 'pool, 'id, I, CAPS>> {
        self.enqueue_with(sender, !method.no_promise_pipelining(), || {
            driver.enqueue_call(
                target,
                method,
                capabilities,
                messages,
                minimum,
                options,
                fill,
            )
        })
    }
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn enqueue_streaming<
        A,
        P: Copy + Eq,
        S,
        const PATH: usize,
        IF: Interface,
        Params: Schema,
    >(
        &mut self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        sender: Sender<'wait, CallResult<'pool, 'id, I, CAPS>>,
        target: &clients::Client<'id>,
        method: StreamingMethod<IF, Params>,
        capabilities: &[&clients::Client<'id>],
        messages: &mut messages::Messages<'pool, '_, '_>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        fill: impl FnOnce(FixedStruct<'_, Params>) -> capnp::Result<()>,
    ) -> Result<dispatch::TaskKey<'id>, RejectedInvocation<'wait, 'pool, 'id, I, CAPS>> {
        self.enqueue_with(sender, false, || {
            driver.enqueue_streaming(
                target,
                method,
                capabilities,
                messages,
                minimum,
                options,
                fill,
            )
        })
    }
    fn enqueue_with(
        &mut self,
        sender: Sender<'wait, CallResult<'pool, 'id, I, CAPS>>,
        allows_pipeline: bool,
        enqueue: impl FnOnce() -> Result<dispatch::TaskKey<'id>, Error>,
    ) -> Result<dispatch::TaskKey<'id>, RejectedInvocation<'wait, 'pool, 'id, I, CAPS>> {
        let Some(slot) = self
            .slots
            .vacant_mut(|_, slot| slot.entry.is_none(), || InvocationSlot::EMPTY)
        else {
            return Err(RejectedInvocation {
                error: Error::Busy,
                sender,
            });
        };
        let task = match enqueue() {
            Ok(task) => task,
            Err(error) => return Err(RejectedInvocation { error, sender }),
        };
        slot.entry = Some(Entry {
            task,
            delivery: sender.track(),
            stage: Stage::Queued,
            pipeline: 0,
            pipeline_remaining: 0,
            allows_pipeline,
        });
        Ok(task)
    }

    /// Resolve a late projection against the invocation's actual stage. An
    /// executing local call can acquire observers before its result is ready;
    /// completion settles them before delivering the parent reply.
    pub fn pipeline<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        task: dispatch::TaskKey<'id>,
        path: pipeline::Path<'_>,
        messages: &messages::Messages<'pool, '_, '_>,
    ) -> Result<clients::Client<'id>, Error> {
        self.pipeline_with(driver, task, path, messages, |_, _, _| Ok(None))
    }
    /// Consult caller-owned early pipelines before local final results,
    /// including failed methods. Returned clients are independently retained.
    pub fn pipeline_with<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        task: dispatch::TaskKey<'id>,
        path: pipeline::Path<'_>,
        messages: &messages::Messages<'pool, '_, '_>,
        resolve: impl FnOnce(
            dispatch::TaskKey<'id>,
            pipeline::Path<'_>,
            &mut Runtime<'pool, 'id, '_, I, P, S, CAPS, PATH>,
        ) -> Result<Option<clients::Client<'id>>, Error>,
    ) -> Result<clients::Client<'id>, Error> {
        driver.check_live()?;
        let entry = self
            .slots
            .iter()
            .filter_map(|slot| slot.entry.as_ref())
            .find(|entry| entry.task == task)
            .ok_or(Error::UnknownInvocation)?;
        if !entry.allows_pipeline {
            return Err(Error::Input(super::super::Error::Wire(
                capnp::ErrorKind::Unimplemented,
            )));
        }
        if matches!(
            entry.stage,
            Stage::Running | Stage::Completed(Ok(CallOutput::Local(_)) | Err(_))
        ) {
            if let Some(client) = resolve(task, path, &mut driver.runtime)? {
                return Ok(client);
            }
        }
        match &entry.stage {
            Stage::Queued => driver.invocation_pipeline(task, path, messages),
            Stage::Sent(question) => driver.pipeline(*question, path),
            Stage::Running => driver.observe_invocation(task, path),
            Stage::Completed(Ok(CallOutput::Local(result))) => result
                .with(messages, |payload| match payload.pipeline(path) {
                    Ok(client) => driver.runtime.retain(client).map_err(Error::Graph),
                    Err(error) => {
                        let kind = match error {
                            super::super::Error::Wire(kind) => kind,
                            super::super::Error::Capacity => capnp::ErrorKind::Failed,
                            error => return Err(Error::Input(error)),
                        };
                        driver
                            .runtime
                            .endpoint_with(clients::Location::Broken, || {
                                binding::Route::broken(kind)
                            })
                            .map_err(Error::Graph)
                    }
                })
                .map_err(Error::Input)?,
            Stage::Completed(Err(kind)) => driver
                .runtime
                .endpoint_with(clients::Location::Broken, || binding::Route::broken(*kind))
                .map_err(Error::Graph),
            Stage::Completed(Ok(CallOutput::Remote(response))) => response
                .with(messages, |reply| match reply {
                    super::super::Reply::Results(payload) => {
                        let capability = match payload.pipeline(path) {
                            Ok(capability) => capability,
                            // A completed payload cannot gain another field on
                            // retry. Match preexisting pipeline propagation by
                            // breaking this projection, while keeping allocation
                            // failures below retryable.
                            Err(_) => {
                                return driver
                                    .runtime
                                    .endpoint_with(clients::Location::Broken, || {
                                        binding::Route::broken(capnp::ErrorKind::Failed)
                                    })
                                    .map_err(Error::Graph)
                            }
                        };
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
                            .map(|bound| bound.into_client())
                            .map_err(|error| Error::Outgoing(outgoing::Error::Binding(error)))
                    }
                    super::super::Reply::Exception(error) => {
                        let kind =
                            super::super::controls::abort_reason(error).map_err(Error::Input)?;
                        driver
                            .runtime
                            .endpoint_with(clients::Location::Broken, || {
                                binding::Route::broken(kind)
                            })
                            .map_err(Error::Graph)
                    }
                    _ => Err(Error::PendingResponse),
                })
                .map_err(Error::Input)?,
            Stage::Delivered => Err(Error::PendingResponse),
        }
    }
    /// Output acceptance and binding transition form one non-suspending step.
    /// Rejection leaves the queued call and completion registration intact.
    pub fn send_one<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        scratch: &mut [u32],
    ) -> Result<Option<SentInvocation<'id>>, Error> {
        self.send_one_using(driver, |driver| {
            driver.send_invocation(messages, queue, minimum, options, scratch)
        })
    }
    /// Resource-aware output preserves the same completion registration and
    /// cancellation checks as ordinary output.
    #[allow(clippy::too_many_arguments)]
    pub fn send_one_with_attachments<A, P: Copy + Eq, S, R, const PATH: usize, const N: usize>(
        &mut self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, crate::attachments::Sender<R, N>>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        scratch: &mut [u32],
        capacity: Capacity,
        imported: impl FnMut(&I) -> Option<R>,
        local: impl FnMut(&S) -> Option<R>,
    ) -> Result<Option<SentInvocation<'id>>, Error> {
        self.send_one_using(driver, |driver| {
            driver.send_invocation_with_attachments(
                messages, queue, minimum, options, scratch, capacity, imported, local,
            )
        })
    }
    /// Publish the oldest registered invocation with a caller-defined resource
    /// bundle, preserving cancellation and completion registration atomically.
    #[allow(clippy::too_many_arguments)]
    pub fn send_one_with_resources<A, P: Copy + Eq, S, R, const PATH: usize>(
        &mut self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, R>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        scratch: &mut [u32],
        resources: R,
        attach: impl FnMut(
            &binding::Route<'id, P, S, PATH>,
            &imports::Imports<'id, '_, I>,
            &mut R,
        ) -> u8,
    ) -> Result<Option<SentInvocation<'id>>, Error> {
        self.send_one_using(driver, |driver| {
            driver.send_invocation_using(
                messages, queue, minimum, options, scratch, resources, attach,
            )
        })
    }
    fn send_one_using<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        send: impl FnOnce(
            &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        ) -> Result<Option<SentInvocation<'id>>, Error>,
    ) -> Result<Option<SentInvocation<'id>>, Error> {
        let Some(ready) = driver.runtime.next_ready().map_err(Error::Dispatch)? else {
            return Ok(None);
        };
        let task = ready.key();
        drop(ready);
        let entry = self
            .slots
            .iter_mut()
            .filter_map(|slot| slot.entry.as_mut())
            .find(|entry| entry.task == task && matches!(entry.stage, Stage::Queued));
        let Some(entry) = entry else {
            return Ok(None);
        };
        if entry.delivery.is_observed() {
            return Ok(None);
        }
        let Some(sent) = send(driver)? else {
            return Ok(None);
        };
        debug_assert_eq!(sent.task, task);
        entry.stage = Stage::Sent(sent.question);
        Ok(Some(sent))
    }
    /// Freeze and accept only the oldest ready registered local invocation.
    /// The caller owns its application future and any chosen cancellation policy.
    pub fn accept_local<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
    ) -> Result<Option<LocalCall<'pool, 'id, I, CAPS, PATH>>, Error> {
        driver.check_live()?;
        let Some(ready) = driver.runtime.next_ready().map_err(Error::Dispatch)? else {
            return Ok(None);
        };
        let task = ready.key();
        if !matches!(
            ready.destination(),
            clients::Destination::Endpoint {
                location: clients::Location::Local,
                ..
            } | clients::Destination::Endpoint {
                owner: binding::Route::Capability(binding::Endpoint::Failed { .. }),
                ..
            }
        ) {
            return Ok(None);
        }
        let Some(entry) = self
            .slots
            .iter_mut()
            .filter_map(|slot| slot.entry.as_mut())
            .find(|entry| entry.task == task && matches!(entry.stage, Stage::Queued))
        else {
            return Ok(None);
        };
        if entry.delivery.is_observed() {
            return Ok(None);
        }
        entry.stage = Stage::Running;
        Ok(Some(LocalCall {
            task,
            running: ready.accept(),
        }))
    }
    #[allow(clippy::result_large_err)]
    pub fn complete_local<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        call: LocalCall<'pool, 'id, I, CAPS, PATH>,
        result: Result<LocalResult<'pool, 'id, CAPS>, capnp::ErrorKind>,
    ) -> Result<(), RejectedLocal<'pool, 'id, I, CAPS, PATH>> {
        let Some(entry) = self
            .slots
            .iter_mut()
            .filter_map(|slot| slot.entry.as_mut())
            .find(|entry| entry.task == call.task && matches!(entry.stage, Stage::Running))
        else {
            return Err(RejectedLocal {
                error: Error::UnknownInvocation,
                call,
                result,
            });
        };
        if let Ok(output) = &result {
            let valid = call.params(messages, |params| {
                params.interface_id() == output.interface && params.method_id() == output.method
            });
            if !matches!(valid, Ok(true)) {
                return Err(RejectedLocal {
                    error: Error::Input(super::super::Error::WrongMessage),
                    call,
                    result,
                });
            }
        }
        call.release(driver, messages);
        entry.stage = Stage::Completed(result.map(CallOutput::Local));
        Ok(())
    }

    /// Observe dropped local caller interest with the same tracked completion
    /// slot. Application cancellation policy remains with its execution owner.
    pub fn poll_local_canceled(
        &mut self,
        task: dispatch::TaskKey<'id>,
        cx: &mut Context<'_>,
    ) -> Result<core::task::Poll<()>, Error> {
        let entry = self
            .slots
            .iter_mut()
            .filter_map(|s| s.entry.as_mut())
            .find(|entry| entry.task == task && matches!(entry.stage, Stage::Running))
            .ok_or(Error::UnknownInvocation)?;
        Ok(entry.delivery.poll_observed(cx))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallProgress {
    Idle,
    Pending,
    Advanced,
    Delivered,
    Consumed,
    Canceled,
    Reclaimed,
}
impl<'wait, 'pool, 'id, I, const CAPS: usize> Invocations<'_, 'wait, 'pool, 'id, I, CAPS> {
    /// Advance one binding, including at most one local result pipeline. Poll
    /// again after Advanced/Delivered; Pending waits on IO, application progress,
    /// graph capacity or the registered receiver notification.
    pub fn poll_one<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        index: usize,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        cx: &mut Context<'_>,
    ) -> Result<CallProgress, Error> {
        self.poll_one_with(index, driver, messages, cx, |_, _, _| Ok(None))
    }
    /// Drive completion with application pipeline resolution. A local producer
    /// may publish paths while running or after failure. Outer errors retry
    /// without losing the observer, result owner or completion binding.
    pub fn poll_one_with<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        index: usize,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        cx: &mut Context<'_>,
        mut resolve: impl FnMut(
            dispatch::TaskKey<'id>,
            pipeline::Path<'_>,
            &mut Runtime<'pool, 'id, '_, I, P, S, CAPS, PATH>,
        ) -> Result<Option<clients::Client<'id>>, Error>,
    ) -> Result<CallProgress, Error> {
        let Some(slot) = self.slots.get_mut(index) else {
            return Ok(CallProgress::Idle);
        };
        let Some(entry) = slot.entry.as_mut() else {
            return Ok(CallProgress::Idle);
        };
        let observed = entry.delivery.poll_observed(cx).is_ready();
        match &entry.stage {
            Stage::Queued => {
                if observed {
                    if !driver.cancel_invocation(entry.task, messages) {
                        return Err(Error::UnknownInvocation);
                    }
                    slot.entry = None;
                    return Ok(CallProgress::Canceled);
                }
                driver.check_live()?;
                let broken = driver
                    .runtime
                    .next_ready()
                    .map_err(Error::Dispatch)?
                    .and_then(|ready| {
                        if ready.key() != entry.task {
                            return None;
                        }
                        match ready.destination() {
                            clients::Destination::Endpoint {
                                owner: binding::Route::Capability(binding::Endpoint::Broken(kind)),
                                ..
                            } => Some(*kind),
                            _ => None,
                        }
                    });
                if let Some(kind) = broken {
                    if !driver.cancel_invocation(entry.task, messages) {
                        return Err(Error::UnknownInvocation);
                    }
                    entry.stage = Stage::Completed(Err(kind));
                    return Ok(CallProgress::Advanced);
                }
                return Ok(CallProgress::Pending);
            }
            Stage::Running => {
                let capacity = driver.invocation_pipelines.len();
                if entry.pipeline_remaining == 0 {
                    entry.pipeline_remaining = capacity;
                }
                let next = (0..capacity)
                    .map(|offset| (entry.pipeline + offset) % capacity)
                    .find(|index| driver.invocation_pipelines[*index].pending_local(entry.task));
                if let Some(index) = next {
                    entry.pipeline = (index + 1) % capacity;
                    let progress = driver.poll_local_invocation_pipeline_when_ready(
                        index,
                        entry.task,
                        |path, runtime| {
                            resolve(entry.task, path, runtime).map(|client| client.map(Ok))
                        },
                    )?;
                    if matches!(progress, Propagation::Resolved | Propagation::Abandoned) {
                        entry.pipeline_remaining = 0;
                        return Ok(CallProgress::Advanced);
                    }
                    entry.pipeline_remaining -= 1;
                    if entry.pipeline_remaining != 0 {
                        return Ok(CallProgress::Advanced);
                    }
                }
                return Ok(CallProgress::Pending);
            }
            Stage::Sent(question) => {
                if observed {
                    driver.cancel(*question)?;
                    slot.entry = None;
                    return Ok(CallProgress::Canceled);
                }
                driver.check_live()?;
                let result = match driver.take_output(*question, messages) {
                    Ok(result) => result,
                    Err(Error::PendingResponse | Error::PendingAuthorization) => {
                        return Ok(CallProgress::Pending)
                    }
                    Err(error) => return Err(error),
                };
                // take_output already waits for remote pipeline observers.
                // Deliver directly instead of moving the affine response into
                // Completed and immediately extracting it again below.
                return Self::deliver_result(slot, driver, messages, result);
            }
            Stage::Completed(result) => {
                let capacity = driver.invocation_pipelines.len();
                let next = (0..capacity)
                    .map(|offset| (entry.pipeline + offset) % capacity)
                    .find(|index| driver.invocation_pipelines[*index].pending_local(entry.task));
                if let Some(index) = next {
                    entry.pipeline = (index + 1) % capacity;
                    let progress = driver.poll_local_invocation_pipeline_when_ready(
                        index,
                        entry.task,
                        |path, runtime| {
                            resolve(entry.task, path, runtime).map(|client| client.map(Ok))
                        },
                    )?;
                    if matches!(progress, Propagation::Resolved | Propagation::Abandoned) {
                        return Ok(CallProgress::Advanced);
                    }
                    match result {
                        Ok(CallOutput::Local(result)) => {
                            driver.poll_local_result(index, entry.task, result, messages)?;
                        }
                        Err(kind) => {
                            driver.poll_local_invocation_pipeline(index, entry.task, |_, _| {
                                Err(*kind)
                            })?;
                        }
                        Ok(CallOutput::Remote(_)) => {
                            unreachable!("remote response already waited for observers")
                        }
                    }
                    return Ok(CallProgress::Advanced);
                }
            }
            Stage::Delivered => {
                if !observed {
                    return Ok(CallProgress::Pending);
                }
                let progress = if let Some(result) = entry.delivery.take_abandoned() {
                    if let Ok(output) = result {
                        driver.release_output(output, messages);
                    }
                    CallProgress::Reclaimed
                } else {
                    CallProgress::Consumed
                };
                slot.entry = None;
                return Ok(progress);
            }
        }
        let Stage::Completed(result) = core::mem::replace(&mut entry.stage, Stage::Delivered)
        else {
            unreachable!()
        };
        Self::deliver_result(slot, driver, messages, result)
    }

    #[inline]
    fn deliver_result<A, P: Copy + Eq, S, const PATH: usize>(
        slot: &mut InvocationSlot<'wait, 'pool, 'id, I, CAPS>,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        result: CallResult<'pool, 'id, I, CAPS>,
    ) -> Result<CallProgress, Error> {
        let entry = slot.entry.as_mut().expect("live invocation completion");
        debug_assert!(matches!(entry.stage, Stage::Sent(_) | Stage::Delivered));
        entry.stage = Stage::Delivered;
        if let Err(result) = entry.delivery.try_send(result) {
            if let Ok(output) = result {
                driver.release_output(output, messages);
            }
            slot.entry = None;
            Ok(CallProgress::Reclaimed)
        } else {
            Ok(CallProgress::Delivered)
        }
    }
    /// End all bindings before connection teardown. Recover unconsumed replies,
    /// including forgotten receivers, and release queued/completed local work.
    /// The return value counts externally running local calls: release those
    /// handles and any already-consumed outputs before terminating the driver.
    pub fn terminate<A, P: Copy + Eq, S, const PATH: usize>(
        mut self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> usize {
        let mut running = 0;
        for slot in self.slots.iter_mut() {
            let Some(mut entry) = slot.entry.take() else {
                continue;
            };
            if let Some(Ok(output)) = entry.delivery.cancel() {
                driver.release_output(output, messages);
            }
            match entry.stage {
                Stage::Queued => {
                    driver.cancel_invocation(entry.task, messages);
                }
                Stage::Sent(question) => {
                    let _ = driver.cancel(question);
                }
                Stage::Running => running += 1,
                Stage::Completed(Ok(output)) => driver.release_output(output, messages),
                Stage::Completed(Err(_)) | Stage::Delivered => (),
            }
        }
        running
    }
}
