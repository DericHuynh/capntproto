// Copyright (c) 2026 ReProto contributors
// Licensed under the MIT license; see LICENSE.

//! Connection-local identifier domains. The wire uses u32 in both directions:
//! a peer's question names our answer, and a peer's export names our import.
//! Decode into the local domain at the message boundary, never by field name
//! alone. These types do not establish connection identity or slot freshness.
#![forbid(unsafe_code)]

use std::{fmt, hash::Hash};

pub(super) trait WireId: Copy + Eq + Ord + Hash {
    fn from_wire(value: u32) -> Self;
    fn to_wire(self) -> u32;
}

macro_rules! ids {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
        pub(super) struct $name(u32);

        impl WireId for $name {
            fn from_wire(value: u32) -> Self { Self(value) }
            fn to_wire(self) -> u32 { self.0 }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    )+};
}

ids!(QuestionId, AnswerId, ExportId, ImportId, EmbargoId);

/// Only locally allocated domains can use the recycling allocator.
pub(super) trait LocalId: WireId {}
impl LocalId for QuestionId {}
impl LocalId for ExportId {}
impl LocalId for EmbargoId {}

/// IDs supplied by the peer are stored sparsely and never locally allocated.
pub(super) trait PeerId: WireId {}
impl PeerId for AnswerId {}
impl PeerId for ImportId {}

impl QuestionId {
    pub(super) fn is_pipeline_only(self) -> bool {
        self.0 & (1 << 31) != 0
    }
    pub(super) fn is_adopted(self) -> bool {
        (1 << 30..1 << 31).contains(&self.0)
    }
}
