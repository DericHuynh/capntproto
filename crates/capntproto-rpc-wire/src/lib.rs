//! RPC wire schemas and bounded input/output admission; no allocator is required.
//! This layer shares its generated wire types with the hosted RPC runtime.
//! Typed call/streaming codecs, pipeline transforms and caller-polled framing
//! compose with the engine's scoped ownership tables and FIFO output.
//! Owning requests and replies bind frames to receipts across task suspension.
//! The owning connection driver coordinates dispatch, cleanup and reflected
//! loopback ordering. Applications still supply IO, execution and storage;
//! tail/third-party orchestration and the hosted ownership migration are ongoing.
#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

mod buffer;
pub use buffer::Capacity;

capnp::generated_code!(pub mod rpc_capnp);
capnp::generated_code!(pub mod rpc_twoparty_capnp);

pub mod admission;
pub mod attachments;
pub mod calls;
pub mod clients;
pub mod inbound;
pub mod io;
pub mod network;
pub mod owned;
pub mod pipeline;
pub mod publication;
pub mod reception;

#[cfg(test)]
mod call_owners;
#[cfg(test)]
mod request_owners;
mod resources;
#[cfg(test)]
mod result_owners;
