// Copyright (c) 2026 ReProto contributors. Licensed under the MIT License.

//! Buffered stream framing with explicit short-lived message ownership.
use core::{
    pin::Pin,
    task::{ready, Context, Poll},
};
use std::sync::Arc;

use capnp::{
    message::{Reader, ReaderOptions, ReaderSegments},
    Error, ErrorKind, Result, Word,
};
use futures_util::AsyncRead;

const DEFAULT_BUFFER_WORDS: usize = 8192;
// Every legal segment table fits, even when a frame's body needs direct I/O.
const MIN_BUFFER_WORDS: usize = capnp::serialize::SEGMENTS_COUNT_LIMIT / 2;
// Borrowed frames have no escaping allocation owner. Reuse one completed spill
// for the next frame, bounded independently of potentially unlimited readers.
const MAX_CACHED_SPILL_WORDS: usize = 1 << 20;
// A completed moderate borrowed frame can become the next read buffer. This
// avoids a prefix read followed by a direct read for every repeated payload.
const MAX_PROMOTED_BUFFER_WORDS: usize = 16 * 1024;

struct Frame {
    // Like C++'s segment0, the common segment needs no tagged lookup or heap
    // metadata. A second segment starts at the first one's end, so only its
    // end offset is needed for the common root-plus-large-body layout.
    first: (usize, usize),
    additional: AdditionalSegments,
    bytes: usize,
}
enum AdditionalSegments {
    None,
    One(usize),
    Many(Box<[(usize, usize)]>),
}
impl Frame {
    fn range(&self, id: u32) -> Option<(usize, usize)> {
        if id == 0 {
            Some(self.first)
        } else {
            match &self.additional {
                AdditionalSegments::One(end) if id == 1 => Some((self.first.1, *end)),
                AdditionalSegments::Many(ranges) => ranges.get(id as usize - 1).copied(),
                _ => None,
            }
        }
    }

    fn len(&self) -> usize {
        1 + match &self.additional {
            AdditionalSegments::None => 0,
            AdditionalSegments::One(_) => 1,
            AdditionalSegments::Many(ranges) => ranges.len(),
        }
    }

    fn parse(bytes: &[u8], options: ReaderOptions) -> Result<Option<Self>> {
        if bytes.len() < 8 {
            return Ok(None);
        }
        let (count, _) = crate::serialize::parse_segment_table_first(bytes)?;
        let table_bytes = (count / 2 + 1) * 8;
        if bytes.len() < table_bytes {
            return Ok(None);
        }
        // Keep both ordinary control messages and two-segment payloads inline.
        if count <= 2 {
            let first_words = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
            let second_words = if count == 2 {
                u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize
            } else {
                0
            };
            let words = first_words
                .checked_add(second_words)
                .ok_or_else(|| Error::from_kind(ErrorKind::MessageSizeOverflow))?;
            if options
                .traversal_limit_in_words
                .is_some_and(|limit| words > limit)
            {
                return Err(Error::failed(
                    "incoming message exceeds traversal limit".into(),
                ));
            }
            let bytes = words
                .checked_mul(8)
                .and_then(|n| n.checked_add(table_bytes))
                .ok_or_else(|| Error::from_kind(ErrorKind::MessageSizeOverflow))?;
            return Ok(Some(Self {
                // The validated total bounds the first segment as well.
                first: (table_bytes, table_bytes + first_words * 8),
                additional: if count == 2 {
                    AdditionalSegments::One(bytes)
                } else {
                    AdditionalSegments::None
                },
                bytes,
            }));
        }
        let mut end = table_bytes;
        let mut first = (table_bytes, table_bytes);
        let mut ranges = Vec::with_capacity(count - 1);
        for i in 0..count {
            let offset = (i + 1) * 4;
            let words = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            let start = end;
            end = words
                .checked_mul(8)
                .and_then(|n| end.checked_add(n))
                .ok_or_else(|| Error::from_kind(ErrorKind::MessageSizeOverflow))?;
            if i == 0 {
                first = (start, end);
            } else {
                ranges.push((start, end));
            }
        }
        if options
            .traversal_limit_in_words
            .is_some_and(|limit| (end - table_bytes) / 8 > limit)
        {
            return Err(Error::failed(
                "incoming message exceeds traversal limit".into(),
            ));
        }
        Ok(Some(Self {
            first,
            additional: AdditionalSegments::Many(ranges.into_boxed_slice()),
            bytes: end,
        }))
    }
}

/// Aligned segments backed by either the receive buffer or independent storage.
/// A shared-buffer message prevents subsequent reads until it is dropped. It
/// remains valid even if the stream itself is dropped. No unsafe aliasing is used.
pub struct BufferedSegments {
    storage: Storage,
    start: usize,
    frame: Frame,
}
enum Storage {
    Shared(Arc<Vec<Word>>),
    Owned(Box<[Word]>),
}
impl core::ops::Deref for Storage {
    type Target = [Word];
    fn deref(&self) -> &[Word] {
        match self {
            Self::Shared(words) => words,
            Self::Owned(words) => words,
        }
    }
}
impl BufferedSegments {
    /// The complete immutable stream frame, including its original segment
    /// table. It remains valid for exactly the same lifetime as these segments.
    pub fn framed_bytes(&self) -> &[u8] {
        &Word::words_to_bytes(&self.storage)[self.start..self.start + self.frame.bytes]
    }

    /// True when this message holds the stream's reusable receive buffer.
    pub fn is_shared_buffer(&self) -> bool {
        matches!(self.storage, Storage::Shared(_))
    }

    fn detach(mut self) -> Self {
        let mut storage = Word::allocate_zeroed_vec(self.frame.bytes / 8);
        Word::words_to_bytes_mut(&mut storage).copy_from_slice(
            &Word::words_to_bytes(&self.storage)[self.start..self.start + self.frame.bytes],
        );
        // Detached frames have one owner. Only views into the stream's receive
        // buffer need reference counting and a separate Arc allocation.
        self.storage = Storage::Owned(storage.into_boxed_slice());
        self.start = 0;
        self
    }
}
impl ReaderSegments for BufferedSegments {
    #[inline(always)]
    fn get_segment(&self, id: u32) -> Option<&[u8]> {
        let (start, end) = self.frame.range(id)?;
        Some(match &self.storage {
            // Owned frames always start at zero (detach, spill or scratch
            // conversion). Only shared receive views need an additional offset.
            Storage::Owned(words) => &Word::words_to_bytes(words)[start..end],
            Storage::Shared(words) => {
                &Word::words_to_bytes(words)[self.start + start..self.start + end]
            }
        })
    }
    fn len(&self) -> usize {
        self.frame.len()
    }
}

enum ScratchStorage<'a> {
    Buffered(BufferedSegments),
    Borrowed { words: &'a mut [Word], frame: Frame },
}

/// A buffered message using caller words, shared receive storage, or an owned
/// fallback. The caller words stay borrowed until this value is dropped, even
/// when the shared-buffer or owned path was selected.
pub struct BufferedScratchSegments<'a>(ScratchStorage<'a>);
impl BufferedScratchSegments<'_> {
    pub fn uses_scratch(&self) -> bool {
        matches!(self.0, ScratchStorage::Borrowed { .. })
    }

    pub fn is_shared_buffer(&self) -> bool {
        matches!(&self.0, ScratchStorage::Buffered(s) if s.is_shared_buffer())
    }

    /// Release the caller's storage by copying a scratch-backed frame. Shared
    /// and owned fallback frames are transferred without another payload copy.
    pub fn into_owned(self) -> BufferedSegments {
        match self.0 {
            ScratchStorage::Buffered(s) => s,
            ScratchStorage::Borrowed { words, frame } => BufferedSegments {
                storage: Storage::Owned(words.into()),
                start: 0,
                frame,
            },
        }
    }
}
impl ReaderSegments for BufferedScratchSegments<'_> {
    fn get_segment(&self, id: u32) -> Option<&[u8]> {
        match &self.0 {
            ScratchStorage::Buffered(s) => s.get_segment(id),
            ScratchStorage::Borrowed { words, frame } => frame
                .range(id)
                .map(|(start, end)| &Word::words_to_bytes(words)[start..end]),
        }
    }
    fn len(&self) -> usize {
        match &self.0 {
            ScratchStorage::Buffered(s) => s.len(),
            ScratchStorage::Borrowed { frame, .. } => frame.len(),
        }
    }
}

impl BufferedSegments {
    fn retain(self, scratch: Option<&mut [Word]>) -> BufferedScratchSegments<'_> {
        if let Some(words) = scratch.filter(|words| words.len() >= self.frame.bytes / 8) {
            let words = &mut words[..self.frame.bytes / 8];
            Word::words_to_bytes_mut(words).copy_from_slice(
                &Word::words_to_bytes(&self.storage)[self.start..self.start + self.frame.bytes],
            );
            BufferedScratchSegments(ScratchStorage::Borrowed {
                words,
                frame: self.frame,
            })
        } else {
            BufferedScratchSegments(ScratchStorage::Buffered(self.detach()))
        }
    }
}

struct Spill {
    storage: Vec<Word>,
    filled: usize,
    frame: Frame,
}
struct Streaming {
    bytes: usize,
    position: usize,
}

/// Reads multiple frames per underlying read. Completed buffered frames are
/// classified by the caller: short-lived messages share the buffer; others are
/// copied to independent storage. Incomplete frames larger than half the buffer
/// are read directly into independent storage without invoking the classifier.
///
/// The next read rejects a live shared-buffer message, without consuming input.
/// Dropping an unfinished read future preserves partial framing/body progress.
/// EOF is sticky; framing, I/O and classifier errors permanently fail the reader.
/// This byte-stream adapter does not receive ancillary file descriptors.
pub struct BufferedRead<R> {
    input: R,
    options: ReaderOptions,
    buffer: Arc<Vec<Word>>,
    begin: usize,
    end: usize,
    frame: Option<Frame>,
    spill: Option<Spill>,
    spare: Vec<Word>,
    terminal: Option<Result<()>>,
    consumed: u64,
    borrowed: bool,
    streaming: Option<Streaming>,
    read_ahead: fn(&R) -> bool,
}
impl<R: AsyncRead + Unpin> BufferedRead<R> {
    /// Creates a reader with a 64 KiB receive buffer.
    pub fn new(input: R, options: ReaderOptions) -> Self {
        Self::with_buffer_size(input, options, DEFAULT_BUFFER_WORDS).unwrap()
    }

    /// The buffer must contain at least 256 words (2 KiB), enough for every legal
    /// segment table. The configured traversal limit is checked before allocating
    /// independent message storage, and counts payload words, excluding framing.
    /// Borrowed-frame reads may grow this buffer to fit a completed frame up to
    /// 128 KiB; owning reads do not grow it. Larger borrowed frames use a separate
    /// reusable spill allocation, bounded as documented by `finish_borrowed_frame`.
    pub fn with_buffer_size(input: R, options: ReaderOptions, words: usize) -> Result<Self> {
        if words < MIN_BUFFER_WORDS || words.checked_mul(8).is_none() {
            return Err(Error::failed(
                "receive buffer must hold at least 256 words and fit in usize".into(),
            ));
        }
        Ok(Self {
            input,
            options,
            buffer: Arc::new(Word::allocate_zeroed_vec(words)),
            begin: 0,
            end: 0,
            frame: None,
            spill: None,
            spare: Vec::new(),
            terminal: None,
            consumed: 0,
            borrowed: false,
            streaming: None,
            read_ahead: |_| true,
        })
    }

    /// Consulted before each underlying read. Returning false limits reads to
    /// the current frame (or its incomplete segment table). Ancillary transports
    /// use this after receiving descriptors, so another frame cannot spend the
    /// current frame's descriptor budget. Large/direct reads are always bounded.
    pub fn set_read_ahead_policy(&mut self, policy: fn(&R) -> bool) {
        self.read_ahead = policy;
    }

    /// The underlying input, including transport-specific ancillary state.
    pub fn get_ref(&self) -> &R {
        &self.input
    }

    /// Framing plus payload bytes in messages already returned to the caller.
    /// Prefetched bytes for later frames are excluded.
    pub fn consumed_bytes(&self) -> u64 {
        self.consumed
    }

    /// Whether a returned message still owns a view into the receive buffer.
    /// Such a view must be dropped before another read can begin.
    pub fn has_outstanding_short_lived_message(&self) -> bool {
        self.borrowed || self.streaming.is_some() || Arc::strong_count(&self.buffer) != 1
    }

    /// Whether the next complete frame is already buffered, without reading or
    /// transferring its ownership. Errors remain for the ordinary receive path.
    pub fn has_buffered_next_frame(&self) -> bool {
        if self.terminal.is_some() || self.has_outstanding_short_lived_message() {
            return false;
        }
        let bytes = &Word::words_to_bytes(&self.buffer)[self.begin..self.end];
        matches!(Frame::parse(bytes, self.options), Ok(Some(frame)) if frame.bytes <= bytes.len())
    }

    fn advance(&mut self, bytes: usize) -> Result<()> {
        self.consumed = self
            .consumed
            .checked_add(bytes as u64)
            .ok_or_else(|| Error::from_kind(ErrorKind::MessageSizeOverflow))?;
        Ok(())
    }

    pub async fn try_read_message(
        &mut self,
        is_short_lived: impl FnOnce(&Reader<BufferedSegments>) -> Result<bool>,
    ) -> Result<Option<Reader<BufferedSegments>>> {
        let options = self.options;
        self.try_read(None, is_short_lived)
            .await
            .map(|message| message.map(|m| Reader::new(m.into_segments().into_owned(), options)))
    }

    /// Like [`Self::try_read_message`], with reusable storage for retained frames.
    /// Scratch capacity includes the segment table and payload, matching C++
    /// BufferedMessageStream (unlike the standalone async scratch functions).
    /// Fully buffered, non-short-lived frames use scratch when they fit; short
    /// messages still share receive storage. Direct reads of incomplete large or
    /// descriptor-bearing frames still use owned storage, as in pinned C++.
    /// Scratch is untouched on fallback, EOF, error or cancellation; copying
    /// happens only after a complete frame has been classified successfully.
    /// Unused scratch words are never modified. Multiple-segment metadata allocates;
    /// single-segment metadata stays in the returned frame.
    ///
    /// ```compile_fail
    /// # async fn example() -> capnp::Result<()> {
    /// let mut stream = capnp_futures::BufferedRead::new(
    ///     futures::io::Cursor::new(Vec::<u8>::new()), Default::default());
    /// let mut scratch = capnp::Word::allocate_zeroed_vec(256);
    /// let message = stream.try_read_message_with_scratch(&mut scratch, |_| Ok(false)).await?;
    /// drop(scratch); // The returned message can still refer to these words.
    /// drop(message);
    /// # Ok(()) }
    /// ```
    pub async fn try_read_message_with_scratch<'a>(
        &mut self,
        scratch: &'a mut [Word],
        is_short_lived: impl FnOnce(&Reader<BufferedSegments>) -> Result<bool>,
    ) -> Result<Option<Reader<BufferedScratchSegments<'a>>>> {
        self.try_read(Some(scratch), is_short_lived).await
    }

    async fn try_read<'a>(
        &mut self,
        mut scratch: Option<&'a mut [Word]>,
        is_short_lived: impl FnOnce(&Reader<BufferedSegments>) -> Result<bool>,
    ) -> Result<Option<Reader<BufferedScratchSegments<'a>>>> {
        let mut classify = Some(is_short_lived);
        core::future::poll_fn(|cx| self.poll_try_read(cx, &mut scratch, &mut classify)).await
    }

    /// Poll the same framing state machine without allocating a receive future.
    /// The returned message may borrow shared receive storage and must be dropped
    /// before the next read. Pending retains partial progress in this reader.
    pub fn poll_read_short_lived_message(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<Reader<BufferedSegments>>>> {
        self.poll_read_message(cx, |_| Ok(true))
    }

    /// Poll a frame without allocating a receive future. The classifier runs
    /// only after the complete frame is available. Return false for messages
    /// that must remain valid independently of subsequent reads.
    pub fn poll_read_message(
        &mut self,
        cx: &mut Context<'_>,
        is_short_lived: impl FnOnce(&Reader<BufferedSegments>) -> Result<bool>,
    ) -> Poll<Result<Option<Reader<BufferedSegments>>>> {
        let options = self.options;
        self.poll_try_read(cx, &mut None, &mut Some(is_short_lived))
            .map_ok(|message| message.map(|m| Reader::new(m.into_segments().into_owned(), options)))
    }

    fn poll_try_read<'a>(
        &mut self,
        cx: &mut Context<'_>,
        scratch: &mut Option<&'a mut [Word]>,
        classify: &mut Option<impl FnOnce(&Reader<BufferedSegments>) -> Result<bool>>,
    ) -> Poll<Result<Option<Reader<BufferedScratchSegments<'a>>>>> {
        if self.has_outstanding_short_lived_message() {
            return Poll::Ready(Err(Error::failed(
                "previous short-lived message is still alive".into(),
            )));
        }
        if let Some(terminal) = &self.terminal {
            return Poll::Ready(terminal.clone().map(|()| None));
        }
        let result = ready!(self.poll_read(cx, scratch, classify));
        match &result {
            Err(error) => {
                self.spill = None;
                self.frame = None;
                self.terminal = Some(Err(error.clone()));
            }
            Ok(None) => self.terminal = Some(Ok(())),
            Ok(Some(_)) => {}
        }
        Poll::Ready(result)
    }

    fn poll_read<'a>(
        &mut self,
        cx: &mut Context<'_>,
        scratch: &mut Option<&'a mut [Word]>,
        classify: &mut Option<impl FnOnce(&Reader<BufferedSegments>) -> Result<bool>>,
    ) -> Poll<Result<Option<Reader<BufferedScratchSegments<'a>>>>> {
        if !ready!(self.poll_prepare_frame(cx, false, false))? {
            return Poll::Ready(Ok(None));
        }
        if let Some(spill) = self.spill.take() {
            self.advance(spill.frame.bytes)?;
            return Poll::Ready(Ok(Some(Reader::new(
                BufferedScratchSegments(ScratchStorage::Buffered(BufferedSegments {
                    storage: Storage::Owned(spill.storage.into_boxed_slice()),
                    start: 0,
                    frame: spill.frame,
                })),
                self.options,
            ))));
        }
        let frame = self.frame.take().expect("prepared buffered frame");
        let bytes = frame.bytes;
        let view = Reader::new(
            BufferedSegments {
                storage: Storage::Shared(self.buffer.clone()),
                start: self.begin,
                frame,
            },
            self.options,
        );
        let short_lived = classify.take().expect("classify one complete frame")(&view)?;
        self.advance(bytes)?;
        let segments = view.into_segments();
        let segments = if short_lived {
            BufferedScratchSegments(ScratchStorage::Buffered(segments))
        } else {
            segments.retain(scratch.take())
        };
        self.consume_buffered(bytes);
        // Classification uses its own traversal budget.
        Poll::Ready(Ok(Some(Reader::new(segments, self.options))))
    }

    fn consume_buffered(&mut self, bytes: usize) {
        self.begin += bytes;
        if self.begin == self.end {
            self.begin = 0;
            self.end = 0;
        }
    }

    /// Receive an immutable frame retained inside this stream until
    /// `finish_borrowed_frame`. Repeated polls return the same bytes without IO.
    /// Pending preserves partial input; errors and EOF remain sticky. Other
    /// receive APIs reject an outstanding borrowed frame without consuming it.
    pub fn poll_borrowed_frame(&mut self, cx: &mut Context<'_>) -> Poll<Result<Option<&[u8]>>> {
        if !self.borrowed {
            if self.has_outstanding_short_lived_message() {
                return Poll::Ready(Err(Error::failed(
                    "previous short-lived message is still alive".into(),
                )));
            }
            if let Some(terminal) = &self.terminal {
                return Poll::Ready(terminal.clone().map(|()| None));
            }
            let result = ready!(self.poll_prepare_frame(cx, true, false)).and_then(|ready| {
                if ready {
                    let bytes = self.spill.as_ref().map_or_else(
                        || self.frame.as_ref().expect("prepared frame").bytes,
                        |spill| spill.frame.bytes,
                    );
                    self.advance(bytes)?;
                }
                Ok(ready)
            });
            match result {
                Ok(true) => self.borrowed = true,
                Ok(false) => {
                    self.terminal = Some(Ok(()));
                    return Poll::Ready(Ok(None));
                }
                Err(error) => {
                    self.spill = None;
                    self.frame = None;
                    self.terminal = Some(Err(error.clone()));
                    return Poll::Ready(Err(error));
                }
            }
        }
        Poll::Ready(Ok(self.borrowed_frame()))
    }

    /// View the last successfully borrowed frame without consuming input.
    pub fn borrowed_frame(&self) -> Option<&[u8]> {
        if !self.borrowed {
            return None;
        }
        Some(match &self.spill {
            Some(spill) => &Word::words_to_bytes(&spill.storage)[..spill.frame.bytes],
            None => {
                let bytes = self.frame.as_ref().expect("borrowed frame").bytes;
                &Word::words_to_bytes(&self.buffer)[self.begin..self.begin + bytes]
            }
        })
    }

    /// Prepare one frame for caller-owned storage. Complete small frames remain
    /// available through `borrowed_frame`; an incomplete frame above
    /// 128 KiB exposes only its validated length, then `poll_stream_frame` copies
    /// its buffered prefix and reads the body directly into the caller's buffer.
    /// No spill allocation is needed. Finish before preparing another frame.
    pub fn poll_stream_frame_size(&mut self, cx: &mut Context<'_>) -> Poll<Result<Option<usize>>> {
        if let Some(Err(error)) = &self.terminal {
            return Poll::Ready(Err(error.clone()));
        }
        if let Some(streaming) = &self.streaming {
            return Poll::Ready(Ok(Some(streaming.bytes)));
        }
        if self.borrowed {
            return Poll::Ready(Ok(self.borrowed_frame().map(<[u8]>::len)));
        }
        if self.has_outstanding_short_lived_message() {
            return Poll::Ready(Err(Error::failed(
                "previous short-lived message is still alive".into(),
            )));
        }
        if let Some(terminal) = &self.terminal {
            return Poll::Ready(terminal.clone().map(|()| None));
        }
        let result = ready!(self.poll_prepare_frame(cx, true, true));
        match result {
            Ok(true) => {
                let bytes = self.spill.as_ref().map_or_else(
                    || self.frame.as_ref().expect("prepared frame").bytes,
                    |spill| spill.frame.bytes,
                );
                if self.spill.is_none() && self.end - self.begin < bytes {
                    self.streaming = Some(Streaming { bytes, position: 0 });
                } else {
                    if let Err(error) = self.advance(bytes) {
                        self.terminal = Some(Err(error.clone()));
                        return Poll::Ready(Err(error));
                    }
                    self.borrowed = true;
                }
                Poll::Ready(Ok(Some(bytes)))
            }
            Ok(false) => {
                self.terminal = Some(Ok(()));
                Poll::Ready(Ok(None))
            }
            Err(error) => {
                self.spill = None;
                self.frame = None;
                self.terminal = Some(Err(error.clone()));
                Poll::Ready(Err(error))
            }
        }
    }

    /// Continue an incomplete frame prepared above, retaining progress across
    /// Pending. Reads never cross its boundary. The caller retains all returned
    /// bytes until the complete frame has been validated and admitted.
    pub fn poll_stream_frame(
        &mut self,
        cx: &mut Context<'_>,
        output: &mut [u8],
    ) -> Poll<Result<usize>> {
        if let Some(Err(error)) = &self.terminal {
            return Poll::Ready(Err(error.clone()));
        }
        let Some(streaming) = &mut self.streaming else {
            return Poll::Ready(Err(Error::failed("no streaming frame prepared".into())));
        };
        let remaining = streaming.bytes - streaming.position;
        let length = remaining.min(output.len());
        let output = &mut output[..length];
        if output.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let prefix = self.end - self.begin;
        let count = if streaming.position < prefix {
            let count = output.len().min(prefix - streaming.position);
            let start = self.begin + streaming.position;
            output[..count]
                .copy_from_slice(&Word::words_to_bytes(&self.buffer)[start..start + count]);
            count
        } else {
            match ready!(Pin::new(&mut self.input).poll_read(cx, output)) {
                Ok(0) => {
                    let error = Error::from_kind(ErrorKind::PrematureEndOfFile);
                    self.terminal = Some(Err(error.clone()));
                    return Poll::Ready(Err(error));
                }
                Ok(count) => count,
                Err(error) => {
                    let error = Error::from(error);
                    self.terminal = Some(Err(error.clone()));
                    return Poll::Ready(Err(error));
                }
            }
        };
        streaming.position += count;
        if streaming.position == streaming.bytes {
            let bytes = streaming.bytes;
            if let Err(error) = self.advance(bytes) {
                self.terminal = Some(Err(error.clone()));
                return Poll::Ready(Err(error));
            }
        }
        Poll::Ready(Ok(count))
    }

    /// Release a borrowed or streamed frame. Abandoning a prepared stream before
    /// its body has been consumed is terminal; an unread suffix cannot become a
    /// new header. Calling this before either API has prepared a frame leaves
    /// partial input untouched, so canceled preparation can resume. A completed spill
    /// of at most 128 KiB can replace a smaller receive buffer; another allocation
    /// of at most 8 MiB is retained for reuse until the stream is dropped. Its
    /// contents cannot be observed until a subsequent complete read overwrites
    /// the entire exposed frame; owning receive APIs still transfer their storage.
    pub fn finish_borrowed_frame(&mut self) {
        if let Some(streaming) = self.streaming.take() {
            if streaming.position != streaming.bytes {
                self.terminal = Some(Err(Error::failed(
                    "incomplete streaming frame abandoned".into(),
                )));
            }
            self.frame = None;
            self.consume_buffered(self.end - self.begin);
        }
        if core::mem::take(&mut self.borrowed) {
            if let Some(mut spill) = self.spill.take() {
                if spill.storage.len() > self.buffer.len()
                    && spill.storage.capacity() <= MAX_PROMOTED_BUFFER_WORDS
                {
                    // Borrowing began with an exclusive receive buffer, and no
                    // owner can escape the borrowed-frame API. The consumed
                    // spill starts at zero; its old bytes stay hidden until a
                    // later complete frame has overwritten its exposed prefix.
                    core::mem::swap(Arc::get_mut(&mut self.buffer).unwrap(), &mut spill.storage);
                }
                if spill.storage.capacity() <= MAX_CACHED_SPILL_WORDS {
                    self.spare = spill.storage;
                }
            } else {
                let bytes = self.frame.take().expect("borrowed frame").bytes;
                self.consume_buffered(bytes);
            }
        }
    }

    // Owning reads retain their half-buffer direct-read threshold. A borrowed
    // frame can fill its reusable buffer because no storage owner escapes.
    // Framing, read-ahead policy and error checks remain shared.
    fn poll_prepare_frame(
        &mut self,
        cx: &mut Context<'_>,
        borrowed: bool,
        streaming: bool,
    ) -> Poll<Result<bool>> {
        loop {
            if let Some(spill) = &mut self.spill {
                if spill.filled < spill.frame.bytes {
                    let count = ready!(Pin::new(&mut self.input).poll_read(
                        cx,
                        &mut Word::words_to_bytes_mut(&mut spill.storage)[spill.filled..],
                    ))?;
                    if count == 0 {
                        return Poll::Ready(Err(Error::from_kind(ErrorKind::PrematureEndOfFile)));
                    }
                    spill.filled += count;
                    continue;
                }
                return Poll::Ready(Ok(true));
            }
            if self.frame.is_none() {
                self.frame = Frame::parse(
                    &Word::words_to_bytes(&self.buffer)[self.begin..self.end],
                    self.options,
                )?;
            }
            if let Some(frame) = &self.frame {
                if self.end - self.begin >= frame.bytes {
                    return Poll::Ready(Ok(true));
                }
                if streaming && frame.bytes > MAX_PROMOTED_BUFFER_WORDS * 8 {
                    return Poll::Ready(Ok(true));
                }
                let threshold = self.buffer.len() * if borrowed { 8 } else { 4 };
                if frame.bytes > threshold || !(self.read_ahead)(&self.input) {
                    let words = frame.bytes / 8;
                    let mut storage = core::mem::take(&mut self.spare);
                    if storage.capacity() < words {
                        storage = Word::allocate_zeroed_vec(words);
                    } else {
                        storage.resize(words, capnp::word(0, 0, 0, 0, 0, 0, 0, 0));
                    }
                    let filled = self.end - self.begin;
                    Word::words_to_bytes_mut(&mut storage)[..filled]
                        .copy_from_slice(&Word::words_to_bytes(&self.buffer)[self.begin..self.end]);
                    self.spill = Some(Spill {
                        storage,
                        filled,
                        frame: self.frame.take().unwrap(),
                    });
                    self.begin = 0;
                    self.end = 0;
                    continue;
                }
            }
            let buffer = Word::words_to_bytes_mut(Arc::get_mut(&mut self.buffer).unwrap());
            if buffer.len() - self.end < buffer.len() / 2 {
                buffer.copy_within(self.begin..self.end, 0);
                self.end -= self.begin;
                self.begin = 0;
            }
            let mut read_end = buffer.len();
            if !(self.read_ahead)(&self.input) {
                // No complete Frame exists here: complete only the current
                // header before choosing an exact-size body allocation.
                let available = self.end - self.begin;
                let header = if available < 8 {
                    8
                } else {
                    let (count, _) =
                        crate::serialize::parse_segment_table_first(&buffer[self.begin..self.end])?;
                    (count / 2 + 1) * 8
                };
                read_end = read_end.min(self.begin + header);
            }
            let count =
                ready!(Pin::new(&mut self.input).poll_read(cx, &mut buffer[self.end..read_end]))?;
            if count == 0 {
                return Poll::Ready(if self.begin == self.end {
                    Ok(false)
                } else {
                    Err(Error::from_kind(ErrorKind::PrematureEndOfFile))
                });
            }
            self.end += count;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fragmented {
        input: futures::io::Cursor<Vec<u8>>,
        chunk: usize,
        pending: bool,
    }
    impl AsyncRead for Fragmented {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            bytes: &mut [u8],
        ) -> Poll<std::io::Result<usize>> {
            if core::mem::replace(&mut self.pending, true) {
                self.pending = false;
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            let length = bytes.len().min(self.chunk);
            Pin::new(&mut self.input).poll_read(cx, &mut bytes[..length])
        }
    }

    #[test]
    fn buffered_successor_hint_never_reads_consumes_or_overlaps_an_owner() {
        let mut message = capnp::message::Builder::new_default();
        message.set_root("buffered").unwrap();
        let frame = capnp::serialize::write_message_to_words(&message);
        let mut source = frame.clone();
        source.extend_from_slice(&frame);
        source.extend_from_slice(&[0xff; 8]);
        let mut input = BufferedRead::new(futures::io::Cursor::new(source), Default::default());
        let mut cx = Context::from_waker(std::task::Waker::noop());
        assert!(!input.has_buffered_next_frame());
        assert_eq!(input.get_ref().position(), 0);
        assert!(matches!(
            input.poll_stream_frame_size(&mut cx),
            Poll::Ready(Ok(Some(_)))
        ));
        assert!(
            !input.has_buffered_next_frame(),
            "current frame still borrowed"
        );
        input.finish_borrowed_frame();
        let read = input.get_ref().position();
        let consumed = input.consumed_bytes();
        assert!(input.has_buffered_next_frame());
        assert!(input.has_buffered_next_frame());
        assert_eq!(input.get_ref().position(), read);
        assert_eq!(input.consumed_bytes(), consumed);
        assert!(matches!(
            input.poll_stream_frame_size(&mut cx),
            Poll::Ready(Ok(Some(_)))
        ));
        input.finish_borrowed_frame();
        assert!(
            !input.has_buffered_next_frame(),
            "bad header remains for receive"
        );
        assert!(matches!(
            input.poll_stream_frame_size(&mut cx),
            Poll::Ready(Err(_))
        ));
        assert!(!input.has_buffered_next_frame());
    }

    #[test]
    #[cfg_attr(miri, ignore = "exercises direct reads above the 128 KiB threshold")]
    fn streamed_frames_retain_partial_progress_and_stop_at_the_next_header() {
        let mut message = capnp::message::Builder::new_default();
        message
            .initn_root::<capnp::data::Builder>((MAX_PROMOTED_BUFFER_WORDS * 8) as u32)
            .fill(0x5a);
        let large = capnp::serialize::write_message_to_words(&message);
        let mut message = capnp::message::Builder::new_default();
        message.initn_root::<capnp::data::Builder>(16).fill(0x3c);
        let small = capnp::serialize::write_message_to_words(&message);
        for chunk in [7, 8192] {
            let mut source = large.clone();
            source.extend_from_slice(&small);
            let mut input = BufferedRead::with_buffer_size(
                Fragmented {
                    input: futures::io::Cursor::new(source),
                    chunk,
                    pending: false,
                },
                Default::default(),
                MIN_BUFFER_WORDS,
            )
            .unwrap();
            let length = futures::executor::block_on(core::future::poll_fn(|cx| {
                input.poll_stream_frame_size(cx)
            }))
            .unwrap()
            .unwrap();
            assert_eq!(length, large.len());
            assert!(input.spill.is_none());
            assert!(input.spare.is_empty());
            assert!(input.borrowed_frame().is_none());
            let mut cx = Context::from_waker(std::task::Waker::noop());
            assert!(matches!(
                input.poll_borrowed_frame(&mut cx),
                Poll::Ready(Err(_))
            ));
            assert!(
                matches!(input.poll_stream_frame_size(&mut cx), Poll::Ready(Ok(Some(n))) if n == length)
            );
            assert_eq!(input.consumed_bytes(), 0);
            let mut output = vec![0xa5; length + 8];
            let mut position = 0;
            while position < length {
                let count = futures::executor::block_on(core::future::poll_fn(|cx| {
                    input.poll_stream_frame(cx, &mut output[position..])
                }))
                .unwrap();
                assert!(count > 0);
                position += count;
            }
            assert_eq!(&output[..length], &large);
            assert_eq!(&output[length..], &[0xa5; 8]);
            assert_eq!(input.consumed_bytes(), length as u64);
            assert_eq!(
                futures::executor::block_on(core::future::poll_fn(
                    |cx| input.poll_stream_frame(cx, &mut output)
                ))
                .unwrap(),
                0
            );
            input.finish_borrowed_frame();
            let size = futures::executor::block_on(core::future::poll_fn(|cx| {
                input.poll_stream_frame_size(cx)
            }))
            .unwrap();
            assert_eq!(size, Some(small.len()));
            assert_eq!(input.borrowed_frame().unwrap(), small);
            input.finish_borrowed_frame();
            assert_eq!(
                futures::executor::block_on(core::future::poll_fn(
                    |cx| input.poll_stream_frame_size(cx)
                ))
                .unwrap(),
                None
            );
        }
    }

    #[test]
    #[cfg_attr(miri, ignore = "exercises direct reads above the 128 KiB threshold")]
    fn streamed_truncation_and_abandonment_are_terminal() {
        let mut message = capnp::message::Builder::new_default();
        message
            .initn_root::<capnp::data::Builder>((MAX_PROMOTED_BUFFER_WORDS * 8) as u32)
            .fill(0x5a);
        let bytes = capnp::serialize::write_message_to_words(&message);
        for abandon in [false, true] {
            let mut input = BufferedRead::with_buffer_size(
                futures::io::Cursor::new(bytes[..bytes.len() - 1].to_vec()),
                Default::default(),
                MIN_BUFFER_WORDS,
            )
            .unwrap();
            futures::executor::block_on(core::future::poll_fn(|cx| {
                input.poll_stream_frame_size(cx)
            }))
            .unwrap();
            if !abandon {
                let mut output = vec![0xa5; bytes.len()];
                loop {
                    if futures::executor::block_on(core::future::poll_fn(|cx| {
                        input.poll_stream_frame(cx, &mut output)
                    }))
                    .is_err()
                    {
                        break;
                    }
                }
                assert!(futures::executor::block_on(core::future::poll_fn(
                    |cx| input.poll_stream_frame_size(cx)
                ))
                .is_err());
            }
            input.finish_borrowed_frame();
            assert!(futures::executor::block_on(core::future::poll_fn(
                |cx| input.poll_stream_frame_size(cx)
            ))
            .is_err());
            assert!(futures::executor::block_on(core::future::poll_fn(|cx| input
                .poll_borrowed_frame(cx)
                .map_ok(|frame| frame.is_some())))
            .is_err());
            assert_eq!(input.consumed_bytes(), 0);
        }
        let mut options = ReaderOptions::new();
        options.traversal_limit_in_words = Some(0);
        let mut input = BufferedRead::new(futures::io::Cursor::new(bytes), options);
        assert!(futures::executor::block_on(core::future::poll_fn(
            |cx| input.poll_stream_frame_size(cx)
        ))
        .is_err());
        assert!(input.spill.is_none());
        assert!(input.streaming.is_none());
    }

    #[test]
    fn streaming_preparation_keeps_borrowed_owners_and_counter_failures_sticky() {
        let mut message = capnp::message::Builder::new_default();
        message.initn_root::<capnp::data::Builder>(16).fill(0x5a);
        let bytes = capnp::serialize::write_message_to_words(&message);
        let mut input =
            BufferedRead::new(futures::io::Cursor::new(bytes.clone()), Default::default());
        let mut cx = Context::from_waker(std::task::Waker::noop());
        let mut scratch = [0; 32];
        assert!(matches!(
            input.poll_stream_frame(&mut cx, &mut scratch),
            Poll::Ready(Err(_))
        ));
        let owned = futures::executor::block_on(input.try_read_message(|_| Ok(true)))
            .unwrap()
            .unwrap();
        assert!(matches!(
            input.poll_stream_frame_size(&mut cx),
            Poll::Ready(Err(_))
        ));
        drop(owned);
        assert!(matches!(
            input.poll_stream_frame_size(&mut cx),
            Poll::Ready(Ok(None))
        ));
        assert!(matches!(
            input.poll_stream_frame_size(&mut cx),
            Poll::Ready(Ok(None))
        ));
        let mut input =
            BufferedRead::new(futures::io::Cursor::new(bytes.clone()), Default::default());
        assert!(
            matches!(input.poll_stream_frame_size(&mut cx), Poll::Ready(Ok(Some(n))) if n == bytes.len())
        );
        assert!(
            matches!(input.poll_stream_frame_size(&mut cx), Poll::Ready(Ok(Some(n))) if n == bytes.len())
        );
        assert_eq!(input.borrowed_frame().unwrap(), bytes);
        assert_eq!(input.consumed_bytes(), bytes.len() as u64);
        input.finish_borrowed_frame();
        let mut input = BufferedRead::new(futures::io::Cursor::new(bytes), Default::default());
        input.consumed = u64::MAX;
        assert!(matches!(
            input.poll_stream_frame_size(&mut cx),
            Poll::Ready(Err(_))
        ));
        assert!(matches!(
            input.poll_stream_frame_size(&mut cx),
            Poll::Ready(Err(_))
        ));
    }

    #[test]
    #[cfg_attr(miri, ignore = "exercises direct reads above the 128 KiB threshold")]
    fn streamed_io_failure_is_sticky_without_another_read() {
        struct Failure {
            bytes: Vec<u8>,
            reads: usize,
        }
        impl AsyncRead for Failure {
            fn poll_read(
                mut self: Pin<&mut Self>,
                _: &mut Context<'_>,
                output: &mut [u8],
            ) -> Poll<std::io::Result<usize>> {
                self.reads += 1;
                if self.reads > 1 {
                    return Poll::Ready(Err(std::io::Error::other("injected")));
                }
                output.copy_from_slice(&self.bytes[..output.len()]);
                Poll::Ready(Ok(output.len()))
            }
        }
        let mut message = capnp::message::Builder::new_default();
        message
            .initn_root::<capnp::data::Builder>((MAX_PROMOTED_BUFFER_WORDS * 8) as u32)
            .fill(0x5a);
        let bytes = capnp::serialize::write_message_to_words(&message);
        let mut input = BufferedRead::with_buffer_size(
            Failure { bytes, reads: 0 },
            Default::default(),
            MIN_BUFFER_WORDS,
        )
        .unwrap();
        let mut cx = Context::from_waker(std::task::Waker::noop());
        assert!(matches!(
            input.poll_stream_frame_size(&mut cx),
            Poll::Ready(Ok(Some(_)))
        ));
        assert!(matches!(
            input.poll_stream_frame(&mut cx, &mut []),
            Poll::Ready(Ok(0))
        ));
        let mut output = vec![0; MAX_PROMOTED_BUFFER_WORDS * 8 + 4096];
        assert!(
            matches!(input.poll_stream_frame(&mut cx, &mut output), Poll::Ready(Ok(n)) if n > 0)
        );
        assert!(matches!(
            input.poll_stream_frame(&mut cx, &mut output),
            Poll::Ready(Err(_))
        ));
        assert!(matches!(
            input.poll_stream_frame(&mut cx, &mut output),
            Poll::Ready(Err(_))
        ));
        assert_eq!(input.get_ref().reads, 2);
        assert_eq!(input.consumed_bytes(), 0);
    }

    #[test]
    fn repeated_borrowed_frame_reuses_bounded_storage_in_one_read() {
        struct Counted {
            input: futures::io::Cursor<Vec<u8>>,
            reads: usize,
        }
        impl AsyncRead for Counted {
            fn poll_read(
                mut self: Pin<&mut Self>,
                cx: &mut Context<'_>,
                bytes: &mut [u8],
            ) -> Poll<std::io::Result<usize>> {
                self.reads += 1;
                Pin::new(&mut self.input).poll_read(cx, bytes)
            }
        }
        let mut message = capnp::message::Builder::new_default();
        message.initn_root::<capnp::data::Builder>(65536).fill(0x5a);
        let bytes = capnp::serialize::write_message_to_words(&message);
        for streaming in [false, true] {
            let mut source = bytes.clone();
            source.extend_from_slice(&bytes);
            let mut input = BufferedRead::with_buffer_size(
                Counted {
                    input: futures::io::Cursor::new(source),
                    reads: 0,
                },
                Default::default(),
                MIN_BUFFER_WORDS,
            )
            .unwrap();
            let mut cx = Context::from_waker(std::task::Waker::noop());
            let mut address = core::ptr::null();
            for iteration in 0..2 {
                let result = if streaming {
                    input.poll_stream_frame_size(&mut cx)
                } else {
                    input
                        .poll_borrowed_frame(&mut cx)
                        .map_ok(|frame| frame.map(<[u8]>::len))
                };
                assert!(matches!(result, Poll::Ready(Ok(Some(n))) if n == bytes.len()));
                let frame = input.borrowed_frame().unwrap();
                assert_eq!(frame, bytes);
                if iteration == 0 {
                    address = frame.as_ptr();
                } else {
                    assert_eq!(frame.as_ptr(), address);
                }
                assert_eq!(input.get_ref().reads, 2 + iteration);
                input.finish_borrowed_frame();
                assert!(input.buffer.capacity() <= MAX_PROMOTED_BUFFER_WORDS);
            }
            assert!(matches!(
                input.poll_borrowed_frame(&mut cx),
                Poll::Ready(Ok(None))
            ));
        }
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "exercises the 8 MiB retention boundary; small fragmented reuse is covered separately"
    )]
    fn borrowed_spill_cache_has_a_fixed_capacity_bound() {
        for length in [8192, MAX_CACHED_SPILL_WORDS * 8 + 8] {
            let mut message = capnp::message::Builder::new_default();
            message
                .initn_root::<capnp::data::Builder>(length as u32)
                .fill(0x5a);
            let bytes = capnp::serialize::write_message_to_words(&message);
            let mut input = BufferedRead::with_buffer_size(
                futures::io::Cursor::new(bytes.clone()),
                Default::default(),
                MIN_BUFFER_WORDS,
            )
            .unwrap();
            let mut cx = Context::from_waker(std::task::Waker::noop());
            let Poll::Ready(Ok(Some(frame))) = input.poll_borrowed_frame(&mut cx) else {
                panic!("complete frame")
            };
            assert_eq!(frame, bytes);
            input.finish_borrowed_frame();
            assert!(input.spare.capacity() <= MAX_CACHED_SPILL_WORDS);
            assert_eq!(input.spare.is_empty(), length > MAX_CACHED_SPILL_WORDS * 8);
        }
    }
}
