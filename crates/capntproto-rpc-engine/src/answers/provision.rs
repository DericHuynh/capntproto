//! Provide holds an ordinary peer question ID until Finish without sending a
//! Return. It cannot be called, adopted or used as a result capability pipeline.
use super::*;

#[must_use = "commit the prepared registration or drop to preserve answer storage"]
pub struct ProvisionAdmission<'a, 'id, 'storage, T>(Admission<'a, 'id, 'storage, T>);
#[must_use = "commit the prepared registration and target receipts together"]
pub struct ProvisionPipelines<'a, 'receipts, 'id, 'storage, T>(
    PipelineAdmission<'a, 'receipts, 'id, 'storage, T>,
);
impl<'id, 'storage, T> Answers<'id, 'storage, T> {
    pub fn is_provision(&self, key: Key<'id>) -> Result<bool, Error> {
        Ok(self.record(key)?.provision)
    }
    pub fn prepare_provision_with_pipelines<'a, 'receipts>(
        &'a mut self,
        id: AnswerId,
        receipts: &'receipts mut [PipelineSlot<'id>],
    ) -> Result<ProvisionPipelines<'a, 'receipts, 'id, 'storage, T>, PipelineError> {
        self.prepare_admission_with_pipelines(id, receipts)
            .map(ProvisionPipelines)
    }
    pub fn prepare_provision(
        &mut self,
        id: AnswerId,
    ) -> Result<ProvisionAdmission<'_, 'id, 'storage, T>, Error> {
        self.prepare_admission(id, None).map(ProvisionAdmission)
    }
}
impl<'receipts, 'id, T> ProvisionPipelines<'_, 'receipts, 'id, '_, T> {
    pub fn accept(self, owner: T) -> (Key<'id>, &'receipts mut [PipelineSlot<'id>]) {
        self.0.accept_with(owner, true)
    }
}
impl<'id, T> ProvisionAdmission<'_, 'id, '_, T> {
    /// Call only after the authenticated network registration has been prepared.
    /// No Return or callback occurs in the commit. Finish hands the registration
    /// owner back to the caller for unregistering outside the table borrow.
    pub fn accept(self, owner: T) -> Key<'id> {
        self.0.accept_with(owner, true).0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::WireId;
    #[test]
    fn provide_is_not_callable_and_finish_retires_without_return() {
        let mut slots = [const { Slot::EMPTY }; 2];
        with_answers(&mut slots, |answers| {
            let id = AnswerId::from_wire(31);
            drop(answers.prepare_provision(id).unwrap());
            assert!(answers.is_empty());
            let key = answers.prepare_provision(id).unwrap().accept(41);
            assert_eq!(answers.prepare_provision(id).err(), Some(Error::Duplicate));
            assert_eq!(answers.pipeline(id).err(), Some(Error::NotCallable));
            assert_eq!(
                answers
                    .prepare_admission(AnswerId::from_wire(33), Some(id))
                    .err(),
                Some(Error::NotCallable)
            );
            let mut receipts = [PipelineSlot::new(id)];
            assert!(matches!(
                answers.prepare_pipelines(&mut receipts),
                Err(PipelineError::Answer(Error::NotCallable))
            ));
            assert_eq!(
                answers.prepare_return(key).err(),
                Some(Error::AlreadyReturned)
            );
            assert_eq!(
                answers.prepare_adoption(id).err(),
                Some(Error::NotRedirected)
            );
            let finish = answers.prepare_finish(id, true).unwrap();
            assert!(!finish.returned());
            assert!(!finish.release_results());
            let finished = finish.accept();
            assert_eq!(finished.retired, Some(41));
            assert!(!finished.cancel && !finished.release_result_caps);
            assert!(answers.is_empty());
            assert!(answers.finish(id, true).retired.is_none());
            let next = answers.admit(id, 43).unwrap();
            assert_ne!(key, next);
            let pipeline = answers.pipeline(id).unwrap();
            answers.finish(id, false);
            assert_eq!(answers.prepare_return(next).unwrap().accept(), None);
            assert_eq!(answers.release_pipeline(pipeline), Some(43));
        })
        .unwrap();
    }
}

#[cfg(kani)]
mod proofs {
    use super::*;
    use crate::ids::WireId;
    #[kani::proof]
    #[kani::unwind(4)]
    fn provision_finish_retires_without_results_or_pipeline_ownership() {
        let mut slots = [const { Slot::EMPTY }; 2];
        with_answers(&mut slots, |answers| {
            let id = AnswerId::from_wire(kani::any());
            let key = answers.prepare_provision(id).unwrap().accept(41u8);
            assert!(
                matches!(answers.pipeline(id), Err(Error::NotCallable)),
                "Provide never grants a result pipeline"
            );
            let mut receipts = [PipelineSlot::new(id)];
            assert!(
                matches!(
                    answers.prepare_pipelines(&mut receipts),
                    Err(PipelineError::Answer(Error::NotCallable))
                ),
                "Provide never grants a descriptor pipeline"
            );
            assert!(
                matches!(answers.prepare_return(key), Err(Error::AlreadyReturned)),
                "Provide never permits Return publication"
            );
            let finish = answers.prepare_finish(id, kani::any()).unwrap();
            kani::assert(
                !finish.returned(),
                "Provide Finish needs no original Return",
            );
            assert!(!finish.release_results(), "Provide owns no result grants");
            assert_eq!(
                finish.accept().retired,
                Some(41),
                "Provide Finish returns registration exactly once"
            );
            assert!(
                answers.finish(id, true).retired.is_none(),
                "duplicate Provide Finish has no owner"
            );
            let next = answers.admit(id, 43).unwrap();
            assert_ne!(
                next, key,
                "finished provision cannot alias reused question ID"
            );
        })
        .unwrap();
    }
}
