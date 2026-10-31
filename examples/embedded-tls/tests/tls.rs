use capntproto::rpc::{
    security::{self, Identity},
    tcp::tls,
    SystemClock,
};
use capntproto_embedded_example::{
    host::{self, Socket},
    publish, Reading, Storage, TlsClock,
};
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose,
};
use std::{sync::Arc, time::Duration};
struct Clock(Option<u64>);
impl TlsClock for Clock {
    fn now(&self) -> Option<u64> {
        self.0
    }
}
fn pki() -> (Vec<u8>, Identity) {
    let mut ca = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca.key_usages = vec![KeyUsagePurpose::KeyCertSign];
    let ca = CertifiedIssuer::self_signed(ca, KeyPair::generate().unwrap()).unwrap();
    let mut leaf = CertificateParams::new(vec!["localhost".into()]).unwrap();
    leaf.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let key = KeyPair::generate().unwrap();
    let cert = leaf.signed_by(&key, &ca).unwrap();
    (
        ca.der().to_vec(),
        Identity {
            certificates: vec![cert.der().clone()],
            private_key: security::rustls::pki_types::PrivatePkcs8KeyDer::from(key.serialize_der())
                .into(),
        },
    )
}
#[tokio::test(flavor = "current_thread")]
async fn no_std_client_publishes_to_real_tls_service_with_fragmented_io() {
    for (fragment, temperature, accepted) in [(3, 21_500, true), (17, 200_000, false)] {
        let (root, identity) = pki();
        let config = tls::server_config(identity, None, Arc::new(SystemClock)).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let reading = Reading {
            device_id: 42,
            sequence: 99,
            milli_celsius: temperature,
        };
        let serving = async {
            let (io, _) = listener.accept().await.unwrap();
            host::serve(io, config, fragment).await.unwrap()
        };
        let calling = async {
            let config =
                capntproto_transport::embedded::ClientConfig::new("localhost", &root, &SystemClock)
                    .unwrap();
            let io = Socket {
                io: tokio::net::TcpStream::connect(address).await.unwrap(),
                fragment,
            };
            publish(io, &config, rand::rngs::OsRng, &mut Storage::new(), reading)
                .await
                .unwrap()
        };
        let (observed, ack) = tokio::time::timeout(Duration::from_secs(15), async {
            tokio::join!(serving, calling)
        })
        .await
        .unwrap();
        assert_eq!(observed, reading);
        assert_eq!(ack.sequence, 99);
        assert_eq!(ack.accepted, accepted);
    }
}
#[tokio::test(flavor = "current_thread")]
async fn device_rejects_bad_time_name_and_trust() {
    for fault in 0..4 {
        let (root, identity) = pki();
        let other = pki().0;
        let clock = Clock(if fault == 0 {
            None
        } else if fault == 1 {
            Some(0)
        } else {
            SystemClock.now()
        });
        let config = tls::server_config(identity, None, Arc::new(SystemClock)).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let serving = async {
            let (io, _) = listener.accept().await.unwrap();
            let _ = host::serve(io, config, 32).await;
        };
        let calling = async {
            let config = capntproto_transport::embedded::ClientConfig::new(
                if fault == 2 {
                    "wrong.local"
                } else {
                    "localhost"
                },
                if fault == 3 { &other } else { &root },
                &clock,
            )
            .unwrap();
            let io = Socket {
                io: tokio::net::TcpStream::connect(address).await.unwrap(),
                fragment: 32,
            };
            assert!(publish(
                io,
                &config,
                rand::rngs::OsRng,
                &mut Storage::new(),
                Reading {
                    device_id: 1,
                    sequence: 1,
                    milli_celsius: 20_000
                }
            )
            .await
            .is_err());
        };
        tokio::time::timeout(Duration::from_secs(15), async {
            tokio::join!(serving, calling)
        })
        .await
        .unwrap();
    }
}
#[test]
fn telemetry_encoding_uses_only_caller_storage() {
    let mut words = [capntproto::capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 32];
    let mut bytes = [0; 512];
    let mut length = 0;
    let counts = allocation_counter::measure(|| {
        length = capntproto_embedded_example::encode(
            Reading {
                device_id: 7,
                sequence: 9,
                milli_celsius: 21_500,
            },
            &mut words,
            &mut bytes,
        )
        .unwrap();
    });
    assert_eq!(counts.count_total, 0);
    let segments = capntproto::capnp::serialize::NoAllocBufferSegments::from_buffer_exact(
        &bytes[..length],
        Default::default(),
    )
    .unwrap();
    let message = capntproto::capnp::message::Reader::new(segments, Default::default());
    let reading = message
        .get_root::<capntproto_embedded_example::telemetry_capnp::reading::Reader>()
        .unwrap();
    assert_eq!(
        (
            reading.get_device_id(),
            reading.get_sequence(),
            reading.get_milli_celsius()
        ),
        (7, 9, 21_500)
    );
    assert!(capntproto_embedded_example::encode(
        Reading {
            device_id: 1,
            sequence: 2,
            milli_celsius: 3
        },
        &mut words,
        &mut bytes[..1]
    )
    .is_err());
}
