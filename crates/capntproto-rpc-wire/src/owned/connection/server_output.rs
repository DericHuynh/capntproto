//! Service completion and Return publication share the answer's retained result.
//! Output pressure never re-executes the service or consumes an unpublished result.
use super::*;
use crate::rpc_capnp::api::{CapDescriptor, Exception, Message, Payload, Return};
use capntproto_rpc_engine::clients::Capabilities;
#[cfg(test)]
mod tests;

impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// Honor an admitted pipeline-only hint after local completion. The result
    /// remains available to promised-answer calls until Finish, without export
    /// grants, a serialized Return, or an output-queue reservation.
    pub fn send_pipeline_only_answer(
        &mut self,
        request: &Request<'pool, 'id, I, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> Result<Option<A>, Error> {
        self.check_live()?;
        let key = request.answer();
        if !request.method().only_promise_pipeline {
            return Err(Error::Input(super::super::Error::WrongMessage));
        }
        if self
            .ledgers
            .answers
            .get(key)
            .map_err(|error| Error::Pipeline(pipelines::Error::Answer(error)))?
            .owner
            .is_pending()
        {
            return Err(Error::PendingResponse);
        }
        let pending = self
            .ledgers
            .answers
            .prepare_return(key)
            .map_err(|error| Error::Pipeline(pipelines::Error::Answer(error)))?;
        if pending.is_redirected() {
            return Err(Error::InvalidNetworkTarget);
        }
        let retired = pending.accept();
        if retired.is_none() {
            self.ledgers
                .answers
                .get_mut(key)
                .expect("retained pipeline-only answer")
                .without_return();
        }
        Ok(retired.map(|answer| self.release_answer(answer, messages)))
    }

    /// Complete an admitted redirected Call after the authenticated network has
    /// retained its execution for the recipient. Publishing the original answer
    /// then emits ResultsSentElsewhere (or Canceled), never a duplicate payload.
    #[allow(clippy::result_large_err)]
    pub fn complete_transferred_answer(
        &mut self,
        request: &Request<'pool, 'id, I, CAPS, PATH>,
        result: Result<LocalResult<'pool, 'id, CAPS>, capnp::ErrorKind>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> Result<(), AnswerHandoffRejected<'pool, 'id, CAPS>> {
        let checked = (|| {
            self.check_live()?;
            let key = request.answer();
            if !self
                .ledgers
                .answers
                .is_third_party(key)
                .map_err(|e| Error::Pipeline(pipelines::Error::Answer(e)))?
            {
                return Err(Error::InvalidNetworkTarget);
            }
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
            let _ = self
                .ledgers
                .answers
                .prepare_return(key)
                .map_err(|e| Error::Pipeline(pipelines::Error::Answer(e)))?;
            if let Ok(result) = &result {
                let method = (request.method().interface, request.method().method);
                if method != (result.interface, result.method) {
                    return Err(Error::InvalidNetworkTarget);
                }
            }
            Ok(())
        })();
        if let Err(error) = checked {
            return Err(AnswerHandoffRejected { error, result });
        }
        self.complete_local_answer(request.answer(), result, messages)
    }
    /// Retain a completed service result in its exact answer generation. This
    /// also makes preexisting answer pipelines resolvable before output capacity
    /// is available. Finish does not discard results needed by admitted children.
    /// Third-party completion uses the authenticated network handoff API instead.
    #[allow(clippy::result_large_err)]
    pub fn complete_answer(
        &mut self,
        request: &Request<'pool, 'id, I, CAPS, PATH>,
        result: Result<LocalResult<'pool, 'id, CAPS>, capnp::ErrorKind>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> Result<(), AnswerHandoffRejected<'pool, 'id, CAPS>> {
        let key = request.answer();
        let checked = (|| {
            self.check_answer_completion(key)?;
            if let Ok(result) = &result {
                let method = (request.method().interface, request.method().method);
                if method != (result.interface, result.method) {
                    return Err(Error::Input(super::super::Error::WrongMessage));
                }
            }
            Ok(())
        })();
        if let Err(error) = checked {
            return Err(AnswerHandoffRejected { error, result });
        }
        self.complete_local_answer(key, result, messages)
    }

    pub(super) fn check_answer_completion(&mut self, key: answers::Key<'id>) -> Result<(), Error> {
        self.check_live()?;
        let state = self
            .ledgers
            .answers
            .get(key)
            .map_err(|e| Error::Pipeline(pipelines::Error::Answer(e)))?;
        if !state.owner.is_pending() {
            return Err(Error::Pipeline(pipelines::Error::AlreadyCompleted));
        }
        if self
            .ledgers
            .answers
            .is_third_party(key)
            .map_err(|e| Error::Pipeline(pipelines::Error::Answer(e)))?
        {
            return Err(Error::InvalidNetworkTarget);
        }
        // A previously published network Return must use complete_network_answer.
        let _ = self
            .ledgers
            .answers
            .prepare_return(key)
            .map_err(|e| Error::Pipeline(pipelines::Error::Answer(e)))?;
        Ok(())
    }

    // Called only after checking the admitted Call/Bootstrap token. Keeping this
    // private prevents callers from completing an arbitrary answer by numeric ID.
    #[allow(clippy::result_large_err)]
    pub(super) fn complete_local_answer(
        &mut self,
        key: answers::Key<'id>,
        result: Result<LocalResult<'pool, 'id, CAPS>, capnp::ErrorKind>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> Result<(), AnswerHandoffRejected<'pool, 'id, CAPS>> {
        let checked = (|| {
            let published = match &result {
                Ok(result) => Ok(pipelines::Published::from_local(
                    result,
                    key.id(),
                    messages,
                    &mut self.runtime,
                )
                .map_err(Error::Pipeline)?),
                Err(kind) => Err(*kind),
            };
            pipelines::complete(&mut self.ledgers.answers, key, published)
                .unwrap_or_else(|_| unreachable!("preflighted service completion"));
            if let Some(adopter) = self.ledgers.answers.get(key).unwrap().owner.adopter {
                self.refresh_exchange(adopter);
            }
            Ok(())
        })();
        match checked {
            Ok(()) => {
                if let Ok(result) = result {
                    result.release(messages, &mut self.runtime);
                }
                Ok(())
            }
            Err(error) => Err(AnswerHandoffRejected { error, result }),
        }
    }

    /// Publish the completed answer's immutable content and staged capabilities.
    /// Returns a retired producer only after FIFO acceptance. The caller still
    /// owns and must release the original running request. Rejection preserves
    /// the stored result and leaves Return owed; only tentative grants roll back.
    #[allow(clippy::too_many_arguments)]
    pub fn send_answer(
        &mut self,
        key: answers::Key<'id>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        scratch: &mut [u32],
        exception: impl FnOnce(&A, capnp::field_api::FixedStruct<'_, Exception>) -> capnp::Result<()>,
    ) -> Result<Option<A>, Error> {
        self.send_answer_using(
            key,
            messages,
            queue,
            minimum,
            options,
            scratch,
            Default::default(),
            |_, _, _| u8::MAX,
            exception,
        )
    }

    /// Attach optional resources from the same retained owners used to stage the
    /// result descriptors. Failure drops temporary resources, never the stored
    /// answer. Full error metadata may be written by the exception callback.
    #[allow(clippy::too_many_arguments)]
    pub fn send_answer_with_attachments<R, const N: usize>(
        &mut self,
        key: answers::Key<'id>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, crate::attachments::Sender<R, N>>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        scratch: &mut [u32],
        capacity: Capacity,
        mut imported: impl FnMut(&I) -> Option<R>,
        mut local: impl FnMut(&S) -> Option<R>,
        exception: impl FnOnce(&A, capnp::field_api::FixedStruct<'_, Exception>) -> capnp::Result<()>,
    ) -> Result<Option<A>, Error> {
        self.send_answer_using(
            key,
            messages,
            queue,
            minimum,
            options,
            scratch,
            crate::attachments::Sender::new(capacity),
            |owner, imports, resources| {
                resources.push(crate::attachments::resource(
                    owner,
                    imports,
                    &mut imported,
                    &mut local,
                ))
            },
            exception,
        )
    }

    #[allow(clippy::too_many_arguments)]
    /// Publish with a caller-defined resource bundle. Attachment indices must
    /// describe that bundle; return 255 for absent or unsupported resources.
    pub fn send_answer_using<R>(
        &mut self,
        key: answers::Key<'id>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, R>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        scratch: &mut [u32],
        resources: R,
        mut attach: impl FnMut(
            &binding::Route<'id, P, S, PATH>,
            &imports::Imports<'id, '_, I>,
            &mut R,
        ) -> u8,
        exception: impl FnOnce(&A, capnp::field_api::FixedStruct<'_, Exception>) -> capnp::Result<()>,
    ) -> Result<Option<A>, Error> {
        let options = self.writer_policy.options(options);
        self.check_live()?;
        // Third-party completion requires network authentication and its own
        // Return publication, even when its local producer has completed.
        let (cancelled, redirected, can_omit_finish) = {
            let pending = self
                .ledgers
                .answers
                .prepare_return(key)
                .map_err(|e| Error::Pipeline(pipelines::Error::Answer(e)))?;
            (
                pending.is_cancelled(),
                pending.is_redirected(),
                pending.can_omit_finish(),
            )
        };
        let owner = &self
            .ledgers
            .answers
            .get(key)
            .map_err(|e| Error::Pipeline(pipelines::Error::Answer(e)))?
            .owner;
        // Adoption acknowledges the redirected source immediately. Its local
        // producer and result remain live for the adopting question.
        let pending = Err(capnp::ErrorKind::Failed);
        let result = owner
            .value
            .as_ref()
            .or_else(|| {
                self.ledgers
                    .answers
                    .execution_state(key)
                    .ok()
                    .filter(|state| state.adopted)
                    .map(|_| &pending)
            })
            .ok_or(Error::PendingResponse)?;
        let mut output = queue
            .reserve_with(resources)
            .map_err(|e| Error::Input(super::super::Error::Storage(e)))?;
        let mut batch = self.ledgers.exports.batch();
        let mut minimum = minimum;
        let base_minimum = minimum;
        let mut prepared = false;
        let mut staged = self
            .message_capabilities
            .buffer::<_, CAPS>(
                if cancelled || redirected {
                    0
                } else {
                    result.as_ref().map_or(0, |result| result.count)
                },
                || reflection::Descriptor::None,
            )
            .ok_or(Error::Input(super::super::Error::Capacity))?;
        let frame = if cancelled || redirected || result.is_err() {
            Frame::build(messages, minimum, options, |builder| {
                if redirected {
                    return calls::results_sent_elsewhere(builder, key.id());
                }
                if cancelled {
                    return calls::cancelled(builder, key.id());
                }
                let kind = *result.as_ref().err().expect("exception");
                calls::exception(builder, key.id(), kind, "")?;
                exception(
                    &owner.producer,
                    builder
                        .try_get_root::<Message>()?
                        .into_field(Message::RETURN)
                        .edit()?
                        .into_field(Return::EXCEPTION)
                        .edit()?,
                )
            })
            .map_err(Error::Input)?
        } else {
            let result = result.as_ref().unwrap_or_else(|_| unreachable!());
            let certified =
                result.count == 0 && result.prepared_return == Some((key.id(), can_omit_finish));
            // Native results need no second body walk. Reuse below checks the
            // exact Return header; rebuilding constructs that header here.
            prepared = (certified
                && result.frame.options.traversal_limit_in_words
                    == options.traversal_limit_in_words
                && result.frame.options.nesting_limit == options.nesting_limit)
                || (self.writer_policy == WriterPolicy::Native && result.count == 0);
            let reusable = certified
                || (result.count == 0
                    && result
                        .frame
                        .with(messages, |source| -> capnp::Result<bool> {
                            let rpc_capnp::message::Return(returned) = source.which()? else {
                                return Ok(false);
                            };
                            let returned = returned?;
                            let rpc_capnp::return_::Results(payload) = returned.which()? else {
                                return Ok(false);
                            };
                            Ok(returned.get_answer_id() == key.id().to_wire()
                                && !returned.get_release_param_caps()
                                && returned.get_no_finish_needed() == can_omit_finish
                                && payload?.get_cap_table()?.is_empty())
                        })
                        .map_err(Error::Input)?
                        .map_err(|e| Error::Input(e.into()))?);
            if reusable {
                // Keep an independent publication lease; rejection releases it
                // without consuming the answer's retained result. Checked
                // writers still enforce the current output reader limits.
                Frame::new(
                    messages
                        .retain(&result.frame.lease)
                        .map_err(|e| Error::Input(super::super::Error::Storage(e)))?,
                    options,
                )
            } else {
                for (index, client) in result.clients[..result.count].iter().enumerate() {
                    let Some(client) = client else { continue };
                    if let Ok(clients::Destination::Endpoint { owner, .. }) =
                        self.runtime.inspect(client)
                    {
                        if let Some(target) =
                            reflection::target(owner, self.imports.peer(), &self.ledgers.questions)
                                .map_err(Error::Question)?
                        {
                            // Imported IDs cannot move during export staging.
                            // Preserve this classification like immediate Calls.
                            if let calls::Target::Imported(id) = target {
                                staged[index] =
                                    reflection::Descriptor::Reflected(calls::Target::Imported(id));
                            }
                            if let calls::Target::Promised { path, .. } = target {
                                minimum = minimum
                                    .checked_add(32)
                                    .and_then(|n| {
                                        n.checked_add(path.fields().len().checked_mul(8)?)
                                    })
                                    .ok_or(Error::Input(super::super::Error::Wire(
                                        capnp::ErrorKind::MessageSizeOverflow,
                                    )))?;
                            }
                            continue;
                        }
                    }
                    let grant = self
                        .exported
                        .stage_using(
                            &mut self.runtime,
                            &mut batch,
                            client,
                            self.descriptors.identity,
                        )
                        .map_err(Error::Exported)?;
                    let attached = match self.runtime.inspect(client) {
                        Ok(clients::Destination::Endpoint { owner, .. }) => {
                            attach(owner, &self.ledgers.imports, output.resources_mut())
                        }
                        _ => u8::MAX,
                    };
                    staged[index] =
                        reflection::Descriptor::Grant(grant.id(), grant.is_promise(), attached);
                    let owner = batch.pending_owner(grant.id()).and_then(|client| {
                        match self.runtime.inspect(client) {
                            Ok(clients::Destination::Endpoint { owner, .. }) => Some(owner),
                            _ => None,
                        }
                    });
                    minimum = minimum
                        .checked_add(
                            self.descriptors
                                .bytes(owner, grant.is_promise())
                                .map_err(|e| Error::Input(e.into()))?,
                        )
                        .ok_or(Error::Input(super::super::Error::Wire(
                            capnp::ErrorKind::MessageSizeOverflow,
                        )))?;
                }
                for (index, client) in result.clients[..result.count].iter().enumerate() {
                    if matches!(
                        staged[index],
                        reflection::Descriptor::Grant(..) | reflection::Descriptor::Reflected(..)
                    ) {
                        continue;
                    }
                    if let Some(client) = client {
                        if let Ok(clients::Destination::Endpoint { owner, .. }) =
                            self.runtime.inspect(client)
                        {
                            if let Some(target) = reflection::target(
                                owner,
                                self.imports.peer(),
                                &self.ledgers.questions,
                            )
                            .map_err(Error::Question)?
                            {
                                staged[index] = reflection::Descriptor::Reflected(target);
                            }
                        }
                    }
                }
                let mut fill = |mut descriptors: capnp::field_api::FixedList<'_, CapDescriptor>| {
                    for (index, grant) in staged[..result.count].iter().enumerate() {
                        let mut descriptor = descriptors.get_mut(index).unwrap();
                        if let Some(target) = grant.reflected() {
                            reflection::write(target, descriptor)?;
                            continue;
                        }
                        let reflection::Descriptor::Grant(id, promise, attached) = *grant else {
                            descriptor.void(CapDescriptor::NONE).set();
                            continue;
                        };
                        descriptor.scalar(CapDescriptor::ATTACHED_FD).set(attached);
                        let owner = batch.pending_owner(id).and_then(|client| {
                            match self.runtime.inspect(client) {
                                Ok(clients::Destination::Endpoint { owner, .. }) => Some(owner),
                                _ => None,
                            }
                        });
                        self.descriptors.describe(owner, id, promise, descriptor)?;
                    }
                    Ok(())
                };
                let copied = if self.writer_policy == WriterPolicy::Native && result.count != 0 {
                    super::payload::copy_return_frame(
                        &result.frame,
                        result.prepared_return.is_some(),
                        messages,
                        key.id(),
                        result.count,
                        minimum,
                        minimum - base_minimum,
                        options,
                        &mut fill,
                    )
                    .map_err(Error::Input)?
                } else {
                    None
                };
                if let Some(frame) = copied {
                    frame
                } else {
                    result
                        .frame
                        .build_from(messages, minimum, options, |source, builder| {
                            let rpc_capnp::message::Return(source) = source.which()? else {
                                unreachable!()
                            };
                            let rpc_capnp::return_::Results(payload) = source?.which()? else {
                                unreachable!()
                            };
                            let mut returned = builder
                                .try_init_root::<Message>()?
                                .into_field(Message::RETURN)
                                .init()?;
                            returned.scalar(Return::ANSWER_ID).set(key.id().to_wire());
                            returned.scalar(Return::RELEASE_PARAM_CAPS).set(false);
                            returned
                                .scalar(Return::NO_FINISH_NEEDED)
                                .set(can_omit_finish && result.count == 0);
                            let mut destination = returned.into_field(Return::RESULTS).init()?;
                            fill(destination.field(Payload::CAP_TABLE).init(result.count)?)?;
                            destination
                                .into_field(Payload::CONTENT)
                                .copy_with_capabilities(
                                    payload?.get_content(),
                                    capnp::wire_capability::IndexMap::identity(result.count as u32),
                                )
                        })
                        .map_err(Error::Input)?
                }
            }
        };
        let retired = if prepared {
            drop(batch);
            answer::send_prepared(
                key,
                &mut self.ledgers.answers,
                messages,
                frame,
                can_omit_finish,
                output,
            )
        } else if cancelled
            || redirected
            || result.is_err()
            || result.as_ref().is_ok_and(|value| value.count == 0)
        {
            drop(batch);
            answer::send(key, &mut self.ledgers.answers, messages, frame, output)
        } else {
            answer::send_granting_reflected_using(
                key,
                &mut self.ledgers.answers,
                batch,
                messages,
                frame,
                scratch,
                output,
                self.writer_policy == WriterPolicy::Checked,
                |descriptor, index| {
                    reflection::check(
                        staged
                            .get(index)
                            .and_then(reflection::Descriptor::reflected),
                        descriptor,
                    )
                },
            )
        }
        .map_err(Error::Input)?;
        Ok(retired.map(|state| {
            state
                .release(messages)
                .release(messages, &mut self.runtime, |child| {
                    self.ledgers.questions.release(child)
                })
        }))
    }
}
