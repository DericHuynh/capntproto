// Copyright (c) 2013-2017 Sandstorm Development Group, Inc. and contributors
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

use core::slice;

use crate::message;
use crate::message::Allocator;
use crate::message::ReadBudget;
use crate::message::ReaderSegments;
use crate::private::units::*;
use crate::OutputSegments;
use crate::{Error, ErrorKind, Result};
use core::borrow::Borrow;

pub(crate) mod fixed;
#[cfg(test)]
mod tests;

pub type SegmentId = u32;

/// Validated access to the segments backing a message.
///
/// # Safety
/// Implementations must return stable, initialized storage for the arena's
/// lifetime, aligned to a word unless the `unaligned` feature is enabled.
/// Bounds checks must reject ranges outside the named segment before callers
/// dereference them. Shared reads must not race with mutation or deallocation.
pub unsafe trait ReaderArena {
    // return pointer to start of segment, and number of words in that segment
    fn get_segment(&self, id: u32) -> Result<(*const u8, u32)>;

    /// Locate a word offset within a segment, allowing its one-past-end pointer.
    ///
    /// # Safety
    /// `start` must derive from the named live segment (or its one-past-end
    /// pointer). The returned pointer may only be dereferenced after validating
    /// the desired access length with `contains_interval`.
    unsafe fn check_offset(
        &self,
        segment_id: u32,
        start: *const u8,
        offset_in_words: i32,
    ) -> Result<*const u8> {
        let (segment_start, segment_len) = self.get_segment(segment_id)?;
        let this_start: usize = segment_start as usize;
        let this_size: usize = segment_len as usize * BYTES_PER_WORD;
        let offset: i64 = i64::from(offset_in_words) * i64::try_from(BYTES_PER_WORD).unwrap();
        let start_idx = start as usize;
        if start_idx < this_start {
            return Err(Error::from_kind(
                ErrorKind::MessageContainsOutOfBoundsPointer,
            ));
        }
        let target_idx = i64::try_from(start_idx - this_start).unwrap() + offset;
        // A wire offset can exceed usize on a 32-bit target. Reject it in
        // the widened domain before converting it to a native pointer offset.
        if target_idx < 0 || target_idx as u64 > this_size as u64 {
            Err(Error::from_kind(
                ErrorKind::MessageContainsOutOfBoundsPointer,
            ))
        } else {
            // SAFETY: start has segment provenance by the caller's contract;
            // target_idx above is within that allocation or one past its end.
            unsafe { Ok(start.offset(isize::try_from(offset).unwrap())) }
        }
    }

    fn contains_interval(&self, segment_id: u32, start: *const u8, size: usize) -> Result<()>;

    /// Validate a relative pointer and its complete target in one operation,
    /// charging the same traversal budget as `contains_interval`.
    ///
    /// # Safety
    /// `start` must derive from the named live segment, as for `check_offset`.
    unsafe fn check_offset_and_read(
        &self,
        segment_id: u32,
        start: *const u8,
        offset_in_words: i32,
        size_in_words: usize,
    ) -> Result<*const u8> {
        // SAFETY: the caller supplies a pointer from the named live segment.
        let target = unsafe { self.check_offset(segment_id, start, offset_in_words)? };
        self.contains_interval(segment_id, target, size_in_words)?;
        Ok(target)
    }

    /// Validate descendants using this arena's concrete bounds implementation.
    ///
    /// # Safety
    /// The pointer must retain this exact arena; its pointer word and segment ID
    /// must have been validated by it. The existing traversal budget is shared.
    unsafe fn validate_capability_indices(
        &self,
        pointer: super::layout::PointerReader<'_>,
        count: u32,
    ) -> Result<()> {
        // SAFETY: the supplied pointer's original arena meets the same contract.
        pointer.validate_capability_indices_default(count)
    }

    fn amplified_read(&self, virtual_amount: u64) -> Result<()>;

    fn nesting_limit(&self) -> i32;

    fn size_in_words(&self) -> usize;

    // TODO(apibump): Consider putting extract_cap(), inject_cap(), drop_cap() here
    //   and on message::Reader. Then we could get rid of Imbue and ImbueMut, and
    //   layout::StructReader, layout::ListReader, etc. could drop their `cap_table` fields.
}

pub struct ReaderArenaImpl<S, B = ReadBudget> {
    segments: S,
    read_limiter: B,
    nesting_limit: i32,
}

#[cfg(feature = "sync_reader")]
fn _assert_sync() {
    fn _assert_sync<T: Sync>() {}
    fn _assert_reader<S: ReaderSegments + Sync>() {
        _assert_sync::<ReaderArenaImpl<S>>();
    }
}

impl<S> ReaderArenaImpl<S>
where
    S: ReaderSegments,
{
    pub fn new(segments: S, options: message::ReaderOptions) -> Self {
        Self::with_budget(
            segments,
            options,
            ReadBudget::new(options.traversal_limit_in_words),
        )
    }
}
impl<S: ReaderSegments, B: Borrow<ReadBudget>> ReaderArenaImpl<S, B> {
    pub(crate) fn with_budget(
        segments: S,
        options: message::ReaderOptions,
        read_limiter: B,
    ) -> Self {
        Self {
            segments,
            read_limiter,
            nesting_limit: options.nesting_limit,
        }
    }

    pub fn into_segments(self) -> S {
        self.segments
    }

    pub(crate) fn get_segments(&self) -> &S {
        &self.segments
    }

    #[inline(always)]
    pub(crate) fn cached(&self) -> Result<CachedReaderArena<&Self>> {
        Ok(CachedReaderArena {
            inner: self,
            first: core::cell::Cell::new(self.get_segment(0)?),
        })
    }
}

/// Cache the common segment only after its arena has a stable address. The
/// private constructors accept either a live borrow or an owning Rc; neither
/// exposes mutable access or moves the arena while cached pointers exist.
#[derive(Clone)]
pub(crate) struct CachedReaderArena<O> {
    inner: O,
    first: core::cell::Cell<(*const u8, u32)>,
}
#[cfg(feature = "alloc")]
pub(crate) struct OwnedArena<S>(ReaderArenaImpl<S>);
#[cfg(feature = "alloc")]
impl<S> core::ops::Deref for OwnedArena<S> {
    type Target = ReaderArenaImpl<S>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
#[cfg(feature = "alloc")]
impl<S: ReaderSegments> CachedReaderArena<OwnedArena<S>> {
    pub(crate) fn owning(arena: ReaderArenaImpl<S>) -> Result<alloc::rc::Rc<Self>> {
        // Pin the complete arena before caching an inline segment's address.
        // Interior initialization touches only the descriptor, never the segment
        // owner, preserving the shared provenance of the cached address.
        let owner = alloc::rc::Rc::new(Self {
            inner: OwnedArena(arena),
            first: core::cell::Cell::new((core::ptr::null(), 0)),
        });
        owner.first.set(owner.inner.get_segment(0)?);
        Ok(owner)
    }
}
impl<
        S: ReaderSegments,
        B: Borrow<ReadBudget>,
        O: core::ops::Deref<Target = ReaderArenaImpl<S, B>>,
    > CachedReaderArena<O>
{
    // Keep non-cached segment lookup and its error storage out of the vtable's
    // common segment-zero bounds check. The same arena validates other segments.
    #[inline(never)]
    unsafe fn check_other_offset_and_read(
        &self,
        id: u32,
        start: *const u8,
        offset: i32,
        words: usize,
    ) -> Result<*const u8> {
        // SAFETY: forward the caller's live pointer and offset unchanged to its
        // original arena, which validates the full target before forming it.
        unsafe { self.inner.check_offset_and_read(id, start, offset, words) }
    }
}
// SAFETY: private constructors cache first only after borrowing the arena or
// placing the complete cache in an Rc. Neither constructor exposes mutation
// or moves its segment owner afterward. ReaderSegments keeps returned storage live until that owner drops.
// Other segments and the traversal budget belong to the same retained arena.
unsafe impl<
        S: ReaderSegments,
        B: Borrow<ReadBudget>,
        O: core::ops::Deref<Target = ReaderArenaImpl<S, B>>,
    > ReaderArena for CachedReaderArena<O>
{
    #[inline(always)]
    fn get_segment(&self, id: u32) -> Result<(*const u8, u32)> {
        if id == 0 {
            Ok(self.first.get())
        } else {
            self.inner.get_segment(id)
        }
    }
    fn contains_interval(&self, id: u32, start: *const u8, words: usize) -> Result<()> {
        let (base, length) = self.get_segment(id)?;
        let offset = (start as usize)
            .checked_sub(base as usize)
            .ok_or_else(|| Error::from_kind(ErrorKind::MessageContainsOutOfBoundsPointer))?;
        let bytes = length as usize * BYTES_PER_WORD;
        if offset > bytes || words > (bytes - offset) / BYTES_PER_WORD {
            return Err(Error::from_kind(
                ErrorKind::MessageContainsOutOfBoundsPointer,
            ));
        }
        self.inner.read_limiter.borrow().limiter.can_read(words)
    }
    #[inline(always)]
    unsafe fn check_offset_and_read(
        &self,
        id: u32,
        start: *const u8,
        offset: i32,
        words: usize,
    ) -> Result<*const u8> {
        if id != 0 {
            // SAFETY: only the lookup strategy changes; the original arena
            // receives the same live pointer, signed offset and requested extent.
            return unsafe { self.check_other_offset_and_read(id, start, offset, words) };
        }
        let (base, length) = self.first.get();
        let invalid = || Error::from_kind(ErrorKind::MessageContainsOutOfBoundsPointer);
        let relative = (start as usize)
            .checked_sub(base as usize)
            .ok_or_else(invalid)?;
        let bytes = length as usize * BYTES_PER_WORD;
        let target = (relative as u64).wrapping_add_signed(i64::from(offset) * 8);
        if target > bytes as u64 {
            return Err(invalid());
        }
        let target = usize::try_from(target).unwrap();
        if words > (bytes - target) / BYTES_PER_WORD {
            return Err(invalid());
        }
        self.inner.read_limiter.borrow().limiter.can_read(words)?;
        // SAFETY: the cached/delegated segment remains live, and the complete
        // target was checked before forming this pointer (including on 32-bit).
        Ok(unsafe { base.add(target) })
    }
    #[inline(always)]
    unsafe fn validate_capability_indices(
        &self,
        pointer: super::layout::PointerReader<'_>,
        count: u32,
    ) -> Result<()> {
        // SAFETY: the caller guarantees pointer and concrete arena have identical
        // identity, so inlining changes lookup cost, not memory or budget checks.
        unsafe { pointer.validate_capability_indices_in(self, count) }
    }
    fn amplified_read(&self, words: u64) -> Result<()> {
        self.inner.amplified_read(words)
    }
    fn nesting_limit(&self) -> i32 {
        self.inner.nesting_limit()
    }
    fn size_in_words(&self) -> usize {
        self.inner.size_in_words()
    }
}

unsafe impl<S, B> ReaderArena for ReaderArenaImpl<S, B>
where
    S: ReaderSegments,
    B: Borrow<ReadBudget>,
{
    #[inline(always)]
    fn get_segment(&self, id: u32) -> Result<(*const u8, u32)> {
        match self.segments.get_segment(id) {
            Some(seg) => {
                #[cfg(not(feature = "unaligned"))]
                {
                    if !(seg.as_ptr() as usize).is_multiple_of(BYTES_PER_WORD) {
                        return Err(Error::from_kind(ErrorKind::UnalignedSegment));
                    }
                }

                Ok((
                    seg.as_ptr(),
                    u32::try_from(seg.len() / BYTES_PER_WORD).unwrap(),
                ))
            }
            None => Err(Error::from_kind(ErrorKind::InvalidSegmentId(id))),
        }
    }

    fn contains_interval(&self, id: u32, start: *const u8, size_in_words: usize) -> Result<()> {
        let (base, words) = self.get_segment(id)?;
        let offset = (start as usize)
            .checked_sub(base as usize)
            .ok_or_else(|| Error::from_kind(ErrorKind::MessageContainsOutOfBoundsPointer))?;
        // Divide the remaining extent instead of multiplying an untrusted word
        // count or adding it to an address. Rejection must not overflow even
        // when the caller supplies usize::MAX on a 32-bit target.
        let bytes = words as usize * BYTES_PER_WORD;
        if offset > bytes || size_in_words > (bytes - offset) / BYTES_PER_WORD {
            return Err(Error::from_kind(
                ErrorKind::MessageContainsOutOfBoundsPointer,
            ));
        }
        self.read_limiter.borrow().limiter.can_read(size_in_words)
    }

    #[inline(always)]
    unsafe fn check_offset_and_read(
        &self,
        id: u32,
        start: *const u8,
        offset_in_words: i32,
        size_in_words: usize,
    ) -> Result<*const u8> {
        let (base, words) = self.get_segment(id)?;
        let invalid = || Error::from_kind(ErrorKind::MessageContainsOutOfBoundsPointer);
        let relative = (start as usize)
            .checked_sub(base as usize)
            .ok_or_else(invalid)?;
        let segment_bytes = words as usize * BYTES_PER_WORD;
        // The caller supplies a position in this live allocation. Its length
        // is at most isize::MAX; a signed 32-bit word offset cannot overflow
        // the positive u64 range. Negative targets wrap above every possible
        // allocation length and are rejected by the same comparison. Widen
        // before multiplying so 32-bit targets reject, rather than truncate,
        // a malicious offset larger than their address space.
        let target = (relative as u64).wrapping_add_signed(i64::from(offset_in_words) * 8);
        if target > segment_bytes as u64 {
            return Err(invalid());
        }
        let target = usize::try_from(target).unwrap();
        if size_in_words > (segment_bytes - target) / BYTES_PER_WORD {
            return Err(invalid());
        }
        self.read_limiter.borrow().limiter.can_read(size_in_words)?;
        // SAFETY: get_segment supplies a live initialized allocation; the
        // complete target was checked above before forming this pointer. Zero
        // sized targets may refer to its one-past-end address.
        Ok(unsafe { base.add(target) })
    }

    #[inline(always)]
    unsafe fn validate_capability_indices(
        &self,
        pointer: super::layout::PointerReader<'_>,
        count: u32,
    ) -> Result<()> {
        // SAFETY: the caller guarantees pointer and concrete arena have identical
        // identity, so inlining changes lookup cost, not memory or budget checks.
        unsafe { pointer.validate_capability_indices_in(self, count) }
    }
    fn amplified_read(&self, virtual_amount: u64) -> Result<()> {
        self.read_limiter
            .borrow()
            .limiter
            .can_read(usize::try_from(virtual_amount).unwrap())
    }

    fn nesting_limit(&self) -> i32 {
        self.nesting_limit
    }

    fn size_in_words(&self) -> usize {
        let mut result = 0;
        for ii in 0..u32::try_from(self.segments.len()).unwrap() {
            if let Some(seg) = self.segments.get_segment(ii) {
                result += seg.len() / BYTES_PER_WORD;
            }
        }
        result
    }
}

/// Allocation and mutable access for a message under construction.
///
/// # Safety
/// In addition to ReaderArena's lifetime and bounds guarantees, allocations
/// must be disjoint, initialized to zero, and remain stable until arena drop.
/// Mutable segment pointers must refer to writable storage; external read-only
/// segments must report `is_writable == false`. A successful resize must preserve
/// existing words, initialize added words and retain disjoint allocation ranges.
/// Implementations must not permit shared readers to race with mutation.
pub unsafe trait BuilderArena: ReaderArena {
    /// Whether copying may invoke allocating capability hooks. Fixed arenas
    /// reject capabilities before extracting a client hook, even with `alloc`.
    fn allows_capabilities(&self) -> bool {
        true
    }

    /// Optional monotonic allocation checkpoint for an unpublished subtree.
    fn allocation_checkpoint(&self) -> Option<u32> {
        None
    }

    /// Discard reservations after a checkpoint returned by this same arena.
    /// Implementations must preserve earlier bytes and zero discarded storage.
    ///
    /// # Safety
    /// No later reservation may be published or borrowed. Its owned resources
    /// must already be released, and no other rewind may invalidate the token.
    unsafe fn rewind_allocations(&mut self, _checkpoint: u32) {
        unreachable!("arena does not support allocation checkpoints")
    }

    /// On failure, leave both allocated storage and its contents unchanged.
    fn allocate(&mut self, segment_id: u32, amount: WordCount32) -> Option<u32>;
    /// Allocate initialized words and return their address. The default retains
    /// compatibility with custom arenas; an arena can combine allocation and
    /// segment lookup. Zero words may return the segment's one-past-end pointer.
    fn allocate_ptr(&mut self, segment_id: u32, amount: WordCount32) -> Option<*mut u8> {
        let index = self.allocate(segment_id, amount)?;
        let (base, _) = self.get_segment_mut(segment_id);
        Some(base.wrapping_add(index as usize * BYTES_PER_WORD))
    }
    fn allocate_anywhere(&mut self, amount: u32) -> (SegmentId, u32);
    /// Fallible growth boundary. Fixed arenas override this without calling an
    /// infallible allocator. Rejection must preserve existing allocations.
    /// The default retains the allocation policy of legacy/heap-backed arenas.
    fn try_allocate_anywhere(&mut self, amount: u32) -> Result<(SegmentId, u32)> {
        Ok(self.allocate_anywhere(amount))
    }
    fn get_segment_mut(&mut self, id: u32) -> (*mut u8, u32);

    fn as_reader(&self) -> &dyn ReaderArena;

    fn is_writable(&self, _id: u32) -> bool {
        true
    }

    /// Resize an allocation at the segment's tail. The caller has cleared any
    /// bytes being released. Newly claimed words were zeroed by the allocator
    /// or when their previous allocation was shrunk.
    fn try_resize_allocation(&mut self, _id: u32, _start: u32, _old: u32, _new: u32) -> bool {
        false
    }

    #[cfg(feature = "alloc")]
    fn add_external_segment(&mut self, _data: crate::dynamic_orphan::ExternalData) -> Result<u32> {
        Err(Error::failed("arena does not support external data".into()))
    }
}

/// A wrapper around a memory segment used in building a message.
struct BuilderSegment {
    #[cfg(feature = "alloc")]
    external: Option<crate::dynamic_orphan::ExternalData>,
    /// Pointer to the start of the segment.
    ptr: *mut u8,

    /// Total number of words the segment could potentially use. That is, all
    /// bytes from `ptr` to `ptr + (capacity * 8)` may be used in the segment.
    capacity: u32,

    /// Number of words already used in the segment.
    allocated: u32,
}

impl BuilderSegment {
    fn allocate(&mut self, amount: WordCount32) -> Option<u32> {
        #[cfg(feature = "alloc")]
        if self.external.is_some() {
            return None;
        }
        if amount > self.capacity - self.allocated {
            return None;
        }
        let index = self.allocated;
        self.allocated += amount;
        Some(index)
    }
}

#[derive(Default)]
struct BuilderSegmentArray {
    // Like C++ BuilderArena::segment0, the common segment's bookkeeping is
    // inline. Moving this metadata never moves its allocator-owned word buffer.
    segment: Option<BuilderSegment>,
    #[cfg(feature = "alloc")]
    additional: alloc::vec::Vec<BuilderSegment>,
}

impl BuilderSegmentArray {
    #[cfg(not(feature = "alloc"))]
    fn iter(&self) -> core::option::Iter<'_, BuilderSegment> {
        self.segment.iter()
    }

    #[cfg(feature = "alloc")]
    fn iter(&self) -> impl Iterator<Item = &BuilderSegment> {
        self.segment.iter().chain(self.additional.iter())
    }

    fn len(&self) -> usize {
        let first = usize::from(self.segment.is_some());
        #[cfg(feature = "alloc")]
        {
            first + self.additional.len()
        }
        #[cfg(not(feature = "alloc"))]
        {
            first
        }
    }

    fn push(&mut self, segment: BuilderSegment) {
        if self.segment.is_some() {
            #[cfg(feature = "alloc")]
            {
                self.additional.push(segment);
                return;
            }
            #[cfg(not(feature = "alloc"))]
            panic!("multiple segments are not supported in no-alloc mode")
        }
        self.segment = Some(segment);
    }
}

impl core::ops::Index<usize> for BuilderSegmentArray {
    type Output = BuilderSegment;

    fn index(&self, index: usize) -> &Self::Output {
        #[cfg(feature = "alloc")]
        if index > 0 {
            return &self.additional[index - 1];
        }
        assert_eq!(index, 0);
        match &self.segment {
            Some(s) => s,
            None => panic!("no segment"),
        }
    }
}

impl core::ops::IndexMut<usize> for BuilderSegmentArray {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        #[cfg(feature = "alloc")]
        if index > 0 {
            return &mut self.additional[index - 1];
        }
        assert_eq!(index, 0);
        match &mut self.segment {
            Some(s) => s,
            None => panic!("no segment"),
        }
    }
}

pub struct BuilderArenaImplInner<A>
where
    A: Allocator,
{
    allocator: Option<A>, // None if has already been deallocated.
    segments: BuilderSegmentArray,
}

pub struct BuilderArenaImpl<A>
where
    A: Allocator,
{
    inner: BuilderArenaImplInner<A>,
}

// BuilderArenaImpl has no interior mutability. Adding these impls
// allows message::Builder<A> to be Send and/or Sync when appropriate.
unsafe impl<A> Send for BuilderArenaImpl<A> where A: Send + Allocator {}
unsafe impl<A> Sync for BuilderArenaImpl<A> where A: Sync + Allocator {}

impl<A> BuilderArenaImpl<A>
where
    A: Allocator,
{
    pub fn retained_segment_bytes(&self) -> Option<usize> {
        self.inner
            .segments
            .iter()
            .try_fold(0usize, |bytes, segment| {
                #[cfg(feature = "alloc")]
                if segment.external.is_some() {
                    // An external range does not describe its owner's full allocation.
                    return None;
                }
                (segment.capacity as usize)
                    .checked_mul(8)?
                    .checked_add(bytes)
            })
    }
    pub fn new(allocator: A) -> Self {
        Self {
            inner: BuilderArenaImplInner {
                allocator: Some(allocator),
                segments: Default::default(),
            },
        }
    }

    /// Allocates a new segment with capacity for at least `minimum_size` words.
    pub fn allocate_segment(&mut self, minimum_size: u32) -> Result<()> {
        self.inner.allocate_segment(minimum_size)
    }

    pub fn get_segments_for_output(&self) -> OutputSegments<'_> {
        let reff = &self.inner;
        if reff.segments.len() == 1 {
            let seg = &reff.segments[0];

            // The user must mutably borrow the `message::Builder` to be able to modify segment memory.
            // No such borrow will be possible while `self` is still immutably borrowed from this method,
            // so returning this slice is safe.
            let slice = unsafe {
                slice::from_raw_parts(seg.ptr as *const _, seg.allocated as usize * BYTES_PER_WORD)
            };
            OutputSegments::SingleSegment([slice])
        } else {
            #[cfg(feature = "alloc")]
            {
                let mut v = alloc::vec::Vec::with_capacity(reff.segments.len());
                for seg in reff.segments.iter() {
                    // See safety argument in above branch.
                    let slice = unsafe {
                        slice::from_raw_parts(
                            seg.ptr as *const _,
                            seg.allocated as usize * BYTES_PER_WORD,
                        )
                    };
                    v.push(slice);
                }
                OutputSegments::MultiSegment(v)
            }
            #[cfg(not(feature = "alloc"))]
            {
                panic!("invalid number of segments: {}", reff.segments.len());
            }
        }
    }

    pub fn len(&self) -> usize {
        self.inner.segments.len()
    }

    pub fn segment_for_output(&self, id: u32) -> Option<&[u8]> {
        if id as usize >= self.len() {
            return None;
        }
        let segment = &self.inner.segments[id as usize];
        // SAFETY: the allocator owns this initialized prefix. Borrowing the
        // arena immutably prevents mutation or deallocation for the slice's life.
        Some(unsafe {
            slice::from_raw_parts(segment.ptr, segment.allocated as usize * BYTES_PER_WORD)
        })
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Retrieves the underlying `Allocator`, deallocating all currently-allocated
    /// segments.
    pub fn into_allocator(mut self) -> A {
        self.inner.deallocate_all();
        self.inner.allocator.take().unwrap()
    }
}

unsafe impl<A> ReaderArena for BuilderArenaImpl<A>
where
    A: Allocator,
{
    fn get_segment(&self, id: u32) -> Result<(*const u8, u32)> {
        let seg = &self.inner.segments[id as usize];
        Ok((seg.ptr, seg.allocated))
    }

    unsafe fn check_offset(
        &self,
        _segment_id: u32,
        start: *const u8,
        offset_in_words: i32,
    ) -> Result<*const u8> {
        unsafe {
            Ok(start.offset(
                isize::try_from(
                    i64::from(offset_in_words) * i64::try_from(BYTES_PER_WORD).unwrap(),
                )
                .unwrap(),
            ))
        }
    }

    fn contains_interval(&self, _id: u32, _start: *const u8, _size: usize) -> Result<()> {
        Ok(())
    }

    fn amplified_read(&self, _virtual_amount: u64) -> Result<()> {
        Ok(())
    }

    fn nesting_limit(&self) -> i32 {
        0x7fffffff
    }

    fn size_in_words(&self) -> usize {
        let mut result = 0;
        for segment in self.inner.segments.iter() {
            result += segment.allocated as usize
        }
        result
    }
}

impl<A> BuilderArenaImplInner<A>
where
    A: Allocator,
{
    /// Allocates a new segment with capacity for at least `minimum_size` words.
    fn allocate_segment(&mut self, minimum_size: WordCount32) -> Result<()> {
        let seg = match &mut self.allocator {
            Some(a) => a.allocate_segment(minimum_size),
            None => unreachable!(),
        };
        self.segments.push(BuilderSegment {
            #[cfg(feature = "alloc")]
            external: None,
            ptr: seg.0,
            capacity: seg.1,
            allocated: 0,
        });
        Ok(())
    }

    fn allocate(&mut self, segment_id: u32, amount: WordCount32) -> Option<u32> {
        self.segments[segment_id as usize].allocate(amount)
    }

    fn allocate_anywhere(&mut self, amount: u32) -> (SegmentId, u32) {
        // first try the existing segments, then try allocating a new segment.
        let allocated_len = u32::try_from(self.segments.len()).unwrap();
        for segment_id in 0..allocated_len {
            if let Some(idx) = self.allocate(segment_id, amount) {
                return (segment_id, idx);
            }
        }

        // Need to allocate a new segment.

        self.allocate_segment(amount).expect("allocate new segment");
        (
            allocated_len,
            self.allocate(allocated_len, amount)
                .expect("use freshly-allocated segment"),
        )
    }

    fn deallocate_all(&mut self) {
        if let Some(a) = &mut self.allocator {
            #[cfg(feature = "alloc")]
            for seg in self.segments.iter() {
                if seg.external.is_some() {
                    continue;
                }
                unsafe {
                    a.deallocate_segment(seg.ptr, seg.capacity, seg.allocated);
                }
            }

            #[cfg(not(feature = "alloc"))]
            if let Some(seg) = &self.segments.segment {
                unsafe {
                    a.deallocate_segment(seg.ptr, seg.capacity, seg.allocated);
                }
            }
        }
    }

    fn get_segment_mut(&mut self, id: u32) -> (*mut u8, u32) {
        let seg = &self.segments[id as usize];
        #[cfg(feature = "alloc")]
        assert!(seg.external.is_none(), "external segment is read-only");
        (seg.ptr, seg.capacity)
    }
}

unsafe impl<A> BuilderArena for BuilderArenaImpl<A>
where
    A: Allocator,
{
    fn allocate(&mut self, segment_id: u32, amount: WordCount32) -> Option<u32> {
        self.inner.allocate(segment_id, amount)
    }

    fn allocate_ptr(&mut self, segment_id: u32, amount: WordCount32) -> Option<*mut u8> {
        let segment = &mut self.inner.segments[segment_id as usize];
        let index = segment.allocate(amount)?;
        Some(segment.ptr.wrapping_add(index as usize * BYTES_PER_WORD))
    }

    fn allocate_anywhere(&mut self, amount: u32) -> (SegmentId, u32) {
        self.inner.allocate_anywhere(amount)
    }

    fn get_segment_mut(&mut self, id: u32) -> (*mut u8, u32) {
        self.inner.get_segment_mut(id)
    }

    fn as_reader(&self) -> &dyn ReaderArena {
        self
    }

    fn is_writable(&self, id: u32) -> bool {
        #[cfg(feature = "alloc")]
        {
            self.inner.segments[id as usize].external.is_none()
        }
        #[cfg(not(feature = "alloc"))]
        {
            let _ = id;
            true
        }
    }

    fn try_resize_allocation(&mut self, id: u32, start: u32, old: u32, new: u32) -> bool {
        if !self.is_writable(id) {
            return false;
        }
        let segment = &mut self.inner.segments[id as usize];
        if start.checked_add(old) != Some(segment.allocated) {
            return false;
        }
        let Some(end) = start.checked_add(new).filter(|n| *n <= segment.capacity) else {
            return false;
        };
        segment.allocated = end;
        true
    }

    #[cfg(feature = "alloc")]
    fn add_external_segment(&mut self, data: crate::dynamic_orphan::ExternalData) -> Result<u32> {
        let id = u32::try_from(self.inner.segments.len())
            .map_err(|_| Error::failed("too many message segments".into()))?;
        let (pointer, _, words) = data.parts();
        self.inner.segments.push(BuilderSegment {
            ptr: pointer.cast_mut(),
            capacity: words,
            allocated: words,
            external: Some(data),
        });
        Ok(id)
    }
}

impl<A> Drop for BuilderArenaImplInner<A>
where
    A: Allocator,
{
    fn drop(&mut self) {
        self.deallocate_all()
    }
}

pub struct NullArena;

unsafe impl ReaderArena for NullArena {
    fn get_segment(&self, _id: u32) -> Result<(*const u8, u32)> {
        Err(Error::from_kind(ErrorKind::TriedToReadFromNullArena))
    }

    unsafe fn check_offset(
        &self,
        _segment_id: u32,
        start: *const u8,
        offset_in_words: i32,
    ) -> Result<*const u8> {
        let offset_in_bytes = (offset_in_words as i64) * i64::try_from(BYTES_PER_WORD).unwrap();
        unsafe { Ok(start.offset(isize::try_from(offset_in_bytes).unwrap())) }
    }

    fn contains_interval(&self, _id: u32, _start: *const u8, _size: usize) -> Result<()> {
        Ok(())
    }

    fn amplified_read(&self, _virtual_amount: u64) -> Result<()> {
        Ok(())
    }

    fn nesting_limit(&self) -> i32 {
        0x7fffffff
    }

    fn size_in_words(&self) -> usize {
        0
    }
}

/// An arena designed for the specific case of reading messages from single-segment
/// `Word` arrays in generated code, including constants and raw schema nodes. Performs
/// bounds checking, so its constructor does not need to be marked `unsafe`. Does
/// *not* enforce a read limit or a nesting limit.
pub struct GeneratedCodeArena {
    words: &'static [crate::Word],
}

impl GeneratedCodeArena {
    pub const fn new(words: &'static [crate::Word]) -> Self {
        assert!((words.len() as u64) < u32::MAX as u64);
        Self { words }
    }
}

unsafe impl ReaderArena for GeneratedCodeArena {
    fn get_segment(&self, id: u32) -> Result<(*const u8, u32)> {
        if id == 0 {
            Ok((
                self.words.as_ptr() as *const _,
                u32::try_from(self.words.len()).unwrap(),
            ))
        } else {
            Err(Error::from_kind(ErrorKind::InvalidSegmentId(id)))
        }
    }

    fn contains_interval(&self, id: u32, start: *const u8, size_in_words: usize) -> Result<()> {
        let (segment_start, segment_len) = self.get_segment(id)?;
        let offset = (start as usize)
            .checked_sub(segment_start as usize)
            .ok_or_else(|| Error::from_kind(ErrorKind::MessageContainsOutOfBoundsPointer))?;
        let bytes = segment_len as usize * BYTES_PER_WORD;
        // The requested extent is untrusted even for generated schema data.
        // Avoid overflowing either its word-to-byte conversion or end address.
        if offset > bytes || size_in_words > (bytes - offset) / BYTES_PER_WORD {
            Err(Error::from_kind(
                ErrorKind::MessageContainsOutOfBoundsPointer,
            ))
        } else {
            Ok(())
        }
    }

    fn amplified_read(&self, _virtual_amount: u64) -> Result<()> {
        Ok(())
    }

    fn nesting_limit(&self) -> i32 {
        0x7fffffff
    }

    fn size_in_words(&self) -> usize {
        self.words.len()
    }
}
