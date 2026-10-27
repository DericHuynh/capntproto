//! Callee-selected answers become visible only with ThirdPartyAnswer publication.
use super::*;
use crate::ids::WireId;

#[must_use = "publish the adoption announcement before accepting the answer"]
pub struct ThirdPartyAdmission<'a, 'id, 's, T> {
    admission: Admission<'a, 'id, 's, T>,
}
impl<'id, T> ThirdPartyAdmission<'_, 'id, '_, T> {
    pub fn id(&self) -> AnswerId {
        self.admission.id()
    }
    pub fn accept(self, owner: T) -> Key<'id> {
        self.admission.accept(owner).0
    }
}
impl<'id, 's, T> Answers<'id, 's, T> {
    /// Choose a free callee ID in [2^30, 2^31). No separate ID heap or allocator
    /// is needed: with a free answer slot, at most capacity - 1 IDs are live.
    /// Detached old generations with retained pipeline leases do not reserve
    /// their former wire IDs, but continue occupying their own storage slots.
    pub fn prepare_third_party(&mut self) -> Result<ThirdPartyAdmission<'_, 'id, 's, T>, Error> {
        self.vacant().ok_or(Error::Full)?;
        let limit = u32::try_from(self.slots.len())
            .unwrap_or(1 << 30)
            .min(1 << 30);
        let id = (0..limit)
            .map(|offset| AnswerId::from_wire((1 << 30) + offset))
            .find(|&id| self.wire_index(id).is_none())
            .ok_or(Error::Full)?;
        Ok(ThirdPartyAdmission {
            admission: self.prepare_admission(id, None)?,
        })
    }
}

#[cfg(any(test, kani))]
fn check(finish_first: bool) {
    let mut slots = [Slot::EMPTY, Slot::EMPTY];
    with_answers(&mut slots, |answers| {
        let abandoned = answers.prepare_third_party().unwrap();
        let id = abandoned.id();
        assert_eq!(id.to_wire(), 1 << 30);
        drop(abandoned);
        assert!(answers.is_empty());
        let first = answers.prepare_third_party().unwrap().accept(41u8);
        let second = answers.prepare_third_party();
        #[cfg(kani)]
        kani::assert(
            second.is_ok(),
            "new callee answer skips every still-live ID",
        );
        let second = second.unwrap().accept(43);
        #[cfg(kani)]
        kani::assert(
            first.id() != second.id(),
            "live callee-selected answers have distinct IDs",
        );
        assert_ne!(first.id(), second.id());
        assert!(matches!(answers.prepare_third_party(), Err(Error::Full)));
        let pipeline = answers.pipeline(id).unwrap();
        if finish_first {
            assert!(!answers.finish(id, false).cancel);
            assert!(answers.prepare_return(first).unwrap().is_cancelled());
            assert_eq!(answers.prepare_return(first).unwrap().accept(), None);
        } else {
            assert_eq!(answers.prepare_return(first).unwrap().accept(), None);
            assert_eq!(answers.finish(id, false).retired, None);
        }
        assert!(answers.lookup(id).is_none());
        answers.prepare_return(second).unwrap().accept();
        assert_eq!(answers.finish(second.id(), false).retired, Some(43));
        let reused = answers.prepare_third_party().unwrap().accept(47);
        assert_eq!(reused.id(), id);
        assert_ne!(first, reused);
        #[cfg(kani)]
        kani::assert(
            *answers.pipelined(&pipeline) == 41,
            "adopted answer pipeline stays on its original producer",
        );
        assert_eq!(*answers.pipelined(&pipeline), 41);
        assert_eq!(answers.release_pipeline(pipeline), Some(41));
        assert_eq!(*answers.get(reused).unwrap(), 47);
        answers.prepare_return(reused).unwrap().accept();
        assert_eq!(answers.finish(reused.id(), false).retired, Some(47));
        assert!(answers.is_empty());
    })
    .unwrap();
}
#[cfg(test)]
mod tests {
    #[cfg(feature = "alloc")]
    #[test]
    fn growing_third_party_answers_reserve_capacity_before_selecting_an_id() {
        use super::*;
        with_growing_answers(0, 2, |answers| {
            let abandoned = answers.prepare_third_party().unwrap();
            assert_eq!(abandoned.id().to_wire(), 1 << 30);
            drop(abandoned);
            assert!(answers.is_empty());
            let first = answers.prepare_third_party().unwrap().accept(41u8);
            let second = answers.prepare_third_party().unwrap().accept(43);
            assert_ne!(first.id(), second.id());
            assert!(matches!(answers.prepare_third_party(), Err(Error::Full)));
            let pipeline = answers.pipeline(first.id()).unwrap();
            answers.prepare_return(first).unwrap().accept();
            assert!(answers.finish(first.id(), false).retired.is_none());
            answers.prepare_return(second).unwrap().accept();
            assert_eq!(answers.finish(second.id(), false).retired, Some(43));
            let reused = answers.prepare_third_party().unwrap().accept(47);
            assert_eq!(reused.id(), first.id());
            assert_eq!(answers.pipelined(&pipeline), &41);
            assert_eq!(answers.release_pipeline(pipeline), Some(41));
            assert_eq!(answers.get(first), Err(Error::StaleKey));
            answers.prepare_return(reused).unwrap().accept();
            assert_eq!(answers.finish(reused.id(), false).retired, Some(47));
            assert!(answers.is_empty());
        })
        .unwrap();
    }
    #[test]
    fn callee_selected_answer_publication_and_pipeline_reuse() {
        let measured = allocation_counter::measure(|| {
            super::check(false);
            super::check(true);
        });
        assert_eq!(measured.count_total, 0);
    }
}
#[cfg(kani)]
mod proofs {
    #[kani::proof]
    #[kani::unwind(5)]
    fn callee_selected_answers_preserve_publication_and_retained_pipeline() {
        super::check(kani::any());
    }
}
