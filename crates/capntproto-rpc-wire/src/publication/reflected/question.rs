use super::{release_references, Error, Reference};
use crate::{admission, rpc_capnp};
use capntproto_rpc_engine::{exports, ids::WireId, imports, messages, questions};

/// Retained target plus one reference slot per parameter descriptor. All belong
/// to this connection. A None target is rejected. Rejection keeps every handle.
pub struct CallReferences<'a, 'imports, 'storage, 'questions, 'path, I> {
    pub imports: &'a mut imports::Imports<'imports, 'storage, I>,
    pub target: &'a mut Reference<'imports, 'questions, 'path>,
    pub params: &'a mut [Reference<'imports, 'questions, 'path>],
}

#[must_use]
pub struct Call<
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
    frame: super::Prepared<'a, 'exports, 'table, 'pool, 'storage, 'bytes, E>,
    reservation: questions::Reservation<'a, 'questions, 'question_storage>,
    references: CallReferences<'a, 'imports, 'import_storage, 'questions, 'path, I>,
}

/// Bind the new question, retained target, reflected parameter references,
/// exact export grants and immutable frame to one FIFO acceptance. No caller
/// callback or fallible operation runs after acceptance starts.
#[allow(clippy::type_complexity)]
pub fn question<
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
    mut reservation: questions::Reservation<'a, 'questions, 'question_storage>,
    batch: exports::Batch<'a, 'exports, 'table, E>,
    messages: &'a mut messages::Messages<'pool, 'storage, 'bytes>,
    lease: messages::Lease<'pool>,
    scratch: &mut [u32],
    options: capnp::message::ReaderOptions,
    references: CallReferences<'a, 'imports, 'import_storage, 'questions, 'path, I>,
) -> Result<
    Call<
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
    admission::Error,
> {
    let mut count = 0;
    let frame = super::super::prepare_with(
        batch,
        messages,
        lease,
        scratch,
        options,
        |descriptor, index| {
            count += 1;
            let slot = references
                .params
                .get(index)
                .ok_or(Error::ReflectionMismatch)?;
            super::check_reference(descriptor, slot, references.imports, reservation.ledger())
        },
        true,
    )?;
    if count != references.params.len() {
        return Err(Error::ReflectionMismatch.into());
    }
    {
        let view = frame.bytes();
        let mut bytes = &*view;
        let message = capnp::serialize::read_message_from_flat_slice_no_alloc(&mut bytes, options)?;
        let root = message.get_root::<rpc_capnp::message::Reader<'_>>()?;
        admission::check_question(reservation.id(), root, true, reservation.destination())?;
        let rpc_capnp::message::Call(call) =
            root.which().map_err(|_| admission::Error::Unsupported)?
        else {
            return Err(admission::Error::Unsupported);
        };
        let target = call?.get_target()?;
        match (
            target.which().map_err(|_| admission::Error::Unsupported)?,
            &*references.target,
        ) {
            (rpc_capnp::message_target::ImportedCap(id), Reference::Hosted(local))
                if id == references.imports.wire_id(local).to_wire() => {}
            (
                rpc_capnp::message_target::PromisedAnswer(answer),
                Reference::Answer { question, path },
            ) => {
                let answer = answer?;
                if reservation
                    .ledger()
                    .pipeline_id(question)
                    .map_err(Error::Question)?
                    .to_wire()
                    != answer.get_question_id()
                {
                    return Err(Error::ReflectionMismatch.into());
                }
                super::check_path(answer, *path)?;
            }
            _ => return Err(Error::ReflectionMismatch.into()),
        }
    }
    Ok(Call {
        frame,
        reservation,
        references,
    })
}

impl<'pool, 'questions, E, I> Call<'_, '_, '_, 'pool, '_, '_, '_, '_, 'questions, '_, '_, E, I> {
    pub fn accept_into(
        mut self,
        slot: messages::QueueSlot<'_, 'pool, '_, impl Sized>,
    ) -> questions::Local<'questions> {
        self.frame.accept_into(slot);
        release_references(
            core::slice::from_mut(self.references.target),
            self.references.imports,
            self.reservation.ledger(),
        );
        release_references(
            self.references.params,
            self.references.imports,
            self.reservation.ledger(),
        );
        self.reservation.accept()
    }
}

#[cfg(test)]
mod tests;
