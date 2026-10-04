//! Immutable, arena-owned backing for zero-copy Data orphans.
use crate::{Error, Result, Word};
use alloc::{boxed::Box, sync::Arc};

/// An aligned immutable buffer retained until the message arena is destroyed,
/// even if every pointer to it has been cleared. The final word's padding is
/// serialized too: constructors require it to be present and zero.
pub struct ExternalData {
    pointer: *const u8,
    len: u32,
    words: u32,
    _owner: Box<dyn Send + Sync>,
}
// SAFETY: constructors require immutable storage, a stable address, and a
// Send + Sync owner. No API exposes a mutable reference to this memory.
unsafe impl Send for ExternalData {}
// SAFETY: all readers see immutable bytes retained by a Sync owner; no API
// exposes mutation or releases the owner while this value is shared.
unsafe impl Sync for ExternalData {}

impl ExternalData {
    /// Retain a shared word allocation without copying its payload.
    pub fn new(words: Arc<[Word]>, len: usize) -> Result<Self> {
        let (pointer, len, count) = Self::validate(Word::words_to_bytes(&words), len)?;
        Ok(Self {
            pointer,
            len,
            words: count,
            _owner: Box::new(words),
        })
    }

    /// Reference a static word slice without copying its payload.
    pub fn from_static(words: &'static [Word], len: usize) -> Result<Self> {
        let (pointer, len, count) = Self::validate(Word::words_to_bytes(words), len)?;
        Ok(Self {
            pointer,
            len,
            words: count,
            _owner: Box::new(words),
        })
    }

    /// Retain another immutable buffer owner, such as a read-only memory mapping.
    ///
    /// # Safety
    /// The slice returned by `owner.as_ref()` must stay at the same address and
    /// remain readable and immutable until `owner` is dropped. Moving the owner
    /// must not move the bytes. In particular, a mapped file must not be modified
    /// or truncated by another process while this message exists.
    pub unsafe fn from_owner<T: AsRef<[u8]> + Send + Sync + 'static>(
        owner: T,
        len: usize,
    ) -> Result<Self> {
        let (pointer, len, words) = Self::validate(owner.as_ref(), len)?;
        Ok(Self {
            pointer,
            len,
            words,
            _owner: Box::new(owner),
        })
    }

    fn validate(bytes: &[u8], len: usize) -> Result<(*const u8, u32, u32)> {
        if len >= (1 << 29) {
            return Err(Error::failed("external data exceeds wire limit".into()));
        }
        let padded = len.div_ceil(8) * 8;
        if !(bytes.as_ptr() as usize).is_multiple_of(8) || padded > bytes.len() {
            return Err(Error::failed(
                "external data requires aligned, word-padded storage".into(),
            ));
        }
        if bytes[len..padded].iter().any(|b| *b != 0) {
            return Err(Error::failed("external data padding must be zero".into()));
        }
        Ok((
            bytes.as_ptr(),
            u32::try_from(len).unwrap(),
            u32::try_from(padded / 8).unwrap(),
        ))
    }

    pub(crate) fn parts(&self) -> (*const u8, u32, u32) {
        (self.pointer, self.len, self.words)
    }
}
