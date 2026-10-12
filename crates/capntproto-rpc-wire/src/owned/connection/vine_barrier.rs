//! Ordered forwarding through an explicitly designated, generation-bound vine.
use super::*;
use capntproto_rpc_engine::clients::Capabilities;

/// The export lease pins the exact proxy even after peer Release. Only an
/// explicitly designated `Route::Vine` can admit this task.
#[must_use = "release the marker and its export lease through their driver"]
pub struct QueuedVineBarrier<'pool, 'id> {
    frame: Frame<'pool>,
    key: exports::Key<'id>,
    lease: exports::Lease<'id>,
}
impl<'pool, 'id> QueuedVineBarrier<'pool, 'id> {
    pub fn release<E>(
        self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        exports: &mut exports::Exports<'id, '_, E>,
    ) {
        self.frame.release(messages);
        exports.release_local_deferred(self.lease);
    }
}
/// This marker has passed older calls in ordered dispatch. It cannot be
/// manufactured from a numeric export ID or a still-queued marker.
///
/// ```compile_fail
/// use capntproto_rpc_wire::owned::connection::{VineBarrier, QueuedVineBarrier};
/// fn bypass<'p, 'id>(queued: QueuedVineBarrier<'p, 'id>) -> VineBarrier<'p, 'id> { queued }
/// ```
#[must_use = "forward this marker or release it through its driver"]
pub struct VineBarrier<'pool, 'id>(pub(super) QueuedVineBarrier<'pool, 'id>);
impl<'pool, 'id> VineBarrier<'pool, 'id> {
    pub fn key(&self) -> exports::Key<'id> {
        self.0.key
    }
    pub(super) fn release<E>(
        self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        exports: &mut exports::Exports<'id, '_, E>,
    ) {
        self.0.release(messages, exports);
    }
}
#[must_use = "retry forwarding or release the original marker"]
pub struct VineRejected<'pool, 'id, E> {
    pub error: EmbargoError<E>,
    pub barrier: VineBarrier<'pool, 'id>,
}

impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// Queue a marker through an exact retained remote route. Callers must first
    /// publish previously admitted calls; the immutable client pins its target.
    pub fn send_accept_embargo(
        &mut self,
        target: &clients::Client<'id>,
        id: &[u8],
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
    ) -> Result<(), Error> {
        self.check_live()?;
        let clients::Destination::Endpoint {
            owner,
            location: clients::Location::Remote(peer),
            ..
        } = self.runtime.inspect(target).map_err(Error::Graph)?
        else {
            return Err(Error::InvalidNetworkTarget);
        };
        if peer != self.imports.peer() {
            return Err(Error::InvalidNetworkTarget);
        }
        let target = owner
            .target(&self.ledgers.questions)
            .map_err(Error::Question)?
            .ok_or(Error::InvalidNetworkTarget)?;
        let slot = queue
            .reserve()
            .map_err(|e| Error::Input(super::super::Error::Storage(e)))?;
        let frame = Frame::build(messages, minimum, options, |builder| {
            crate::network::disembargo_accept(builder, target, id)
        })
        .map_err(Error::Input)?;
        let offer = match messages.offer(&frame.lease) {
            Ok(offer) => offer,
            Err(error) => {
                frame.release(messages);
                return Err(Error::Input(super::super::Error::Storage(error)));
            }
        };
        slot.accept(messages, offer);
        frame.release(messages);
        Ok(())
    }

    pub(super) fn receive_vine_barrier(
        &mut self,
        frame: Frame<'pool>,
        id: ExportId,
    ) -> Result<(), Rejected<'pool, A>> {
        let prepared = (|| {
            if self.call.is_some() || self.echo.is_some() || !self.normal_dispatch_available() {
                return Err(Error::Busy);
            }
            let key = self
                .ledgers
                .exports
                .lookup(id)
                .ok_or(Error::InvalidNetworkTarget)?;
            let route = self
                .ledgers
                .exports
                .resolve(key)
                .ok_or(Error::InvalidNetworkTarget)?;
            // No callback can turn an ordinary exported service into a vine.
            if !matches!(
                self.runtime.inspect(route).map_err(Error::Graph)?,
                clients::Destination::Endpoint {
                    owner: binding::Route::Vine(_),
                    location: clients::Location::Local,
                    ..
                }
            ) {
                return Err(Error::InvalidNetworkTarget);
            }
            let lease = self
                .ledgers
                .exports
                .lease(id)
                .map_err(|_| Error::InvalidNetworkTarget)?;
            Ok((key, lease))
        })();
        let (key, lease) = match prepared {
            Ok(value) => value,
            Err(error) => {
                return Err(Rejected {
                    error,
                    frame,
                    producer: None,
                })
            }
        };
        let mut task = Some(Task::VineBarrier(QueuedVineBarrier { frame, key, lease }));
        let Some(Task::VineBarrier(barrier)) = task.as_ref() else {
            unreachable!()
        };
        let route = self.ledgers.exports.get(&barrier.lease);
        match self
            .runtime
            .prepare_call(route, &mut task)
            .map(|pending| pending.accept())
        {
            Ok(_) => Ok(()),
            Err(error) => {
                let Some(Task::VineBarrier(barrier)) = task else {
                    unreachable!()
                };
                self.ledgers.exports.release_local_deferred(barrier.lease);
                Err(Rejected {
                    error: Error::Dispatch(error),
                    frame: barrier.frame,
                    producer: None,
                })
            }
        }
    }

    /// Forward along this vine's authenticated upstream path after older calls
    /// have entered that same path. The caller-owned proxy must enqueue the
    /// marker before returning success; an error must leave it unpublished.
    /// Queue exhaustion preserves the frame and proxy lease for retry.
    /// The admitted marker retains its exact export generation even if a later
    /// peer Release retires the wire ID. Disconnection still revokes forwarding.
    #[allow(clippy::result_large_err)]
    pub fn forward_vine_embargo<E>(
        &mut self,
        barrier: VineBarrier<'pool, 'id>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        scratch: &mut [u8],
        forward: impl FnOnce(&[u8], &S, &mut messages::Messages<'pool, '_, '_>) -> Result<(), E>,
    ) -> Result<(), VineRejected<'pool, 'id, E>> {
        let outcome = (|| {
            self.check_live().map_err(EmbargoError::Connection)?;
            // Admission already authorized and leased this export. A later
            // Release prevents new requests; it cannot cancel this older marker
            // while it waits for preceding calls to enter the upstream route.
            let route = self.ledgers.exports.get(&barrier.0.lease);
            let clients::Destination::Endpoint {
                owner: binding::Route::Vine(owner),
                location: clients::Location::Local,
                ..
            } = self
                .runtime
                .inspect(route)
                .map_err(|e| EmbargoError::Connection(Error::Graph(e)))?
            else {
                return Err(EmbargoError::Connection(Error::InvalidNetworkTarget));
            };
            let length = barrier
                .0
                .frame
                .with(messages, |root| -> Result<_, super::super::Error> {
                    let rpc_capnp::message::Disembargo(d) = root
                        .which()
                        .map_err(|_| super::super::Error::WrongMessage)?
                    else {
                        unreachable!()
                    };
                    let rpc_capnp::disembargo::context::Accept(id) = d?
                        .get_context()
                        .which()
                        .map_err(|_| super::super::Error::WrongMessage)?
                    else {
                        unreachable!()
                    };
                    let id = id?;
                    let bytes = scratch
                        .get_mut(..id.len())
                        .ok_or(super::super::Error::Capacity)?;
                    bytes.copy_from_slice(id);
                    Ok(id.len())
                })
                .map_err(|e| EmbargoError::Connection(Error::Input(e)))?
                .map_err(|e| EmbargoError::Connection(Error::Input(e)))?;
            forward(&scratch[..length], owner, messages).map_err(EmbargoError::Network)
        })();
        match outcome {
            Ok(()) => {
                barrier.release(messages, &mut self.ledgers.exports);
                Ok(())
            }
            Err(error) => Err(VineRejected { error, barrier }),
        }
    }

    /// Send an acceptance marker toward an owned provision. The affine handle
    /// prevents Finish and ID reuse until FIFO output has accepted the marker.
    pub fn send_provision_embargo(
        &mut self,
        provision: &questions::Provision<'id>,
        id: &[u8],
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
    ) -> Result<(), Error> {
        self.check_live()?;
        if !self.ledgers.questions.is_live(provision.key()) {
            return Err(Error::InvalidNetworkTarget);
        }
        let output = queue
            .reserve()
            .map_err(|e| Error::Input(super::super::Error::Storage(e)))?;
        let frame = Frame::build(messages, minimum, options, |b| {
            crate::network::disembargo_accept(
                b,
                calls::Target::Promised {
                    question: provision.key().id(),
                    path: pipeline::Path::new(&[]),
                },
                id,
            )
        })
        .map_err(Error::Input)?;
        let offer = match messages.offer(&frame.lease) {
            Ok(offer) => offer,
            Err(e) => {
                frame.release(messages);
                return Err(Error::Input(super::super::Error::Storage(e)));
            }
        };
        output.accept(messages, offer);
        frame.release(messages);
        Ok(())
    }
}
