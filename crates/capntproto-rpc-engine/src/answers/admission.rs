//! Preflight answer insertion together with an optional pipeline dependency.
use super::*;

/// Exclusive preflight. Rejection or drop leaves both records untouched,
/// including generations and the parent's pipeline reference count. Optional
/// growing storage can retain reserved empty capacity after rollback.
#[must_use]
pub struct Admission<'a, 'id, 'storage, T> {
    pub(super) answers: &'a mut Answers<'id, 'storage, T>,
    pub(super) key: Key<'id>,
    parent: Option<Key<'id>>,
    redirect: Redirect,
    allow_handoff: bool,
    require_finish: bool,
}
impl<'id, 'storage, T> Answers<'id, 'storage, T> {
    pub fn prepare_admission(
        &mut self,
        id: AnswerId,
        parent: Option<AnswerId>,
    ) -> Result<Admission<'_, 'id, 'storage, T>, Error> {
        if self.wire_index(id).is_some() {
            return Err(Error::Duplicate);
        }
        let generation = self
            .generation
            .checked_add(1)
            .ok_or(Error::GenerationExhausted)?;
        let index = self.vacant().ok_or(Error::Full)?;
        let parent = parent
            .map(|id| {
                let key = self.lookup(id).ok_or(Error::StaleKey)?;
                if self.record(key)?.provision {
                    return Err(Error::NotCallable);
                }
                self.record(key)?
                    .pipelines
                    .checked_add(1)
                    .ok_or(Error::ReferenceLimit)?;
                Ok(key)
            })
            .transpose()?;
        Ok(Admission {
            answers: self,
            key: Key {
                index,
                id,
                generation,
                brand: PhantomData,
            },
            parent,
            redirect: Redirect::None,
            allow_handoff: false,
            require_finish: false,
        })
    }
}
impl<'id, T> Admission<'_, 'id, '_, T> {
    /// Keep this answer until explicit Finish, including capability-free replies.
    pub fn require_finish(mut self) -> Self {
        self.require_finish = true;
        self
    }
    /// Permit a network handoff only when the immutable incoming Call opted in.
    /// The caller asks this vat to retain results for one later adoption.
    pub fn allow_handoff(mut self, allowed: bool) -> Self {
        self.allow_handoff = allowed;
        self
    }
    /// A third-party destination is never available for local tail adoption.
    pub fn third_party(mut self) -> Self {
        self.redirect = Redirect::ThirdParty;
        self
    }
    pub fn redirected(mut self) -> Self {
        self.redirect = Redirect::Available;
        self
    }
    pub fn id(&self) -> AnswerId {
        self.key.id
    }
    /// No fallible operation or user callback occurs after preflight. The new
    /// answer and its source dependency become visible in one transition.
    #[inline(always)]
    pub fn accept(self, value: T) -> (Key<'id>, Option<Pipeline<'id>>) {
        self.accept_with(value, false)
    }
    #[inline(always)]
    pub(super) fn accept_with(
        self,
        value: T,
        provision: bool,
    ) -> (Key<'id>, Option<Pipeline<'id>>) {
        if let Some(key) = self.parent {
            self.answers
                .record_mut(key)
                .expect("preflight parent")
                .pipelines += 1;
        }
        let reserved = self
            .answers
            .free
            .take(&mut self.answers.slots, |slot| &mut slot.free);
        debug_assert_eq!(reserved, self.key.index);
        self.answers.displaced +=
            usize::from(usize::try_from(self.key.id.to_wire()).ok() != Some(self.key.index));
        self.answers.generation = self.key.generation;
        self.answers.slots[self.key.index].record = Some(Record {
            #[cfg(feature = "alloc")]
            presence: self.answers.observation.enter(),
            id: self.key.id,
            generation: self.key.generation,
            state: if provision {
                State::Returned
            } else {
                State::Queued
            },
            finished: false,
            pipelines: 0,
            redirect: self.redirect,
            allow_handoff: self.allow_handoff,
            require_finish: self.require_finish,
            provision,
            value,
        });
        (self.key, self.parent.map(|key| Pipeline { key }))
    }
}
