//! Join replies retain the caller's network registration owner until Finish.
use super::*;

#[must_use = "retry completion or release the original Join request"]
pub struct JoinRejected<'pool, 'id, const PATH: usize> {
    pub error: Error,
    pub join: super::super::join::Join<'pool, 'id, 'id, PATH>,
}
impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// Return an authenticated network's Join result. The network's registration
    /// guard belongs in the answer's producer; Finish returns that guard only
    /// after the reply and any accepted pipeline references are done. The
    /// metadata builder must only serialize the already-prepared network result.
    /// No metadata callback runs when the peer already canceled or output is full.
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn complete_join(
        &mut self,
        join: super::super::join::Join<'pool, 'id, 'id, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        metadata: impl FnOnce(
            capnp::field_api::FixedField<'_, capnp::field_api::Generic<capnp::any_pointer::Owned>>,
        ) -> capnp::Result<()>,
    ) -> Result<Option<A>, JoinRejected<'pool, 'id, PATH>> {
        self.finish_join(
            join,
            messages,
            queue,
            minimum,
            options,
            true,
            |builder, id| crate::network::join_result(builder, id, metadata),
        )
    }
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn reject_join(
        &mut self,
        join: super::super::join::Join<'pool, 'id, 'id, PATH>,
        error: capnp::ErrorKind,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
    ) -> Result<Option<A>, JoinRejected<'pool, 'id, PATH>> {
        self.finish_join(
            join,
            messages,
            queue,
            minimum,
            options,
            false,
            |builder, id| calls::exception(builder, id, error, "network Join failed"),
        )
    }
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    fn finish_join(
        &mut self,
        join: super::super::join::Join<'pool, 'id, 'id, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        keep_target: bool,
        build: impl FnOnce(&mut capnp::message::FixedBuilder<'_>, AnswerId) -> capnp::Result<()>,
    ) -> Result<Option<A>, JoinRejected<'pool, 'id, PATH>> {
        let key = join.key();
        let mut authority = None;
        let mut observer = false;
        let outcome = (|| {
            self.check_live()?;
            if !self
                .ledgers
                .answers
                .get(key)
                .map_err(|e| Error::Pipeline(pipelines::Error::Answer(e)))?
                .owner
                .is_pending()
            {
                return Err(Error::Pipeline(pipelines::Error::AlreadyCompleted));
            }
            let canceled = self
                .ledgers
                .answers
                .prepare_return(key)
                .map_err(|e| Error::Pipeline(pipelines::Error::Answer(e)))?
                .is_cancelled();
            let output = queue
                .reserve()
                .map_err(|e| Error::Input(super::super::Error::Storage(e)))?;
            if !canceled && keep_target {
                authority = Some(match join.target() {
                    TargetRef::Hosted(lease) => {
                        binding::reflected(&mut self.runtime, &self.ledgers.exports, lease)
                            .map_err(Error::Graph)?
                    }
                    TargetRef::Promised { answer, path } => {
                        let client = self
                            .pipelines
                            .bind(&mut self.runtime, &mut self.ledgers.answers, answer, path)
                            .map_err(Error::Pipeline)?;
                        observer = true;
                        client
                    }
                });
            }
            let frame = Frame::build(messages, minimum, options, |builder| {
                if canceled {
                    calls::cancelled(builder, key.id())
                } else {
                    build(builder, key.id())
                }
            })
            .map_err(Error::Input)?;
            let retired = answer::try_send(key, &mut self.ledgers.answers, messages, frame, output)
                .map_err(|rejected| {
                    rejected.frame.release(messages);
                    Error::Input(rejected.error)
                })?;
            if retired.is_none() {
                self.ledgers
                    .answers
                    .get_mut(key)
                    .expect("preflighted Join answer")
                    .owner
                    .network_authority = authority.take();
                // JoinResult is network metadata, never a capability payload.
                // Any admitted result-capability pipeline must fail, including
                // after early Finish retained the answer on its behalf.
                pipelines::complete(
                    &mut self.ledgers.answers,
                    key,
                    Err(capnp::ErrorKind::Failed),
                )
                .unwrap_or_else(|_| unreachable!("pending Join held through publication"));
            }
            Ok(retired.map(|state| state.release(messages).release(messages, &mut self.runtime)))
        })();
        match outcome {
            Ok(retired) => {
                join.release(
                    messages,
                    &mut self.ledgers.exports,
                    &mut self.ledgers.answers,
                );
                Ok(retired)
            }
            Err(error) => {
                if let Some(client) = authority {
                    let key = client.key();
                    self.runtime.release(client);
                    if observer {
                        self.pipelines
                            .discard_unexposed(key, &mut self.ledgers.answers);
                    }
                }
                Err(JoinRejected { error, join })
            }
        }
    }
}
