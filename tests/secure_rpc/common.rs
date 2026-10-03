use capnp_rpc::twoparty::TwoPartyClient;
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose,
};
use reproto::rpc::tls::{self, rustls, Identity};
use reproto_test_support::structured::runtime_test_capnp::harness;
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
    async fn echo(
        self: Rc<Self>,
        params: harness::EchoParams,
        results: harness::EchoResults,
    ) -> capnp::Result<()> {
        results.complete(|mut r| {
            r.set_value(params.get()?.get_value());
            Ok(())
        })
    }
    async fn bounce(
        self: Rc<Self>,
        params: harness::BounceParams,
        results: harness::BounceResults,
    ) -> capnp::Result<()> {
        let cap = params.get()?.get_cap()?;
        assert_eq!(echo(&cap, 73).await?, 73);
        results.complete(|mut r| {
            r.set_cap(cap);
            Ok(())
        })
    }
    async fn generic(
        self: Rc<Self>,
        _: harness::GenericParams,
        results: harness::GenericResults,
    ) -> capnp::Result<()> {
        results.complete(|mut r| r.set_value(&vec![0x73; 100_000][..]))
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
    let connection = reproto::rpc::Connection::spawn(driver);
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
