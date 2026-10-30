//! Small mutable envelopes stay inside the retained output owner. Large
//! envelopes use aligned heap storage; payload segments are never copied here.
use capnp::message::ReaderSegments;
use capnp::Word;

pub(super) enum Header {
    Inline([u8; 32]),
    Heap(alloc::vec::Vec<Word>),
}
impl Header {
    pub(super) fn new(segments: &impl ReaderSegments) -> Option<Self> {
        let (length, _) = super::payload::frame_layout(segments)?;
        let mut header = if length <= 32 {
            Self::Inline([0; 32])
        } else {
            Self::Heap(Word::allocate_zeroed_vec(length / 8))
        };
        let bytes = match &mut header {
            Self::Inline(bytes) => &mut bytes[..],
            Self::Heap(words) => Word::words_to_bytes_mut(words),
        };
        bytes[..4].copy_from_slice(&((segments.len() - 1) as u32).to_le_bytes());
        for index in 0..segments.len() {
            let segment = segments.get_segment(index as u32)?;
            let slot = (index + 1) * 4;
            bytes[slot..slot + 4].copy_from_slice(&((segment.len() / 8) as u32).to_le_bytes());
        }
        Some(header)
    }
    pub(super) fn bytes(&self) -> &[u8] {
        match self {
            Self::Inline(bytes) => {
                let count = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize + 1;
                &bytes[..(count / 2 + 1) * 8]
            }
            Self::Heap(words) => Word::words_to_bytes(words),
        }
    }
    pub(super) fn capacity_bytes(&self) -> usize {
        match self {
            Self::Inline(_) => 32,
            Self::Heap(words) => words.capacity() * 8,
        }
    }
}

/// Only the first segment contains mutable RPC IDs. Every remaining segment
/// continues to borrow the frozen application allocation through this owner.
pub(super) struct FramedParts<S> {
    pub header: Header,
    pub first: Envelope,
    pub source: alloc::rc::Rc<S>,
    pub capacity: usize,
}
impl<S: ReaderSegments> capntproto_rpc_engine::messages::Parts for FramedParts<S> {
    fn count(&self) -> usize {
        self.source.len() + 1
    }
    fn part(&self, index: usize) -> &[u8] {
        match index {
            0 => self.header.bytes(),
            1 => self.first.bytes(),
            _ => self
                .source
                .get_segment((index - 1) as u32)
                .expect("frozen segment"),
        }
    }
    fn retained_capacity(&self) -> usize {
        self.capacity
    }
}

pub(super) enum Envelope {
    Inline { words: [Word; 16], length: usize },
    Heap(alloc::vec::Vec<Word>),
}
impl Envelope {
    pub(super) fn copy_from(bytes: &[u8]) -> Self {
        assert_eq!(bytes.len() % 8, 0);
        let mut result = if bytes.len() <= 128 {
            Self::Inline {
                words: [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 16],
                length: bytes.len() / 8,
            }
        } else {
            Self::Heap(Word::allocate_zeroed_vec(bytes.len() / 8))
        };
        result.bytes_mut().copy_from_slice(bytes);
        result
    }
    pub(super) fn bytes(&self) -> &[u8] {
        match self {
            Self::Inline { words, length } => Word::words_to_bytes(&words[..*length]),
            Self::Heap(words) => Word::words_to_bytes(words),
        }
    }
    pub(super) fn bytes_mut(&mut self) -> &mut [u8] {
        match self {
            Self::Inline { words, length } => Word::words_to_bytes_mut(&mut words[..*length]),
            Self::Heap(words) => Word::words_to_bytes_mut(words),
        }
    }
    pub(super) fn capacity_bytes(&self) -> usize {
        match self {
            Self::Inline { .. } => 128,
            Self::Heap(words) => words.capacity() * 8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inline_and_heap_headers_round_trip_the_wire_segment_table() {
        use capnp::message::{ReaderOptions, SegmentArray};
        for count in [1, 2, 3, 4, 7, 8, 31, if cfg!(miri) { 32 } else { 511 }] {
            let word = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0)];
            let segments = alloc::vec![Word::words_to_bytes(&word); count];
            let source = SegmentArray::new(&segments);
            let header = Header::new(&source).unwrap();
            assert_eq!(matches!(header, Header::Inline(_)), count <= 7);
            let mut bytes = Word::allocate_zeroed_vec((header.bytes().len() + count * 8) / 8);
            let input = Word::words_to_bytes_mut(&mut bytes);
            input[..header.bytes().len()].copy_from_slice(header.bytes());
            input[header.bytes().len()..].fill(0);
            let mut input = &input[..];
            let decoded = capnp::serialize::read_message_from_flat_slice_no_alloc(
                &mut input,
                ReaderOptions::new(),
            )
            .unwrap();
            assert!(input.is_empty());
            assert_eq!(decoded.get_segments().len(), count);
            assert_eq!(
                decoded
                    .get_segments()
                    .get_segment((count - 1) as u32)
                    .unwrap(),
                segments[count - 1]
            );
        }
        assert!(Header::new(&SegmentArray::new(&[])).is_none());
    }

    #[test]
    fn inline_and_heap_envelopes_preserve_extent_alignment_and_charge() {
        for length in [0, 8, 120, 128, 136, 4096] {
            let bytes = alloc::vec![0xa5; length];
            let mut envelope = Envelope::copy_from(&bytes);
            assert_eq!(envelope.bytes(), bytes);
            assert_eq!(envelope.bytes().as_ptr().align_offset(8), 0);
            assert!(envelope.capacity_bytes() >= length);
            if let Some(first) = envelope.bytes_mut().first_mut() {
                *first = 42;
            }
            if length > 0 {
                assert_eq!(envelope.bytes()[0], 42);
            }
            assert_eq!(envelope.bytes().len(), length);
            assert_eq!(matches!(envelope, Envelope::Inline { .. }), length <= 128);
        }
    }
}
