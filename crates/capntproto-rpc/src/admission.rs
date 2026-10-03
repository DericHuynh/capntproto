//! Local outgoing call admission. This does not limit message allocation or
//! application calls queued on a purely local unresolved capability.

use std::{cell::Cell, rc::Rc};

pub(crate) struct Admission {
    pub(crate) limit: Cell<usize>,
    pub(crate) used: Cell<usize>,
}

impl Admission {
    pub(crate) fn new(limit: usize) -> Rc<Self> {
        Rc::new(Self {
            limit: Cell::new(limit),
            used: Cell::new(0),
        })
    }

    pub(crate) fn reserve(self: &Rc<Self>) -> capnp::Result<Rc<Permit>> {
        let used = self.used.get();
        if used >= self.limit.get() {
            return Err(capnp::Error::overloaded(
                "outgoing RPC call limit reached; call was not sent".into(),
            ));
        }
        self.used.set(used + 1);
        Ok(Rc::new(Permit(self.clone())))
    }
}

/// Shared by the request, question table and local write completion. Retaining
/// any one of them keeps the reservation, including a canceled call whose
/// Return has not arrived, or a pipeline-only call blocked in the writer.
pub(crate) struct Permit(Rc<Admission>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.used.set(self.0.used.get() - 1);
    }
}

/// Exact table occupancy at the instant the local executor takes a snapshot.
/// Counts are objects, not heap bytes. IDs identify local connection instances.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConnectionSnapshot {
    pub connection_id: usize,
    pub outgoing_call_limit: usize,
    /// Reservations, including unsent requests and writes awaiting completion.
    pub outgoing_calls: usize,
    /// Includes bootstrap, Join and other protocol questions, not only Calls.
    pub questions: usize,
    pub answers: usize,
    pub imports: usize,
    pub exports: usize,
    pub embargoes: usize,
    /// Incoming call words charged until their result contexts are released.
    pub incoming_call_words: usize,
    /// Received wire response contexts still retained by promises/pipelines or
    /// application readers. Clones sharing one context count once.
    pub held_responses: usize,
}

/// Diagnostics retain only a weak registry reference, never an RPC driver.
#[derive(Clone)]
pub struct RpcDiagnostics(pub(crate) Rc<dyn Fn() -> Vec<ConnectionSnapshot>>);
impl RpcDiagnostics {
    /// Active connections only, sorted by local connection ID. An empty result
    /// is also returned after the system is dropped.
    pub fn snapshot(&self) -> Vec<ConnectionSnapshot> {
        (self.0)()
    }
}
