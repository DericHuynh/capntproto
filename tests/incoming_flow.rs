mod support;
use capnp::traits::HasTypeId;
use capnp_rpc::{
    rpc_capnp::{cap_descriptor, message, return_},
    Connection, RpcSystem,
};
use capntproto_test_support::runtime_test_capnp::harness;
use futures::{channel::oneshot, FutureExt};
use std::{cell::RefCell, collections::VecDeque, future::Future, pin::Pin, rc::Rc, time::Duration};
use support::{Endpoint, Hub};

#[derive(Default)]
struct Control {
    gates: VecDeque<oneshot::Receiver<()>>,
    started: usize,
    completed: [bool; 2],
    dropped: [bool; 2],
    held: Option<harness::PendingResults>,
    fail_first: bool,
}
struct Running(Rc<RefCell<Control>>, usize);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.borrow_mut().dropped[self.1] = true;
    }
}
struct Service(Rc<RefCell<Control>>);
impl harness::Server for Service {
    async fn pending(
        self: Rc<Self>,
        p: harness::PendingParams,
        r: harness::PendingResults,
    ) -> capnp::Result<()> {
        let (index, gate) = {
            let mut c = self.0.borrow_mut();
            let index = c.started;
            c.started += 1;
            (index, c.gates.pop_front().unwrap())
        };
        let _running = Running(self.0.clone(), index);
        let results = if p.get()?.get_hold_context() {
            self.0.borrow_mut().held = Some(r);
            None
        } else {
            Some(r)
        };
        drop(p); // Parameter release must not release flow credit.
        gate.await.unwrap();
        self.0.borrow_mut().completed[index] = true;
        if index == 0 && self.0.borrow().fail_first {
            return Err(capnp::Error::failed("test failure".into()));
        }
        let mut results = results.or_else(|| self.0.borrow_mut().held.take()).unwrap();
        let cap: harness::Client = capnp_rpc::new_client_from_rc(self.clone());
        results.get().set_cap(cap);
        Ok(())
    }
    async fn echo(
        self: Rc<Self>,
        _: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        r.get().set_value(7);
        Ok(())
    }
}
fn pending(m: message::Builder<'_>, id: u32, export: u32, hold: bool) {
    let mut call = m.init_call();
    call.set_question_id(id);
    call.set_interface_id(harness::Client::TYPE_ID);
    call.set_method_id(3);
    call.reborrow().init_target().set_imported_cap(export);
    call.init_params()
        .get_content()
        .init_as::<harness::pending_params::Builder>()
        .set_hold_context(hold);
}
async fn settle() {
    for _ in 0..32 {
        tokio::task::yield_now().await;
    }
}
struct Fixture {
    control: Rc<RefCell<Control>>,
    gates: [Option<oneshot::Sender<()>>; 2],
    system: Rc<RefCell<RpcSystem<u8>>>,
    task: tokio::task::JoinHandle<capnp::Result<()>>,
    peer: Endpoint,
    hub: Rc<RefCell<Hub>>,
    words: usize,
    disconnected: bool,
}
impl Fixture {
    async fn new(hold: bool) -> Self {
        let control = Rc::new(RefCell::new(Control::default()));
        let (tx1, rx1) = oneshot::channel();
        let (tx2, rx2) = oneshot::channel();
        control.borrow_mut().gates.extend([rx1, rx2]);
        let hub = Rc::new(RefCell::new(Hub::default()));
        let network = Hub::network(&hub, 1);
        let service: harness::Client = capnp_rpc::new_client(Service(control.clone()));
        let mut sample = capnp::message::Builder::new_default();
        pending(sample.init_root(), 1, 0, hold);
        let words = sample.size_in_words();
        assert!(words > 0);
        let mut system = RpcSystem::new(Box::new(network), Some(service.client));
        system.set_flow_limit(words);
        let system = Rc::new(RefCell::new(system));
        let driver = system.clone();
        let task = tokio::task::spawn_local(futures::future::poll_fn(move |cx| {
            Pin::new(&mut *driver.borrow_mut()).poll(cx)
        }));
        let mut peer = hub.borrow_mut().connect(2, 1);
        peer.send(|m| m.init_bootstrap().set_question_id(0));
        let response = peer.recv().await;
        let message::Return(ret) = response
            .get_body()
            .unwrap()
            .get_as::<message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        else {
            panic!()
        };
        let return_::Results(payload) = ret.unwrap().which().unwrap() else {
            panic!()
        };
        let cap_descriptor::SenderHosted(export) = payload
            .unwrap()
            .get_cap_table()
            .unwrap()
            .get(0)
            .which()
            .unwrap()
        else {
            panic!()
        };
        peer.send(|m| pending(m, 1, export, hold));
        settle().await;
        assert_eq!(control.borrow().started, 1);
        peer.send(|m| pending(m, 2, export, false));
        settle().await;
        // The second call crosses the threshold and is still dispatched.
        assert_eq!(control.borrow().started, 2);
        Self {
            control,
            gates: [Some(tx1), Some(tx2)],
            system,
            task,
            peer,
            hub,
            words,
            disconnected: false,
        }
    }
    fn limit(&self, units: usize) {
        self.system.borrow_mut().set_flow_limit(units * self.words);
    }
    fn finish(&mut self) {
        self.peer.send(|m| {
            let mut f = m.init_finish();
            f.set_question_id(1);
            f.set_require_early_cancellation_workaround(false);
        });
    }
    fn probe(&mut self) {
        self.peer.send(|m| m.init_bootstrap().set_question_id(50));
    }
    fn complete(&mut self, i: usize) {
        self.gates[i].take().unwrap().send(()).unwrap();
    }
    fn drop_context(&self) {
        let held = self.control.borrow_mut().held.take();
        drop(held);
    }
    async fn prepare_held_credit(&mut self) {
        self.limit(3);
        self.finish();
        settle().await;
        assert!(self.control.borrow().dropped[0]);
        assert!(self.drain().is_empty());
        self.limit(1);
        // A read already in progress is not canceled by lowering the limit.
        // Let that read complete; the next read must stop on retained credit.
        self.peer.send(|m| m.init_bootstrap().set_question_id(49));
        settle().await;
        assert_eq!(self.drain(), vec![(49, 1)]);
    }
    async fn disconnect(&mut self) {
        self.disconnected = true;
        let d = self.system.borrow().get_disconnector();
        d.await.unwrap();
        settle().await;
    }
    async fn cleanup(self) {
        self.task.abort();
        let _ = self.task.await;
        drop(self.system);
        let held = self.control.borrow_mut().held.take();
        drop(held);
    }
    fn drain(&mut self) -> Vec<(u32, u32)> {
        let mut messages = vec![];
        loop {
            let Some(input) = self.peer.receive_incoming_message().now_or_never() else {
                break;
            };
            let Some(input) = input.unwrap() else {
                break;
            };
            match input
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            {
                message::Return(r) => {
                    let r = r.unwrap();
                    let kind = match r.which().unwrap() {
                        return_::Results(_) => 1,
                        return_::Canceled(()) => 2,
                        return_::Exception(_) => 3,
                        _ => panic!("unexpected Return"),
                    };
                    messages.push((r.get_answer_id(), kind));
                }
                message::Abort(_) => assert!(self.disconnected, "unexpected connection Abort"),
                _ => panic!("unexpected message"),
            }
        }
        messages
    }
}
#[tokio::test(flavor = "current_thread")]
async fn incoming_flow_counts_until_return_and_resumes_strictly_below_limit() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let mut f = Fixture::new(false).await;
                f.probe();
                settle().await;
                assert!(f.drain().is_empty());
                f.complete(0);
                settle().await;
                assert_eq!(f.drain(), vec![(1, 1)]);
                // Equality is not enough to wake a paused reader.
                f.complete(1);
                settle().await;
                assert_eq!(f.drain(), vec![(2, 1), (50, 1)]);
                f.cleanup().await;
            })
            .await
            .unwrap();
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn incoming_flow_retains_context_credit_and_zero_limit_needs_raise() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let mut f = Fixture::new(true).await;
                f.limit(0);
                f.finish();
                f.probe();
                settle().await;
                assert!(f.drain().is_empty());
                f.limit(2);
                settle().await;
                assert!(!f.control.borrow().dropped[0]);
                f.limit(3);
                settle().await;
                assert!(f.control.borrow().dropped[0]);
                assert_eq!(f.drain(), vec![(50, 1)]);
                f.drop_context();
                settle().await;
                assert_eq!(f.drain(), vec![(1, 2)]);
                f.cleanup().await;
            })
            .await
            .unwrap();
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn incoming_flow_is_per_connection_and_disconnect_unblocks_shutdown() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let mut f = Fixture::new(true).await;
                let mut other = f.hub.borrow_mut().connect(3, 1);
                other.send(|m| m.init_bootstrap().set_question_id(0));
                let reply = other.recv().await;
                assert!(matches!(
                    reply
                        .get_body()
                        .unwrap()
                        .get_as::<message::Reader>()
                        .unwrap()
                        .which()
                        .unwrap(),
                    message::Return(_)
                ));
                assert_eq!(f.hub.borrow().provision_count(), 0);
                f.finish();
                f.probe();
                settle().await;
                assert!(f.drain().is_empty());
                f.disconnect().await;
                assert_eq!(f.control.borrow().dropped, [true, true]);
                f.drop_context();
                settle().await;
                assert!(f.drain().is_empty());
                f.cleanup().await;
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn canceled_held_context_keeps_reader_paused_until_its_return() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let mut f = Fixture::new(true).await;
                f.prepare_held_credit().await;
                f.probe();
                settle().await;
                assert!(f.drain().is_empty());
                f.complete(1);
                settle().await;
                assert_eq!(f.drain(), vec![(2, 1)]);
                f.drop_context();
                settle().await;
                assert_eq!(f.drain(), vec![(1, 2), (50, 1)]);
                f.cleanup().await;
            })
            .await
            .unwrap();
        })
        .await;
}

#[derive(serde::Deserialize, Debug)]
struct Step {
    action: String,
    state: [u32; 12],
}
#[derive(serde::Deserialize, Debug)]
struct Trace {
    hold: bool,
    precanceled: bool,
    fail_first: bool,
    steps: Vec<Step>,
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_incoming_flow_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_INCOMING_FLOW_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!traces.is_empty());
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(120), async {
                for trace in traces {
                    let mut f = Fixture::new(trace.hold).await;
                    f.control.borrow_mut().fail_first = trace.fail_first;
                    if trace.precanceled {
                        f.prepare_held_credit().await;
                    }
                    let mut returns = [0; 3];
                    for step in &trace.steps {
                        match step.action.as_str() {
                            "complete_first" => f.complete(0),
                            "complete_second" => f.complete(1),
                            "finish" => f.finish(),
                            "probe" => f.probe(),
                            "limit" => f.limit(step.state[5] as usize),
                            "drop_context" => f.drop_context(),
                            "disconnect" => f.disconnect().await,
                            other => panic!("unknown action {other}"),
                        }
                        settle().await;
                        for (question, kind) in f.drain() {
                            let idx = match question {
                                1 => 0,
                                2 => 1,
                                50 => 2,
                                other => panic!("unexpected Return {other}"),
                            };
                            assert_eq!(
                                returns[idx], 0,
                                "duplicate Return at {step:?} in {trace:?}"
                            );
                            returns[idx] = kind;
                        }
                        assert_eq!(
                            returns,
                            [step.state[7], step.state[8], u32::from(step.state[4] == 2)],
                            "wire state at {step:?} in {trace:?}"
                        );
                        let c = f.control.borrow();
                        assert_eq!(
                            c.completed,
                            [step.state[0] == 1, step.state[1] == 1],
                            "completion at {step:?} in {trace:?}"
                        );
                        assert_eq!(
                            c.dropped,
                            [step.state[0] != 0, step.state[1] != 0],
                            "task lifetime at {step:?} in {trace:?}"
                        );
                        assert_eq!(
                            c.held.is_some(),
                            step.state[2] != 0,
                            "held context at {step:?} in {trace:?}"
                        );
                    }
                    f.cleanup().await;
                }
            })
            .await
            .expect("incoming flow trace replay timed out");
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn exception_returns_release_incoming_flow_credit() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let mut f = Fixture::new(false).await;
                f.control.borrow_mut().fail_first = true;
                f.probe();
                f.complete(0);
                settle().await;
                assert_eq!(f.drain(), vec![(1, 3)]);
                f.complete(1);
                settle().await;
                assert_eq!(f.drain(), vec![(2, 1), (50, 1)]);
                f.cleanup().await;
            })
            .await
            .unwrap();
        })
        .await;
}
