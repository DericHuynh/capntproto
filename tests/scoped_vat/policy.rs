use super::*;
use std::{cell::RefCell, future::Future, pin::Pin};

fn options() -> capnp_rpc::portable::Options {
    capnp_rpc::portable::Options::bounded(
        NonZeroUsize::new(1).unwrap(),
        NonZeroUsize::new(64).unwrap(),
        NonZeroUsize::new(128 << 10).unwrap(),
        NonZeroUsize::new(8 << 20).unwrap(),
    )
    .unwrap()
}
async fn networks() -> (Box<dyn VatNetwork<Side>>, Box<dyn VatNetwork<Side>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (a, b) = tokio::join!(
        tokio::net::TcpStream::connect(listener.local_addr().unwrap()),
        listener.accept()
    );
    (
        Box::new(capntproto::rpc::tcp::network(
            a.unwrap(),
            Side::Client,
            Default::default(),
        )),
        Box::new(capntproto::rpc::tcp::network(
            b.unwrap().0,
            Side::Server,
            Default::default(),
        )),
    )
}

#[tokio::test(flavor = "current_thread")]
async fn zero_flow_limit_pauses_after_one_call_and_live_increase_resumes_tcp() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let (a, b) = networks().await;
        let calls = Rc::new(Cell::new(0));
        let mut client = capnp_rpc::RpcSystem::new(a, None);
        let server = Rc::new(RefCell::new(
            capnp_rpc::portable::RpcSystem::new(b, Some(echo(calls.clone())), options()).unwrap(),
        ));
        server.borrow_mut().set_flow_limit(0);
        let remote: Client = client.bootstrap(Side::Server);
        let cclose = client.get_disconnector();
        let sclose = server.borrow().get_disconnector();
        let drive = std::future::poll_fn(|cx| Pin::new(&mut *server.borrow_mut()).poll(cx));
        let operation = async {
            remote.when_resolved().await.unwrap(); // Bootstrap is not a Call charge.
            let first = payload(&remote, 11);
            let second = payload(&remote, 12);
            futures::pin_mut!(first, second);
            assert!(first.as_mut().now_or_never().is_none());
            assert!(second.as_mut().now_or_never().is_none());
            first.await;
            for _ in 0..32 {
                tokio::task::yield_now().await;
            }
            assert!(second.as_mut().now_or_never().is_none());
            assert_eq!(calls.get(), 1);
            server.borrow_mut().set_flow_limit(1);
            second.await; // The setter must wake a parked reader.
            payload(&remote, 13).await; // Nonzero credit resumes after Return.
            server.borrow_mut().set_flow_limit(0);
            payload(&remote, 14).await;
            assert_eq!(calls.get(), 4);
            // Shutdown must still be polled while the reader is paused.
            let (c, s) = futures::join!(cclose, sclose);
            c.unwrap();
            s.unwrap();
        };
        let (c, s, ()) = futures::join!(client, drive, operation);
        c.unwrap();
        s.unwrap();
    })
    .await
    .expect("flow policy must not strand input or shutdown");
}

#[tokio::test(flavor = "current_thread")]
async fn bootstrap_traces_follow_live_opt_in_policy_and_never_echo_remote_trace() {
    struct Deny;
    impl capnp_rpc::BootstrapFactory<Side> for Deny {
        fn create_for(&self, _: &Side) -> capnp::Result<Client> {
            let mut error = capnp::Error::overloaded("policy denied".into());
            error.set_remote_trace("untrusted remote trace".into());
            Err(error)
        }
    }
    tokio::time::timeout(Duration::from_secs(10), async {
        let (a, b) = networks().await;
        let client = capnp_rpc::RpcSystem::new(a, None);
        let server = Rc::new(RefCell::new(
            capnp_rpc::portable::RpcSystem::new_with_bootstrap_factory(
                b,
                Side::Server,
                Rc::new(Deny),
                options(),
            )
            .unwrap(),
        ));
        let bootstrap = client.get_bootstrapper();
        let cclose = client.get_disconnector();
        let sclose = server.borrow().get_disconnector();
        let drive = std::future::poll_fn(|cx| Pin::new(&mut *server.borrow_mut()).poll(cx));
        let encoded = Rc::new(Cell::new(0));
        let operation = async {
            for mode in 0..4 {
                if mode == 1 || mode == 2 {
                    let encoded = encoded.clone();
                    server.borrow_mut().set_trace_encoder(move |error| {
                        encoded.set(encoded.get() + 1);
                        assert_eq!(error.extra, "policy denied");
                        format!("local {mode}: {}", "trace".repeat(2048))
                    });
                } else if mode == 3 {
                    server.borrow_mut().clear_trace_encoder();
                }
                let cap: Client = bootstrap.bootstrap(Side::Server);
                let error = cap.when_resolved().await.unwrap_err();
                assert_eq!(error.kind, capnp::ErrorKind::Overloaded);
                assert_eq!(error.extra, "remote exception: policy denied");
                if mode == 1 || mode == 2 {
                    assert_eq!(
                        error.remote_trace(),
                        Some(format!("local {mode}: {}", "trace".repeat(2048)).as_str())
                    );
                } else {
                    assert_eq!(error.remote_trace(), None);
                }
            }
            assert_eq!(encoded.get(), 2);
            let (c, s) = futures::join!(cclose, sclose);
            c.unwrap();
            s.unwrap();
        };
        let (c, s, ()) = futures::join!(client, drive, operation);
        c.unwrap();
        s.unwrap();
    })
    .await
    .expect("trace policy must preserve connection reuse");
}

#[tokio::test(flavor = "current_thread")]
async fn outgoing_limit_counts_unsent_requests_responses_and_pipelines_without_disconnect() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let (a, b) = networks().await;
        let calls = Rc::new(Cell::new(0));
        let server = capnp_rpc::RpcSystem::new(b, Some(echo(calls.clone())));
        let client = Rc::new(RefCell::new(
            capnp_rpc::portable::RpcSystem::new(a, None, options()).unwrap(),
        ));
        client.borrow_mut().set_outgoing_call_limit(0);
        let remote: Client = client.borrow_mut().bootstrap(Side::Server);
        let diagnostics = client.borrow().diagnostics();
        let cclose = client.borrow().get_disconnector();
        let sclose = server.get_disconnector();
        let drive = std::future::poll_fn(|cx| Pin::new(&mut *client.borrow_mut()).poll(cx));
        let operation = async {
            // An unresolved destination acquires credit only when routed remotely.
            let rejected = remote
                .new_call::<any_pointer::Owned, any_pointer::Owned>(0, 0, None)
                .send();
            assert_eq!(
                rejected.promise.await.err().unwrap().kind,
                capnp::ErrorKind::Overloaded
            );
            remote.when_resolved().await.unwrap();
            assert_eq!(calls.get(), 0);
            assert_eq!(diagnostics.snapshot()[0].outgoing_calls, 0);
            client.borrow_mut().set_outgoing_call_limit(1);
            let request = remote.new_call::<any_pointer::Owned, any_pointer::Owned>(0, 0, None);
            assert_eq!(diagnostics.snapshot()[0].outgoing_calls, 1);
            let rejected = remote
                .new_call::<any_pointer::Owned, any_pointer::Owned>(0, 0, None)
                .send();
            assert_eq!(
                rejected.promise.await.err().unwrap().kind,
                capnp::ErrorKind::Overloaded
            );
            drop(request);
            assert_eq!(diagnostics.snapshot()[0].outgoing_calls, 0);
            let sent = remote
                .new_call::<any_pointer::Owned, any_pointer::Owned>(0, 0, None)
                .send();
            let response = sent.promise.await.unwrap();
            client.borrow_mut().set_outgoing_call_limit(0); // Never revoke accepted work.
            let snap = diagnostics.snapshot()[0];
            assert_eq!(snap.outgoing_calls, 1);
            assert_eq!(snap.held_responses, 1);
            let response = Rc::new(response);
            let clone = response.clone();
            drop(response);
            drop(sent.pipeline);
            assert_eq!(diagnostics.snapshot()[0].held_responses, 1);
            drop(clone);
            // Response contexts release synchronously, even before the next poll.
            assert_eq!(diagnostics.snapshot()[0].held_responses, 0);
            for _ in 0..64 {
                tokio::task::yield_now().await;
            }
            assert_eq!(diagnostics.snapshot()[0].outgoing_calls, 0);
            client.borrow_mut().set_outgoing_call_limit(1);
            let sent = remote
                .new_call::<any_pointer::Owned, any_pointer::Owned>(0, 0, None)
                .send();
            drop(sent.promise.await.unwrap());
            assert_eq!(diagnostics.snapshot()[0].held_responses, 1); // Pipeline retains context.
            drop(sent.pipeline);
            for _ in 0..64 {
                tokio::task::yield_now().await;
            }
            assert_eq!(diagnostics.snapshot()[0].outgoing_calls, 0);
            payload(&remote, 19).await;
            assert_eq!(calls.get(), 3);
            let (c, s) = futures::join!(cclose, sclose);
            c.unwrap();
            s.unwrap();
        };
        let (c, s, ()) = futures::join!(drive, server, operation);
        c.unwrap();
        s.unwrap();
    })
    .await
    .expect("outgoing rejection must preserve connection progress");
}

#[tokio::test(flavor = "current_thread")]
async fn queued_and_membrane_calls_retain_response_credit_and_local_calls_ignore_remote_limit() {
    struct Allow;
    impl capnp_rpc::membrane::Policy for Allow {
        fn call(
            &self,
            _: capnp_rpc::membrane::Direction,
            _: u64,
            _: u16,
            _: &Client,
        ) -> capnp::Result<Option<Client>> {
            Ok(None)
        }
    }
    tokio::time::timeout(Duration::from_secs(10), async {
        let (a, b) = networks().await;
        let calls = Rc::new(Cell::new(0));
        let local_calls = Rc::new(Cell::new(0));
        let server = capnp_rpc::RpcSystem::new(b, Some(echo(calls.clone())));
        let client = Rc::new(RefCell::new(
            capnp_rpc::portable::RpcSystem::new(a, Some(echo(local_calls.clone())), options())
                .unwrap(),
        ));
        client.borrow_mut().set_outgoing_call_limit(1);
        let remote: Client = client.borrow_mut().bootstrap(Side::Server);
        let local: Client = client.borrow_mut().bootstrap(Side::Client);
        let diagnostics = client.borrow().diagnostics();
        let cclose = client.borrow().get_disconnector();
        let sclose = server.get_disconnector();
        let drive = std::future::poll_fn(|cx| Pin::new(&mut *client.borrow_mut()).poll(cx));
        let operation = async {
            remote.when_resolved().await.unwrap();
            for wrapped in [false, true] {
                let membrane = capnp_rpc::membrane::Membrane::new(Rc::new(Allow));
                let target = if wrapped {
                    membrane.export(remote.clone())
                } else {
                    remote.clone()
                };
                let queued: Client = capnp_rpc::new_future_client(std::future::ready(Ok(target)));
                // This request is created before the generic queued client resolves.
                let sent = queued
                    .new_call::<any_pointer::Owned, any_pointer::Owned>(0, 0, None)
                    .send();
                drop(sent.pipeline);
                let response = sent.promise.await.unwrap();
                for _ in 0..32 {
                    tokio::task::yield_now().await;
                }
                assert_eq!(diagnostics.snapshot()[0].held_responses, 1);
                assert_eq!(diagnostics.snapshot()[0].outgoing_calls, 1);
                let blocked = remote
                    .new_call::<any_pointer::Owned, any_pointer::Owned>(0, 0, None)
                    .send();
                assert_eq!(
                    blocked.promise.await.err().unwrap().kind,
                    capnp::ErrorKind::Overloaded
                );
                drop(response);
                assert_eq!(diagnostics.snapshot()[0].held_responses, 0);
                for _ in 0..32 {
                    tokio::task::yield_now().await;
                }
                assert_eq!(diagnostics.snapshot()[0].outgoing_calls, 0);
            }
            client.borrow_mut().set_outgoing_call_limit(0);
            payload(&local, 31).await;
            assert_eq!(local_calls.get(), 1);
            assert_eq!(calls.get(), 2);
            assert_eq!(diagnostics.snapshot()[0].outgoing_calls, 0);
            let (c, s) = futures::join!(cclose, sclose);
            c.unwrap();
            s.unwrap();
        };
        let (c, s, ()) = futures::join!(drive, server, operation);
        c.unwrap();
        s.unwrap();
    })
    .await
    .expect("queued and membrane forwarding must preserve response ownership");
}
