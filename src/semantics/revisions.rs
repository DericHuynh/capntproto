//! Object-local revision counters and checked revision-state transitions.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::fmt;

/// An object-local revision counter, distinct from object IDs and generations.
/// Zero is the initial counter (no revision), never a stored entry. Importing a
/// number does not establish existence, publication, object binding or authority.
/// Store operations check those conditions separately.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Revision(u64);
impl Revision {
    pub const INITIAL: Self = Self(0);
    pub const MAX: Self = Self(u64::MAX);

    /// Explicit wire/disk/checkpoint import. Every counter value is representable.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
    /// Advance without wrapping; the last representable revision remains usable.
    #[must_use = "handle revision exhaustion before writing"]
    pub fn checked_next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}
impl fmt::Display for Revision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Checked revision state: publication never exceeds the staged head.
/// This is transition metadata, not proof that an entry exists in a Store.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[must_use]
pub struct Revisions {
    head: Revision,
    published: Revision,
}
impl Default for Revisions {
    fn default() -> Self {
        Self {
            head: Revision::INITIAL,
            published: Revision::INITIAL,
        }
    }
}
impl Revisions {
    #[must_use = "handle invalid publication bounds"]
    pub fn new(head: Revision, published: Revision) -> Option<Self> {
        (published <= head).then_some(Self { head, published })
    }
    #[must_use]
    pub fn head(self) -> Revision {
        self.head
    }
    #[must_use]
    pub fn published(self) -> Revision {
        self.published
    }
    #[must_use = "use the next state or handle the rejected write"]
    pub fn stage(self, expected: Revision) -> Option<Self> {
        if self.head != expected {
            return None;
        }
        Some(Self {
            head: self.head.checked_next()?,
            ..self
        })
    }
    #[must_use = "use the next state or handle the rejected publication"]
    pub fn publish(self, revision: Revision, expected: Revision) -> Option<Self> {
        if expected != self.published || revision <= expected || revision > self.head {
            return None;
        }
        Some(Self {
            published: revision,
            ..self
        })
    }
}
