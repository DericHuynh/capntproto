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
    #[cfg(feature = "alloc")]
    Parts(SharedParts),
    Tracked(SendKey<'pool>),
    Owned {
        lease: Lease<'pool>,
        position: usize,
    },
}

/// Immutable bytes transferred directly into a hosted physical FIFO.
#[cfg(feature = "alloc")]
pub enum Publication {
    Bytes(SharedBytes),
    Parts(SharedParts),
}
/// Publication and flush share these counters, so scheduling reads neither
/// borrow a transport nor call through its publication interface.
#[cfg(feature = "alloc")]
pub struct OutputStatus {
    pending: Cell<usize>,
    maximum: Cell<usize>,
    failed: Cell<bool>,
    closed: Cell<bool>,
}
#[cfg(feature = "alloc")]
impl OutputStatus {
    pub fn new(maximum: usize) -> Self {
        Self {
            pending: Cell::new(0),
            maximum: Cell::new(maximum),
            failed: Cell::new(false),
            closed: Cell::new(false),
        }
    }
    pub fn pending(&self) -> usize {
        self.pending.get()
    }
    pub fn is_closed(&self) -> bool {
        self.closed.get()
    }
    pub fn close(&self) -> bool {
        self.closed.replace(true)
    }
    pub fn failed(&self) -> bool {
        self.failed.get()
    }
    pub fn fail(&self) {
        self.failed.set(true);
    }
    pub fn published(&self) {
        self.pending.set(self.pending.get() + 1);
    }
    pub fn flushed(&self) {
        self.pending.set(0);
    }
    pub fn full(&self) -> bool {
        self.pending.get() >= self.maximum.get()
    }
    pub fn set_maximum(&self, maximum: usize) -> Result<(), Error> {
        if self.pending.get() != 0 {
            return Err(Error::NonemptyStorage);
        }
        self.maximum.set(maximum);
        Ok(())
    }
    fn ensure_capacity(&self) -> Result<(), Error> {
        if self.failed.get() || self.closed.get() {
            Err(Error::QueueFailed)
        } else if self.full() {
            Err(Error::NoSendSlot)
        } else {
            Ok(())
        }
    }
}
/// A single physical output queue. Reservation is exclusive until publication
/// or cancellation; accepted owners must survive through physical flush.
#[cfg(feature = "alloc")]
pub trait PublicationSink<R> {
    fn status(&self) -> &alloc::rc::Rc<OutputStatus>;
    fn reserve(&mut self) -> Result<(), Error>;
    fn cancel_reservation(&mut self);
    fn publish_reserved(&mut self, frame: Publication, resources: R);
}
#[cfg(feature = "alloc")]
struct Direct<'s, R> {
    sink: alloc::boxed::Box<dyn PublicationSink<R> + 's>,
    status: alloc::rc::Rc<OutputStatus>,
}

pub struct Queue<'pool, 'storage, R = ()> {
    slots: crate::storage::Storage<'storage, Option<QueueEntry<'pool, R>>>,
    head: usize,
    len: usize,
    failed: bool,
    #[cfg(feature = "alloc")]
    sink: Option<Direct<'storage, R>>,
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
            #[cfg(feature = "alloc")]
            sink: None,
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
            #[cfg(feature = "alloc")]
            sink: None,
        })
    }
    /// Hosted publication requires an allocating pool whose immutable buffers
    /// can outlive this scope. Fixed storage continues to use its local FIFO.
    #[cfg(feature = "alloc")]
    pub fn hosted(
        messages: &Messages<'pool, '_, '_>,
        sink: alloc::boxed::Box<dyn PublicationSink<R> + 'storage>,
    ) -> Result<Self, Error> {
        if !matches!(messages.buffers, Buffers::Growing(_)) {
            return Err(Error::BorrowedBuffer);
        }
        Ok(Self {
            slots: crate::storage::Storage::borrowed(&mut []),
            head: 0,
            len: 0,
            failed: false,
            sink: Some(Direct {
                status: sink.status().clone(),
                sink,
            }),
        })
    }
    pub(crate) fn is_direct(&self) -> bool {
        #[cfg(feature = "alloc")]
        {
            self.sink.is_some()
        }
        #[cfg(not(feature = "alloc"))]
        {
            false
        }
    }
    pub fn len(&self) -> usize {
        #[cfg(feature = "alloc")]
        if let Some(sink) = &self.sink {
            return sink.status.pending();
        }
        self.len
    }
    pub fn maximum(&self) -> usize {
        #[cfg(feature = "alloc")]
        if let Some(sink) = &self.sink {
            return sink.status.maximum.get();
        }
        self.slots.maximum()
    }
    pub fn available(&self) -> usize {
        #[cfg(feature = "alloc")]
        if let Some(sink) = &self.sink {
            return sink
                .status
                .maximum
                .get()
                .saturating_sub(sink.status.pending());
        }
        self.slots.maximum().saturating_sub(self.len)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn is_failed(&self) -> bool {
        #[cfg(feature = "alloc")]
        if let Some(sink) = &self.sink {
            return self.failed || sink.status.failed();
        }
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
        #[cfg(feature = "alloc")]
        if let Some(sink) = &mut self.sink {
            sink.sink.reserve()?;
        }
        Ok(QueueSlot {
            queue: self,
            resources: Some(resources),
            accepted: false,
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
        #[cfg(feature = "alloc")]
        if let Some(sink) = &self.sink {
            return sink.status.ensure_capacity();
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
        if self.is_direct() || self.is_empty() {
            return Ok(None);
        }
        #[cfg(feature = "alloc")]
        if let QueuedMessage::Parts(parts) = &self.slots[self.head].as_ref().unwrap().message {
            let position = parts.position();
            return Ok(Some(QueueWindow {
                position,
                queue: self,
                body: WindowBody::Parts,
            }));
        }
        let (tracked, buffer, bytes, position) =
            match &self.slots[self.head].as_ref().unwrap().message {
                #[cfg(feature = "alloc")]
                QueuedMessage::Parts(_) => unreachable!("handled above"),
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
            body: WindowBody::Buffer {
                tracked,
                buffer,
                bytes,
            },
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
        if self.is_direct() || self.is_empty() {
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
    resources: Option<R>,
    accepted: bool,
}
impl<R> Drop for QueueSlot<'_, '_, '_, R> {
    fn drop(&mut self) {
        #[cfg(feature = "alloc")]
        if !self.accepted {
            if let Some(sink) = &mut self.queue.sink {
                sink.sink.cancel_reservation();
            }
        }
    }
}
impl<'pool, R> QueueSlot<'_, 'pool, '_, R> {
    /// Populate metadata while admission is still private and reversible.
    pub fn resources_mut(&mut self) -> &mut R {
        self.resources.as_mut().expect("private output resources")
    }

    pub fn accept(
        mut self,
        messages: &mut Messages<'pool, '_, '_>,
        offer: Offer<'pool>,
    ) -> SendKey<'pool> {
        let key = messages.accept(offer);
        #[cfg(feature = "alloc")]
        if let Some(sink) = &mut self.queue.sink {
            let window = messages.send_window(key).expect("accepted send");
            let bytes = window.share().expect("hosted allocating pool");
            let length = bytes.len();
            window
                .advance(length)
                .expect("complete immutable publication");
            self.accepted = true;
            sink.sink
                .publish_reserved(Publication::Bytes(bytes), self.resources.take().unwrap());
            return key;
        }
        let index = policy::ring_offset(self.queue.head, self.queue.len, self.queue.slots.len());
        self.queue.slots[index] = Some(QueueEntry {
            message: QueuedMessage::Tracked(key),
            resources: self.resources.take().unwrap(),
        });
        self.accepted = true;
        self.queue.len += 1;
        key
    }

    /// Transfer a lease directly to this reserved FIFO slot. Capacity is bounded
    /// by this queue; no separately addressable send ID or Messages send slot is
    /// created. The queue owns progress, resources and terminal cleanup. Partial
    /// writes cannot expose a successor, and dropping a pending window preserves
    /// the lease. Stop IO and discard failed queues before reusing their pool.
    pub fn accept_owned(mut self, messages: &mut Messages<'pool, '_, '_>, lease: Lease<'pool>) {
        #[cfg(feature = "alloc")]
        if let Some(sink) = &mut self.queue.sink {
            let bytes = messages.share(&lease).expect("hosted allocating pool");
            messages.release(lease);
            self.accepted = true;
            sink.sink
                .publish_reserved(Publication::Bytes(bytes), self.resources.take().unwrap());
            return;
        }
        #[cfg(not(feature = "alloc"))]
        let _ = messages;
        let index = policy::ring_offset(self.queue.head, self.queue.len, self.queue.slots.len());
        self.queue.slots[index] = Some(QueueEntry {
            message: QueuedMessage::Owned { lease, position: 0 },
            resources: self.resources.take().unwrap(),
        });
        self.accepted = true;
        self.queue.len += 1;
    }

    /// Transfer a charged scatter/gather owner into the same ordered FIFO.
    /// Its charge follows all transport aliases through partial writes/flush.
    #[cfg(feature = "alloc")]
    pub fn accept_parts(mut self, parts: SharedParts) {
        if let Some(sink) = &mut self.queue.sink {
            self.accepted = true;
            sink.sink
                .publish_reserved(Publication::Parts(parts), self.resources.take().unwrap());
            return;
        }
        let index = policy::ring_offset(self.queue.head, self.queue.len, self.queue.slots.len());
        self.queue.slots[index] = Some(QueueEntry {
            message: QueuedMessage::Parts(parts),
            resources: self.resources.take().unwrap(),
        });
        self.accepted = true;
        self.queue.len += 1;
    }
}

pub struct QueueWindow<'a, 'pool, 'queue, 'buffers, 'bytes, R = ()> {
    queue: &'a mut Queue<'pool, 'queue, R>,
    body: WindowBody<'a, 'buffers, 'bytes>,
    position: usize,
}
enum WindowBody<'a, 'buffers, 'bytes> {
    Buffer {
        tracked: Option<&'a mut Option<Send>>,
        buffer: &'buffers Buffer<'bytes>,
        bytes: Ref<'buffers, [u8]>,
    },
    #[cfg(feature = "alloc")]
    Parts,
}
impl<R> QueueWindow<'_, '_, '_, '_, '_, R> {
    pub fn bytes(&self) -> &[u8] {
        match &self.body {
            WindowBody::Buffer { bytes, .. } => bytes,
            #[cfg(feature = "alloc")]
            WindowBody::Parts => {
                let QueuedMessage::Parts(parts) =
                    &self.queue.slots[self.queue.head].as_ref().unwrap().message
                else {
                    unreachable!("parts window")
                };
                parts.first_remaining()
            }
        }
    }
    #[cfg(feature = "alloc")]
    pub fn share(&self) -> Option<SharedBytes> {
        match &self.body {
            WindowBody::Buffer { buffer, bytes, .. } => buffer
                .bytes
                .borrow()
                .share(self.position..self.position + bytes.len()),
            WindowBody::Parts => None,
        }
    }
    #[cfg(feature = "alloc")]
    pub fn share_parts(&self) -> Option<SharedParts> {
        match &self.body {
            WindowBody::Parts => {
                let QueuedMessage::Parts(parts) =
                    &self.queue.slots[self.queue.head].as_ref().unwrap().message
                else {
                    unreachable!("parts window")
                };
                Some(parts.clone())
            }
            WindowBody::Buffer { .. } => None,
        }
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
        if count > self.bytes().len() {
            return Err(Error::InvalidWriteCount);
        }
        self.advance_inner(count)
    }
    /// Commit an adapter's acceptance from the complete shared portions, which
    /// may span more bytes than the first borrowed slice exposed by bytes().
    #[cfg(feature = "alloc")]
    pub fn advance_parts(self, count: usize) -> Result<bool, Error> {
        if !matches!(&self.body, WindowBody::Parts) {
            return Err(Error::InvalidWriteCount);
        }
        self.advance_inner(count)
    }
    fn advance_inner(self, count: usize) -> Result<bool, Error> {
        let (tracked, buffer, bytes) = match self.body {
            WindowBody::Buffer {
                tracked,
                buffer,
                bytes,
            } => (tracked, buffer, bytes),
            #[cfg(feature = "alloc")]
            WindowBody::Parts => {
                let QueuedMessage::Parts(parts) =
                    &mut self.queue.slots[self.queue.head].as_mut().unwrap().message
                else {
                    unreachable!("parts window")
                };
                let complete = parts.advance(count)?;
                if complete {
                    self.queue.pop();
                }
                return Ok(complete);
            }
        };
        let (next, complete) = policy::advance(self.position, bytes.len(), count)?;
        drop(bytes);
        if complete {
            let state = policy::release(buffer.state.get(), tracked.is_some());
            buffer.state.set(if policy::reclaimable(state) {
                State::Free
            } else {
                state
            });
            buffer.release_received();
            if let Some(send) = tracked {
                send.take();
            }
            self.queue.pop();
        } else if let Some(send) = tracked {
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
