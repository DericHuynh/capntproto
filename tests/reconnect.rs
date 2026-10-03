#[allow(dead_code)]
mod support;
use capnp::{capability::Promise, Error, ErrorKind};
use capnp_rpc::{RpcSystem, SetTarget};
use capntproto_test_support::runtime_test_capnp::harness;
use futures::channel::oneshot;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};
use support::Hub;

struct Server {
    identity: u32,
    calls: Rc<Cell<usize>>,
    gates: RefCell<Vec<Option<oneshot::Receiver<()>>>>,
    disconnected: bool,
}
impl harness::Server for Server {
    async fn echo(
        self: Rc<Self>,
        p: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        let value = p.get()?.get_value();
        if value < 2 {
            self.calls.set(self.calls.get() + 1);
            let gate = self.gates.borrow_mut()[value as usize]
                .take()
                .expect("request must not replay");
            gate.await.unwrap();
            return Err(if self.disconnected {
                Error::disconnected("original failure".into())
            } else {
                Error::failed("original failure".into())
            });
        }
        r.get().set_value(self.identity);
        Ok(())
    }
}
// A local forwarding capability preserves completion-based send_streaming
// semantics while the operation itself traverses a real RPC connection.
struct Forwarder(harness::Client);
impl harness::Server for Forwarder {
    async fn echo(
        self: Rc<Self>,
        p: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        // This fixture models an application awaiting a response. A transparent
        // tail-call proxy now preserves streaming readiness before that response.
        let mut request = self.0.echo_request();
        request.get().set_value(p.get()?.get_value());
        let response = request.send().promise.await?;
        r.get().set_value(response.get()?.get_value());
        Ok(())
    }
}
async fn settle() {
    for _ in 0..32 {
        tokio::task::yield_now().await;
    }
}
type Request = capnp::capability::Request<harness::echo_params::Owned, harness::value::Owned>;
struct Fixture {
    client: harness::Client,
    replacement: harness::Client,
    controller: Box<dyn SetTarget<harness::Client>>,
    requests: Vec<Option<Request>>,
    pending: Vec<Option<tokio::task::JoinHandle<capnp::Result<()>>>>,
    gates: Vec<Option<oneshot::Sender<()>>>,
    connects: Rc<Cell<usize>>,
    calls: Rc<Cell<usize>>,
    tasks: Vec<tokio::task::JoinHandle<capnp::Result<()>>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        for task in self.pending.iter().flatten() {
            task.abort();
        }
    }
}
impl Fixture {
    async fn new(disconnected: bool) -> Self {
        Self::with_completion(disconnected, true).await
    }
    async fn with_completion(disconnected: bool, local_completion: bool) -> Self {
        let hub = Rc::new(RefCell::new(Hub::default()));
        let calls = Rc::new(Cell::new(0));
        let mut gates = vec![];
        let mut receivers = vec![];
        for _ in 0..2 {
            let (tx, rx) = oneshot::channel();
            gates.push(Some(tx));
            receivers.push(Some(rx));
        }
        let mut tasks = vec![];
        for identity in 0..3 {
            let cap: harness::Client = capnp_rpc::new_client(Server {
                identity,
                calls: calls.clone(),
                disconnected,
                gates: RefCell::new(if identity == 0 {
                    std::mem::take(&mut receivers)
                } else {
                    vec![]
                }),
            });
            let network = Hub::network(&hub, identity as u8 + 1);
            tasks.push(tokio::task::spawn_local(RpcSystem::new(
                Box::new(network),
                Some(cap.client),
            )));
        }
        let network = Hub::network(&hub, 0);
        let mut system = RpcSystem::new(Box::new(network), None);
        let original: harness::Client = system.bootstrap(1);
        let original = if local_completion {
            capnp_rpc::new_client(Forwarder(original))
        } else {
            original
        };
        let backup: harness::Client = system.bootstrap(2);
        let replacement: harness::Client = system.bootstrap(3);
        tasks.push(tokio::task::spawn_local(system));
        let connects = Rc::new(Cell::new(0));
        let count = connects.clone();
        let (client, controller) = capnp_rpc::lazy_auto_reconnect(move || {
            let n = count.get();
            count.set(n + 1);
            Ok(if n == 0 {
                original.clone()
            } else {
                backup.clone()
            })
        });
        let requests = (0..2)
            .map(|i| {
                let mut r = client.echo_request();
                r.get().set_value(i);
                Some(r)
            })
            .collect();
        settle().await;
        Self {
            client,
            replacement,
            controller,
            requests,
            pending: vec![None, None],
            gates,
            connects,
            calls,
            tasks,
        }
    }
    async fn trace(
        &mut self,
        steps: &[serde_json::Value],
        streaming: bool,
        disconnected: bool,
        reset: bool,
    ) {
        let mut sends = 0;
        for step in steps {
            let action = step["action"].as_str().unwrap();
            match action {
                "send_first" | "send_second" => {
                    let i = usize::from(action == "send_second");
                    let request = self.requests[i].take().unwrap();
                    let promise = if streaming {
                        request.hook.send_streaming()
                    } else {
                        Promise::from_future(
                            async move { request.send().promise.await.map(|_| ()) },
                        )
                    };
                    self.pending[i] = Some(tokio::task::spawn_local(promise));
                    sends += 1;
                }
                "finish_first" | "finish_second" => {
                    let i = usize::from(action == "finish_second");
                    self.gates[i].take().unwrap().send(()).unwrap();
                    let error = self.pending[i].take().unwrap().await.unwrap().unwrap_err();
                    assert_eq!(
                        error.kind,
                        if disconnected {
                            ErrorKind::Disconnected
                        } else {
                            ErrorKind::Failed
                        }
                    );
                    assert!(error.extra.contains("original failure"));
                }
                "change" => {
                    if reset {
                        self.controller.reset();
                    } else {
                        self.controller.set_target(self.replacement.clone());
                    }
                }
                "probe" => {
                    let mut r = self.client.echo_request();
                    r.get().set_value(99);
                    let response = r.send().promise.await.unwrap();
                    assert_eq!(
                        response.get().unwrap().get_value(),
                        step["state"][9].as_u64().unwrap() as u32
                    );
                }
                _ => panic!("unknown action {action}"),
            }
            settle().await;
            assert_eq!(
                self.calls.get(),
                sends,
                "{action}: no automatic replay or lost calls"
            );
            assert_eq!(
                self.connects.get(),
                step["state"][7].as_u64().unwrap() as usize + 1,
                "{action}: stale error must not reconnect"
            );
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_reconnect_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_RECONNECT_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for case in cases {
                for streaming in [false, true] {
                    tokio::time::timeout(Duration::from_secs(5), async {
                        let disconnected = case["disconnected"].as_bool().unwrap();
                        let reset = case["reset"].as_bool().unwrap();
                        let mut f = Fixture::new(disconnected).await;
                        f.trace(
                            case["steps"].as_array().unwrap(),
                            streaming,
                            disconnected,
                            reset,
                        )
                        .await;
                    })
                    .await
                    .unwrap();
                }
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn replacement_protects_against_unsent_old_streaming_requests() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut f = Fixture::new(true).await;
            f.controller.set_target(f.replacement.clone());
            for i in 0..2 {
                let promise = f.requests[i].take().unwrap().hook.send_streaming();
                f.gates[i].take().unwrap().send(()).unwrap();
                assert_eq!(promise.await.unwrap_err().kind, ErrorKind::Disconnected);
            }
            assert_eq!(f.connects.get(), 1);
            let mut r = f.client.echo_request();
            r.get().set_value(99);
            assert_eq!(
                r.send().promise.await.unwrap().get().unwrap().get_value(),
                2
            );
            assert!(f.client.echo_request().hook.tail_send().is_none());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn remote_streaming_credit_precedes_ack_and_late_error_reconnects() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(2), async {
                let mut f = Fixture::with_completion(true, false).await;
                // The first send grants credit while its server task is gated.
                f.requests[0]
                    .take()
                    .unwrap()
                    .hook
                    .send_streaming()
                    .await
                    .unwrap();
                settle().await;
                assert_eq!(f.calls.get(), 1);
                assert_eq!(f.connects.get(), 1);
                f.gates[0].take().unwrap().send(()).unwrap();
                settle().await;
                // The stream retains the remote exception for the next send.
                let send = f.requests[1].take().unwrap().hook.send_streaming();
                f.gates[1].take().unwrap().send(()).unwrap();
                assert_eq!(send.await.unwrap_err().kind, ErrorKind::Disconnected);
                settle().await;
                assert_eq!(f.calls.get(), 2);
                assert_eq!(f.connects.get(), 2);
                let mut probe = f.client.echo_request();
                probe.get().set_value(99);
                assert_eq!(
                    probe
                        .send()
                        .promise
                        .await
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_value(),
                    1
                );
            })
            .await
            .unwrap();
        })
        .await;
}
