//! Atomic incoming Call admission over the production ownership ledgers.
#![forbid(unsafe_code)]
use crate::{pipeline, reception, rpc_capnp};
pub mod full;
mod resolution;
mod returns;
use capntproto_rpc_engine::{
    answers, exports,
    ids::{AnswerId, ExportId, WireId},
    imports,
};
pub use resolution::{resolve, Resolution, Resolved};
pub use returns::{returned, Reply, Returned};

pub(crate) fn caller_question(id: u32) -> Result<(), Error> {
    if (1 << 30..1 << 31).contains(&id) {
        return Err(Error::Unsupported);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Wire(capnp::ErrorKind),
    Unsupported,
    Target(pipeline::Error),
    Answer(answers::Error),
    Export(exports::Error),
    Reception(reception::Error),
    Question(capntproto_rpc_engine::questions::Error),
    Release(exports::ReleaseError),
    Mismatch,
    UnexpectedCancellation,
    Embargo(capntproto_rpc_engine::embargoes::Error),
}
impl From<capnp::Error> for Error {
    fn from(error: capnp::Error) -> Self {
        Self::Wire(error.kind)
    }
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "incoming RPC admission: {self:?}")
    }
}
impl core::error::Error for Error {}

/// Detach a matching accepted loopback owner before the caller wakes queued work.
pub fn receiver_loopback<T, S>(
    barrier: rpc_capnp::disembargo::Reader<'_>,
    ledger: &mut capntproto_rpc_engine::embargoes::Embargoes<T, S>,
) -> Result<Option<T>, Error>
where
    S: capntproto_rpc_engine::peer_table::SparseStorage<
        capntproto_rpc_engine::ids::EmbargoId,
        capntproto_rpc_engine::embargoes::Record<T>,
    >,
{
    let rpc_capnp::disembargo::context::ReceiverLoopback(id) = barrier
        .get_context()
        .which()
        .map_err(|_| Error::Unsupported)?
    else {
        return Err(Error::Unsupported);
    };
    ledger
        .receive(capntproto_rpc_engine::ids::EmbargoId::from_wire(id))
        .map_err(Error::Embargo)
}

/// Dropping this ticket changes no committed counts or answer state. Every
/// potentially fallible check happens before accepting any part of the call.
#[must_use]
pub struct Call<
    'a,
    'receipts,
    'path,
    'message,
    'answer,
    'export,
    'import,
    'answers,
    'exports,
    'imports,
    A,
    E,
    I,
> {
    incoming: crate::calls::Incoming<'message>,
    answer: answers::Admission<'a, 'answer, 'answers, A>,
    hosted: Option<exports::LeaseReservation<'a, 'export, 'exports, E>>,
    path: Option<pipeline::Path<'path>>,
    payload: reception::Prepared<'a, 'receipts, 'import, 'imports, 'message, I>,
}

/// Check an ordinary caller-directed Call and all its parameter descriptors.
/// The target is retained with the answer insertion and import receipt commit.
/// Scratch holds normalized pipeline fields; receipts are bound to the actual
/// immutable payload. The caller retains that message until dispatch finishes.
/// Payloads support imported hosted/promised capabilities and forwarding vines;
/// targets support imported capabilities and promised answers.
#[allow(clippy::type_complexity)]
pub fn call<
    'a,
    'receipts,
    'path,
    'message,
    'answer,
    'export,
    'import,
    'answers,
    'exports,
    'imports,
    A,
    E,
    I,
>(
    call: rpc_capnp::call::Reader<'message>,
    answers: &'a mut answers::Answers<'answer, 'answers, A>,
    exports: &'a mut exports::Exports<'export, 'exports, E>,
    imports: &'a mut imports::Imports<'import, 'imports, I>,
    receipts: &'receipts mut [imports::ReceiveSlot<'import, I>],
    scratch: &'path mut [u16],
) -> Result<
    Call<
        'a,
        'receipts,
        'path,
        'message,
        'answer,
        'export,
        'import,
        'answers,
        'exports,
        'imports,
        A,
        E,
        I,
    >,
    Error,
> {
    caller_question(call.get_question_id())?;
    if !matches!(
        call.get_send_results_to().which(),
        Ok(rpc_capnp::call::send_results_to::Caller(()))
    ) {
        return Err(Error::Unsupported);
    }
    let payload =
        reception::prepare(call.get_params()?, imports, receipts).map_err(Error::Reception)?;
    let (hosted, parent, path) = match call.get_target()?.which().map_err(|_| Error::Unsupported)? {
        rpc_capnp::message_target::ImportedCap(id) => (
            Some(
                exports
                    .prepare_lease(ExportId::from_wire(id))
                    .map_err(Error::Export)?,
            ),
            None,
            None,
        ),
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
    let answer = answers
        .prepare_admission(AnswerId::from_wire(call.get_question_id()), parent)
        .map_err(Error::Answer)?
        .allow_handoff(call.get_allow_third_party_tail_call());
    Ok(Call {
        incoming: crate::calls::Incoming::new(call),
        answer,
        hosted,
        path,
        payload,
    })
}

#[cfg(test)]
mod tests;

pub struct AdmittedCall<'receipts, 'path, 'message, 'answer, 'export, 'import, I> {
    pub incoming: crate::calls::Incoming<'message>,
    pub answer: answers::Key<'answer>,
    pub target: pipeline::Target<'export, 'answer, 'path>,
    pub payload: reception::Received<'receipts, 'import, 'message, I>,
}
impl<'receipts, 'path, 'message, 'answer, 'export, 'import, A, E, I>
    Call<'_, 'receipts, 'path, 'message, 'answer, 'export, 'import, '_, '_, '_, A, E, I>
{
    /// No callback, destructor, IO, allocation or further admission failure can
    /// occur within this transition. The caller owns the returned target lease.
    pub fn accept(
        self,
        owner: A,
    ) -> AdmittedCall<'receipts, 'path, 'message, 'answer, 'export, 'import, I> {
        let (answer, parent) = self.answer.accept(owner);
        let target = match (self.hosted, parent, self.path) {
            (Some(hosted), None, None) => pipeline::Target::Hosted(hosted.accept()),
            (None, Some(answer), Some(path)) => pipeline::Target::Promised { answer, path },
            _ => unreachable!("validated target admission"),
        };
        AdmittedCall {
            incoming: self.incoming,
            answer,
            target,
            payload: self.payload.accept(),
        }
    }
}

mod controls;
pub use controls::{finished, released, Finished};

pub(crate) use controls::finished_parts;
