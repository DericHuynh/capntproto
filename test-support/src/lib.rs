//! Generated fixtures for integration tests and interoperability tools.
/// The same wire schema with affine server reply signatures and field editors.
pub mod structured {
    #[allow(clippy::extra_unused_type_parameters)]
    pub mod runtime_test_capnp {
        include!(concat!(
            env!("OUT_DIR"),
            "/structured/runtime_test_capnp.rs"
        ));
    }
    pub mod rpc_api_capnp {
        include!(concat!(env!("OUT_DIR"), "/structured/rpc_api_capnp.rs"));
    }
}
pub mod rpc_api_capnp {
    include!(concat!(env!("OUT_DIR"), "/rpc_api_capnp.rs"));
}
#[allow(clippy::extra_unused_type_parameters)] // Generated test schema helpers.
pub mod runtime_test_capnp {
    include!(concat!(env!("OUT_DIR"), "/runtime_test_capnp.rs"));
}

#[allow(clippy::extra_unused_type_parameters)]
pub mod cancellation_policy_capnp {
    include!(concat!(env!("OUT_DIR"), "/cancellation_policy_capnp.rs"));
}

#[allow(clippy::extra_unused_type_parameters)] // Generated generic schema helpers.
pub mod dynamic_test_capnp {
    include!(concat!(env!("OUT_DIR"), "/dynamic_test_capnp.rs"));
}

#[allow(clippy::extra_unused_type_parameters)] // Generated generic annotation helpers.
pub mod presence_capnp {
    include!(concat!(env!("OUT_DIR"), "/presence_capnp.rs"));
}
pub mod conversion_capnp {
    include!(concat!(env!("OUT_DIR"), "/conversion_capnp.rs"));
}

#[allow(clippy::extra_unused_type_parameters)]
#[deny(unreachable_patterns)] // Diamond dispatch must not emit duplicate interface arms.
pub mod reflection_lookup_capnp {
    include!(concat!(env!("OUT_DIR"), "/reflection_lookup_capnp.rs"));
}

#[allow(clippy::extra_unused_type_parameters)]
pub mod enum_brand_capnp {
    include!(concat!(env!("OUT_DIR"), "/enum_brand_capnp.rs"));
}

#[allow(clippy::extra_unused_type_parameters)] // Generated generic annotation helpers.
pub mod native_list_capnp {
    include!(concat!(env!("OUT_DIR"), "/native_list_capnp.rs"));
}

#[allow(clippy::extra_unused_type_parameters)]
pub mod native_rpc_capnp {
    include!(concat!(env!("OUT_DIR"), "/native_rpc_capnp.rs"));
}

pub mod membrane_copy_capnp {
    include!(concat!(env!("OUT_DIR"), "/membrane_copy_capnp.rs"));
}

#[allow(clippy::extra_unused_type_parameters)]
pub mod field_api_capnp {
    include!(concat!(env!("OUT_DIR"), "/field_api_capnp.rs"));
}

pub mod schedules;
pub mod traces;
pub mod verification;
