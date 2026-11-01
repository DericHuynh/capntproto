//! Shared state tables, completions, credit and allocation-free message IO for legacy RPC.
#![no_std]
#![forbid(unsafe_code)]
#[cfg(feature = "alloc")]
extern crate alloc;
pub mod completion;
pub mod embargoes;
pub mod flow_control;
pub mod handoff;
pub mod ids;
pub mod io;
pub mod local_table;
pub mod messages;
#[cfg(feature = "alloc")]
pub mod observation;
pub mod peer_table;
pub mod promise;
pub mod provision;
pub mod question;
#[cfg(all(target_has_atomic = "ptr", target_has_atomic = "8"))]
pub mod ready_queue;
pub mod references;
pub mod storage;
