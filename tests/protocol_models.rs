// Native Cargo entry points for every bounded model and negative control.
use capntproto_test_support::verification::{catalog, verify};

#[test]
fn tlc_storage_recovery_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "storage_recovery_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_route_lifecycle_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "route_lifecycle_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_schema_loader_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "schema_loader_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_multiparty_join_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "multiparty_join_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_answer_adoption_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "answer_adoption_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_caller_pipeline_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "caller_pipeline_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_two_party_join_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "two_party_join_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_membrane_join_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "membrane_join_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_storage_compaction_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "storage_compaction_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_orm_history_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "orm_history_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_persistence_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "persistence_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_persistence_expiry_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "persistence_expiry_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_generic_parameters_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "generic_parameters_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_dynamic_orphans_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "dynamic_orphans_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_orphan_access_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "orphan_access_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_orphan_concat_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "orphan_concat_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_external_data_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "external_data_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_arena_resize_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "arena_resize_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_orphan_groups_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "orphan_groups_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_native_arbitration_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "native_arbitration_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_native_provisioning_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "native_provisioning_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_native_listener_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "native_listener_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_native_shutdown_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "native_shutdown_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_realtime_datagram_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "realtime_datagram_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_realtime_fragments_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "realtime_fragments_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_bulk_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "bulk_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_durable_bulk_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "durable_bulk_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_realtime_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "realtime_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_schema_exchange_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "schema_exchange_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_native_routes_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "native_routes_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_native_multiparty_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "native_multiparty_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_field_owners_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "field_owners_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_field_entry_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "field_entry_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_field_values_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "field_values_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_field_ownership_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "field_ownership_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_field_group_staging_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "field_group_staging_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_disconnect_cleanup_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "disconnect_cleanup_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_idle_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "idle_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_deferred_handoff_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "deferred_handoff_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_cancellation_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "cancellation_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_tail_adoption_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "tail_adoption_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_tail_transfer_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "tail_transfer_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_bootstrap_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "bootstrap_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_incoming_flow_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "incoming_flow_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_reconnect_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "reconnect_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_streaming_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "streaming_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_membrane_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "membrane_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_exception_trace_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "exception_trace_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_import_alias_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "import_alias_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_revoked_policy_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "revoked_policy_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_membrane_transform_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "membrane_transform_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_fd_framing_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "fd_framing_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_queued_streaming_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "queued_streaming_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_exception_disconnect_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "exception_disconnect_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_exception_metadata_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "exception_metadata_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_call_hints_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "call_hints_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_incoming_hints_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "incoming_hints_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_static_cancellation_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "static_cancellation_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_call_executor_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "call_executor_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_schema_hints_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "schema_hints_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_fd_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "fd_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_revocable_server_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "revocable_server_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_revocable_streaming_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "revocable_streaming_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_server_set_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "server_set_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_server_hooks_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "server_hooks_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_self_capability_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "self_capability_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_shorten_lookup_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "shorten_lookup_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_dynamic_capability_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "dynamic_capability_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_dynamic_ownership_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "dynamic_ownership_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_dynamic_brand_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "dynamic_brand_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_wire_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "wire_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_guard_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "guard_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "full composed state-space exploration; cargo test --test protocol_models tlc_protocol_reference -- --ignored --exact"]
fn tlc_protocol_reference() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "protocol_reference")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn tlc_runtime_reference() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "runtime_reference")
            .unwrap(),
    )
    .unwrap();
}
