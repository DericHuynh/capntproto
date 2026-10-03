#![cfg(feature = "native")]

use capntproto::{
    native_rpc::{Backend, Options, RouteStatus, Vat},
    rpc::QuicVersion,
    transport::{self, AuthenticatedSession, Identity},
};
use capntproto_test_support::runtime_test_capnp::harness;
use std::{
    cell::{Cell, RefCell},
    future::Future,
    rc::Rc,
    time::Duration,
};

const TIMEOUT: Duration = Duration::from_secs(3);

struct Service {
    stored: Rc<RefCell<Option<harness::Client>>>,
}
impl harness::Server for Service {
    async fn echo(
        self: Rc<Self>,
        params: harness::EchoParams,
        mut results: harness::EchoResults,
    ) -> capnp::Result<()> {
        results.get().set_value(params.get()?.get_value());
        Ok(())
    }
    async fn bounce(
        self: Rc<Self>,
        params: harness::BounceParams,
        mut results: harness::BounceResults,
    ) -> capnp::Result<()> {
        let cap = params.get()?.get_cap()?;
        *self.stored.borrow_mut() = Some(cap.clone());
        results.get().set_cap(cap);
        Ok(())
    }
}
fn service(stored: Rc<RefCell<Option<harness::Client>>>) -> harness::Client {
    capnp_rpc::new_client(Service { stored })
}
async fn echo(cap: &harness::Client, value: u32) -> capnp::Result<()> {
    let mut request = cap.echo_request();
    request.get().set_value(value);
    assert_eq!(request.send().promise.await?.get()?.get_value(), value);
    Ok(())
}
async fn local(test: impl Future<Output = ()>) {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(20), test)
                .await
                .unwrap();
        })
        .await;
}
async fn sessions(
    a: &Identity,
    b: &Identity,
    backend: Backend,
) -> (AuthenticatedSession, AuthenticatedSession) {
    let secret = Some([71; 32]);
    let context = b"native-vat-test";
    let (a, b) = if matches!(backend, Backend::Tcp) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        tokio::join!(
            transport::tcp::connect(
                listener.local_addr().unwrap(),
                a,
                b.public_key(),
                secret,
                context,
                TIMEOUT
            ),
            async {
                transport::tcp::accept(
                    listener.accept().await.unwrap().0,
                    b,
                    a.public_key(),
                    secret,
                    context,
                    TIMEOUT,
                )
                .await
            }
        )
    } else {
        let left = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let right = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let version = if matches!(backend, Backend::QuicheV2) {
            QuicVersion::V2
        } else {
            QuicVersion::V1
        };
        tokio::join!(
            transport::connect_for_version(
                left,
                right.local_addr().unwrap(),
                a,
                b.public_key(),
                secret,
                context,
                version
            ),
            transport::accept_authenticated(right, b, a.public_key(), secret, context)
        )
    };
    (a.unwrap(), b.unwrap())
}
async fn attach(a: &Vat, b: &Vat, ids: (&Identity, &Identity), backend: Backend) {
    let (left, right) = sessions(ids.0, ids.1, backend).await;
    a.attach(left).unwrap();
    b.attach(right).unwrap();
}

async fn handoff(backend: Backend) {
    local(async {
        let ids = Rc::new([
            Identity::generate(),
            Identity::generate(),
            Identity::generate(),
        ]);
        let host = Vat::builder(ids[0].public_key())
            .bootstrap(service(Default::default()))
            .start()
            .unwrap();
        let broker = Vat::builder(ids[1].public_key())
            .outgoing_call_limit(8)
            .start()
            .unwrap();
        let stored = Rc::new(RefCell::new(None));
        let dials = Rc::new(Cell::new(0));
        let recipient = Vat::builder(ids[2].public_key())
            .outgoing_call_limit(8)
            .bootstrap(service(stored.clone()))
            .connect_with({
                let ids = ids.clone();
                let host = host.handle();
                let dials = dials.clone();
                move |peer| {
                    let ids = ids.clone();
                    let host = host.clone();
                    let dials = dials.clone();
                    async move {
                        assert_eq!(peer, ids[0].public_key());
                        dials.set(dials.get() + 1);
                        let (outgoing, incoming) = sessions(&ids[2], &ids[0], backend).await;
                        host.attach(incoming)?;
                        Ok(outgoing)
                    }
                }
            })
            .start()
            .unwrap();
        attach(&host, &broker, (&ids[0], &ids[1]), backend).await;
        attach(&broker, &recipient, (&ids[1], &ids[2]), backend).await;

        // Bootstrap happens after task startup. Forward an ordinary generated
        // capability field; the recipient dials the missing direct route.
        let source: harness::Client = broker.bootstrap(ids[0].public_key());
        let receiver: harness::Client = broker.bootstrap(ids[2].public_key());
        let mut request = receiver.bounce_request();
        request.get().set_cap(source);
        let pending = request.send();
        echo(&pending.pipeline.get_cap(), 10).await.unwrap();
        echo(
            &pending
                .promise
                .await
                .unwrap()
                .get()
                .unwrap()
                .get_cap()
                .unwrap(),
            11,
        )
        .await
        .unwrap();
        let direct = stored.borrow().as_ref().unwrap().clone();
        echo(&direct, 12).await.unwrap();
        assert_eq!(dials.get(), 1);
        let reserved: Vec<_> = (0..8).map(|_| direct.echo_request()).collect();
        let snapshots = recipient.diagnostics().snapshot();
        assert!(snapshots.iter().any(|s| s.outgoing_calls == 8));
        assert!(snapshots.iter().all(|s| s.outgoing_call_limit == 8));
        assert_eq!(
            direct
                .echo_request()
                .send()
                .promise
                .await
                .err()
                .unwrap()
                .kind,
            capnp::ErrorKind::Overloaded
        );
        drop(reserved);
        assert!(host.handle().stats().published > 0);
        assert_eq!(
            recipient.handle().route_status(ids[0].public_key()),
            Some(RouteStatus::Authenticated)
        );

        // Join an introduced reference with an independently obtained bootstrap.
        let second: harness::Client = recipient.bootstrap(ids[0].public_key());
        let joined = recipient
            .joiner()
            .join(vec![direct.clone(), second])
            .await
            .unwrap()
            .unwrap();
        let closed = broker.on_disconnect();
        drop(broker);
        assert!(closed.await.is_err());
        echo(&direct, 13).await.unwrap();
        echo(&joined, 14).await.unwrap();

        let bootstrapper = recipient.bootstrapper();
        let joiner = recipient.joiner();
        let handle = recipient.handle();
        let done = recipient.on_disconnect();
        let diagnostics = recipient.diagnostics();
        let report = recipient.shutdown(TIMEOUT).await.unwrap();
        let receipt = report
            .iter()
            .find(|(peer, _)| *peer == ids[0].public_key())
            .unwrap()
            .1
            .as_ref()
            .unwrap();
        assert!(receipt.bytes > 0);
        done.await.unwrap();
        assert!(diagnostics.snapshot().is_empty());
        assert!(handle.route_status(ids[0].public_key()).is_none());
        assert!(echo(&bootstrapper.bootstrap(ids[0].public_key()), 15)
            .await
            .is_err());
        assert!(joiner.join(vec![direct.clone(), joined]).await.is_err());
        assert!(echo(&direct, 16).await.is_err());
        stored.borrow_mut().take();
        host.shutdown(TIMEOUT).await.unwrap();
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn tcp_tls_three_party_pipeline_join_and_drain() {
    handoff(Backend::Tcp).await;
}
#[tokio::test(flavor = "current_thread")]
async fn quic_v1_three_party_pipeline_join_and_drain() {
    handoff(Backend::Quiche).await;
}
#[tokio::test(flavor = "current_thread")]
async fn quic_v2_three_party_pipeline_join_and_drain() {
    handoff(Backend::QuicheV2).await;
}

#[tokio::test(flavor = "current_thread")]
async fn bootstrap_factory_uses_authenticated_peer_and_preserves_connection_after_denial() {
    local(async {
        let a = Identity::generate();
        let b = Identity::generate();
        let allowed = Rc::new(Cell::new(false));
        let callers = Rc::new(RefCell::new(Vec::new()));
        let host = Vat::builder(a.public_key())
            .bootstrap_with({
                let allowed = allowed.clone();
                let callers = callers.clone();
                let service = service(Default::default());
                move |peer| {
                    callers.borrow_mut().push(*peer);
                    if allowed.get() {
                        Ok(service.client.clone())
                    } else {
                        Err(capnp::Error::failed("policy denied bootstrap".into()))
                    }
                }
            })
            .start()
            .unwrap();
        let client = Vat::builder(b.public_key()).start().unwrap();
        attach(&host, &client, (&a, &b), Backend::Tcp).await;
        assert!(echo(&client.bootstrap(a.public_key()), 1).await.is_err());
        assert_eq!(&*callers.borrow(), &[b.public_key()]);
        allowed.set(true);
        let cap: harness::Client = client.bootstrapper().bootstrap(a.public_key());
        echo(&cap, 2).await.unwrap();
        echo(&host.bootstrap(a.public_key()), 3).await.unwrap();
        assert_eq!(
            &*callers.borrow(),
            &[b.public_key(), b.public_key(), a.public_key()]
        );
        client.shutdown(TIMEOUT).await.unwrap();
        host.shutdown(TIMEOUT).await.unwrap();
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn retained_control_handles_do_not_keep_bootstrap_factory_alive() {
    local(async {
        let token = Rc::new(());
        let weak = Rc::downgrade(&token);
        let id = Identity::generate().public_key();
        let vat = Vat::builder(id)
            .bootstrap_with(move |_| {
                let _keep = &token;
                Err(capnp::Error::failed("unavailable".into()))
            })
            .start()
            .unwrap();
        let bootstrapper = vat.bootstrapper().clone();
        let _joiner = vat.joiner();
        let _routes = vat.handle();
        let done = vat.on_disconnect();
        drop(vat);
        assert!(done.await.is_err());
        assert!(weak.upgrade().is_none());
        let error = echo(&bootstrapper.bootstrap(id), 1).await.unwrap_err();
        assert_eq!(error.kind, capnp::ErrorKind::Disconnected);
    })
    .await;
}

struct Dropped(Rc<Cell<bool>>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn canceled_shutdown_cancels_pending_connect_without_replaying_calls() {
    local(async {
        let started = Rc::new(Cell::new(0));
        let dropped = Rc::new(Cell::new(false));
        let vat = Vat::builder(Identity::generate().public_key())
            .connect_with({
                let started = started.clone();
                let dropped = dropped.clone();
                move |_| {
                    started.set(started.get() + 1);
                    let guard = Dropped(dropped.clone());
                    async move {
                        let _guard = guard;
                        futures::future::pending().await
                    }
                }
            })
            .start()
            .unwrap();
        let cap: harness::Client = vat.bootstrap(Identity::generate().public_key());
        let call = cap.echo_request().send();
        while started.get() == 0 {
            tokio::task::yield_now().await;
        }
        let done = vat.on_disconnect();
        drop(vat.shutdown(TIMEOUT));
        assert!(done.await.is_err());
        assert!(call.promise.await.is_err());
        // Session worker aborts are scheduled on the same LocalSet.
        while !dropped.get() {
            tokio::task::yield_now().await;
        }
        assert_eq!(started.get(), 1);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn attach_rejects_a_session_owned_by_another_identity() {
    local(async {
        let a = Identity::generate();
        let b = Identity::generate();
        let (session, _other) = sessions(&a, &b, Backend::Tcp).await;
        let wrong = Vat::builder(Identity::generate().public_key())
            .start()
            .unwrap();
        assert!(wrong.attach(session).is_err());
        assert!(wrong.shutdown(TIMEOUT).await.unwrap().is_empty());
    })
    .await;
}

#[test]
fn invalid_options_fail_before_spawning_a_task() {
    let options = Options {
        connect_timeout: Duration::ZERO,
        ..Options::default()
    };
    assert!(Vat::builder([0; 32]).options(options).start().is_err());
}
