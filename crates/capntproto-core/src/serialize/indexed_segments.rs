//! Constant-time segment lookup with metadata supplied by the caller.
#![forbid(unsafe_code)]

use crate::{
    message::{ReaderOptions, ReaderSegments},
    Error, ErrorKind, Result,
};

/// One entry of caller-owned segment metadata. Offsets can only be populated
/// by [`IndexedSliceSegments::from_slice`], after validating the complete frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SegmentOffset {
    start: usize,
    end: usize,
}

impl SegmentOffset {
    pub const EMPTY: Self = Self { start: 0, end: 0 };
}

/// An allocation-free reader with O(1) segment access. The frame and its index
/// remain borrowed for the reader's lifetime. No payload is copied. Unlike
/// [`super::NoAllocSliceSegments`], it requires one metadata entry per segment.
pub struct IndexedSliceSegments<'data, 'index> {
    bytes: &'data [u8],
    index: &'index [SegmentOffset],
}

impl<'data, 'index> IndexedSliceSegments<'data, 'index> {
    /// Validate one complete stream frame, fill its index and advance `input`.
    /// On any error, both `input` and `index` are unchanged. Extra index entries
    /// and trailing frames are untouched. An insufficient index returns
    /// `BufferNotLargeEnough`; it never falls back to heap allocation.
    pub fn from_slice(
        input: &mut &'data [u8],
        index: &'index mut [SegmentOffset],
        options: ReaderOptions,
    ) -> Result<Self> {
        let table = super::no_alloc_buffer_segments::read_segment_table(input, options)?;
        if index.len() < table.segments_count {
            return Err(Error::from_kind(ErrorKind::BufferNotLargeEnough));
        }
        let length = table
            .segment_table_length_bytes
            .checked_add(table.total_segments_length_bytes)
            .ok_or_else(|| Error::from_kind(ErrorKind::MessageSizeOverflow))?;
        let (bytes, remaining) = input.split_at(length);
        let index = &mut index[..table.segments_count];
        let mut offset = table.segment_table_length_bytes;
        // Validation established that every length and their sum fit in the
        // frame. Filling the index cannot fail or partially publish a message.
        for (entry, encoded) in index.iter_mut().zip(bytes[4..].as_chunks::<4>().0) {
            let words = u32::from_le_bytes(*encoded) as usize;
            let end = offset + words * 8;
            *entry = SegmentOffset { start: offset, end };
            offset = end;
        }
        *input = remaining;
        Ok(Self { bytes, index })
    }
}

impl ReaderSegments for IndexedSliceSegments<'_, '_> {
    fn get_segment(&self, id: u32) -> Option<&[u8]> {
        let entry = self.index.get(id as usize)?;
        Some(&self.bytes[entry.start..entry.end])
    }

    fn len(&self) -> usize {
        self.index.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{word, Word};

    #[cfg(feature = "alloc")]
    #[test]
    fn owned_index_rejects_body_truncation_including_the_header_length() {
        let words = [word(0, 0, 0, 0, 2, 0, 0, 0), word(0, 0, 0, 0, 0, 0, 0, 0)];
        // The 16-byte buffer is as long as the claimed body, but includes an
        // 8-byte header. The old check accepted it, then panicked on traversal.
        assert!(matches!(
            super::super::BufferSegments::new(Word::words_to_bytes(&words), ReaderOptions::new(),),
            Err(Error {
                kind: ErrorKind::MessageEndsPrematurely(2, 1),
                ..
            })
        ));
    }

    #[test]
    fn no_alloc_stream_rejects_lengths_that_do_not_fit_the_target_or_buffer() {
        let header = [word(0, 0, 0, 0, 255, 255, 255, 255)];
        let mut storage = [word(0, 0, 0, 0, 0, 0, 0, 0); 2];
        let options = ReaderOptions {
            traversal_limit_in_words: None,
            ..ReaderOptions::new()
        };
        let result = super::super::try_read_message_no_alloc(
            Word::words_to_bytes(&header),
            Word::words_to_bytes_mut(&mut storage),
            options,
        );
        assert!(matches!(
            result,
            Err(Error {
                kind: ErrorKind::MessageSizeOverflow | ErrorKind::BufferNotLargeEnough,
                ..
            })
        ));
    }

    #[test]
    fn sparse_and_empty_segments_match_the_low_memory_reader() {
        let words = [
            word(2, 0, 0, 0, 1, 0, 0, 0),
            word(0, 0, 0, 0, 2, 0, 0, 0),
            word(11, 0, 0, 0, 0, 0, 0, 0),
            word(22, 0, 0, 0, 0, 0, 0, 0),
            word(33, 0, 0, 0, 0, 0, 0, 0),
            word(0, 0, 0, 0, 0, 0, 0, 0), // another frame
        ];
        let bytes = Word::words_to_bytes(&words);
        let mut input = bytes;
        let mut entries = [SegmentOffset::EMPTY; 4];
        let indexed =
            IndexedSliceSegments::from_slice(&mut input, &mut entries, ReaderOptions::new())
                .unwrap();
        let mut reference = bytes;
        let reference =
            super::super::NoAllocSliceSegments::from_slice(&mut reference, ReaderOptions::new())
                .unwrap();
        assert_eq!(indexed.len(), 3);
        for id in [0, 1, 2, 3, u32::MAX] {
            assert_eq!(indexed.get_segment(id), reference.get_segment(id));
        }
        assert_eq!(input, &bytes[40..]);
        assert_eq!(entries[3], SegmentOffset::EMPTY);
    }

    #[test]
    fn failed_validation_or_capacity_never_modifies_input_or_metadata() {
        let words = [
            word(1, 0, 0, 0, 1, 0, 0, 0),
            word(1, 0, 0, 0, 0, 0, 0, 0),
            word(0, 0, 0, 0, 0, 0, 0, 0),
            word(0, 0, 0, 0, 0, 0, 0, 0),
        ];
        let bytes = Word::words_to_bytes(&words);
        let sentinel = SegmentOffset {
            start: 99,
            end: 123,
        };
        for length in 0..bytes.len() {
            let original = &bytes[..length];
            let mut input = original;
            let mut entries = [sentinel; 2];
            assert!(IndexedSliceSegments::from_slice(
                &mut input,
                &mut entries,
                ReaderOptions::new()
            )
            .is_err());
            assert_eq!(input, original);
            assert_eq!(entries, [sentinel; 2]);
        }
        for capacity in 0..2 {
            let mut input = bytes;
            let mut entries = [sentinel; 2];
            assert!(matches!(
                IndexedSliceSegments::from_slice(
                    &mut input,
                    &mut entries[..capacity],
                    ReaderOptions::new()
                ),
                Err(Error {
                    kind: ErrorKind::BufferNotLargeEnough,
                    ..
                })
            ));
            assert_eq!(input, bytes);
            assert_eq!(entries, [sentinel; 2]);
        }
        let mut input = bytes;
        let mut entries = [sentinel; 2];
        let options = ReaderOptions {
            traversal_limit_in_words: Some(1),
            ..ReaderOptions::new()
        };
        assert!(IndexedSliceSegments::from_slice(&mut input, &mut entries, options).is_err());
        assert_eq!(input, bytes);
        assert_eq!(entries, [sentinel; 2]);
    }
}
