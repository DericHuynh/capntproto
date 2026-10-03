#[allow(dead_code)]
mod support;
use capnp::{traits::HasTypeId, Error};
use capnp_rpc::{
    rpc_capnp::{cap_descriptor, exception, message, return_},
    IncomingMessage, RpcSystem,
};
use capntproto_test_support::runtime_test_capnp::harness;
use std::{cell::RefCell, future::Future, pin::Pin, rc::Rc};
use support::{Endpoint, Hub};

struct Forward(Rc<RefCell<Option<Error>>>);
impl harness::Server for Forward {
    async fn echo(
        self: Rc<Self>,
        _: harness::EchoParams,
        _: harness::EchoResults,
    ) -> capnp::Result<()> {
        Err(self.0.borrow().as_ref().unwrap().clone())
    }
}
struct Fixture {
    _hub: Rc<RefCell<Hub>>,
    _network: support::Network,
    system: Rc<RefCell<RpcSystem<u8>>>,
    task: tokio::task::JoinHandle<capnp::Result<()>>,
    peer: Endpoint,
    remote: harness::Client,
    export: u32,
    original: Option<Error>,
    cloned: Rc<RefCell<Option<Error>>>,
    forwarded: Option<Box<dyn IncomingMessage>>,
    observed: Option<Error>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new() -> Self {
        let hub = Rc::new(RefCell::new(Hub::default()));
        let network = Hub::network(&hub, 2);
        let cloned = Rc::new(RefCell::new(None));
        let local: harness::Client = capnp_rpc::new_client(Forward(cloned.clone()));
        let mut system = RpcSystem::new(Box::new(Hub::network(&hub, 1)), Some(local.client));
        let remote: harness::Client = system.bootstrap(2);
        let mut peer = hub.borrow_mut().connect(2, 1);
        let system = Rc::new(RefCell::new(system));
        let driver = system.clone();
        let task = tokio::task::spawn_local(futures::future::poll_fn(move |cx| {
            Pin::new(&mut *driver.borrow_mut()).poll(cx)
        }));
        let input = peer.recv().await;
        let message::Bootstrap(b) = input
            .get_body()
            .unwrap()
            .get_as::<message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        else {
            panic!()
        };
        peer.send(|m| {
            let mut r = m.init_return();
            r.set_answer_id(b.unwrap().get_question_id());
            let mut payload = r.init_results();
            // The capability pointer references senderHosted(77).
            let mut caps = vec![];
            use capnp::traits::ImbueMut;
            let mut content = payload.reborrow().get_content();
            content.imbue_mut(&mut caps);
            content.set_as_capability(
                capnp_rpc::new_client::<harness::Client, _>(Forward(Rc::new(RefCell::new(None))))
                    .client
                    .hook,
            );
            payload.init_cap_table(1).get(0).set_sender_hosted(77);
        });
        let remote = capnp::capability::get_resolved_cap(remote).await;
        peer.send(|m| m.init_bootstrap().set_question_id(100));
        let export = loop {
            let input = peer.recv().await;
            match input
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            {
                message::Return(r) => {
                    let return_::Results(p) = r.unwrap().which().unwrap() else {
                        panic!()
                    };
                    let cap_descriptor::SenderHosted(id) =
                        p.unwrap().get_cap_table().unwrap().get(0).which().unwrap()
                    else {
                        panic!()
                    };
                    break id;
                }
                message::Finish(_) | message::Release(_) => (),
                _ => panic!("unexpected bootstrap traffic"),
            }
        };
        Self {
            _hub: hub,
            _network: network,
            system,
            task,
            peer,
            remote,
            export,
            original: None,
            cloned,
            forwarded: None,
            observed: None,
        }
    }
    async fn next_call(&mut self) -> u32 {
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
                message::Call(c) => return c.unwrap().get_question_id(),
                message::Finish(_) | message::Release(_) => (),
                _ => panic!("unexpected traffic before call"),
            }
        }
    }
    async fn receive(&mut self, kind: u16, trace: bool) {
        let call = self.remote.echo_request().send();
        let id = self.next_call().await;
        self.peer.send(|m| {
            let mut r = m.init_return();
            r.set_answer_id(id);
            let mut e = r.init_exception();
            e.set_reason("failure");
            // Exercise an unknown enum as well as all four defined categories.
            use capnp::introspect::Introspect;
            let capnp::introspect::TypeVariant::Enum(schema) =
                exception::Type::introspect().which()
            else {
                panic!()
            };
            let dynamic: capnp::dynamic_value::Builder = e.reborrow().into();
            dynamic
                .downcast::<capnp::dynamic_struct::Builder>()
                .set_named(
                    "type",
                    capnp::dynamic_value::Enum::new(kind, schema.into()).into(),
                )
                .unwrap();
            if trace {
                e.set_trace("origin trace");
            }
            let mut details = e.init_details(4);
            details.reborrow().get(0).set_detail_id(11);
            details.reborrow().get(0).set_data(b"old");
            details.reborrow().get(1).set_detail_id(11);
            details.reborrow().get(1).set_data(b"new\0opaque");
            details.reborrow().get(2).set_detail_id(12);
            details.reborrow().get(2).set_data(b"");
            details.get(3).set_detail_id(13); // Absent data must not create a detail.
        });
        self.original = Some(call.promise.await.err().unwrap());
        let expected = match kind {
            1 => capnp::ErrorKind::Overloaded,
            2 => capnp::ErrorKind::Disconnected,
            3 => capnp::ErrorKind::Unimplemented,
            _ => capnp::ErrorKind::Failed,
        };
        assert_eq!(self.original.as_ref().unwrap().kind, expected);
    }
    async fn forward(&mut self) {
        self.peer.send(|m| {
            let mut c = m.init_call();
            c.set_question_id(101);
            c.set_interface_id(harness::Client::TYPE_ID);
            c.set_method_id(0);
            c.reborrow().init_target().set_imported_cap(self.export);
            c.init_params();
        });
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
                    assert_eq!(r.unwrap().get_answer_id(), 101);
                    self.forwarded = Some(input);
                    return;
                }
                message::Finish(_) | message::Release(_) => (),
                _ => panic!("unexpected forward traffic"),
            }
        }
    }
    async fn decode(&mut self) {
        let call = self.remote.echo_request().send();
        let id = self.next_call().await;
        let message::Return(r) = self
            .forwarded
            .as_ref()
            .unwrap()
            .get_body()
            .unwrap()
            .get_as::<message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        else {
            panic!()
        };
        let return_::Exception(e) = r.unwrap().which().unwrap() else {
            panic!()
        };
        self.peer.send(|m| {
            let mut r = m.init_return();
            r.set_answer_id(id);
            r.set_exception(e.unwrap()).unwrap();
        });
        self.observed = Some(call.promise.await.err().unwrap());
    }
    fn check(&self, state: &[serde_json::Value]) {
        let trace = state[6].as_u64().unwrap();
        let expected_trace = match trace {
            0 => None,
            1 => Some("origin trace"),
            2 => Some("encoded trace"),
            _ => panic!(),
        };
        if let Some(error) = &self.observed {
            check_error(error, expected_trace);
        } else if let Some(forwarded) = &self.forwarded {
            let message::Return(r) = forwarded
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            else {
                panic!()
            };
            let return_::Exception(e) = r.unwrap().which().unwrap() else {
                panic!()
            };
            let e = e.unwrap();
            assert_eq!(
                e.get_reason().unwrap().to_str().unwrap(),
                "remote exception: failure"
            );
            assert_eq!(
                if e.has_trace() {
                    Some(e.get_trace().unwrap().to_str().unwrap())
                } else {
                    None
                },
                expected_trace
            );
            let details = e.get_details().unwrap();
            assert_eq!(details.len(), 2);
            assert_eq!(details.get(0).get_detail_id(), 11);
            assert_eq!(details.get(0).get_data().unwrap(), b"new\0opaque");
            assert_eq!(details.get(1).get_detail_id(), 12);
            assert_eq!(details.get(1).get_data().unwrap(), b"");
        } else if let Some(error) = self.cloned.borrow().as_ref().or(self.original.as_ref()) {
            check_error(error, expected_trace);
        }
    }
}
fn check_error(error: &Error, trace: Option<&str>) {
    assert_eq!(error.extra, "remote exception: failure");
    assert_eq!(error.remote_trace(), trace);
    assert_eq!(error.detail(11), Some(b"new\0opaque".as_slice()));
    assert_eq!(error.detail(12), Some(b"".as_slice()));
    assert_eq!(error.detail(13), None);
    assert_eq!(error.details().count(), 2);
}
async fn replay(steps: &[serde_json::Value], kind: u16, trace: bool) {
    let mut f = Fixture::new().await;
    for step in steps {
        let state = step["state"].as_array().unwrap();
        match step["action"].as_str().unwrap() {
            "receive" => f.receive(kind, trace).await,
            "clone" => *f.cloned.borrow_mut() = f.original.clone(),
            "drop" => {
                f.original.take();
            }
            "configure" => {
                if state[5] == 1 {
                    f.system
                        .borrow_mut()
                        .set_trace_encoder(|_| "encoded trace".into());
                } else {
                    f.system.borrow_mut().clear_trace_encoder();
                }
            }
            "forward" => f.forward().await,
            "decode" => f.decode().await,
            _ => panic!(),
        }
        f.check(state);
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_exception_metadata_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_EXCEPTION_METADATA_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for case in cases {
                for kind in [0, 1, 2, 3, 65535] {
                    replay(
                        case["steps"].as_array().unwrap(),
                        kind,
                        case["trace"].as_bool().unwrap(),
                    )
                    .await;
                }
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn owned_details_survive_clone_forwarding_and_second_decode() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                let mut f = Fixture::new().await;
                f.receive(65535, true).await;
                check_error(f.original.as_ref().unwrap(), Some("origin trace"));
                *f.cloned.borrow_mut() = f.original.take();
                f.forward().await;
                f.decode().await;
                check_error(f.observed.as_ref().unwrap(), None);
            })
            .await
            .unwrap();
        })
        .await;
}

async fn disconnect_trace(steps: &[serde_json::Value], original_kind: exception::Type) {
    let mut f = Fixture::new().await;
    let mut first = Some(f.remote.echo_request().send().promise);
    let mut second = Some(f.remote.echo_request().send().promise);
    f.next_call().await;
    f.next_call().await;
    let mut third = None;
    let mut aborted = false;
    for step in steps {
        match step["action"].as_str().unwrap() {
            "abort" => {
                f.peer.send(|m| {
                    let mut e = m.init_abort();
                    e.set_type(original_kind);
                    e.set_reason("failure");
                    e.set_trace("origin trace");
                    let mut details = e.init_details(2);
                    details.reborrow().get(0).set_detail_id(11);
                    details.reborrow().get(0).set_data(b"new\0opaque");
                    details.reborrow().get(1).set_detail_id(12);
                    details.get(1).set_data(b"");
                });
                // Drain the executor so a post-Abort call tests the broken import.
                for _ in 0..24 {
                    tokio::task::yield_now().await;
                }
                aborted = true;
            }
            "create" => {
                third = Some(f.remote.echo_request().send().promise);
                if !aborted {
                    f.next_call().await;
                }
            }
            action => {
                let promise = match action {
                    "first" => first.take().unwrap(),
                    "second" => second.take().unwrap(),
                    "third" => third.take().unwrap(),
                    _ => panic!(),
                };
                let error = promise.await.err().unwrap();
                assert_eq!(error.kind, capnp::ErrorKind::Disconnected);
                check_error(&error, Some("origin trace"));
            }
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_exception_disconnect_traces() {
    let path =
        capntproto_test_support::verification::input("CAPNTPROTO_EXCEPTION_DISCONNECT_TRACES")
            .expect("prepare verified trace corpus");
    let cases: Vec<Vec<serde_json::Value>> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for trace in cases {
                for kind in [
                    exception::Type::Failed,
                    exception::Type::Overloaded,
                    exception::Type::Disconnected,
                    exception::Type::Unimplemented,
                ] {
                    disconnect_trace(&trace, kind).await;
                }
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn abort_rejects_pending_and_future_calls_with_owned_diagnostics() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                disconnect_trace(
                    &["abort", "second", "create", "third", "first"]
                        .map(|action| serde_json::json!({"action": action})),
                    exception::Type::Overloaded,
                )
                .await;
            })
            .await
            .unwrap();
        })
        .await;
}
