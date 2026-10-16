//! Result reception composed with implicit releases of the original parameters.
use super::*;
#[must_use]
pub struct WithReleases<'b, 's, 'r, 'm, 'i, 'e, 'a, 'is, 'es, 'as_, I, E, A> {
    imports: imports::Receive<'b, 'r, 'i, 'is, I>,
    exports: exports::LeasedReleases<'b, 's, 'r, 'e, 'es, E>,
    answers: answers::Pipelines<'b, 'r, 'a, 'as_, A>,
    payload: rpc_capnp::payload::Reader<'m>,
}
#[allow(clippy::type_complexity)]
pub fn prepare_with_releases<'b, 's, 'r, 'm, 'i, 'e, 'a, 'is, 'es, 'as_, I, E, A>(
    payload: rpc_capnp::payload::Reader<'m>,
    imports: &'b mut imports::Imports<'i, 'is, I>,
    releases: exports::Releases<'b, 's, 'e, 'es, E>,
    answers: &'b mut answers::Answers<'a, 'as_, A>,
    slots: Slots<'r, 'i, 'e, 'a, I>,
) -> Result<WithReleases<'b, 's, 'r, 'm, 'i, 'e, 'a, 'is, 'es, 'as_, I, E, A>, Error> {
    validate(payload, &slots)?;
    prepare_validated_with_releases(payload, imports, releases, answers, slots)
}

#[allow(clippy::type_complexity)]
pub(crate) fn prepare_checked_with_releases<'b, 's, 'r, 'm, 'i, 'e, 'a, 'is, 'es, 'as_, I, E, A>(
    payload: CheckedPayload<'m>,
    imports: &'b mut imports::Imports<'i, 'is, I>,
    releases: exports::Releases<'b, 's, 'e, 'es, E>,
    answers: &'b mut answers::Answers<'a, 'as_, A>,
    slots: Slots<'r, 'i, 'e, 'a, I>,
) -> Result<WithReleases<'b, 's, 'r, 'm, 'i, 'e, 'a, 'is, 'es, 'as_, I, E, A>, Error> {
    payload.validate_slots(&slots)?;
    prepare_validated_with_releases(payload.payload, imports, releases, answers, slots)
}

#[allow(clippy::type_complexity)]
fn prepare_validated_with_releases<'b, 's, 'r, 'm, 'i, 'e, 'a, 'is, 'es, 'as_, I, E, A>(
    payload: rpc_capnp::payload::Reader<'m>,
    imports: &'b mut imports::Imports<'i, 'is, I>,
    releases: exports::Releases<'b, 's, 'e, 'es, E>,
    answers: &'b mut answers::Answers<'a, 'as_, A>,
    slots: Slots<'r, 'i, 'e, 'a, I>,
) -> Result<WithReleases<'b, 's, 'r, 'm, 'i, 'e, 'a, 'is, 'es, 'as_, I, E, A>, Error> {
    let imports = imports
        .prepare_receive(slots.imports)
        .map_err(Error::Imports)?;
    let exports = releases
        .with_leases(slots.exports)
        .map_err(Error::Exports)?;
    let answers = answers
        .prepare_pipelines(slots.answers)
        .map_err(Error::Answers)?;
    Ok(WithReleases {
        imports,
        exports,
        answers,
        payload,
    })
}
impl<'r, 'm, 'i, 'e, 'a, I, E, A> WithReleases<'_, '_, 'r, 'm, 'i, 'e, 'a, '_, '_, '_, I, E, A> {
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
