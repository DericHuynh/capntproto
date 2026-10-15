//! Incoming call ownership and exactly-once Return publication over caller slots.
//! An early Finish can cancel queued work; a running method is cancelled only
//! when its generated policy permits it. Return is still owed after cancellation.
#![forbid(unsafe_code)]
use crate::ids::AnswerId;
use core::marker::PhantomData;
#[cfg(capntproto_verus)]
use vstd::prelude::*;
#[cfg(capntproto_verus)]
include!("answers/verified.rs");
mod admission;
mod network;
mod third_party;
pub use third_party::ThirdPartyAdmission;
mod pipelines;
pub use admission::Admission;
pub use pipelines::{PipelineAdmission, PipelineError, PipelineSlot, Pipelines};
#[cfg_attr(capntproto_verus, verus_verify)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Redirect {
    None,
    Available,
    Adopted,
    ThirdParty,
}

type Brand<'id> = PhantomData<fn(&'id ()) -> &'id ()>;

#[cfg_attr(capntproto_verus, verus_verify)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    NonemptyStorage,
    Full,
    Duplicate,
    GenerationExhausted,
    StaleKey,
    AlreadyDispatched,
    AlreadyReturned,
    NotRedirected,
    AlreadyAdopted,
    Finished,
    ReferenceLimit,
    NotCallable,
    FinishRequired,
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "answer ledger: {self:?}")
    }
}
impl core::error::Error for Error {}
#[derive(Debug)]
pub struct Rejected<T> {
    pub error: Error,
    pub value: T,
}

#[cfg_attr(capntproto_verus, verus_verify)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Queued,
    Running { cancellation: bool },
    Returned,
}
#[cfg_attr(capntproto_verus, verus_verify)]
pub struct Record<T> {
    #[cfg(feature = "alloc")]
    presence: crate::observation::Presence,
    id: AnswerId,
    generation: u64,
    state: State,
    finished: bool,
    pipelines: u32,
    redirect: Redirect,
    allow_handoff: bool,
    provision: bool,
    value: T,
}
impl<T> Record<T> {
    #[inline]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == reclaimable(*self)))]
    fn reclaimable(&self) -> bool {
        self.finished && matches!(self.state, State::Returned) && self.pipelines == 0
    }
    #[inline]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == wire_live(*self)))]
    fn wire_live(&self) -> bool {
        !(self.finished && matches!(self.state, State::Returned))
    }
    #[inline]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == cancel_requested(*self)))]
    fn cancellation_requested(&self) -> bool {
        self.finished
            && self.pipelines == 0
            && matches!(
                self.state,
                State::Queued | State::Running { cancellation: true }
            )
    }
    #[inline]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures retained(*old(self), *final(self), r)))]
    fn retain_pipeline(&mut self) -> Result<(), Error> {
        self.pipelines = self.pipelines.checked_add(1).ok_or(Error::ReferenceLimit)?;
        Ok(())
    }
    #[inline]
    #[cfg_attr(capntproto_verus, verus_spec( requires old(self).pipelines > 0,
        ensures *final(self) == (Record { pipelines: (old(self).pipelines - 1) as u32, ..*old(self) })
    ))]
    fn release_pipeline(&mut self) {
        self.pipelines -= 1;
    }
    #[inline]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures dispatched(*old(self), *final(self), cancellation, r)))]
    fn dispatch(&mut self, cancellation: bool) -> Result<bool, Error> {
        if !matches!(self.state, State::Queued) {
            return Err(Error::AlreadyDispatched);
        }
        self.state = State::Running { cancellation };
        Ok(!self.finished || self.pipelines != 0)
    }
    #[inline]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures finished(*old(self), *final(self), release, r)))]
    fn finish(&mut self, release: bool) -> Option<(bool, bool)> {
        if self.finished {
            return None;
        }
        self.finished = true;
        Some((self.cancellation_requested(), release && !self.provision))
    }
    #[inline]
    #[cfg_attr(capntproto_verus, verus_spec(ensures
        *final(self) == (Record { state: State::Returned, finished: old(self).finished || no_finish, ..*old(self) })
    ))]
    fn accept_return(&mut self, no_finish: bool) {
        self.finished |= no_finish;
        self.state = State::Returned;
    }
    #[inline]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == (omission_allowed && self.pipelines == 0)))]
    fn can_retire_on_return(&self, omission_allowed: bool) -> bool {
        omission_allowed && self.pipelines == 0
    }
    #[inline]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == adoption_result(*self)))]
    fn prepare_adoption(&self) -> Result<(), Error> {
        match self.redirect {
            Redirect::None | Redirect::ThirdParty => return Err(Error::NotRedirected),
            Redirect::Adopted => return Err(Error::AlreadyAdopted),
            Redirect::Available => (),
        }
        self.pipelines.checked_add(1).ok_or(Error::ReferenceLimit)?;
        Ok(())
    }
    #[inline]
    #[cfg_attr(capntproto_verus, verus_spec(requires adoption_result(*old(self)) == Ok(()),
        ensures *final(self) == (Record { redirect: Redirect::Adopted,
            pipelines: (old(self).pipelines as int + if retain { 1int } else { 0int }) as u32, ..*old(self) })
    ))]
    fn adopt(&mut self, retain: bool) {
        self.redirect = Redirect::Adopted;
        if retain {
            self.pipelines += 1;
        }
    }
}
/// Caller storage holds both wire-live answers and retired answers retained by
/// local pipeline leases. A retired record no longer reserves its peer ID.
pub struct Slot<T> {
    record: Option<Record<T>>,
}
impl<T> Slot<T> {
    pub const EMPTY: Self = Self { record: None };
    pub fn is_empty(&self) -> bool {
        self.record.is_none()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key<'id> {
    index: usize,
    id: AnswerId,
    generation: u64,
    brand: Brand<'id>,
}
impl Key<'_> {
    pub const fn id(self) -> AnswerId {
        self.id
    }
}

/// A previously admitted promised-answer call owns its source answer even if
/// the peer sends Finish before the application produces the promised result.
#[derive(Debug)]
#[must_use = "release this pipeline lease after resolving/routing the queued call"]
pub struct Pipeline<'id> {
    key: Key<'id>,
}
impl<'id> Pipeline<'id> {
    /// The retained slot/generation identity, including after Finish and reuse
    /// of its former wire ID. This key does not create another owning reference.
    pub fn key(&self) -> Key<'id> {
        self.key
    }
}

pub struct Answers<'id, 'storage, T> {
    #[cfg(feature = "alloc")]
    observation: crate::observation::Counter,
    slots: crate::storage::Storage<'storage, Slot<T>>,
    generation: u64,
    brand: Brand<'id>,
}
pub fn with_answers<T, R>(
    slots: &mut [Slot<T>],
    f: impl for<'id> FnOnce(&mut Answers<'id, '_, T>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_scope(slots)?))
}
/// Caller-polled asynchronous domain. Answers/pipelines can survive suspension;
/// cancellation of the enclosing future ends the connection's local ownership.
pub async fn with_answers_async<'storage, T, R>(
    slots: &'storage mut [Slot<T>],
    f: impl for<'id> core::ops::AsyncFnOnce(&mut Answers<'id, 'storage, T>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_scope(slots)?).await)
}
pub(crate) fn new_scope<'id, T>(slots: &mut [Slot<T>]) -> Result<Answers<'id, '_, T>, Error> {
    if slots.iter().any(|slot| !slot.is_empty()) {
        return Err(Error::NonemptyStorage);
    }
    Ok(Answers {
        #[cfg(feature = "alloc")]
        observation: Default::default(),
        slots: crate::storage::Storage::borrowed(slots),
        generation: 0,
        brand: PhantomData,
    })
}

#[cfg(feature = "alloc")]
pub(crate) fn new_growing_scope<'id, 's, T>(
    initial: usize,
    maximum: usize,
) -> Result<Answers<'id, 's, T>, Error> {
    let slots =
        crate::storage::Storage::growing(initial, maximum, || Slot::EMPTY).ok_or(Error::Full)?;
    Ok(Answers {
        #[cfg(feature = "alloc")]
        observation: Default::default(),
        slots,
        generation: 0,
        brand: PhantomData,
    })
}
#[cfg(feature = "alloc")]
pub fn with_growing_answers<T, R>(
    initial: usize,
    maximum: usize,
    f: impl for<'id> FnOnce(&mut Answers<'id, '_, T>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_growing_scope(initial, maximum)?))
}

impl<'id, 'storage, T> Answers<'id, 'storage, T> {
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }
    /// Conservatively certify that no finished answer can be reclaimed,
    /// examining at most `maximum` slots. Larger tables return false and must
    /// use the budgeted scan. Returned answers still awaiting Finish or a local
    /// pipeline lease retain their owner and do not require reclamation work.
    pub fn reclamation_is_idle(&self, maximum: usize) -> bool {
        self.slots.len() <= maximum
            && self.slots.iter().all(|slot| {
                slot.record
                    .as_ref()
                    .is_none_or(|record| !record.reclaimable())
            })
    }
    /// Includes detached records retained by already-admitted pipeline calls.
    #[cfg(feature = "alloc")]
    pub fn observe_len(&mut self) -> crate::observation::Observation {
        self.observation.observe(
            self.slots
                .iter_mut()
                .filter_map(|s| s.record.as_mut())
                .map(|r| &mut r.presence),
        )
    }
    pub fn len(&self) -> usize {
        self.slots.iter().filter(|s| !s.is_empty()).count()
    }
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(Slot::is_empty)
    }
    pub fn admit(&mut self, id: AnswerId, value: T) -> Result<Key<'id>, Rejected<T>> {
        match self.prepare_admission(id, None) {
            Ok(pending) => Ok(pending.accept(value).0),
            Err(error) => Err(Rejected { error, value }),
        }
    }
    fn record(&self, key: Key<'id>) -> Result<&Record<T>, Error> {
        self.slots
            .get(key.index)
            .and_then(|s| s.record.as_ref())
            .filter(|r| r.generation == key.generation)
            .ok_or(Error::StaleKey)
    }
    fn record_mut(&mut self, key: Key<'id>) -> Result<&mut Record<T>, Error> {
        self.slots
            .get_mut(key.index)
            .and_then(|s| s.record.as_mut())
            .filter(|r| r.generation == key.generation)
            .ok_or(Error::StaleKey)
    }
    pub fn is_third_party(&self, key: Key<'id>) -> Result<bool, Error> {
        Ok(self.record(key)?.redirect == Redirect::ThirdParty)
    }
    pub fn get(&self, key: Key<'id>) -> Result<&T, Error> {
        Ok(&self.record(key)?.value)
    }
    pub fn get_mut(&mut self, key: Key<'id>) -> Result<&mut T, Error> {
        Ok(&mut self.record_mut(key)?.value)
    }
    pub fn lookup(&self, id: AnswerId) -> Option<Key<'id>> {
        let index = self.wire_index(id)?;
        self.slots[index]
            .record
            .as_ref()
            .filter(|r| !r.finished)
            .map(|r| Key {
                index,
                id,
                generation: r.generation,
                brand: PhantomData,
            })
    }
    pub fn pipeline(&mut self, id: AnswerId) -> Result<Pipeline<'id>, Error> {
        let index = self.wire_index(id).ok_or(Error::StaleKey)?;
        let r = self.slots[index].record.as_mut().unwrap();
        if r.finished {
            return Err(Error::Finished);
        }
        if r.provision {
            return Err(Error::NotCallable);
        }
        r.retain_pipeline()?;
        Ok(Pipeline {
            key: Key {
                index,
                id,
                generation: r.generation,
                brand: PhantomData,
            },
        })
    }
    pub fn pipelined(&self, lease: &Pipeline<'id>) -> &T {
        &self.record(lease.key).expect("live pipeline lease").value
    }
    /// Retain an already admitted dependency even after its peer sends Finish.
    /// Wire lookup is intentionally not used: that ID may already be reused.
    pub fn retain_pipeline(&mut self, lease: &Pipeline<'id>) -> Result<Pipeline<'id>, Error> {
        let record = self.record_mut(lease.key)?;
        record.retain_pipeline()?;
        Ok(Pipeline { key: lease.key })
    }
    /// Re-evaluate when polling a task or after releasing a pipeline lease.
    /// Accepted pipeline calls still need the result after their parent's Finish.
    pub fn cancellation_requested(&self, key: Key<'id>) -> Result<bool, Error> {
        let r = self.record(key)?;
        Ok(r.cancellation_requested())
    }
    pub fn release_pipeline(&mut self, lease: Pipeline<'id>) -> Option<T> {
        let index = lease.key.index;
        self.release_pipeline_deferred(lease);
        self.reclaim(index)
    }
    /// Retain the finished owner for explicit draining outside ledger borrows.
    pub fn release_pipeline_deferred(&mut self, lease: Pipeline<'id>) {
        let r = self.record_mut(lease.key).expect("live pipeline lease");
        r.release_pipeline();
    }
    pub fn take_finished(&mut self, index: usize) -> Option<T> {
        self.slots.get(index)?;
        self.reclaim(index)
    }
    fn wire_index(&self, id: AnswerId) -> Option<usize> {
        self.slots.iter().position(|s| {
            s.record
                .as_ref()
                .is_some_and(|r| r.id == id && r.wire_live())
        })
    }
    #[inline(always)]
    fn reclaim(&mut self, index: usize) -> Option<T> {
        if self.slots[index]
            .record
            .as_ref()
            .is_some_and(Record::reclaimable)
        {
            self.slots[index].record.take().map(|r| r.value)
        } else {
            None
        }
    }
    /// False means Finish arrived while queued and no admitted pipeline still
    /// needs the result: skip application execution and publish cancelled Return.
    pub fn dispatch(&mut self, key: Key<'id>, allow_cancellation: bool) -> Result<bool, Error> {
        let r = self.record_mut(key)?;
        r.dispatch(allow_cancellation)
    }
    /// Unknown/late/duplicate Finish is harmless, as in the hosted runtime.
    /// Result grants may be released only on the first Finish. Returned owners
    /// must be cleaned up after the ledger borrow is released.
    pub fn finish(&mut self, id: AnswerId, release_result_caps: bool) -> Finished<T> {
        let Some(index) = self.wire_index(id) else {
            return Finished::ignored();
        };
        let r = self.slots[index].record.as_mut().unwrap();
        let Some((cancel, release_result_caps)) = r.finish(release_result_caps) else {
            return Finished::ignored();
        };
        let retired = self.reclaim(index);
        Finished {
            cancel,
            release_result_caps,
            retired,
        }
    }
    pub fn prepare_return(&mut self, key: Key<'id>) -> Result<Return<'_, 'id, 'storage, T>, Error> {
        if self.record(key)?.state == State::Returned {
            return Err(Error::AlreadyReturned);
        }
        Ok(Return {
            answers: self,
            key,
            no_finish: false,
        })
    }
}
impl<T> Drop for Answers<'_, '_, T> {
    fn drop(&mut self) {
        for slot in self.slots.iter_mut() {
            drop(slot.record.take());
        }
    }
}
impl<T> Answers<'_, '_, T> {
    pub(crate) fn take_terminal_owner(&mut self, index: usize) -> Option<T> {
        self.slots
            .get_mut(index)?
            .record
            .take()
            .map(|record| record.value)
    }
}
pub struct Finished<T> {
    pub cancel: bool,
    pub release_result_caps: bool,
    pub retired: Option<T>,
}
impl<T> Finished<T> {
    fn ignored() -> Self {
        Self {
            cancel: false,
            release_result_caps: false,
            retired: None,
        }
    }
}

/// Exclusive publication ticket. Drop/rejection keeps the answer and its
/// owner live; accept is called with the immutable Return's FIFO admission.
#[must_use]
pub struct Return<'a, 'id, 'storage, T> {
    answers: &'a mut Answers<'id, 'storage, T>,
    key: Key<'id>,
    no_finish: bool,
}
impl<T> Return<'_, '_, '_, T> {
    pub fn id(&self) -> AnswerId {
        self.key.id
    }
    pub fn is_redirected(&self) -> bool {
        self.answers.record(self.key).unwrap().redirect != Redirect::None
    }
    pub fn allows_handoff(&self) -> bool {
        self.answers.record(self.key).unwrap().allow_handoff
    }
    pub fn is_cancelled(&self) -> bool {
        self.answers.record(self.key).unwrap().finished
    }
    /// Ordinary successful results can release their wire answer with the
    /// Return. The wire layer must additionally validate an empty capability
    /// table. Redirects, provisions and callee-allocated IDs need explicit Finish.
    pub fn can_omit_finish(&self) -> bool {
        use crate::ids::WireId;
        let record = self.answers.record(self.key).unwrap();
        !record.finished
            && record.redirect == Redirect::None
            && !record.provision
            && self.key.id.to_wire() < (1 << 30)
    }
    /// A data-only Return can retire immediately when no admitted pipeline
    /// needs a retained result. Omission still requires wire-layer validation.
    pub fn can_retire_without_finish(&self) -> bool {
        self.answers
            .record(self.key)
            .unwrap()
            .can_retire_on_return(self.can_omit_finish())
    }
    /// Select omission on this publication ticket only. Rejection/drop leaves
    /// the answer untouched; admitted local pipelines survive acceptance.
    pub fn omit_finish(&mut self) -> Result<(), Error> {
        if !self.can_omit_finish() {
            return Err(Error::FinishRequired);
        }
        self.no_finish = true;
        Ok(())
    }
    // Consume the detached owner at its caller so large inline protocol state
    // does not cross an additional return-value boundary.
    #[inline(always)]
    pub fn accept(self) -> Option<T> {
        let r = self.answers.record_mut(self.key).unwrap();
        r.accept_return(self.no_finish);
        self.answers.reclaim(self.key.index)
    }
}

#[cfg(kani)]
mod proofs;
#[cfg(test)]
mod tests;

mod adoption;
pub use adoption::{Adopted, Adoption};

mod finish;
pub use finish::Finish;
mod provision;
pub use provision::{ProvisionAdmission, ProvisionPipelines};
