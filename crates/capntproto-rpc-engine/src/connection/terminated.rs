//! Terminal owner transfer without running destructors inside table transitions.
use super::*;

/// Consumes all live ledger authority. Outstanding handles cannot be used with
/// these tables again; the connection has no IO or publication operations.
/// Quiesce transport and return externally executing tasks before termination.
/// Dropping this value drops any owners the application has not extracted.
pub struct Terminated<'id, 's, I, E, A> {
    connection: Connection<'id, 's, I, E, A>,
}
impl<'id, 's, I, E, A> Connection<'id, 's, I, E, A> {
    pub fn terminate(self) -> Terminated<'id, 's, I, E, A> {
        Terminated { connection: self }
    }
}
impl<I, E, A> Terminated<'_, '_, I, E, A> {
    pub fn answer_capacity(&self) -> usize {
        self.connection.answers.capacity()
    }
    pub fn export_capacity(&self) -> usize {
        self.connection.exports.capacity()
    }
    pub fn import_capacity(&self) -> usize {
        self.connection.imports.capacity()
    }
    /// Terminal state ignores pending wire grants and retained pipelines.
    /// Detach dependent wire tasks before consuming their ledger domain.
    pub fn take_answer(&mut self, index: usize) -> Option<A> {
        self.connection.answers.take_terminal_owner(index)
    }
    pub fn take_export(&mut self, index: usize) -> Option<E> {
        self.connection.exports.take_terminal_owner(index)
    }
    pub fn take_import(&mut self, index: usize) -> Option<I> {
        self.connection.imports.take_terminal_owner(index)
    }
}

#[cfg(any(test, kani))]
mod checks {
    use super::*;
    use crate::ids::{AnswerId, ImportId, WireId};
    fn check(owner: u8) {
        let mut imports = [imports::Slot::EMPTY];
        let mut exports = [exports::Slot::EMPTY];
        let mut answers = [answers::Slot::EMPTY];
        let mut questions = [questions::Slot::EMPTY];
        let mut connection = new_scope(Storage {
            imports: &mut imports,
            exports: &mut exports,
            answers: &mut answers,
            questions: &mut questions,
            pipeline_questions: &mut [],
        })
        .unwrap();
        let _import = connection
            .imports
            .admit(ImportId::from_wire(17), owner)
            .unwrap();
        let mut batch = connection.exports.batch();
        let grant = batch.export(owner).unwrap();
        batch.commit();
        let _lease = connection.exports.lease(grant.id()).unwrap();
        let answer = connection
            .answers
            .admit(AnswerId::from_wire(23), owner)
            .unwrap();
        let _pipeline = connection.answers.pipeline(answer.id()).unwrap();
        let mut closed = connection.terminate();
        let first = closed.take_answer(0);
        #[cfg(kani)]
        kani::assert(
            first == Some(owner),
            "terminal answer transfers its retained producer",
        );
        assert_eq!(first, Some(owner));
        assert_eq!(closed.take_answer(0), None);
        assert_eq!(closed.take_export(0), Some(owner));
        assert_eq!(closed.take_export(0), None);
        assert_eq!(closed.take_import(0), Some(owner));
        assert_eq!(closed.take_import(0), None);
        assert_eq!(closed.take_answer(1), None);
    }
    #[cfg(test)]
    #[test]
    fn terminal_ledgers_transfer_retained_owners_without_allocating() {
        assert_eq!(allocation_counter::measure(|| check(41)).count_total, 0);
    }
    #[cfg(kani)]
    #[kani::proof]
    #[kani::unwind(4)]
    fn terminal_ledgers_detach_each_owner_once() {
        check(kani::any());
    }
}
