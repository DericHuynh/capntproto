//! Published snapshot coordinates and bytes cannot be relabeled after delivery.
#![forbid(unsafe_code)]

use std::rc::Rc;

/// An immutable value published by a realtime receiver. Retaining it preserves
/// the coordinates and data even when newer values replace it or the stream
/// closes. Its deadline describes admission/publication, not a read-time lease.
#[must_use]
#[derive(Clone, Debug)]
pub struct Snapshot {
    sequence: u64,
    key: u32,
    not_after: u64,
    bytes: Rc<[u8]>,
}
impl Snapshot {
    pub(super) fn new(sequence: u64, key: u32, not_after: u64, bytes: &[u8]) -> Self {
        Self {
            sequence,
            key,
            not_after,
            bytes: bytes.into(),
        }
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn key(&self) -> u32 {
        self.key
    }
    pub fn not_after(&self) -> u64 {
        self.not_after
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
