//! Pure production transition guards, also exercised by the finite graph explorer.
#![forbid(unsafe_code)]
mod handoff;
mod revisions;
mod stream;
pub use handoff::{HandoffPhase, HandoffState};
pub use revisions::{Revision, Revisions};
pub use stream::{NativeStreamGate, NativeStreamState, StreamRole};
