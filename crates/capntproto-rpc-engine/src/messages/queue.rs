//! FIFO framing over the message pool. A later frame cannot overtake a partial
//! write. Queue admission is reserved before committing any RPC side effects.
#![forbid(unsafe_code)]
use super::*;

/// Caller-owned queue storage. Entries can only be created by consuming an
/// offered message together with its resources; copied send IDs cannot enqueue.
pub struct QueueEntry<'pool, R = ()> {
    message: QueuedMessage<'pool>,
    resources: R,
}
enum QueuedMessage<'pool> {
    Tracked(SendKey<'pool>),
    Owned {
        lease: Lease<'pool>,
        position: usize,
    },
}

pub struct Queue<'pool, 'storage, R = ()> {
    slots: crate::storage::Storage<'storage, Option<QueueEntry<'pool, R>>>,
    head: usize,
    len: usize,
    failed: bool,
}
impl<'pool, 'storage> Queue<'pool, 'storage> {
    pub fn new(slots: &'storage mut [Option<QueueEntry<'pool>>]) -> Result<Self, Error> {
        Self::with_resources(slots)
    }
    /// Optional bounded allocation; fixed queues never allocate.
    #[cfg(feature = "alloc")]
    pub fn new_growing(initial: usize, maximum: usize) -> Result<Self, Error> {
        Self::growing_with_resources(initial, maximum)
    }
}
impl<'pool, 'storage, R> Queue<'pool, 'storage, R> {
    /// Attachments and bytes occupy the same FIFO entry. Resource types need
    /// neither Clone nor an allocator. Dropping the queue drops its resources;
    /// accepted byte leases then require terminal message-pool teardown.
    pub fn with_resources(
        slots: &'storage mut [Option<QueueEntry<'pool, R>>],
    ) -> Result<Self, Error> {
        if slots.iter().any(Option::is_some) {
            return Err(Error::NonemptyStorage);
        }
        Ok(Self {
            slots: crate::storage::Storage::borrowed(slots),
            head: 0,
            len: 0,
            failed: false,
        })
    }
    #[cfg(feature = "alloc")]
    pub fn growing_with_resources(initial: usize, maximum: usize) -> Result<Self, Error> {
        let slots =
            crate::storage::Storage::growing(initial, maximum, || None).ok_or(Error::NoSendSlot)?;
        Ok(Self {
            slots,
            head: 0,
            len: 0,
            failed: false,
        })
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn is_failed(&self) -> bool {
        self.failed
    }

    /// Reserve a message with no optional resources. The default resource value
    /// must represent an empty bundle (e.g. None or an empty attachment list).
    pub fn reserve(&mut self) -> Result<QueueSlot<'_, 'pool, 'storage, R>, Error>
    where
        R: Default,
    {
        self.reserve_with(R::default())
    }
    /// Resources move into the reservation before any protocol state commits.
    /// Rejection or dropping an unused reservation releases them exactly once.
    #[inline(always)]
    pub fn reserve_with(
        &mut self,
        resources: R,
    ) -> Result<QueueSlot<'_, 'pool, 'storage, R>, Error> {
        self.ensure_capacity()?;
        Ok(QueueSlot {
            queue: self,
            resources,
        })
    }
    /// Check room for one publication, growing bounded storage if necessary.
    /// Does not construct or reserve resources. A subsequent reservation still
    /// checks capacity, so intervening publications cannot overfill the FIFO.
    #[inline]
    pub fn ensure_capacity(&mut self) -> Result<(), Error> {
        if self.failed {
            return Err(Error::QueueFailed);
        }
        if self.len == self.slots.len() {
            return self.grow_one();
        }
        Ok(())
    }
    // Keep allocation and ring relocation out of the publication fast path.
    // Fixed queues still report exhaustion without allocating or moving owners.
    #[cold]
    fn grow_one(&mut self) -> Result<(), Error> {
        let previous = self.slots.len();
        if !self.slots.grow(1, || None) {
            return Err(Error::NoSendSlot);
        }
        self.slots[..previous].rotate_left(self.head);
        self.head = 0;
        Ok(())
    }
    /// Borrow only the oldest frame and its first-write resources. Pending/drop
    /// preserves both. A positive advance prevents re-sending its attachments.
    pub fn window<'a, 'buffers, 'bytes>(
        &'a mut self,
        messages: &'a mut Messages<'pool, 'buffers, 'bytes>,
    ) -> Result<Option<QueueWindow<'a, 'pool, 'storage, 'buffers, 'bytes, R>>, Error> {
        if self.failed {
            return Err(Error::QueueFailed);
        }
        if self.is_empty() {
            return Ok(None);
        }
        let (tracked, buffer, bytes, position) =
            match &self.slots[self.head].as_ref().unwrap().message {
                QueuedMessage::Tracked(key) => {
                    let SendWindow {
                        send,
                        buffer,
                        bytes,
                        ..
                    } = messages.send_window(*key)?;
                    let position = send.expect("live window").position;
                    (Some(send), buffer, bytes, position)
                }
                QueuedMessage::Owned { lease, position } => {
                    let buffer = messages.buffers.at(lease.key.index);
                    debug_assert_eq!(buffer.generation.get(), lease.key.generation);
                    let State::Sealed { length, .. } = buffer.state.get() else {
                        unreachable!("owned FIFO lease")
                    };
                    let bytes = Ref::map(buffer.bytes.borrow(), |bytes| &bytes[*position..length]);
                    (None, buffer, bytes, *position)
                }
            };
        Ok(Some(QueueWindow {
            queue: self,
            tracked,
            buffer,
            bytes,
            position,
        }))
    }
    /// Stop IO before terminal cleanup; this queue can never publish again.
    pub fn fail(&mut self) {
        self.failed = true;
    }
    pub fn discard_one(&mut self, messages: &mut Messages<'pool, '_, '_>) -> Result<bool, Error> {
        if !self.failed {
            return Err(Error::QueueStillLive);
        }
        if self.is_empty() {
            return Ok(false);
        }
        if let QueuedMessage::Tracked(key) = &self.slots[self.head]
            .as_ref()
            .expect("occupied queue head")
            .message
        {
            match messages.fail_send(*key) {
                // A public send ID may already have completed. Its generation
                // cannot authorize touching a reused slot during cleanup.
                Ok(()) | Err(Error::StaleSend) => (),
                Err(error) => return Err(error),
            }
        }
        let entry = self.slots[self.head].take().expect("occupied queue head");
        self.pop();
        if let QueuedMessage::Owned { lease, .. } = entry.message {
            messages.release(lease);
        }
        drop(entry.resources);
        Ok(true)
    }
    fn pop(&mut self) {
        let entry = self.slots[self.head].take();
        self.head = policy::ring_offset(self.head, 1, self.slots.len());
        self.len -= 1;
        drop(entry);
    }
}
impl<R> Drop for Queue<'_, '_, R> {
    fn drop(&mut self) {
        for slot in self.slots.iter_mut() {
            *slot = None;
        }
    }
}

#[must_use]
pub struct QueueSlot<'a, 'pool, 'storage, R = ()> {
    queue: &'a mut Queue<'pool, 'storage, R>,
    resources: R,
}
impl<'pool, R> QueueSlot<'_, 'pool, '_, R> {
    /// Populate metadata while admission is still private and reversible.
    pub fn resources_mut(&mut self) -> &mut R {
        &mut self.resources
    }

    pub fn accept(
        self,
        messages: &mut Messages<'pool, '_, '_>,
        offer: Offer<'pool>,
    ) -> SendKey<'pool> {
        let key = messages.accept(offer);
        let index = policy::ring_offset(self.queue.head, self.queue.len, self.queue.slots.len());
        self.queue.slots[index] = Some(QueueEntry {
            message: QueuedMessage::Tracked(key),
            resources: self.resources,
        });
        self.queue.len += 1;
        key
    }

    /// Transfer a lease directly to this reserved FIFO slot. Capacity is bounded
    /// by this queue; no separately addressable send ID or Messages send slot is
    /// created. The queue owns progress, resources and terminal cleanup. Partial
    /// writes cannot expose a successor, and dropping a pending window preserves
    /// the lease. Stop IO and discard failed queues before reusing their pool.
    pub fn accept_owned(self, lease: Lease<'pool>) {
        let index = policy::ring_offset(self.queue.head, self.queue.len, self.queue.slots.len());
        self.queue.slots[index] = Some(QueueEntry {
            message: QueuedMessage::Owned { lease, position: 0 },
            resources: self.resources,
        });
        self.queue.len += 1;
    }
}

pub struct QueueWindow<'a, 'pool, 'queue, 'buffers, 'bytes, R = ()> {
    queue: &'a mut Queue<'pool, 'queue, R>,
    tracked: Option<&'a mut Option<Send>>,
    buffer: &'buffers Buffer<'bytes>,
    bytes: Ref<'buffers, [u8]>,
    position: usize,
}
impl<R> QueueWindow<'_, '_, '_, '_, '_, R> {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    #[cfg(feature = "alloc")]
    pub fn share(&self) -> Option<SharedBytes> {
        self.buffer
            .bytes
            .borrow()
            .share(self.position..self.position + self.bytes.len())
    }
    /// Resources are offered only with the first byte. After any positive
    /// accepted write this returns None, even if the frame remains incomplete.
    pub fn resources(&self) -> Option<&R> {
        let entry = self.queue.slots[self.queue.head].as_ref().unwrap();
        (self.position == 0).then_some(&entry.resources)
    }
    /// Complete the oldest frame before allowing access to its successor.
    /// Invalid counts preserve both byte progress and attachment eligibility.
    pub fn advance(self, count: usize) -> Result<bool, Error> {
        let (next, complete) = policy::advance(self.position, self.bytes.len(), count)?;
        drop(self.bytes);
        if complete {
            let state = policy::release(self.buffer.state.get(), self.tracked.is_some());
            self.buffer.state.set(if policy::reclaimable(state) {
                State::Free
            } else {
                state
            });
            if let Some(send) = self.tracked {
                send.take();
            }
            self.queue.pop();
        } else if let Some(send) = self.tracked {
            send.as_mut().expect("live window").position = next;
        } else {
            let QueuedMessage::Owned { position, .. } =
                &mut self.queue.slots[self.queue.head].as_mut().unwrap().message
            else {
                unreachable!("owned FIFO window")
            };
            *position = next;
        }
        Ok(complete)
    }
}
#[cfg(kani)]
mod proofs;
#[cfg(test)]
mod tests;
