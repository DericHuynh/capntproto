//! Input control messages with failure-atomic ownership accounting.
use super::*;

#[must_use]
pub struct Finished<'b, 's, 'a, 'e, 'as_, 'es, A, E> {
    answer: Option<answers::Finish<'b, 'a, 'as_, A>>,
    releases: Option<exports::Releases<'b, 's, 'e, 'es, E>>,
}
/// Bind Finish to the exact retained Return (if already sent) and every result
/// grant it releases. A malformed/over-released result table preserves the
/// answer, cancellation policy and all export counts. Unknown/duplicate Finish
/// ignores the supplied original entirely, preventing repeated cleanup.
#[allow(clippy::type_complexity)]
pub fn finished<'b, 's, 'a, 'e, 'as_, 'es, A, E>(
    finish: rpc_capnp::finish::Reader<'_>,
    original: Option<rpc_capnp::message::Reader<'_>>,
    answers: &'b mut answers::Answers<'a, 'as_, A>,
    exports: &'b mut exports::Exports<'e, 'es, E>,
    scratch: &'s mut [u32],
) -> Result<Finished<'b, 's, 'a, 'e, 'as_, 'es, A, E>, Error> {
    let Some(answer) = answers.prepare_finish(
        AnswerId::from_wire(finish.get_question_id()),
        finish.get_release_result_caps(),
    ) else {
        return Ok(Finished {
            answer: None,
            releases: None,
        });
    };
    if answer.returned() != original.is_some() {
        return Err(Error::Mismatch);
    }
    let payload = if let Some(original) = original {
        let rpc_capnp::message::Return(result) =
            original.which().map_err(|_| Error::Unsupported)?
        else {
            return Err(Error::Mismatch);
        };
        let result = result?;
        if result.get_answer_id() != answer.id().to_wire() {
            return Err(Error::Mismatch);
        }
        match result.which().map_err(|_| Error::Unsupported)? {
            rpc_capnp::return_::Results(payload) => Some(payload?),
            rpc_capnp::return_::Exception(_)
            | rpc_capnp::return_::Canceled(())
            | rpc_capnp::return_::TakeFromOtherQuestion(_)
            | rpc_capnp::return_::ResultsSentElsewhere(())
            | rpc_capnp::return_::AwaitFromThirdParty(_) => None,
        }
    } else {
        None
    };
    let mut releases = exports.prepare_releases(scratch).map_err(Error::Release)?;
    if answer.release_results() {
        super::returns::record_params(&mut releases, payload)?;
    }
    let releases = releases.finish().map_err(Error::Release)?;
    Ok(Finished {
        answer: Some(answer),
        releases: Some(releases),
    })
}
impl<A, E> Finished<'_, '_, '_, '_, '_, '_, A, E> {
    pub fn is_ignored(&self) -> bool {
        self.answer.is_none()
    }
    /// None denotes an unknown/already-finished answer. Retired result exports
    /// remain in their ledger for explicit draining outside all table borrows.
    pub fn accept(self) -> Option<answers::Finished<A>> {
        if let Some(releases) = self.releases {
            releases.accept();
        }
        self.answer.map(|answer| answer.accept())
    }
}

/// Validate one explicit Release without dropping the export owner during the
/// ledger transition. The caller drains any retired owner afterward.
pub fn released<'b, 's, 'e, 'storage, E>(
    release: rpc_capnp::release::Reader<'_>,
    exports: &'b mut exports::Exports<'e, 'storage, E>,
    scratch: &'s mut [u32],
) -> Result<exports::Releases<'b, 's, 'e, 'storage, E>, Error> {
    let mut pending = exports.prepare_releases(scratch).map_err(Error::Release)?;
    pending
        .record(
            ExportId::from_wire(release.get_id()),
            release.get_reference_count(),
        )
        .map_err(Error::Release)?;
    pending.finish().map_err(Error::Release)
}

#[cfg(test)]
mod tests;
