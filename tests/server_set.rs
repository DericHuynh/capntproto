use capnp::Error;
use capnp_rpc::CapabilityServerSet;
use futures::channel::oneshot;
use reproto_test_support::runtime_test_capnp::harness;
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    future::Future,
    pin::Pin,
    rc::Rc,
};
#[derive(Default)]
struct State {
    active: Cell<u32>,
    started: Cell<u32>,
    gates: RefCell<VecDeque<oneshot::Receiver<()>>>,
    fail: bool,
}
struct Running(Rc<State>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.active.set(self.0.active.get() - 1);
    }
}
struct Service(Rc<State>);
impl harness::Server for Service {
    async fn stream(self: Rc<Self>, _: harness::StreamParams) -> capnp::Result<()> {
        self.0.active.set(self.0.active.get() + 1);
        self.0.started.set(self.0.started.get() + 1);
        let _running = Running(self.0.clone());
        let gate = self.0.gates.borrow_mut().pop_front().unwrap();
        gate.await
            .map_err(|_| Error::failed("gate dropped".into()))?;
        if self.0.fail {
            Err(Error::failed("stream failed".into()))
        } else {
            Ok(())
        }
    }
}
type Lookup<'a> = Pin<Box<dyn Future<Output = Option<Rc<Service>>> + 'a>>;
async fn trace(steps: &[serde_json::Value], fail: bool) {
    let state = Rc::new(State {
        fail,
        ..Default::default()
    });
    let mut gates = vec![];
    for _ in 0..2 {
        let (tx, rx) = oneshot::channel();
        gates.push(Some(tx));
        state.gates.borrow_mut().push_back(rx);
    }
    let mut servers = CapabilityServerSet::<Service, harness::Client>::new();
    let client = servers.new_client(Service(state.clone()));
    let mut calls = [Some(client.stream_request().send()), None];
    let mut outcomes = [0u32; 2];
    assert!(futures::poll!(calls[0].as_mut().unwrap()).is_pending());
    let mut lookup: Option<Lookup<'_>> = None;
    let mut server = None;
    for step in steps {
        match step["action"].as_str().unwrap() {
            "send" => {
                calls[1] = Some(client.stream_request().send());
            }
            "lookup" => {
                lookup = Some(Box::pin(servers.get_local_server(&client)));
            }
            "complete-a" => {
                gates[0].take().unwrap().send(()).unwrap();
            }
            "complete-b" => {
                gates[1].take().unwrap().send(()).unwrap();
            }
            "drop-a" => {
                calls[0].take();
            }
            "drop-b" => {
                calls[1].take();
            }
            "drop-lookup" => {
                lookup.take();
                server.take();
            }
            _ => panic!(),
        }
        for _ in 0..32 {
            for (i, call) in calls.iter_mut().enumerate() {
                if let Some(p) = call {
                    if let std::task::Poll::Ready(result) = futures::poll!(p) {
                        outcomes[i] = if result.is_ok() { 1 } else { 2 };
                        call.take();
                    }
                }
            }
            if let Some(p) = lookup.as_mut() {
                if let std::task::Poll::Ready(result) = futures::poll!(p) {
                    server = Some(result.unwrap());
                    lookup.take();
                }
            }
            tokio::task::yield_now().await;
        }
        let expected = step["state"].as_array().unwrap();
        let a = expected[0].as_u64().unwrap();
        let b = expected[1].as_u64().unwrap();
        assert_eq!(
            state.active.get(),
            u32::from(a == 2 || b == 2),
            "active method"
        );
        assert_eq!(
            state.started.get(),
            1 + u32::from(expected[5] == 1),
            "queued dispatch"
        );
        assert_eq!(server.is_some(), expected[2] == 2, "lookup readiness");
        if let Some(server) = &server {
            assert!(Rc::ptr_eq(&server.0, &state));
        }
        assert_eq!(
            servers.get_local_server_of_resolved(&client).is_some(),
            a != 2 && b != 2,
            "synchronous lookup cannot bypass a stream"
        );
        for (i, status) in [a, b].into_iter().enumerate() {
            assert_eq!(
                outcomes[i],
                match status {
                    3 => 1,
                    5 => 2,
                    _ => 0,
                },
                "call outcome"
            );
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn server_lookup_waits_for_prior_streams_but_not_later_calls() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let steps = serde_json::json!([
                {"action":"lookup","state":[2,0,1,1,0,0]},
                {"action":"send","state":[2,1,1,1,0,0]},
                {"action":"complete-a","state":[3,2,2,1,0,1]},
                {"action":"complete-b","state":[3,3,2,1,0,1]}
            ]);
            trace(steps.as_array().unwrap(), false).await;
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_server_set_traces() {
    let path = reproto_test_support::verification::input("REPROTO_SERVER_SET_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for case in cases {
                trace(
                    case["steps"].as_array().unwrap(),
                    case["fail"].as_bool().unwrap(),
                )
                .await;
            }
        })
        .await;
}
