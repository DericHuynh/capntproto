//! Network-local route identifiers. Allocation never wraps or reuses a value.
#![forbid(unsafe_code)]

use std::{cell::Cell, num::NonZeroU64};

/// Opaque generation of a route within one [`super::Network`].
///
/// Only the network allocates these values. They are diagnostic identities, not
/// authentication evidence. Compare them only within the same network; separate
/// networks have independent counters. Route cleanup still checks owner identity.
///
/// ```compile_fail,E0308
/// use capntproto::native_rpc::RouteGeneration;
/// let generation: RouteGeneration = 1u64;
/// ```
/// ```compile_fail,E0423
/// use capntproto::native_rpc::RouteGeneration;
/// let generation = RouteGeneration(std::num::NonZeroU64::new(1).unwrap());
/// ```
/// ```compile_fail,E0308
/// fn count_is_not_generation(generation: capntproto::native_rpc::RouteGeneration) {
///     let byte_count: u64 = generation;
/// }
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct RouteGeneration(NonZeroU64);
impl RouteGeneration {
    /// Numeric projection for diagnostics and trace encoding. It cannot be used
    /// to construct a generation or authorize access to a route.
    #[must_use]
    pub fn get(self) -> u64 {
        self.0.get()
    }
}
impl std::fmt::Display for RouteGeneration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

pub(super) struct Generations {
    next: Cell<Option<RouteGeneration>>,
}
impl Generations {
    pub(super) fn new() -> Self {
        Self {
            next: Cell::new(Some(RouteGeneration(NonZeroU64::MIN))),
        }
    }
    pub(super) fn allocate(&self) -> capnp::Result<RouteGeneration> {
        let generation = self
            .next
            .get()
            .ok_or_else(|| capnp::Error::overloaded("Native route generation exhausted".into()))?;
        self.next.set(
            generation
                .get()
                .checked_add(1)
                .and_then(NonZeroU64::new)
                .map(RouteGeneration),
        );
        Ok(generation)
    }
}

#[cfg(test)]
pub(super) fn fixture(value: u64) -> RouteGeneration {
    RouteGeneration(NonZeroU64::new(value).unwrap())
}

#[cfg(test)]
mod tests;
