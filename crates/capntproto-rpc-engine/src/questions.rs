//! Scoped outgoing calls with transactional admission and retained Finish work.
//! A question ID cannot be recycled until Return and accepted Finish (when
//! needed), even if cancellation races with Return or output is backpressured.
#![forbid(unsafe_code)]

use crate::{
    ids::QuestionId,
    local_table::{self, BorrowedSlots, LocalTable},
    peer_table::{self, BorrowedSparse, NoSparse},
    question::{self, Question},
};
use core::marker::PhantomData;
type Brand<'id> = PhantomData<fn(&'id ()) -> &'id ()>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    NonemptyStorage,
    Full,
    GenerationExhausted,
    StaleKey,
    ReferenceLimit,
    UnknownQuestion,
    DuplicateReturn,
    PipelineReleased,
    InvalidAdoptedId,
    DuplicateAdoption,
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "question ledger: {self:?}")
    }
}
impl core::error::Error for Error {}

/// Opaque caller-storage record. Use Slot::EMPTY to initialize storage.
/// Result ownership fixed when an outgoing question is reserved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultsTo {
    Caller,
    Yourself,
    ThirdParty,
}
pub struct Record {
    #[cfg(feature = "alloc")]
    presence: crate::observation::Presence,
    #[cfg(feature = "alloc")]
    credit: Option<alloc::rc::Rc<crate::flow_control::outgoing::OwnedPermit>>,
    id: QuestionId,
    generation: u64,
    references: u32,
    lifecycle: Question,
    finish: Option<question::Finish>,
    published: bool,
    destination: ResultsTo,
    provision: bool,
}
pub type Slot = local_table::Slot<Record>;
pub type AdoptedSlot = peer_table::Slot<QuestionId, Record>;
type Table<'s> = LocalTable<
    QuestionId,
    Record,
    QuestionSlots<'s>,
    NoSparse,
    BorrowedSparse<'s, QuestionId, Record>,
>;

// Both providers run the same question and local-ID transitions. Fixed storage
// never falls back to allocation when the alloc feature is enabled.
enum QuestionSlots<'s> {
    Fixed(BorrowedSlots<'s, Record>),
    #[cfg(feature = "alloc")]
    Growing(local_table::GrowingSlots<Record>),
}
impl local_table::Storage<Record> for QuestionSlots<'_> {
    fn slots(&self) -> &[Slot] {
        match self {
            Self::Fixed(slots) => slots.slots(),
            #[cfg(feature = "alloc")]
            Self::Growing(slots) => slots.slots(),
        }
    }
    fn slots_mut(&mut self) -> &mut [Slot] {
        match self {
            Self::Fixed(slots) => slots.slots_mut(),
            #[cfg(feature = "alloc")]
            Self::Growing(slots) => slots.slots_mut(),
        }
    }
    fn try_grow(&mut self) -> bool {
        match self {
            Self::Fixed(slots) => slots.try_grow(),
            #[cfg(feature = "alloc")]
            Self::Growing(slots) => slots.try_grow(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key<'id> {
    id: QuestionId,
    generation: u64,
    brand: Brand<'id>,
}
impl Key<'_> {
    pub const fn id(self) -> QuestionId {
        self.id
    }
}

#[derive(Debug)]
#[must_use = "release through its question ledger; dropping retains it until teardown"]
pub struct Local<'id> {
    key: Key<'id>,
}
impl<'id> Local<'id> {
    pub const fn key(&self) -> Key<'id> {
        self.key
    }
}

pub struct Questions<'id, 'storage> {
    #[cfg(feature = "alloc")]
    observation: crate::observation::Counter,
    table: Table<'storage>,
    generation: u64,
    pending_finishes: usize,
    brand: Brand<'id>,
}
pub fn with_questions<R>(
    storage: &mut [Slot],
    f: impl for<'id> FnOnce(&mut Questions<'id, '_>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_scope(storage)?))
}
/// Retain question identities through caller-polled suspension without boxing.
/// Ending the scope with outstanding calls is terminal connection teardown.
pub async fn with_questions_async<'storage, R>(
    storage: &'storage mut [Slot],
    f: impl for<'id> core::ops::AsyncFnOnce(&mut Questions<'id, 'storage>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_scope(storage)?).await)
}
pub(crate) fn new_scope<'id>(storage: &mut [Slot]) -> Result<Questions<'id, '_>, Error> {
    new_scope_with_adoptions(storage, &mut [])
}
pub(crate) fn new_scope_with_adoptions<'id, 's>(
    storage: &'s mut [Slot],
    adopted: &'s mut [AdoptedSlot],
) -> Result<Questions<'id, 's>, Error> {
    if storage.iter().any(|slot| !slot.is_empty()) || adopted.iter().any(|slot| !slot.is_empty()) {
        return Err(Error::NonemptyStorage);
    }
    let table = LocalTable::try_new(
        QuestionSlots::Fixed(BorrowedSlots::new(storage)),
        NoSparse,
        BorrowedSparse::new(adopted),
    )
    .map_err(|_| Error::NonemptyStorage)?;
    Ok(Questions {
        #[cfg(feature = "alloc")]
        observation: Default::default(),
        table,
        generation: 0,
        pending_finishes: 0,
        brand: PhantomData,
    })
}

#[cfg(feature = "alloc")]
pub(crate) fn new_growing_scope<'id, 's>(
    initial: usize,
    maximum: usize,
    adopted: &'s mut [AdoptedSlot],
) -> Result<Questions<'id, 's>, Error> {
    if adopted.iter().any(|slot| !slot.is_empty()) {
        return Err(Error::NonemptyStorage);
    }
    new_growing_with_adoptions(initial, maximum, BorrowedSparse::new(adopted))
}

#[cfg(feature = "alloc")]
pub(crate) fn new_growing_with_adoptions<'id, 's>(
    initial: usize,
    maximum: usize,
    adopted: BorrowedSparse<'s, QuestionId, Record>,
) -> Result<Questions<'id, 's>, Error> {
    let dense =
        local_table::GrowingSlots::try_with_capacity(initial, maximum).ok_or(Error::Full)?;
    let table = LocalTable::try_new(QuestionSlots::Growing(dense), NoSparse, adopted)
        .map_err(|_| Error::NonemptyStorage)?;
    Ok(Questions {
        #[cfg(feature = "alloc")]
        observation: Default::default(),
        table,
        generation: 0,
        pending_finishes: 0,
        brand: PhantomData,
    })
}

/// Growing ordinary calls with an independent caller-backed adoption budget.
/// Allocation failure returns Full; it never consumes a question ID or owner.
#[cfg(feature = "alloc")]
pub fn with_growing_questions<R>(
    initial: usize,
    maximum: usize,
    adopted: &mut [AdoptedSlot],
    f: impl for<'id> FnOnce(&mut Questions<'id, '_>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_growing_scope(initial, maximum, adopted)?))
}

/// Independently bounded growth for ordinary and callee-selected questions.
/// Adopted IDs consume entries, never storage indexed by the numeric wire ID.
/// Both providers use the same cancellation, Return and Finish transitions.
#[cfg(feature = "alloc")]
pub fn with_growing_adoptions<R>(
    ordinary: crate::connection::Growth,
    adopted: crate::connection::Growth,
    f: impl for<'id> FnOnce(&mut Questions<'id, '_>) -> R,
) -> Result<R, Error> {
    let adopted = BorrowedSparse::growing(adopted.initial, adopted.maximum).ok_or(Error::Full)?;
    Ok(f(&mut new_growing_with_adoptions(
        ordinary.initial,
        ordinary.maximum,
        adopted,
    )?))
}

impl<'id, 'storage> Questions<'id, 'storage> {
    #[cfg(feature = "alloc")]
    pub fn observe_len(&mut self) -> crate::observation::Observation {
        self.observation
            .observe(self.table.values_mut().map(|r| &mut r.presence))
    }
    pub fn len(&self) -> usize {
        self.table.len()
    }
    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }
    pub fn reserve(&mut self) -> Result<Reservation<'_, 'id, 'storage>, Error> {
        self.reserve_kind(ResultsTo::Caller)
    }
    /// Reserve a tail call whose remote acknowledgement carries no results.
    pub fn reserve_redirected(&mut self) -> Result<Reservation<'_, 'id, 'storage>, Error> {
        self.reserve_kind(ResultsTo::Yourself)
    }
    /// Reserve a network destination, which is ineligible for local tail adoption.
    pub fn reserve_third_party(&mut self) -> Result<Reservation<'_, 'id, 'storage>, Error> {
        self.reserve_kind(ResultsTo::ThirdParty)
    }
    fn reserve_kind(
        &mut self,
        destination: ResultsTo,
    ) -> Result<Reservation<'_, 'id, 'storage>, Error> {
        let generation = self
            .generation
            .checked_add(1)
            .ok_or(Error::GenerationExhausted)?;
        let id = self
            .table
            .try_push(Record {
                #[cfg(feature = "alloc")]
                presence: self.observation.enter(),
                #[cfg(feature = "alloc")]
                credit: None,
                id: <QuestionId as crate::ids::WireId>::from_wire(0),
                generation,
                references: 1,
                lifecycle: Question::new(),
                finish: None,
                published: false,
                destination,
                provision: false,
            })
            .map_err(|_| Error::Full)?;
        self.generation = generation;
        self.table.get_mut(id).unwrap().id = id;
        Ok(Reservation {
            questions: self,
            key: Key {
                id,
                generation,
                brand: PhantomData,
            },
            committed: false,
        })
    }
    fn record(&self, key: Key<'id>) -> Result<&Record, Error> {
        self.table
            .get(key.id)
            .filter(|r| r.generation == key.generation)
            .ok_or(Error::StaleKey)
    }
    fn record_mut(&mut self, key: Key<'id>) -> Result<&mut Record, Error> {
        self.table
            .get_mut(key.id)
            .filter(|r| r.generation == key.generation)
            .ok_or(Error::StaleKey)
    }
    pub fn is_live(&self, key: Key<'id>) -> bool {
        self.record(key)
            .is_ok_and(|r| r.published && r.references != 0)
    }
    /// Validate an owning driver's identity even after its last local observer
    /// cancels. A pending late Return still belongs to this exact generation.
    pub fn contains(&self, key: Key<'id>) -> bool {
        self.record(key).is_ok_and(|record| record.published)
    }
    /// Attach a hosted Call's reservation to this exact question generation.
    /// The guard invokes no callbacks and survives cancellation until both the
    /// Return and Finish obligations retire. Fixed profiles allocate nothing.
    #[cfg(feature = "alloc")]
    pub fn retain_credit(
        &mut self,
        key: Key<'id>,
        credit: alloc::rc::Rc<crate::flow_control::outgoing::OwnedPermit>,
    ) -> Result<(), Error> {
        let record = self.record_mut(key)?;
        // One question owns one reservation. Repeated attachment of the same
        // owner is harmless; replacing it would prematurely release old credit.
        if let Some(current) = &record.credit {
            if !alloc::rc::Rc::ptr_eq(current, &credit) {
                return Err(Error::StaleKey);
            }
        } else {
            record.credit = Some(credit);
        }
        Ok(())
    }
    /// A receiverAnswer descriptor must retain this local reference through
    /// output acceptance. A noFinishNeeded Return closes the pipeline even
    /// while the application keeps the result/question reference alive.
    pub fn pipeline_id(&self, local: &Local<'id>) -> Result<QuestionId, Error> {
        let record = self.record(local.key)?;
        if !record.published
            || record.references == 0
            || record.provision
            || !record.lifecycle.can_pipeline()
        {
            return Err(Error::PipelineReleased);
        }
        Ok(record.id)
    }
    pub fn retain(&mut self, local: &Local<'id>) -> Result<Local<'id>, Error> {
        let record = self.record_mut(local.key)?;
        record.references = record
            .references
            .checked_add(1)
            .ok_or(Error::ReferenceLimit)?;
        Ok(Local { key: local.key })
    }
    /// Keep the Finish obligation in the record until output accepts it.
    pub fn release(&mut self, local: Local<'id>) {
        let record = self.record_mut(local.key).expect("live local question");
        record.references -= 1;
        if record.references == 0 {
            record.finish = record.lifecycle.release().finish;
            if record.finish.is_some() {
                self.pending_finishes += 1;
            }
            self.reclaim(local.key.id);
        }
    }
    fn reclaim(&mut self, id: QuestionId) {
        if self.table.get(id).is_some_and(|r| {
            r.references == 0 && r.finish.is_none() && !r.lifecycle.is_awaiting_return()
        }) {
            self.table.remove(id);
        }
    }
    /// Decode/validate the Return and its descriptors before this transition.
    /// A discarded response must not import result capabilities: the earlier
    /// Finish releases them remotely. No callbacks or IO run during admission.
    pub fn returned(
        &mut self,
        id: QuestionId,
        no_finish_needed: bool,
    ) -> Result<(Key<'id>, question::Delivery), Error> {
        let record = self
            .table
            .get_mut(id)
            .filter(|r| r.published)
            .ok_or(Error::UnknownQuestion)?;
        let delivery = record
            .lifecycle
            .returned(no_finish_needed)
            .map_err(|_| Error::DuplicateReturn)?;
        let key = Key {
            id,
            generation: record.generation,
            brand: PhantomData,
        };
        self.reclaim(id);
        Ok((key, delivery))
    }
    pub fn results_to(&self, id: QuestionId) -> Result<ResultsTo, Error> {
        self.table
            .get(id)
            .filter(|r| r.published)
            .map(|r| r.destination)
            .ok_or(Error::UnknownQuestion)
    }
    pub fn is_redirected(&self, id: QuestionId) -> Result<bool, Error> {
        self.table
            .get(id)
            .filter(|r| r.published)
            .map(|r| r.destination != ResultsTo::Caller)
            .ok_or(Error::UnknownQuestion)
    }
    pub fn pending_finish(&mut self) -> Option<Finish<'_, 'id, 'storage>> {
        if !self.has_pending_finish() {
            return None;
        }
        let key = self
            .table
            .values()
            .find(|r| r.finish.is_some())
            .map(|r| Key {
                id: r.id,
                generation: r.generation,
                brand: PhantomData,
            })?;
        Some(Finish {
            questions: self,
            key,
        })
    }
    pub fn has_pending_finish(&self) -> bool {
        self.pending_finishes != 0
    }
    /// Inspect response delivery without releasing the outstanding question.
    /// Accept only after payload and associated capability effects are prepared.
    pub fn prepare_receive(
        &mut self,
        id: QuestionId,
        no_finish_needed: bool,
    ) -> Result<Receive<'_, 'id, 'storage>, Error> {
        let record = self
            .table
            .get(id)
            .filter(|r| r.published)
            .ok_or(Error::UnknownQuestion)?;
        let delivery = record
            .lifecycle
            .return_delivery()
            .map_err(|_| Error::DuplicateReturn)?;
        Ok(Receive {
            questions: self,
            id,
            no_finish_needed,
            delivery,
        })
    }
}

/// Rejected input leaves Return outstanding. Exclusive preflight prevents a
/// cancellation or duplicate Return from changing the selected delivery mode.
#[must_use]
pub struct Receive<'a, 'id, 'storage> {
    questions: &'a mut Questions<'id, 'storage>,
    id: QuestionId,
    no_finish_needed: bool,
    delivery: question::Delivery,
}
impl<'id> Receive<'_, 'id, '_> {
    pub fn delivery(&self) -> question::Delivery {
        self.delivery
    }
    pub fn accept(self) -> (Key<'id>, question::Delivery) {
        self.questions
            .returned(self.id, self.no_finish_needed)
            .expect("preflight Return")
    }
}

/// An unpublished question. Failure/drop rolls its ID back; no Finish is owed.
#[must_use]
pub struct Reservation<'a, 'id, 'storage> {
    questions: &'a mut Questions<'id, 'storage>,
    key: Key<'id>,
    committed: bool,
}
impl<'id, 'storage> Reservation<'_, 'id, 'storage> {
    pub fn id(&self) -> QuestionId {
        self.key.id
    }
    /// Borrow the same ledger while this new question remains unpublished.
    /// Combined admission uses this to validate and release retained pipeline
    /// references without giving up the new question's rollback guard.
    pub fn ledger(&mut self) -> &mut Questions<'id, 'storage> {
        self.questions
    }
    pub fn destination(&self) -> ResultsTo {
        self.questions.record(self.key).unwrap().destination
    }
    pub fn is_redirected(&self) -> bool {
        self.destination() != ResultsTo::Caller
    }
    /// Call only as the corresponding immutable message enters ordered output.
    pub fn accept(mut self) -> Local<'id> {
        self.questions.record_mut(self.key).unwrap().published = true;
        self.committed = true;
        Local { key: self.key }
    }
}
impl Drop for Reservation<'_, '_, '_> {
    fn drop(&mut self) {
        if !self.committed {
            self.questions.table.remove(self.key.id);
        }
    }
}

/// Retryable control work. Inspect/serialize, reserve output, then accept. Drop
/// leaves it pending; an exhausted ordinary-call budget cannot lose cleanup.
#[must_use]
pub struct Finish<'a, 'id, 'storage> {
    questions: &'a mut Questions<'id, 'storage>,
    key: Key<'id>,
}
impl Finish<'_, '_, '_> {
    pub fn id(&self) -> QuestionId {
        self.key.id
    }
    pub fn release_result_caps(&self) -> bool {
        self.questions
            .record(self.key)
            .unwrap()
            .finish
            .unwrap()
            .release_result_caps
    }
    pub fn accept(self) {
        self.questions.record_mut(self.key).unwrap().finish = None;
        self.questions.pending_finishes -= 1;
        self.questions.reclaim(self.key.id);
    }
}

#[cfg(kani)]
mod proofs;
mod provision;
#[cfg(test)]
mod tests;
pub use provision::{Provision, ProvisionReservation};
mod adoption;
mod network;
pub use adoption::{with_adoptions, AdoptionReservation};
