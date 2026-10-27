//! Ordered publication of capabilities reflected back to their original owner.
//!
//! Each descriptor borrows an affine reference from caller storage. Rejection
//! leaves those references available for retry. Acceptance queues the frame
//! before releasing them, so any resulting Release/Finish follows that frame.
#![forbid(unsafe_code)]
use super::{Error, Prepared};
use crate::{pipeline::Path, rpc_capnp};
use capntproto_rpc_engine::{exports, ids::WireId, imports, messages, questions};

#[derive(Debug)]
pub enum Reference<'imports, 'questions, 'path> {
    /// Sender-owned descriptors and null entries do not reflect a local handle.
    None,
    Hosted(imports::Local<'imports>),
    Answer {
        question: questions::Local<'questions>,
        path: Path<'path>,
    },
}

/// Parallel to the exact descriptor table, including None for sender-owned
/// grants. Both ledgers stay exclusively borrowed through output acceptance.
/// Use the same connection's ledgers and FIFO as the export publication.
pub struct References<'a, 'imports, 'import_storage, 'questions, 'question_storage, 'path, I> {
    pub imports: &'a mut imports::Imports<'imports, 'import_storage, I>,
    pub questions: &'a mut questions::Questions<'questions, 'question_storage>,
    pub slots: &'a mut [Reference<'imports, 'questions, 'path>],
}

#[must_use = "accept into ordered output or drop to preserve references for retry"]
pub struct Publication<
    'a,
    'exports,
    'table,
    'pool,
    'storage,
    'bytes,
    'imports,
    'import_storage,
    'questions,
    'question_storage,
    'path,
    E,
    I,
> {
    frame: Prepared<'a, 'exports, 'table, 'pool, 'storage, 'bytes, E>,
    references: References<'a, 'imports, 'import_storage, 'questions, 'question_storage, 'path, I>,
}

/// Validate receiverHosted/receiverAnswer against retained references in the
/// same connection. IDs, descriptor kinds and every transform must match.
/// Sender grants still require the exact export batch. No prefix is published
/// on any error. This layer does not itself reserve a Call's question record.
#[allow(clippy::type_complexity)]
pub fn prepare<
    'a,
    'exports,
    'table,
    'pool,
    'storage,
    'bytes,
    'imports,
    'import_storage,
    'questions,
    'question_storage,
    'path,
    E,
    I,
>(
    batch: exports::Batch<'a, 'exports, 'table, E>,
    messages: &'a mut messages::Messages<'pool, 'storage, 'bytes>,
    lease: messages::Lease<'pool>,
    scratch: &mut [u32],
    options: capnp::message::ReaderOptions,
    references: References<'a, 'imports, 'import_storage, 'questions, 'question_storage, 'path, I>,
) -> Result<
    Publication<
        'a,
        'exports,
        'table,
        'pool,
        'storage,
        'bytes,
        'imports,
        'import_storage,
        'questions,
        'question_storage,
        'path,
        E,
        I,
    >,
    Error,
> {
    let mut count = 0;
    let frame = super::prepare_with(
        batch,
        messages,
        lease,
        scratch,
        options,
        |descriptor, index| {
            count += 1;
            let slot = references
                .slots
                .get(index)
                .ok_or(Error::ReflectionMismatch)?;
            check_reference(descriptor, slot, references.imports, references.questions)
        },
        true,
    )?;
    if count != references.slots.len() {
        return Err(Error::ReflectionMismatch);
    }
    Ok(Publication { frame, references })
}

impl<'pool, E, I> Publication<'_, '_, '_, 'pool, '_, '_, '_, '_, '_, '_, '_, E, I> {
    pub fn bytes(&self) -> core::cell::Ref<'_, [u8]> {
        self.frame.bytes()
    }
    /// Queue the exact frame before making its final Release/Finish eligible.
    /// Draining cleanup remains the caller's job; no user callbacks run here.
    pub fn accept_into(
        self,
        slot: messages::QueueSlot<'_, 'pool, '_, impl Sized>,
    ) -> messages::SendKey<'pool> {
        let key = self.frame.accept_into(slot);
        release_references(
            self.references.slots,
            self.references.imports,
            self.references.questions,
        );
        key
    }
}

#[cfg(test)]
mod tests;

pub(crate) fn check_reference<'i, 'q, I>(
    descriptor: rpc_capnp::cap_descriptor::Reader<'_>,
    slot: &Reference<'i, 'q, '_>,
    imports: &imports::Imports<'i, '_, I>,
    questions: &questions::Questions<'q, '_>,
) -> Result<bool, Error> {
    match (
        descriptor
            .which()
            .map_err(|_| Error::UnsupportedDescriptor)?,
        slot,
    ) {
        (rpc_capnp::cap_descriptor::ReceiverHosted(id), Reference::Hosted(local)) => {
            if imports.wire_id(local).to_wire() != id {
                return Err(Error::ReflectionMismatch);
            }
            Ok(true)
        }
        (
            rpc_capnp::cap_descriptor::ReceiverAnswer(answer),
            Reference::Answer { question, path },
        ) => {
            let answer = answer?;
            let id = questions.pipeline_id(question).map_err(Error::Question)?;
            if answer.get_question_id() != id.to_wire() {
                return Err(Error::ReflectionMismatch);
            }
            check_path(answer, *path)?;
            Ok(true)
        }
        (
            rpc_capnp::cap_descriptor::None(())
            | rpc_capnp::cap_descriptor::SenderHosted(_)
            | rpc_capnp::cap_descriptor::SenderPromise(_),
            Reference::None,
        ) => Ok(false),
        (rpc_capnp::cap_descriptor::ThirdPartyHosted(_), Reference::None) => Ok(false),
        _ => Err(Error::ReflectionMismatch),
    }
}

pub(crate) fn release_references<'i, 'q, I>(
    slots: &mut [Reference<'i, 'q, '_>],
    imports: &mut imports::Imports<'i, '_, I>,
    questions: &mut questions::Questions<'q, '_>,
) {
    for reference in slots {
        match core::mem::replace(reference, Reference::None) {
            Reference::None => (),
            Reference::Hosted(local) => imports.release(local),
            Reference::Answer { question, .. } => questions.release(question),
        }
    }
}

mod question;
pub use question::{question, Call, CallReferences};

fn check_path(answer: rpc_capnp::promised_answer::Reader<'_>, path: Path<'_>) -> Result<(), Error> {
    let mut fields = path.fields().iter();
    for operation in answer.get_transform()? {
        match operation
            .which()
            .map_err(|_| Error::UnsupportedDescriptor)?
        {
            rpc_capnp::promised_answer::op::Noop(()) => (),
            rpc_capnp::promised_answer::op::GetPointerField(field)
                if fields.next() == Some(&field) => {}
            _ => return Err(Error::ReflectionMismatch),
        }
    }
    if fields.next().is_some() {
        return Err(Error::ReflectionMismatch);
    }

    Ok(())
}

mod answer;
pub use answer::{answer, Answer};
