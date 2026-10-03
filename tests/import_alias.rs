#[allow(dead_code)]
mod support;
use capnp::{capability::FromClientHook, traits::ImbueMut};
use capnp_rpc::{
    rpc_capnp::{message, message_target},
    RpcSystem,
};
use capntproto_test_support::runtime_test_capnp::harness;
use std::{cell::RefCell, rc::Rc};
use support::{Endpoint, Hub};
struct Dummy;
impl harness::Server for Dummy {}
struct Fixture {
    system: Rc<RefCell<RpcSystem<u8>>>,
    task: tokio::task::JoinHandle<capnp::Result<()>>,
    peer: Endpoint,
    _hub: Rc<RefCell<Hub>>,
    _network: support::Network,
    original: Option<harness::Client>,
    alias: Option<harness::Client>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new() -> Self {
        use std::{future::Future, pin::Pin};
        let hub = Rc::new(RefCell::new(Hub::default()));
        let network = Hub::network(&hub, 2);
        let system = Rc::new(RefCell::new(RpcSystem::new(
            Box::new(Hub::network(&hub, 1)),
            None,
        )));
        let peer = hub.borrow_mut().connect(1, 2);
        drop(peer);
        let peer = hub.borrow_mut().connect(2, 1);
        let driver = system.clone();
        let task = tokio::task::spawn_local(futures::future::poll_fn(move |cx| {
            Pin::new(&mut *driver.borrow_mut()).poll(cx)
        }));
        let mut f = Self {
            system,
            task,
            peer,
            _hub: hub,
            _network: network,
            original: None,
            alias: None,
        };
        f.original = Some(f.bootstrap().await);
        f
    }
    async fn bootstrap(&mut self) -> harness::Client {
        let cap: harness::Client = self.system.borrow_mut().bootstrap(2);
        let question = loop {
            let m = self.peer.recv().await;
            match m
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            {
                message::Bootstrap(b) => break b.unwrap().get_question_id(),
                message::Finish(_) => {}
                _ => panic!("unexpected message during re-import"),
            }
        };
        self.peer.send(|m| {
            let mut ret = m.init_return();
            ret.set_answer_id(question);
            let mut p = ret.init_results();
            let mut caps = vec![];
            let dummy: harness::Client = capnp_rpc::new_client(Dummy);
            {
                let mut content = p.reborrow().get_content();
                content.imbue_mut(&mut caps);
                content.set_as_capability(dummy.client.hook);
            }
            p.init_cap_table(1).get(0).set_sender_hosted(77);
        });
        capnp::capability::get_resolved_cap(cap).await
    }
    async fn call(&mut self) {
        let promise = self
            .original
            .as_ref()
            .unwrap()
            .echo_request()
            .send()
            .promise;
        let question = loop {
            let m = self.peer.recv().await;
            match m
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            {
                message::Call(c) => {
                    let c = c.unwrap();
                    assert!(matches!(
                        c.get_target().unwrap().which().unwrap(),
                        message_target::ImportedCap(77)
                    ));
                    break c.get_question_id();
                }
                message::Finish(_) => {}
                _ => panic!("live original import must not be released or broken"),
            }
        };
        self.peer.send(|m| {
            let mut r = m.init_return();
            r.set_answer_id(question);
            r.init_results()
                .get_content()
                .init_as::<harness::value::Builder>()
                .set_value(99);
        });
        assert_eq!(promise.await.unwrap().get().unwrap().get_value(), 99);
    }
    async fn replay(&mut self, steps: &[serde_json::Value]) {
        for step in steps {
            match step["action"].as_str().unwrap() {
                "acquire" => {
                    let alias = self.bootstrap().await;
                    assert_eq!(
                        self.original.as_ref().unwrap().as_client_hook().get_ptr(),
                        alias.as_client_hook().get_ptr()
                    );
                    self.alias = Some(alias);
                }
                "drop" => {
                    self.alias.take();
                    for _ in 0..16 {
                        tokio::task::yield_now().await;
                    }
                }
                "first" | "last" => self.call().await,
                _ => panic!(),
            }
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_import_alias_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_IMPORT_ALIAS_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Vec<serde_json::Value>> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for trace in traces {
                tokio::time::timeout(std::time::Duration::from_secs(3), async {
                    Fixture::new().await.replay(&trace).await;
                })
                .await
                .unwrap();
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn dropping_reimport_does_not_break_original_capability() {
    let trace = serde_json::json!([{"action":"first"},{"action":"acquire"},{"action":"drop"},{"action":"last"}]);
    tokio::task::LocalSet::new()
        .run_until(async {
            Fixture::new().await.replay(trace.as_array().unwrap()).await;
        })
        .await;
}
