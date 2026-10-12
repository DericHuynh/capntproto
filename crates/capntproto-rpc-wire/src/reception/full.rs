//! Complete two-party descriptor reception with caller-backed parallel receipts.
use super::{rpc_capnp, Error};
mod releases;
use capntproto_rpc_engine::{
    answers, exports,
    ids::{AnswerId, ExportId, WireId},
    imports,
};
pub use releases::{prepare_with_releases, WithReleases};

/// Each array has one slot per descriptor. Exactly one ownership kind is set at
/// each index. Caller-supplied import owners remain in their receipts on failure.
pub struct Slots<'r, 'i, 'e, 'a, I> {
    pub imports: &'r mut [imports::ReceiveSlot<'i, I>],
    pub exports: &'r mut [exports::LeaseSlot<'e>],
    pub answers: &'r mut [answers::PipelineSlot<'a>],
}

pub(crate) fn validate<I>(
    payload: rpc_capnp::payload::Reader<'_>,
    slots: &Slots<'_, '_, '_, '_, I>,
) -> Result<(), Error> {
    let descriptors = payload.get_cap_table()?;
    let count = usize::try_from(descriptors.len()).map_err(|_| Error::DescriptorCount)?;
    if slots.imports.len() != count || slots.exports.len() != count || slots.answers.len() != count
    {
        return Err(Error::DescriptorCount);
    }
    payload
        .get_content()
        .validate_capability_indices(descriptors.len())?;
    for (index, descriptor) in descriptors.iter().enumerate() {
        validate_descriptor(
            descriptor,
            &slots.imports[index],
            &slots.exports[index],
            &slots.answers[index],
        )?;
    }
    Ok(())
}
pub(crate) fn validate_descriptor<I>(
    descriptor: rpc_capnp::cap_descriptor::Reader<'_>,
    import_slot: &imports::ReceiveSlot<'_, I>,
    export_slot: &exports::LeaseSlot<'_>,
    answer_slot: &answers::PipelineSlot<'_>,
) -> Result<(), Error> {
    let (export, answer) = match descriptor
        .which()
        .map_err(|_| Error::UnsupportedDescriptor)?
    {
        rpc_capnp::cap_descriptor::ReceiverHosted(id) => (Some(ExportId::from_wire(id)), None),
        rpc_capnp::cap_descriptor::ReceiverAnswer(answer) => {
            let answer = answer?;
            for op in answer.get_transform()? {
                op.which().map_err(|_| Error::UnsupportedDescriptor)?;
            }
            (None, Some(AnswerId::from_wire(answer.get_question_id())))
        }
        _ => {
            super::check_descriptor(descriptor, import_slot)?;
            (None, None)
        }
    };
    if export_slot.id() != export
        || answer_slot.id() != answer
        || ((export.is_some() || answer.is_some())
            && (import_slot.id().is_some() || import_slot.is_promise()))
    {
        return Err(Error::DescriptorMismatch);
    }
    Ok(())
}

#[must_use]
pub struct Prepared<'b, 'r, 'm, 'i, 'e, 'a, 'is, 'es, 'as_, I, E, A> {
    imports: imports::Receive<'b, 'r, 'i, 'is, I>,
    exports: exports::Leases<'b, 'r, 'e, 'es, E>,
    answers: answers::Pipelines<'b, 'r, 'a, 'as_, A>,
    payload: rpc_capnp::payload::Reader<'m>,
}
#[allow(clippy::type_complexity)]
pub fn prepare<'b, 'r, 'm, 'i, 'e, 'a, 'is, 'es, 'as_, I, E, A>(
    payload: rpc_capnp::payload::Reader<'m>,
    imports: &'b mut imports::Imports<'i, 'is, I>,
    exports: &'b mut exports::Exports<'e, 'es, E>,
    answers: &'b mut answers::Answers<'a, 'as_, A>,
    slots: Slots<'r, 'i, 'e, 'a, I>,
) -> Result<Prepared<'b, 'r, 'm, 'i, 'e, 'a, 'is, 'es, 'as_, I, E, A>, Error> {
    validate(payload, &slots)?;
    let imports = imports
        .prepare_receive(slots.imports)
        .map_err(Error::Imports)?;
    let exports = exports
        .prepare_leases(slots.exports)
        .map_err(Error::Exports)?;
    let answers = answers
        .prepare_pipelines(slots.answers)
        .map_err(Error::Answers)?;
    Ok(Prepared {
        imports,
        exports,
        answers,
        payload,
    })
}
impl<'r, 'm, 'i, 'e, 'a, I, E, A> Prepared<'_, 'r, 'm, 'i, 'e, 'a, '_, '_, '_, I, E, A> {
    pub fn accept(self) -> Received<'r, 'm, 'i, 'e, 'a, I> {
        Received {
            slots: Slots {
                imports: self.imports.commit(),
                exports: self.exports.accept(),
                answers: self.answers.accept(),
            },
            payload: self.payload,
        }
    }
}

#[must_use = "release or transfer every descriptor owner before discarding the view"]
pub struct Received<'r, 'm, 'i, 'e, 'a, I> {
    pub(crate) slots: Slots<'r, 'i, 'e, 'a, I>,
    pub(crate) payload: rpc_capnp::payload::Reader<'m>,
}
pub enum Capability<'r, 'm, 'i, 'e, 'a> {
    Remote(&'r imports::Local<'i>),
    Local(&'r exports::Lease<'e>),
    Promised {
        answer: &'r answers::Pipeline<'a>,
        path: rpc_capnp::promised_answer::Reader<'m>,
    },
}
impl<'m, 'i, 'e, 'a, I> Received<'_, 'm, 'i, 'e, 'a, I> {
    pub fn content(&self) -> capnp::any_pointer::Reader<'m> {
        self.payload.get_content()
    }
    pub fn capability<T>(
        &self,
        pointer: capnp::wire_capability::Reader<'_, T>,
    ) -> Result<Capability<'_, 'm, 'i, 'e, 'a>, Error> {
        if !pointer.belongs_to(self.payload.get_content()) {
            return Err(Error::ForeignMessage);
        }
        self.at(pointer.index())
    }
    pub fn pipeline(
        &self,
        path: crate::pipeline::Path<'_>,
    ) -> Result<Capability<'_, 'm, 'i, 'e, 'a>, Error> {
        self.at(path.resolve(self.payload.get_content())?)
    }
    fn at(&self, index: u32) -> Result<Capability<'_, 'm, 'i, 'e, 'a>, Error> {
        let i = usize::try_from(index).map_err(|_| Error::MissingCapability)?;
        let import = self.slots.imports.get(i).ok_or(Error::MissingCapability)?;
        descriptor_capability(
            self.payload.get_cap_table()?.get(index),
            import,
            &self.slots.exports[i],
            &self.slots.answers[i],
        )
    }
    /// Release counts only. Retired export/answer owners stay in their slots for
    /// explicit take_retired/take_finished draining outside runtime borrows.
    pub fn release<E, A>(
        self,
        imports: &mut imports::Imports<'i, '_, I>,
        exports: &mut exports::Exports<'e, '_, E>,
        answers: &mut answers::Answers<'a, '_, A>,
    ) {
        release_slots(self.slots, imports, exports, answers);
    }
}
pub(crate) fn descriptor_capability<'r, 'm, 'i, 'e, 'a, I>(
    descriptor: rpc_capnp::cap_descriptor::Reader<'m>,
    import: &'r imports::ReceiveSlot<'i, I>,
    export: &'r exports::LeaseSlot<'e>,
    answer: &'r answers::PipelineSlot<'a>,
) -> Result<Capability<'r, 'm, 'i, 'e, 'a>, Error> {
    if let Some(local) = import.local() {
        return Ok(Capability::Remote(local));
    }
    if let Some(local) = export.lease() {
        return Ok(Capability::Local(local));
    }
    if let Some(answer) = answer.pipeline() {
        let rpc_capnp::cap_descriptor::ReceiverAnswer(path) = descriptor
            .which()
            .map_err(|_| Error::UnsupportedDescriptor)?
        else {
            unreachable!("validated receiverAnswer receipt");
        };
        return Ok(Capability::Promised {
            answer,
            path: path?,
        });
    }
    Err(Error::MissingCapability)
}
pub(crate) fn release_slots<'i, 'e, 'a, I, E, A>(
    slots: Slots<'_, 'i, 'e, 'a, I>,
    imports: &mut imports::Imports<'i, '_, I>,
    exports: &mut exports::Exports<'e, '_, E>,
    answers: &mut answers::Answers<'a, '_, A>,
) {
    for receipt in slots.imports {
        if let Some(local) = receipt.take_local() {
            imports.release(local);
        }
    }
    for receipt in slots.exports {
        if let Some(local) = receipt.take() {
            exports.release_local_deferred(local);
        }
    }
    for receipt in slots.answers {
        if let Some(local) = receipt.take() {
            answers.release_pipeline_deferred(local);
        }
    }
}

#[cfg(test)]
mod tests;

impl Capability<'_, '_, '_, '_, '_> {
    /// Preserve descriptor provenance through promise resolution. Reflection
    /// needs a loopback barrier even if its current endpoint is the same peer.
    pub fn resolution_source(&self) -> capntproto_rpc_engine::clients::Source {
        use capntproto_rpc_engine::clients::Source;
        match self {
            Self::Remote(_) => Source::Direct,
            Self::Local(_) | Self::Promised { .. } => Source::Reflected,
        }
    }
}
