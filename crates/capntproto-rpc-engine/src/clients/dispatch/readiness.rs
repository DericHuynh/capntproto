//! Dispatch waits on the exact unresolved graph node. Resolving a promise or
//! opening its embargo queues that node; unrelated tasks are never inspected.
use super::*;
pub(super) fn append<C>(tasks: &mut [TaskSlot<C>], list: &mut List, index: usize) {
    let entry = tasks[index].entry.as_mut().unwrap();
    entry.previous = list.tail;
    entry.next = None;
    if let Some(tail) = list.tail {
        tasks[tail].entry.as_mut().unwrap().next = Some(index);
    } else {
        list.head = Some(index);
    }
    list.tail = Some(index);
}
pub(super) fn unlink<C>(tasks: &mut [TaskSlot<C>], list: &mut List, index: usize) {
    let entry = tasks[index].entry.as_mut().unwrap();
    let previous = entry.previous.take();
    let next = entry.next.take();
    if let Some(previous) = previous {
        tasks[previous].entry.as_mut().unwrap().next = next;
    } else {
        list.head = next;
    }
    if let Some(next) = next {
        tasks[next].entry.as_mut().unwrap().previous = previous;
    } else {
        list.tail = previous;
    }
}
impl<'id, 's, T: 's, P: Copy + Eq + 's, C, G: GraphStorage<'id, 's, T, P>>
    Runtime<'id, 's, '_, T, P, C, G>
{
    pub(super) fn unlink_task(&mut self, index: usize) {
        match self.tasks[index].entry.as_ref().unwrap().waiting {
            None => unlink(&mut self.tasks, &mut self.ready, index),
            Some(raw) => {
                let graph = self.graph.graph_mut();
                unlink(&mut self.tasks, &mut graph.slots[raw.index].waiting, index);
                if graph.slots[raw.index].waiting.head.is_none() {
                    graph.unlink_dispatch_change(raw.index);
                }
            }
        }
    }
    fn insert_ready(&mut self, index: usize) {
        let sequence = self.tasks[index].entry.as_ref().unwrap().sequence;
        let mut previous = self.ready.tail;
        while let Some(before) = previous {
            let entry = self.tasks[before].entry.as_ref().unwrap();
            if entry.sequence < sequence {
                break;
            }
            previous = entry.previous;
        }
        let next = previous.map_or(self.ready.head, |before| {
            self.tasks[before].entry.as_ref().unwrap().next
        });
        let entry = self.tasks[index].entry.as_mut().unwrap();
        entry.previous = previous;
        entry.next = next;
        if let Some(previous) = previous {
            self.tasks[previous].entry.as_mut().unwrap().next = Some(index);
        } else {
            self.ready.head = Some(index);
        }
        if let Some(next) = next {
            self.tasks[next].entry.as_mut().unwrap().previous = Some(index);
        } else {
            self.ready.tail = Some(index);
        }
    }
    /// Whether a producer queued route changes for parked calls.
    pub fn dispatch_is_pending(&self) -> bool {
        self.graph.graph().dispatch_changes.head.is_some()
    }
    /// Re-route one affected call. The driver charges this exact work against
    /// its cooperative budget; retained inactive task capacity has no cost.
    pub fn advance_dispatch_one(&mut self) -> Result<bool, DispatchError> {
        let Some(node) = self.graph.graph().dispatch_changes.head else {
            return Ok(false);
        };
        let index = self.graph.graph().slots[node]
            .waiting
            .head
            .expect("changed promise has waiters");
        #[cfg(test)]
        {
            self.routing_visits += 1;
        }
        let target = self.tasks[index].entry.as_ref().unwrap().target;
        let (end, waiting, _) = self
            .graph
            .graph()
            .walk(target)
            .map_err(DispatchError::Graph)?;
        self.unlink_task(index);
        self.tasks[index].entry.as_mut().unwrap().waiting = waiting.then_some(end);
        if waiting {
            append(
                &mut self.tasks,
                &mut self.graph.graph_mut().slots[end.index].waiting,
                index,
            );
        } else {
            self.insert_ready(index);
        }
        Ok(true)
    }
}
