//! Shared generated protocol and connection configuration for the service binaries.
pub mod connection;
pub mod key_value_capnp {
    include!(concat!(env!("OUT_DIR"), "/key_value_capnp.rs"));
}
