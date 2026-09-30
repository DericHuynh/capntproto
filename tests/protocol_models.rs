// Native Cargo entry points for every bounded model and negative control.
use reproto_test_support::verification::{catalog, verify};

#[test]
fn storage_recovery_model() {
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
fn route_lifecycle_model() {
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
fn schema_loader_model() {
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
fn multiparty_join_model() {
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
fn answer_adoption_model() {
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
fn caller_pipeline_model() {
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
fn two_party_join_model() {
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
fn membrane_join_model() {
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
fn storage_compaction_model() {
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
fn orm_history_model() {
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
fn persistence_model() {
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
fn persistence_expiry_model() {
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
fn generic_parameters_model() {
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
fn dynamic_orphans_model() {
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
fn orphan_access_model() {
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
fn orphan_concat_model() {
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
fn external_data_model() {
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
fn arena_resize_model() {
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
fn orphan_groups_model() {
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
fn noise_arbitration_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "noise_arbitration_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn noise_provisioning_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "noise_provisioning_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn noise_listener_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "noise_listener_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn noise_shutdown_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "noise_shutdown_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn realtime_datagram_model() {
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
fn realtime_fragments_model() {
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
fn bulk_model() {
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
fn durable_bulk_model() {
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
fn realtime_model() {
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
fn schema_exchange_model() {
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
fn noise_routes_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "noise_routes_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn noise_multiparty_model() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "noise_multiparty_model")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn field_owners_model() {
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
fn field_entry_model() {
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
fn field_values_model() {
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
fn field_ownership_model() {
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
fn field_group_staging_model() {
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
fn disconnect_cleanup_model() {
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
fn idle_model() {
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
fn deferred_handoff_model() {
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
fn cancellation_model() {
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
fn tail_adoption_model() {
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
fn tail_transfer_model() {
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
fn bootstrap_model() {
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
fn incoming_flow_model() {
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
fn reconnect_model() {
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
fn streaming_model() {
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
fn membrane_model() {
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
fn exception_trace_model() {
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
fn import_alias_model() {
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
fn revoked_policy_model() {
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
fn membrane_transform_model() {
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
fn fd_framing_model() {
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
fn queued_streaming_model() {
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
fn exception_disconnect_model() {
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
fn exception_metadata_model() {
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
fn call_hints_model() {
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
fn incoming_hints_model() {
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
fn static_cancellation_model() {
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
fn call_executor_model() {
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
fn schema_hints_model() {
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
fn fd_model() {
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
fn revocable_server_model() {
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
fn revocable_streaming_model() {
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
fn server_set_model() {
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
fn server_hooks_model() {
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
fn self_capability_model() {
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
fn shorten_lookup_model() {
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
fn dynamic_capability_model() {
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
fn dynamic_ownership_model() {
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
fn dynamic_brand_model() {
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
fn wire_model() {
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
fn guard_model() {
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
#[ignore = "full composed state-space exploration; cargo test --test protocol_models protocol_reference -- --ignored --exact"]
fn protocol_reference() {
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
fn runtime_reference() {
    verify(
        catalog()
            .groups
            .iter()
            .find(|g| g.id == "runtime_reference")
            .unwrap(),
    )
    .unwrap();
}
