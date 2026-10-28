//! Own a Join part and its target until caller-provided network processing ends.
use super::*;
#[must_use = "complete the Join and release its target receipts"]
pub struct Join<'pool, 'e, 'a, const PATH: usize> {
    frame: Frame<'pool>,
    key: answers::Key<'a>,
    target: Target<'e, 'a>,
    fields: Buffer<u16, PATH>,
    length: usize,
}
impl<'pool, 'e, 'a, const PATH: usize> Join<'pool, 'e, 'a, PATH> {
    #[allow(clippy::result_large_err)]
    pub fn admit<E, A>(
        frame: Frame<'pool>,
        messages: &messages::Messages<'pool, '_, '_>,
        exports: &mut exports::Exports<'e, '_, E>,
        answers: &mut answers::Answers<'a, '_, A>,
        owner: A,
    ) -> Result<Self, Rejected<'pool, A>> {
        Self::admit_with_capacity(frame, messages, exports, answers, owner, Capacity::Inline)
    }
    #[allow(clippy::result_large_err)]
    pub fn admit_with_capacity<E, A>(
        frame: Frame<'pool>,
        messages: &messages::Messages<'pool, '_, '_>,
        exports: &mut exports::Exports<'e, '_, E>,
        answers: &mut answers::Answers<'a, '_, A>,
        owner: A,
        capacity: Capacity,
    ) -> Result<Self, Rejected<'pool, A>> {
        let mut fields = Buffer::Inline([0; PATH]);
        let mut local = [exports::LeaseSlot::none()];
        let mut promised = [answers::PipelineSlot::none()];
        let mut owner = Some(owner);
        let admission = frame
            .with(messages, |root| {
                let rpc_capnp::message::Join(join) =
                    root.which().map_err(|_| Error::WrongMessage)?
                else {
                    return Err(Error::WrongMessage);
                };
                let join = join?;
                prepare_target_path(join.get_target()?, &mut fields, capacity)?;
                if join.get_question_id() >= 1 << 30 {
                    return Err(Error::Admission(inbound::Error::Unsupported));
                }
                join.get_key_part().validate_capability_indices(0)?;
                let length = match join
                    .get_target()?
                    .which()
                    .map_err(|_| Error::WrongMessage)?
                {
                    rpc_capnp::message_target::ImportedCap(id) => {
                        local[0] = exports::LeaseSlot::new(ExportId::from_wire(id));
                        0
                    }
                    rpc_capnp::message_target::PromisedAnswer(answer) => {
                        let answer = answer?;
                        let path =
                            pipeline::Path::decode(answer, &mut fields).map_err(Error::Target)?;
                        promised[0] = answers::PipelineSlot::new(AnswerId::from_wire(
                            answer.get_question_id(),
                        ));
                        path.fields().len()
                    }
                };
                let leases = exports.prepare_leases(&mut local).map_err(|e| {
                    Error::Admission(inbound::Error::Reception(reception::Error::Exports(e)))
                })?;
                let admission = answers
                    .prepare_admission_with_pipelines(
                        AnswerId::from_wire(join.get_question_id()),
                        &mut promised,
                    )
                    .map_err(|e| {
                        Error::Admission(inbound::Error::Reception(reception::Error::Answers(e)))
                    })?;
                let (key, promised) = admission.require_finish().accept(owner.take().unwrap());
                let local = leases.accept();
                let target = if let Some(lease) = local[0].take() {
                    Target::Hosted(lease)
                } else {
                    Target::Promised(promised[0].take().unwrap())
                };
                Ok((key, target, length))
            })
            .and_then(core::convert::identity);
        match admission {
            Ok((key, target, length)) => Ok(Self {
                frame,
                key,
                target,
                fields,
                length,
            }),
            Err(error) => Err(Rejected {
                error,
                frame,
                owner: owner.unwrap(),
            }),
        }
    }
    pub fn key(&self) -> answers::Key<'a> {
        self.key
    }
    pub fn target(&self) -> TargetRef<'_, 'e, 'a> {
        match &self.target {
            Target::Hosted(lease) => TargetRef::Hosted(lease),
            Target::Promised(answer) => TargetRef::Promised {
                answer,
                path: pipeline::Path::new(&self.fields[..self.length]),
            },
        }
    }
    /// Inspect only the metadata belonging to this admitted request. The caller
    /// must verify its authenticated key_part before granting a completion.
    pub fn with_key_part<R>(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
        f: impl FnOnce(capnp::any_pointer::Reader<'_>) -> R,
    ) -> Result<R, Error> {
        self.frame.with(messages, |root| {
            let rpc_capnp::message::Join(join) = root.which().map_err(|_| Error::WrongMessage)?
            else {
                return Err(Error::WrongMessage);
            };
            Ok(f(join?.get_key_part()))
        })?
    }
    pub fn release<E, A>(
        self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        exports: &mut exports::Exports<'e, '_, E>,
        answers: &mut answers::Answers<'a, '_, A>,
    ) {
        self.frame.release(messages);
        match self.target {
            Target::Hosted(lease) => exports.release_local_deferred(lease),
            Target::Promised(answer) => answers.release_pipeline_deferred(answer),
        }
    }
}
