//! Keep the exact sent Return in its answer owner through Finish and retained
//! pipelines. Callers cannot substitute another frame when applying releases.
use super::*;
use crate::publication::ResultExports;

#[cfg(test)]
mod tests;

pub struct State<'pool, A> {
    pub owner: A,
    frame: Option<Frame<'pool>>,
    // Installed only together with the exact validated, accepted Return.
    result_exports: ResultExports,
}
impl<'pool, A> State<'pool, A> {
    pub fn new(owner: A) -> Self {
        Self {
            owner,
            frame: None,
            result_exports: ResultExports::Serialized,
        }
    }
    pub fn with_result<R>(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
        f: impl FnOnce(rpc_capnp::message::Reader<'_>) -> R,
    ) -> Result<Option<R>, Error> {
        self.frame
            .as_ref()
            .map(|frame| frame.with(messages, f))
            .transpose()
    }
    /// Drain only after the answer ledger detaches this owner. Previously
    /// admitted pipeline/adoption leases prevent premature frame reclamation.
    pub fn release(self, messages: &mut messages::Messages<'pool, '_, '_>) -> A {
        if let Some(frame) = self.frame {
            frame.release(messages);
        }
        self.owner
    }
}

/// An unaccepted reply still owns its exact bytes and all tentative grants.
/// Retry it after draining output, or explicitly release the frame and abort
/// the batch. Never rebuild application results merely because output is full.
#[must_use = "retry the reply, or release its frame and abort its grants"]
pub struct Rejected<'pool, B = ()> {
    pub error: Error,
    pub frame: Frame<'pool>,
    pub grants: B,
}
impl<'pool, B> Rejected<'pool, B> {
    pub fn into_parts(self) -> (Error, Frame<'pool>, B) {
        (self.error, self.frame, self.grants)
    }
}

/// Publish a completed reply, preserving it on every admission error.
pub fn try_send<'pool, 'a, A>(
    key: answers::Key<'a>,
    answers: &mut answers::Answers<'a, '_, State<'pool, A>>,
    messages: &mut messages::Messages<'pool, '_, '_>,
    frame: Frame<'pool>,
    queue: messages::QueueSlot<'_, 'pool, '_, impl Sized>,
) -> Result<Option<State<'pool, A>>, Rejected<'pool>> {
    let mut pending = match answers.prepare_return(key) {
        Ok(pending) => pending,
        Err(error) => {
            return Err(Rejected {
                error: Error::Admission(inbound::Error::Answer(error)),
                frame,
                grants: (),
            })
        }
    };
    let prepared = (|| {
        frame
            .with(messages, |root| {
                crate::admission::check_return(&mut pending, root, false)
            })?
            .map_err(Error::Output)?;
        messages.offer(&frame.lease).map_err(Error::Storage)
    })();
    let offer = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            return Err(Rejected {
                error,
                frame,
                grants: (),
            })
        }
    };
    let retired = pending.accept();
    queue.accept(messages, offer);
    Ok(keep_frame(
        key,
        answers,
        retired,
        frame,
        ResultExports::Empty,
    ))
}

/// Convenience for callers which discard replies on rejection. Use `try_send`
/// when send-capacity rejection should be retried without rerunning the service.
pub fn send<'pool, 'a, A>(
    key: answers::Key<'a>,
    answers: &mut answers::Answers<'a, '_, State<'pool, A>>,
    messages: &mut messages::Messages<'pool, '_, '_>,
    frame: Frame<'pool>,
    queue: messages::QueueSlot<'_, 'pool, '_, impl Sized>,
) -> Result<Option<State<'pool, A>>, Error> {
    try_send(key, answers, messages, frame, queue).map_err(|rejected| {
        rejected.frame.release(messages);
        rejected.error
    })
}

/// Internal fast path for a privately built data-only result whose immutable
/// frame was validated under the current reader limits. The caller checks the
/// certificate's answer ID and no-Finish bit before selecting this path.
pub(super) fn send_prepared<'pool, 'a, A>(
    key: answers::Key<'a>,
    answers: &mut answers::Answers<'a, '_, State<'pool, A>>,
    messages: &mut messages::Messages<'pool, '_, '_>,
    frame: Frame<'pool>,
    no_finish: bool,
    queue: messages::QueueSlot<'_, 'pool, '_, impl Sized>,
) -> Result<Option<State<'pool, A>>, Error> {
    let result = (|| {
        let mut pending = answers
            .prepare_return(key)
            .map_err(|e| Error::Admission(inbound::Error::Answer(e)))?;
        if pending.is_cancelled() || pending.is_redirected() {
            return Err(Error::WrongMessage);
        }
        if no_finish {
            pending
                .omit_finish()
                .map_err(|e| Error::Admission(inbound::Error::Answer(e)))?;
        }
        let offer = messages.offer(&frame.lease).map_err(Error::Storage)?;
        let retired = pending.accept();
        queue.accept(messages, offer);
        Ok(retired)
    })();
    match result {
        Ok(retired) => Ok(keep_frame(
            key,
            answers,
            retired,
            frame,
            ResultExports::Empty,
        )),
        Err(error) => {
            frame.release(messages);
            Err(error)
        }
    }
}

/// Retryable capability-bearing reply admission. Validation and capacity
/// reservation leave the batch intact on failure, including its exact wire IDs.
/// Acceptance commits grants, answer state and FIFO output without callbacks.
#[allow(clippy::too_many_arguments)]
pub fn try_send_granting<'pool, 'a, 'batch, 'e, 'table, A, E>(
    key: answers::Key<'a>,
    answers: &mut answers::Answers<'a, '_, State<'pool, A>>,
    batch: exports::Batch<'batch, 'e, 'table, E>,
    messages: &mut messages::Messages<'pool, '_, '_>,
    frame: Frame<'pool>,
    scratch: &mut [u32],
    queue: messages::QueueSlot<'_, 'pool, '_, impl Sized>,
) -> Result<Option<State<'pool, A>>, Rejected<'pool, exports::Batch<'batch, 'e, 'table, E>>> {
    let mut pending = match answers.prepare_return(key) {
        Ok(pending) => pending,
        Err(error) => {
            return Err(Rejected {
                error: Error::Admission(inbound::Error::Answer(error)),
                frame,
                grants: batch,
            })
        }
    };
    let prepared = (|| {
        let result_exports = frame.with(messages, |root| {
            crate::admission::check_return(&mut pending, root, true).map_err(Error::Output)?;
            crate::publication::validate_root(&batch, root, scratch, |_, _| Ok(false))
                .map_err(|error| Error::Output(error.into()))
        })??;
        let offer = messages.offer(&frame.lease).map_err(Error::Storage)?;
        Ok((offer, result_exports))
    })();
    let (offer, result_exports) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            return Err(Rejected {
                error,
                frame,
                grants: batch,
            })
        }
    };
    batch.commit();
    let retired = pending.accept();
    queue.accept(messages, offer);
    Ok(keep_frame(key, answers, retired, frame, result_exports))
}

#[allow(clippy::too_many_arguments)]
pub fn send_granting<'pool, 'a, A, E>(
    key: answers::Key<'a>,
    answers: &mut answers::Answers<'a, '_, State<'pool, A>>,
    batch: exports::Batch<'_, '_, '_, E>,
    messages: &mut messages::Messages<'pool, '_, '_>,
    frame: Frame<'pool>,
    scratch: &mut [u32],
    queue: messages::QueueSlot<'_, 'pool, '_, impl Sized>,
) -> Result<Option<State<'pool, A>>, Error> {
    try_send_granting(key, answers, batch, messages, frame, scratch, queue).map_err(|rejected| {
        rejected.frame.release(messages);
        rejected.error
    })
}

fn keep_frame<'pool, 'a, A>(
    key: answers::Key<'a>,
    answers: &mut answers::Answers<'a, '_, State<'pool, A>>,
    mut retired: Option<State<'pool, A>>,
    frame: Frame<'pool>,
    result_exports: ResultExports,
) -> Option<State<'pool, A>> {
    let state = if let Some(retired) = retired.as_mut() {
        retired
    } else {
        answers
            .get_mut(key)
            .expect("accepted answer is retained until Finish")
    };
    debug_assert!(state.frame.is_none());
    state.frame = Some(frame);
    state.result_exports = result_exports;
    retired
}

pub fn finished<'pool, 'a, 'e, A, E>(
    frame: &Frame<'pool>,
    messages: &mut messages::Messages<'pool, '_, '_>,
    answers: &mut answers::Answers<'a, '_, State<'pool, A>>,
    exports: &mut exports::Exports<'e, '_, E>,
    scratch: &mut [u32],
) -> Result<Option<answers::Finished<State<'pool, A>>>, Error> {
    let (id, release_result_caps) = frame.with(messages, |root| {
        let rpc_capnp::message::Finish(finish) = root.which().map_err(|_| Error::WrongMessage)?
        else {
            return Err(Error::WrongMessage);
        };
        let finish = finish?;
        Ok::<_, Error>((
            AnswerId::from_wire(finish.get_question_id()),
            finish.get_release_result_caps(),
        ))
    })??;
    finished_parts(id, release_result_caps, messages, answers, exports, scratch)
}

pub(super) fn finished_parts<'pool, 'a, 'e, A, E>(
    id: AnswerId,
    release_result_caps: bool,
    messages: &mut messages::Messages<'pool, '_, '_>,
    answers: &mut answers::Answers<'a, '_, State<'pool, A>>,
    exports: &mut exports::Exports<'e, '_, E>,
    scratch: &mut [u32],
) -> Result<Option<answers::Finished<State<'pool, A>>>, Error> {
    let result_exports = answers
        .lookup(id)
        .and_then(|key| answers.get(key).ok())
        .map(|state| state.result_exports);
    if let Some(result_exports @ (ResultExports::Empty | ResultExports::One { .. })) =
        result_exports
    {
        let Some(pending) = answers.prepare_finish(id, release_result_caps) else {
            return Ok(None);
        };
        if pending.release_results() {
            if let ResultExports::One { id, count } = result_exports {
                exports.release_deferred(id, count).map_err(|error| {
                    Error::Admission(inbound::Error::Release(exports::ReleaseError::Export(
                        error,
                    )))
                })?;
            }
        }
        return Ok(Some(pending.accept()));
    }
    let original = match answers
        .lookup(id)
        .and_then(|key| answers.get(key).ok())
        .and_then(|s| s.frame.as_ref())
    {
        Some(original) => Some(Frame::new(
            messages.retain(&original.lease).map_err(Error::Storage)?,
            original.options,
        )),
        None => None,
    };
    let mut apply = |original: Option<rpc_capnp::message::Reader<'_>>| {
        inbound::finished_parts(id, release_result_caps, original, answers, exports, scratch)
            .map(|pending| pending.accept())
            .map_err(Error::Admission)
    };
    let result = match &original {
        Some(original) => original
            .with(messages, |reader| apply(Some(reader)))
            .and_then(core::convert::identity),
        None => apply(None),
    };
    if let Some(original) = original {
        original.release(messages);
    }
    result
}

/// Publish an owning Return containing reflected and/or sender-owned results.
/// Rejection preserves its exact bytes, export grants and reflection receipts.
/// Acceptance places the Return before any Release/Finish made eligible by
/// consuming those receipts.
#[allow(clippy::too_many_arguments)]
pub fn try_send_reflected<'pool, 'a, 'batch, 'e, 'table, A, E, I>(
    key: answers::Key<'a>,
    answers: &mut answers::Answers<'a, '_, State<'pool, A>>,
    batch: exports::Batch<'batch, 'e, 'table, E>,
    messages: &mut messages::Messages<'pool, '_, '_>,
    frame: Frame<'pool>,
    scratch: &mut [u32],
    references: &mut crate::publication::reflected::References<'_, '_, '_, '_, '_, '_, I>,
    queue: messages::QueueSlot<'_, 'pool, '_, impl Sized>,
) -> Result<Option<State<'pool, A>>, Rejected<'pool, exports::Batch<'batch, 'e, 'table, E>>> {
    let mut pending = match answers.prepare_return(key) {
        Ok(pending) => pending,
        Err(error) => {
            return Err(Rejected {
                error: Error::Admission(inbound::Error::Answer(error)),
                frame,
                grants: batch,
            })
        }
    };
    let prepared = (|| {
        let mut count = 0;
        let result_exports = frame.with(messages, |root| {
            crate::admission::check_return(&mut pending, root, true).map_err(Error::Output)?;
            crate::publication::validate_root(&batch, root, scratch, |descriptor, index| {
                count += 1;
                let reference = references
                    .slots
                    .get(index)
                    .ok_or(crate::publication::Error::ReflectionMismatch)?;
                crate::publication::reflected::check_reference(
                    descriptor,
                    reference,
                    references.imports,
                    references.questions,
                )
            })
            .map_err(|error| Error::Output(error.into()))
        })??;
        if count != references.slots.len() {
            return Err(Error::Output(
                crate::publication::Error::ReflectionMismatch.into(),
            ));
        }
        let offer = messages.offer(&frame.lease).map_err(Error::Storage)?;
        Ok((offer, result_exports))
    })();
    let (offer, result_exports) = match prepared {
        Ok(offer) => offer,
        Err(error) => {
            return Err(Rejected {
                error,
                frame,
                grants: batch,
            })
        }
    };
    batch.commit();
    let retired = pending.accept();
    queue.accept(messages, offer);
    crate::publication::reflected::release_references(
        references.slots,
        references.imports,
        references.questions,
    );
    Ok(keep_frame(key, answers, retired, frame, result_exports))
}

mod tail;
pub use tail::try_send_tail;
