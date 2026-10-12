//! Pure production transition guards, also exercised by the finite graph explorer.
#![forbid(unsafe_code)]
mod revisions;
mod stream;
/// Portable production guards; authentication remains the hosted adapter's job.
///
/// ```compile_fail,E0277
/// let state: capntproto::semantics::HandoffState = serde_json::from_str("{}").unwrap();
/// ```
pub use capntproto_rpc_engine::handoff::{HandoffPhase, HandoffState};
pub use revisions::{Revision, Revisions};
pub use stream::{NativeStreamGate, NativeStreamState, StreamRole};
