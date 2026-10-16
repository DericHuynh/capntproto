//! Buffer addresses remain stable for the whole externally borrowed scope.
use super::*;
use core::ops::{Deref, DerefMut, Index};

pub(super) enum Bytes<'bytes> {
    Borrowed(&'bytes mut [u8]),
    #[cfg(feature = "alloc")]
    Owned {
        allocation: alloc::rc::Rc<Allocation>,
        offset: usize,
        length: usize,
    },
    #[cfg(all(feature = "alloc", target_has_atomic = "ptr"))]
    Received {
        owner: alloc::rc::Rc<Allocation>,
        range: core::ops::Range<usize>,
    },
    #[cfg(all(feature = "alloc", target_has_atomic = "ptr"))]
    Released(alloc::rc::Weak<Allocation>),
}
impl Bytes<'_> {
    pub(super) fn exclusive(&self) -> bool {
        match self {
            Self::Borrowed(_) => true,
            #[cfg(feature = "alloc")]
            Self::Owned { allocation, .. } => alloc::rc::Rc::strong_count(allocation) == 1,
            #[cfg(all(feature = "alloc", target_has_atomic = "ptr"))]
            Self::Received { .. } => false,
            #[cfg(all(feature = "alloc", target_has_atomic = "ptr"))]
            Self::Released(owner) => owner.strong_count() == 0,
        }
    }
    #[cfg(feature = "alloc")]
    pub(super) fn share(&self, range: core::ops::Range<usize>) -> Option<SharedBytes> {
        match self {
            Self::Borrowed(_) => None,
            Self::Owned {
                allocation,
                offset,
                length,
            } => {
                assert!(range.start <= range.end && range.end <= *length);
                Some(SharedBytes {
                    allocation: allocation.clone(),
                    range: offset + range.start..offset + range.end,
                })
            }
            #[cfg(target_has_atomic = "ptr")]
            Self::Received {
                owner,
                range: source,
            } => {
                assert!(range.start <= range.end && range.end <= source.len());
                Some(SharedBytes {
                    allocation: owner.clone(),
                    range: source.start + range.start..source.start + range.end,
                })
            }
            #[cfg(target_has_atomic = "ptr")]
            Self::Released(_) => None,
        }
    }
}

// Pool accounting follows immutable protocol owners, including owners which
// outlive a message scope. The transport's independent reference is not a
// protocol lease and cannot mutate these bytes while this owner exists.
#[cfg(all(feature = "alloc", target_has_atomic = "ptr"))]
pub(super) struct Received {
    pub(super) allocation: alloc::sync::Arc<alloc::vec::Vec<u8>>,
    pub(super) charged: alloc::rc::Rc<Cell<usize>>,
}
#[cfg(all(feature = "alloc", target_has_atomic = "ptr"))]
impl Drop for Received {
    fn drop(&mut self) {
        self.charged
            .set(self.charged.get() - self.allocation.capacity());
    }
}
#[cfg(feature = "alloc")]
pub(super) enum Allocation {
    Local(alloc::vec::Vec<u8>),
    #[cfg(target_has_atomic = "ptr")]
    Received(Received),
}
#[cfg(feature = "alloc")]
impl Allocation {
    pub(super) fn bytes(&self) -> &[u8] {
        match self {
            Self::Local(bytes) => bytes,
            #[cfg(target_has_atomic = "ptr")]
            Self::Received(owner) => &owner.allocation,
        }
    }
    pub(super) fn capacity(&self) -> usize {
        match self {
            Self::Local(bytes) => bytes.capacity(),
            #[cfg(target_has_atomic = "ptr")]
            Self::Received(owner) => owner.allocation.capacity(),
        }
    }
    fn local_mut(&mut self) -> &mut alloc::vec::Vec<u8> {
        match self {
            Self::Local(bytes) => bytes,
            #[cfg(target_has_atomic = "ptr")]
            Self::Received(_) => unreachable!("received storage is immutable"),
        }
    }
}

/// An immutable used range of an allocating message buffer. Cloning retains
/// bytes without copying or allocating. The pool cannot reuse or grow this
/// buffer until all shared owners are gone, even after its scope terminates.
/// No mutable access or underlying allocation owner is exposed.
#[cfg(feature = "alloc")]
#[derive(Clone)]
pub struct SharedBytes {
    allocation: alloc::rc::Rc<Allocation>,
    range: core::ops::Range<usize>,
}
#[cfg(feature = "alloc")]
impl Deref for SharedBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.allocation.bytes()[self.range.clone()]
    }
}
#[cfg(feature = "alloc")]
impl AsRef<[u8]> for SharedBytes {
    fn as_ref(&self) -> &[u8] {
        self
    }
}
impl Default for Bytes<'_> {
    fn default() -> Self {
        Self::Borrowed(&mut [])
    }
}
impl Deref for Bytes<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Borrowed(bytes) => bytes,
            #[cfg(feature = "alloc")]
            Self::Owned {
                allocation,
                offset,
                length,
            } => &allocation.bytes()[*offset..*offset + *length],
            #[cfg(all(feature = "alloc", target_has_atomic = "ptr"))]
            Self::Received { owner, range } => &owner.bytes()[range.clone()],
            #[cfg(all(feature = "alloc", target_has_atomic = "ptr"))]
            Self::Released(_) => &[],
        }
    }
}
impl DerefMut for Bytes<'_> {
    fn deref_mut(&mut self) -> &mut [u8] {
        match self {
            Self::Borrowed(bytes) => bytes,
            #[cfg(feature = "alloc")]
            Self::Owned {
                allocation,
                offset,
                length,
            } => &mut alloc::rc::Rc::get_mut(allocation)
                .expect("a writing buffer has no shared immutable owners")
                .local_mut()[*offset..*offset + *length],
            #[cfg(all(feature = "alloc", target_has_atomic = "ptr"))]
            Self::Received { .. } => {
                unreachable!("received storage is immutable")
            }
            #[cfg(all(feature = "alloc", target_has_atomic = "ptr"))]
            Self::Released(_) => &mut [],
        }
    }
}

pub(super) enum Buffers<'storage, 'bytes> {
    Borrowed(&'storage [Buffer<'bytes>]),
    #[cfg(feature = "alloc")]
    Growing(&'storage GrowingBuffers<'bytes>),
}
impl<'storage, 'bytes> Buffers<'storage, 'bytes> {
    #[inline(always)]
    pub fn at(&self, index: usize) -> &'storage Buffer<'bytes> {
        match self {
            Self::Borrowed(buffers) => &buffers[index],
            #[cfg(feature = "alloc")]
            Self::Growing(buffers) => buffers.at(index),
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = &'storage Buffer<'bytes>> + '_ {
        let len = match self {
            Self::Borrowed(buffers) => buffers.len(),
            #[cfg(feature = "alloc")]
            Self::Growing(buffers) => buffers.len(),
        };
        (0..len).map(|index| self.at(index))
    }
    pub fn append(&self, minimum: usize) -> Option<usize> {
        match self {
            Self::Borrowed(_) => {
                let _ = minimum;
                None
            }
            #[cfg(feature = "alloc")]
            Self::Growing(buffers) => buffers.append(minimum),
        }
    }
}
impl<'bytes> Index<usize> for Buffers<'_, 'bytes> {
    type Output = Buffer<'bytes>;
    #[inline(always)]
    fn index(&self, index: usize) -> &Self::Output {
        self.at(index)
    }
}

/// Segment zero holds one slot; later segments double. Reject the only index
/// that cannot belong to a usize-sized collection before adding or shifting.
#[cfg(any(feature = "alloc", kani))]
pub(super) fn segment_position(index: usize) -> Option<(usize, usize)> {
    let next = index.checked_add(1)?;
    let segment = (usize::BITS - next.leading_zeros() - 1) as usize;
    Some((segment, index - ((1usize << segment) - 1)))
}

#[cfg(kani)]
mod proofs {
    #[kani::proof]
    fn stable_segments_cover_full_width_indices() {
        let index: usize = kani::any();
        match super::segment_position(index) {
            Some((segment, offset)) => {
                assert!(segment < usize::BITS as usize);
                let width = 1usize << segment;
                kani::assert(offset < width, "segment offset stays within its allocation");
                kani::assert(
                    width - 1 + offset == index,
                    "segment position identifies original index",
                );
            }
            None => assert_eq!(index, usize::MAX),
        }
    }
}
