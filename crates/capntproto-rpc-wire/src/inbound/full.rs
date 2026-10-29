//! Incoming calls with all two-party capability descriptor ownership kinds.
use super::*;
mod resolution;
mod returns;
use crate::reception::full::{self as capabilities, Slots};
pub use resolution::{resolve, Resolution, Resolved};
pub use returns::{returned, Reply, Returned};
pub(crate) use returns::{
    returned_prepared, Original, OriginalMetadata, ParameterExports, PreparedReturn,
};

/// Imported receipts have one entry per payload descriptor. Export/answer
/// receipts add one final entry for the call target. All wire identities must
/// match the immutable Call; acceptance retains payload and target together.
#[must_use]
pub struct Call<'b, 'r, 'p, 'm, 'i, 'e, 'a, 'is, 'es, 'as_, I, E, A> {
    incoming: crate::calls::Incoming<'m>,
    imports: imports::Receive<'b, 'r, 'i, 'is, I>,
    exports: exports::Leases<'b, 'r, 'e, 'es, E>,
    answer: answers::PipelineAdmission<'b, 'r, 'a, 'as_, A>,
    path: Option<pipeline::Path<'p>>,
    payload: rpc_capnp::payload::Reader<'m>,
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn call<'b, 'r, 'p, 'm, 'i, 'e, 'a, 'is, 'es, 'as_, I, E, A>(
    call: rpc_capnp::call::Reader<'m>,
    imports: &'b mut imports::Imports<'i, 'is, I>,
    exports: &'b mut exports::Exports<'e, 'es, E>,
    answers: &'b mut answers::Answers<'a, 'as_, A>,
    slots: Slots<'r, 'i, 'e, 'a, I>,
    scratch: &'p mut [u16],
) -> Result<Call<'b, 'r, 'p, 'm, 'i, 'e, 'a, 'is, 'es, 'as_, I, E, A>, Error> {
    call_parts(
        call,
        call.get_target()?,
        capabilities::PayloadEnvelope::new(call.get_params()?)?,
        imports,
        exports,
        answers,
        slots,
        scratch,
    )
}

// Owned admission already reads these fields to reserve receipts and path
// storage. Reuse those readers; validation and ledger preparation still happen
// here, before any receipt commits.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn call_parts<'b, 'r, 'p, 'm, 'i, 'e, 'a, 'is, 'es, 'as_, I, E, A>(
    call: rpc_capnp::call::Reader<'m>,
    target: rpc_capnp::message_target::Reader<'m>,
    payload: capabilities::PayloadEnvelope<'m>,
    imports: &'b mut imports::Imports<'i, 'is, I>,
    exports: &'b mut exports::Exports<'e, 'es, E>,
    answers: &'b mut answers::Answers<'a, 'as_, A>,
    slots: Slots<'r, 'i, 'e, 'a, I>,
    scratch: &'p mut [u16],
) -> Result<Call<'b, 'r, 'p, 'm, 'i, 'e, 'a, 'is, 'es, 'as_, I, E, A>, Error> {
    caller_question(call.get_question_id())?;
    // onlyPromisePipeline is an optional hint. This profile sends a Return
    // for every call, which the protocol explicitly permits.
    let redirected = match call.get_send_results_to().which() {
        Ok(rpc_capnp::call::send_results_to::Caller(())) => crate::calls::ResultsTo::Caller,
        Ok(rpc_capnp::call::send_results_to::Yourself(())) => crate::calls::ResultsTo::Yourself,
        Ok(rpc_capnp::call::send_results_to::ThirdParty(metadata)) => {
            metadata.validate_capability_indices(0)?;
            crate::calls::ResultsTo::ThirdParty
        }
        _ => return Err(Error::Unsupported),
    };
    let (last_export, payload_exports) = slots.exports.split_last_mut().ok_or(Error::Mismatch)?;
    let (last_answer, payload_answers) = slots.answers.split_last_mut().ok_or(Error::Mismatch)?;
    payload
        .validate_slots(&Slots {
            imports: slots.imports,
            exports: payload_exports,
            answers: payload_answers,
        })
        .map_err(Error::Reception)?;
    let payload = payload.payload();
    let (export, answer, path) = match target.which().map_err(|_| Error::Unsupported)? {
        rpc_capnp::message_target::ImportedCap(id) => (Some(ExportId::from_wire(id)), None, None),
        rpc_capnp::message_target::PromisedAnswer(answer) => {
            let answer = answer?;
            let path = pipeline::Path::decode(answer, scratch).map_err(Error::Target)?;
            (
                None,
                Some(AnswerId::from_wire(answer.get_question_id())),
                Some(path),
            )
        }
    };
    if last_export.id() != export || last_answer.id() != answer {
        return Err(Error::Mismatch);
    }
    let imports = imports
        .prepare_receive(slots.imports)
        .map_err(|e| Error::Reception(reception::Error::Imports(e)))?;
    let exports = exports
        .prepare_leases(slots.exports)
        .map_err(|e| Error::Reception(reception::Error::Exports(e)))?;
    let answer = answers
        .prepare_admission_with_pipelines(
            AnswerId::from_wire(call.get_question_id()),
            slots.answers,
        )
        .map_err(|e| Error::Reception(reception::Error::Answers(e)))?;
    let answer = match redirected {
        crate::calls::ResultsTo::Caller => answer,
        crate::calls::ResultsTo::Yourself => answer.redirected(),
        crate::calls::ResultsTo::ThirdParty => answer.third_party(),
    }
    .allow_handoff(call.get_allow_third_party_tail_call());
    Ok(Call {
        incoming: crate::calls::Incoming::new(call),
        imports,
        exports,
        answer,
        path,
        payload,
    })
}

pub struct AdmittedCall<'r, 'p, 'm, 'i, 'e, 'a, I> {
    pub incoming: crate::calls::Incoming<'m>,
    pub answer: answers::Key<'a>,
    pub target: pipeline::Target<'e, 'a, 'p>,
    pub payload: capabilities::Received<'r, 'm, 'i, 'e, 'a, I>,
}
impl<'r, 'p, 'm, 'i, 'e, 'a, I, E, A> Call<'_, 'r, 'p, 'm, 'i, 'e, 'a, '_, '_, '_, I, E, A> {
    pub fn accept(self, owner: A) -> AdmittedCall<'r, 'p, 'm, 'i, 'e, 'a, I> {
        let imports = self.imports.commit();
        let (target_export, exports) = self.exports.accept().split_last_mut().unwrap();
        let (answer, pipelines) = self.answer.accept(owner);
        let (target_answer, answers) = pipelines.split_last_mut().unwrap();
        let target = match (target_export.take(), target_answer.take(), self.path) {
            (Some(export), None, None) => pipeline::Target::Hosted(export),
            (None, Some(answer), Some(path)) => pipeline::Target::Promised { answer, path },
            _ => unreachable!("preflighted target kind"),
        };
        AdmittedCall {
            incoming: self.incoming,
            answer,
            target,
            payload: capabilities::Received {
                slots: Slots {
                    imports,
                    exports,
                    answers,
                },
                payload: self.payload,
            },
        }
    }
}
#[cfg(test)]
mod tail_tests;
#[cfg(test)]
mod tests;
