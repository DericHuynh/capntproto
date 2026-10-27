//! Bounded promised-answer transforms and target ownership.
#![forbid(unsafe_code)]
use crate::rpc_capnp;
use capntproto_rpc_engine::{
    answers, exports,
    ids::{AnswerId, ExportId, QuestionId, WireId},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Wire(capnp::ErrorKind),
    UnknownTransform,
    Capacity,
    Export(exports::Error),
    Answer(answers::Error),
}
impl From<capnp::Error> for Error {
    fn from(error: capnp::Error) -> Self {
        Self::Wire(error.kind)
    }
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "promised-answer target: {self:?}")
    }
}
impl core::error::Error for Error {}

/// A normalized transform copied into caller storage. No pointers into a
/// receive buffer survive, so that buffer can be reused while the call waits.
#[derive(Clone, Copy, Debug)]
pub struct Path<'a> {
    fields: &'a [u16],
}
/// Validated immutable transform. Retain its reader while reserving caller
/// storage, so decoding need not fetch and validate the same list again.
pub(crate) struct PreparedPath<'a> {
    transform: capnp::struct_list::Reader<'a, rpc_capnp::promised_answer::op::Owned>,
    length: usize,
}
impl<'a> PreparedPath<'a> {
    pub(crate) fn new(source: rpc_capnp::promised_answer::Reader<'a>) -> Result<Self, Error> {
        let transform = source.get_transform()?;
        let mut length = 0usize;
        for op in transform {
            match op.which().map_err(|_| Error::UnknownTransform)? {
                rpc_capnp::promised_answer::op::Noop(()) => (),
                rpc_capnp::promised_answer::op::GetPointerField(_) => {
                    length = length.checked_add(1).ok_or(Error::Capacity)?;
                }
            }
        }
        Ok(Self { transform, length })
    }
    pub(crate) fn len(&self) -> usize {
        self.length
    }
    pub(crate) fn decode(self, scratch: &mut [u16]) -> Result<Path<'_>, Error> {
        if self.length > scratch.len() {
            return Err(Error::Capacity);
        }
        let mut index = 0;
        for op in self.transform {
            if let rpc_capnp::promised_answer::op::GetPointerField(field) =
                op.which().expect("validated immutable transform")
            {
                scratch[index] = field;
                index += 1;
            }
        }
        Ok(Path {
            fields: &scratch[..self.length],
        })
    }
}
impl<'a> Path<'a> {
    /// Explicit wire boundary. Values are pointer-section offsets, not ordinals.
    pub const fn new(fields: &'a [u16]) -> Self {
        Self { fields }
    }
    pub const fn fields(self) -> &'a [u16] {
        self.fields
    }
    /// Decode every operation before modifying scratch. Unknown operations and
    /// capacity failure cannot turn a partially decoded path into authority.
    pub fn decode(
        source: rpc_capnp::promised_answer::Reader<'_>,
        scratch: &'a mut [u16],
    ) -> Result<Self, Error> {
        PreparedPath::new(source)?.decode(scratch)
    }
    pub(crate) fn required(source: rpc_capnp::promised_answer::Reader<'_>) -> Result<usize, Error> {
        Ok(PreparedPath::new(source)?.len())
    }
    /// Apply to retained result content under its message reader limits. The
    /// returned index still belongs to that result's capability table.
    pub fn resolve(self, mut content: capnp::any_pointer::Reader<'_>) -> capnp::Result<u32> {
        for &field in self.fields {
            content = content.pointer_field(field)?;
        }
        content.capability_index()
    }
    pub fn write(
        self,
        mut output: capnp::field_api::FixedStruct<'_, rpc_capnp::api::PromisedAnswer>,
        id: QuestionId,
    ) -> capnp::Result<()> {
        use rpc_capnp::api::{promised_answer::Op, PromisedAnswer};
        output.scalar(PromisedAnswer::QUESTION_ID).set(id.to_wire());
        let mut operations = output
            .field(PromisedAnswer::TRANSFORM)
            .init(self.fields.len())?;
        for (i, &field) in self.fields.iter().enumerate() {
            operations
                .get_mut(i)
                .expect("allocated transform length")
                .scalar(Op::GET_POINTER_FIELD)
                .set(field);
        }
        Ok(())
    }
}

/// Ownership acquired while admitting an incoming Call. Release its lease after
/// dispatch/routing, or retain it in caller storage until the answer resolves.
/// These are local leases, not additional peer reference grants.
#[must_use = "release the target lease through its originating table"]
pub enum Target<'exports, 'answers, 'path> {
    Hosted(exports::Lease<'exports>),
    Promised {
        answer: answers::Pipeline<'answers>,
        path: Path<'path>,
    },
}
pub fn admit<'exports, 'answers, 'path, E, A>(
    target: rpc_capnp::message_target::Reader<'_>,
    exports: &mut exports::Exports<'exports, '_, E>,
    answers: &mut answers::Answers<'answers, '_, A>,
    scratch: &'path mut [u16],
) -> Result<Target<'exports, 'answers, 'path>, Error> {
    match target.which().map_err(|_| Error::UnknownTransform)? {
        rpc_capnp::message_target::ImportedCap(id) => Ok(Target::Hosted(
            exports
                .lease(ExportId::from_wire(id))
                .map_err(Error::Export)?,
        )),
        rpc_capnp::message_target::PromisedAnswer(p) => {
            let p = p?;
            let path = Path::decode(p, scratch)?;
            let answer = answers
                .pipeline(AnswerId::from_wire(p.get_question_id()))
                .map_err(Error::Answer)?;
            Ok(Target::Promised { answer, path })
        }
    }
}

#[cfg(test)]
mod tests;
