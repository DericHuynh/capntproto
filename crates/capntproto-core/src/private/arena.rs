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
use crate::message::ReaderSegments;
use crate::private::read_limiter::ReadLimiter;
use crate::private::units::*;
use crate::OutputSegments;
use crate::{Error, ErrorKind, Result};

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
        if target_idx < 0 || usize::try_from(target_idx).unwrap() > this_size {
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

    fn amplified_read(&self, virtual_amount: u64) -> Result<()>;

    fn nesting_limit(&self) -> i32;

    fn size_in_words(&self) -> usize;

    // TODO(apibump): Consider putting extract_cap(), inject_cap(), drop_cap() here
    //   and on message::Reader. Then we could get rid of Imbue and ImbueMut, and
    //   layout::StructReader, layout::ListReader, etc. could drop their `cap_table` fields.
}

pub struct ReaderArenaImpl<S> {
    segments: S,
    read_limiter: ReadLimiter,
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
        let limiter = ReadLimiter::new(options.traversal_limit_in_words);
        Self {
            segments,
            read_limiter: limiter,
            nesting_limit: options.nesting_limit,
        }
    }

    pub fn into_segments(self) -> S {
        self.segments
    }

    pub(crate) fn get_segments(&self) -> &S {
        &self.segments
    }
}

unsafe impl<S> ReaderArena for ReaderArenaImpl<S>
where
    S: ReaderSegments,
{
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
        let (segment_start, segment_len) = self.get_segment(id)?;
        let this_start: usize = segment_start as usize;
        let this_size: usize = segment_len as usize * BYTES_PER_WORD;
        let start = start as usize;
        let size = size_in_words * BYTES_PER_WORD;

        if !(start >= this_start && start - this_start + size <= this_size) {
            Err(Error::from_kind(
                ErrorKind::MessageContainsOutOfBoundsPointer,
            ))
        } else {
            self.read_limiter.can_read(size_in_words)
        }
    }

    unsafe fn check_offset_and_read(
        &self,
        id: u32,
        start: *const u8,
        offset_in_words: i32,
        size_in_words: usize,
    ) -> Result<*const u8> {
        let (base, words) = self.get_segment(id)?;
        let offset = i64::from(offset_in_words) * i64::try_from(BYTES_PER_WORD).unwrap();
        let invalid = || Error::from_kind(ErrorKind::MessageContainsOutOfBoundsPointer);
        let relative = (start as usize)
            .checked_sub(base as usize)
            .ok_or_else(invalid)?;
        let target = i64::try_from(relative)
            .map_err(|_| invalid())?
            .checked_add(offset)
            .ok_or_else(invalid)?;
        let target = usize::try_from(target).map_err(|_| invalid())?;
        let bytes = size_in_words
            .checked_mul(BYTES_PER_WORD)
            .ok_or_else(invalid)?;
        let segment_bytes = words as usize * BYTES_PER_WORD;
        if target > segment_bytes || bytes > segment_bytes - target {
            return Err(invalid());
        }
        self.read_limiter.can_read(size_in_words)?;
        // SAFETY: get_segment supplies a live initialized allocation; the
        // complete target was checked above before forming this pointer. Zero
        // sized targets may refer to its one-past-end address.
        Ok(unsafe { base.add(target) })
    }

    fn amplified_read(&self, virtual_amount: u64) -> Result<()> {
        self.read_limiter
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
        let this_start: usize = segment_start as usize;
        let this_size: usize = segment_len as usize * BYTES_PER_WORD;
        let start = start as usize;
        let size = size_in_words * BYTES_PER_WORD;

        if !(start >= this_start && start - this_start + size <= this_size) {
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
