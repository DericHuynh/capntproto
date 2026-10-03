//! Replay the composed model's wire-handler projection through actual RPC.
//! Export counts are observed through destruction and subsequent Release
//! acceptance, not private runtime table access. Method names in the model
//! map to two distinct harness methods; their application semantics are outside
//! this boundary check. No annotation or caller operation is sent on the wire.
mod composed_wire_history;
#[allow(dead_code)]
mod support;

use capnp::traits::{HasTypeId, ImbueMut};
use capnp_rpc::{
    rpc_capnp::{cap_descriptor, message, return_},
    RpcSystem,
};
use capntproto_test_support::{runtime_test_capnp::harness, verification::exploration};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};
use support::{ConnectionStatus, Endpoint, Hub};

type Calls = Rc<RefCell<Vec<(usize, u16, u32)>>>;
struct Empty;
impl harness::Server for Empty {}
struct Tracked {
    index: usize,
    destroyed: Rc<Cell<bool>>,
    calls: Calls,
}
impl Drop for Tracked {
    fn drop(&mut self) {
        self.destroyed.set(true);
    }
}
impl harness::Server for Tracked {
    async fn echo(
        self: Rc<Self>,
        p: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        let value = p.get()?.get_value();
        self.calls.borrow_mut().push((self.index, 0, value));
        r.get().set_value(value);
        Ok(())
    }
    async fn tail(
        self: Rc<Self>,
        p: harness::TailParams,
        mut r: harness::TailResults,
    ) -> capnp::Result<()> {
        let value = p.get()?.get_value();
        self.calls.borrow_mut().push((self.index, 2, value));
        r.get().set_value(value);
        Ok(())
    }
}
async fn drain() {
    for _ in 0..40 {
        tokio::task::yield_now().await;
    }
}
struct Fixture {
    _hub: Rc<RefCell<Hub>>,
    _network: support::Network,
    task: tokio::task::JoinHandle<capnp::Result<()>>,
    peer: Endpoint,
    status: Rc<RefCell<ConnectionStatus>>,
    _remote: harness::Client,
    _questions: Vec<capnp::capability::RemotePromise<harness::bounce_results::Owned>>,
    exports: [u32; 2],
    destroyed: [Rc<Cell<bool>>; 2],
    calls: Calls,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new(counts: [u64; 2]) -> Self {
        let hub = Rc::new(RefCell::new(Hub::default()));
        let network = Hub::network(&hub, 2);
        let mut system = RpcSystem::new(Box::new(Hub::network(&hub, 1)), None);
        let remote: harness::Client = system.bootstrap(2);
        let mut peer = hub.borrow_mut().connect(2, 1);
        let status = hub.borrow_mut().connect(1, 2).status();
        let task = tokio::task::spawn_local(system);
        {
            let input = peer.recv().await;
            let message::Bootstrap(b) = input
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            else {
                panic!("expected bootstrap")
            };
            peer.send(|m| {
                let mut r = m.init_return();
                r.set_answer_id(b.unwrap().get_question_id());
                let mut payload = r.init_results();
                let mut caps = vec![];
                let mut content = payload.reborrow().get_content();
                content.imbue_mut(&mut caps);
                content.set_as_capability(
                    capnp_rpc::new_client::<harness::Client, _>(Empty)
                        .client
                        .hook,
                );
                payload.init_cap_table(1).get(0).set_sender_hosted(77);
            });
        }
        let remote = capnp::capability::get_resolved_cap(remote).await;
        let destroyed = [Rc::new(Cell::new(false)), Rc::new(Cell::new(false))];
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut questions = vec![];
        let mut exports = [u32::MAX; 2];
        for index in 0..2 {
            let cap: harness::Client = capnp_rpc::new_client(Tracked {
                index,
                destroyed: destroyed[index].clone(),
                calls: calls.clone(),
            });
            for reference in 0..counts[index] {
                let mut request = remote.bounce_request();
                request.get().set_cap(cap.clone());
                questions.push(request.send());
                let export = loop {
                    let input = peer.recv().await;
                    if let message::Call(c) = input
                        .get_body()
                        .unwrap()
                        .get_as::<message::Reader>()
                        .unwrap()
                        .which()
                        .unwrap()
                    {
                        let c = c.unwrap();
                        assert_eq!(c.get_method_id(), 1);
                        let cap_descriptor::SenderHosted(id) = c
                            .get_params()
                            .unwrap()
                            .get_cap_table()
                            .unwrap()
                            .get(0)
                            .which()
                            .unwrap()
                        else {
                            panic!("expected exported capability")
                        };
                        break id;
                    }
                };
                if reference == 0 {
                    exports[index] = export;
                } else {
                    assert_eq!(exports[index], export, "reuse the same export");
                }
            }
        }
        assert_ne!(exports[0], exports[1]);
        assert!(!exports.contains(&u32::MAX));
        drain().await;
        assert!(destroyed.iter().all(|d| !d.get()));
        Self {
            _hub: hub,
            _network: network,
            task,
            peer,
            status,
            _remote: remote,
            _questions: questions,
            exports,
            destroyed,
            calls,
        }
    }
    fn release(&mut self, id: u64, count: u32) {
        let export = if id == 0 {
            u32::MAX
        } else {
            self.exports[id as usize - 1]
        };
        self.peer.send(|m| {
            let mut r = m.init_release();
            r.set_id(export);
            r.set_reference_count(count);
        });
    }
    async fn call(&mut self, id: u64, method: u64, value: u32, question: u32, yields: u8) {
        let export = self.exports[id as usize - 1];
        let method = if method == 0 { 0 } else { 2 };
        self.peer.send(|m| {
            let mut c = m.init_call();
            c.set_question_id(question);
            c.set_interface_id(harness::Client::TYPE_ID);
            c.set_method_id(method);
            c.reborrow().init_target().set_imported_cap(export);
            let content = c.init_params().get_content();
            if method == 0 {
                content
                    .init_as::<harness::echo_params::Builder>()
                    .set_value(value);
            } else {
                content
                    .init_as::<harness::tail_params::Builder>()
                    .set_value(value);
            }
        });
        for _ in 0..yields {
            tokio::task::yield_now().await;
        }
        loop {
            let input = self.peer.recv().await;
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
                    assert_eq!(r.get_answer_id(), question);
                    let return_::Results(p) = r.which().unwrap() else {
                        panic!("expected successful call")
                    };
                    assert_eq!(
                        p.unwrap()
                            .get_content()
                            .get_as::<harness::value::Reader>()
                            .unwrap()
                            .get_value(),
                        value
                    );
                    break;
                }
                message::Finish(_) => {}
                _ => panic!("unexpected callback response"),
            }
        }
        self.peer.send(|m| {
            let mut f = m.init_finish();
            f.set_question_id(question);
            f.set_release_result_caps(false);
        });
    }
}

async fn replay(path: &[exploration::State], yields: &[u8]) {
    assert_eq!(path[0]["event"], 1);
    assert_eq!(path.len(), yields.len());
    let mut fixture = Fixture::new([path[0]["a"], path[0]["b"]]).await;
    let mut calls = vec![];
    for (step, (state, pause)) in path.iter().zip(yields).enumerate() {
        match state["event"] {
            1 => {}
            2 => fixture.release(state["id"], state["amount"] as u32),
            3 => {
                // Reuse two question IDs after Return/Finish. This is local
                // question cleanup, not stale capability ID replay protection.
                fixture
                    .call(
                        state["id"],
                        state["method"],
                        state["data"] as u32,
                        (step % 2) as u32,
                        *pause,
                    )
                    .await;
                calls.push((
                    state["id"] as usize - 1,
                    if state["seenMethod"] == 0 { 0 } else { 2 },
                    state["seenData"] as u32,
                ));
            }
            _ => panic!("unknown model action"),
        }
        for _ in 0..*pause {
            tokio::task::yield_now().await;
        }
        drain().await;
        assert_eq!(
            fixture.destroyed[0].get(),
            state["a"] == 0,
            "step {step}: {path:?}"
        );
        assert_eq!(
            fixture.destroyed[1].get(),
            state["b"] == 0,
            "step {step}: {path:?}"
        );
        assert_eq!(
            fixture.status.borrow().aborts,
            (1 - state["live"]) as usize,
            "step {step}: {path:?}"
        );
        assert_eq!(*fixture.calls.borrow(), calls, "step {step}: {path:?}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn tlc_replay_composed_wire_handler_traces() {
    const MODEL: &str = "verification/ComposedWireBoundary.tla";
    const CONFIG: &str = include_str!("../verification/ComposedWireBoundary.cfg");
    let traces = exploration::traces(MODEL, "composed-wire-boundary", CONFIG).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(60), async {
                for path in &traces {
                    replay(path, &vec![0; path.len()]).await;
                }
            })
            .await
            .unwrap();
        })
        .await;
    exploration::controls(
        MODEL,
        "composed-wire-boundary",
        CONFIG,
        &[
            ("wireReleaseTickets", "WireRelease"),
            ("requestMethodPair", "WireMethod"),
            ("requestDataPair", "WireData"),
        ],
        None,
    )
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn maximum_wire_release_count_aborts_without_wrapping() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let mut fixture = Fixture::new([2, 1]).await;
                fixture.release(1, u32::MAX);
                drain().await;
                assert_eq!(fixture.status.borrow().aborts, 1);
                assert!(fixture.destroyed.iter().all(|d| d.get()));
            })
            .await
            .unwrap();
        })
        .await;
}
