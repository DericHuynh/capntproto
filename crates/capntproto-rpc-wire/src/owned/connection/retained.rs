//! Small mutable envelopes stay inside the retained output owner. Large
//! envelopes use aligned heap storage; payload segments are never copied here.
use capnp::Word;

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
