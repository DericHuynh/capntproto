//! Allocation-free RPC state transitions shared by hosted and embedded drivers.
//!
//! Extraction is in progress: this crate currently owns streaming credit and
//! adaptive-window accounting, question return/cancellation lifecycles and
//! peer-chosen answer/import tables and local question/export/embargo allocation
//! with bounded or hosted storage, plus application handoff guards and checked
//! wire capability-reference accounting with explicit cleanup acceptance and
//! bounded, scoped import/export ownership with stale-key rejection, local call
//! leases and transactional export publication, plus caller-backed immutable
//! message leases, ordered resumable sends, atomic incoming descriptor batches,
//! caller-polled IO and scoped question/answer ownership with retained pipelines.
//! It is not yet a complete RPC implementation.
//! Callers own IO, clocks, scheduling and storage for outstanding work.
#![no_std]
#![forbid(unsafe_code)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod answers;
pub mod clients;
pub mod completion;
pub mod connection;
pub mod exports;
pub mod flow_control;
pub mod handoff;
pub mod ids;
pub mod imports;
pub mod io;
pub mod local_table;
pub mod messages;
#[cfg(feature = "alloc")]
pub mod observation;
pub mod peer_table;
pub mod promise;
pub mod provision;
pub mod question;
pub mod questions;
#[cfg(all(target_has_atomic = "ptr", target_has_atomic = "8"))]
pub mod readiness;
pub mod references;
#[doc(hidden)]
pub mod storage;
pub mod waiting;
#[doc(hidden)]
pub mod work_queue;

#[cfg(test)]
mod async_scopes;
#[cfg(kani)]
mod proofs;

pub mod embargoes;
