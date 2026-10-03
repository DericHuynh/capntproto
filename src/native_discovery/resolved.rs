//! Immutable evidence of a decoded directory response, not an authenticated peer.
#![forbid(unsafe_code)]

use super::{Binding, DiscoveryGeneration};
use tokio::time::Instant;

/// A short-lived lookup result. Re-resolve names for subsequent sessions to
/// follow administrative rotation. The binding and expiry cannot be replaced
/// after validation. Connection setup rechecks recipient and expiry and still
/// requires a successful Native handshake with the pinned host.
#[must_use]
pub struct Resolved {
    binding: Binding,
    generation: DiscoveryGeneration,
    expires: Instant,
}

impl Resolved {
    pub(super) fn new(binding: Binding, generation: DiscoveryGeneration, expires: Instant) -> Self {
        Self {
            binding,
            generation,
            expires,
        }
    }

    pub fn binding(&self) -> &Binding {
        &self.binding
    }

    pub fn generation(&self) -> DiscoveryGeneration {
        self.generation
    }

    /// Conservative local deadline measured from the lookup's request start.
    pub fn expires(&self) -> Instant {
        self.expires
    }

    pub(super) fn into_parts(self) -> (Binding, Instant) {
        (self.binding, self.expires)
    }
}
