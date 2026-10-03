//! Optional, executor-neutral adapters for the pinned C++ compatibility protocols.
#![forbid(unsafe_code)]

pub mod byte_stream;
mod format;
pub mod http;
pub mod json;
pub mod json_rpc;
pub mod text;
pub mod websocket;

#[allow(clippy::extra_unused_type_parameters)]
pub mod byte_stream_capnp {
    include!(concat!(env!("OUT_DIR"), "/byte_stream_capnp.rs"));
}
#[allow(clippy::extra_unused_type_parameters)]
pub mod http_over_capnp_capnp {
    include!(concat!(env!("OUT_DIR"), "/http_over_capnp_capnp.rs"));
}
#[allow(clippy::extra_unused_type_parameters)]
pub mod json_capnp {
    include!(concat!(env!("OUT_DIR"), "/json_capnp.rs"));
}

fn invalid(message: impl Into<String>) -> capnp::Error {
    capnp::Error::failed(message.into())
}
