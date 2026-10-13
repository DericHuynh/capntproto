//! Shared by the benchmark runner and the independent artifact validator.
pub const VERSION: u32 = 3;
pub const IMPLEMENTATIONS: [&str; 6] = [
    "legacy",
    "portable",
    "capnp-cpp",
    "grpc",
    "websocket",
    "capnp-rust",
];
pub fn supported(implementation: &str, case: &str) -> bool {
    IMPLEMENTATIONS.contains(&implementation)
        && (case != "pipelined-read"
            || matches!(
                implementation,
                "legacy" | "portable" | "capnp-cpp" | "capnp-rust"
            ))
}
