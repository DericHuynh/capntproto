//! Hosted wakeup adapter over the shared no-std/no-alloc notification FIFO.
pub(crate) use capntproto_rpc_engine::ready_queue::CAPACITY;
use futures::task::AtomicWaker;

pub(crate) struct ReadyQueue {
    queue: capntproto_rpc_engine::ready_queue::ReadyQueue,
    pub(crate) parent: AtomicWaker,
}
impl ReadyQueue {
    pub(crate) fn new() -> Self {
        Self {
            queue: capntproto_rpc_engine::ready_queue::ReadyQueue::new(),
            parent: AtomicWaker::new(),
        }
    }
    #[inline]
    pub(crate) fn enqueue(&self, index: usize) {
        if self.queue.enqueue(index) {
            self.parent.wake();
        }
    }
    #[inline]
    pub(crate) fn front(&self, position: usize) -> Option<usize> {
        self.queue.front(position)
    }
    #[inline]
    pub(crate) fn pending(&self, position: usize) -> bool {
        self.queue.pending(position)
    }
    #[inline]
    pub(crate) fn consume(&self, position: usize, index: usize) {
        self.queue.consume(position, index);
    }
}
