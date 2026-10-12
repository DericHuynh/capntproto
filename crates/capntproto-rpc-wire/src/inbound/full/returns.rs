use super::*;
use capntproto_rpc_engine::{ids::QuestionId, question::Delivery, questions};

#[must_use]
pub struct Returned<'b, 's, 'r, 'm, 'q, 'i, 'e, 'a, 'qs, 'is, 'es, 'as_, I, E, A> {
    question: questions::Receive<'b, 'q, 'qs>,
    pending: Pending<'b, 's, 'r, 'm, 'i, 'e, 'a, 'is, 'es, 'as_, I, E, A>,
}
enum Pending<'b, 's, 'r, 'm, 'i, 'e, 'a, 'is, 'es, 'as_, I, E, A> {
    Results(capabilities::WithReleases<'b, 's, 'r, 'm, 'i, 'e, 'a, 'is, 'es, 'as_, I, E, A>),
    Exception(
        rpc_capnp::exception::Reader<'m>,
        exports::Releases<'b, 's, 'e, 'es, E>,
    ),
    Adopted(
        answers::Adoption<'b, 'a, 'as_, A>,
        exports::Releases<'b, 's, 'e, 'es, E>,
        bool,
    ),
    Await(
        capnp::any_pointer::Reader<'m>,
        exports::Releases<'b, 's, 'e, 'es, E>,
    ),
    Elsewhere(exports::Releases<'b, 's, 'e, 'es, E>),
    Discarded(exports::Releases<'b, 's, 'e, 'es, E>),
}
pub enum Reply<'r, 'm, 'i, 'e, 'a, I> {
    Results(capabilities::Received<'r, 'm, 'i, 'e, 'a, I>),
    Exception(rpc_capnp::exception::Reader<'m>),
    Adopted(answers::Adopted<'a>),
    AwaitFromThirdParty(capnp::any_pointer::Reader<'m>),
    ResultsSentElsewhere,
    Discarded,
}

/// Metadata from the exact retained request. The data-only constructor is used
/// only after private template publication has validated the complete payload
/// and proved its descriptor table empty.
pub(crate) struct Original<'m> {
    params: Option<rpc_capnp::payload::Reader<'m>>,
    redirected: bool,
    allows_handoff: bool,
    pub(crate) method: Option<(u64, u16)>,
}
impl<'m> Original<'m> {
    pub(crate) fn data_only(method: (u64, u16), allows_handoff: bool) -> Self {
        Self {
            params: None,
            redirected: false,
            allows_handoff,
            method: Some(method),
        }
    }
    pub(crate) fn read(id: u32, root: rpc_capnp::message::Reader<'m>) -> Result<Self, Error> {
        let (question, params, redirected, allows_handoff, method) = match root
            .which()
            .map_err(|_| Error::Unsupported)?
        {
            rpc_capnp::message::Call(call) => {
                let call = call?;
                let redirected = match call
                    .get_send_results_to()
                    .which()
                    .map_err(|_| Error::Unsupported)?
                {
                    rpc_capnp::call::send_results_to::Caller(()) => false,
                    rpc_capnp::call::send_results_to::Yourself(()) => true,
                    rpc_capnp::call::send_results_to::ThirdParty(metadata) => {
                        metadata.validate_capability_indices(0)?;
                        true
                    }
                };
                (
                    call.get_question_id(),
                    Some(call.get_params()?),
                    redirected,
                    call.get_allow_third_party_tail_call(),
                    Some((call.get_interface_id(), call.get_method_id())),
                )
            }
            rpc_capnp::message::Bootstrap(value) => {
                (value?.get_question_id(), None, false, false, None)
            }
            rpc_capnp::message::Accept(value) => {
                (value?.get_question_id(), None, false, false, None)
            }
            rpc_capnp::message::Join(value) => (value?.get_question_id(), None, false, false, None),
            rpc_capnp::message::ThirdPartyAnswer(value) => {
                (value?.get_answer_id(), None, false, false, None)
            }
            _ => return Err(Error::Unsupported),
        };
        if question != id {
            return Err(Error::Mismatch);
        }
        Ok(Self {
            params,
            redirected,
            allows_handoff,
            method,
        })
    }
}

/// Admit results and their complete two-party descriptor table, including a
/// receiverHosted lease acquired as its final parameter grant is released.
/// The caller retains the exact original accepted Call/Bootstrap until Return.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn returned<'b, 's, 'r, 'm, 'q, 'i, 'e, 'a, 'qs, 'is, 'es, 'as_, I, E, A>(
    result: rpc_capnp::return_::Reader<'m>,
    original: rpc_capnp::message::Reader<'_>,
    questions: &'b mut questions::Questions<'q, 'qs>,
    imports: &'b mut imports::Imports<'i, 'is, I>,
    exports: &'b mut exports::Exports<'e, 'es, E>,
    answers: &'b mut answers::Answers<'a, 'as_, A>,
    slots: Slots<'r, 'i, 'e, 'a, I>,
    scratch: &'s mut [u32],
) -> Result<Returned<'b, 's, 'r, 'm, 'q, 'i, 'e, 'a, 'qs, 'is, 'es, 'as_, I, E, A>, Error> {
    let original = Original::read(result.get_answer_id(), original)?;
    returned_prepared(
        result, original, questions, imports, exports, answers, slots, scratch,
    )
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn returned_prepared<'b, 's, 'r, 'm, 'q, 'i, 'e, 'a, 'qs, 'is, 'es, 'as_, I, E, A>(
    result: rpc_capnp::return_::Reader<'m>,
    original: Original<'_>,
    questions: &'b mut questions::Questions<'q, 'qs>,
    imports: &'b mut imports::Imports<'i, 'is, I>,
    exports: &'b mut exports::Exports<'e, 'es, E>,
    answers: &'b mut answers::Answers<'a, 'as_, A>,
    slots: Slots<'r, 'i, 'e, 'a, I>,
    scratch: &'s mut [u32],
) -> Result<Returned<'b, 's, 'r, 'm, 'q, 'i, 'e, 'a, 'qs, 'is, 'es, 'as_, I, E, A>, Error> {
    let id = result.get_answer_id();
    let redirected = questions
        .is_redirected(QuestionId::from_wire(id))
        .map_err(Error::Question)?;
    if redirected != original.redirected {
        return Err(Error::Mismatch);
    }
    let question = questions
        .prepare_receive(QuestionId::from_wire(id), result.get_no_finish_needed())
        .map_err(Error::Question)?;
    let mut releases = exports.prepare_releases(scratch).map_err(Error::Release)?;
    if result.get_release_param_caps() {
        super::super::returns::record_params(&mut releases, original.params)?;
    }
    let releases = releases.finish().map_err(Error::Release)?;
    let discard = question.delivery() == Delivery::Discard;
    let pending = match result.which().map_err(|_| Error::Unsupported)? {
        rpc_capnp::return_::Results(payload) if !redirected => {
            let payload = payload?;
            let table = payload.get_cap_table()?;
            if result.get_no_finish_needed() && !table.is_empty() {
                return Err(Error::Unsupported);
            }
            if discard {
                // Live results are validated by prepare_with_releases below.
                // Discarded results still need the same bounded wire checks.
                payload
                    .get_content()
                    .validate_capability_indices(table.len())?;
                Pending::Discarded(releases)
            } else {
                Pending::Results(
                    capabilities::prepare_with_releases(payload, imports, releases, answers, slots)
                        .map_err(Error::Reception)?,
                )
            }
        }
        rpc_capnp::return_::Exception(exception) if !redirected => {
            let exception = exception?;
            exception
                .get_reason()?
                .to_str()
                .map_err(capnp::Error::from)?;
            if discard {
                Pending::Discarded(releases)
            } else {
                Pending::Exception(exception, releases)
            }
        }
        rpc_capnp::return_::AwaitFromThirdParty(metadata) if !redirected => {
            if !original.allows_handoff {
                return Err(Error::Unsupported);
            }
            metadata.validate_capability_indices(0)?;
            if discard {
                Pending::Discarded(releases)
            } else {
                Pending::Await(metadata, releases)
            }
        }
        rpc_capnp::return_::TakeFromOtherQuestion(source) if !redirected => {
            let adoption = answers
                .prepare_adoption(AnswerId::from_wire(source))
                .map_err(Error::Answer)?;
            Pending::Adopted(adoption, releases, discard)
        }
        rpc_capnp::return_::ResultsSentElsewhere(()) if redirected => {
            if discard {
                Pending::Discarded(releases)
            } else {
                Pending::Elsewhere(releases)
            }
        }
        rpc_capnp::return_::Canceled(()) if discard => Pending::Discarded(releases),
        rpc_capnp::return_::Canceled(()) => return Err(Error::UnexpectedCancellation),
        _ => return Err(Error::Unsupported),
    };
    Ok(Returned { question, pending })
}
impl<'r, 'm, 'q, 'i, 'e, 'a, I, E, A>
    Returned<'_, '_, 'r, 'm, 'q, 'i, 'e, 'a, '_, '_, '_, '_, I, E, A>
{
    pub fn accept(self) -> (questions::Key<'q>, Reply<'r, 'm, 'i, 'e, 'a, I>) {
        let (key, _) = self.question.accept();
        let result = match self.pending {
            Pending::Results(result) => Reply::Results(result.accept()),
            Pending::Exception(exception, releases) => {
                releases.accept();
                Reply::Exception(exception)
            }
            Pending::Adopted(adoption, releases, discard) => {
                releases.accept();
                if discard {
                    adoption.discard();
                    Reply::Discarded
                } else {
                    Reply::Adopted(adoption.accept())
                }
            }
            Pending::Await(metadata, releases) => {
                releases.accept();
                Reply::AwaitFromThirdParty(metadata)
            }
            Pending::Elsewhere(releases) => {
                releases.accept();
                Reply::ResultsSentElsewhere
            }
            Pending::Discarded(releases) => {
                releases.accept();
                Reply::Discarded
            }
        };
        (key, result)
    }
}
