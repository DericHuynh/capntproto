//! Incremental framing over caller-owned storage; no IO, executor or allocator.
#![forbid(unsafe_code)]

use super::{NoAllocSegmentTableInfo, NoAllocSliceSegments, SEGMENTS_COUNT_LIMIT};
use crate::{message, Error, ErrorKind, Result};

/// Validated first word of standard Cap'n Proto stream framing.
/// Shared by fixed-buffer and hosted readers. Parsing does not allocate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentTableHeader {
    count: usize,
    first_words: usize,
}

impl SegmentTableHeader {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let word = bytes
            .get(..8)
            .ok_or_else(|| Error::from_kind(ErrorKind::PrematureEndOfFile))?;
        let count =
            usize::try_from(u32::from_le_bytes(word[..4].try_into().unwrap()).wrapping_add(1))
                .map_err(|_| Error::from_kind(ErrorKind::MessageSizeOverflow))?;
        if count == 0 || count >= SEGMENTS_COUNT_LIMIT {
            return Err(Error::from_kind(ErrorKind::InvalidNumberOfSegments(count)));
        }
        Ok(Self {
            count,
            first_words: usize::try_from(u32::from_le_bytes(word[4..].try_into().unwrap()))
                .map_err(|_| Error::from_kind(ErrorKind::MessageSizeOverflow))?,
        })
    }

    pub const fn segment_count(self) -> usize {
        self.count
    }
    pub const fn first_segment_words(self) -> usize {
        self.first_words
    }
    pub const fn table_bytes(self) -> usize {
        (self.count / 2 + 1) * 8
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Header,
    Table,
    Body,
    Complete,
    End,
    Failed(ErrorKind),
}

/// Resumable, allocation-free receiver for one standard stream frame at a time.
///
/// Supply aligned storage including space for the segment table. Each input
/// window is exactly the remaining header, table or body; no read can prefetch
/// the next message. Drop an uncommitted window when IO is pending. Commit every
/// completed read, including zero for EOF, before polling again. Dropping a
/// window does not change progress. IO errors may be recorded with [`Self::abort`].
///
/// After completion, [`Self::message`] lends a reader and [`Self::next_message`]
/// reuses storage once every view is gone. Errors are terminal for this receiver;
/// constructing another receiver is appropriate only at a known frame boundary.
/// No method allocates, even when the core's `alloc` feature is enabled.
///
/// A live reader prevents reusing its buffer:
/// ```compile_fail,E0502
/// use capnp::{serialize::FrameReceiver, Word};
/// let mut words = [capnp::word(0,0,0,0,0,0,0,0); 2];
/// let mut receiver = FrameReceiver::new(Word::words_to_bytes_mut(&mut words), Default::default()).unwrap();
/// let mut input = receiver.input().unwrap().unwrap();
/// input.bytes().fill(0);
/// input.commit(8).unwrap();
/// let message = receiver.message().unwrap().unwrap();
/// receiver.next_message().unwrap();
/// let _ = message.get_root::<capnp::any_pointer::Reader<'_>>();
/// ```
pub struct FrameReceiver<'buffer> {
    buffer: &'buffer mut [u8],
    options: message::ReaderOptions,
    phase: Phase,
    position: usize,
    end: usize,
    count: usize,
    table_bytes: usize,
}

impl<'buffer> FrameReceiver<'buffer> {
    pub fn new(buffer: &'buffer mut [u8], options: message::ReaderOptions) -> Result<Self> {
        if buffer.len() < 8 {
            return Err(Error::from_kind(ErrorKind::BufferNotLargeEnough));
        }
        if !cfg!(feature = "unaligned") && !(buffer.as_ptr() as usize).is_multiple_of(8) {
            return Err(Error::from_kind(ErrorKind::UnalignedSegment));
        }
        Ok(Self {
            buffer,
            options,
            phase: Phase::Header,
            position: 0,
            end: 8,
            count: 0,
            table_bytes: 0,
        })
    }

    /// Bytes committed for the current frame, including its segment table.
    pub const fn received_bytes(&self) -> usize {
        self.position
    }

    /// The next bounded IO destination. `None` means complete frame or clean EOF.
    pub fn input(&mut self) -> Result<Option<ReadWindow<'_, 'buffer>>> {
        match self.phase {
            Phase::Complete | Phase::End => Ok(None),
            Phase::Failed(kind) => Err(Error::from_kind(kind)),
            _ => Ok(Some(ReadWindow { receiver: self })),
        }
    }

    /// Preserve the first transport/framing failure; a failed receiver cannot resume.
    pub fn abort(&mut self, kind: ErrorKind) {
        if !matches!(self.phase, Phase::Failed(_)) {
            self.phase = Phase::Failed(kind);
        }
    }

    /// Return the current frame, or `None` at clean EOF. Incomplete data is never exposed.
    pub fn message(&self) -> Result<Option<message::Reader<NoAllocSliceSegments<'_>>>> {
        match self.phase {
            Phase::Complete => Ok(Some(message::Reader::new(
                NoAllocSliceSegments::from_segment_table_info(
                    &self.buffer[..self.end],
                    self.info(),
                ),
                self.options,
            ))),
            Phase::End => Ok(None),
            Phase::Failed(kind) => Err(Error::from_kind(kind)),
            _ => Err(Error::from_kind(ErrorKind::IncompleteFill)),
        }
    }

    /// Consume the receiver while keeping its backing storage borrowed by the message.
    pub fn into_message(self) -> Result<Option<message::Reader<NoAllocSliceSegments<'buffer>>>> {
        match self.phase {
            Phase::Complete => {
                let info = self.info();
                Ok(Some(message::Reader::new(
                    NoAllocSliceSegments::from_segment_table_info(&self.buffer[..self.end], info),
                    self.options,
                )))
            }
            Phase::End => Ok(None),
            Phase::Failed(kind) => Err(Error::from_kind(kind)),
            _ => Err(Error::from_kind(ErrorKind::IncompleteFill)),
        }
    }

    /// Begin another frame after completion. Borrowing prevents live readers from being overwritten.
    pub fn next_message(&mut self) -> Result<()> {
        match self.phase {
            Phase::Complete => {
                self.phase = Phase::Header;
                self.position = 0;
                self.end = 8;
                self.count = 0;
                self.table_bytes = 0;
                Ok(())
            }
            Phase::Failed(kind) => Err(Error::from_kind(kind)),
            _ => Err(Error::from_kind(ErrorKind::IncompleteFill)),
        }
    }

    fn info(&self) -> NoAllocSegmentTableInfo {
        NoAllocSegmentTableInfo {
            segments_count: self.count,
            segment_table_length_bytes: self.table_bytes,
            total_segments_length_bytes: self.end - self.table_bytes,
        }
    }

    fn advance(&mut self, count: usize) -> Result<()> {
        let available = self.end - self.position;
        if count > available {
            self.abort(ErrorKind::InvalidReadCount(count, available));
        } else if count == 0 {
            if self.position == 0 {
                self.phase = Phase::End;
            } else {
                self.abort(ErrorKind::PrematureEndOfFile);
            }
        } else {
            self.position += count;
            if self.position == self.end {
                if let Err(error) = self.complete_part() {
                    self.abort(error.kind);
                }
            }
        }
        if let Phase::Failed(kind) = self.phase {
            Err(Error::from_kind(kind))
        } else {
            Ok(())
        }
    }

    fn complete_part(&mut self) -> Result<()> {
        if self.phase == Phase::Header {
            let header = SegmentTableHeader::parse(&self.buffer[..8])?;
            self.count = header.segment_count();
            self.table_bytes = header.table_bytes();
            if self.table_bytes > self.buffer.len() {
                return Err(Error::from_kind(ErrorKind::BufferNotLargeEnough));
            }
            self.end = self.table_bytes;
            self.phase = Phase::Table;
        }
        if self.phase == Phase::Table && self.position == self.end {
            let words = self.buffer[4..4 + self.count * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .try_fold(0usize, |total, length| {
                    let length = usize::try_from(u32::from_le_bytes(*length))
                        .map_err(|_| Error::from_kind(ErrorKind::MessageSizeOverflow))?;
                    total
                        .checked_add(length)
                        .ok_or_else(|| Error::from_kind(ErrorKind::MessageSizeOverflow))
                })?;
            if self
                .options
                .traversal_limit_in_words
                .is_some_and(|limit| words > limit)
            {
                return Err(Error::from_kind(ErrorKind::MessageTooLarge(words)));
            }
            let end = words
                .checked_mul(8)
                .and_then(|bytes| self.table_bytes.checked_add(bytes))
                .ok_or_else(|| Error::from_kind(ErrorKind::MessageSizeOverflow))?;
            if end > self.buffer.len() {
                return Err(Error::from_kind(ErrorKind::BufferNotLargeEnough));
            }
            self.end = end;
            self.phase = Phase::Body;
        }
        if self.phase == Phase::Body && self.position == self.end {
            self.phase = Phase::Complete;
        }
        Ok(())
    }
}

/// An exclusive destination for exactly one caller IO operation. Committing
/// consumes the window; it cannot be committed twice or reused after advancement.
pub struct ReadWindow<'receiver, 'buffer> {
    receiver: &'receiver mut FrameReceiver<'buffer>,
}
impl ReadWindow<'_, '_> {
    pub fn bytes(&mut self) -> &mut [u8] {
        &mut self.receiver.buffer[self.receiver.position..self.receiver.end]
    }
    pub fn commit(self, count: usize) -> Result<()> {
        self.receiver.advance(count)
    }
}

#[cfg(test)]
mod tests;
