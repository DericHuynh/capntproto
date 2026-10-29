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

// Only local completions wait here for pipeline observers. Remote responses
// are delivered directly from Sent after take_output resolves their observers.
// Local results retain their affine capability owners inline.
// Boxing this variant would require an allocator in the fixed-storage profile.
#[allow(clippy::large_enum_variant)]
enum Stage<'pool, 'id, const CAPS: usize> {
    Queued,
    Sent(questions::Key<'id>),
    Running,
    Completed(Result<LocalResult<'pool, 'id, CAPS>, capnp::ErrorKind>),
    Delivered,
}
struct Entry<'wait, 'pool, 'id, I, const CAPS: usize> {
    task: InvocationKey<'id>,
    delivery: Delivery<'wait, CallResult<'pool, 'id, I, CAPS>>,
    stage: Stage<'pool, 'id, CAPS>,
    pipeline: usize,
    pipeline_remaining: usize,
    allows_pipeline: bool,
}
pub struct InvocationSlot<'wait, 'pool, 'id, I, const CAPS: usize> {
    entry: Option<Entry<'wait, 'pool, 'id, I, CAPS>>,
    free: capntproto_rpc_engine::free_slots::Link,
}
impl<I, const CAPS: usize> InvocationSlot<'_, '_, '_, I, CAPS> {
    pub const EMPTY: Self = Self {
        entry: None,
        free: capntproto_rpc_engine::free_slots::Link::EMPTY,
    };
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
    task: InvocationKey<'id>,
    running: dispatch::Running<'id, Task<'pool, 'id, I, CAPS, PATH>>,
}
impl<'pool, 'id, I, const CAPS: usize, const PATH: usize> LocalCall<'pool, 'id, I, CAPS, PATH> {
    pub fn task(&self) -> InvocationKey<'id> {
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
        if let Ok(operation) = driver.operations.get_mut(self.task) {
            operation.state = operations::State::Completed;
        }
        driver.operations.retire(self.task);
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
    live: usize,
    queued: usize,
    free: capntproto_rpc_engine::free_slots::FreeSlots,
}
impl<'s, 'wait, 'pool, 'id, I, const CAPS: usize> Invocations<'s, 'wait, 'pool, 'id, I, CAPS> {
    pub fn new(slots: &'s mut [InvocationSlot<'wait, 'pool, 'id, I, CAPS>]) -> Result<Self, Error> {
        if slots.iter().any(|slot| slot.entry.is_some()) {
            return Err(Error::NonemptyStorage);
        }
        Ok(Self {
            slots: SlotStorage::borrowed(slots),
            live: 0,
            queued: 0,
            free: capntproto_rpc_engine::free_slots::FreeSlots::new(),
        })
    }
    /// Grow completion bindings independently of dispatch. Reply slots and the
    /// futures borrowing them still belong to the caller and never relocate.
    #[cfg(feature = "alloc")]
    pub fn new_growing(initial: usize, maximum: usize) -> Result<Self, Error> {
        Ok(Self {
            live: 0,
            queued: 0,
            free: capntproto_rpc_engine::free_slots::FreeSlots::new(),
            slots: SlotStorage::growing(initial, maximum, || InvocationSlot::EMPTY)
                .ok_or(Error::Input(super::super::Error::Capacity))?,
        })
    }
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }
    /// Whether another completion binding fits the configured bound. A
    /// delivered reply retains its slot until consumption/cancellation is
    /// observed; hosted request capacity alone is not sufficient for admission.
    /// Growing storage may still reject allocation at the actual reservation.
    pub fn has_capacity(&self) -> bool {
        self.live < self.slots.maximum()
    }
    pub fn has_queued(&self) -> bool {
        self.queued != 0
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
    ) -> Result<InvocationKey<'id>, RejectedInvocation<'wait, 'pool, 'id, I, CAPS>> {
        self.enqueue_dynamic_to(
            driver,
            sender,
            target,
            method,
            capabilities.iter().copied(),
            messages,
            minimum,
            options,
            calls::CallDestination::Caller,
            fill,
        )
    }
    /// Track a dynamic call whose result is delivered through an authenticated
    /// third-party rendezvous. Completion still owns the original wire question.
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn enqueue_dynamic_to<'c, A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        sender: Sender<'wait, CallResult<'pool, 'id, I, CAPS>>,
        target: &clients::Client<'id>,
        method: calls::DynamicCall,
        capabilities: impl ExactSizeIterator<Item = Option<&'c clients::Client<'id>>>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        destination: calls::CallDestination<'_>,
        fill: impl FnOnce(
            capnp::field_api::FixedField<'_, capnp::field_api::Generic<capnp::any_pointer::Owned>>,
        ) -> capnp::Result<()>,
    ) -> Result<InvocationKey<'id>, RejectedInvocation<'wait, 'pool, 'id, I, CAPS>>
    where
        'id: 'c,
    {
        self.enqueue_with(driver, sender, !method.no_promise_pipelining, |driver| {
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
    /// Track a call while retaining each borrowed parameter directly into its
    /// invocation. A failed table lookup preserves the completion sender.
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn enqueue_dynamic_to_table<
        A,
        P: Copy + Eq,
        S,
        const PATH: usize,
        T: super::CapabilityTable<'id> + ?Sized,
    >(
        &mut self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        sender: Sender<'wait, CallResult<'pool, 'id, I, CAPS>>,
        target: &clients::Client<'id>,
        method: calls::DynamicCall,
        capabilities: &T,
        messages: &mut messages::Messages<'pool, '_, '_>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        destination: calls::CallDestination<'_>,
        fill: impl FnOnce(
            capnp::field_api::FixedField<'_, capnp::field_api::Generic<capnp::any_pointer::Owned>>,
        ) -> capnp::Result<()>,
    ) -> Result<InvocationKey<'id>, RejectedInvocation<'wait, 'pool, 'id, I, CAPS>> {
        self.enqueue_with(driver, sender, !method.no_promise_pipelining, |driver| {
            driver.enqueue_dynamic_to_table(
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
    ) -> Result<InvocationKey<'id>, RejectedInvocation<'wait, 'pool, 'id, I, CAPS>> {
        self.enqueue_with(driver, sender, !method.no_promise_pipelining(), |driver| {
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
    ) -> Result<InvocationKey<'id>, RejectedInvocation<'wait, 'pool, 'id, I, CAPS>> {
        self.enqueue_with(driver, sender, false, |driver| {
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
    fn enqueue_with<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        sender: Sender<'wait, CallResult<'pool, 'id, I, CAPS>>,
        allows_pipeline: bool,
        enqueue: impl FnOnce(
            &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        ) -> Result<InvocationKey<'id>, Error>,
    ) -> Result<InvocationKey<'id>, RejectedInvocation<'wait, 'pool, 'id, I, CAPS>> {
        if self.free.peek(&self.slots, |slot| &slot.free).is_none() {
            self.slots.grow(1, || InvocationSlot::EMPTY);
        }
        let Some(index) = self.free.peek(&self.slots, |slot| &slot.free) else {
            return Err(RejectedInvocation {
                error: Error::Busy,
                sender,
            });
        };
        let task = match enqueue(driver) {
            Ok(task) => task,
            Err(error) => return Err(RejectedInvocation { error, sender }),
        };
        driver
            .operations
            .get_mut(task)
            .expect("new operation binding")
            .binding = Some(index);
        let reserved = self.free.take(&mut self.slots, |slot| &mut slot.free);
        debug_assert_eq!(reserved, index);
        self.slots[index].entry = Some(Entry {
            task,
            delivery: sender.track(),
            stage: Stage::Queued,
            pipeline: usize::MAX,
            pipeline_remaining: 0,
            allows_pipeline,
        });
        self.live += 1;
        self.queued += 1;
        Ok(task)
    }

    /// Attach a completion owner to an existing, still-queued invocation. This
    /// permits a caller which normally consumes remote results directly to
    /// acquire the local completion machinery only if its target resolves
    /// locally. The exact task generation must still own a Call, and a task
    /// cannot acquire a second binding. Rejection leaves both owners intact.
    #[allow(clippy::result_large_err)]
    pub fn bind_queued<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &messages::Messages<'pool, '_, '_>,
        task: InvocationKey<'id>,
        sender: Sender<'wait, CallResult<'pool, 'id, I, CAPS>>,
    ) -> Result<(), RejectedInvocation<'wait, 'pool, 'id, I, CAPS>> {
        let allowed = (|| {
            driver.check_live()?;
            if driver.operations.get(task)?.binding.is_some() {
                return Err(Error::UnknownInvocation);
            }
            let invocation = driver.queued_invocation(task)?;
            invocation.allows_pipeline(messages).map_err(Error::Input)
        })();
        let allows_pipeline = match allowed {
            Ok(allowed) => allowed,
            Err(error) => return Err(RejectedInvocation { error, sender }),
        };
        self.enqueue_with(driver, sender, allows_pipeline, |_| Ok(task))
            .map(|_| ())
    }

    /// Resolve a late projection against the invocation's actual stage. An
    /// executing local call can acquire observers before its result is ready;
    /// completion settles them before delivering the parent reply.
    pub fn pipeline<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        task: InvocationKey<'id>,
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
        task: InvocationKey<'id>,
        path: pipeline::Path<'_>,
        messages: &messages::Messages<'pool, '_, '_>,
        resolve: impl FnOnce(
            InvocationKey<'id>,
            pipeline::Path<'_>,
            &mut Scope<'_, 'pool, 'id, '_, I, P, S, CAPS, PATH>,
        ) -> Result<Option<clients::Client<'id>>, Error>,
    ) -> Result<clients::Client<'id>, Error> {
        driver.check_live()?;
        let index = driver
            .invocation_binding(task)
            .ok_or(Error::UnknownInvocation)?;
        let entry = self
            .slots
            .get(index)
            .and_then(|slot| slot.entry.as_ref())
            .filter(|entry| entry.task == task)
            .ok_or(Error::UnknownInvocation)?;
        if !entry.allows_pipeline {
            return Err(Error::Input(super::super::Error::Wire(
                capnp::ErrorKind::Unimplemented,
            )));
        }
        if matches!(entry.stage, Stage::Running | Stage::Completed(_)) {
            if let Some(client) = resolve(
                task,
                path,
                &mut clients::Access::with_export_sources(
                    &mut driver.runtime,
                    Some(&driver.ledgers.exports),
                ),
            )? {
                return Ok(client);
            }
        }
        match &entry.stage {
            Stage::Queued => driver.invocation_pipeline(task, path, messages),
            Stage::Sent(question) => driver.pipeline(*question, path),
            Stage::Running => driver.observe_invocation(task, path),
            Stage::Completed(Ok(result)) => result
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
        let Some(ready) = driver.runtime.ready().map_err(Error::Dispatch)? else {
            return Ok(None);
        };
        let Task::Invoke(invocation) = ready.owner() else {
            return Ok(None);
        };
        let task = invocation.key;
        drop(ready);
        let entry = driver
            .operations
            .get(task)
            .ok()
            .and_then(|operation| operation.binding)
            .and_then(|index| self.slots.get_mut(index))
            .and_then(|slot| slot.entry.as_mut())
            .filter(|entry| entry.task == task && matches!(entry.stage, Stage::Queued));
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
        self.queued -= 1;
        Ok(Some(sent))
    }
    /// Freeze and accept only the oldest ready registered local invocation.
    /// The caller owns its application future and any chosen cancellation policy.
    pub fn accept_local<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
    ) -> Result<Option<LocalCall<'pool, 'id, I, CAPS, PATH>>, Error> {
        driver.check_live()?;
        let Some(ready) = driver.runtime.ready().map_err(Error::Dispatch)? else {
            return Ok(None);
        };
        let Task::Invoke(invocation) = ready.owner() else {
            return Ok(None);
        };
        let task = invocation.key;
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
        let Some(entry) = driver
            .operations
            .get(task)
            .ok()
            .and_then(|operation| operation.binding)
            .and_then(|index| self.slots.get_mut(index))
            .and_then(|slot| slot.entry.as_mut())
            .filter(|entry| entry.task == task && matches!(entry.stage, Stage::Queued))
        else {
            return Ok(None);
        };
        if entry.delivery.is_observed() {
            return Ok(None);
        }
        entry.stage = Stage::Running;
        self.queued -= 1;
        driver.operations.get_mut(task)?.state = operations::State::Running;
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
        let Some(entry) = driver
            .operations
            .get(call.task)
            .ok()
            .and_then(|operation| operation.binding)
            .and_then(|index| self.slots.get_mut(index))
            .and_then(|slot| slot.entry.as_mut())
            .filter(|entry| entry.task == call.task && matches!(entry.stage, Stage::Running))
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
        entry.stage = Stage::Completed(result);
        Ok(())
    }

    /// Observe dropped local caller interest with the same tracked completion
    /// slot. Application cancellation policy remains with its execution owner.
    pub fn poll_local_canceled<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        driver: &Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        task: InvocationKey<'id>,
        cx: &mut Context<'_>,
    ) -> Result<core::task::Poll<()>, Error> {
        let entry = driver
            .invocation_binding(task)
            .and_then(|index| self.slots.get_mut(index))
            .and_then(|slot| slot.entry.as_mut())
            .filter(|entry| entry.task == task && matches!(entry.stage, Stage::Running))
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
            InvocationKey<'id>,
            pipeline::Path<'_>,
            &mut Scope<'_, 'pool, 'id, '_, I, P, S, CAPS, PATH>,
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
                    if let Some(entry) = slot.entry.take() {
                        self.live -= 1;
                        self.queued -= usize::from(matches!(entry.stage, Stage::Queued));
                        driver.operations.unbind(entry.task);
                        self.free
                            .release(index, &mut self.slots, |slot| &mut slot.free);
                    }
                    return Ok(CallProgress::Canceled);
                }
                driver.check_live()?;
                let broken = driver
                    .runtime
                    .ready()
                    .map_err(Error::Dispatch)?
                    .and_then(|ready| {
                        if !matches!(ready.owner(), Task::Invoke(invocation) if invocation.key == entry.task) {
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
                    self.queued -= 1;
                    return Ok(CallProgress::Advanced);
                }
                return Ok(CallProgress::Pending);
            }
            Stage::Running => {
                let operation = driver.operations.get(entry.task)?;
                if entry.pipeline_remaining == 0
                    || entry.pipeline_remaining > operation.observer_count
                {
                    entry.pipeline_remaining = operation.observer_count;
                }
                let next = driver.invocation_pipelines.local_next(
                    entry.task,
                    entry.pipeline,
                    operation.observers,
                );
                if let Some(index) = next {
                    entry.pipeline = driver
                        .invocation_pipelines
                        .next(index)
                        .or(operation.observers)
                        .unwrap_or(usize::MAX);
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
                    if let Some(entry) = slot.entry.take() {
                        self.live -= 1;
                        self.queued -= usize::from(matches!(entry.stage, Stage::Queued));
                        driver.operations.unbind(entry.task);
                        self.free
                            .release(index, &mut self.slots, |slot| &mut slot.free);
                    }
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
                return Self::deliver_result(
                    &mut self.slots,
                    index,
                    &mut self.live,
                    &mut self.free,
                    driver,
                    messages,
                    result,
                );
            }
            Stage::Completed(result) => {
                let operation = driver.operations.get(entry.task)?;
                let next = driver.invocation_pipelines.local_next(
                    entry.task,
                    entry.pipeline,
                    operation.observers,
                );
                if let Some(index) = next {
                    entry.pipeline = driver
                        .invocation_pipelines
                        .next(index)
                        .or(operation.observers)
                        .unwrap_or(usize::MAX);
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
                        Ok(result) => {
                            driver.poll_local_result(index, entry.task, result, messages)?;
                        }
                        Err(kind) => {
                            driver.poll_local_invocation_pipeline(index, entry.task, |_, _| {
                                Err(*kind)
                            })?;
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
                if let Some(entry) = slot.entry.take() {
                    self.live -= 1;
                    self.queued -= usize::from(matches!(entry.stage, Stage::Queued));
                    driver.operations.unbind(entry.task);
                    self.free
                        .release(index, &mut self.slots, |slot| &mut slot.free);
                }
                return Ok(progress);
            }
        }
        let Stage::Completed(result) = core::mem::replace(&mut entry.stage, Stage::Delivered)
        else {
            unreachable!()
        };
        Self::deliver_result(
            &mut self.slots,
            index,
            &mut self.live,
            &mut self.free,
            driver,
            messages,
            result.map(CallOutput::Local),
        )
    }

    #[inline]
    fn deliver_result<A, P: Copy + Eq, S, const PATH: usize>(
        slots: &mut SlotStorage<'_, InvocationSlot<'wait, 'pool, 'id, I, CAPS>>,
        index: usize,
        live: &mut usize,
        free: &mut capntproto_rpc_engine::free_slots::FreeSlots,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        result: CallResult<'pool, 'id, I, CAPS>,
    ) -> Result<CallProgress, Error> {
        let slot = &mut slots[index];
        let entry = slot.entry.as_mut().expect("live invocation completion");
        debug_assert!(matches!(entry.stage, Stage::Sent(_) | Stage::Delivered));
        entry.stage = Stage::Delivered;
        if let Err(result) = entry.delivery.try_send(result) {
            if let Ok(output) = result {
                driver.release_output(output, messages);
            }
            if let Some(entry) = slot.entry.take() {
                *live -= 1;
                driver.operations.unbind(entry.task);
                free.release(index, slots, |slot| &mut slot.free);
            }
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
            driver.operations.unbind(entry.task);
            match entry.stage {
                Stage::Queued => {
                    driver.cancel_invocation(entry.task, messages);
                }
                Stage::Sent(question) => {
                    let _ = driver.cancel(question);
                }
                Stage::Running => running += 1,
                Stage::Completed(Ok(output)) => {
                    driver.release_output(CallOutput::Local(output), messages)
                }
                Stage::Completed(Err(_)) | Stage::Delivered => (),
            }
        }
        running
    }
}
