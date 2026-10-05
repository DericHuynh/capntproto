#[allow(dead_code)]
mod support;

use capnp::capability::FromClientHook;
use capnp_rpc::RpcSystem;
use capntproto_test_support::structured::rpc_api_capnp::{base, service};
use futures::{channel::oneshot, FutureExt};
use std::{
    cell::{Cell, RefCell},
    future::Future,
    rc::Rc,
    time::Duration,
};

struct Echo(Rc<Cell<u32>>);
impl base::Server for Echo {
    async fn echo(self: Rc<Self>, p: base::EchoParams, r: base::EchoResults) -> capnp::Result<()> {
        self.0.set(self.0.get() + 1);
        r.complete(|mut out| {
            out.set_value(p.get()?.get_value());
            Ok(())
        })
    }
}
struct Service {
    child: base::Client,
    gate: RefCell<Option<oneshot::Receiver<bool>>>,
    published: Rc<Cell<bool>>,
    streams: Rc<Cell<usize>>,
}
impl base::Server for Service {
    async fn echo(self: Rc<Self>, p: base::EchoParams, r: base::EchoResults) -> capnp::Result<()> {
        let mut call = self.child.echo_request();
        call.get().set_value(p.get()?.get_value());
        r.tail_call(call).await
    }
}
impl service::Server for Service {
    async fn open(
        self: Rc<Self>,
        _: service::OpenParams,
        r: service::OpenResults,
    ) -> capnp::Result<()> {
        let mut reply = r.build();
        reply.edit()?.set_cap(self.child.clone());
        let gate = self.gate.borrow_mut().take();
        if let Some(gate) = gate {
            let published = reply.publish()?;
            self.published.set(true);
            if gate
                .await
                .map_err(|_| capnp::Error::failed("gate dropped".into()))?
            {
                published.finish()
            } else {
                Err(capnp::Error::failed("late parent failure".into()))
            }
        } else {
            reply.finish()
        }
    }
    async fn forward(
        self: Rc<Self>,
        p: service::ForwardParams,
        r: service::ForwardResults,
    ) -> capnp::Result<()> {
        let p = p.get()?;
        let mut request = p.get_cap()?.echo_request();
        request.get().set_value(p.get_value());
        r.tail_call(request).await
    }
    async fn forward_cap(
        self: Rc<Self>,
        p: service::ForwardCapParams,
        r: service::ForwardCapResults,
    ) -> capnp::Result<()> {
        r.tail_call(p.get()?.get_cap()?.open_request()).await
    }
    async fn stream(self: Rc<Self>, _: service::StreamParams) -> capnp::Result<()> {
        self.streams.set(self.streams.get() + 1);
        Ok(())
    }
}
struct Drivers(Vec<tokio::task::JoinHandle<capnp::Result<()>>>);
impl Drop for Drivers {
    fn drop(&mut self) {
        for driver in &self.0 {
            driver.abort();
        }
    }
}
async fn run(future: impl Future<Output = ()>) {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), future)
                .await
                .unwrap();
        })
        .await;
}
async fn settle() {
    for _ in 0..32 {
        tokio::task::yield_now().await;
    }
}
fn service(
    gate: Option<oneshot::Receiver<bool>>,
    published: Rc<Cell<bool>>,
    calls: Rc<Cell<u32>>,
    streams: Rc<Cell<usize>>,
) -> service::Client {
    capnp_rpc::new_client(Service {
        child: capnp_rpc::new_client(Echo(calls)),
        gate: RefCell::new(gate),
        published,
        streams,
    })
}
fn connect(server: service::Client, wire: bool) -> (service::Client, Drivers) {
    if !wire {
        return (server, Drivers(vec![]));
    }
    let hub = Rc::new(RefCell::new(support::Hub::default()));
    let host = RpcSystem::new(
        Box::new(support::Hub::network(&hub, 1)),
        Some(server.client),
    );
    let mut caller = RpcSystem::new(Box::new(support::Hub::network(&hub, 2)), None);
    let client = caller.bootstrap(1);
    (
        client,
        Drivers(vec![
            tokio::task::spawn_local(host),
            tokio::task::spawn_local(caller),
        ]),
    )
}
async fn echo(client: &base::Client, value: u32) -> capnp::Result<u32> {
    Ok(client
        .echo_request()
        .with_params(|mut p| {
            p.set_value(value);
            Ok(())
        })?
        .send()
        .await?
        .get()?
        .get_value())
}

#[tokio::test(flavor = "current_thread")]
async fn non_pipelined_calls_keep_pending_wakes_and_late_errors() {
    struct DelayedEcho {
        gate: RefCell<Option<oneshot::Receiver<bool>>>,
        polls: Rc<Cell<usize>>,
    }
    impl base::Server for DelayedEcho {
        async fn echo(
            self: Rc<Self>,
            p: base::EchoParams,
            r: base::EchoResults,
        ) -> capnp::Result<()> {
            // The first poll wakes itself, but still returns Pending. A later
            // independent wake must reach the scheduled continuation as well.
            futures::future::poll_fn(|cx| {
                let n = self.polls.get();
                self.polls.set(n + 1);
                if n == 0 {
                    cx.waker().wake_by_ref();
                    std::task::Poll::Pending
                } else {
                    std::task::Poll::Ready(())
                }
            })
            .await;
            let gate = self.gate.borrow_mut().take().unwrap();
            if !gate
                .await
                .map_err(|_| capnp::Error::failed("gate dropped".into()))?
            {
                return Err(capnp::Error::failed("late echo failure".into()));
            }
            r.complete(|mut out| {
                out.set_value(p.get()?.get_value());
                Ok(())
            })
        }
    }
    run(async {
        for succeed in [true, false] {
            let hub = Rc::new(RefCell::new(support::Hub::default()));
            let (release, gate) = oneshot::channel();
            let polls = Rc::new(Cell::new(0));
            let service: base::Client = capnp_rpc::new_client(DelayedEcho {
                gate: RefCell::new(Some(gate)),
                polls: polls.clone(),
            });
            let host = RpcSystem::new(
                Box::new(support::Hub::network(&hub, 1)),
                Some(service.client),
            );
            let mut caller = RpcSystem::new(Box::new(support::Hub::network(&hub, 2)), None);
            let client: base::Client = caller.bootstrap(1);
            let _drivers = Drivers(vec![
                tokio::task::spawn_local(host),
                tokio::task::spawn_local(caller),
            ]);
            let mut request = client.echo_request();
            request.hook.set_hints(capnp::capability::CallHints {
                no_promise_pipelining: true,
                ..Default::default()
            });
            request.get().set_value(73);
            let mut response = request.send().promise;
            settle().await;
            assert!(polls.get() >= 2, "pending call was never rescheduled");
            assert!((&mut response).now_or_never().is_none());
            release.send(succeed).unwrap();
            match response.await {
                Ok(reply) => {
                    assert!(succeed);
                    assert_eq!(reply.get().unwrap().get_value(), 73);
                }
                Err(error) => {
                    assert!(!succeed);
                    assert!(error.extra.contains("late echo failure"), "{error}");
                }
            }
        }
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn structured_replies_support_inheritance_typed_tail_calls_and_legacy_clients() {
    run(async {
        for wire in [false, true] {
            let streams = Rc::new(Cell::new(0));
            let (client, _drivers) = connect(
                service(
                    None,
                    Default::default(),
                    Default::default(),
                    streams.clone(),
                ),
                wire,
            );
            let inherited: base::Client = client.clone().cast_to();
            assert_eq!(echo(&inherited, 19).await.unwrap(), 19);
            let callback: base::Client = capnp_rpc::new_client(Echo(Default::default()));
            let mut call = client.forward_request();
            call.get().set_cap(callback);
            call.get().set_value(42);
            assert_eq!(call.send().await.unwrap().get().unwrap().get_value(), 42);
            let mut forwarded = client.forward_cap_request();
            forwarded.get().set_cap(client.clone());
            let (completion, pipeline) = forwarded.send().into_parts();
            assert_eq!(echo(&pipeline.get_cap(), 73).await.unwrap(), 73);
            let cap = completion.await.unwrap().get().unwrap().get_cap().unwrap();
            assert_eq!(echo(&cap, 74).await.unwrap(), 74);
            client.stream_request().send().await.unwrap();
            // An ordinary call is the completion barrier for a streaming send.
            assert_eq!(echo(&inherited, 75).await.unwrap(), 75);
            assert_eq!(streams.get(), 1);
            let legacy: capntproto_test_support::rpc_api_capnp::service::Client =
                client.clone().cast_to();
            let cap = legacy
                .open_request()
                .send()
                .await
                .unwrap()
                .get()
                .unwrap()
                .get_cap()
                .unwrap();
            let mut call = cap.echo_request();
            call.get().set_value(76);
            assert_eq!(call.send().await.unwrap().get().unwrap().get_value(), 76);
            // Existing field editors work with the same structured-reply server.
            let mut call = client.forward_call();
            call.edit().cap().copy_from(inherited.clone()).unwrap();
            call.edit().value().set(77);
            assert_eq!(call.send().await.unwrap().read().unwrap().value(), 77);
        }
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn frozen_publication_runs_children_before_parent_and_preserves_late_error() {
    run(async {
        for wire in [false, true] {
            for success in [false, true] {
                let (gate, receiver) = oneshot::channel();
                let published = Rc::new(Cell::new(false));
                let calls = Rc::new(Cell::new(0));
                let (client, _drivers) = connect(
                    service(
                        Some(receiver),
                        published.clone(),
                        calls.clone(),
                        Default::default(),
                    ),
                    wire,
                );
                let mut parent = client.open_request().send();
                let child = parent.pipeline.get_cap();
                assert_eq!(echo(&child, 51).await.unwrap(), 51);
                assert!(published.get());
                assert_eq!(calls.get(), 1);
                assert!((&mut parent).now_or_never().is_none());
                gate.send(success).unwrap();
                let response = parent.await;
                if success {
                    let returned = response.unwrap().get().unwrap().get_cap().unwrap();
                    assert_eq!(echo(&returned, 52).await.unwrap(), 52);
                } else {
                    assert!(response
                        .err()
                        .unwrap()
                        .extra
                        .contains("late parent failure"));
                }
                // A late wire exception can break the unresolved parent path.
                // The already completed child effect is never rolled back.
                assert!(calls.get() >= 1);
                if wire && !success {
                    assert!(echo(&child, 53).await.is_err());
                } else {
                    assert_eq!(echo(&child, 53).await.unwrap(), 53);
                }
            }
        }
    })
    .await;
}

struct Proxy(service::Client);
impl base::Server for Proxy {}
impl service::Server for Proxy {
    async fn open(
        self: Rc<Self>,
        _: service::OpenParams,
        r: service::OpenResults,
    ) -> capnp::Result<()> {
        r.tail_call(self.0.open_request()).await
    }
}
struct Factory(RefCell<Option<capnp::capability::Client>>);
impl capnp_rpc::BootstrapFactory<u8> for Factory {
    fn create_for(&self, _: &u8) -> capnp::Result<capnp::capability::Client> {
        Ok(self.0.borrow().as_ref().unwrap().clone())
    }
}
#[tokio::test(flavor = "current_thread")]
async fn typed_tail_forwarding_preserves_third_party_adoption() {
    run(async {
        let hub = Rc::new(RefCell::new(support::Hub::default()));
        hub.borrow_mut().introductions = true;
        hub.borrow_mut().answer_adoption = true;
        let a = support::Hub::network(&hub, 1);
        let b = support::Hub::network(&hub, 2);
        let c = support::Hub::network(&hub, 3);
        let host = RpcSystem::new(
            Box::new(c),
            Some(
                service(
                    None,
                    Default::default(),
                    Default::default(),
                    Default::default(),
                )
                .client,
            ),
        );
        let factory = Rc::new(Factory(RefCell::new(None)));
        let mut relay = RpcSystem::new_with_bootstrap_factory(Box::new(b), 2, factory.clone());
        let proxy: service::Client = capnp_rpc::new_client(Proxy(relay.bootstrap(3)));
        *factory.0.borrow_mut() = Some(proxy.client);
        let mut caller = RpcSystem::new(Box::new(a), None);
        let client: service::Client = caller.bootstrap(2);
        let drivers = Drivers(vec![
            tokio::task::spawn_local(host),
            tokio::task::spawn_local(relay),
            tokio::task::spawn_local(caller),
        ]);
        let parent = client.open_request().send();
        assert_eq!(echo(&parent.pipeline.get_cap(), 90).await.unwrap(), 90);
        let cap = parent.await.unwrap().get().unwrap().get_cap().unwrap();
        assert!(
            hub.borrow_mut()
                .connect(3, 1)
                .status()
                .borrow()
                .adoptions_sent
                > 0
        );
        drivers.0[1].abort();
        settle().await;
        assert_eq!(echo(&cap, 91).await.unwrap(), 91);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn failed_parameter_construction_releases_admission_without_sending() {
    run(async {
        let hub = Rc::new(RefCell::new(support::Hub::default()));
        let calls = Rc::new(Cell::new(0));
        let host = RpcSystem::new(
            Box::new(support::Hub::network(&hub, 1)),
            Some(service(None, Default::default(), calls.clone(), Default::default()).client),
        );
        let mut caller = RpcSystem::new(Box::new(support::Hub::network(&hub, 2)), None);
        caller.set_outgoing_call_limit(1);
        let diagnostics = caller.diagnostics();
        let client: service::Client = caller.bootstrap(1);
        let _drivers = Drivers(vec![
            tokio::task::spawn_local(host),
            tokio::task::spawn_local(caller),
        ]);
        client.client.when_resolved().await.unwrap();
        let inherited: base::Client = client.clone().cast_to();
        let error = inherited
            .echo_request()
            .with_params(|mut p| {
                p.set_value(999);
                Err(capnp::Error::failed("invalid input".into()))
            })
            .err()
            .unwrap();
        assert_eq!(error.extra, "invalid input");
        assert_eq!(diagnostics.snapshot()[0].outgoing_calls, 0);
        let _ = client
            .forward_call()
            .with_params(|mut p| {
                p.value().set(999);
                Err(capnp::Error::failed("invalid field input".into()))
            })
            .err()
            .unwrap();
        assert_eq!(diagnostics.snapshot()[0].outgoing_calls, 0);
        assert_eq!(hub.borrow_mut().connect(2, 1).status().borrow().calls, 0);
        assert_eq!(calls.get(), 0);
        assert_eq!(echo(&inherited, 42).await.unwrap(), 42);
    })
    .await;
}
