//! Result handoff retains the original question through authentication and every
//! preexisting pipeline observer. Foreign graph clients never cross brands: the
//! caller's network supplies result data and local proxy authority explicitly.
use super::*;

#[must_use = "complete the authenticated handoff or release through the driver"]
pub struct AwaitingResult<'pool, 'id> {
    pub(super) frame: Frame<'pool>,
    pub(super) key: questions::Key<'id>,
}
impl<'pool, 'id> AwaitingResult<'pool, 'id> {
    pub fn key(&self) -> questions::Key<'id> {
        self.key
    }
    pub fn with_metadata<R>(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
        f: impl FnOnce(capnp::any_pointer::Reader<'_>) -> R,
    ) -> Result<R, super::super::Error> {
        self.frame.with(messages, |root| {
            let rpc_capnp::message::Return(returned) = root
                .which()
                .map_err(|_| super::super::Error::WrongMessage)?
            else {
                return Err(super::super::Error::WrongMessage);
            };
            let rpc_capnp::return_::AwaitFromThirdParty(metadata) = returned?
                .which()
                .map_err(|_| super::super::Error::WrongMessage)?
            else {
                return Err(super::super::Error::WrongMessage);
            };
            Ok(f(metadata))
        })?
    }
}
#[derive(Debug)]
pub enum HandoffError<E> {
    Connection(Error),
    Network(E),
}
#[must_use = "retry handoff or release both the job and result"]
pub struct HandoffRejected<'pool, 'id, E, const CAPS: usize> {
    pub error: HandoffError<E>,
    pub awaiting: AwaitingResult<'pool, 'id>,
    pub result: Result<LocalResult<'pool, 'id, CAPS>, capnp::ErrorKind>,
}
impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// Enable the protocol permission only when the hosting network can
    /// authenticate and drive cross-connection answer rendezvous.
    pub fn allow_answer_handoff(&mut self, enabled: bool) {
        self.allow_answer_handoff = enabled;
    }

    pub fn set_descriptors(
        &mut self,
        policy: crate::publication::descriptors::Descriptors<binding::Route<'id, P, S, PATH>>,
    ) {
        self.descriptors = policy;
    }

    /// Configure outgoing owner identity without replacing descriptor metadata
    /// or authenticated introduction policy.
    pub fn set_export_identity(
        &mut self,
        identity: fn(&binding::Route<'id, P, S, PATH>) -> Option<exports::Identity>,
    ) {
        self.descriptors.identity = identity;
    }

    /// The original method and cancellation state of this affine handoff job.
    /// Never infer either from metadata or an unscoped numeric question ID.
    pub fn awaiting_method(&self, awaiting: &AwaitingResult<'pool, 'id>) -> Option<(u64, u16)> {
        let index = ExchangeSlot::index(&self.exchanges, &self.ledgers.questions, awaiting.key)?;
        let slot = &self.exchanges[index];
        if slot.canceled || !slot.authorized {
            return None;
        }
        match &slot.exchange {
            Some(Exchange::Returned(response)) => response.await_method(),
            _ => None,
        }
    }

    /// Match this exact original call to the authenticated new peer's completion.
    /// The callback must verify its rendezvous and bind it to the supplied result;
    /// matching numeric question IDs is insufficient. Capability proxies in the
    /// result already belong to this driver's graph. No callback runs for a
    /// canceled, disconnected, stale, already completed or wrong-method request.
    /// Authentication failure preserves both affine owners for retry.
    #[allow(clippy::result_large_err)]
    pub fn complete_await<E>(
        &mut self,
        awaiting: AwaitingResult<'pool, 'id>,
        result: Result<LocalResult<'pool, 'id, CAPS>, capnp::ErrorKind>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        authenticate: impl FnOnce(capnp::any_pointer::Reader<'_>) -> Result<(), E>,
    ) -> Result<(), HandoffRejected<'pool, 'id, E, CAPS>> {
        self.complete_await_using(awaiting, result, messages, authenticate, false)
    }
    /// Adopt a result from another connection while retaining all previously
    /// exposed promised-answer routes. New projections may use the authenticated
    /// direct answer. Existing projections cannot bypass their relay ordering.
    #[allow(clippy::result_large_err)]
    pub fn complete_await_retaining_routes<E>(
        &mut self,
        awaiting: AwaitingResult<'pool, 'id>,
        result: Result<LocalResult<'pool, 'id, CAPS>, capnp::ErrorKind>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        authenticate: impl FnOnce(capnp::any_pointer::Reader<'_>) -> Result<(), E>,
    ) -> Result<(), HandoffRejected<'pool, 'id, E, CAPS>> {
        self.complete_await_using(awaiting, result, messages, authenticate, true)
    }
    #[allow(clippy::result_large_err)]
    fn complete_await_using<E>(
        &mut self,
        awaiting: AwaitingResult<'pool, 'id>,
        result: Result<LocalResult<'pool, 'id, CAPS>, capnp::ErrorKind>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        authenticate: impl FnOnce(capnp::any_pointer::Reader<'_>) -> Result<(), E>,
        retain_routes: bool,
    ) -> Result<(), HandoffRejected<'pool, 'id, E, CAPS>> {
        let checked = (|| {
            self.check_live().map_err(HandoffError::Connection)?;
            let index = ExchangeSlot::index(&self.exchanges, &self.ledgers.questions, awaiting.key)
                .ok_or(HandoffError::Connection(Error::UnknownQuestion))?;
            let slot = &mut self.exchanges[index];
            if slot.canceled
                || !slot.authorized
                || slot.handoff.is_some()
                || !matches!(&slot.exchange, Some(Exchange::Returned(response)) if response.await_method().is_some())
            {
                return Err(HandoffError::Connection(Error::UnknownQuestion));
            }
            let Some(Exchange::Returned(response)) = &slot.exchange else {
                unreachable!()
            };
            if let Ok(result) = &result {
                if response.await_method() != Some((result.interface, result.method)) {
                    return Err(HandoffError::Connection(Error::Input(
                        super::super::Error::WrongMessage,
                    )));
                }
                // noFinishNeeded released the old answer's pipeline obligation.
                // A later handoff cannot reintroduce capability results there.
                if result.count != 0
                    && response
                        .no_finish_needed(messages)
                        .map_err(|e| HandoffError::Connection(Error::Input(e)))?
                {
                    return Err(HandoffError::Connection(Error::Input(
                        super::super::Error::WrongMessage,
                    )));
                }
            }
            awaiting
                .with_metadata(messages, authenticate)
                .map_err(|e| HandoffError::Connection(Error::Input(e)))?
                .map_err(HandoffError::Network)?;
            Ok(slot)
        })();
        match checked {
            Ok(slot) => {
                slot.retain_handoff_routes = retain_routes;
                slot.handoff = Some(result);
                self.refresh_exchange(awaiting.key);
                awaiting.frame.release(messages);
                Ok(())
            }
            Err(error) => Err(HandoffRejected {
                error,
                awaiting,
                result,
            }),
        }
    }

    /// Take an admitted data-only result after every existing pipeline observer
    /// has consumed its route. Other result kinds remain owned by the driver
    /// for `take_output`; authentication and cancellation follow the same ledger.
    #[cfg(feature = "alloc")]
    #[inline(always)]
    pub fn take_plain_output(
        &mut self,
        key: questions::Key<'id>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> Result<Option<alloc::boxed::Box<super::super::question::PlainResults>>, Error> {
        self.check_live()?;
        if self.outgoing.pending_for(key, &self.ledgers.questions)
            || self
                .invocation_pipelines
                .pending_for(key, &self.ledgers.questions)
        {
            return Err(Error::PendingResponse);
        }
        let index = ExchangeSlot::index(&self.exchanges, &self.ledgers.questions, key)
            .ok_or(Error::UnknownQuestion)?;
        let slot = &mut self.exchanges[index];
        if !slot.authorized {
            return Err(Error::PendingAuthorization);
        }
        let Some(Exchange::Returned(response)) = &mut slot.exchange else {
            return Err(Error::PendingResponse);
        };
        let Some(plain) = response.take_plain_results() else {
            return Ok(None);
        };
        let Some(Exchange::Returned(response)) = slot.exchange.take() else {
            unreachable!()
        };
        self.release_response(response, messages);
        Ok(Some(plain))
    }

    /// Take the general result, including capability and authenticated handoff
    /// ownership. Existing observers must consume their routes first.
    pub fn take_output(
        &mut self,
        key: questions::Key<'id>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> Result<CallResult<'pool, 'id, I, CAPS>, Error> {
        self.check_live()?;
        if self.outgoing.pending_for(key, &self.ledgers.questions)
            || self
                .invocation_pipelines
                .pending_for(key, &self.ledgers.questions)
        {
            return Err(Error::PendingResponse);
        }
        let index = ExchangeSlot::index(&self.exchanges, &self.ledgers.questions, key)
            .ok_or(Error::UnknownQuestion)?;
        let slot = &mut self.exchanges[index];
        if !slot.authorized {
            return Err(Error::PendingAuthorization);
        }
        let Some(Exchange::Returned(response)) = &slot.exchange else {
            return Err(Error::PendingResponse);
        };
        if let Some((adopted, method)) = response.adopted() {
            let result = match &self.ledgers.answers.adopted(adopted).owner.value {
                None => return Err(Error::PendingResponse),
                Some(Err(kind)) => Err(*kind),
                Some(Ok(published)) => Ok(published
                    .retain_local(
                        messages,
                        &mut self.runtime,
                        self.message_capabilities,
                        method,
                    )
                    .map_err(Error::Pipeline)?),
            };
            let Some(Exchange::Returned(response)) = slot.exchange.take() else {
                unreachable!()
            };
            self.release_response(response, messages);
            Ok(result.map(CallOutput::Local))
        } else if response.await_method().is_some() {
            let result = slot.handoff.take().ok_or(Error::PendingResponse)?;
            let Some(Exchange::Returned(response)) = slot.exchange.take() else {
                unreachable!()
            };
            self.release_response(response, messages);
            Ok(result.map(CallOutput::Local))
        } else {
            let Some(Exchange::Returned(response)) = slot.exchange.take() else {
                unreachable!()
            };
            self.clear_exchange_work(key);
            self.ledgers.questions.release_owner(key);
            Ok(Ok(CallOutput::Remote(response)))
        }
    }
}

#[must_use = "retry completion or release the preserved result"]
pub struct AnswerHandoffRejected<'pool, 'id, const CAPS: usize> {
    pub error: Error,
    pub result: Result<LocalResult<'pool, 'id, CAPS>, capnp::ErrorKind>,
}
impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// Complete the original answer's retained pipeline after its network Return
    /// was published. The request fixes both method and answer generation. The
    /// network supplies local authority proxies for the actual result. Existing
    /// pipelines keep their frozen result even if Finish precedes completion.
    /// Rejection preserves the supplied result and every existing route.
    #[allow(clippy::result_large_err)]
    pub fn complete_network_answer(
        &mut self,
        request: &Request<'pool, 'id, I, CAPS, PATH>,
        result: Result<LocalResult<'pool, 'id, CAPS>, capnp::ErrorKind>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> Result<(), AnswerHandoffRejected<'pool, 'id, CAPS>> {
        let key = request.answer();
        let checked = (|| {
            self.check_live()?;
            let state = self
                .ledgers
                .answers
                .get(key)
                .map_err(|e| Error::Pipeline(pipelines::Error::Answer(e)))?;
            if !state.owner.is_pending() {
                return Err(Error::Pipeline(pipelines::Error::AlreadyCompleted));
            }
            let third_party = self
                .ledgers
                .answers
                .is_third_party(key)
                .map_err(|e| Error::Pipeline(pipelines::Error::Answer(e)))?;
            let eligible = state
                .with_result(messages, |root| {
                    let Ok(rpc_capnp::message::Return(Ok(returned))) = root.which() else {
                        return false;
                    };
                    matches!(
                        returned.which(),
                        Ok(rpc_capnp::return_::AwaitFromThirdParty(_)
                            | rpc_capnp::return_::Canceled(()))
                    ) || ((third_party
                        || self
                            .ledgers
                            .answers
                            .execution_state(key)
                            .is_ok_and(|state| state.adopted))
                        && matches!(
                            returned.which(),
                            Ok(rpc_capnp::return_::ResultsSentElsewhere(()))
                        ))
                })
                .map_err(Error::Input)?;
            if eligible != Some(true) {
                return Err(Error::InvalidNetworkTarget);
            }
            if let Ok(result) = &result {
                let method = request
                    .with(messages, |request| {
                        (
                            request.incoming().interface_id(),
                            request.incoming().method_id(),
                        )
                    })
                    .map_err(Error::Input)?;
                if method != (result.interface, result.method) {
                    return Err(Error::Input(super::super::Error::WrongMessage));
                }
            }
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
                .unwrap_or_else(|_| unreachable!("preflighted network answer"));
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
}
