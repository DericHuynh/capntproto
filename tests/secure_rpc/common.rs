use capnp_rpc::twoparty::TwoPartyClient;
use capntproto::rpc::tls::{self, rustls, Identity};
use capntproto_test_support::structured::runtime_test_capnp::harness;
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose,
};
use rustls::pki_types::PrivatePkcs8KeyDer;
use std::{future::Future, rc::Rc, sync::Arc, time::Duration};

pub const TIMEOUT: Duration = Duration::from_secs(5);

pub struct Pki {
    ca: CertifiedIssuer<'static, KeyPair>,
}

impl Pki {
    pub fn new() -> Self {
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        Self {
            ca: CertifiedIssuer::self_signed(params, KeyPair::generate().unwrap()).unwrap(),
        }
    }

    #[cfg(feature = "quic")]
    pub fn roots_der(&self) -> Vec<rustls::pki_types::CertificateDer<'static>> {
        vec![self.ca.der().clone()]
    }

    pub fn roots(&self) -> rustls::RootCertStore {
        let mut roots = rustls::RootCertStore::empty();
        roots.add(self.ca.der().clone()).unwrap();
        roots
    }

    pub fn identity(&self, name: &str, client: bool, expired: bool) -> Identity {
        let mut params = CertificateParams::new(vec![name.into()]).unwrap();
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![if client {
            ExtendedKeyUsagePurpose::ClientAuth
        } else {
            ExtendedKeyUsagePurpose::ServerAuth
        }];
        if expired {
            params.not_before = rcgen::date_time_ymd(2000, 1, 1);
            params.not_after = rcgen::date_time_ymd(2001, 1, 1);
        }
        let key = KeyPair::generate().unwrap();
        let cert = params.signed_by(&key, &self.ca).unwrap();
        Identity {
            certificates: vec![cert.der().clone()],
            private_key: PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
        }
    }

    pub fn server(&self, mutual: bool) -> Arc<rustls::ServerConfig> {
        tls::server_config(
            self.identity("localhost", false, false),
            mutual.then(|| self.roots()),
        )
        .unwrap()
    }

    pub fn client(&self, mutual: bool) -> Arc<rustls::ClientConfig> {
        tls::client_config(
            self.roots(),
            mutual.then(|| self.identity("client.local", true, false)),
        )
        .unwrap()
    }
}

pub async fn local(future: impl Future<Output = ()>) {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(20), future)
                .await
                .expect("secure RPC test timed out");
        })
        .await;
}

pub struct Echo;
impl harness::Server for Echo {
    fn echo(
        self: Rc<Self>,
        context: harness::EchoContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (params, results) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            results.complete(|mut r| {
                r.set_value(params.get()?.get_value());
                Ok(())
            })
        })
    }
    fn bounce(
        self: Rc<Self>,
        context: harness::BounceContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (params, results) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            let cap = params.get()?.get_cap()?;
            assert_eq!(echo(&cap, 73).await?, 73);
            results.complete(|mut r| {
                r.set_cap(cap);
                Ok(())
            })
        })
    }
    fn generic(
        self: Rc<Self>,
        context: harness::GenericContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (_, results) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            results.complete(|mut r| r.set_value(&vec![0x73; 100_000][..]))
        })
    }
}

pub fn bootstrap() -> capnp::capability::Client {
    capnp_rpc::new_client::<harness::Client, _>(Echo).client
}

pub async fn echo(client: &harness::Client, value: u32) -> capnp::Result<u32> {
    let mut request = client.echo_request();
    request.get().set_value(value);
    Ok(request.send().await?.get()?.get_value())
}

pub async fn exercise(mut driver: TwoPartyClient<'static>) {
    driver.set_outgoing_call_limit(4);
    let connection = capntproto::rpc::Connection::spawn(driver);
    let cap: harness::Client = connection.bootstrap();
    let disconnected = connection.on_disconnect();
    let diagnostics = connection.diagnostics();
    // Check admission before sending application work on TLS/mTLS and both
    // QUIC versions. Unsent builders reserve capacity but allocate no questions.
    let reserved: Vec<_> = (0..4).map(|_| cap.echo_request()).collect();
    assert_eq!(diagnostics.snapshot()[0].outgoing_calls, 4);
    assert_eq!(
        cap.echo_request().send().promise.await.err().unwrap().kind,
        capnp::ErrorKind::Overloaded
    );
    drop(reserved);
    assert_eq!(echo(&cap, 17).await.unwrap(), 17);
    let callback: harness::Client = capnp_rpc::new_client(Echo);
    let mut bounce = cap.bounce_request();
    bounce.get().set_cap(callback);
    let pending = bounce.send();
    let pipelined = pending.pipeline.get_cap();
    assert_eq!(echo(&pipelined, 29).await.unwrap(), 29);
    let returned = pending
        .promise
        .await
        .unwrap()
        .get()
        .unwrap()
        .get_cap()
        .unwrap();
    assert_eq!(echo(&returned, 31).await.unwrap(), 31);
    let large = cap.generic_request().send().promise.await.unwrap();
    assert_eq!(
        large.get().unwrap().get_value().unwrap(),
        vec![0x73; 100_000]
    );
    connection.shutdown(TIMEOUT).await.unwrap();
    disconnected.await.unwrap();
    assert!(diagnostics.snapshot().is_empty());
    assert!(echo(&cap, 19).await.is_err());
}

// Production network diagnostic: grow to 32 parked RPCs, then release one.
// The counters belong to application futures and therefore detect any broad
// worker repoll caused by IO, Return, cleanup, or an unrelated application wake.
#[derive(Clone)]
pub struct SparseCalls {
    next_bounce: Rc<std::cell::Cell<usize>>,
    released: Rc<Vec<std::cell::Cell<bool>>>,
    polls: Rc<Vec<std::cell::Cell<usize>>>,
    wakes: Rc<Vec<std::cell::RefCell<Option<std::task::Waker>>>>,
}
impl SparseCalls {
    pub fn new() -> Self {
        Self {
            next_bounce: Rc::new(std::cell::Cell::new(0)),
            released: Rc::new((0..32).map(|_| std::cell::Cell::new(false)).collect()),
            polls: Rc::new((0..32).map(|_| std::cell::Cell::new(0)).collect()),
            wakes: Rc::new((0..32).map(|_| std::cell::RefCell::new(None)).collect()),
        }
    }
    pub fn bootstrap(&self) -> capnp::capability::Client {
        capnp_rpc::new_client::<harness::Client, _>(self.clone()).client
    }
    fn release(&self, index: usize) {
        self.released[index].set(true);
        if let Some(wake) = self.wakes[index].borrow_mut().take() {
            wake.wake();
        }
    }
    pub async fn exercise(&self, client: harness::Client) {
        let mut replies: Vec<_> = (0..32)
            .map(|value| {
                let mut request = client.bounce_request();
                request
                    .get()
                    .set_cap(capnp_rpc::new_client::<harness::Client, _>(Echo));
                let response = request.send();
                let pipeline = response.pipeline.get_cap();
                Some(tokio::task::spawn_local(async move {
                    // Each child is sent through its unresolved parent question.
                    // Releasing one parent must settle just its own projection.
                    let result = echo(&pipeline, value).await.unwrap();
                    response
                        .promise
                        .await
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_cap()
                        .unwrap();
                    result
                }))
            })
            .collect();
        while self.polls.iter().any(|polls| polls.get() == 0) {
            tokio::task::yield_now().await;
        }
        let before: Vec<_> = self.polls.iter().map(|polls| polls.get()).collect();
        self.release(17);
        assert_eq!(replies[17].take().unwrap().await.unwrap(), 17);
        for _ in 0..64 {
            tokio::task::yield_now().await;
        }
        for (index, expected) in before.into_iter().enumerate() {
            if index != 17 {
                assert_eq!(
                    self.polls[index].get(),
                    expected,
                    "unrelated parked RPC {index}"
                );
                assert!(!replies[index].as_ref().unwrap().is_finished());
            }
        }
        for index in 0..32 {
            self.release(index);
        }
        for (index, reply) in replies.into_iter().enumerate() {
            if let Some(reply) = reply {
                assert_eq!(reply.await.unwrap(), index as u32);
            }
        }
    }
}
impl harness::Server for SparseCalls {
    fn bounce(
        self: Rc<Self>,
        context: harness::BounceContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        // Calls on the same capability preserve admission order. The index also
        // checks that a used pipeline does not reorder its parent's dispatch.
        let index = self.next_bounce.get();
        self.next_bounce.set(index + 1);
        let (params, results) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            assert!(index < 32);
            let cap = params.get()?.get_cap()?;
            std::future::poll_fn(|cx| {
                self.polls[index].set(self.polls[index].get() + 1);
                if self.released[index].get() {
                    std::task::Poll::Ready(())
                } else {
                    *self.wakes[index].borrow_mut() = Some(cx.waker().clone());
                    std::task::Poll::Pending
                }
            })
            .await;
            results.complete(|mut r| {
                r.set_cap(cap);
                Ok(())
            })
        })
    }
}

pub fn portable_options() -> capnp_rpc::portable::Options {
    capnp_rpc::portable::Options::bounded(
        1.try_into().unwrap(),
        256.try_into().unwrap(),
        (512 * 1024).try_into().unwrap(),
        (8 * 1024 * 1024).try_into().unwrap(),
    )
    .unwrap()
}
