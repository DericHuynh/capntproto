//! Trusted host selection of an upload journal's storage slot.
#![forbid(unsafe_code)]
use crate::storage::ObjectKey;
use std::fmt;

/// Store-local upload journal ID. Zero is valid, as for any Store key.
/// This is not an object authority ID, revision, reservation or bearer token.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct JournalId(ObjectKey);
impl JournalId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(ObjectKey::new(value))
    }
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
    /// Explicit projection into the storage address domain.
    #[must_use]
    pub const fn key(self) -> ObjectKey {
        self.0
    }
}
impl fmt::Display for JournalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
