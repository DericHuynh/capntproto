// Copyright (c) 2026 ReProto contributors. Licensed under the MIT License.

//! Buffered stream framing with explicit short-lived message ownership.
use std::sync::Arc;

use capnp::{
    message::{Reader, ReaderOptions, ReaderSegments},
    serialize::SegmentLengthsBuilder,
    Error, ErrorKind, Result, Word,
};
use futures_util::{AsyncRead, AsyncReadExt};

const DEFAULT_BUFFER_WORDS: usize = 8192;
// Every legal segment table fits, even when a frame's body needs direct I/O.
const MIN_BUFFER_WORDS: usize = capnp::serialize::SEGMENTS_COUNT_LIMIT / 2;

struct Frame {
    ranges: Vec<(usize, usize)>,
    bytes: usize,
}
impl Frame {
    fn parse(bytes: &[u8], options: ReaderOptions) -> Result<Option<Self>> {
        if bytes.len() < 8 {
            return Ok(None);
        }
        let (count, _) = crate::serialize::parse_segment_table_first(bytes)?;
        let table_bytes = (count / 2 + 1) * 8;
        if bytes.len() < table_bytes {
            return Ok(None);
        }
        let mut lengths = SegmentLengthsBuilder::with_capacity(count);
        for i in 0..count {
            let offset = (i + 1) * 4;
            lengths.try_push_segment(u32::from_le_bytes(
                bytes[offset..offset + 4].try_into().unwrap(),
            ) as usize)?;
        }
        if options
            .traversal_limit_in_words
            .is_some_and(|limit| lengths.total_words() > limit)
        {
            return Err(Error::failed(
                "incoming message exceeds traversal limit".into(),
            ));
        }
        let bytes = lengths
            .total_words()
            .checked_mul(8)
            .and_then(|size| size.checked_add(table_bytes))
            .ok_or_else(|| Error::from_kind(ErrorKind::MessageSizeOverflow))?;
        let ranges = lengths
            .to_segment_indices()
            .into_iter()
            .map(|(start, end)| (table_bytes + start * 8, table_bytes + end * 8))
            .collect();
        Ok(Some(Self { ranges, bytes }))
    }
}

/// Aligned segments backed by either the receive buffer or independent storage.
/// A shared-buffer message prevents subsequent reads until it is dropped. It
/// remains valid even if the stream itself is dropped. No unsafe aliasing is used.
pub struct BufferedSegments {
    storage: Arc<Vec<Word>>,
    start: usize,
    frame: Frame,
    shared: bool,
}
impl BufferedSegments {
    /// True when this message holds the stream's reusable receive buffer.
    pub fn is_shared_buffer(&self) -> bool {
        self.shared
    }

    fn detach(mut self) -> Self {
        let mut storage = Word::allocate_zeroed_vec(self.frame.bytes / 8);
        Word::words_to_bytes_mut(&mut storage).copy_from_slice(
            &Word::words_to_bytes(&self.storage)[self.start..self.start + self.frame.bytes],
        );
        self.storage = Arc::new(storage);
        self.start = 0;
        self.shared = false;
        self
    }
}
impl ReaderSegments for BufferedSegments {
    fn get_segment(&self, id: u32) -> Option<&[u8]> {
        self.frame.ranges.get(id as usize).map(|&(start, end)| {
            &Word::words_to_bytes(&self.storage)[self.start + start..self.start + end]
        })
    }
    fn len(&self) -> usize {
        self.frame.ranges.len()
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
                storage: Arc::new(words.to_vec()),
                start: 0,
                frame,
                shared: false,
            },
        }
    }
}
impl ReaderSegments for BufferedScratchSegments<'_> {
    fn get_segment(&self, id: u32) -> Option<&[u8]> {
        match &self.0 {
            ScratchStorage::Buffered(s) => s.get_segment(id),
            ScratchStorage::Borrowed { words, frame } => frame
                .ranges
                .get(id as usize)
                .map(|&(start, end)| &Word::words_to_bytes(words)[start..end]),
        }
    }
    fn len(&self) -> usize {
        match &self.0 {
            ScratchStorage::Buffered(s) => s.len(),
            ScratchStorage::Borrowed { frame, .. } => frame.ranges.len(),
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
    terminal: Option<Result<()>>,
    consumed: u64,
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
            terminal: None,
            consumed: 0,
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
        Arc::strong_count(&self.buffer) != 1
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
    /// Unused scratch words are never modified. Segment metadata still allocates.
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
        scratch: Option<&'a mut [Word]>,
        is_short_lived: impl FnOnce(&Reader<BufferedSegments>) -> Result<bool>,
    ) -> Result<Option<Reader<BufferedScratchSegments<'a>>>> {
        if self.has_outstanding_short_lived_message() {
            return Err(Error::failed(
                "previous short-lived message is still alive".into(),
            ));
        }
        if let Some(terminal) = &self.terminal {
            return terminal.clone().map(|()| None);
        }
        let result = self.read(scratch, is_short_lived).await;
        match &result {
            Err(error) => {
                self.spill = None;
                self.frame = None;
                self.terminal = Some(Err(error.clone()));
            }
            Ok(None) => self.terminal = Some(Ok(())),
            Ok(Some(_)) => {}
        }
        result
    }

    async fn read<'a>(
        &mut self,
        scratch: Option<&'a mut [Word]>,
        is_short_lived: impl FnOnce(&Reader<BufferedSegments>) -> Result<bool>,
    ) -> Result<Option<Reader<BufferedScratchSegments<'a>>>> {
        loop {
            if let Some(spill) = &mut self.spill {
                if spill.filled < spill.frame.bytes {
                    let count = self
                        .input
                        .read(&mut Word::words_to_bytes_mut(&mut spill.storage)[spill.filled..])
                        .await?;
                    if count == 0 {
                        return Err(Error::from_kind(ErrorKind::PrematureEndOfFile));
                    }
                    spill.filled += count;
                    continue;
                }
                let spill = self.spill.take().unwrap();
                self.advance(spill.frame.bytes)?;
                return Ok(Some(Reader::new(
                    BufferedScratchSegments(ScratchStorage::Buffered(BufferedSegments {
                        storage: Arc::new(spill.storage),
                        start: 0,
                        frame: spill.frame,
                        shared: false,
                    })),
                    self.options,
                )));
            }
            if self.frame.is_none() {
                self.frame = Frame::parse(
                    &Word::words_to_bytes(&self.buffer)[self.begin..self.end],
                    self.options,
                )?;
            }
            if let Some(frame) = &self.frame {
                if self.end - self.begin >= frame.bytes {
                    let frame = self.frame.take().unwrap();
                    let bytes = frame.bytes;
                    let view = Reader::new(
                        BufferedSegments {
                            storage: self.buffer.clone(),
                            start: self.begin,
                            frame,
                            shared: true,
                        },
                        self.options,
                    );
                    let short_lived = is_short_lived(&view)?;
                    self.advance(bytes)?;
                    let segments = view.into_segments();
                    let segments = if short_lived {
                        BufferedScratchSegments(ScratchStorage::Buffered(segments))
                    } else {
                        segments.retain(scratch)
                    };
                    self.begin += bytes;
                    if self.begin == self.end {
                        self.begin = 0;
                        self.end = 0;
                    }
                    // Classification uses its own traversal budget.
                    return Ok(Some(Reader::new(segments, self.options)));
                }
                if frame.bytes > self.buffer.len() * 4 || !(self.read_ahead)(&self.input) {
                    let mut storage = Word::allocate_zeroed_vec(frame.bytes / 8);
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
            let count = self.input.read(&mut buffer[self.end..read_end]).await?;
            if count == 0 {
                return if self.begin == self.end {
                    Ok(None)
                } else {
                    Err(Error::from_kind(ErrorKind::PrematureEndOfFile))
                };
            }
            self.end += count;
        }
    }
}
