//! Shared numeric domains for object authority and persistent factory selection.
//! These are checked metadata, not capability or authenticated identity proofs.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::{
    fmt,
    num::{NonZeroU64, TryFromIntError},
};

macro_rules! identifier {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(NonZeroU64);

        impl $name {
            /// Validate an explicit numeric boundary. Zero is invalid; all
            /// other u64 values are preserved without allocating authority.
            #[must_use]
            pub const fn new(value: u64) -> Option<Self> {
                match NonZeroU64::new(value) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0.get()
            }
        }
        impl TryFrom<u64> for $name {
            type Error = TryFromIntError;
            fn try_from(value: u64) -> Result<Self, Self::Error> {
                NonZeroU64::try_from(value).map(Self)
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

#[cfg(feature = "storage")]
identifier!(
    ObjectKind,
    "Realm-local factory kind. Distinct from object IDs and generations."
);
identifier!(ObjectId, "A nonzero object authority ID. This is not a revision, factory kind, or proof of store ownership.");
identifier!(ObjectGeneration, "A nonzero object generation chosen by the trusted host. This is not a route generation or a store revision, and does not itself enforce freshness.");
