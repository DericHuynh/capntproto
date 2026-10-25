//! Canonical hosted collections and an allocator-only hash backend.
pub(crate) use alloc::collections::{btree_map, BTreeMap, VecDeque};
#[cfg(not(feature = "std"))]
pub(crate) use hashbrown::{hash_map, HashMap, HashSet};
#[cfg(feature = "std")]
pub(crate) use std::collections::{hash_map, HashMap, HashSet};
