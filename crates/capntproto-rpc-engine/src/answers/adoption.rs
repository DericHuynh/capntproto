//! Once-only local result/producer ownership for takeFromOtherQuestion.
use super::*;

#[must_use = "accept with the matching Return or drop to preserve the source"]
pub struct Adoption<'a, 'id, 'storage, T> {
    answers: &'a mut Answers<'id, 'storage, T>,
    key: Key<'id>,
}

/// Retains the exact source generation and its producer after the peer finishes
/// that answer. It is separate from the adopting outgoing question's wire ID.
#[derive(Debug)]
#[must_use = "release the adopted result/producer through its answer ledger"]
pub struct Adopted<'id> {
    pipeline: Pipeline<'id>,
}
impl<'id> Adopted<'id> {
    pub fn key(&self) -> Key<'id> {
        self.pipeline.key
    }
}
impl<'id, 'storage, T> Answers<'id, 'storage, T> {
    pub fn prepare_adoption(
        &mut self,
        id: AnswerId,
    ) -> Result<Adoption<'_, 'id, 'storage, T>, Error> {
        let key = self.lookup(id).ok_or(Error::StaleKey)?;
        let record = self.record(key)?;
        match record.redirect {
            Redirect::None | Redirect::ThirdParty => return Err(Error::NotRedirected),
            Redirect::Adopted => return Err(Error::AlreadyAdopted),
            Redirect::Available => (),
        }
        record
            .pipelines
            .checked_add(1)
            .ok_or(Error::ReferenceLimit)?;
        Ok(Adoption { answers: self, key })
    }
    pub fn adopted(&self, adopted: &Adopted<'id>) -> &T {
        self.pipelined(&adopted.pipeline)
    }
    pub fn release_adopted(&mut self, adopted: Adopted<'id>) -> Option<T> {
        self.release_pipeline(adopted.pipeline)
    }
    pub fn release_adopted_deferred(&mut self, adopted: Adopted<'id>) {
        self.release_pipeline_deferred(adopted.pipeline);
    }
}
impl<'id, T> Adoption<'_, 'id, '_, T> {
    /// The original result can only be adopted once. Acceptance retains its
    /// producer even if Finish arrives before local execution has completed.
    pub fn accept(self) -> Adopted<'id> {
        let record = self
            .answers
            .record_mut(self.key)
            .expect("preflight adoption");
        record.redirect = Redirect::Adopted;
        record.pipelines += 1;
        Adopted {
            pipeline: Pipeline { key: self.key },
        }
    }
    /// A late Return still consumes the once-only adoption, even after the
    /// adopting caller canceled. No extra owner needs to survive in that case.
    pub fn discard(self) {
        let record = self
            .answers
            .record_mut(self.key)
            .expect("preflight adoption");
        record.redirect = Redirect::Adopted;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::WireId;

    #[test]
    fn adoption_keeps_producer_once_across_finish_and_peer_id_reuse() {
        let measured = allocation_counter::measure(|| {
            for returned_first in [false, true] {
                let mut slots = [const { Slot::EMPTY }; 2];
                with_answers(&mut slots, |answers| {
                    let id = AnswerId::from_wire(u32::MAX);
                    let (key, _) = answers
                        .prepare_admission(id, None)
                        .unwrap()
                        .redirected()
                        .accept(41);
                    drop(answers.prepare_adoption(id).unwrap());
                    let adopted = answers.prepare_adoption(id).unwrap().accept();
                    assert!(matches!(
                        answers.prepare_adoption(id),
                        Err(Error::AlreadyAdopted)
                    ));
                    if returned_first {
                        assert_eq!(answers.prepare_return(key).unwrap().accept(), None);
                    }
                    let finish = answers.finish(id, false);
                    assert!(!finish.cancel);
                    assert_eq!(finish.retired, None);
                    if !returned_first {
                        assert!(answers.dispatch(key, true).unwrap());
                        assert_eq!(answers.prepare_return(key).unwrap().accept(), None);
                    }
                    let next = answers.admit(id, 43).unwrap();
                    assert_eq!(answers.adopted(&adopted), &41);
                    assert_ne!(adopted.key(), next);
                    assert_eq!(answers.release_adopted(adopted), Some(41));
                    assert_eq!(answers.get(next), Ok(&43));
                    assert!(matches!(
                        answers.prepare_adoption(id),
                        Err(Error::NotRedirected)
                    ));
                    answers.finish(id, false);
                    assert_eq!(answers.prepare_return(next).unwrap().accept(), Some(43));
                    assert!(answers.is_empty());
                })
                .unwrap();
            }
            let mut slots = [Slot::EMPTY];
            with_answers(&mut slots, |answers| {
                let id = AnswerId::from_wire(11);
                let (key, _) = answers
                    .prepare_admission(id, None)
                    .unwrap()
                    .redirected()
                    .accept(47);
                answers.prepare_adoption(id).unwrap().discard();
                assert!(matches!(
                    answers.prepare_adoption(id),
                    Err(Error::AlreadyAdopted)
                ));
                assert!(answers.finish(id, true).cancel);
                assert_eq!(answers.prepare_return(key).unwrap().accept(), Some(47));
            })
            .unwrap();
        });
        assert_eq!(measured.count_total, 0);
    }
}

#[cfg(kani)]
mod proofs {
    use super::*;
    use crate::ids::WireId;
    #[kani::proof]
    #[kani::unwind(4)]
    fn redirected_answer_adoption_is_once_and_retains_producer() {
        let count: u32 = kani::any();
        let mut slots = [Slot::EMPTY];
        with_answers(&mut slots, |answers| {
            let id = AnswerId::from_wire(kani::any());
            let (key, _) = answers
                .prepare_admission(id, None)
                .unwrap()
                .redirected()
                .accept(41u8);
            answers.record_mut(key).unwrap().pipelines = count;
            let plan = answers.prepare_adoption(id);
            if count == u32::MAX {
                assert!(matches!(plan, Err(Error::ReferenceLimit)));
                return;
            }
            drop(plan.unwrap());
            assert_eq!(answers.record(key).unwrap().pipelines, count);
            let adopted = answers.prepare_adoption(id).unwrap().accept();
            kani::assert(
                matches!(answers.prepare_adoption(id), Err(Error::AlreadyAdopted)),
                "redirected result may only be adopted once",
            );
            kani::assert(
                answers.record(key).unwrap().pipelines == count + 1,
                "adoption retains the local result producer",
            );
            assert!(!answers.finish(id, false).cancel);
            assert_eq!(answers.prepare_return(key).unwrap().accept(), None);
            assert_eq!(answers.adopted(&adopted), &41);
            assert_eq!(
                answers.release_adopted(adopted),
                if count == 0 { Some(41) } else { None }
            );
        })
        .unwrap();
    }
}
