//! Small dependency boundary for native and interpreted memory-safety tests.
#[allow(clippy::extra_unused_type_parameters)]
pub mod field_api_capnp {
    include!(concat!(env!("OUT_DIR"), "/field_api_capnp.rs"));
}
