//! Bounded, aligned reads of unpacked messages used by struct embeds.
use capnp::{any_struct, message, serialize, Word};

use std::sync::{Arc, OnceLock};

pub(crate) struct File {
    pub bytes: Arc<[u8]>,
    parsed: OnceLock<Result<Arc<Embedded>, String>>,
}

impl File {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes: bytes.into(),
            parsed: OnceLock::new(),
        }
    }

    pub fn structure(&self) -> capnp::Result<Arc<Embedded>> {
        self.parsed
            .get_or_init(|| {
                Embedded::new(&self.bytes)
                    .map(Arc::new)
                    .map_err(|e| e.to_string())
            })
            .clone()
            .map_err(capnp::Error::failed)
    }
}

#[derive(Debug)]
pub(crate) struct Embedded {
    words: Vec<Word>,
    pub size: usize,
}

impl Embedded {
    pub fn new(bytes: &[u8]) -> capnp::Result<Self> {
        if !bytes.len().is_multiple_of(8) {
            return Err(capnp::Error::failed(
                "embedded file is not a word-aligned Cap'n Proto message".into(),
            ));
        }
        // Vec<u8> has no alignment guarantee. Copy into words before parsing;
        // do not depend on the runtime's optional `unaligned` feature.
        let mut words = Word::allocate_zeroed_vec(bytes.len() / 8);
        Word::words_to_bytes_mut(&mut words).copy_from_slice(bytes);
        let mut value = Self { words, size: 0 };
        let size = value
            .reader()?
            .get_root::<any_struct::Reader<'_>>()?
            .total_size()?;
        if size.cap_count != 0 {
            return Err(capnp::Error::failed(
                "embedded messages cannot contain capabilities".into(),
            ));
        }
        value.size = usize::try_from(size.word_count)
            .ok()
            .and_then(|words| words.checked_mul(8))
            .filter(|bytes| *bytes <= 16 * 1024 * 1024)
            .ok_or_else(|| {
                capnp::Error::failed("expanded embedded message exceeds 16 MiB limit".into())
            })?;
        Ok(value)
    }

    pub fn reader(&self) -> capnp::Result<message::Reader<serialize::BufferSegments<&[u8]>>> {
        let options = message::ReaderOptions {
            traversal_limit_in_words: Some(2 * 1024 * 1024),
            nesting_limit: 64,
        };
        // Like the reference, consume the first framed message and allow
        // whole-word trailing data. Packed messages are not embed inputs.
        serialize::read_message_from_flat_slice(&mut Word::words_to_bytes(&self.words), options)
    }
}
