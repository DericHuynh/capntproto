//! A Provide has a Finish obligation but no reply or callable result pipeline.
use super::*;

#[derive(Debug)]
#[must_use = "retain until the recipient releases its vine, then release_provision"]
pub struct Provision<'id> {
    local: Local<'id>,
}
impl<'id> Provision<'id> {
    pub fn key(&self) -> Key<'id> {
        self.local.key
    }
}
#[must_use = "commit only with immutable Provide output acceptance"]
pub struct ProvisionReservation<'a, 'id, 'storage> {
    reservation: Reservation<'a, 'id, 'storage>,
}
impl<'id> ProvisionReservation<'_, 'id, '_> {
    pub fn id(&self) -> QuestionId {
        self.reservation.id()
    }
    pub fn accept(self) -> Provision<'id> {
        Provision {
            local: self.reservation.accept(),
        }
    }
}
impl<'id, 'storage> Questions<'id, 'storage> {
    pub fn reserve_provision(&mut self) -> Result<ProvisionReservation<'_, 'id, 'storage>, Error> {
        let reservation = self.reserve()?;
        let record = reservation.questions.record_mut(reservation.key).unwrap();
        record.lifecycle = Question::no_return();
        record.provision = true;
        Ok(ProvisionReservation { reservation })
    }
    pub fn release_provision(&mut self, provision: Provision<'id>) {
        self.release(provision.local);
    }
    pub fn retain_provision(
        &mut self,
        provision: &Provision<'id>,
    ) -> Result<Provision<'id>, Error> {
        self.retain(&provision.local)
            .map(|local| Provision { local })
    }
}

#[cfg(any(test, kani))]
fn lifecycle(rejected: bool) {
    let mut slots = [Slot::EMPTY];
    with_questions(&mut slots, |questions| {
        let reservation = questions.reserve_provision().unwrap();
        let id = reservation.id();
        if rejected {
            drop(reservation);
            assert!(questions.is_empty(), "unpublished Provide owes no Finish");
            assert!(questions.pending_finish().is_none());
        } else {
            let provision = reservation.accept();
            assert_eq!(
                questions.pipeline_id(&provision.local),
                Err(Error::PipelineReleased),
                "Provide does not grant callable pipelines"
            );
            let returned = questions.returned(id, false);
            #[cfg(kani)]
            kani::assert(
                returned == Err(Error::DuplicateReturn),
                "Provide rejects a Return",
            );
            #[cfg(not(kani))]
            assert_eq!(
                returned,
                Err(Error::DuplicateReturn),
                "Provide rejects a Return"
            );
            let alias = questions.retain_provision(&provision).unwrap();
            questions.release_provision(provision);
            assert!(
                questions.pending_finish().is_none(),
                "a retained vine keeps Provide registered"
            );
            questions.release_provision(alias);
            let pending = questions.pending_finish().unwrap();
            assert!(!pending.release_result_caps());
            drop(pending);
            assert!(
                questions.reserve().is_err(),
                "Provide ID survives rejected Finish output"
            );
            questions.pending_finish().unwrap().accept();
            assert!(
                questions.is_empty(),
                "Provide reclaims after Finish without a Return"
            );
            assert!(questions.pending_finish().is_none());
            assert_eq!(questions.reserve_provision().unwrap().id(), id);
        }
    })
    .unwrap();
}
#[cfg(test)]
mod tests {
    #[test]
    fn provide_has_no_return_and_retains_id_until_finish_acceptance() {
        super::lifecycle(false);
        super::lifecycle(true);
    }
}
#[cfg(kani)]
mod proofs {
    #[kani::proof]
    #[kani::unwind(4)]
    fn provision_question_needs_only_an_accepted_finish() {
        super::lifecycle(kani::any());
    }
}
