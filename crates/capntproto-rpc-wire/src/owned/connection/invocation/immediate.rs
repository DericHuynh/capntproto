//! Publish settled, data-only calls without a temporary queued Call envelope.
use super::*;

/// Parameters for immediate publication. An encoded Call is an unframed single
/// segment, validated under the destination reader policy before copying. Its
/// method/hints must match; target/question IDs are assigned by the driver.
pub enum ImmediateParams<'a> {
    Content(capnp::any_pointer::Reader<'a>),
    EncodedCall(EncodedCall<'a>),
}

impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// Try immediate publication of a settled imported call without pipelines or
    /// parameter capabilities. `None` leaves the caller's request untouched and
    /// requires ordinary queued invocation. Older tasks, pending promises and
    /// deferred wire input are never overtaken. All storage is caller supplied.
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
        self.check_live()?;
        if !method.no_promise_pipelining
            || method.only_promise_pipeline
            || self.call.is_some()
            || self.echo.is_some()
        {
            return Ok(None);
        }
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
        let target = remote.id();
        let (template, minimum) = match params {
            ImmediateParams::Content(params) => {
                let size = params.target_size().map_err(|e| Error::Input(e.into()))?;
                if size.cap_count != 0 {
                    return Ok(None);
                }
                let minimum = size
                    .word_count
                    .checked_add(16)
                    .and_then(|words| words.checked_mul(8))
                    .and_then(|bytes| usize::try_from(bytes).ok())
                    .ok_or(Error::Input(super::super::super::Error::Capacity))?
                    .max(minimum);
                (None, minimum)
            }
            ImmediateParams::EncodedCall(encoded) => {
                let template = encoded.for_policy(method, options).map_err(Error::Input)?;
                (Some(template), minimum)
            }
        };
        let slot = self
            .exchanges
            .vacant_mut(|_, slot| slot.exchange.is_none(), || ExchangeSlot::EMPTY)
            .ok_or(Error::Busy)?;
        let output = queue
            .reserve_with(resources)
            .map_err(|e| Error::Input(super::super::super::Error::Storage(e)))?;
        let reservation = self.ledgers.questions.reserve().map_err(Error::Question)?;
        let frame = match params {
            ImmediateParams::EncodedCall(encoded) => template
                .expect("validated encoded Call")
                .copy_segment(
                    encoded.segment(),
                    messages,
                    reservation.id(),
                    target,
                    self.allow_answer_handoff,
                    options,
                )
                .map_err(Error::Input)?,
            ImmediateParams::Content(params) => {
                let frame = Frame::build(messages, minimum, options, |builder| {
                    calls::dynamic_call_with_handoff(
                        builder,
                        reservation.id(),
                        calls::Target::Imported(target),
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
                // Match queued invocation admission, including the final envelope's
                // traversal/nesting limits. No unvalidated raw frame gets a certificate.
                let validation = frame
                    .with(messages, |root| -> capnp::Result<()> {
                        let rpc_capnp::message::Call(call) = root.which()? else {
                            unreachable!()
                        };
                        call?
                            .get_params()?
                            .get_content()
                            .validate_capability_indices(0)
                    })
                    .and_then(|result| result.map_err(super::super::super::Error::from));
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
        slot.exchange = Some(Exchange::Pending(question));
        slot.canceled = false;
        slot.tail_taken = false;
        slot.authorized = true;
        ticket.accept_published();
        Ok(Some(key))
    }
}
