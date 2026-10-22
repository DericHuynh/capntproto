use super::*;

/// An owned dispatcher with no remaining call/publication authority. Borrowed
/// graph dispatchers cannot be consumed this way: their graph is still live in
/// the enclosing scope. Drain tasks before detaching graph endpoint owners.
pub struct Terminated<'id, 'nodes, 'tasks, T: 'nodes, P: Copy + Eq + 'nodes, C> {
    runtime: Runtime<'id, 'nodes, 'tasks, T, P, C>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminateError {
    PendingTasks,
}
impl core::fmt::Display for TerminateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("terminal dispatcher still owns queued tasks")
    }
}
impl core::error::Error for TerminateError {}
impl<'id, 'nodes, 'tasks, T: 'nodes, P: Copy + Eq + 'nodes, C>
    Runtime<'id, 'nodes, 'tasks, T, P, C>
{
    pub fn terminate(self) -> Terminated<'id, 'nodes, 'tasks, T, P, C> {
        Terminated { runtime: self }
    }
}
impl<T, P: Copy + Eq, C> Terminated<'_, '_, '_, T, P, C> {
    pub fn take_task(&mut self) -> Option<C> {
        self.runtime.drain_one()
    }
    pub fn endpoint_capacity(&self) -> usize {
        self.runtime.graph.slots.len()
    }
    pub fn take_endpoint(&mut self, index: usize) -> Result<Option<T>, TerminateError> {
        if self.runtime.queued() != 0 {
            return Err(TerminateError::PendingTasks);
        }
        // No task can subsequently traverse graph edges. Dropping this terminal
        // graph discards remaining entries without following their references.
        let entry = self
            .runtime
            .graph
            .slots
            .get_mut(index)
            .and_then(|slot| slot.entry.take());
        Ok(entry.and_then(|entry| match entry.kind {
            Kind::Endpoint { owner, .. } => Some(owner),
            Kind::Promise { .. } => None,
        }))
    }
}

#[cfg(any(test, kani))]
mod checks {
    use super::*;
    fn check(payload: u8) {
        let mut nodes = [const { Slot::<u8, u8>::EMPTY }; 2];
        let graph =
            crate::clients::new_scope(&mut nodes, crate::clients::NotificationStorage::empty())
                .unwrap();
        let mut tasks = [TaskSlot::EMPTY];
        let mut runtime = own_runtime(graph, &mut tasks).unwrap_or_else(|_| panic!("fresh tasks"));
        let _server = runtime.endpoint(17, Location::Local).unwrap();
        let pending = runtime.promise(&mut None).unwrap();
        runtime
            .prepare_call(&pending, &mut Some(payload))
            .unwrap()
            .accept();
        let mut closed = runtime.terminate();
        let before = closed.take_endpoint(0);
        #[cfg(kani)]
        kani::assert(
            before == Err(TerminateError::PendingTasks),
            "terminal endpoints retain ownership until queued tasks drain",
        );
        assert_eq!(before, Err(TerminateError::PendingTasks));
        assert_eq!(closed.take_task(), Some(payload));
        assert_eq!(closed.take_task(), None);
        assert_eq!(closed.take_endpoint(0), Ok(Some(17)));
        assert_eq!(closed.take_endpoint(0), Ok(None));
        assert_eq!(closed.take_endpoint(1), Ok(None));
        assert_eq!(closed.take_endpoint(2), Ok(None));
    }
    #[cfg(test)]
    #[test]
    fn terminal_dispatch_preserves_pending_owners_without_allocating() {
        assert_eq!(allocation_counter::measure(|| check(43)).count_total, 0);
    }
    #[cfg(kani)]
    #[kani::proof]
    #[kani::unwind(5)]
    #[kani::solver(kissat)]
    fn terminal_dispatch_detaches_each_owner_once() {
        check(kani::any());
    }
}
