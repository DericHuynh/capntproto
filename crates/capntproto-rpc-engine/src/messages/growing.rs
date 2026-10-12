//! Stable, fallible allocating storage for the same scoped message transitions.
use super::*;
use alloc::vec::Vec;
use core::cell::OnceCell;

/// Optional allocating buffer provider. Own this outside `with_growing_messages`
/// so suspended tasks can borrow individual buffers for the complete scope.
/// Buffers are retained for reuse until the provider is dropped. Reservations
/// enforce independent buffer-count, per-buffer size and retained-byte limits.
/// The byte limit includes alignment padding and allocator-reported capacity;
/// slot metadata is bounded separately by the maximum buffer count. Growing a
/// free buffer validates a temporary replacement before releasing its old bytes,
/// so transient allocation can exceed the retained-byte limit by that replacement.
///
/// Once installed, a buffer record never moves. The first three records are
/// embedded in the provider; later records use geometrically sized segments.
/// Both provide constant-time lookup without relocating a borrowed record. Each
/// byte buffer exposes an eight-byte-aligned slice without pointer casts or
/// lifetime extension. Fixed message storage still never allocates.
/// Shared byte owners keep their buffer charged and unavailable for reuse.
/// Byte growth uses fallible reservations; each installed buffer also has one
/// reference-count metadata allocation with the global allocator's OOM policy.
///
/// ```
/// # #[cfg(feature = "alloc")]
/// # {
/// use capntproto_rpc_engine::{connection::Growth, messages};
/// // At most two buffers, 1024 usable bytes each, including padding in the
/// // aggregate retained-byte budget. No buffer is allocated yet.
/// let mut pool = messages::GrowingBuffers::new(2, 1024, 2 * (1024 + 7));
/// messages::with_growing_messages(&mut pool, Growth { initial: 0, maximum: 2 }, |messages| {
///     let writer = messages.reserve(64).unwrap();
///     messages.write(&writer)[..3].copy_from_slice(b"rpc");
///     let lease = messages.seal(writer, 3).unwrap();
///     assert_eq!(&*messages.read(&lease), b"rpc");
///     messages.release(lease);
/// }).unwrap();
/// # }
/// ```
pub struct GrowingBuffers<'bytes> {
    inline: [OnceCell<Buffer<'bytes>>; 3],
    segments: [OnceCell<Vec<OnceCell<Buffer<'bytes>>>>; usize::BITS as usize],
    count: Cell<usize>,
    retained_bytes: Cell<usize>,
    maximum_buffers: usize,
    maximum_buffer_bytes: usize,
    maximum_bytes: usize,
}
impl<'bytes> GrowingBuffers<'bytes> {
    /// Start empty; reserve memory only as messages are admitted.
    pub const fn new(
        maximum_buffers: usize,
        maximum_buffer_bytes: usize,
        maximum_bytes: usize,
    ) -> Self {
        Self {
            inline: [const { OnceCell::new() }; 3],
            segments: [const { OnceCell::new() }; usize::BITS as usize],
            count: Cell::new(0),
            retained_bytes: Cell::new(0),
            maximum_buffers,
            maximum_buffer_bytes,
            maximum_bytes,
        }
    }
    pub fn len(&self) -> usize {
        self.count.get()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes.get()
    }

    #[inline(always)]
    pub(super) fn at(&self, index: usize) -> &Buffer<'bytes> {
        if index < self.inline.len() {
            return self.inline[index].get().expect("installed buffer");
        }
        let (segment, offset) = backing::segment_position(index).expect("live buffer index");
        self.segments[segment]
            .get()
            .expect("allocated buffer segment")[offset]
            .get()
            .expect("installed buffer")
    }
    pub(super) fn append(&self, minimum: usize) -> Option<usize> {
        if minimum > self.maximum_buffer_bytes {
            return None;
        }
        // Reuse a free, smaller record before consuming another buffer slot.
        // The temporary replacement is validated before ownership or accounting
        // changes; rejected growth leaves the old bytes and generation intact.
        for index in 0..self.len() {
            let buffer = self.at(index);
            if buffer.state.get() != State::Free || buffer.generation.get() == u64::MAX {
                continue;
            }
            let Ok(mut bytes) = buffer.bytes.try_borrow_mut() else {
                continue;
            };
            if !bytes.exclusive() {
                continue;
            }
            let Bytes::Owned { allocation, .. } = &*bytes else {
                unreachable!()
            };
            let previous = allocation.capacity();
            let available = self.maximum_bytes - (self.retained_bytes() - previous);
            if let Some((replacement, charged)) = Self::allocate(minimum, available) {
                *bytes = replacement;
                buffer.capacity.set(minimum);
                self.retained_bytes
                    .set(self.retained_bytes() - previous + charged);
                return Some(index);
            }
        }
        let index = self.len();
        if index >= self.maximum_buffers {
            return None;
        }
        let allocation_len = minimum.checked_add(7)?;
        let available = self.maximum_bytes.checked_sub(self.retained_bytes())?;
        if allocation_len > available {
            return None;
        }
        let (segment, offset) = backing::segment_position(index).expect("live buffer index");
        if index >= self.inline.len() && self.segments[segment].get().is_none() {
            let first = (1usize << segment) - 1;
            let length = (1usize << segment).min(self.maximum_buffers - first);
            let mut records = Vec::new();
            records.try_reserve_exact(length).ok()?;
            records.resize_with(length, OnceCell::new);
            // Installation retains only empty metadata if byte allocation fails.
            self.segments[segment].set(records).ok()?;
        }
        let (bytes, charged) = Self::allocate(minimum, available)?;
        let buffer = Buffer {
            bytes: RefCell::new(bytes),
            capacity: Cell::new(minimum),
            generation: Cell::new(0),
            state: Cell::new(State::Free),
        };
        if index < self.inline.len() {
            self.inline[index].set(buffer).ok()?;
        } else {
            self.segments[segment].get()?[offset].set(buffer).ok()?;
        }
        self.retained_bytes.set(self.retained_bytes() + charged);
        self.count.set(index + 1);
        Some(index)
    }
    fn allocate(minimum: usize, available: usize) -> Option<(Bytes<'bytes>, usize)> {
        let allocation_len = minimum.checked_add(7).filter(|n| *n <= available)?;
        let mut allocation = Vec::new();
        allocation.try_reserve_exact(allocation_len).ok()?;
        // Charge allocator-reported capacity, including alignment padding.
        let charged = allocation.capacity();
        if charged > available {
            return None;
        }
        allocation.resize(allocation_len, 0);
        let offset = allocation.as_ptr().align_offset(8);
        debug_assert!(offset <= 7);
        Some((
            Bytes::Owned {
                allocation: alloc::rc::Rc::new(allocation),
                offset,
                length: minimum,
            },
            charged,
        ))
    }
    fn prepare_scope(&mut self) -> Result<(), Error> {
        for slot in &mut self.inline {
            Self::prepare_slot(slot)?;
        }
        for segment in &mut self.segments {
            if let Some(records) = segment.get_mut() {
                for slot in records {
                    Self::prepare_slot(slot)?;
                }
            }
        }
        Ok(())
    }
    fn prepare_slot(slot: &mut OnceCell<Buffer<'bytes>>) -> Result<(), Error> {
        if let Some(buffer) = slot.get_mut() {
            if buffer.state.get() != State::Free {
                return Err(Error::NonemptyStorage);
            }
            // Exclusive provider access proves live views have gone; reset
            // borrow flags left by deliberately forgotten views.
            let bytes = core::mem::take(buffer.bytes.get_mut());
            buffer.bytes = RefCell::new(bytes);
        }
        Ok(())
    }
}

fn new_growing_scope<'id, 'storage, 'bytes>(
    buffers: &'storage mut GrowingBuffers<'bytes>,
    sends: crate::connection::Growth,
) -> Result<Messages<'id, 'storage, 'bytes>, Error> {
    buffers.prepare_scope()?;
    let sends = crate::storage::Storage::growing(sends.initial, sends.maximum, || SendSlot::EMPTY)
        .ok_or(Error::NoSendSlot)?;
    Ok(Messages {
        buffers: Buffers::Growing(buffers),
        sends,
        brand: PhantomData,
    })
}

/// Lend stable growing buffers and fallible send slots to the shared protocol.
/// Buffer memory is reused across scopes. Send identities are scoped, so no
/// completion from a previous scope can be presented to a new one.
pub fn with_growing_messages<'storage, 'bytes, R>(
    buffers: &'storage mut GrowingBuffers<'bytes>,
    sends: crate::connection::Growth,
    f: impl for<'id> FnOnce(&mut Messages<'id, 'storage, 'bytes>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_growing_scope(buffers, sends)?))
}

/// Keep the same stable message loans through caller-polled async suspension.
/// Stop IO before dropping this future or tearing down its scope.
pub async fn with_growing_messages_async<'storage, 'bytes, R>(
    buffers: &'storage mut GrowingBuffers<'bytes>,
    sends: crate::connection::Growth,
    f: impl for<'id> core::ops::AsyncFnOnce(&mut Messages<'id, 'storage, 'bytes>) -> R,
) -> Result<R, Error> {
    Ok(f(&mut new_growing_scope(buffers, sends)?).await)
}

#[cfg(test)]
mod tests;
