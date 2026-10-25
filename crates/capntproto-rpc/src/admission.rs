//! Local outgoing call admission. This does not limit message allocation or
//! application calls queued on a purely local unresolved capability.

use capntproto_rpc_engine::flow_control::outgoing::Outgoing;
pub(crate) use capntproto_rpc_engine::flow_control::outgoing::OwnedPermit as Permit;
use std::rc::Rc;

pub(crate) fn retain_context(
    slot: &mut Option<Rc<dyn std::any::Any>>,
    context: Rc<dyn std::any::Any>,
) {
    if slot.as_ref().is_some_and(|old| Rc::ptr_eq(old, &context)) {
        return;
    }
    *slot = Some(match slot.take() {
        None => context,
        Some(old) => Rc::new((old, context)),
    });
}

pub(crate) struct Admission {
    counter: Rc<Outgoing>,
    pub(crate) held_responses: std::cell::Cell<usize>,
}

impl Admission {
    pub(crate) fn new(limit: usize) -> Rc<Self> {
        Rc::new(Self {
            counter: Rc::new(Outgoing::new(limit)),
            held_responses: std::cell::Cell::new(0),
        })
    }

    pub(crate) fn reserve(self: &Rc<Self>) -> capnp::Result<Rc<Permit>> {
        self.counter.reserve_owned().map(Rc::new).map_err(|_| {
            capnp::Error::overloaded("outgoing RPC call limit reached; call was not sent".into())
        })
    }
    pub(crate) fn limit(&self) -> usize {
        self.counter.limit()
    }
    pub(crate) fn used(&self) -> usize {
        self.counter.used()
    }
    pub(crate) fn set_limit(&self, limit: usize) {
        self.counter.set_limit(limit);
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
