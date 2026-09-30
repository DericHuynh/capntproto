//! Pending-message diagnostics. Tickets contain no messages or descriptors;
//! starting, rejecting or canceling a write removes exactly its own entry.
use capnp_rpc::twoparty::{QueueDiagnostics, QueueSnapshot};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Instant,
};

#[derive(Default)]
struct State {
    next: u64,
    entries: BTreeMap<u64, (usize, Instant)>,
}
#[derive(Clone, Default)]
pub(super) struct Metrics(Arc<Mutex<State>>);
pub(super) struct Pending {
    state: Arc<Mutex<State>>,
    id: u64,
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.state.lock().unwrap().entries.remove(&self.id);
    }
}
impl Metrics {
    pub(super) fn enqueue(&self, bytes: usize) -> capnp::Result<Pending> {
        let mut state = self.0.lock().unwrap();
        let id = state.next;
        state.next = id
            .checked_add(1)
            .ok_or_else(|| capnp::Error::overloaded("Unix queue sequence exhausted".into()))?;
        state.entries.insert(id, (bytes, Instant::now()));
        Ok(Pending {
            state: self.0.clone(),
            id,
        })
    }
    pub(super) fn observer(&self) -> QueueDiagnostics {
        let state = self.0.clone();
        QueueDiagnostics::new(move || {
            let state = state.lock().unwrap();
            QueueSnapshot {
                message_count: state.entries.len(),
                bytes: state.entries.values().map(|(bytes, _)| bytes).sum(),
                wait_time: state
                    .entries
                    .first_key_value()
                    .map(|(_, (_, time))| time.elapsed())
                    .unwrap_or_default(),
            }
        })
    }
}
