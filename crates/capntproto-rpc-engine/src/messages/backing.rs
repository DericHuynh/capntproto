//! Buffer addresses remain stable for the whole externally borrowed scope.
use super::*;
use core::ops::{Deref, DerefMut, Index};

pub(super) enum Bytes<'bytes> {
    Borrowed(&'bytes mut [u8]),
    #[cfg(feature = "alloc")]
    Owned {
        allocation: alloc::rc::Rc<alloc::vec::Vec<u8>>,
        offset: usize,
        length: usize,
    },
}
impl Bytes<'_> {
    pub(super) fn exclusive(&self) -> bool {
        match self {
            Self::Borrowed(_) => true,
            #[cfg(feature = "alloc")]
            Self::Owned { allocation, .. } => alloc::rc::Rc::strong_count(allocation) == 1,
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
    allocation: alloc::rc::Rc<alloc::vec::Vec<u8>>,
    range: core::ops::Range<usize>,
}
#[cfg(feature = "alloc")]
impl Deref for SharedBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.allocation[self.range.clone()]
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
            } => &allocation[*offset..*offset + *length],
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
                [*offset..*offset + *length],
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
