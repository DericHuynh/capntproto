use alloc::vec::Vec;
use core::slice;

use crate::error::Error;

use super::{HEADER_SIZE, MAX_PAYLOAD, MessageError, PlainMessage};
use crate::enums::{ContentType, ProtocolVersion};
use crate::msgs::base::Payload;
use crate::msgs::codec::{Codec, Reader};
use crate::record_layer::RecordLayer;

/// A TLS message with encoded (but not necessarily encrypted) payload.
#[derive(Clone, Debug)]
pub struct EncodedMessage<P> {
    /// The content type of this message.
    pub typ: ContentType,
    /// The protocol version of this message.
    pub version: ProtocolVersion,
    /// The payload of this message.
    pub payload: P,
}

impl<P> EncodedMessage<P> {
    /// Create a new `EncodedMessage` with the given fields.
    pub fn new(typ: ContentType, version: ProtocolVersion, payload: P) -> Self {
        Self {
            typ,
            version,
            payload,
        }
    }
}

/// A TLS frame, named `TLSPlaintext` in the standard.
///
/// This outbound type borrows its "to be encrypted" payload from the "user".
/// It is used for fragmenting and is consumed by encryption.
#[derive(Debug)]
pub struct OutboundPlainMessage<'a> {
    pub typ: ContentType,
    pub version: ProtocolVersion,
    pub payload: OutboundChunks<'a>,
}

impl OutboundPlainMessage<'_> {
    pub(crate) fn encoded_len(&self, record_layer: &RecordLayer) -> usize {
        HEADER_SIZE + record_layer.encrypted_len(self.payload.len())
    }

    pub(crate) fn to_unencrypted_opaque(&self) -> OutboundOpaqueMessage {
        let mut payload = PrefixedPayload::with_capacity(self.payload.len());
        payload.extend_from_chunks(&self.payload);
        OutboundOpaqueMessage {
            version: self.version,
            typ: self.typ,
            payload,
        }
    }
}

/// A collection of borrowed plaintext slices.
///
/// Warning: OutboundChunks does not guarantee that the simplest variant is used.
/// Multiple can hold non fragmented or empty payloads.
#[derive(Debug, Clone)]
pub enum OutboundChunks<'a> {
    /// A single byte slice. Contrary to `Multiple`, this uses a single pointer indirection
    Single(&'a [u8]),
    /// A collection of chunks (byte slices)
    /// and cursors to single out a fragmented range of bytes.
    /// OutboundChunks assumes that start <= end
    Multiple {
        chunks: &'a [&'a [u8]],
        start: usize,
        end: usize,
    },
}

impl<'a> OutboundChunks<'a> {
    /// Create a payload from a slice of byte slices.
    /// If fragmented the cursors are added by default: start = 0, end = length
    pub fn new(chunks: &'a [&'a [u8]]) -> Self {
        if chunks.len() == 1 {
            Self::Single(chunks[0])
        } else {
            Self::Multiple {
                chunks,
                start: 0,
                end: chunks
                    .iter()
                    .map(|chunk| chunk.len())
                    .sum(),
            }
        }
    }

    /// Create a payload with a single empty slice
    pub fn new_empty() -> Self {
        Self::Single(&[])
    }

    /// Flatten the slice of byte slices to an owned vector of bytes
    pub fn to_vec(&self) -> Vec<u8> {
        let mut vec = Vec::with_capacity(self.len());
        self.copy_to_vec(&mut vec);
        vec
    }

    /// Append all bytes to a vector
    pub fn copy_to_vec(&self, vec: &mut Vec<u8>) {
        match *self {
            Self::Single(chunk) => vec.extend_from_slice(chunk),
            Self::Multiple { chunks, start, end } => {
                let mut size = 0;
                for chunk in chunks.iter() {
                    let psize = size;
                    let len = chunk.len();
                    size += len;
                    if size <= start || psize >= end {
                        continue;
                    }
                    let start = start.saturating_sub(psize);
                    let end = if end - psize < len { end - psize } else { len };
                    vec.extend_from_slice(&chunk[start..end]);
                }
            }
        }
    }

    /// Iterate over the selected non-empty plaintext slices.
    pub fn chunks(&self) -> impl Iterator<Item = &[u8]> + '_ {
        match self {
            Self::Single(chunk) => Chunks::Single((!chunk.is_empty()).then_some(*chunk)),
            Self::Multiple { chunks, start, end } => Chunks::Multiple {
                chunks: chunks.iter(),
                skip: *start,
                remaining: end - start,
            },
        }
    }

    /// Split self in two, around an index
    /// Works similarly to `split_at` in the core library, except it doesn't panic if out of bound
    pub fn split_at(&self, mid: usize) -> (Self, Self) {
        match *self {
            Self::Single(chunk) => {
                let mid = Ord::min(mid, chunk.len());
                (Self::Single(&chunk[..mid]), Self::Single(&chunk[mid..]))
            }
            Self::Multiple { chunks, start, end } => {
                let mid = Ord::min(start + mid, end);
                (
                    Self::Multiple {
                        chunks,
                        start,
                        end: mid,
                    },
                    Self::Multiple {
                        chunks,
                        start: mid,
                        end,
                    },
                )
            }
        }
    }

    /// Returns true if the payload is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the cumulative length of all chunks
    pub fn len(&self) -> usize {
        match self {
            Self::Single(chunk) => chunk.len(),
            Self::Multiple { start, end, .. } => end - start,
        }
    }
}

impl<'a> From<&'a [u8]> for OutboundChunks<'a> {
    fn from(payload: &'a [u8]) -> Self {
        Self::Single(payload)
    }
}

/// A TLS frame, named `TLSPlaintext` in the standard.
///
/// This outbound type owns all memory for its interior parts.
/// It results from encryption and is used for io write.
#[derive(Clone, Debug)]
pub struct OutboundOpaqueMessage {
    pub typ: ContentType,
    pub version: ProtocolVersion,
    pub payload: PrefixedPayload,
}

impl OutboundOpaqueMessage {
    /// Construct a new `OpaqueMessage` from constituent fields.
    ///
    /// `body` is moved into the `payload` field.
    pub fn new(typ: ContentType, version: ProtocolVersion, payload: PrefixedPayload) -> Self {
        Self {
            typ,
            version,
            payload,
        }
    }

    /// Construct by decoding from a [`Reader`].
    ///
    /// `MessageError` allows callers to distinguish between valid prefixes (might
    /// become valid if we read more data) and invalid data.
    pub fn read(r: &mut Reader<'_>) -> Result<Self, MessageError> {
        let (typ, version, len) = read_opaque_message_header(r)?;

        let content = r
            .take(len as usize)
            .ok_or(MessageError::TooShortForLength)?;

        Ok(Self {
            typ,
            version,
            payload: PrefixedPayload::from(content),
        })
    }

    pub fn encode(self) -> Vec<u8> {
        let length = self.payload.len() as u16;
        let mut encoded_payload = self.payload.0;
        encoded_payload[0] = self.typ.into();
        encoded_payload[1..3].copy_from_slice(&self.version.to_array());
        encoded_payload[3..5].copy_from_slice(&(length).to_be_bytes());
        encoded_payload
    }

    /// Force conversion into a plaintext message.
    ///
    /// This should only be used for messages that are known to be in plaintext. Otherwise, the
    /// `OutboundOpaqueMessage` should be decrypted into a `PlainMessage` using a `MessageDecrypter`.
    pub fn into_plain_message(self) -> PlainMessage {
        PlainMessage {
            version: self.version,
            typ: self.typ,
            payload: Payload::Owned(self.payload.as_ref().to_vec()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct PrefixedPayload(Vec<u8>);

impl PrefixedPayload {
    pub fn with_capacity(capacity: usize) -> Self {
        let mut prefixed_payload = Vec::with_capacity(HEADER_SIZE + capacity);
        prefixed_payload.resize(HEADER_SIZE, 0);
        Self(prefixed_payload)
    }

    pub fn extend_from_slice(&mut self, slice: &[u8]) {
        self.0.extend_from_slice(slice)
    }

    pub fn extend_from_chunks(&mut self, chunks: &OutboundChunks<'_>) {
        chunks.copy_to_vec(&mut self.0)
    }

    pub fn truncate(&mut self, len: usize) {
        self.0.truncate(len + HEADER_SIZE)
    }

    fn len(&self) -> usize {
        self.0.len() - HEADER_SIZE
    }
}

impl AsRef<[u8]> for PrefixedPayload {
    fn as_ref(&self) -> &[u8] {
        &self.0[HEADER_SIZE..]
    }
}

impl AsMut<[u8]> for PrefixedPayload {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0[HEADER_SIZE..]
    }
}

impl<'a> Extend<&'a u8> for PrefixedPayload {
    fn extend<T: IntoIterator<Item = &'a u8>>(&mut self, iter: T) {
        self.0.extend(iter)
    }
}

impl From<&[u8]> for PrefixedPayload {
    fn from(content: &[u8]) -> Self {
        let mut payload = Vec::with_capacity(HEADER_SIZE + content.len());
        payload.extend(&[0u8; HEADER_SIZE]);
        payload.extend(content);
        Self(payload)
    }
}

impl<const N: usize> From<&[u8; N]> for PrefixedPayload {
    fn from(content: &[u8; N]) -> Self {
        Self::from(&content[..])
    }
}

pub(crate) fn read_opaque_message_header(
    r: &mut Reader<'_>,
) -> Result<(ContentType, ProtocolVersion, u16), MessageError> {
    let typ = ContentType::read(r).map_err(|_| MessageError::TooShortForHeader)?;
    // Don't accept any new content-types.
    if let ContentType::Unknown(_) = typ {
        return Err(MessageError::InvalidContentType);
    }

    let version = ProtocolVersion::read(r).map_err(|_| MessageError::TooShortForHeader)?;
    // Accept only versions 0x03XX for any XX.
    match &version {
        ProtocolVersion::Unknown(v) if (v & 0xff00) != 0x0300 => {
            return Err(MessageError::UnknownProtocolVersion);
        }
        _ => {}
    };

    let len = u16::read(r).map_err(|_| MessageError::TooShortForHeader)?;

    // Reject undersize messages
    //  implemented per section 5.1 of RFC8446 (TLSv1.3)
    //              per section 6.2.1 of RFC5246 (TLSv1.2)
    if typ != ContentType::ApplicationData && len == 0 {
        return Err(MessageError::InvalidEmptyPayload);
    }

    // Reject oversize messages
    if len >= MAX_PAYLOAD {
        return Err(MessageError::MessageTooLarge);
    }

    Ok((typ, version, len))
}

#[cfg(test)]
mod tests {
    use std::{println, vec};

    use super::*;

    #[test]
    fn split_at_with_single_slice() {
        let owner: &[u8] = &[0, 1, 2, 3, 4, 5, 6, 7];
        let borrowed_payload = OutboundChunks::Single(owner);

        let (before, after) = borrowed_payload.split_at(6);
        println!("before:{before:?}\nafter:{after:?}");
        assert_eq!(before.to_vec(), &[0, 1, 2, 3, 4, 5]);
        assert_eq!(after.to_vec(), &[6, 7]);
    }

    #[test]
    fn split_at_with_multiple_slices() {
        let owner: Vec<&[u8]> = vec![&[0, 1, 2, 3], &[4, 5], &[6, 7, 8], &[9, 10, 11, 12]];
        let borrowed_payload = OutboundChunks::new(&owner);

        let (before, after) = borrowed_payload.split_at(3);
        println!("before:{before:?}\nafter:{after:?}");
        assert_eq!(before.to_vec(), &[0, 1, 2]);
        assert_eq!(after.to_vec(), &[3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);

        let (before, after) = borrowed_payload.split_at(8);
        println!("before:{before:?}\nafter:{after:?}");
        assert_eq!(before.to_vec(), &[0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(after.to_vec(), &[8, 9, 10, 11, 12]);

        let (before, after) = borrowed_payload.split_at(11);
        println!("before:{before:?}\nafter:{after:?}");
        assert_eq!(before.to_vec(), &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(after.to_vec(), &[11, 12]);
    }

    #[test]
    fn split_out_of_bounds() {
        let owner: Vec<&[u8]> = vec![&[0, 1, 2, 3], &[4, 5], &[6, 7, 8], &[9, 10, 11, 12]];

        let single_payload = OutboundChunks::Single(owner[0]);
        let (before, after) = single_payload.split_at(17);
        println!("before:{before:?}\nafter:{after:?}");
        assert_eq!(before.to_vec(), &[0, 1, 2, 3]);
        assert!(after.is_empty());

        let multiple_payload = OutboundChunks::new(&owner);
        let (before, after) = multiple_payload.split_at(17);
        println!("before:{before:?}\nafter:{after:?}");
        assert_eq!(before.to_vec(), &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        assert!(after.is_empty());

        let empty_payload = OutboundChunks::new_empty();
        let (before, after) = empty_payload.split_at(17);
        println!("before:{before:?}\nafter:{after:?}");
        assert!(before.is_empty());
        assert!(after.is_empty());
    }

    #[test]
    fn empty_slices_mixed() {
        let owner: Vec<&[u8]> = vec![&[], &[], &[0], &[], &[1, 2], &[], &[3], &[4], &[], &[]];
        let mut borrowed_payload = OutboundChunks::new(&owner);
        let mut fragment_count = 0;
        let mut fragment;
        let expected_fragments: &[&[u8]] = &[&[0, 1], &[2, 3], &[4]];

        while !borrowed_payload.is_empty() {
            (fragment, borrowed_payload) = borrowed_payload.split_at(2);
            println!("{fragment:?}");
            assert_eq!(&expected_fragments[fragment_count], &fragment.to_vec());
            fragment_count += 1;
        }
        assert_eq!(fragment_count, expected_fragments.len());
    }

    #[test]
    fn exhaustive_splitting() {
        let owner: Vec<u8> = (0..127).collect();
        let slices = (0..7)
            .map(|i| &owner[((1 << i) - 1)..((1 << (i + 1)) - 1)])
            .collect::<Vec<_>>();
        let payload = OutboundChunks::new(&slices);

        assert_eq!(payload.to_vec(), owner);
        println!("{payload:#?}");

        for start in 0..128 {
            for end in start..128 {
                for mid in 0..(end - start) {
                    let witness = owner[start..end].split_at(mid);
                    let split_payload = payload
                        .split_at(end)
                        .0
                        .split_at(start)
                        .1
                        .split_at(mid);
                    assert_eq!(
                        witness.0,
                        split_payload.0.to_vec(),
                        "start: {start}, mid:{mid}, end:{end}"
                    );
                    assert_eq!(
                        witness.1,
                        split_payload.1.to_vec(),
                        "start: {start}, mid:{mid}, end:{end}"
                    );
                }
            }
        }
    }
}

/// Iterator over an [`OutboundChunks`]'s chunks, returned by [`OutboundChunks::chunks()`].
enum Chunks<'a> {
    Single(Option<&'a [u8]>),
    Multiple {
        /// Chunks not yet visited, including any leading ones `skip` covers.
        chunks: slice::Iter<'a, &'a [u8]>,
        /// How many leading bytes remain to be skipped.
        skip: usize,
        /// How many bytes remain to be yielded.
        remaining: usize,
    },
}

impl<'a> Iterator for Chunks<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        let (chunks, skip, remaining) = match self {
            Self::Single(chunk) => return chunk.take(),
            Self::Multiple {
                chunks,
                skip,
                remaining,
            } => (chunks, skip, remaining),
        };

        loop {
            if *remaining == 0 {
                return None;
            }

            let chunk = chunks.next()?;
            if *skip > chunk.len() {
                *skip -= chunk.len();
                continue;
            }
            let chunk = &chunk[*skip..];

            *skip = 0;
            if chunk.is_empty() {
                continue;
            }

            let take = Ord::min(chunk.len(), *remaining);
            *remaining -= take;
            return Some(&chunk[..take]);
        }
    }
}

/// A fixed-size buffer into which a [`MessageEncrypter`][] writes an encrypted message payload.
///
/// This wraps the output buffer passed to [`MessageEncrypter::encrypt()`][], tracking how
/// much of it has been written as the append methods fill it front-to-back. It writes
/// into caller-owned memory and cannot grow. [`Self::new()`] checks that the caller's
/// buffer can hold the `len` bytes the encrypter declared, and the append methods then
/// panic if the writes exceed that length.
///
/// Such a panic always indicates a bug in the `MessageEncrypter` implementation, not a
/// runtime condition the caller can handle. The same implementation declares the total
/// length up front (via [`MessageEncrypter::encrypted_payload_len()`]) and performs the
/// writes, so overflowing the buffer means the two disagree. The record layer also
/// relies on that declared length for framing, so there is no way to recover from the
/// mismatch after the fact.
///
/// [`MessageEncrypter`]: crate::crypto::cipher::MessageEncrypter
/// [`MessageEncrypter::encrypt()`]: crate::crypto::cipher::MessageEncrypter::encrypt()
/// [`MessageEncrypter::encrypted_payload_len()`]: crate::crypto::cipher::MessageEncrypter::encrypted_payload_len()
pub struct EncryptBuffer<'a> {
    buf: &'a mut [u8],
    used: usize,
}

impl<'a> EncryptBuffer<'a> {
    /// Wrap the first `len` bytes of `out`, all of which are as yet unwritten.
    ///
    /// Returns [`Error::EncryptError`] if `out` is shorter than `len` bytes.
    pub fn new(out: &'a mut [u8], len: usize) -> Result<Self, Error> {
        match out.get_mut(..len) {
            Some(buf) => Ok(Self { buf, used: 0 }),
            None => Err(Error::EncryptError),
        }
    }

    /// Append bytes from an `OutboundChunks`'s chunks.
    ///
    /// Panics if the write would extend beyond the `len` given to [`Self::new()`],
    /// which indicates a bug in the calling `MessageEncrypter` implementation (see
    /// the type-level documentation).
    pub fn extend_from_chunks(&mut self, chunks: &OutboundChunks<'_>) {
        match chunks {
            // for the common case with a single chunk we want to avoid iteration overhead.
            OutboundChunks::Single(chunk) => self.extend_from_slice(chunk),
            chunks => {
                for chunk in chunks.chunks() {
                    self.extend_from_slice(chunk);
                }
            }
        }
    }

    /// Append bytes from a slice.
    ///
    /// Panics if the write would extend beyond the `len` given to [`Self::new()`],
    /// which indicates a bug in the calling `MessageEncrypter` implementation (see
    /// the type-level documentation).
    pub fn extend_from_slice(&mut self, slice: &[u8]) {
        self.buf[self.used..self.used + slice.len()].copy_from_slice(slice);
        self.used += slice.len();
    }

    /// Consume this value, returning the written prefix of the wrapped buffer.
    pub fn into_written(self) -> &'a [u8] {
        &self.buf[..self.used]
    }
}

impl AsMut<[u8]> for EncryptBuffer<'_> {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.buf[..self.used]
    }
}

pub(crate) fn encode_record_header(
    typ: ContentType,
    version: ProtocolVersion,
    len: u16,
) -> [u8; HEADER_SIZE] {
    let [version_hi, version_lo] = version.to_array();
    let [len_hi, len_lo] = len.to_be_bytes();
    [typ.into(), version_hi, version_lo, len_hi, len_lo]
}

#[cfg(test)]
mod upstream_encrypt_buffer_tests {
    use alloc::vec;

    use super::*;

    #[test]
    fn encrypt_buffer_appends() {
        let mut space = [0u8; 8];
        let mut buf = EncryptBuffer::new(&mut space[..], 6).unwrap();
        buf.extend_from_slice(&[1, 2]);
        buf.extend_from_chunks(&OutboundChunks::new(&[&[3u8, 4][..], &[5][..]]));
        buf.extend_from_slice(&[6]);
        assert_eq!(buf.as_mut(), &mut [1, 2, 3, 4, 5, 6]);
        assert_eq!(buf.into_written(), &[1, 2, 3, 4, 5, 6]);
        assert_eq!(space, [1, 2, 3, 4, 5, 6, 0, 0]);
    }

    #[test]
    fn encrypt_buffer_rejects_short_buffer() {
        let mut space = [0u8; 4];
        assert!(matches!(
            EncryptBuffer::new(&mut space[..], 5),
            Err(Error::EncryptError)
        ));
    }

    #[test]
    fn chunks_iteration() {
        // `Single` yields its chunk, unless empty
        assert_eq!(
            OutboundChunks::Single(&[1, 2, 3])
                .chunks()
                .collect::<Vec<_>>(),
            [&[1u8, 2, 3][..]],
        );
        assert_eq!(
            OutboundChunks::new_empty()
                .chunks()
                .count(),
            0
        );

        // `Multiple` yields the in-window part of each chunk, skipping
        // empty chunks
        let owner: Vec<&[u8]> = vec![&[], &[1, 2, 3], &[], &[4, 5], &[], &[6, 7], &[]];
        let (_, tail) = OutboundChunks::new(&owner).split_at(1);
        let (window, _) = tail.split_at(5);
        assert_eq!(
            window.chunks().collect::<Vec<_>>(),
            [&[2u8, 3][..], &[4, 5][..], &[6][..]],
        );
    }
}
