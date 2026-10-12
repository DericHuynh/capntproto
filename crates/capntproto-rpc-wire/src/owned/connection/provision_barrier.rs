//! A Provide's disembargo follows older calls through the same dispatch queue.
use super::*;

/// Constructed only from a validated Provide-targeted disembargo. The driver
/// queues it on the registered capability before exposing it as a network job.
#[must_use = "release the ordered embargo through its originating driver"]
pub struct QueuedProvisionBarrier<'pool, 'id> {
    frame: Frame<'pool>,
    key: answers::Key<'id>,
}
impl<'pool, 'id> QueuedProvisionBarrier<'pool, 'id> {
    pub fn key(&self) -> answers::Key<'id> {
        self.key
    }
    pub fn release(self, messages: &mut messages::Messages<'pool, '_, '_>) {
        self.frame.release(messages);
    }
}
/// An ordered marker. Only the driver's dispatch transition can construct it.
///
/// ```compile_fail
/// use capntproto_rpc_wire::owned::connection::{ProvisionBarrier, QueuedProvisionBarrier};
/// fn bypass<'pool, 'id>(queued: QueuedProvisionBarrier<'pool, 'id>) -> ProvisionBarrier<'pool, 'id> {
///     queued
/// }
/// ```
#[must_use = "release the embargo through its originating driver"]
pub struct ProvisionBarrier<'pool, 'id>(QueuedProvisionBarrier<'pool, 'id>);
impl<'pool, 'id> ProvisionBarrier<'pool, 'id> {
    pub fn key(&self) -> answers::Key<'id> {
        self.0.key
    }
    pub(super) fn release(self, messages: &mut messages::Messages<'pool, '_, '_>) {
        self.0.release(messages);
    }
}
#[derive(Debug)]
pub enum EmbargoError<E> {
    Connection(Error),
    Network(E),
}
#[must_use = "retry releasing the embargo or release the original network job"]
pub struct EmbargoRejected<'pool, 'id, E> {
    pub error: EmbargoError<E>,
    pub barrier: ProvisionBarrier<'pool, 'id>,
}
impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    pub(super) fn receive_provision_barrier(
        &mut self,
        frame: Frame<'pool>,
        messages: &messages::Messages<'pool, '_, '_>,
    ) -> Result<(), Rejected<'pool, A>> {
        let imported = frame
            .with(messages, |root| -> Result<_, super::super::Error> {
                let rpc_capnp::message::Disembargo(d) = root
                    .which()
                    .map_err(|_| super::super::Error::WrongMessage)?
                else {
                    return Err(super::super::Error::WrongMessage);
                };
                let d = d?;
                let rpc_capnp::disembargo::context::Accept(id) = d
                    .get_context()
                    .which()
                    .map_err(|_| super::super::Error::WrongMessage)?
                else {
                    return Err(super::super::Error::WrongMessage);
                };
                id?;
                Ok(
                    match d
                        .get_target()?
                        .which()
                        .map_err(|_| super::super::Error::WrongMessage)?
                    {
                        rpc_capnp::message_target::ImportedCap(id) => Some(ExportId::from_wire(id)),
                        rpc_capnp::message_target::PromisedAnswer(_) => None,
                    },
                )
            })
            .and_then(core::convert::identity);
        match imported {
            Ok(Some(id)) => return self.receive_vine_barrier(frame, id),
            Ok(None) => (),
            Err(error) => {
                return Err(Rejected {
                    error: Error::Input(error),
                    frame,
                    producer: None,
                })
            }
        }
        let admission = (|| {
            if self.call.is_some() || self.echo.is_some() || !self.normal_dispatch_available() {
                return Err(Error::Busy);
            }
            let id = frame
                .with(messages, |root| -> Result<_, Error> {
                    let rpc_capnp::message::Disembargo(d) =
                        root.which().map_err(|_| Error::InvalidNetworkTarget)?
                    else {
                        return Err(Error::InvalidNetworkTarget);
                    };
                    let d = d.map_err(|e| Error::Input(e.into()))?;
                    let rpc_capnp::disembargo::context::Accept(bytes) = d
                        .get_context()
                        .which()
                        .map_err(|_| Error::InvalidNetworkTarget)?
                    else {
                        return Err(Error::InvalidNetworkTarget);
                    };
                    bytes.map_err(|e| Error::Input(e.into()))?;
                    let rpc_capnp::message_target::PromisedAnswer(target) = d
                        .get_target()
                        .map_err(|e| Error::Input(e.into()))?
                        .which()
                        .map_err(|_| Error::InvalidNetworkTarget)?
                    else {
                        return Err(Error::InvalidNetworkTarget);
                    };
                    let target = target.map_err(|e| Error::Input(e.into()))?;
                    if !target
                        .get_transform()
                        .map_err(|e| Error::Input(e.into()))?
                        .is_empty()
                    {
                        return Err(Error::InvalidNetworkTarget);
                    }
                    Ok(AnswerId::from_wire(target.get_question_id()))
                })
                .map_err(Error::Input)??;
            let key = self
                .ledgers
                .answers
                .lookup(id)
                .ok_or(Error::InvalidNetworkTarget)?;
            if !self
                .ledgers
                .answers
                .is_provision(key)
                .map_err(|_| Error::InvalidNetworkTarget)?
            {
                return Err(Error::InvalidNetworkTarget);
            }
            let client = self
                .ledgers
                .answers
                .get(key)
                .map_err(|_| Error::InvalidNetworkTarget)?
                .owner
                .network_authority
                .as_ref()
                .ok_or(Error::Busy)?;
            let route = self.runtime.retain(client).map_err(Error::Graph)?;
            Ok((key, route))
        })();
        let (key, route) = match admission {
            Ok(value) => value,
            Err(error) => {
                return Err(Rejected {
                    error,
                    frame,
                    producer: None,
                })
            }
        };
        let mut task = Some(Task::ProvisionBarrier(QueuedProvisionBarrier {
            frame,
            key,
        }));
        let accepted = self
            .runtime
            .prepare_call(&route, &mut task)
            .map(|pending| pending.accept());
        self.runtime.release(route);
        match accepted {
            Ok(_) => Ok(()),
            Err(error) => {
                let Some(Task::ProvisionBarrier(barrier)) = task else {
                    unreachable!("unaccepted network barrier")
                };
                Err(Rejected {
                    error: Error::Dispatch(error),
                    frame: barrier.frame,
                    producer: None,
                })
            }
        }
    }
    /// Move only the oldest ready network barrier to the caller's network slot.
    /// Older calls must have been dispatched first. No frame is discarded when
    /// the slot is occupied or the target promise is still pending.
    pub fn poll_network(&mut self) -> Result<bool, Error> {
        self.check_live()?;
        if self.network.is_some() {
            return Ok(false);
        }
        let Some(ready) = self.runtime.next_ready().map_err(Error::Dispatch)? else {
            return Ok(false);
        };
        if !matches!(
            ready.owner(),
            Task::ProvisionBarrier(_) | Task::VineBarrier(_)
        ) {
            return Ok(false);
        }
        let running = ready.accept();
        self.network = Some(match self.runtime.complete(running) {
            Task::ProvisionBarrier(barrier) => {
                NetworkRequest::Disembargo(ProvisionBarrier(barrier))
            }
            Task::VineBarrier(barrier) => NetworkRequest::ForwardEmbargo(VineBarrier(barrier)),
            _ => unreachable!(),
        });
        Ok(true)
    }
    /// Release a previously registered embargo at its authenticated network.
    /// The byte identifier is delivered only after ordered dispatch. The exact
    /// live Provide generation supplies the registration owner; a Finish or a
    /// reused question ID cannot authorize a stale barrier callback.
    #[allow(clippy::result_large_err)]
    pub fn release_accept_embargo<E>(
        &mut self,
        barrier: ProvisionBarrier<'pool, 'id>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        release: impl FnOnce(&[u8], &mut A) -> Result<(), E>,
    ) -> Result<(), EmbargoRejected<'pool, 'id, E>> {
        let outcome = (|| {
            self.check_live().map_err(EmbargoError::Connection)?;
            let state = self
                .ledgers
                .answers
                .get_mut(barrier.key())
                .map_err(|_| EmbargoError::Connection(Error::InvalidNetworkTarget))?;
            if state.owner.network_authority.is_none() {
                return Err(EmbargoError::Connection(Error::InvalidNetworkTarget));
            }
            barrier
                .0
                .frame
                .with(messages, |root| -> Result<_, Error> {
                    let rpc_capnp::message::Disembargo(d) =
                        root.which().map_err(|_| Error::InvalidNetworkTarget)?
                    else {
                        unreachable!()
                    };
                    let d = d.map_err(|e| Error::Input(e.into()))?;
                    let rpc_capnp::disembargo::context::Accept(id) = d
                        .get_context()
                        .which()
                        .map_err(|_| Error::InvalidNetworkTarget)?
                    else {
                        unreachable!()
                    };
                    Ok(release(
                        id.map_err(|e| Error::Input(e.into()))?,
                        &mut state.owner.producer,
                    ))
                })
                .map_err(|e| EmbargoError::Connection(Error::Input(e)))?
                .map_err(EmbargoError::Connection)?
                .map_err(EmbargoError::Network)
        })();
        match outcome {
            Ok(()) => {
                barrier.release(messages);
                Ok(())
            }
            Err(error) => Err(EmbargoRejected { error, barrier }),
        }
    }
}
