//! Cap'n Proto codec and legacy RPC fuzz harnesses.
#![forbid(unsafe_code)]

pub mod framing;
pub mod pointers;
pub mod rpc_lifecycle;
pub mod schema;
mod wire;

pub mod lifecycle_capnp {
    include!(concat!(env!("OUT_DIR"), "/lifecycle_capnp.rs"));
}

/// Bound the stateful fuzz workload independently of fuzzer input size.
pub const MAX_INPUT: usize = 4096;
