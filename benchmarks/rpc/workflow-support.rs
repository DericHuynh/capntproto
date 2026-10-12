//! Shared by the benchmark runner and the independent artifact validator.
pub const VERSION: u32 = 3;
pub const IMPLEMENTATIONS: [&str; 5] = ["legacy", "portable", "capnp-cpp", "grpc", "websocket"];
pub fn supported(implementation: &str, case: &str) -> bool {
    IMPLEMENTATIONS.contains(&implementation)
        && (case != "pipelined-read"
            || matches!(implementation, "legacy" | "portable" | "capnp-cpp"))
}
