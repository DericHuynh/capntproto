//! Verify production no_std transitions; no copied execution model.
#![allow(dead_code)]
#![feature(proc_macro_hygiene)]
#[path = "../../crates/capntproto-rpc-engine/src/lib.rs"]
mod engine;
pub use engine::*;
fn main() {}

#[path = "../../src/semantics/sequence.rs"]
mod sequence;
#[path = "../../src/semantics/stream.rs"]
mod stream;
#[path = "../../src/semantics/revisions.rs"]
mod revisions;
#[path = "../../crates/capntproto-rpc-engine/src/storage/limits.rs"]
mod limits;
#[path = "../../crates/capntproto-core/src/private/arena/fixed/limits.rs"]
mod fixed;
#[path = "../../crates/capntproto-core/src/wire_capability/map/policy.rs"]
mod indices;
