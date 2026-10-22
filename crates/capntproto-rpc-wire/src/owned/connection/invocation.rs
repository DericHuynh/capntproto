//! Outgoing application calls share the graph's ordered task sequence. Parameter
//! capabilities stay local until output admits the exact generated descriptor
//! table. A rejected send preserves the queued invocation and its references.
use super::*;
use crate::rpc_capnp::api::{Call as WireCall, CapDescriptor, Message, Payload as WirePayload};
use capnp::{
    field_api::{FixedStruct, Schema},
    wire_capability::{Interface, Method, StreamingMethod},
};
use capntproto_rpc_engine::clients::Capabilities;
mod immediate;
mod template;
pub use immediate::ImmediateParams;
pub use template::EncodedCall;
use template::PlainTemplate;
#[cfg(feature = "alloc")]
pub use template::{PreparedCall, PreparedCallTemplate};

pub(super) fn map_capabilities<'a, 'id, const CAPS: usize>(
    capabilities: &[&'a clients::Client<'id>],
    capacity: Capacity,
) -> Result<Buffer<Option<&'a clients::Client<'id>>, CAPS>, Error> {
    let mut mapped = capacity
        .buffer(capabilities.len(), || None)
        .ok_or(Error::Input(super::super::Error::Capacity))?;
    for (slot, client) in mapped.iter_mut().zip(capabilities) {
        *slot = Some(*client);
    }
    Ok(mapped)
}

#[must_use = "complete or release the invocation through its originating graph and pool"]
pub struct Invocation<'pool, 'id, const CAPS: usize> {
    frame: Frame<'pool>,
    template: Option<PlainTemplate>,
    capabilities: Buffer<Option<clients::Client<'id>>, CAPS>,
    count: usize,
    completion_owner: Option<core::num::NonZeroUsize>,
}
/// A local invocation's parameters retain their message and capability domain.
/// The template's placeholder wire IDs are deliberately not exposed.
pub struct InvocationParams<'m, 'r, 'id, const CAPS: usize> {
    call: rpc_capnp::call::Reader<'m>,
    capabilities: &'r [Option<clients::Client<'id>>],
    count: usize,
}
impl<'m, 'r, 'id, const CAPS: usize> InvocationParams<'m, 'r, 'id, CAPS> {
    /// Validated content paired with its exact retained capability table. The
    /// wire IDs in the private template are not application authority.
    pub fn content(&self) -> capnp::Result<capnp::any_pointer::Reader<'m>> {
        Ok(self.call.get_params()?.get_content())
    }
    pub fn capabilities(&self) -> &'r [Option<clients::Client<'id>>] {
        &self.capabilities[..self.count]
    }
    pub fn hints(&self) -> calls::DynamicCall {
        calls::DynamicCall {
            interface: self.interface_id(),
            method: self.method_id(),
            no_promise_pipelining: self.call.get_no_promise_pipelining(),
            only_promise_pipeline: self.call.get_only_promise_pipeline(),
        }
    }
    pub fn context_service_info<C>(
        &self,
        service: &impl capnp::wire_capability::ContextService<C>,
    ) -> capnp::Result<capnp::wire_capability::MethodInfo> {
        service
            .method_info(self.interface_id(), self.method_id())
            .ok_or_else(|| capnp::Error::from_kind(capnp::ErrorKind::Unimplemented))
    }
    /// Execute a generated local service against caller-owned result content.
    /// Parameter capabilities were validated against the retained graph table
    /// at enqueue, so no placeholder wire descriptor table is consulted here.
    pub async fn serve_with_context<'call, C>(
        &'call self,
        service: &'call mut impl capnp::wire_capability::ContextService<C>,
        results: capnp::field_api::FixedField<
            'call,
            capnp::field_api::Generic<capnp::any_pointer::Owned>,
        >,
        context: &'call mut C,
    ) -> capnp::Result<()>
    where
        'm: 'call,
    {
        self.context_service_info(service)?;
        service
            .dispatch_with_context(
                self.interface_id(),
                self.method_id(),
                self.call.get_params()?.get_content(),
                results,
                context,
            )
            .await
    }
    pub fn interface_id(&self) -> u64 {
        self.call.get_interface_id()
    }
    pub fn method_id(&self) -> u16 {
        self.call.get_method_id()
    }
    pub fn params<I: Interface, P: Schema, R: Schema>(
        &self,
        method: Method<I, P, R>,
    ) -> capnp::Result<P::Ref<'m>> {
        if method.interface_id() != self.interface_id() || method.ordinal() != self.method_id() {
            return Err(capnp::Error::from_kind(capnp::ErrorKind::Unimplemented));
        }
        method.read_params(self.call.get_params()?.get_content())
    }
    pub fn capability<I>(
        &self,
        pointer: capnp::wire_capability::Reader<'_, I>,
    ) -> Result<&'r clients::Client<'id>, super::super::Error> {
        if !pointer.belongs_to(self.call.get_params()?.get_content()) {
            return Err(super::super::Error::WrongMessage);
        }
        usize::try_from(pointer.index())
            .ok()
            .filter(|index| *index < self.count)
            .and_then(|index| self.capabilities[index].as_ref())
            .ok_or(super::super::Error::Capacity)
    }
    pub fn streaming_params<I: Interface, P: Schema>(
        &self,
        method: StreamingMethod<I, P>,
    ) -> capnp::Result<P::Ref<'m>> {
        if method.interface_id() != self.interface_id() || method.ordinal() != self.method_id() {
            return Err(capnp::Error::from_kind(capnp::ErrorKind::Unimplemented));
        }
        method.read_params(self.call.get_params()?.get_content())
    }
}
impl<'pool, 'id, const CAPS: usize> Invocation<'pool, 'id, CAPS> {
    pub fn completion_owner(&self) -> Option<usize> {
        self.completion_owner.map(|owner| owner.get() - 1)
    }
    pub(super) fn allows_pipeline(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
    ) -> Result<bool, super::super::Error> {
        self.with(messages, |params| !params.call.get_no_promise_pipelining())
    }
    pub fn with<R>(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
        f: impl FnOnce(InvocationParams<'_, '_, 'id, CAPS>) -> R,
    ) -> Result<R, super::super::Error> {
        self.frame.with(messages, |root| {
            let rpc_capnp::message::Call(call) = root
                .which()
                .map_err(|_| super::super::Error::WrongMessage)?
            else {
                return Err(super::super::Error::WrongMessage);
            };
            Ok(f(InvocationParams {
                call: call?,
                capabilities: &self.capabilities,
                count: self.count,
            }))
        })?
    }
    pub fn release<G: clients::Capabilities<'id>>(
        self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        graph: &mut G,
    ) {
        self.frame.release(messages);
        for client in self.capabilities.into_iter().flatten() {
            graph.release(client)
        }
    }

    /// Borrow parameters across suspension without borrowing the driver's pool.
    /// The call retains this immutable frame and its capability references;
    /// independent message buffers and graph operations remain available.
    pub fn with_async<'call, 'storage: 'call, R, F>(
        &'call self,
        messages: &messages::Messages<'pool, 'storage, '_>,
        f: F,
    ) -> impl core::future::Future<Output = Result<R, super::super::Error>>
           + 'call
           + use<'call, 'pool, 'id, R, F, CAPS>
    where
        F: for<'m> core::ops::AsyncFnOnce(InvocationParams<'m, 'call, 'id, CAPS>) -> R + 'call,
    {
        let view = messages.read(&self.frame.lease);
        async move {
            let mut bytes = &*view;
            let reader = capnp::serialize::read_message_from_flat_slice_no_alloc(
                &mut bytes,
                self.frame.options,
            )?;
            if !bytes.is_empty() {
                return Err(super::super::Error::TrailingBytes);
            }
            let root = reader.get_root::<rpc_capnp::message::Reader<'_>>()?;
            let rpc_capnp::message::Call(call) = root
                .which()
                .map_err(|_| super::super::Error::WrongMessage)?
            else {
                return Err(super::super::Error::WrongMessage);
            };
            Ok(f(InvocationParams {
                call: call?,
                capabilities: &self.capabilities,
                count: self.count,
            })
            .await)
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SentInvocation<'id> {
    pub task: dispatch::TaskKey<'id>,
    pub question: questions::Key<'id>,
}
impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// Queue a generated method against a capability, including unresolved local
    /// promises. The parameter table is supplied as retained graph clients in
    /// index order. Its reachable pointers are checked before admission. No
    /// question or wire grant exists until `send_invocation` accepts output.
    #[allow(clippy::too_many_arguments)]
    pub fn enqueue_call<InterfaceType: Interface, Params: Schema, Results: Schema>(
        &mut self,
        target: &clients::Client<'id>,
        method: Method<InterfaceType, Params, Results>,
        capabilities: &[&clients::Client<'id>],
        messages: &mut messages::Messages<'pool, '_, '_>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        fill: impl FnOnce(FixedStruct<'_, Params>) -> capnp::Result<()>,
    ) -> Result<dispatch::TaskKey<'id>, Error> {
        let mapped = map_capabilities::<CAPS>(capabilities, self.message_capabilities)?;
        self.enqueue_encoded(
            target,
            &mapped[..capabilities.len()],
            messages,
            minimum,
            options,
            |builder| {
                calls::call_to(
                    builder,
                    capntproto_rpc_engine::ids::QuestionId::from_wire(0),
                    calls::Target::Imported(ImportId::from_wire(0)),
                    method,
                    fill,
                )
            },
        )
    }
    /// Streaming calls use the same ordering and receipt ownership as ordinary
    /// calls, but explicitly disable promise pipelining and have only an ack.
    #[allow(clippy::too_many_arguments)]
    pub fn enqueue_streaming<InterfaceType: Interface, Params: Schema>(
        &mut self,
        target: &clients::Client<'id>,
        method: StreamingMethod<InterfaceType, Params>,
        capabilities: &[&clients::Client<'id>],
        messages: &mut messages::Messages<'pool, '_, '_>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        fill: impl FnOnce(FixedStruct<'_, Params>) -> capnp::Result<()>,
    ) -> Result<dispatch::TaskKey<'id>, Error> {
        let mapped = map_capabilities::<CAPS>(capabilities, self.message_capabilities)?;
        self.enqueue_encoded(
            target,
            &mapped[..capabilities.len()],
            messages,
            minimum,
            options,
            |builder| {
                calls::streaming_call(
                    builder,
                    capntproto_rpc_engine::ids::QuestionId::from_wire(0),
                    calls::Target::Imported(ImportId::from_wire(0)),
                    method,
                    fill,
                )
            },
        )
    }
    fn enqueue_encoded(
        &mut self,
        target: &clients::Client<'id>,
        capabilities: &[Option<&clients::Client<'id>>],
        messages: &mut messages::Messages<'pool, '_, '_>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        encode: impl FnOnce(&mut capnp::message::FixedBuilder<'_>) -> capnp::Result<()>,
    ) -> Result<dispatch::TaskKey<'id>, Error> {
        self.check_live()?;
        // Deferred input was already admitted before this application call.
        // Give it a dispatch position before admitting newer ordinary work.
        if self.call.is_some() || self.echo.is_some() {
            return Err(Error::Busy);
        }
        if !self.normal_dispatch_available() {
            return Err(Error::Dispatch(dispatch::DispatchError::Full));
        }
        let retained = self
            .message_capabilities
            .buffer::<_, CAPS>(capabilities.len(), || None)
            .ok_or(Error::Input(super::super::Error::Capacity))?;
        let count = u32::try_from(capabilities.len())
            .map_err(|_| Error::Input(super::super::Error::Capacity))?;
        let frame = Frame::build(messages, minimum, options, encode).map_err(Error::Input)?;
        let validation = frame
            .with(messages, |root| -> capnp::Result<Option<PlainTemplate>> {
                let rpc_capnp::message::Call(call) = root.which()? else {
                    unreachable!()
                };
                let call = call?;
                call.get_params()?
                    .get_content()
                    .validate_capability_indices(count)?;
                Ok(if count == 0 {
                    PlainTemplate::new(call, &messages.read(&frame.lease))
                } else {
                    None
                })
            })
            .and_then(|result| result.map_err(super::super::Error::from));
        let template = match validation {
            Ok(template) => template,
            Err(error) => {
                frame.release(messages);
                return Err(Error::Input(error));
            }
        };
        let mut invocation = Invocation {
            frame,
            template,
            capabilities: retained,
            count: capabilities.len(),
            completion_owner: None,
        };
        for (index, client) in capabilities.iter().enumerate() {
            let Some(client) = client else { continue };
            match self.runtime.retain(client) {
                Ok(client) => invocation.capabilities[index] = Some(client),
                Err(error) => {
                    invocation.release(messages, &mut self.runtime);
                    return Err(Error::Graph(error));
                }
            }
        }
        let mut owner = Some(Task::Invoke(invocation));
        match self
            .runtime
            .prepare_call(target, &mut owner)
            .map(|pending| pending.accept())
        {
            Ok(key) => Ok(key),
            Err(error) => {
                let Some(Task::Invoke(invocation)) = owner else {
                    unreachable!()
                };
                invocation.release(messages, &mut self.runtime);
                Err(Error::Dispatch(error))
            }
        }
    }

    /// Queue an owned/generated ClientHook call through the same transactional
    /// admission as typed methods. Failed encoding or admission retains no new
    /// graph references, question IDs, descriptor grants, or output entries.
    #[allow(clippy::too_many_arguments)]
    pub fn enqueue_dynamic(
        &mut self,
        target: &clients::Client<'id>,
        method: calls::DynamicCall,
        capabilities: &[Option<&clients::Client<'id>>],
        messages: &mut messages::Messages<'pool, '_, '_>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        fill: impl FnOnce(
            capnp::field_api::FixedField<'_, capnp::field_api::Generic<capnp::any_pointer::Owned>>,
        ) -> capnp::Result<()>,
    ) -> Result<dispatch::TaskKey<'id>, Error> {
        self.enqueue_dynamic_to(
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

    /// Queue a dynamic invocation with a caller-authenticated third-party result
    /// destination. Metadata is copied into the immutable template before any
    /// capability or question ownership changes, and survives output retries.
    #[allow(clippy::too_many_arguments)]
    pub fn enqueue_dynamic_to(
        &mut self,
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
    ) -> Result<dispatch::TaskKey<'id>, Error> {
        if let Some(destination) = destination {
            destination
                .validate_capability_indices(0)
                .map_err(|error| Error::Input(error.into()))?;
        }
        self.enqueue_encoded(
            target,
            capabilities,
            messages,
            minimum,
            options,
            |builder| {
                calls::dynamic_call(
                    builder,
                    capntproto_rpc_engine::ids::QuestionId::from_wire(0),
                    calls::Target::Imported(ImportId::from_wire(0)),
                    method,
                    fill,
                )?;
                if let Some(destination) = destination {
                    crate::network::send_results_to_third_party(builder, |field| {
                        field.copy_from(destination)
                    })?;
                }
                Ok(())
            },
        )
    }
    /// Bind a hosted completion before publication, preserving its identity
    /// when dispatch later assigns a wire question.
    pub fn bind_invocation_completion(
        &mut self,
        task: dispatch::TaskKey<'id>,
        owner: usize,
    ) -> Result<(), Error> {
        let Some(Task::Invoke(invocation)) = self.runtime.queued_owner_mut(task) else {
            return Err(Error::UnknownInvocation);
        };
        if invocation.completion_owner.is_some() {
            return Err(Error::Question(questions::Error::AlreadyOwned));
        }
        invocation.completion_owner = Some(
            core::num::NonZeroUsize::new(owner.checked_add(1).ok_or(Error::Busy)?)
                .ok_or(Error::Busy)?,
        );
        Ok(())
    }
    /// Cancel only a queued outgoing invocation. It owes no Finish or peer
    /// Release because neither a question nor parameter grant was published.
    pub fn cancel_invocation(
        &mut self,
        task: dispatch::TaskKey<'id>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> bool {
        if !(0..self.runtime.task_capacity()).any(|index| {
            self.runtime
                .queued_at(index)
                .is_some_and(|(key, owner)| key == task && matches!(owner, Task::Invoke(_)))
        }) {
            return false;
        }
        let Task::Invoke(invocation) = self.runtime.cancel(task).unwrap_or_else(|_| unreachable!())
        else {
            unreachable!()
        };
        invocation.release(messages, &mut self.runtime);
        self.invocation_pipelines.canceled(task);
        true
    }
    /// Send the oldest ready task if it is an outgoing call to this peer. Local
    /// tasks and protocol controls remain available to their respective caller-
    /// polled handlers. Rejected output keeps the invocation in sequence.
    /// Consume the returned question key for response/pipeline ownership.
    pub fn send_invocation(
        &mut self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        scratch: &mut [u32],
    ) -> Result<Option<SentInvocation<'id>>, Error> {
        self.send_invocation_using(
            messages,
            queue,
            minimum,
            options,
            scratch,
            Default::default(),
            |_, _, _| u8::MAX,
        )
    }
    /// Attach resources from the exact routed capability owner. Local callbacks
    /// must consult the original server hook, preserving policy membranes. The
    /// bundle is reserved before grants commit; rejection leaves the invocation
    /// queued and drops only these tentative transport references.
    #[allow(clippy::too_many_arguments)]
    pub fn send_invocation_with_attachments<R, const N: usize>(
        &mut self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, crate::attachments::Sender<R, N>>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        scratch: &mut [u32],
        capacity: Capacity,
        mut imported: impl FnMut(&I) -> Option<R>,
        mut local: impl FnMut(&S) -> Option<R>,
    ) -> Result<Option<SentInvocation<'id>>, Error> {
        self.send_invocation_using(
            messages,
            queue,
            minimum,
            options,
            scratch,
            crate::attachments::Sender::new(capacity),
            |owner, imports, resources| {
                resources.push(crate::attachments::resource(
                    owner,
                    imports,
                    &mut imported,
                    &mut local,
                ))
            },
        )
    }
    #[allow(clippy::too_many_arguments)]
    /// Publish with a caller-defined resource bundle. Attachment indices must
    /// describe this bundle; use 255 when the transport omits a resource.
    pub fn send_invocation_using<R>(
        &mut self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, R>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        scratch: &mut [u32],
        resources: R,
        mut attach: impl FnMut(
            &binding::Route<'id, P, S, PATH>,
            &imports::Imports<'id, '_, I>,
            &mut R,
        ) -> u8,
    ) -> Result<Option<SentInvocation<'id>>, Error> {
        self.check_live()?;
        let Some(ready) = self.runtime.next_ready().map_err(Error::Dispatch)? else {
            return Ok(None);
        };
        let Task::Invoke(invocation) = ready.owner() else {
            return Ok(None);
        };
        let clients::Destination::Endpoint {
            owner, location, ..
        } = ready.destination()
        else {
            return Ok(None);
        };
        let clients::Location::Remote(peer) = location else {
            return Ok(None);
        };
        if peer != self.imports.peer() {
            return Err(Error::InvalidInvocationTarget);
        }
        let target = owner
            .target(&self.ledgers.questions)
            .map_err(Error::Question)?
            .ok_or(Error::InvalidInvocationTarget)?;
        // Own the path across the graph borrow needed to stage parameter exports.
        let mut fields = Buffer::<u16, PATH>::Inline([0; PATH]);
        let (import, question, length) = match target {
            calls::Target::Imported(id) => (Some(id), None, 0),
            calls::Target::Promised { question, path } => {
                let source = path.fields();
                fields = self
                    .message_path
                    .copy(source)
                    .ok_or(Error::Input(super::super::Error::Capacity))?;
                (None, Some(question), source.len())
            }
        };
        let task = ready.key();
        let completion_owner = invocation.completion_owner;
        let count = invocation.count;
        let template = invocation.template.filter(|_| import.is_some());
        if let Some(template) = template {
            let slot_index = self
                .exchanges
                .vacant(|_, slot| slot.exchange.is_none(), || ExchangeSlot::EMPTY)
                .ok_or(Error::Busy)?;
            let slot = &mut self.exchanges[slot_index];
            let output = queue
                .reserve_with(resources)
                .map_err(|error| Error::Input(super::super::Error::Storage(error)))?;
            let reservation = self.ledgers.questions.reserve().map_err(Error::Question)?;
            let frame = template
                .copy(
                    &invocation.frame,
                    messages,
                    reservation.id(),
                    import.unwrap(),
                    self.allow_answer_handoff,
                    options,
                )
                .map_err(Error::Input)?;
            // The source was fully validated before this unforgeable private
            // template was cached. Copying changes only this reservation's ID,
            // the resolved imported target and advisory bits. Stricter or
            // different reader limits still require full publication checks.
            let source_options = invocation.frame.options;
            let question = if options.traversal_limit_in_words
                == source_options.traversal_limit_in_words
                && options.nesting_limit == source_options.nesting_limit
            {
                Question::publish_template(
                    reservation,
                    messages,
                    frame,
                    output,
                    template.method,
                    self.allow_answer_handoff,
                    self.owned_output,
                )
                .map_err(Error::Input)?
            } else {
                Question::send_granting(
                    reservation,
                    self.ledgers.exports.batch(),
                    messages,
                    frame,
                    scratch,
                    output,
                )
                .map_err(|error| Error::Input(super::super::Error::Output(error)))?
            };
            let key = question.key();
            self.ledgers
                .questions
                .bind_owner(question.key(), slot_index)
                .expect("new exchange owner");
            slot.completion_owner = completion_owner;
            slot.exchange = Some(Exchange::Pending(question));
            slot.canceled = false;
            slot.tail_taken = false;
            slot.authorized = true;
            self.invocation_pipelines
                .sent(task, key, &mut self.ledgers.questions);
            let Task::Invoke(invocation) =
                self.runtime.cancel(task).unwrap_or_else(|_| unreachable!())
            else {
                unreachable!()
            };
            invocation.release(messages, &mut self.runtime);
            return Ok(Some(SentInvocation {
                task,
                question: key,
            }));
        }
        let third_party = invocation
            .frame
            .with(messages, |source| {
                let rpc_capnp::message::Call(source) = source.which()? else {
                    unreachable!()
                };
                Ok::<_, capnp::Error>(matches!(
                    source?.get_send_results_to().which()?,
                    rpc_capnp::call::send_results_to::ThirdParty(_)
                ))
            })
            .map_err(Error::Input)?
            .map_err(|error| Error::Input(error.into()))?;
        // A queued template has no descriptor table or promised-answer path.
        // Size its final contiguous frame before reserving output. This works
        // for fixed providers too: insufficient buffers reject without consuming
        // the invocation, rather than relying on a hosted retry size guess.
        let sized = invocation
            .frame
            .with(messages, |source| -> capnp::Result<usize> {
                use crate::rpc_capnp::api::{promised_answer::Op, PromisedAnswer};
                let words = |size: capnp::private::layout::StructSize| {
                    u64::from(size.data) + u64::from(size.pointers)
                };
                let path_words = (length as u64).checked_mul(words(Op::SIZE));
                let descriptors = (count as u64).checked_mul(words(CapDescriptor::SIZE));
                source
                    .total_size()?
                    .word_count
                    // Third-party metadata has its own transactional copy
                    // staging pointer, separate from the parameter payload.
                    .checked_add(5 + u64::from(third_party) + words(PromisedAnswer::SIZE))
                    .and_then(|n| n.checked_add(path_words?))
                    .and_then(|n| n.checked_add(descriptors?))
                    .and_then(|n| n.checked_mul(8))
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(|| capnp::Error::from_kind(capnp::ErrorKind::MessageSizeOverflow))
            })
            .map_err(Error::Input)?
            .map_err(|error| Error::Input(error.into()))?;
        let mut minimum = minimum.max(sized);
        let mut keys = self
            .message_capabilities
            .buffer::<_, CAPS>(count, || None)
            .ok_or(Error::Input(super::super::Error::Capacity))?;
        for (key, client) in keys.iter_mut().zip(invocation.capabilities.iter()) {
            *key = client.as_ref().map(clients::Client::key);
        }
        drop(ready);
        let slot_index = self
            .exchanges
            .vacant(|_, slot| slot.exchange.is_none(), || ExchangeSlot::EMPTY)
            .ok_or(Error::Busy)?;
        let slot = &mut self.exchanges[slot_index];
        let mut output = queue
            .reserve_with(resources)
            .map_err(|error| Error::Input(super::super::Error::Storage(error)))?;
        let reservation = if third_party {
            self.ledgers.questions.reserve_third_party()
        } else {
            self.ledgers.questions.reserve()
        }
        .map_err(Error::Question)?;
        let mut batch = self.ledgers.exports.batch();
        let mut grants = self
            .message_capabilities
            .buffer::<_, CAPS>(count, || None)
            .ok_or(Error::Input(super::super::Error::Capacity))?;
        for index in 0..count {
            let Some(key) = keys[index] else { continue };
            let client = self.runtime.retain_key(key).map_err(Error::Graph)?;
            let staged = self.exported.stage(&mut self.runtime, &mut batch, &client);
            let attached = match self.runtime.inspect(&client) {
                Ok(clients::Destination::Endpoint { owner, .. }) if staged.is_ok() => {
                    attach(owner, &self.ledgers.imports, output.resources_mut())
                }
                _ => u8::MAX,
            };
            self.runtime.release(client);
            let grant = staged.map_err(Error::Exported)?;
            grants[index] = Some((grant.id(), grant.is_promise(), attached));
            let owner = batch.pending_owner(grant.id()).and_then(|client| {
                match self.runtime.inspect(client) {
                    Ok(clients::Destination::Endpoint { owner, .. }) => Some(owner),
                    _ => None,
                }
            });
            minimum = minimum
                .checked_add(
                    self.descriptors
                        .bytes(owner, grant.is_promise())
                        .map_err(|e| Error::Input(e.into()))?,
                )
                .ok_or(Error::Input(super::super::Error::Wire(
                    capnp::ErrorKind::MessageSizeOverflow,
                )))?;
        }
        let (_, owner) = (0..self.runtime.task_capacity())
            .find_map(|index| {
                self.runtime
                    .queued_at(index)
                    .filter(|(key, _)| *key == task)
            })
            .unwrap();
        let Task::Invoke(invocation) = owner else {
            unreachable!()
        };
        let target = match import {
            Some(id) => calls::Target::Imported(id),
            None => calls::Target::Promised {
                question: question.unwrap(),
                path: pipeline::Path::new(&fields[..length]),
            },
        };
        let frame = invocation
            .frame
            .build_from(messages, minimum, options, |source, builder| {
                let rpc_capnp::message::Call(source) = source.which()? else {
                    unreachable!()
                };
                let source = source?;
                let root = builder.try_init_root::<Message>()?;
                let mut call = root.into_field(Message::CALL).init()?;
                call.scalar(WireCall::QUESTION_ID)
                    .set(reservation.id().to_wire());
                call.scalar(WireCall::INTERFACE_ID)
                    .set(source.get_interface_id());
                call.scalar(WireCall::METHOD_ID).set(source.get_method_id());
                call.scalar(WireCall::NO_PROMISE_PIPELINING)
                    .set(source.get_no_promise_pipelining());
                // Pipeline-only interest is retained by the caller's completion
                // adapter. Use an ordinary question until the question ledger
                // supports high-ID calls without Return obligations. The hint
                // is advisory; emitting it on an ordinary question would leave
                // that ledger waiting forever for a Return the peer may omit.
                call.scalar(WireCall::ONLY_PROMISE_PIPELINE).set(false);
                call.scalar(WireCall::ALLOW_THIRD_PARTY_TAIL_CALL)
                    .set(self.allow_answer_handoff && !third_party);
                if let rpc_capnp::call::send_results_to::ThirdParty(destination) =
                    source.get_send_results_to().which()?
                {
                    use crate::rpc_capnp::api::call::SendResultsTo;
                    call.group(WireCall::SEND_RESULTS_TO)
                        .select()
                        .field(SendResultsTo::THIRD_PARTY)
                        .copy_from(destination)?;
                }
                calls::write_target(call.field(WireCall::TARGET).init()?, target)?;
                let mut payload = call.field(WireCall::PARAMS).init()?;
                let mut descriptors = payload.field(WirePayload::CAP_TABLE).init(count)?;
                for (index, grant) in grants[..count].iter().enumerate() {
                    let mut descriptor = descriptors.get_mut(index).unwrap();
                    let Some((id, promise, attached)) = *grant else {
                        descriptor.void(CapDescriptor::NONE).set();
                        continue;
                    };
                    descriptor.scalar(CapDescriptor::ATTACHED_FD).set(attached);
                    let owner = batch.pending_owner(id).and_then(|client| {
                        match self.runtime.inspect(client) {
                            Ok(clients::Destination::Endpoint { owner, .. }) => Some(owner),
                            _ => None,
                        }
                    });
                    self.descriptors.describe(owner, id, promise, descriptor)?;
                }
                let mapping = capnp::wire_capability::IndexMap::identity(count as u32);
                payload
                    .field(WirePayload::CONTENT)
                    .copy_with_capabilities(source.get_params()?.get_content(), mapping)
            })
            .map_err(Error::Input)?;
        let question =
            Question::send_granting(reservation, batch, messages, frame, scratch, output)
                .map_err(|error| Error::Input(super::super::Error::Output(error)))?;
        let key = question.key();
        self.ledgers
            .questions
            .bind_owner(question.key(), slot_index)
            .expect("new exchange owner");
        slot.completion_owner = completion_owner;
        slot.exchange = Some(Exchange::Pending(question));
        slot.canceled = false;
        slot.tail_taken = false;
        slot.authorized = true;
        self.invocation_pipelines
            .sent(task, key, &mut self.ledgers.questions);
        // No callback or graph resolution occurs between selecting this task
        // and FIFO acceptance. Release its route only after output owns the Call.
        let Task::Invoke(invocation) = self.runtime.cancel(task).unwrap_or_else(|_| unreachable!())
        else {
            unreachable!()
        };
        invocation.release(messages, &mut self.runtime);
        Ok(Some(SentInvocation {
            task,
            question: key,
        }))
    }
}
