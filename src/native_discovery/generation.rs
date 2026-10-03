//! Discovery metadata has its own nonzero numeric domain.
#![forbid(unsafe_code)]

use std::num::NonZeroU64;

/// A directory publication's compare-and-replace generation.
///
/// Generations are scoped to the issuing directory. Importing a number does
/// not establish ownership, freshness or authority; publication/revocation
/// still compare it against that directory's current entry. Separate directory
/// instances can issue the same number.
#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DiscoveryGeneration(NonZeroU64);

impl DiscoveryGeneration {
    /// Explicitly import wire/checkpoint metadata. Zero means absence and is
    /// represented by `None` at administrative compare-and-replace boundaries.
    pub fn new(value: u64) -> Option<Self> {
        NonZeroU64::new(value).map(Self)
    }

    pub fn get(self) -> u64 {
        self.0.get()
    }

    pub(super) fn after(last: u64) -> capnp::Result<Self> {
        last.checked_add(1)
            .and_then(Self::new)
            .ok_or_else(|| capnp::Error::overloaded("Native discovery generation exhausted".into()))
    }
}
