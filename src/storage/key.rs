//! Explicit storage address domain. Keys identify slots, not capability authority.
#![forbid(unsafe_code)]

use crate::authority::ObjectId;
use std::fmt;

/// Store-local object key, distinct from revisions and authority generations.
/// Every u64, including zero, is valid. Keys are not branded to a Store instance.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ObjectKey(u64);
impl ObjectKey {
    /// Import an explicit numeric storage address; no authority is allocated.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}
impl From<ObjectId> for ObjectKey {
    fn from(value: ObjectId) -> Self {
        Self(value.get())
    }
}
impl fmt::Display for ObjectKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
