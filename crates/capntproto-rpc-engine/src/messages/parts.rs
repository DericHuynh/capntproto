//! Immutable scatter/gather output charged through the last asynchronous owner.
use super::*;
use alloc::rc::Rc;

/// Immutable portions of one byte stream. Implementations must keep each part,
/// its extent and its contents unchanged for their whole lifetime. The retained
/// capacity includes every backing allocation, not just the exposed prefixes.
/// This is an ownership/accounting contract, not an unsafe memory interface.
pub trait Parts {
    fn count(&self) -> usize;
    fn part(&self, index: usize) -> &[u8];
    fn retained_capacity(&self) -> usize;
}

#[derive(Default)]
pub(super) struct Charges {
    pub(super) bytes: Cell<usize>,
    pub(super) slots: Cell<usize>,
}
struct Allocation {
    source: Rc<dyn Parts>,
    charges: Rc<Charges>,
    capacity: usize,
    length: usize,
}
impl Drop for Allocation {
    fn drop(&mut self) {
        self.charges
            .bytes
            .set(self.charges.bytes.get() - self.capacity);
        self.charges.slots.set(self.charges.slots.get() - 1);
    }
}

/// An immutable stream owner. Clones retain the same pool charge, including
/// after queue completion, scope termination or destruction of the provider.
#[derive(Clone)]
pub struct SharedParts {
    allocation: Rc<Allocation>,
    position: usize,
}
impl SharedParts {
    pub fn len(&self) -> usize {
        self.allocation.length - self.position
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn position(&self) -> usize {
        self.position
    }
    pub fn count(&self) -> usize {
        self.allocation.source.count()
    }
    /// The original complete parts; position() describes consumed stream bytes.
    pub fn part(&self, index: usize) -> &[u8] {
        self.allocation.source.part(index)
    }
    pub fn first_remaining(&self) -> &[u8] {
        let mut offset = self.position;
        for index in 0..self.count() {
            let part = self.part(index);
            if offset < part.len() {
                return &part[offset..];
            }
            offset -= part.len();
        }
        &[]
    }
    pub(super) fn advance(&mut self, count: usize) -> Result<bool, Error> {
        let (next, complete) = policy::advance(self.position, self.len(), count)?;
        self.position = next;
        Ok(complete)
    }
}

impl GrowingBuffers<'_> {
    pub(super) fn parts_slots(&self) -> usize {
        self.parts_charges.get().map_or(0, |c| c.slots.get())
    }
    pub(super) fn parts_bytes(&self) -> usize {
        self.parts_charges.get().map_or(0, |c| c.bytes.get())
    }
    pub(super) fn retain_parts(&self, source: Rc<dyn Parts>) -> Result<SharedParts, Error> {
        let length = (0..source.count()).try_fold(0usize, |n, i| {
            n.checked_add(source.part(i).len())
                .ok_or(Error::InvalidLength)
        })?;
        let capacity = source.retained_capacity();
        if length > capacity {
            return Err(Error::InvalidLength);
        }
        if length > self.maximum_buffer_bytes
            || self.parts_slots() >= self.maximum_buffers - self.len()
            || capacity > self.maximum_bytes - self.retained_bytes()
        {
            return Err(Error::NoBuffer);
        }
        let charges = self
            .parts_charges
            .get_or_init(|| Rc::new(Charges::default()));
        charges.bytes.set(charges.bytes.get() + capacity);
        charges.slots.set(charges.slots.get() + 1);
        Ok(SharedParts {
            allocation: Rc::new(Allocation {
                source,
                charges: charges.clone(),
                capacity,
                length,
            }),
            position: 0,
        })
    }
}
impl Messages<'_, '_, '_> {
    /// Charge immutable output segments to a growing provider. Fixed providers
    /// return None and keep the ordinary copy path. Failure leaves every existing
    /// owner and charge intact. Aliased independent adoptions are charged again.
    pub fn retain_parts(&mut self, source: Rc<dyn Parts>) -> Result<Option<SharedParts>, Error> {
        match &self.buffers {
            Buffers::Borrowed(_) => Ok(None),
            Buffers::Growing(buffers) => buffers.retain_parts(source).map(Some),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::Growth;

    struct Source([alloc::vec::Vec<u8>; 3]);
    impl Parts for Source {
        fn count(&self) -> usize {
            self.0.len()
        }
        fn part(&self, index: usize) -> &[u8] {
            &self.0[index]
        }
        fn retained_capacity(&self) -> usize {
            self.0.iter().map(|p| p.capacity()).sum()
        }
    }
    fn source() -> Rc<dyn Parts> {
        let mut parts = [
            alloc::vec::Vec::with_capacity(32),
            alloc::vec::Vec::new(),
            alloc::vec::Vec::with_capacity(32),
        ];
        parts[0].extend_from_slice(b"abc");
        parts[2].extend_from_slice(b"def");
        Rc::new(Source(parts))
    }
    #[test]
    fn charges_survive_aliases_scopes_and_provider_drop() {
        let source = source();
        let capacity = source.retained_capacity();
        let mut pool = GrowingBuffers::new(2, 64, capacity * 2);
        let owner = with_growing_messages(
            &mut pool,
            Growth {
                initial: 0,
                maximum: 0,
            },
            |messages| {
                let first = messages.retain_parts(source.clone()).unwrap().unwrap();
                let alias = first.clone();
                let second = messages.retain_parts(source.clone()).unwrap().unwrap();
                assert!(matches!(
                    messages.retain_parts(source.clone()),
                    Err(Error::NoBuffer)
                ));
                assert!(matches!(messages.reserve(1), Err(Error::NoBuffer)));
                drop(first);
                drop(second);
                alias
            },
        )
        .unwrap();
        assert_eq!(pool.retained_bytes(), capacity);
        with_growing_messages(
            &mut pool,
            Growth {
                initial: 0,
                maximum: 0,
            },
            |messages| {
                let second = messages.retain_parts(source.clone()).unwrap().unwrap();
                assert!(matches!(
                    messages.retain_parts(source.clone()),
                    Err(Error::NoBuffer)
                ));
                drop(second);
            },
        )
        .unwrap();
        drop(pool);
        assert_eq!(owner.first_remaining(), b"abc");
        assert_eq!(owner.part(2), b"def");
        drop(owner);
        assert_eq!(Rc::strong_count(&source), 1);
    }
    #[test]
    fn segmented_fifo_preserves_partial_progress_resources_and_failure_cleanup() {
        let source = source();
        let capacity = source.retained_capacity();
        let mut pool = GrowingBuffers::new(2, 64, capacity + 16);
        let escaped = with_growing_messages(
            &mut pool,
            Growth {
                initial: 0,
                maximum: 0,
            },
            |messages| {
                let parts = messages.retain_parts(source.clone()).unwrap().unwrap();
                let escaped = parts.clone();
                let mut slots = [None, None];
                let mut queue = Queue::with_resources(&mut slots).unwrap();
                queue.reserve_with(17).unwrap().accept_parts(parts);
                let successor = messages.copy_from(b"ghi").unwrap();
                queue.reserve_with(19).unwrap().accept_owned(successor);
                let window = queue.window(messages).unwrap().unwrap();
                assert_eq!(window.bytes(), b"abc");
                assert_eq!(window.resources(), Some(&17));
                assert_eq!(window.advance(7), Err(Error::InvalidWriteCount));
                // A borrowed writer cannot claim bytes beyond its first part.
                assert_eq!(
                    queue.window(messages).unwrap().unwrap().advance(4),
                    Err(Error::InvalidWriteCount)
                );
                let window = queue.window(messages).unwrap().unwrap();
                assert_eq!(window.advance(0), Err(Error::WriteZero));
                let window = queue.window(messages).unwrap().unwrap();
                assert_eq!(window.advance(2), Ok(false));
                let window = queue.window(messages).unwrap().unwrap();
                assert_eq!(window.bytes(), b"c");
                assert_eq!(window.resources(), None);
                drop(window); // Pending does not advance or expose the successor.
                assert_eq!(
                    queue.window(messages).unwrap().unwrap().advance(1),
                    Ok(false)
                );
                let window = queue.window(messages).unwrap().unwrap();
                assert_eq!(window.bytes(), b"def"); // Empty intermediate part is skipped.
                assert_eq!(window.advance(3), Ok(true));
                let window = queue.window(messages).unwrap().unwrap();
                assert_eq!(window.bytes(), b"ghi");
                assert_eq!(window.resources(), Some(&19));
                drop(window);
                queue.fail();
                assert_eq!(queue.discard_one(messages), Ok(true));
                assert_eq!(queue.discard_one(messages), Ok(false));
                escaped
            },
        )
        .unwrap();
        assert!(pool.retained_bytes() >= capacity);
        drop(escaped);
        assert!(pool.retained_bytes() < capacity);
        let mut bytes = [0; 8];
        let mut buffers = [Buffer::new(&mut bytes)];
        with_messages(&mut buffers, &mut [], |messages| {
            assert!(messages.retain_parts(source).unwrap().is_none());
            let writer = messages.reserve(8).unwrap();
            messages.discard(writer);
        })
        .unwrap();
    }
}
