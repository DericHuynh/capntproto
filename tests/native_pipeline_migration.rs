use capnp::{capability::FromClientHook, Error};
use capntproto::{
    native_rpc::{Handle, Network, Options},
    transport::{self, Identity},
};
use capntproto_test_support::runtime_test_capnp::harness;
use futures::channel::oneshot;
use std::{cell::RefCell, rc::Rc, time::Duration};

struct Task(tokio::task::JoinHandle<capnp::Result<()>>);
impl Drop for Task {
    fn drop(&mut self) {
        self.0.abort();
    }
}
struct Factory(RefCell<Option<harness::Client>>);
impl capnp_rpc::BootstrapFactory<[u8; 32]> for Factory {
    fn create_for(&self, _: &[u8; 32]) -> capnp::Result<capnp::capability::Client> {
        Ok(self.0.borrow().as_ref().unwrap().client.clone())
    }
}
struct Tail(harness::Client, Rc<RefCell<Option<harness::Client>>>);
impl harness::Server for Tail {
    async fn bounce(
        self: Rc<Self>,
        p: harness::BounceParams,
        _: harness::BounceResults,
    ) -> capnp::Result<()> {
        *self.1.borrow_mut() = Some(p.get()?.get_cap()?);
        Ok(())
    }
    async fn pending(
        self: Rc<Self>,
        _: harness::PendingParams,
        r: harness::PendingResults,
    ) -> capnp::Result<()> {
        r.hook.tail_call(self.0.pending_request().hook).await
    }
}
struct Echo(Rc<RefCell<Vec<u32>>>);
impl harness::Server for Echo {
    async fn echo(
        self: Rc<Self>,
        p: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        let value = p.get()?.get_value();
        self.0.borrow_mut().push(value);
        r.get().set_value(value);
        Ok(())
    }
}
struct Early {
    cap: harness::Client,
    publish: RefCell<Option<oneshot::Receiver<()>>>,
    gate: RefCell<Option<oneshot::Receiver<()>>>,
}
impl harness::Server for Early {
    async fn pending(
        self: Rc<Self>,
        _: harness::PendingParams,
        mut r: harness::PendingResults,
    ) -> capnp::Result<()> {
        let publish = self.publish.borrow_mut().take().unwrap();
        publish
            .await
            .map_err(|_| Error::failed("publish dropped".into()))?;
        r.get().set_cap(self.cap.clone());
        r.hook.set_pipeline()?;
        let gate = self.gate.borrow_mut().take().unwrap();
        gate.await.map_err(|_| Error::failed("gate dropped".into()))
    }
}
async fn pair(a: &Identity, b: &Identity, ah: &Handle, bh: &Handle) {
    let left = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let right = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = right.local_addr().unwrap();
    let (a, b) = tokio::join!(
        transport::connect_authenticated(left, address, a, b.public_key(), None, b"pipeline"),
        transport::accept_authenticated(right, b, a.public_key(), None, b"pipeline")
    );
    ah.attach(a.unwrap()).unwrap();
    bh.attach(b.unwrap()).unwrap();
}
async fn echo(cap: &harness::Client, n: u32) {
    let mut request = cap.echo_request();
    request.get().set_value(n);
    assert_eq!(
        request
            .send()
            .promise
            .await
            .unwrap()
            .get()
            .unwrap()
            .get_value(),
        n
    );
}

#[tokio::test(flavor = "current_thread")]
async fn already_used_pipeline_joins_old_path_and_survives_relay_loss() {
    migration(false).await;
}
#[tokio::test(flavor = "current_thread")]
async fn migration_deadline_keeps_ordered_relay_and_does_not_replay_calls() {
    migration(true).await;
}
#[tokio::test(flavor = "current_thread")]
async fn encoded_pipeline_reference_migrates_without_rebinding_other_holders() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let mut fixture = Fixture::setup(Duration::from_secs(2), true).await;
                fixture.prove().await;
                let holder = fixture.retained.borrow().as_ref().unwrap().clone();
                echo(&holder, 1).await;
                echo(&fixture.used, 2).await;
                fixture.lose_relay();
                echo(&fixture.used, 3).await;
                assert!(holder.echo_request().send().promise.await.is_err());
                assert_eq!(*fixture.log.borrow(), vec![1, 2, 3]);
            })
            .await
            .unwrap();
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn joined_pipeline_capability_survives_parent_drop_before_return() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let mut fixture = Fixture::new(Duration::from_secs(2)).await;
                fixture.publish.take().unwrap().send(()).unwrap();
                fixture.used.as_client_hook().when_resolved().await.unwrap();
                assert!(fixture.host.join_stats().accepted > 0);
                for call in fixture.old_calls.drain(..) {
                    call.await.unwrap();
                }
                // No final Return: release both the original response and pipeline.
                drop(fixture.result.take());
                fixture.lose_relay();
                echo(&fixture.used, 3).await;
                assert_eq!(*fixture.log.borrow(), vec![1, 2, 3]);
            })
            .await
            .unwrap();
        })
        .await;
}
struct Fixture {
    used: harness::Client,
    gate: Option<oneshot::Sender<()>>,
    publish: Option<oneshot::Sender<()>>,
    old_calls:
        Vec<capnp::capability::Promise<capnp::capability::Response<harness::value::Owned>, Error>>,
    result: Option<capnp::capability::RemotePromise<harness::pending_results::Owned>>,
    log: Rc<RefCell<Vec<u32>>>,
    caller: Handle,
    relay: Handle,
    host: Handle,
    retained: Rc<RefCell<Option<harness::Client>>>,
    a: [u8; 32],
    b: [u8; 32],
    c: [u8; 32],
    _tasks: Vec<Task>,
}
impl Fixture {
    async fn new(timeout: Duration) -> Self {
        Self::setup(timeout, false).await
    }
    async fn setup(timeout: Duration, encoded: bool) -> Self {
        let a = Identity::generate();
        let b = Identity::generate();
        let c = Identity::generate();
        let (an, ah) = Network::with_options(
            a.public_key(),
            None,
            Options {
                answer_setup_timeout: timeout,
                ..Options::default()
            },
        )
        .unwrap();
        let (bn, bh) = Network::new(b.public_key());
        let (cn, ch) = Network::new(c.public_key());
        pair(&a, &b, &ah, &bh).await;
        pair(&b, &c, &bh, &ch).await;
        pair(&a, &c, &ah, &ch).await;
        let log = Rc::new(RefCell::new(Vec::new()));
        let (gate, wait) = oneshot::channel();
        let (publish, publishing) = oneshot::channel();
        let early: harness::Client = capnp_rpc::new_client(Early {
            cap: capnp_rpc::new_client(Echo(log.clone())),
            publish: RefCell::new(Some(publishing)),
            gate: RefCell::new(Some(wait)),
        });
        let host = Task(tokio::task::spawn_local(capnp_rpc::RpcSystem::new(
            Box::new(cn),
            Some(early.client),
        )));
        let factory = Rc::new(Factory(RefCell::new(None)));
        let mut relay = capnp_rpc::RpcSystem::new_with_bootstrap_factory(
            Box::new(bn),
            b.public_key(),
            factory.clone(),
        );
        let remote: harness::Client = relay.bootstrap(c.public_key());
        let retained = Rc::new(RefCell::new(None));
        *factory.0.borrow_mut() = Some(capnp_rpc::new_client(Tail(remote, retained.clone())));
        let relay = Task(tokio::task::spawn_local(relay));
        let mut caller = capnp_rpc::RpcSystem::new(Box::new(an), None);
        let relay_cap: harness::Client = caller.bootstrap(b.public_key());
        let caller = Task(tokio::task::spawn_local(caller));
        let result = relay_cap.pending_request().send();
        let used = result.pipeline.get_cap();
        // Both sends precede any possible redirect/adoption.
        let old_calls = if encoded {
            let mut export = relay_cap.bounce_request();
            export.get().set_cap(used.clone());
            export.send().promise.await.unwrap();
            vec![]
        } else {
            let mut first = used.echo_request();
            first.get().set_value(1);
            let first = first.send().promise;
            let mut second = used.echo_request();
            second.get().set_value(2);
            let second = second.send().promise;
            vec![first, second]
        };
        while used.as_client_hook().get_resolved().is_none() {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            used.as_client_hook().get_resolved().unwrap().get_brand(),
            0,
            "active reference must be fenced behind a local queue"
        );
        Self {
            used,
            gate: Some(gate),
            publish: Some(publish),
            old_calls,
            result: Some(result),
            log,
            caller: ah,
            relay: bh,
            host: ch,
            retained,
            a: a.public_key(),
            b: b.public_key(),
            c: c.public_key(),
            _tasks: vec![host, relay, caller],
        }
    }
    async fn prove(&mut self) {
        self.publish.take().unwrap().send(()).unwrap();
        self.used.as_client_hook().when_resolved().await.unwrap();
        assert!(
            self.host.join_stats().accepted > 0,
            "migration must acquire the joined capability"
        );
        self.finish().await;
    }
    async fn expire(&mut self) {
        self.used.as_client_hook().when_resolved().await.unwrap();
        assert_eq!(
            self.host.join_stats().accepted,
            0,
            "unpublished pipeline must not authenticate a Join"
        );
        self.publish.take().unwrap().send(()).unwrap();
        self.finish().await;
    }
    async fn finish(&mut self) {
        // The ordering fence must finish before the final method Return.
        self.gate.take().unwrap().send(()).unwrap();
        self.result.take().unwrap().promise.await.unwrap();
        for (i, call) in self.old_calls.drain(..).enumerate() {
            assert_eq!(call.await.unwrap().get().unwrap().get_value(), i as u32 + 1);
        }
    }
    fn lose_relay(&self) {
        self.caller.disconnect(self.b);
        self.relay.disconnect(self.a);
        self.relay.disconnect(self.c);
    }
}
async fn migration(expire: bool) {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(8), async {
                let mut fixture =
                    Fixture::new(Duration::from_millis(if expire { 200 } else { 2000 })).await;
                if expire {
                    fixture.expire().await;
                } else {
                    fixture.prove().await;
                }
                echo(&fixture.used, 3).await;
                fixture.lose_relay();
                if expire {
                    assert!(fixture.used.echo_request().send().promise.await.is_err());
                    assert_eq!(*fixture.log.borrow(), vec![1, 2, 3]);
                } else {
                    echo(&fixture.used, 4).await;
                    assert_eq!(*fixture.log.borrow(), vec![1, 2, 3, 4]);
                }
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_active_pipeline_fence() {
    use capntproto_test_support::verification::exploration;
    use futures::FutureExt;
    let config = include_str!("../verification/RpcPipelineFence.cfg");
    let live = config.replace("SPECIFICATION Spec", "SPECIFICATION LiveSpec")
        + "\nPROPERTIES Settles CallsSettle\n";
    exploration::controls(
        "verification/RpcPipelineFence.tla",
        "pipeline-fence",
        config,
        &[
            ("early", "QueueUntilProof"),
            ("fallback", "Authenticated"),
            ("cancellation", "Cancellation"),
        ],
        Some(&live),
    )
    .unwrap();
    let traces = exploration::traces(
        "verification/RpcPipelineFence.tla",
        "pipeline-fence",
        config,
    )
    .unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for trace in traces {
                tokio::time::timeout(Duration::from_secs(4), async {
                    let mut fixture = Fixture::new(Duration::from_millis(200)).await;
                    let mut queued = None;
                    for state in trace {
                        match state["event"] {
                            1 => {
                                let mut call = fixture.used.echo_request();
                                call.get().set_value(3);
                                let mut promise = call.send().promise;
                                assert!((&mut promise).now_or_never().is_none());
                                queued = Some(promise);
                            }
                            2 => drop(queued.take()),
                            3 => fixture.prove().await,
                            4 => fixture.expire().await,
                            5 => {
                                assert_eq!(
                                    queued
                                        .take()
                                        .unwrap()
                                        .await
                                        .unwrap()
                                        .get()
                                        .unwrap()
                                        .get_value(),
                                    3
                                );
                            }
                            6 => fixture.lose_relay(),
                            7 => {
                                let mut call = fixture.used.echo_request();
                                call.get().set_value(4);
                                let result = call.send().promise.await;
                                assert_eq!(result.is_ok(), state["route"] == 1);
                            }
                            _ => panic!("unexpected fence event"),
                        }
                        let log = fixture.log.borrow();
                        if state["route"] != 0 {
                            assert_eq!(&log[..2], &[1, 2]);
                        }
                        if state["route"] == 0 || state["canceled"] == 1 {
                            assert!(!log.contains(&3));
                        }
                        if state["delivered"] == 1 {
                            assert_eq!(log.iter().filter(|v| **v == 3).count(), 1);
                        }
                        assert!(log.len() <= 4);
                    }
                })
                .await
                .unwrap();
            }
        })
        .await;
}
