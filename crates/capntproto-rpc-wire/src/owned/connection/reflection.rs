//! Reflect exact retained routes instead of exporting another forwarding proxy.
//! Callers keep the graph clients alive through ordered output acceptance. The
//! borrowed transforms and copied IDs never escape that publication transaction.
use super::*;
use crate::rpc_capnp::api::CapDescriptor;

// One staging table covers both sender grants and reflected routes. Growing
// storage needs no second allocation when a payload contains capabilities.
#[derive(Clone, Copy)]
pub(super) enum Descriptor<'a> {
    None,
    Grant(capntproto_rpc_engine::ids::ExportId, bool, u8),
    Reflected(calls::Target<'a>),
}
impl Descriptor<'_> {
    pub(super) fn reflected(&self) -> Option<calls::Target<'_>> {
        match self {
            Self::Reflected(target) => Some(*target),
            _ => None,
        }
    }
}

pub(crate) fn target<'a, 'id, P: Copy + Eq, S, const PATH: usize>(
    owner: &'a binding::Route<'id, P, S, PATH>,
    peer: P,
    questions: &questions::Questions<'id, '_>,
) -> Result<Option<calls::Target<'a>>, questions::Error> {
    match owner {
        binding::Route::Capability(binding::Endpoint::Imported(remote))
            if remote.peer() == peer =>
        {
            Ok(Some(calls::Target::Imported(remote.id())))
        }
        binding::Route::Question(route) if route.peer() == peer => {
            let (question, path) = route.target(questions)?;
            Ok(Some(calls::Target::Promised { question, path }))
        }
        _ => Ok(None),
    }
}

pub(crate) fn write(
    target: calls::Target<'_>,
    mut descriptor: capnp::field_api::FixedStruct<'_, CapDescriptor>,
) -> capnp::Result<()> {
    match target {
        calls::Target::Imported(id) => descriptor
            .scalar(CapDescriptor::RECEIVER_HOSTED)
            .set(id.to_wire()),
        calls::Target::Promised { question, path } => {
            path.write(
                descriptor.field(CapDescriptor::RECEIVER_ANSWER).init()?,
                question,
            )?;
        }
    }
    Ok(())
}

pub(crate) fn check(
    target: Option<calls::Target<'_>>,
    descriptor: rpc_capnp::cap_descriptor::Reader<'_>,
) -> Result<bool, crate::publication::Error> {
    use crate::publication::Error;
    match (
        target,
        descriptor
            .which()
            .map_err(|_| Error::UnsupportedDescriptor)?,
    ) {
        (None, _) => Ok(false),
        (
            Some(calls::Target::Imported(expected)),
            rpc_capnp::cap_descriptor::ReceiverHosted(id),
        ) if expected.to_wire() == id => Ok(true),
        (
            Some(calls::Target::Promised { question, path }),
            rpc_capnp::cap_descriptor::ReceiverAnswer(answer),
        ) => {
            let answer = answer?;
            if answer.get_question_id() != question.to_wire() {
                return Err(Error::ReflectionMismatch);
            }
            crate::publication::reflected::check_path(answer, path)?;
            Ok(true)
        }
        _ => Err(Error::ReflectionMismatch),
    }
}
