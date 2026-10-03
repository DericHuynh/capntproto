//! Capability RPC over TCP/TLS and standard quiche QUIC, with whole-entry storage.
// capnpc emits generic reflection helpers whose type parameter is unused.
#[allow(clippy::extra_unused_type_parameters)]
#[cfg(feature = "storage")]
pub mod store_capnp {
    include!(concat!(env!("OUT_DIR"), "/store_capnp.rs"));
}
pub mod authority;
#[cfg(all(feature = "native", feature = "storage"))]
pub mod handoff;
#[cfg(all(feature = "native", feature = "storage"))]
pub mod introduction;
#[cfg(feature = "native")]
pub mod nat;
#[cfg(feature = "native")]
pub mod native_arbitration;
#[cfg(feature = "native")]
pub mod native_discovery;
#[cfg(feature = "native")]
pub mod native_listener;
mod object_ids;
#[allow(clippy::extra_unused_type_parameters)]
#[cfg(feature = "native")]
pub mod native_discovery_capnp {
    include!(concat!(env!("OUT_DIR"), "/native_discovery_capnp.rs"));
}
#[cfg(feature = "native")]
pub mod native_provisioning;
#[allow(clippy::extra_unused_type_parameters)]
#[cfg(feature = "native")]
pub mod native_provisioning_capnp {
    include!(concat!(env!("OUT_DIR"), "/native_provisioning_capnp.rs"));
}
#[cfg(feature = "native")]
pub mod native_rpc;
#[cfg(feature = "native")]
pub mod native_shutdown;
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
#[cfg(feature = "native")]
pub mod transport;

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
#[cfg(all(feature = "services", feature = "native"))]
pub mod realtime_datagram;
#[allow(clippy::extra_unused_type_parameters)]
#[cfg(feature = "services")]
pub mod realtime_capnp {
    include!(concat!(env!("OUT_DIR"), "/realtime_capnp.rs"));
}
