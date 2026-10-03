#![cfg(feature = "tls")]

#[path = "secure_rpc/common.rs"]
mod common;
#[cfg(feature = "quic")]
#[path = "secure_rpc/quic.rs"]
mod quic_tests;
#[path = "secure_rpc/tls.rs"]
mod tls_tests;
