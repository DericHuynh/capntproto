//! Cap'n Proto messages and legacy RPC over Tokio TCP with rustls TLS 1.3.
//!
//! [`capnp`] provides messages and generated capability types; [`capnp_rpc`]
//! provides protocol drivers. [`rpc::Connection`] owns a running connection and
//! cancels it on drop. Storage and services are optional.
//!
//! Start with `message_basic`, then run the separate `server` and `client`
//! binaries in the [key/value service example](https://github.com/DericHuynh/capntproto/tree/main/examples/kv).
//! Its `deep-client` binary exercises concurrent directory capability pipelines.

pub use capnp;
pub use capnp_futures;
pub use capnp_rpc;
// capnpc emits generic reflection helpers whose type parameter is unused.
#[allow(clippy::extra_unused_type_parameters)]
#[cfg(feature = "storage")]
pub mod store_capnp {
    include!(concat!(env!("OUT_DIR"), "/store_capnp.rs"));
}
pub mod authority;
mod object_ids;

#[cfg(feature = "storage")]
pub mod orm;
#[cfg(feature = "storage")]
pub mod persistence;
#[allow(clippy::extra_unused_type_parameters)]
#[cfg(feature = "storage")]
pub mod persistence_capnp {
    include!(concat!(env!("OUT_DIR"), "/persistence_capnp.rs"));
}
pub mod rpc;
pub mod semantics;
#[cfg(feature = "storage")]
pub mod storage;

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod unix_rpc;

#[cfg(feature = "services")]
pub mod schema_exchange;
#[allow(clippy::extra_unused_type_parameters)]
#[cfg(feature = "services")]
pub mod schema_exchange_capnp {
    include!(concat!(env!("OUT_DIR"), "/schema_exchange_capnp.rs"));
}

#[cfg(feature = "services")]
pub mod bulk;
#[cfg(all(feature = "services", feature = "storage"))]
pub mod durable_bulk;
#[allow(clippy::extra_unused_type_parameters)]
#[cfg(feature = "services")]
pub mod bulk_capnp {
    include!(concat!(env!("OUT_DIR"), "/bulk_capnp.rs"));
}
#[cfg(feature = "services")]
pub mod realtime;
#[allow(clippy::extra_unused_type_parameters)]
#[cfg(feature = "services")]
pub mod realtime_capnp {
    include!(concat!(env!("OUT_DIR"), "/realtime_capnp.rs"));
}
