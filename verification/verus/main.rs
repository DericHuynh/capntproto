//! Verify the actual no_std implementation, without rewriting its bodies.
#![allow(dead_code)]
#[path = "../../crates/capntproto-rpc-engine/src/handoff.rs"]
mod handoff;
fn main() {}

#[path = "../../crates/capntproto-rpc-engine/src/question.rs"]
mod question;
