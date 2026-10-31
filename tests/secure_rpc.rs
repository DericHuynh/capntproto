#![cfg(feature = "tls")]

#[path = "secure_rpc/common.rs"]
mod common;
#[cfg(feature = "quic")]
#[path = "secure_rpc/quic.rs"]
mod quic_tests;
#[path = "secure_rpc/tls.rs"]
mod tls_tests;

#[path = "secure_rpc/unbuffered.rs"]
mod unbuffered;

#[path = "secure_rpc/rustls_direct.rs"]
mod rustls_direct;

#[cfg(all(feature = "monoio", any(target_os = "linux", target_os = "macos")))]
#[path = "secure_rpc/driven.rs"]
mod driven_tests;
#[cfg(feature = "embedded-tls")]
#[path = "secure_rpc/embedded.rs"]
mod embedded_tests;
#[cfg(feature = "webtransport")]
#[path = "secure_rpc/webtransport.rs"]
mod webtransport_tests;
