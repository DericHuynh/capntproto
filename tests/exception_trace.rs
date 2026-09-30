#[allow(dead_code)]
mod support;
use capnp::{capability::Client, traits::HasTypeId, Error};
use capnp_rpc::{
    rpc_capnp::{cap_descriptor, exception, message, resolve, return_},
    BootstrapFactory, RpcSystem,
};
use futures::channel::oneshot;
use reproto_test_support::runtime_test_capnp::harness;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    future::Future,
    pin::Pin,
    rc::Rc,
};
use support::{Endpoint, Hub};
struct Fail;
impl harness::Server for Fail {
    async fn echo(
        self: Rc<Self>,
        _: harness::EchoParams,
        _: harness::EchoResults,
    ) -> capnp::Result<()> {
        Err(Error::overloaded("call failed".into()))
    }
}
struct Factory {
    kind: u32,
    pending: RefCell<HashMap<u8, oneshot::Sender<capnp::Result<harness::Client>>>>,
}
impl BootstrapFactory<u8> for Factory {
    fn create_for(&self, peer: &u8) -> capnp::Result<Client> {
        if self.kind == 0 {
            return Err(Error::unimplemented("bootstrap failed".into()));
        }
        let cap: harness::Client = if self.kind == 2 {
            let (tx, rx) = oneshot::channel();
            self.pending.borrow_mut().insert(*peer, tx);
            capnp_rpc::new_future_client(async move { rx.await.unwrap() })
        } else {
            capnp_rpc::new_client(Fail)
        };
        Ok(cap.client)
    }
}
async fn settle() {
    for _ in 0..24 {
        tokio::task::yield_now().await;
    }
}
struct Fixture {
    hub: Rc<RefCell<Hub>>,
    system: Rc<RefCell<RpcSystem<u8>>>,
    task: tokio::task::JoinHandle<capnp::Result<()>>,
    peers: Vec<Endpoint>,
    exports: Vec<u32>,
    factory: Rc<Factory>,
    encodings: Rc<Cell<u32>>,
    expected_encodings: u32,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new(kind: u32) -> Self {
        let hub = Rc::new(RefCell::new(Hub::default()));
        let factory = Rc::new(Factory {
            kind,
            pending: RefCell::new(HashMap::new()),
        });
        let system = Rc::new(RefCell::new(RpcSystem::new_with_bootstrap_factory(
            Box::new(Hub::network(&hub, 1)),
            1,
            factory.clone(),
        )));
        let driver = system.clone();
        let task = tokio::task::spawn_local(futures::future::poll_fn(move |cx| {
            Pin::new(&mut *driver.borrow_mut()).poll(cx)
        }));
        let mut f = Self {
            hub,
            system,
            task,
            peers: vec![],
            exports: vec![],
            factory,
            encodings: Rc::new(Cell::new(0)),
            expected_encodings: 0,
        };
        f.connect().await;
        f
    }
    async fn connect(&mut self) {
        let mut peer = self.hub.borrow_mut().connect(self.peers.len() as u8 + 2, 1);
        let mut export = 0;
        if matches!(self.factory.kind, 1 | 2) {
            peer.send(|m| m.init_bootstrap().set_question_id(0));
            let msg = peer.recv().await;
            let message::Return(r) = msg
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            else {
                panic!()
            };
            let return_::Results(p) = r.unwrap().which().unwrap() else {
                panic!()
            };
            export = match p.unwrap().get_cap_table().unwrap().get(0).which().unwrap() {
                cap_descriptor::SenderHosted(id) if self.factory.kind == 1 => id,
                cap_descriptor::SenderPromise(id) if self.factory.kind == 2 => id,
                _ => panic!("wrong bootstrap capability kind"),
            };
        }
        self.peers.push(peer);
        self.exports.push(export);
        settle().await;
    }
    fn configure(&mut self, config: u32) {
        if config == 3 {
            self.system.borrow_mut().clear_trace_encoder();
        } else {
            let count = self.encodings.clone();
            self.system.borrow_mut().set_trace_encoder(move |e| {
                count.set(count.get() + 1);
                format!("trace-{config}:{}", e.extra)
            });
        }
    }
    async fn emit(&mut self, index: usize, encoder: u32) {
        let export = self.exports[index];
        match self.factory.kind {
            0 => self.peers[index].send(|m| m.init_bootstrap().set_question_id(1)),
            1 => self.peers[index].send(|m| {
                let mut c = m.init_call();
                c.set_question_id(1);
                c.set_interface_id(harness::Client::TYPE_ID);
                c.set_method_id(0);
                c.reborrow().init_target().set_imported_cap(export);
                c.init_params()
                    .get_content()
                    .init_as::<harness::echo_params::Builder>();
            }),
            2 => {
                assert!(self
                    .factory
                    .pending
                    .borrow_mut()
                    .remove(&(index as u8 + 2))
                    .unwrap()
                    .send(Err(Error::disconnected("promise failed".into())))
                    .is_ok());
            }
            3 => self.peers[index].send(|m| {
                let mut r = m.init_return();
                r.set_answer_id(0x7fff_ffff);
                r.set_canceled(());
            }),
            _ => unreachable!(),
        }
        loop {
            let msg = self.peers[index].recv().await;
            let exc = match msg
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            {
                message::Return(r) if self.factory.kind < 2 => {
                    let r = r.unwrap();
                    assert_eq!(r.get_answer_id(), 1);
                    let return_::Exception(e) = r.which().unwrap() else {
                        panic!()
                    };
                    e.unwrap()
                }
                message::Resolve(r) if self.factory.kind == 2 => {
                    let r = r.unwrap();
                    assert_eq!(r.get_promise_id(), export);
                    let resolve::Exception(e) = r.which().unwrap() else {
                        panic!()
                    };
                    e.unwrap()
                }
                message::Abort(e) if self.factory.kind == 3 => e.unwrap(),
                message::Release(_) | message::Finish(_) => continue,
                _ => panic!("wrong exception channel"),
            };
            let reason = exc.get_reason().unwrap().to_str().unwrap();
            assert!(!reason.is_empty());
            assert_eq!(
                exc.get_type().unwrap(),
                [
                    exception::Type::Unimplemented,
                    exception::Type::Overloaded,
                    exception::Type::Disconnected,
                    exception::Type::Failed
                ][self.factory.kind as usize]
            );
            assert_eq!(exc.has_trace(), encoder != 0);
            assert_eq!(
                exc.get_trace().unwrap().to_str().unwrap(),
                if encoder == 0 {
                    String::new()
                } else {
                    format!("trace-{encoder}:{reason}")
                }
            );
            self.expected_encodings += u32::from(encoder != 0);
            assert_eq!(self.encodings.get(), self.expected_encodings);
            break;
        }
    }
    async fn replay(&mut self, steps: &[serde_json::Value]) {
        for step in steps {
            let state = step["state"].as_array().unwrap();
            match step["action"].as_str().unwrap() {
                "configure" => self.configure(state[0].as_u64().unwrap() as u32),
                "connect" => self.connect().await,
                "first" => self.emit(0, state[4].as_u64().unwrap() as u32).await,
                "second" => self.emit(1, state[5].as_u64().unwrap() as u32).await,
                _ => panic!(),
            }
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_exception_trace_traces() {
    let path = reproto_test_support::verification::input("REPROTO_EXCEPTION_TRACE_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Vec<serde_json::Value>> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for kind in 0..4 {
                for trace in &traces {
                    tokio::time::timeout(std::time::Duration::from_secs(3), async {
                        Fixture::new(kind).await.replay(trace).await;
                    })
                    .await
                    .unwrap();
                }
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn encoders_apply_to_existing_and_future_connections_on_every_exception_channel() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for kind in 0..4 {
                let mut f = Fixture::new(kind).await;
                f.configure(1);
                f.connect().await;
                f.configure(2);
                f.emit(0, 2).await;
                f.configure(3);
                f.emit(1, 0).await;
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn rejected_capability_resolution_preserves_error() {
    let cap: harness::Client = capnp_rpc::new_future_client(async {
        Err(Error::disconnected("resolution failed".into()))
    });
    let error = cap.client.hook.when_resolved().await.unwrap_err();
    assert_eq!(error.kind, capnp::ErrorKind::Disconnected);
    assert_eq!(error.extra, "resolution failed");
    let error = cap.echo_request().send().promise.await.err().unwrap();
    assert_eq!(error.kind, capnp::ErrorKind::Disconnected);
    assert_eq!(error.extra, "resolution failed");
}
