//! A Finish transition can be bound to implicit result-capability releases.
use super::*;

#[must_use = "accept after validating the original Return and all result releases"]
pub struct Finish<'a, 'id, 'storage, T> {
    answers: &'a mut Answers<'id, 'storage, T>,
    key: Key<'id>,
    release_results: bool,
}
impl<'id, 'storage, T> Answers<'id, 'storage, T> {
    /// Unknown and duplicate Finish messages have no effects. A live ticket
    /// excludes both return publication and pipeline/owner changes until commit.
    pub fn prepare_finish(
        &mut self,
        id: AnswerId,
        release_results: bool,
    ) -> Option<Finish<'_, 'id, 'storage, T>> {
        let key = self.lookup(id)?;
        Some(Finish {
            answers: self,
            key,
            release_results,
        })
    }
}
impl<T> Finish<'_, '_, '_, T> {
    pub fn id(&self) -> AnswerId {
        self.key.id
    }
    pub fn returned(&self) -> bool {
        let record = self.answers.record(self.key).unwrap();
        record.state == State::Returned && !record.provision
    }
    pub fn release_results(&self) -> bool {
        self.release_results && !self.answers.record(self.key).unwrap().provision
    }
    /// No owner destructor runs in this transition. Dispose of retired work
    /// only after the combined transaction has finished borrowing its ledgers.
    pub fn accept(self) -> Finished<T> {
        self.answers.finish(self.key.id, self.release_results)
    }
}
