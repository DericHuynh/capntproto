//! Retained Accept metadata. Authentication and embargo rendezvous are provided
//! by the caller's network; ordinary answer publication keeps pipelining closed
//! until that rendezvous has authorized and released the capability.
use super::*;
use capntproto_rpc_engine::provision;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthorizationError {
    Input(Error),
    Embargo(provision::Error),
}
#[must_use = "retry or release both the request and independently retained authority"]
pub struct AuthorizationRejected<'p, 'a, T> {
    pub error: AuthorizationError,
    pub accept: Accept<'p, 'a>,
    pub authority: T,
}
#[must_use = "poll the embargo or cancel and release its retained authority"]
pub struct PendingAccept<'p, 'a, 'e, T> {
    accept: Accept<'p, 'a>,
    authority: T,
    claim: Option<provision::Claim<'e>>,
}
#[must_use = "retry polling or cancel this pending acceptance"]
pub struct PendingRejected<'p, 'a, 'e, T> {
    pub error: provision::Error,
    pub pending: PendingAccept<'p, 'a, 'e, T>,
}
#[must_use = "publish the accepted capability or explicitly release its owners"]
pub struct ReadyAccept<'p, 'a, T> {
    accept: Accept<'p, 'a>,
    authority: T,
}
impl<'p, 'a, T> ReadyAccept<'p, 'a, T> {
    pub fn key(&self) -> answers::Key<'a> {
        self.accept.key()
    }
    pub fn authority(&self) -> &T {
        &self.authority
    }
    pub fn into_parts(self) -> (Accept<'p, 'a>, T) {
        (self.accept, self.authority)
    }
}
impl<'p, 'a, 'e, T> PendingAccept<'p, 'a, 'e, T> {
    pub fn key(&self) -> answers::Key<'a> {
        self.accept.key()
    }
    #[allow(clippy::result_large_err)]
    pub fn poll<const BYTES: usize>(
        mut self,
        embargoes: &mut provision::Embargoes<'e, '_, BYTES>,
    ) -> Result<ReadyAccept<'p, 'a, T>, PendingRejected<'p, 'a, 'e, T>> {
        if let Some(claim) = self.claim.take() {
            if let Err(rejected) = embargoes.complete(claim) {
                self.claim = Some(rejected.claim);
                return Err(PendingRejected {
                    error: rejected.error,
                    pending: self,
                });
            }
        }
        Ok(ReadyAccept {
            accept: self.accept,
            authority: self.authority,
        })
    }
    pub fn cancel<const BYTES: usize>(
        self,
        embargoes: &mut provision::Embargoes<'e, '_, BYTES>,
    ) -> (Accept<'p, 'a>, T) {
        if let Some(claim) = self.claim {
            embargoes.cancel(claim)
        }
        (self.accept, self.authority)
    }
}

#[must_use = "complete or cancel the network lookup, then release its frame"]
pub struct Accept<'pool, 'id> {
    frame: Frame<'pool>,
    key: answers::Key<'id>,
}
pub struct Acceptance<'m> {
    pub completion: capnp::any_pointer::Reader<'m>,
    pub embargo: Option<&'m [u8]>,
}
impl<'pool, 'id> Accept<'pool, 'id> {
    /// The caller authenticates completion metadata and independently retains
    /// authority first. Bind the embargo claim to this exact immutable Accept;
    /// a present empty identifier still waits. All failures return both owners.
    #[allow(clippy::result_large_err)]
    pub fn authorize<'e, T, const BYTES: usize>(
        self,
        messages: &messages::Messages<'pool, '_, '_>,
        embargoes: &mut provision::Embargoes<'e, '_, BYTES>,
        authority: T,
    ) -> Result<PendingAccept<'pool, 'id, 'e, T>, AuthorizationRejected<'pool, 'id, T>> {
        let claim = self
            .with(messages, |request| {
                request.embargo.map(|id| embargoes.claim(id)).transpose()
            })
            .map_err(AuthorizationError::Input)
            .and_then(|r| r.map_err(AuthorizationError::Embargo));
        match claim {
            Ok(claim) => Ok(PendingAccept {
                accept: self,
                authority,
                claim,
            }),
            Err(error) => Err(AuthorizationRejected {
                error,
                accept: self,
                authority,
            }),
        }
    }
    pub fn admit<A>(
        frame: Frame<'pool>,
        messages: &messages::Messages<'pool, '_, '_>,
        answers: &mut answers::Answers<'id, '_, A>,
        owner: A,
    ) -> Result<Self, Rejected<'pool, A>> {
        let validated = frame
            .with(messages, |root| {
                let rpc_capnp::message::Accept(accept) =
                    root.which().map_err(|_| Error::WrongMessage)?
                else {
                    return Err(Error::WrongMessage);
                };
                let accept = accept?;
                if accept.get_question_id() >= 1 << 30 {
                    return Err(Error::Admission(inbound::Error::Unsupported));
                }
                accept.get_provision().validate_capability_indices(0)?;
                accept.get_embargo()?;
                Ok(AnswerId::from_wire(accept.get_question_id()))
            })
            .and_then(core::convert::identity);
        let id = match validated {
            Ok(id) => id,
            Err(error) => {
                return Err(Rejected {
                    frame,
                    owner,
                    error,
                })
            }
        };
        match answers.admit(id, owner) {
            Ok(key) => Ok(Self { frame, key }),
            Err(rejected) => Err(Rejected {
                frame,
                owner: rejected.value,
                error: Error::Admission(inbound::Error::Answer(rejected.error)),
            }),
        }
    }
    pub fn key(&self) -> answers::Key<'id> {
        self.key
    }
    pub fn with<R>(
        &self,
        messages: &messages::Messages<'pool, '_, '_>,
        f: impl FnOnce(Acceptance<'_>) -> R,
    ) -> Result<R, Error> {
        self.frame.with(messages, |root| {
            let rpc_capnp::message::Accept(accept) =
                root.which().map_err(|_| Error::WrongMessage)?
            else {
                return Err(Error::WrongMessage);
            };
            let accept = accept?;
            Ok(f(Acceptance {
                completion: accept.get_provision(),
                embargo: if accept.has_embargo() {
                    Some(accept.get_embargo()?)
                } else {
                    None
                },
            }))
        })?
    }
    pub fn release(self, messages: &mut messages::Messages<'pool, '_, '_>) {
        self.frame.release(messages);
    }
}
