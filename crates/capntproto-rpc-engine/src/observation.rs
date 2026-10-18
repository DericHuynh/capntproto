//! Optional read-only observations of actual record lifetimes.
//! Allocating an observer is explicit; unobserved tables allocate nothing here.
use alloc::rc::Rc;
use core::cell::Cell;

/// Live occupancy, including retained records awaiting cleanup. This handle
/// cannot access the table or keep any of its owners alive.
#[derive(Clone)]
pub struct Observation(Rc<Cell<usize>>);
impl Observation {
    pub fn get(&self) -> usize {
        self.0.get()
    }
}

#[derive(Default)]
pub(crate) struct Counter(Option<Rc<Cell<usize>>>);
#[derive(Default)]
pub(crate) struct Presence(Option<Rc<Cell<usize>>>);
impl Counter {
    /// Unknown occupancy is conservative: creating an observer remains explicit.
    pub(crate) fn known_empty(&self) -> bool {
        self.0.as_ref().is_some_and(|count| count.get() == 0)
    }

    pub(crate) fn enter(&self) -> Presence {
        if let Some(count) = &self.0 {
            count.set(count.get().checked_add(1).expect("live record count"));
        }
        Presence(self.0.clone())
    }
    pub(crate) fn observe<'a>(
        &mut self,
        records: impl Iterator<Item = &'a mut Presence>,
    ) -> Observation {
        if self.0.is_none() {
            self.0 = Some(Rc::new(Cell::new(0)));
            for presence in records {
                *presence = self.enter();
            }
        }
        Observation(self.0.as_ref().unwrap().clone())
    }
}
impl Drop for Presence {
    fn drop(&mut self) {
        if let Some(count) = &self.0 {
            count.set(count.get() - 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        answers, exports,
        ids::{AnswerId, ImportId, WireId},
        imports,
    };

    #[test]
    fn observed_empty_tables_skip_retained_capacity_but_not_pending_cleanup() {
        let mut storage = [const { answers::Slot::EMPTY }; 32];
        answers::with_answers(&mut storage, |answers| {
            assert!(!answers.reclamation_is_idle(0));
            let count = answers.observe_len();
            assert!(answers.reclamation_is_idle(0));
            let id = AnswerId::from_wire(7);
            let key = answers.admit(id, 41).unwrap();
            let pipeline = answers.pipeline(id).unwrap();
            assert!(!answers.reclamation_is_idle(0));
            answers.finish(id, false);
            assert!(answers.prepare_return(key).unwrap().accept().is_none());
            answers.release_pipeline_deferred(pipeline);
            assert_eq!(count.get(), 1);
            assert!(!answers.reclamation_is_idle(0));
            assert_eq!(answers.take_finished(0), Some(41));
            assert!(answers.reclamation_is_idle(0));
        })
        .unwrap();

        let mut storage = [const { exports::Slot::EMPTY }; 32];
        exports::with_exports(&mut storage, |exports| {
            assert!(!exports.retirement_is_idle(0));
            let count = exports.observe_len();
            assert!(exports.retirement_is_idle(0));
            // Even an empty forgotten batch must be recovered by maintenance.
            core::mem::forget(exports.batch());
            assert_eq!(count.get(), 0);
            assert!(!exports.retirement_is_idle(0));
            assert_eq!(exports.take_retired(0), None::<u32>);
            assert!(exports.retirement_is_idle(0));
            let mut batch = exports.batch();
            let _ = batch.export(43).unwrap();
            drop(batch);
            assert!(!exports.retirement_is_idle(0));
            assert_eq!(exports.take_retired(0), Some(43));
            assert!(exports.retirement_is_idle(0));
        })
        .unwrap();
    }

    #[test]
    fn observations_follow_retained_records_rollback_and_terminal_owner_transfer() {
        let mut storage = [imports::Slot::EMPTY];
        let observed = imports::with_imports(&mut storage, |imports| {
            let local = imports.admit(ImportId::from_wire(1), 42).unwrap();
            let count = imports.observe_len();
            let again = imports.observe_len();
            assert_eq!(count.get(), 1);
            imports.release(local);
            assert_eq!(count.get(), 1); // Pending Release still owns the entry.
            assert_eq!(imports.pending_release(0).unwrap().commit(), 42);
            assert_eq!(again.get(), 0);
            let _ = imports.admit(ImportId::from_wire(2), 43).unwrap();
            assert_eq!(count.get(), 1);
            count
        })
        .unwrap();
        assert_eq!(observed.get(), 0);

        let mut storage = [exports::Slot::EMPTY];
        exports::with_exports(&mut storage, |exports| {
            let count = exports.observe_len();
            let mut batch = exports.batch();
            let _ = batch.export(41).unwrap();
            assert_eq!(count.get(), 1);
            drop(batch); // Retains owner for an explicit drain.
            assert_eq!(count.get(), 1);
            assert_eq!(exports.take_retired(0), Some(41));
            assert_eq!(count.get(), 0);
            let mut batch = exports.batch();
            let grant = batch.export(42).unwrap();
            batch.commit();
            let lease = exports.lease(grant.id()).unwrap();
            assert_eq!(exports.release(grant.id(), 1), Ok(None));
            assert_eq!(count.get(), 1);
            assert_eq!(exports.release_local(lease), Some(42));
            assert_eq!(count.get(), 0);
        })
        .unwrap();

        let mut storage = [answers::Slot::EMPTY, answers::Slot::EMPTY];
        let observed = answers::with_answers(&mut storage, |answers| {
            let id = AnswerId::from_wire(4);
            let old = answers.admit(id, 17).unwrap();
            let count = answers.observe_len();
            let pipeline = answers.pipeline(id).unwrap();
            answers.finish(id, false);
            assert!(answers.prepare_return(old).unwrap().accept().is_none());
            let _ = answers.admit(id, 19).unwrap();
            assert_eq!(count.get(), 2); // Same wire ID; distinct retained generations.
            assert_eq!(answers.release_pipeline(pipeline), Some(17));
            assert_eq!(count.get(), 1);
            count
        })
        .unwrap();
        assert_eq!(observed.get(), 0);
    }

    #[test]
    fn detached_owner_destructors_observe_updated_occupancy() {
        struct Owner(Observation);
        impl Drop for Owner {
            fn drop(&mut self) {
                assert_eq!(self.0.get(), 0);
            }
        }
        let mut storage = [imports::Slot::EMPTY];
        imports::with_imports(&mut storage, |imports| {
            let count = imports.observe_len();
            let local = imports
                .admit(ImportId::from_wire(1), Owner(count))
                .ok()
                .unwrap();
            imports.release(local);
            drop(imports.pending_release(0).unwrap().commit());
            let count = imports.observe_len();
            let _ = imports
                .admit(ImportId::from_wire(1), Owner(count))
                .ok()
                .unwrap();
            // Terminal drop must also decrement before invoking the owner.
        })
        .unwrap();
    }

    #[test]
    fn embargo_observation_includes_canceled_tombstones_and_rejected_offers() {
        use crate::{
            embargoes::Embargoes,
            peer_table::{BorrowedSparse, Slot},
        };
        let mut slots = [Slot::EMPTY];
        let mut table = Embargoes::new(BorrowedSparse::new(&mut slots))
            .ok()
            .unwrap();
        let count = table.observe_len();
        let id = table.reserve(17).unwrap().accept();
        assert_eq!(count.get(), 1);
        assert_eq!(table.cancel(id), Ok(Some(17)));
        assert_eq!(count.get(), 1);
        assert_eq!(table.receive(id), Ok(None));
        assert_eq!(count.get(), 0);
        drop(table.reserve(19).unwrap());
        assert_eq!(count.get(), 1);
        assert_eq!(table.take_rejected(), Some(19));
        assert_eq!(count.get(), 0);
        table.reserve(23).unwrap().accept();
        assert_eq!(table.drain_one(), Some(Some(23)));
        assert_eq!(count.get(), 0);
    }
}
