//! A recoverable checkout saga over three independent, durable RPC services.
mod client;
mod faults;
mod service;
mod store;
use capntproto::capnp_rpc::twoparty::RpcDriver;
pub use capntproto::{capnp, capnp_rpc};
use clap::ValueEnum;
pub use client::{Client, ResourceClient, Wire};
pub use faults::{FaultAction, FaultPlan, FaultStage};
use serde::{Deserialize, Serialize};
pub use service::{serve, NodeConfig, Role};
pub use store::{Account, Audit, Bucket, Claim, Config, Entry, OrderRow, Snapshot, State, Status};
pub mod checkout_capnp {
    include!(concat!(env!("OUT_DIR"), "/checkout_capnp.rs"));
}
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub const RPC_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Driver {
    Legacy,
    Portable,
}
impl Driver {
    pub fn protocol(self) -> capnp::Result<RpcDriver> {
        Ok(match self {
            Self::Legacy => RpcDriver::Legacy,
            Self::Portable => RpcDriver::Portable(capnp_rpc::portable::Options::bounded(
                4.try_into().unwrap(),
                1024.try_into().unwrap(),
                (1024 * 1024).try_into().unwrap(),
                (4 * 1024 * 1024).try_into().unwrap(),
            )?),
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BusinessError {
    Invalid,
    Conflict,
    NotFound,
    Unauthorized,
    Transport(String),
}
impl std::fmt::Display for BusinessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for BusinessError {}
impl From<capnp::Error> for BusinessError {
    fn from(error: capnp::Error) -> Self {
        let text = error.to_string();
        for (tag, kind) in [
            ("invalid:", Self::Invalid),
            ("conflict:", Self::Conflict),
            ("not found:", Self::NotFound),
            ("unauthorized:", Self::Unauthorized),
        ] {
            if text.contains(tag) {
                return kind;
            }
        }
        Self::Transport(text)
    }
}
fn error(text: impl Into<String>) -> capnp::Error {
    capnp::Error::failed(text.into())
}
fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
}
pub fn quote(quantity: u32) -> capnp::Result<u64> {
    if !(1..=8).contains(&quantity) {
        return Err(error("invalid: quantity must be between 1 and 8"));
    }
    Ok(u64::from(quantity) * if quantity >= 3 { 900 } else { 1000 })
}
