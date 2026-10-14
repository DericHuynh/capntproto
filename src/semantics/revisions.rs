//! Object-local revision counters and checked revision-state transitions.
#![forbid(unsafe_code)]

#[cfg(not(capntproto_verus))]
use serde::{Deserialize, Serialize};
#[cfg(capntproto_verus)]
use vstd::prelude::*;
#[cfg(capntproto_verus)]
include!("verified/revisions.rs");
use std::fmt;

/// An object-local revision counter, distinct from object IDs and generations.
/// Zero is the initial counter (no revision), never a stored entry. Importing a
/// number does not establish existence, publication, object binding or authority.
/// Store operations check those conditions separately.
#[cfg_attr(capntproto_verus, verus_verify)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[cfg_attr(
    not(capntproto_verus),
    derive(Serialize, Deserialize),
    serde(transparent)
)]
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
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == value(self)))]
    pub const fn get(self) -> u64 {
        self.0
    }
    /// Advance without wrapping; the last representable revision remains usable.
    #[must_use = "handle revision exhaustion before writing"]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures successor(self,r)))]
    pub fn checked_next(self) -> Option<Self> {
        Some(Self(self.0.checked_add(1)?))
    }
}
impl fmt::Display for Revision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Checked revision state: publication never exceeds the staged head.
/// This is transition metadata, not proof that an entry exists in a Store.
#[cfg_attr(capntproto_verus, verus_verify)]
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
#[cfg_attr(capntproto_verus, verus_spec(r => ensures construct(head,published,r)))]
fn checked(head: Revision, published: Revision) -> Option<Revisions> {
    if published.0 <= head.0 {
        Some(Revisions { head, published })
    } else {
        None
    }
}
impl Revisions {
    #[must_use = "handle invalid publication bounds"]
    pub fn new(head: Revision, published: Revision) -> Option<Self> {
        checked(head, published)
    }
    #[must_use]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == head_spec(self)))]
    pub fn head(self) -> Revision {
        self.head
    }
    #[must_use]
    #[cfg_attr(capntproto_verus, verus_spec(r => ensures r == published_spec(self)))]
    pub fn published(self) -> Revision {
        self.published
    }
    #[must_use = "use the next state or handle the rejected write"]
    #[cfg_attr(capntproto_verus, verus_spec(r => requires valid(self), ensures stage_contract(self, expected, r)))]
    pub fn stage(self, expected: Revision) -> Option<Self> {
        if self.head.0 != expected.0 {
            return None;
        }
        Some(Self {
            head: self.head.checked_next()?,
            ..self
        })
    }
    #[must_use = "use the next state or handle the rejected publication"]
    #[cfg_attr(capntproto_verus, verus_spec(r => requires valid(self), ensures publish_contract(self, revision, expected, r)))]
    pub fn publish(self, revision: Revision, expected: Revision) -> Option<Self> {
        if expected.0 != self.published.0 || revision.0 <= expected.0 || revision.0 > self.head.0 {
            return None;
        }
        Some(Self {
            published: revision,
            ..self
        })
    }
}
