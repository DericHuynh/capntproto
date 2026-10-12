//! Affine graph barriers paired with non-reusable wire embargo identities.
//! Register before queueing a senderLoopback through ordered dispatch. Prepare
//! output while its dispatch ticket is borrowed; accept only with FIFO capacity.
use capntproto_rpc_engine::{
    clients, embargoes,
    ids::EmbargoId,
    peer_table::{self, BorrowedSparse},
    storage::Storage as SlotStorage,
};

type Receipts<'s, 'id> = embargoes::Embargoes<
    clients::Key<'id>,
    BorrowedSparse<'s, EmbargoId, embargoes::Record<clients::Key<'id>>>,
>;
pub type ReceiptSlot<'id> = peer_table::Slot<EmbargoId, embargoes::Record<clients::Key<'id>>>;

struct Entry<'id> {
    barrier: clients::Barrier<'id>,
    sent: Option<EmbargoId>,
}
pub struct Slot<'id> {
    entry: Option<Entry<'id>>,
}
impl Slot<'_> {
    pub const EMPTY: Self = Self { entry: None };
}
#[must_use = "queue through the ordered dispatcher or terminate the connection"]
pub struct Ticket<'id> {
    key: clients::Key<'id>,
}

pub struct Bindings<'s, 'id> {
    slots: SlotStorage<'s, Slot<'id>>,
    receipts: Receipts<'s, 'id>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    NonemptyStorage,
    Capacity,
    Missing,
    AlreadySent,
    Embargo(embargoes::Error),
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "loopback binding: {self:?}")
    }
}
impl core::error::Error for Error {}
pub struct Rejected<'id> {
    pub error: Error,
    pub barrier: clients::Barrier<'id>,
}

impl<'s, 'id> Bindings<'s, 'id> {
    #[cfg(feature = "alloc")]
    pub(crate) fn observe_receipts(&mut self) -> capntproto_rpc_engine::observation::Observation {
        self.receipts.observe_len()
    }
    pub fn new(
        slots: &'s mut [Slot<'id>],
        receipts: &'s mut [ReceiptSlot<'id>],
    ) -> Result<Self, Error> {
        Self::from_storage(
            SlotStorage::borrowed(slots),
            SlotStorage::borrowed(receipts),
        )
    }
    pub(crate) fn from_storage(
        slots: SlotStorage<'s, Slot<'id>>,
        receipts: SlotStorage<'s, ReceiptSlot<'id>>,
    ) -> Result<Self, Error> {
        if slots.iter().any(|slot| slot.entry.is_some()) {
            return Err(Error::NonemptyStorage);
        }
        let receipts = embargoes::Embargoes::new(BorrowedSparse::from_storage(receipts))
            .map_err(|_| Error::NonemptyStorage)?;
        Ok(Self { slots, receipts })
    }
    pub fn register(
        &mut self,
        barrier: clients::Barrier<'id>,
    ) -> Result<Ticket<'id>, Rejected<'id>> {
        let Some(slot) = self
            .slots
            .vacant_mut(|_, slot| slot.entry.is_none(), || Slot::EMPTY)
        else {
            return Err(Rejected {
                error: Error::Capacity,
                barrier,
            });
        };
        let key = barrier.key();
        slot.entry = Some(Entry {
            barrier,
            sent: None,
        });
        Ok(Ticket { key })
    }
    pub fn barrier(&self, ticket: &Ticket<'id>) -> Result<&clients::Barrier<'id>, Error> {
        self.slots
            .iter()
            .filter_map(|slot| slot.entry.as_ref())
            .find(|entry| entry.barrier.key() == ticket.key)
            .map(|entry| &entry.barrier)
            .ok_or(Error::Missing)
    }
    /// Preflight before taking an affine graph barrier; allocation failure must
    /// occur before the transition which creates that obligation.
    pub fn reserve_capacity(&mut self) -> bool {
        self.slots
            .vacant(|_, slot| slot.entry.is_none(), || Slot::EMPTY)
            .is_some()
    }
    pub fn has_capacity(&self) -> bool {
        self.slots.iter().any(|slot| slot.entry.is_none())
    }
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|slot| slot.entry.is_none()) && self.receipts.is_empty()
    }
    pub fn prepare(&mut self, ticket: &Ticket<'id>) -> Result<Prepared<'_, 's, 'id>, Error> {
        let entry = self
            .slots
            .iter_mut()
            .filter_map(|slot| slot.entry.as_mut())
            .find(|entry| entry.barrier.key() == ticket.key)
            .ok_or(Error::Missing)?;
        if entry.sent.is_some() {
            return Err(Error::AlreadySent);
        }
        // Rejected offers own only a copied graph key. The affine barrier stays
        // in its original entry through arbitrary output retries.
        while self.receipts.take_rejected().is_some() {}
        let offered = self
            .receipts
            .reserve(ticket.key)
            .map_err(|rejected| Error::Embargo(rejected.error))?;
        Ok(Prepared { entry, offered })
    }
    pub fn receive(&mut self, id: EmbargoId) -> Result<clients::Barrier<'id>, Error> {
        let key = self
            .receipts
            .receive(id)
            .map_err(Error::Embargo)?
            .expect("bindings do not cancel accepted receipts");
        let slot = self
            .slots
            .iter_mut()
            .find(|slot| {
                slot.entry
                    .as_ref()
                    .is_some_and(|entry| entry.barrier.key() == key && entry.sent == Some(id))
            })
            .expect("accepted receipt has its exact affine barrier");
        Ok(slot.entry.take().unwrap().barrier)
    }
    /// Terminal cleanup only. Stop IO and application dispatch before dropping
    /// returned barriers and ending their graph scope; do not open them early.
    pub fn drain_one(&mut self) -> Option<clients::Barrier<'id>> {
        while self.receipts.drain_one().is_some() {}
        self.slots
            .iter_mut()
            .find_map(|slot| slot.entry.take().map(|entry| entry.barrier))
    }
}
pub struct Prepared<'b, 's, 'id> {
    entry: &'b mut Entry<'id>,
    offered: embargoes::Offer<
        'b,
        clients::Key<'id>,
        BorrowedSparse<'s, EmbargoId, embargoes::Record<clients::Key<'id>>>,
    >,
}
impl Prepared<'_, '_, '_> {
    pub fn id(&self) -> EmbargoId {
        self.offered.id()
    }
    pub fn accept(self) -> EmbargoId {
        let id = self.offered.accept();
        self.entry.sent = Some(id);
        id
    }
}

use super::*;
/// An admitted senderLoopback owns the original target through ordered dispatch.
/// Its frame can be recycled immediately; the path and opaque echo ID are copied.
pub struct Echo<'id, const PATH: usize> {
    target: super::Target<'id, 'id>,
    fields: Buffer<u16, PATH>,
    length: usize,
    context: u32,
}
impl<'id, const PATH: usize> Echo<'id, PATH> {
    pub fn context(&self) -> u32 {
        self.context
    }
    pub fn admit<'p, E, A>(
        frame: Frame<'p>,
        messages: &mut messages::Messages<'p, '_, '_>,
        exports: &mut exports::Exports<'id, '_, E>,
        answers: &mut answers::Answers<'id, '_, A>,
    ) -> Result<Self, super::Rejected<'p, ()>> {
        Self::admit_with_capacity(frame, messages, exports, answers, Capacity::Inline)
    }
    pub fn admit_with_capacity<'p, E, A>(
        frame: Frame<'p>,
        messages: &mut messages::Messages<'p, '_, '_>,
        exports: &mut exports::Exports<'id, '_, E>,
        answers: &mut answers::Answers<'id, '_, A>,
        capacity: Capacity,
    ) -> Result<Self, super::Rejected<'p, ()>> {
        let mut fields = Buffer::Inline([0; PATH]);
        let outcome = frame
            .with(messages, |root| -> Result<_, super::Error> {
                let rpc_capnp::message::Disembargo(barrier) =
                    root.which().map_err(|_| super::Error::WrongMessage)?
                else {
                    return Err(super::Error::WrongMessage);
                };
                let barrier = barrier?;
                let rpc_capnp::disembargo::context::SenderLoopback(context) = barrier
                    .get_context()
                    .which()
                    .map_err(|_| super::Error::WrongMessage)?
                else {
                    return Err(super::Error::WrongMessage);
                };
                prepare_target_path(barrier.get_target()?, &mut fields, capacity)?;
                let (target, length) =
                    match pipeline::admit(barrier.get_target()?, exports, answers, &mut fields)
                        .map_err(super::Error::Target)?
                    {
                        pipeline::Target::Hosted(lease) => (super::Target::Hosted(lease), 0),
                        pipeline::Target::Promised { answer, path } => {
                            (super::Target::Promised(answer), path.fields().len())
                        }
                    };
                Ok((target, length, context))
            })
            .and_then(core::convert::identity);
        match outcome {
            Ok((target, length, context)) => {
                frame.release(messages);
                Ok(Self {
                    target,
                    fields,
                    length,
                    context,
                })
            }
            Err(error) => Err(super::Rejected {
                error,
                frame,
                owner: (),
            }),
        }
    }
    pub fn target(&self) -> super::TargetRef<'_, 'id, 'id> {
        match &self.target {
            super::Target::Hosted(lease) => super::TargetRef::Hosted(lease),
            super::Target::Promised(answer) => super::TargetRef::Promised {
                answer,
                path: pipeline::Path::new(&self.fields[..self.length]),
            },
        }
    }
    pub fn release<E, A>(
        self,
        exports: &mut exports::Exports<'id, '_, E>,
        answers: &mut answers::Answers<'id, '_, A>,
    ) {
        match self.target {
            super::Target::Hosted(lease) => exports.release_local_deferred(lease),
            super::Target::Promised(pipeline) => answers.release_pipeline_deferred(pipeline),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use capntproto_rpc_engine::clients::{dispatch, Location, Source};
    #[test]
    fn queued_loopback_keeps_affine_barrier_through_output_rejection_and_duplicate_echo() {
        let measured = allocation_counter::measure(|| {
            let mut nodes = [const { clients::Slot::EMPTY }; 3];
            clients::with_graph(&mut nodes, |graph| {
                let mut tasks = [dispatch::TaskSlot::EMPTY];
                let mut runtime = dispatch::borrow_runtime(graph, &mut tasks).unwrap();
                let original = runtime.endpoint(17u32, Location::Remote(1u8)).unwrap();
                let promise = runtime.promise(&mut Some(original)).unwrap();
                let old = runtime
                    .prepare_call(&promise, &mut Some(None))
                    .unwrap()
                    .accept();
                assert!(runtime.cancel(old).unwrap().is_none());
                let local = runtime.endpoint(23, Location::Local).unwrap();
                let barrier = runtime
                    .prepare_resolution(promise.key(), &mut Some(local), Source::Reflected)
                    .unwrap()
                    .accept()
                    .unwrap();
                let mut slots = [Slot::EMPTY];
                let mut receipts = [ReceiptSlot::EMPTY];
                let mut bindings = Bindings::new(&mut slots, &mut receipts).unwrap();
                let ticket = bindings
                    .register(barrier)
                    .unwrap_or_else(|_| panic!("barrier"));
                let original = runtime
                    .retain_barrier_route(bindings.barrier(&ticket).unwrap())
                    .unwrap();
                runtime
                    .prepare_call(&original, &mut Some(Some(ticket)))
                    .unwrap()
                    .accept();
                runtime.release(original);
                let ready = runtime.next_ready().unwrap().unwrap();
                assert!(matches!(
                    ready.destination(),
                    clients::Destination::Endpoint { owner: &17, .. }
                ));
                let output = bindings.prepare(ready.owner().as_ref().unwrap()).unwrap();
                let rejected_id = output.id();
                drop(output);
                assert!(matches!(
                    bindings.receive(rejected_id),
                    Err(Error::Embargo(embargoes::Error::NotAccepted))
                ));
                // Queue acceptance can be retried without losing the graph
                // barrier or accepting its old rejected wire identity.
                let output = bindings.prepare(ready.owner().as_ref().unwrap()).unwrap();
                let id = output.id();
                assert_ne!(id, rejected_id);
                let running = ready.accept();
                output.accept();
                let ticket = runtime.complete(running).unwrap();
                assert!(matches!(bindings.prepare(&ticket), Err(Error::AlreadySent)));
                runtime
                    .prepare_call(&promise, &mut Some(None))
                    .unwrap()
                    .accept();
                assert!(runtime.next_ready().unwrap().is_none());
                assert!(matches!(
                    bindings.receive(rejected_id),
                    Err(Error::Embargo(embargoes::Error::Unknown))
                ));
                runtime.open(bindings.receive(id).unwrap());
                assert!(matches!(
                    bindings.receive(id),
                    Err(Error::Embargo(embargoes::Error::Unknown))
                ));
                let ready = runtime.next_ready().unwrap().unwrap();
                assert!(matches!(
                    ready.destination(),
                    clients::Destination::Endpoint { owner: &23, .. }
                ));
                let running = ready.accept();
                assert!(runtime.complete(running).is_none());
                runtime.release(promise);
                assert_eq!(runtime.take_retired(), Some(17));
                assert_eq!(runtime.take_retired(), Some(23));
                assert!(bindings.is_empty() && runtime.is_empty());
            })
            .unwrap();
        });
        assert_eq!(measured.count_total, 0);
    }
}
