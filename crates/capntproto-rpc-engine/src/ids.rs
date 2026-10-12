// Copyright (c) 2026 ReProto contributors
// Licensed under the MIT license; see LICENSE.

//! Connection-local identifier domains. A peer's question names our answer,
//! and a peer's export names our import. These IDs do not establish connection
//! identity or slot freshness; only the protocol lifecycle permits reuse.

use core::{fmt, hash::Hash};

mod sealed {
    pub trait Id {}
}

pub trait WireId: sealed::Id + Copy + Eq + Ord + Hash {
    fn from_wire(value: u32) -> Self;
    fn to_wire(self) -> u32;
}

macro_rules! ids {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
        pub struct $name(u32);

        impl sealed::Id for $name {}
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

/// Only locally allocated domains can use a recycling allocator.
pub trait LocalId: WireId {}
impl LocalId for QuestionId {}
impl LocalId for ExportId {}
impl LocalId for EmbargoId {}

/// Peer-chosen keys cannot be used by a local recycling allocator.
///
/// ```compile_fail
/// use capntproto_rpc_engine::ids::{PeerId, QuestionId, WireId};
/// fn peer_key<I: PeerId>(_: I) {}
/// peer_key(QuestionId::from_wire(0));
/// ```
pub trait PeerId: WireId {}
impl PeerId for AnswerId {}
impl PeerId for ImportId {}

impl QuestionId {
    pub fn is_pipeline_only(self) -> bool {
        self.0 & (1 << 31) != 0
    }
    pub fn is_adopted(self) -> bool {
        (1 << 30..1 << 31).contains(&self.0)
    }
}
