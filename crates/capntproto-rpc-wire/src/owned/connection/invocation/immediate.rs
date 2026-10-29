//! Publish routable calls without a temporary queued Call envelope.
use super::*;
use core::borrow::Borrow;

/// Parameters for immediate publication. An encoded Call has one or two unframed
/// segments. Checked writers validate its payload before copying. Its method
/// and hints must match; target/question IDs are assigned by the driver.
pub enum ImmediateParams<'a> {
    Content(capnp::any_pointer::Reader<'a>),
    EncodedCall(EncodedCall<'a>),
}

/// Borrowed parameter authority in payload index order. The table owns or
/// borrows each client through publication; reading an entry must preserve its
/// identity. None denotes a null slot. Lookup failures roll back publication.
/// Implementations need no allocation or additional graph references.
pub trait CapabilityTable<'id> {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn capability(&self, index: usize) -> capnp::Result<Option<&clients::Client<'id>>>;
}
impl<'id, C: Borrow<clients::Client<'id>>> CapabilityTable<'id> for [Option<C>] {
    fn len(&self) -> usize {
        self.len()
    }
    fn capability(&self, index: usize) -> capnp::Result<Option<&clients::Client<'id>>> {
        self.get(index)
            .map(|entry| entry.as_ref().map(Borrow::borrow))
            .ok_or_else(|| capnp::Error::from_kind(capnp::ErrorKind::Failed))
    }
}

impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// Publish a settled remote call directly from its retained capability table.
    /// Null slots and duplicate entries preserve their indices. Caller ownership
    /// protects every capability through publication; output rejection rolls back
    /// all grants without consuming a question or dispatch sequence. Older queued
    /// work and unsettled destinations return `None` for ordinary queued dispatch.
    #[allow(clippy::too_many_arguments)]
    pub fn try_send_immediate_with_capabilities<R, C: Borrow<clients::Client<'id>>>(
        &mut self,
        target: &clients::Client<'id>,
        method: calls::DynamicCall,
        params: capnp::any_pointer::Reader<'_>,
        capabilities: &[Option<C>],
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
    ) -> Result<Option<questions::Key<'id>>, Error> {
        self.try_send_immediate_with_capability_table(
            target,
            method,
            params,
            capabilities,
            messages,
            queue,
            minimum,
            options,
            scratch,
            resources,
            attach,
        )
    }

    /// Publish from existing caller-owned capability storage. Unlike building a
    /// temporary retained table, lookups borrow their clients only while needed.
    /// Transactional grants, reflected ordering and queued fallback are the same
    /// as for `try_send_immediate_with_capabilities`.
    #[allow(clippy::too_many_arguments)]
    pub fn try_send_immediate_with_capability_table<R, T: CapabilityTable<'id> + ?Sized>(
        &mut self,
        target: &clients::Client<'id>,
        method: calls::DynamicCall,
        params: capnp::any_pointer::Reader<'_>,
        capabilities: &T,
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
    ) -> Result<Option<questions::Key<'id>>, Error> {
        if capabilities.len() == 1 {
            if let Some(capability) = capabilities
                .capability(0)
                .map_err(|e| Error::Input(e.into()))?
            {
                return self.try_send_immediate_single(
                    target, method, params, capability, messages, queue, minimum, options, scratch,
                    resources, attach,
                );
            }
        }
        if capabilities.is_empty() {
            return self.try_send_immediate(
                target, method, params, messages, queue, minimum, options, resources,
            );
        }
        self.check_live()?;
        if method.only_promise_pipeline || self.call.is_some() || self.echo.is_some() {
            return Ok(None);
        }
        let remote = {
            let Some(ticket) = self
                .runtime
                .prepare_immediate(target)
                .map_err(Error::Dispatch)?
            else {
                return Ok(None);
            };
            let clients::Destination::Endpoint {
                owner: binding::Route::Capability(binding::Endpoint::Imported(remote)),
                location: clients::Location::Remote(peer),
                ..
            } = ticket.destination()
            else {
                return Ok(None);
            };
            if peer != self.imports.peer() {
                return Err(Error::InvalidInvocationTarget);
            }
            remote.id()
        };
        let count = capabilities.len();
        if count > self.capability_capacity() {
            return Err(Error::Input(super::super::super::Error::Capacity));
        }
        let wire_count =
            u32::try_from(count).map_err(|_| Error::Input(super::super::super::Error::Capacity))?;
        let options = self.writer_policy.options(options);
        if self.writer_policy == WriterPolicy::Checked {
            params
                .validate_capability_indices(wire_count)
                .map_err(|e| Error::Input(e.into()))?;
        }
        let words = params
            .target_size()
            .map_err(|e| Error::Input(e.into()))?
            .word_count;
        let descriptor_words = (count as u64).checked_mul(
            u64::from(CapDescriptor::SIZE.data) + u64::from(CapDescriptor::SIZE.pointers),
        );
        let sized = words
            .checked_add(16)
            .and_then(|words| words.checked_add(descriptor_words?))
            .and_then(|words| words.checked_mul(8))
            .and_then(|bytes| usize::try_from(bytes).ok())
            .ok_or(Error::Input(super::super::super::Error::Capacity))?;
        let mut minimum = minimum.max(sized);
        let slot_index =
            ExchangeSlot::vacant(&mut self.exchanges, &self.free_exchanges).ok_or(Error::Busy)?;
        let slot = &mut self.exchanges[slot_index];
        let mut output = queue
            .reserve_with(resources)
            .map_err(|e| Error::Input(super::super::super::Error::Storage(e)))?;
        let mut batch = self.ledgers.exports.batch();
        // Imported endpoints own immutable IDs, as on the single-capability
        // path. Copy those once. Promised transforms borrow graph storage, so
        // select them only through the final exclusive ticket after staging.
        let mut staged = self
            .message_capabilities
            .buffer::<_, CAPS>(count, || reflection::Descriptor::None)
            .ok_or(Error::Input(super::super::super::Error::Capacity))?;
        let mut exports = crate::inbound::full::ParameterExports::single(None);
        for index in 0..count {
            let Some(capability) = capabilities
                .capability(index)
                .map_err(|e| Error::Input(e.into()))?
            else {
                continue;
            };
            if let clients::Destination::Endpoint { owner, .. } =
                self.runtime.inspect(capability).map_err(Error::Graph)?
            {
                if let Some(target) =
                    reflection::target(owner, self.imports.peer(), &self.ledgers.questions)
                        .map_err(Error::Question)?
                {
                    if let calls::Target::Imported(id) = target {
                        staged[index] =
                            reflection::Descriptor::Reflected(calls::Target::Imported(id));
                        continue;
                    }
                    let extra = match target {
                        calls::Target::Imported(_) => Some(0),
                        calls::Target::Promised { path, .. } => path
                            .fields()
                            .len()
                            .checked_mul(8)
                            .and_then(|n| n.checked_add(32)),
                    };
                    minimum = extra
                        .and_then(|extra| minimum.checked_add(extra))
                        .ok_or(Error::Input(super::super::super::Error::Capacity))?;
                    continue;
                }
            }
            let grant = self
                .exported
                .stage_using(
                    &mut self.runtime,
                    &mut batch,
                    capability,
                    self.descriptors.identity,
                )
                .map_err(Error::Exported)?;
            let attached = match self.runtime.inspect(capability) {
                Ok(clients::Destination::Endpoint { owner, .. }) => {
                    attach(owner, &self.ledgers.imports, output.resources_mut())
                }
                _ => u8::MAX,
            };
            exports
                .push(grant.id(), self.message_capabilities, count)
                .ok_or(Error::Input(super::super::super::Error::Capacity))?;
            staged[index] = reflection::Descriptor::Grant(grant.id(), grant.is_promise(), attached);
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
                .ok_or(Error::Input(super::super::super::Error::Capacity))?;
        }
        let ticket = match self.runtime.prepare_immediate(target) {
            Ok(Some(ticket)) => ticket,
            Ok(None) => return Ok(None),
            Err(error) => return Err(Error::Dispatch(error)),
        };
        if !matches!(ticket.destination(), clients::Destination::Endpoint {
            owner: binding::Route::Capability(binding::Endpoint::Imported(import)),
            location: clients::Location::Remote(peer), ..
        } if import.id() == remote && peer == self.imports.peer())
        {
            return Ok(None);
        }
        for (index, descriptor) in staged[..count].iter_mut().enumerate() {
            if matches!(
                descriptor,
                reflection::Descriptor::Grant(..) | reflection::Descriptor::Reflected(..)
            ) {
                continue;
            }
            let Some(capability) = capabilities
                .capability(index)
                .map_err(|e| Error::Input(e.into()))?
            else {
                continue;
            };
            let clients::Destination::Endpoint { owner, .. } =
                ticket.inspect(capability).map_err(Error::Graph)?
            else {
                return Err(Error::InvalidInvocationTarget);
            };
            let reflected = reflection::target(owner, self.imports.peer(), &self.ledgers.questions)
                .map_err(Error::Question)?
                .ok_or(Error::InvalidInvocationTarget)?;
            *descriptor = reflection::Descriptor::Reflected(reflected);
        }
        let reservation = self.ledgers.questions.reserve().map_err(Error::Question)?;
        let frame = Frame::build(messages, minimum, options, |builder| {
            calls::dynamic_call_payload(
                builder,
                reservation.id(),
                calls::Target::Imported(remote),
                method,
                self.allow_answer_handoff,
                |mut payload| {
                    payload.field(WirePayload::CONTENT).copy_with_capabilities(
                        params,
                        capnp::wire_capability::IndexMap::identity(wire_count),
                    )?;
                    let mut table = payload.field(WirePayload::CAP_TABLE).init(count)?;
                    for (index, grant) in staged[..count].iter().enumerate() {
                        let mut descriptor = table.get_mut(index).unwrap();
                        if let reflection::Descriptor::Grant(id, promise, attached) = *grant {
                            descriptor.scalar(CapDescriptor::ATTACHED_FD).set(attached);
                            let owner = batch.pending_owner(id).and_then(|client| {
                                match ticket.inspect(client) {
                                    Ok(clients::Destination::Endpoint { owner, .. }) => Some(owner),
                                    _ => None,
                                }
                            });
                            self.descriptors.describe(owner, id, promise, descriptor)?;
                        } else if let Some(target) = grant.reflected() {
                            reflection::write(target, descriptor)?;
                        } else {
                            descriptor.void(CapDescriptor::NONE).set();
                        }
                    }
                    Ok(())
                },
            )
        })
        .map_err(Error::Input)?;
        let question = Question::send_granting_compact(
            reservation,
            batch,
            messages,
            frame,
            scratch,
            output,
            crate::inbound::full::OriginalMetadata::call(
                (method.interface, method.method),
                false,
                self.allow_answer_handoff,
                exports,
            ),
            self.writer_policy == WriterPolicy::Checked,
            |descriptor, index| {
                reflection::check(
                    staged
                        .get(index)
                        .and_then(reflection::Descriptor::reflected),
                    descriptor,
                )
            },
        )
        .map_err(|e| Error::Input(super::super::super::Error::Output(e)))?;
        let key = question.key();
        self.ledgers
            .questions
            .bind_owner(key, slot_index)
            .expect("new exchange owner");
        slot.completion_owner = None;
        let reserved = self
            .free_exchanges
            .take(&mut self.exchanges, |slot| &mut slot.free);
        debug_assert_eq!(reserved, slot_index);
        let slot = &mut self.exchanges[slot_index];
        slot.exchange = Some(Exchange::Pending(question));
        slot.canceled = false;
        slot.tail_taken = false;
        slot.authorized = true;
        // Growing storage can retain borrowed reflection paths until dropped.
        // Inline-only storage has no destructor and its last use ends the loan.
        #[cfg(feature = "alloc")]
        drop(staged);
        ticket.accept_published();
        Ok(Some(key))
    }

    /// Publish a settled call whose parameter table contains exactly one
    /// capability. The caller retains that capability through this operation;
    /// successful publication transfers an ordinary descriptor grant to the
    /// question. Rejection preserves the request and rolls back the grant.
    /// Pending destinations and older queued work retain the queued path.
    #[allow(clippy::too_many_arguments)]
    fn try_send_immediate_single<R>(
        &mut self,
        target: &clients::Client<'id>,
        method: calls::DynamicCall,
        params: capnp::any_pointer::Reader<'_>,
        capability: &clients::Client<'id>,
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
    ) -> Result<Option<questions::Key<'id>>, Error> {
        self.check_live()?;
        if method.only_promise_pipeline || self.call.is_some() || self.echo.is_some() {
            return Ok(None);
        }
        let remote = {
            let Some(ticket) = self
                .runtime
                .prepare_immediate(target)
                .map_err(Error::Dispatch)?
            else {
                return Ok(None);
            };
            let clients::Destination::Endpoint {
                owner: binding::Route::Capability(binding::Endpoint::Imported(remote)),
                location: clients::Location::Remote(peer),
                ..
            } = ticket.destination()
            else {
                return Ok(None);
            };
            if peer != self.imports.peer() {
                return Err(Error::InvalidInvocationTarget);
            }
            remote.id()
        };
        if self.capability_capacity() == 0 {
            return Err(Error::Input(super::super::super::Error::Capacity));
        }
        let reflected_import = if let clients::Destination::Endpoint { owner, .. } =
            self.runtime.inspect(capability).map_err(Error::Graph)?
        {
            match reflection::target(owner, self.imports.peer(), &self.ledgers.questions)
                .map_err(Error::Question)?
            {
                Some(calls::Target::Imported(id)) => Some(id),
                // Longer transforms keep the general bounded encoder.
                Some(calls::Target::Promised { .. }) => return Ok(None),
                None => None,
            }
        } else {
            None
        };
        let options = self.writer_policy.options(options);
        if self.writer_policy == WriterPolicy::Checked {
            params
                .validate_capability_indices(1)
                .map_err(|error| Error::Input(error.into()))?;
        }
        let words = params
            .target_size()
            .map_err(|error| Error::Input(error.into()))?
            .word_count;
        let sized = words
            .checked_add(
                16 + u64::from(CapDescriptor::SIZE.data) + u64::from(CapDescriptor::SIZE.pointers),
            )
            .and_then(|words| words.checked_mul(8))
            .and_then(|bytes| usize::try_from(bytes).ok())
            .ok_or(Error::Input(super::super::super::Error::Capacity))?;
        let slot_index =
            ExchangeSlot::vacant(&mut self.exchanges, &self.free_exchanges).ok_or(Error::Busy)?;
        let slot = &mut self.exchanges[slot_index];
        let mut output = queue
            .reserve_with(resources)
            .map_err(|error| Error::Input(super::super::super::Error::Storage(error)))?;
        let reservation = self.ledgers.questions.reserve().map_err(Error::Question)?;
        let mut batch = self.ledgers.exports.batch();
        let grant = if reflected_import.is_none() {
            Some(
                self.exported
                    .stage_using(
                        &mut self.runtime,
                        &mut batch,
                        capability,
                        self.descriptors.identity,
                    )
                    .map_err(Error::Exported)?,
            )
        } else {
            None
        };
        let attached = match self.runtime.inspect(capability) {
            Ok(clients::Destination::Endpoint { owner, .. }) if grant.is_some() => {
                attach(owner, &self.ledgers.imports, output.resources_mut())
            }
            _ => u8::MAX,
        };
        let id = grant.as_ref().map(|grant| grant.id());
        let promise = grant.as_ref().is_some_and(|grant| grant.is_promise());
        let owner = id
            .and_then(|id| batch.pending_owner(id))
            .and_then(|client| match self.runtime.inspect(client) {
                Ok(clients::Destination::Endpoint { owner, .. }) => Some(owner),
                _ => None,
            });
        let minimum = minimum
            .max(sized)
            .checked_add(
                self.descriptors
                    .bytes(owner, promise)
                    .map_err(|error| Error::Input(error.into()))?,
            )
            .ok_or(Error::Input(super::super::super::Error::Capacity))?;
        let frame = Frame::build(messages, minimum, options, |builder| {
            calls::dynamic_call_payload(
                builder,
                reservation.id(),
                calls::Target::Imported(remote),
                method,
                self.allow_answer_handoff,
                |mut payload| {
                    payload.field(WirePayload::CONTENT).copy_with_capabilities(
                        params,
                        capnp::wire_capability::IndexMap::identity(1),
                    )?;
                    let mut descriptors = payload.field(WirePayload::CAP_TABLE).init(1)?;
                    let mut descriptor = descriptors.get_mut(0).unwrap();
                    descriptor.scalar(CapDescriptor::ATTACHED_FD).set(attached);
                    match reflected_import {
                        Some(id) => reflection::write(calls::Target::Imported(id), descriptor),
                        None => self.descriptors.describe(
                            owner,
                            id.expect("staged export"),
                            promise,
                            descriptor,
                        ),
                    }
                },
            )
        })
        .map_err(Error::Input)?;
        // Export staging needs the graph, so reacquire its exclusive dispatch
        // ticket before publication. No borrowed ticket crosses a callback.
        let ticket = match self.runtime.prepare_immediate(target) {
            Ok(Some(ticket)) => ticket,
            Ok(None) => {
                frame.release(messages);
                return Ok(None);
            }
            Err(error) => {
                frame.release(messages);
                return Err(Error::Dispatch(error));
            }
        };
        if !matches!(ticket.destination(), clients::Destination::Endpoint {
            owner: binding::Route::Capability(binding::Endpoint::Imported(import)),
            location: clients::Location::Remote(peer), ..
        } if import.id() == remote && peer == self.imports.peer())
        {
            frame.release(messages);
            return Ok(None);
        }
        let question = Question::send_granting_compact(
            reservation,
            batch,
            messages,
            frame,
            scratch,
            output,
            crate::inbound::full::OriginalMetadata::call(
                (method.interface, method.method),
                false,
                self.allow_answer_handoff,
                crate::inbound::full::ParameterExports::single(id),
            ),
            self.writer_policy == WriterPolicy::Checked,
            |descriptor, _| {
                reflection::check(reflected_import.map(calls::Target::Imported), descriptor)
            },
        )
        .map_err(|error| Error::Input(super::super::super::Error::Output(error)))?;
        let key = question.key();
        self.ledgers
            .questions
            .bind_owner(question.key(), slot_index)
            .expect("new exchange owner");
        slot.completion_owner = None;
        let reserved = self
            .free_exchanges
            .take(&mut self.exchanges, |slot| &mut slot.free);
        debug_assert_eq!(reserved, slot_index);
        let slot = &mut self.exchanges[slot_index];
        slot.exchange = Some(Exchange::Pending(question));
        slot.canceled = false;
        slot.tail_taken = false;
        slot.authorized = true;
        ticket.accept_published();
        Ok(Some(key))
    }

    /// Try immediate publication of a routable remote call without parameter
    /// capabilities. `None` leaves the caller's request untouched and
    /// requires ordinary queued invocation. Older tasks, waiting promises and
    /// deferred wire input are never overtaken. An unresolved remote promise
    /// uses its authenticated original route and preserves resolution barriers.
    /// All storage is caller supplied.
    ///
    /// The returned question has the ordinary response/cancel/Finish lifecycle.
    /// Output failure rolls back the reservation without consuming a question,
    /// dispatch sequence, graph reference, or any caller-owned parameter data.
    #[allow(clippy::too_many_arguments)]
    pub fn try_send_immediate<R>(
        &mut self,
        target: &clients::Client<'id>,
        method: calls::DynamicCall,
        params: capnp::any_pointer::Reader<'_>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, R>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        resources: R,
    ) -> Result<Option<questions::Key<'id>>, Error> {
        self.try_send_immediate_from(
            target,
            method,
            ImmediateParams::Content(params),
            messages,
            queue,
            minimum,
            options,
            resources,
        )
    }

    /// The same transactional publication, optionally retaining the application's
    /// already constructed Call envelope. Wrappers should supply Content unless
    /// their complete wire representation preserves their parameter semantics.
    #[allow(clippy::too_many_arguments)]
    pub fn try_send_immediate_from<R>(
        &mut self,
        target: &clients::Client<'id>,
        method: calls::DynamicCall,
        params: ImmediateParams<'_>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, R>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        resources: R,
    ) -> Result<Option<questions::Key<'id>>, Error> {
        self.try_send_immediate_borrowed(
            target, method, &params, messages, queue, minimum, options, resources,
        )
    }
    /// Borrow the prepared envelope through publication, avoiding moves of the
    /// general content-reader variant through hosted scheduling adapters.
    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    pub fn try_send_immediate_borrowed<R>(
        &mut self,
        target: &clients::Client<'id>,
        method: calls::DynamicCall,
        params: &ImmediateParams<'_>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, R>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        resources: R,
    ) -> Result<Option<questions::Key<'id>>, Error> {
        let options = self.writer_policy.options(options);
        self.check_live()?;
        if method.only_promise_pipeline || self.call.is_some() || self.echo.is_some() {
            return Ok(None);
        }
        let Some(ticket) = self
            .runtime
            .prepare_immediate_routed(target)
            .map_err(Error::Dispatch)?
        else {
            return Ok(None);
        };
        let clients::Destination::Endpoint {
            owner,
            location: clients::Location::Remote(peer),
            ..
        } = ticket.destination()
        else {
            return Ok(None);
        };
        if peer != self.imports.peer() {
            return Err(Error::InvalidInvocationTarget);
        }
        let Some(target) = owner
            .target(&self.ledgers.questions)
            .map_err(Error::Question)?
        else {
            return Ok(None);
        };
        let path_words = match target {
            calls::Target::Imported(_) => 0,
            calls::Target::Promised { path, .. } => (path.fields().len() as u64)
                .checked_mul(2)
                .and_then(|words| words.checked_add(2))
                .ok_or(Error::Input(super::super::super::Error::Capacity))?,
        };
        let (template, minimum) = match params {
            ImmediateParams::Content(params) => {
                let size = params.target_size().map_err(|e| Error::Input(e.into()))?;
                if size.cap_count != 0 {
                    return Ok(None);
                }
                let minimum = size
                    .word_count
                    .checked_add(16)
                    .and_then(|words| words.checked_add(path_words))
                    .and_then(|words| words.checked_mul(8))
                    .and_then(|bytes| usize::try_from(bytes).ok())
                    .ok_or(Error::Input(super::super::super::Error::Capacity))?
                    .max(minimum);
                (None, minimum)
            }
            ImmediateParams::EncodedCall(encoded) => {
                let template = encoded
                    .for_publication(method, options, self.writer_policy)
                    .map_err(Error::Input)?;
                (Some(template), minimum)
            }
        };
        let slot_index =
            ExchangeSlot::vacant(&mut self.exchanges, &self.free_exchanges).ok_or(Error::Busy)?;
        let slot = &mut self.exchanges[slot_index];
        let output = queue
            .reserve_with(resources)
            .map_err(|e| Error::Input(super::super::super::Error::Storage(e)))?;
        let reservation = self.ledgers.questions.reserve().map_err(Error::Question)?;
        #[cfg(feature = "alloc")]
        if self.owned_output {
            if let (ImmediateParams::EncodedCall(encoded), calls::Target::Imported(target)) =
                (params, target)
            {
                if let Some(parts) = encoded
                    .retain_parts(
                        template.expect("validated encoded Call"),
                        messages,
                        reservation.id(),
                        target,
                        self.allow_answer_handoff,
                    )
                    .map_err(Error::Input)?
                {
                    let question = Question::publish_parts(
                        reservation,
                        parts,
                        output,
                        (method.interface, method.method),
                        self.allow_answer_handoff,
                    );
                    let key = question.key();
                    self.ledgers
                        .questions
                        .bind_owner(question.key(), slot_index)
                        .expect("new exchange owner");
                    slot.completion_owner = None;
                    let reserved = self
                        .free_exchanges
                        .take(&mut self.exchanges, |slot| &mut slot.free);
                    debug_assert_eq!(reserved, slot_index);
                    let slot = &mut self.exchanges[slot_index];
                    slot.exchange = Some(Exchange::Pending(question));
                    slot.canceled = false;
                    slot.tail_taken = false;
                    slot.authorized = true;
                    ticket.accept_published();
                    return Ok(Some(key));
                }
            }
        }
        let frame = match params {
            ImmediateParams::EncodedCall(encoded) => match target {
                calls::Target::Imported(target) => encoded
                    .copy(
                        template.expect("validated encoded Call"),
                        messages,
                        reservation.id(),
                        target,
                        self.allow_answer_handoff,
                        options,
                    )
                    .map_err(Error::Input)?,
                calls::Target::Promised { question, path } => encoded
                    .copy_promised(
                        messages,
                        reservation.id(),
                        question,
                        path,
                        method,
                        minimum,
                        self.allow_answer_handoff,
                        options,
                        self.writer_policy,
                    )
                    .map_err(Error::Input)?,
            },
            ImmediateParams::Content(params) => {
                let frame = Frame::build(messages, minimum, options, |builder| {
                    calls::dynamic_call_with_handoff(
                        builder,
                        reservation.id(),
                        target,
                        method,
                        self.allow_answer_handoff,
                        |field| {
                            field.copy_with_capabilities(
                                *params,
                                capnp::wire_capability::IndexMap::identity(0),
                            )
                        },
                    )
                })
                .map_err(Error::Input)?;
                // Match queued invocation admission under this writer policy.
                // Checked publication validates the completed envelope; Native
                // publication keeps the local builder's ordinary write path.
                let validation = if self.writer_policy == WriterPolicy::Native {
                    Ok(())
                } else {
                    frame
                        .with(messages, |root| -> capnp::Result<()> {
                            let rpc_capnp::message::Call(call) = root.which()? else {
                                unreachable!()
                            };
                            call?
                                .get_params()?
                                .get_content()
                                .validate_capability_indices(0)
                        })
                        .and_then(|result| result.map_err(super::super::super::Error::from))
                };
                if let Err(error) = validation {
                    frame.release(messages);
                    return Err(Error::Input(error));
                }
                frame
            }
        };
        let question = Question::publish_template(
            reservation,
            messages,
            frame,
            output,
            (method.interface, method.method),
            self.allow_answer_handoff,
            self.owned_output,
        )
        .map_err(Error::Input)?;
        let key = question.key();
        self.ledgers
            .questions
            .bind_owner(question.key(), slot_index)
            .expect("new exchange owner");
        slot.completion_owner = None;
        let reserved = self
            .free_exchanges
            .take(&mut self.exchanges, |slot| &mut slot.free);
        debug_assert_eq!(reserved, slot_index);
        let slot = &mut self.exchanges[slot_index];
        slot.exchange = Some(Exchange::Pending(question));
        slot.canceled = false;
        slot.tail_taken = false;
        slot.authorized = true;
        ticket.accept_published();
        Ok(Some(key))
    }
}
