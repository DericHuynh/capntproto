//! Caller-authenticated third-party jobs owned by the ordinary connection.
use super::*;
use capntproto_rpc_engine::clients::Capabilities;

#[must_use = "process the network job or release it through its originating driver"]
pub enum NetworkRequest<'pool, 'id, const PATH: usize> {
    Provide(super::super::provision::Provide<'pool, 'id, 'id, PATH>),
    Accept(super::super::provision::Accept<'pool, 'id>),
    Adopt(super::super::question::IncomingAdoption<'pool, 'id>),
    Await(AwaitingResult<'pool, 'id>),
    Disembargo(ProvisionBarrier<'pool, 'id>),
    ForwardEmbargo(VineBarrier<'pool, 'id>),
    Join(super::super::join::Join<'pool, 'id, 'id, PATH>),
}
impl<'pool, 'id, const PATH: usize> NetworkRequest<'pool, 'id, PATH> {
    pub fn answer_key(&self) -> Option<answers::Key<'id>> {
        match self {
            Self::Provide(p) => Some(p.key()),
            Self::Accept(a) => Some(a.key()),
            Self::Adopt(_) | Self::Await(_) | Self::ForwardEmbargo(_) => None,
            Self::Disembargo(barrier) => Some(barrier.key()),
            Self::Join(join) => Some(join.key()),
        }
    }
    pub fn question_key(&self) -> Option<questions::Key<'id>> {
        match self {
            Self::Adopt(a) => Some(a.key()),
            Self::Await(a) => Some(a.key()),
            _ => None,
        }
    }
    pub(super) fn release<I, A, P: Copy + Eq, S, const CAPS: usize>(
        self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) {
        match self {
            Self::Provide(provide) => provide.release(
                messages,
                &mut driver.ledgers.exports,
                &mut driver.ledgers.answers,
            ),
            Self::Accept(accept) => accept.release(messages),
            Self::Disembargo(barrier) => barrier.release(messages),
            Self::ForwardEmbargo(barrier) => barrier.release(messages, &mut driver.ledgers.exports),
            Self::Join(join) => join.release(
                messages,
                &mut driver.ledgers.exports,
                &mut driver.ledgers.answers,
            ),
            Self::Await(awaiting) => {
                let _ = driver.cancel(awaiting.key());
                awaiting.frame.release(messages);
            }
            Self::Adopt(adoption) => {
                // A previously canceled response may already have been drained.
                let _ = driver.cancel(adoption.key());
                adoption.release(messages);
            }
        }
    }
}
#[derive(Debug)]
pub enum AdoptionError<E> {
    Connection(Error),
    Network(E),
}
#[must_use = "retry authentication or release the original adoption job"]
pub struct AdoptionRejected<'pool, 'id, E> {
    pub error: AdoptionError<E>,
    pub adoption: super::super::question::IncomingAdoption<'pool, 'id>,
}
#[must_use = "retry publication or recover the unadmitted producer"]
pub struct AdoptedAnswerRejected<A> {
    pub error: Error,
    pub producer: A,
}
#[derive(Debug)]
pub enum RegistrationError<E> {
    Connection(Error),
    Answer(answers::Error),
    AlreadyRegistered,
    Network(E),
}
#[must_use = "retry registration or release the original admitted request"]
pub struct RegistrationRejected<'pool, 'id, E, const PATH: usize> {
    pub error: RegistrationError<E>,
    pub provide: super::super::provision::Provide<'pool, 'id, 'id, PATH>,
}
#[must_use = "retry the ready acceptance or release its request and authority"]
pub struct AcceptRejected<'pool, 'id> {
    pub error: Error,
    pub ready: super::super::provision::ReadyAccept<'pool, 'id, clients::Client<'id>>,
}
#[must_use = "retry failure publication or release the original network request"]
pub struct AcceptFailureRejected<'pool, 'id> {
    pub error: Error,
    pub accept: super::super::provision::Accept<'pool, 'id>,
}
impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// Complete a locally allocated third-party answer. Only the generation
    /// returned by `send_adopted_answer` is eligible; ordinary peer Calls cannot
    /// enter this path. Caller-owned network code supplies the result authority.
    #[allow(clippy::result_large_err)]
    pub fn complete_adopted_answer(
        &mut self,
        key: answers::Key<'id>,
        result: Result<LocalResult<'pool, 'id, CAPS>, capnp::ErrorKind>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> Result<(), AnswerHandoffRejected<'pool, 'id, CAPS>> {
        let checked = if (1 << 30..1 << 31).contains(&key.id().to_wire()) {
            self.check_answer_completion(key)
        } else {
            Err(Error::InvalidNetworkTarget)
        };
        if let Err(error) = checked {
            return Err(AnswerHandoffRejected { error, result });
        }
        self.complete_local_answer(key, result, messages)
    }

    /// Publish an authenticated metadata-only Return. Admission checks the
    /// original Call's permission and destination. A racing Finish wins and
    /// emits Canceled instead, retaining the same generation for pipeline cleanup.
    #[allow(clippy::too_many_arguments)]
    pub fn send_network_answer<R: Default>(
        &mut self,
        key: answers::Key<'id>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, R>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        build: impl FnOnce(
            &mut capnp::message::FixedBuilder<'_>,
            capntproto_rpc_engine::ids::AnswerId,
        ) -> capnp::Result<()>,
    ) -> Result<Option<A>, Error> {
        self.check_live()?;
        let cancelled = self
            .ledgers
            .answers
            .prepare_return(key)
            .map_err(|e| Error::Pipeline(pipelines::Error::Answer(e)))?
            .is_cancelled();
        let output = queue
            .reserve()
            .map_err(|e| Error::Input(super::super::Error::Storage(e)))?;
        let frame = Frame::build(messages, minimum, options, |builder| {
            if cancelled {
                calls::cancelled(builder, key.id())
            } else {
                build(builder, key.id())
            }
        })
        .map_err(Error::Input)?;
        let retired = answer::send(key, &mut self.ledgers.answers, messages, frame, output)
            .map_err(Error::Input)?;
        Ok(retired.map(|state| {
            state
                .release(messages)
                .release(messages, &mut self.runtime, |child| {
                    self.ledgers.questions.release(child)
                })
        }))
    }

    /// Keep the original promised-answer route after ResultsSentElsewhere.
    /// The retained response proves the question generation and reply type.
    pub fn tail_pipeline(
        &mut self,
        response: &Response<'pool, 'id, I, CAPS>,
        path: pipeline::Path<'_>,
        messages: &messages::Messages<'pool, '_, '_>,
    ) -> Result<clients::Client<'id>, Error> {
        self.check_live()?;
        if !response
            .with(messages, |reply| {
                matches!(reply, super::super::Reply::ResultsSentElsewhere)
            })
            .map_err(Error::Input)?
        {
            return Err(Error::InvalidNetworkTarget);
        }
        let question = response.local_ref().ok_or(Error::UnknownQuestion)?;
        outgoing::freeze(
            &mut self.runtime,
            &mut self.ledgers.questions,
            question,
            path,
            self.imports.peer(),
            self.message_path,
        )
        .map_err(Error::Outgoing)
    }

    /// Publish a callee-selected answer before any Return can reference it.
    /// The caller authenticates the destination and prepares completion metadata
    /// through its network. Queue, buffer, metadata, or ledger exhaustion leaves
    /// the producer unadmitted. Successful publication uses ordinary answer,
    /// pipeline, cancellation and Return cleanup from this point onward.
    #[allow(clippy::too_many_arguments)]
    pub fn send_adopted_answer(
        &mut self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        producer: A,
        completion: impl FnOnce(
            capnp::field_api::FixedField<'_, capnp::field_api::Generic<capnp::any_pointer::Owned>>,
        ) -> capnp::Result<()>,
    ) -> Result<answers::Key<'id>, AdoptedAnswerRejected<A>> {
        let mut producer = Some(producer);
        let outcome = (|| {
            self.check_live()?;
            let output = queue
                .reserve()
                .map_err(|e| Error::Input(super::super::Error::Storage(e)))?;
            let pending = self.ledgers.answers.prepare_third_party().map_err(|e| {
                Error::Input(super::super::Error::Admission(inbound::Error::Answer(e)))
            })?;
            let frame = Frame::build(messages, minimum, options, |b| {
                crate::network::third_party_answer(b, pending.id(), completion)
            })
            .map_err(Error::Input)?;
            let prepared =
                crate::admission::third_party_answer(pending, messages, frame.lease, frame.options)
                    .map_err(|e| Error::Input(super::super::Error::Output(e)))?;
            Ok(prepared.accept(
                answer::State::new(pipelines::Results::new(producer.take().unwrap())),
                output,
            ))
        })();
        outcome.map_err(|error| AdoptedAnswerRejected {
            error,
            producer: producer.unwrap(),
        })
    }
    /// Authenticate the completion against a caller-owned rendezvous before
    /// exposing the adopted result or pipeline. The callback sees immutable
    /// metadata from this exact generation. It must validate the peer and the
    /// original introducer's authorization, not merely the numeric answer ID.
    /// A pending/failed callback returns the owning job unchanged for retry or
    /// cancellation. No callback runs after cancellation or disconnection.
    #[allow(clippy::result_large_err)]
    pub fn authorize_adoption<E>(
        &mut self,
        adoption: super::super::question::IncomingAdoption<'pool, 'id>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        authenticate: impl FnOnce(capnp::any_pointer::Reader<'_>) -> Result<(), E>,
    ) -> Result<questions::Key<'id>, AdoptionRejected<'pool, 'id, E>> {
        let key = adoption.key();
        let outcome = (|| {
            self.check_live().map_err(AdoptionError::Connection)?;
            let index = ExchangeSlot::index(&self.exchanges, &self.ledgers.questions, key)
                .ok_or(AdoptionError::Connection(Error::UnknownQuestion))?;
            let slot = &mut self.exchanges[index];
            if slot.canceled || slot.authorized {
                return Err(AdoptionError::Connection(Error::UnknownQuestion));
            }
            adoption
                .with_completion(messages, authenticate)
                .map_err(|e| AdoptionError::Connection(Error::Input(e)))?
                .map_err(AdoptionError::Network)?;
            slot.authorized = true;
            Ok(())
        })();
        match outcome {
            Ok(()) => {
                self.refresh_exchange(key);
                adoption.release(messages);
                Ok(key)
            }
            Err(error) => Err(AdoptionRejected { error, adoption }),
        }
    }
    /// Publish a provision without consuming an ordinary reply-tracking slot.
    /// Its affine handle remains with the network's vine until acceptance ends.
    /// The caller obtains target authority from its import/question bindings,
    /// as with the low-level `send` entry point.
    pub fn send_provide(
        &mut self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        build: impl FnOnce(
            &mut capnp::message::FixedBuilder<'_>,
            capntproto_rpc_engine::ids::QuestionId,
        ) -> capnp::Result<()>,
    ) -> Result<questions::Provision<'id>, Error> {
        self.check_live()?;
        let output = queue
            .reserve()
            .map_err(|e| Error::Input(super::super::Error::Storage(e)))?;
        let reservation = self
            .ledgers
            .questions
            .reserve_provision()
            .map_err(Error::Question)?;
        let frame = Frame::build(messages, minimum, options, |builder| {
            build(builder, reservation.id())
        })
        .map_err(Error::Input)?;
        super::super::provision::send(reservation, messages, frame, output)
            .map_err(|e| Error::Input(super::super::Error::Output(e)))
    }
    pub fn release_provision(&mut self, provision: questions::Provision<'id>) {
        self.ledgers.questions.release_provision(provision);
    }
    /// Whether a network operation is waiting for the caller's policy handler.
    pub fn has_network(&self) -> bool {
        self.network.is_some()
    }
    pub fn take_network(&mut self) -> Option<NetworkRequest<'pool, 'id, PATH>> {
        self.network.take()
    }
    pub fn release_network(
        &mut self,
        request: NetworkRequest<'pool, 'id, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) {
        request.release(self, messages);
    }
    /// Register while the answer generation and its prepared authority are
    /// exclusively held. The callback authenticates opaque recipient metadata
    /// and installs its registration guard in the producer. It cannot suspend.
    /// The driver retains the graph client until Finish/terminal cleanup, so a
    /// registry may refer to this answer key without owning a borrowed frame.
    /// Callback failure must roll back external registration; the intact job is
    /// returned for retry. No callback runs for a finished/stale generation.
    #[allow(clippy::result_large_err)]
    pub fn register_provide<E>(
        &mut self,
        provide: super::super::provision::Provide<'pool, 'id, 'id, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        register: impl FnOnce(
            capnp::any_pointer::Reader<'_>,
            &clients::Client<'id>,
            &mut A,
        ) -> Result<(), E>,
    ) -> Result<(), RegistrationRejected<'pool, 'id, E, PATH>> {
        let outcome = (|| {
            self.check_live().map_err(RegistrationError::Connection)?;
            let state = self
                .ledgers
                .answers
                .get(provide.key())
                .map_err(RegistrationError::Answer)?;
            if state.owner.network_authority.is_some() {
                return Err(RegistrationError::AlreadyRegistered);
            }
            let observer = matches!(provide.target(), TargetRef::Promised { .. });
            let client = match provide.target() {
                TargetRef::Hosted(lease) => {
                    binding::reflected(&mut self.runtime, &self.ledgers.exports, lease)
                        .map_err(Error::Graph)
                }
                TargetRef::Promised { answer, path } => self
                    .pipelines
                    .bind(&mut self.runtime, &mut self.ledgers.answers, answer, path)
                    .map_err(Error::Pipeline),
            }
            .map_err(RegistrationError::Connection)?;
            let key = client.key();
            let registered = provide
                .with_recipient(messages, |metadata| {
                    register(
                        metadata,
                        &client,
                        &mut self
                            .ledgers
                            .answers
                            .get_mut(provide.key())
                            .expect("generation held through registration")
                            .owner
                            .producer,
                    )
                })
                .map_err(|e| RegistrationError::Connection(Error::Input(e)))
                .and_then(|r| r.map_err(RegistrationError::Network));
            if let Err(error) = registered {
                if observer {
                    self.pipelines.discard_unexposed(
                        key,
                        &mut self.ledgers.answers,
                        &mut self.runtime,
                    );
                }
                self.runtime.release(client);
                return Err(error);
            }
            self.ledgers
                .answers
                .get_mut(provide.key())
                .expect("generation held through registration")
                .owner
                .network_authority = Some(client);
            Ok(())
        })();
        match outcome {
            Ok(()) => {
                provide.release(
                    messages,
                    &mut self.ledgers.exports,
                    &mut self.ledgers.answers,
                );
                Ok(())
            }
            Err(error) => Err(RegistrationRejected { error, provide }),
        }
    }
    /// Retain independently owned authority only while this generation's
    /// authenticated registration is live. Network policy authorizes the caller
    /// before invoking this; possession of a numeric question ID is insufficient.
    pub fn retain_provided(
        &mut self,
        key: answers::Key<'id>,
    ) -> Result<clients::Client<'id>, Error> {
        self.check_live()?;
        if !self.ledgers.answers.is_provision(key).unwrap_or(false) {
            return Err(Error::InvalidNetworkTarget);
        }
        let state =
            self.ledgers.answers.get(key).map_err(|e| {
                Error::Input(super::super::Error::Admission(inbound::Error::Answer(e)))
            })?;
        let client = state
            .owner
            .network_authority
            .as_ref()
            .ok_or(Error::InvalidInvocationTarget)?;
        self.runtime.retain_key(client.key()).map_err(Error::Graph)
    }
    /// Complete an Accept whose caller-owned network has asynchronously
    /// authenticated the provision and completed its embargo. The affine Accept
    /// token ties this completion to the admitted generation. Publish through
    /// send_answer_using, then release the token; Finish follows ordinary answer
    /// and pipeline cleanup. No numeric ID can construct this authority.
    #[allow(clippy::result_large_err)] // Return the affine result without requiring alloc.
    pub fn complete_authenticated_accept(
        &mut self,
        accept: &super::super::provision::Accept<'pool, 'id>,
        result: Result<LocalResult<'pool, 'id, CAPS>, capnp::ErrorKind>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> Result<(), super::AnswerHandoffRejected<'pool, 'id, CAPS>> {
        self.complete_local_answer(accept.key(), result, messages)
    }

    /// Publish an authenticated, disembargoed capability through the same
    /// frozen result pipeline as ordinary calls. Queue/capacity errors preserve
    /// the ready request and authority. Early Finish sends a canceled Return
    /// while any already-admitted pipelines retain the locally completed result.
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn complete_accept(
        &mut self,
        ready: super::super::provision::ReadyAccept<'pool, 'id, clients::Client<'id>>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
        scratch: &mut [u32],
    ) -> Result<Option<A>, AcceptRejected<'pool, 'id>> {
        let key = ready.key();
        let outcome = (|| {
            self.check_live()?;
            let state = self
                .ledgers
                .answers
                .get(key)
                .map_err(|e| Error::Pipeline(pipelines::Error::Answer(e)))?;
            if !state.owner.is_pending() {
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
            if self
                .ledgers
                .answers
                .cancellation_requested(key)
                .map_err(|e| Error::Pipeline(pipelines::Error::Answer(e)))?
            {
                let frame = Frame::build(messages, minimum, options, |builder| {
                    calls::cancelled(builder, key.id())
                })
                .map_err(Error::Input)?;
                let retired =
                    answer::try_send(key, &mut self.ledgers.answers, messages, frame, output)
                        .map_err(|rejected| {
                            rejected.frame.release(messages);
                            Error::Input(rejected.error)
                        })?;
                return Ok(retired.map(|state| {
                    state
                        .release(messages)
                        .release(messages, &mut self.runtime, |child| {
                            self.ledgers.questions.release(child)
                        })
                }));
            }
            let mut batch = self.ledgers.exports.batch();
            let grant = self
                .exported
                .stage(&mut self.runtime, &mut batch, ready.authority())
                .map_err(Error::Exported)?;
            let frame = Frame::build(messages, minimum, options, |builder| {
                crate::network::accepted(builder, key.id(), &grant)
            })
            .map_err(Error::Input)?;
            let published = match pipelines::Published::<CAPS>::from_exports_with_capacity(
                &frame,
                messages,
                &batch,
                &mut self.runtime,
                scratch,
                self.message_capabilities,
            ) {
                Ok(published) => published,
                Err(error) => {
                    frame.release(messages);
                    return Err(Error::Pipeline(error));
                }
            };
            let sent = if canceled {
                drop(batch);
                frame.release(messages);
                match Frame::build(messages, minimum, options, |builder| {
                    calls::cancelled(builder, key.id())
                }) {
                    Ok(frame) => {
                        answer::try_send(key, &mut self.ledgers.answers, messages, frame, output)
                            .map_err(|rejected| {
                                rejected.frame.release(messages);
                                Error::Input(rejected.error)
                            })
                    }
                    Err(error) => Err(Error::Input(error)),
                }
            } else {
                answer::try_send_granting(
                    key,
                    &mut self.ledgers.answers,
                    batch,
                    messages,
                    frame,
                    scratch,
                    output,
                )
                .map_err(|rejected| {
                    rejected.frame.release(messages);
                    drop(rejected.grants);
                    Error::Input(rejected.error)
                })
            };
            match sent {
                Err(error) => {
                    published.release(messages, &mut self.runtime);
                    Err(error)
                }
                Ok(Some(retired)) => {
                    published.release(messages, &mut self.runtime);
                    Ok(Some(retired.release(messages).release(
                        messages,
                        &mut self.runtime,
                        |child| self.ledgers.questions.release(child),
                    )))
                }
                Ok(None) => {
                    // Preflight checked pending state, the immutable encoder set
                    // the same answer ID, and publication ran no user callback.
                    pipelines::complete(&mut self.ledgers.answers, key, Ok(published))
                        .unwrap_or_else(|_| unreachable!("preflighted network completion"));
                    Ok(None)
                }
            }
        })();
        match outcome {
            Ok(retired) => {
                let (accept, authority) = ready.into_parts();
                accept.release(messages);
                self.runtime.release(authority);
                Ok(retired)
            }
            Err(error) => Err(AcceptRejected { error, ready }),
        }
    }
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn reject_accept(
        &mut self,
        accept: super::super::provision::Accept<'pool, 'id>,
        reason: capnp::ErrorKind,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
    ) -> Result<Option<A>, AcceptFailureRejected<'pool, 'id>> {
        let key = accept.key();
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
            let frame = Frame::build(messages, minimum, options, |builder| {
                if canceled {
                    calls::cancelled(builder, key.id())
                } else {
                    calls::exception(builder, key.id(), reason, "network acceptance failed")
                }
            })
            .map_err(Error::Input)?;
            let retired = answer::try_send(key, &mut self.ledgers.answers, messages, frame, output)
                .map_err(|rejected| {
                    rejected.frame.release(messages);
                    Error::Input(rejected.error)
                })?;
            if retired.is_none() {
                pipelines::complete(&mut self.ledgers.answers, key, Err(reason))
                    .unwrap_or_else(|_| unreachable!("preflighted failed acceptance"));
            }
            Ok(retired.map(|state| {
                state
                    .release(messages)
                    .release(messages, &mut self.runtime, |child| {
                        self.ledgers.questions.release(child)
                    })
            }))
        })();
        match outcome {
            Ok(retired) => {
                accept.release(messages);
                Ok(retired)
            }
            Err(error) => Err(AcceptFailureRejected { error, accept }),
        }
    }
}
