//! Hosted policy handles. The incoming counter itself is allocation-free.
use super::*;
use alloc::string::String;
use capntproto_rpc_engine::flow_control::incoming::{Incoming, OwnedCharge};

type TraceEncoder = Rc<dyn Fn(&Error) -> String>;
pub(crate) struct Settings {
    pub(crate) outgoing_limit: Cell<usize>,
    pub(crate) flow_limit: Cell<usize>,
    pub(crate) trace: RefCell<Option<TraceEncoder>>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            outgoing_limit: Cell::new(usize::MAX),
            flow_limit: Cell::new(usize::MAX),
            trace: RefCell::default(),
        }
    }
}

/// Live incoming-flow policy for one coordinator. Polling and IO stay with the
/// caller. Changes wake a parked reader; callbacks execute without cell borrows.
pub struct Policy {
    pub(crate) admission: Rc<crate::admission::Admission>,
    pub(crate) tables: RefCell<Option<Tables>>,
    settings: Rc<Settings>,
    incoming: Rc<Incoming>,
    wake: RefCell<Option<Waker>>,
}
impl Policy {
    pub fn new() -> Rc<Self> {
        Self::with_settings(Rc::default())
    }
    pub(crate) fn with_settings(settings: Rc<Settings>) -> Rc<Self> {
        Rc::new(Self {
            admission: crate::admission::Admission::new(settings.outgoing_limit.get()),
            tables: RefCell::default(),
            incoming: Rc::new(Incoming::new(settings.flow_limit.get())),
            settings,
            wake: RefCell::default(),
        })
    }
    pub fn incoming_call_words(&self) -> usize {
        self.incoming.words()
    }
    pub fn flow_limit(&self) -> usize {
        self.settings.flow_limit.get()
    }
    pub fn set_flow_limit(&self, words: usize) {
        self.settings.flow_limit.set(words);
        self.notify();
    }
    pub fn set_trace_encoder(&self, encoder: impl Fn(&Error) -> String + 'static) {
        let previous = self.settings.trace.borrow_mut().replace(Rc::new(encoder));
        drop(previous);
    }
    pub fn clear_trace_encoder(&self) {
        let previous = self.settings.trace.borrow_mut().take();
        drop(previous);
    }
    pub(crate) fn notify(&self) {
        let wake = self.wake.borrow_mut().take();
        if let Some(wake) = wake {
            wake.wake();
        }
    }
    pub(super) fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<()> {
        self.incoming.set_limit(self.settings.flow_limit.get());
        if self.incoming.paused() {
            let old = self.wake.borrow_mut().replace(cx.waker().clone());
            drop(old);
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
    pub(crate) fn trace(&self, error: &Error) -> Option<String> {
        let encoder = self.settings.trace.borrow().clone();
        encoder.map(|encoder| encoder(error))
    }
    pub(super) fn charge(self: &Rc<Self>, words: usize) -> capnp::Result<Option<Charge>> {
        if words == 0 {
            return Ok(None);
        }
        self.incoming.set_limit(self.settings.flow_limit.get());
        let credit = self.incoming.charge_owned(words).map_err(wire_error)?;
        Ok(Some(Charge {
            credit: Some(credit),
            policy: self.clone(),
        }))
    }
    pub(super) fn close(&self) {
        let wake = self.wake.borrow_mut().take();
        drop(wake);
    }
}

pub(crate) struct Tables {
    pub(crate) questions: capntproto_rpc_engine::observation::Observation,
    pub(crate) answers: capntproto_rpc_engine::observation::Observation,
    pub(crate) imports: capntproto_rpc_engine::observation::Observation,
    pub(crate) exports: capntproto_rpc_engine::observation::Observation,
    pub(crate) embargoes: capntproto_rpc_engine::observation::Observation,
}
impl Policy {
    pub(crate) fn snapshot(&self, connection_id: usize) -> crate::ConnectionSnapshot {
        let tables = self.tables.borrow();
        crate::ConnectionSnapshot {
            connection_id,
            outgoing_call_limit: self.admission.limit(),
            outgoing_calls: self.admission.used(),
            questions: tables.as_ref().map_or(0, |t| t.questions.get()),
            answers: tables.as_ref().map_or(0, |t| t.answers.get()),
            imports: tables.as_ref().map_or(0, |t| t.imports.get()),
            exports: tables.as_ref().map_or(0, |t| t.exports.get()),
            embargoes: tables.as_ref().map_or(0, |t| t.embargoes.get()),
            incoming_call_words: self.incoming_call_words(),
            held_responses: self.admission.held_responses.get(),
        }
    }
}
pub(in crate::scoped::wire) struct Charge {
    credit: Option<OwnedCharge>,
    policy: Rc<Policy>,
}
impl Drop for Charge {
    fn drop(&mut self) {
        drop(self.credit.take());
        self.policy.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::Ordering, Arc};
    #[derive(Default)]
    struct WakeCount(std::sync::atomic::AtomicUsize);
    impl std::task::Wake for WakeCount {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[test]
    fn retained_results_release_credit_on_return_and_wake_without_dropping_pipelines() {
        let policy = Policy::new();
        policy.set_flow_limit(4);
        let results = server::Results::with_charge(policy.charge(5).unwrap(), None);
        let retained = results.clone();
        let wake = Arc::new(WakeCount::default());
        let waker = Waker::from(wake.clone());
        let mut cx = Context::from_waker(&waker);
        assert!(policy.poll_ready(&mut cx).is_pending());
        assert_eq!(policy.incoming_call_words(), 5);
        results.returned();
        assert_eq!(wake.0.load(Ordering::SeqCst), 1);
        assert!(policy.poll_ready(&mut cx).is_ready());
        retained.returned();
        assert_eq!(policy.incoming_call_words(), 0);
        let unadmitted = policy.charge(8).unwrap();
        drop(unadmitted);
        assert_eq!(policy.incoming_call_words(), 0);
    }
}
