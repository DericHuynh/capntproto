//! Bind Return delivery, implicit parameter releases and result imports together.
use super::*;
use capntproto_rpc_engine::{ids::QuestionId, question::Delivery, questions};

#[must_use]
pub struct Returned<
    'a,
    'scratch,
    'receipts,
    'message,
    'question,
    'export,
    'import,
    'questions,
    'exports,
    'imports,
    E,
    I,
> {
    question: questions::Receive<'a, 'question, 'questions>,
    releases: exports::Releases<'a, 'scratch, 'export, 'exports, E>,
    result: Pending<'a, 'receipts, 'message, 'import, 'imports, I>,
}
enum Pending<'a, 'receipts, 'message, 'import, 'imports, I> {
    Results(reception::Prepared<'a, 'receipts, 'import, 'imports, 'message, I>),
    Exception(rpc_capnp::exception::Reader<'message>),
    Discarded,
}
pub enum Reply<'receipts, 'message, 'import, I> {
    Results(reception::Received<'receipts, 'import, 'message, I>),
    Exception(rpc_capnp::exception::Reader<'message>),
    /// An early Finish owns remote result cleanup; no result imports were made.
    Discarded,
}

/// `original` must be the retained immutable Bootstrap/Call accepted for this
/// question, not a reconstructed frame. Its ID and reachable indices are checked.
/// Supports results, exceptions and replies to cancelled calls. Tail-call adoption
/// and third-party result destinations require their own admission transitions.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn returned<
    'a,
    'scratch,
    'receipts,
    'message,
    'question,
    'export,
    'import,
    'questions,
    'exports,
    'imports,
    E,
    I,
>(
    result: rpc_capnp::return_::Reader<'message>,
    original: rpc_capnp::message::Reader<'_>,
    questions: &'a mut questions::Questions<'question, 'questions>,
    exports: &'a mut exports::Exports<'export, 'exports, E>,
    imports: &'a mut imports::Imports<'import, 'imports, I>,
    receipts: &'receipts mut [imports::ReceiveSlot<'import, I>],
    release_scratch: &'scratch mut [u32],
) -> Result<
    Returned<
        'a,
        'scratch,
        'receipts,
        'message,
        'question,
        'export,
        'import,
        'questions,
        'exports,
        'imports,
        E,
        I,
    >,
    Error,
> {
    let id = result.get_answer_id();
    let params = original_params(id, original)?;
    if original_redirected(original)?
        || questions
            .is_redirected(QuestionId::from_wire(id))
            .map_err(Error::Question)?
    {
        return Err(Error::Unsupported);
    }
    let question = questions
        .prepare_receive(QuestionId::from_wire(id), result.get_no_finish_needed())
        .map_err(Error::Question)?;
    let mut releases = exports
        .prepare_releases(release_scratch)
        .map_err(Error::Release)?;
    if result.get_release_param_caps() {
        record_params(&mut releases, params)?;
    }
    let releases = releases.finish().map_err(Error::Release)?;
    let discard = question.delivery() == Delivery::Discard;
    let pending = match result.which().map_err(|_| Error::Unsupported)? {
        rpc_capnp::return_::Results(payload) => {
            let payload = payload?;
            let table = payload.get_cap_table()?;
            payload
                .get_content()
                .validate_capability_indices(table.len())?;
            if result.get_no_finish_needed() && !table.is_empty() {
                return Err(Error::Unsupported);
            }
            if discard {
                Pending::Discarded
            } else {
                Pending::Results(
                    reception::prepare(payload, imports, receipts).map_err(Error::Reception)?,
                )
            }
        }
        rpc_capnp::return_::Exception(exception) => {
            let exception = exception?;
            exception
                .get_reason()?
                .to_str()
                .map_err(capnp::Error::from)?;
            if discard {
                Pending::Discarded
            } else {
                Pending::Exception(exception)
            }
        }
        rpc_capnp::return_::Canceled(()) if discard => Pending::Discarded,
        rpc_capnp::return_::Canceled(()) => return Err(Error::UnexpectedCancellation),
        _ => return Err(Error::Unsupported),
    };
    Ok(Returned {
        question,
        releases,
        result: pending,
    })
}
pub(super) fn original_params(
    answer_id: u32,
    original: rpc_capnp::message::Reader<'_>,
) -> Result<Option<rpc_capnp::payload::Reader<'_>>, Error> {
    let (id, params) = match original.which().map_err(|_| Error::Unsupported)? {
        rpc_capnp::message::Bootstrap(b) => (b?.get_question_id(), None),
        rpc_capnp::message::Accept(a) => (a?.get_question_id(), None),
        rpc_capnp::message::Join(j) => (j?.get_question_id(), None),
        rpc_capnp::message::ThirdPartyAnswer(a) => (a?.get_answer_id(), None),
        rpc_capnp::message::Call(c) => {
            let c = c?;
            (c.get_question_id(), Some(c.get_params()?))
        }
        _ => return Err(Error::Unsupported),
    };
    if id != answer_id {
        return Err(Error::Mismatch);
    }
    Ok(params)
}
pub(super) fn original_redirected(original: rpc_capnp::message::Reader<'_>) -> Result<bool, Error> {
    match original.which().map_err(|_| Error::Unsupported)? {
        rpc_capnp::message::Bootstrap(_)
        | rpc_capnp::message::Accept(_)
        | rpc_capnp::message::ThirdPartyAnswer(_)
        | rpc_capnp::message::Join(_) => Ok(false),
        rpc_capnp::message::Call(call) => match call?.get_send_results_to().which() {
            Ok(rpc_capnp::call::send_results_to::Caller(())) => Ok(false),
            Ok(rpc_capnp::call::send_results_to::Yourself(())) => Ok(true),
            Ok(rpc_capnp::call::send_results_to::ThirdParty(metadata)) => {
                metadata.validate_capability_indices(0)?;
                Ok(true)
            }
            _ => Err(Error::Unsupported),
        },
        _ => Err(Error::Unsupported),
    }
}
pub(super) fn record_params<E>(
    releases: &mut exports::ReleaseBatch<'_, '_, '_, '_, E>,
    params: Option<rpc_capnp::payload::Reader<'_>>,
) -> Result<(), Error> {
    if let Some(params) = params {
        let table = params.get_cap_table()?;
        params
            .get_content()
            .validate_capability_indices(table.len())?;
        record_param_descriptors(releases, table)?;
    }
    Ok(())
}
pub(super) fn record_param_descriptors<E>(
    releases: &mut exports::ReleaseBatch<'_, '_, '_, '_, E>,
    table: capnp::struct_list::Reader<'_, rpc_capnp::cap_descriptor::Owned>,
) -> Result<(), Error> {
    for descriptor in table {
        match descriptor.which().map_err(|_| Error::Unsupported)? {
            rpc_capnp::cap_descriptor::None(())
            | rpc_capnp::cap_descriptor::ReceiverHosted(_)
            | rpc_capnp::cap_descriptor::ReceiverAnswer(_) => (),
            rpc_capnp::cap_descriptor::SenderHosted(id)
            | rpc_capnp::cap_descriptor::SenderPromise(id) => releases
                .record(ExportId::from_wire(id), 1)
                .map_err(Error::Release)?,
            rpc_capnp::cap_descriptor::ThirdPartyHosted(cap) => releases
                .record(ExportId::from_wire(cap?.get_vine_id()), 1)
                .map_err(Error::Release)?,
        }
    }
    Ok(())
}

impl<'receipts, 'message, 'question, 'import, E, I>
    Returned<'_, '_, 'receipts, 'message, 'question, '_, 'import, '_, '_, '_, E, I>
{
    /// Retired export owners remain available through take_retired(); result
    /// references belong to the returned payload view. No callbacks run here.
    pub fn accept(
        self,
    ) -> (
        questions::Key<'question>,
        Reply<'receipts, 'message, 'import, I>,
    ) {
        self.releases.accept();
        let (key, _) = self.question.accept();
        let reply = match self.result {
            Pending::Results(result) => Reply::Results(result.accept()),
            Pending::Exception(exception) => Reply::Exception(exception),
            Pending::Discarded => Reply::Discarded,
        };
        (key, reply)
    }
}

#[cfg(test)]
mod tests;
