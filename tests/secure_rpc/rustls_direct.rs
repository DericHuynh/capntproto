//! Differential records use the original owned ring sealing operation as the oracle.
use super::common::{Pki, TIMEOUT};
use capntproto::rpc::{security::rustls, tcp::tls::TlsStream};
use rustls::{
    crypto::cipher::{
        make_tls13_aad, Iv, MessageEncrypter, Nonce, OutboundChunks, OutboundPlainMessage,
    },
    ClientConfig, ConnectionTrafficSecrets, ContentType, ProtocolVersion, ServerConfig,
    SupportedCipherSuite,
};
use std::{io::Cursor, sync::Arc};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

fn configurations(suite: SupportedCipherSuite) -> (ClientConfig, ServerConfig, Pki) {
    let pki = Pki::new();
    let mut provider = rustls::crypto::ring::default_provider();
    provider.cipher_suites = vec![suite];
    let provider = Arc::new(provider);
    let mut client = ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_root_certificates(pki.roots())
        .with_no_client_auth();
    let identity = pki.identity("localhost", false, false);
    let mut server = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(identity.certificates, identity.private_key)
        .unwrap();
    // Test-only key extraction supplies identically derived keys to both paths,
    // including AES-128's key length, without adding a production key API.
    client.enable_secret_extraction = true;
    server.enable_secret_extraction = true;
    (client, server, pki)
}

fn matching_encrypters(suite: SupportedCipherSuite) -> (Box<dyn MessageEncrypter>, OwnedReference) {
    fn transfer(from: &mut rustls::Connection, to: &mut rustls::Connection) {
        let mut wire = Vec::new();
        while from.wants_write() {
            from.write_tls(&mut wire).unwrap();
        }
        let mut input = Cursor::new(&wire);
        while input.position() < wire.len() as u64 {
            assert!(to.read_tls(&mut input).unwrap() > 0);
            to.process_new_packets().unwrap();
        }
    }
    let (client, server, _) = configurations(suite);
    let mut client = rustls::Connection::Client(
        rustls::ClientConnection::new(Arc::new(client), "localhost".try_into().unwrap()).unwrap(),
    );
    let mut server =
        rustls::Connection::Server(rustls::ServerConnection::new(Arc::new(server)).unwrap());
    for _ in 0..16 {
        transfer(&mut client, &mut server);
        transfer(&mut server, &mut client);
        if !client.is_handshaking()
            && !server.is_handshaking()
            && !client.wants_write()
            && !server.wants_write()
        {
            break;
        }
    }
    assert!(!client.is_handshaking() && !server.is_handshaking());
    let algorithm = suite.tls13().unwrap().aead_alg;
    let make = |secret| match secret {
        ConnectionTrafficSecrets::Aes128Gcm { key, iv }
        | ConnectionTrafficSecrets::Aes256Gcm { key, iv }
        | ConnectionTrafficSecrets::Chacha20Poly1305 { key, iv } => algorithm.encrypter(key, iv),
        _ => panic!("unexpected test cipher"),
    };
    (
        make(client.dangerous_extract_secrets().unwrap().tx.1),
        OwnedReference::new(server.dangerous_extract_secrets().unwrap().rx.1),
    )
}

// The pre-backport path gathered into an owned vector before sealing it. Keep
// that independent path in the test instead of comparing the new API to itself.
struct OwnedReference {
    key: ring::aead::LessSafeKey,
    iv: Iv,
}
impl OwnedReference {
    fn new(secret: ConnectionTrafficSecrets) -> Self {
        let (algorithm, key, iv) = match secret {
            ConnectionTrafficSecrets::Aes128Gcm { key, iv } => (&ring::aead::AES_128_GCM, key, iv),
            ConnectionTrafficSecrets::Aes256Gcm { key, iv } => (&ring::aead::AES_256_GCM, key, iv),
            ConnectionTrafficSecrets::Chacha20Poly1305 { key, iv } => {
                (&ring::aead::CHACHA20_POLY1305, key, iv)
            }
            _ => panic!("unexpected test cipher"),
        };
        Self {
            key: ring::aead::LessSafeKey::new(
                ring::aead::UnboundKey::new(algorithm, key.as_ref()).unwrap(),
            ),
            iv,
        }
    }
    fn encrypt(&self, msg: OutboundPlainMessage<'_>, seq: u64) -> Vec<u8> {
        let mut payload = msg.payload.to_vec();
        payload.push(msg.typ.into());
        let aad = ring::aead::Aad::from(make_tls13_aad(payload.len() + 16));
        let nonce = ring::aead::Nonce::assume_unique_for_key(Nonce::new(&self.iv, seq).0);
        self.key
            .seal_in_place_append_tag(nonce, aad, &mut payload)
            .unwrap();
        payload
    }
}

#[test]
fn direct_records_match_owned_records_without_allocating() {
    for suite in rustls::crypto::ring::ALL_CIPHER_SUITES
        .iter()
        .copied()
        .filter(|s| s.tls13().is_some())
    {
        let (mut direct, owned) = matching_encrypters(suite);
        let mut seq = 0u64;
        for size in [0, 1, 15, 16, 17, 255, 1024, 16_383, 16_384] {
            let data: Vec<_> = (0..size).map(|i| (i % 251) as u8).collect();
            let chunks: &[&[u8]] = &[&[], &data[..size / 3], &[], &data[size / 3..], &[]];
            let shapes = [
                OutboundChunks::Single(&data),
                OutboundChunks::new(chunks),
                OutboundChunks::Multiple {
                    chunks,
                    start: size.min(1),
                    end: size.saturating_sub(1).max(size.min(1)),
                },
            ];
            for payload in shapes {
                for typ in [
                    ContentType::ApplicationData,
                    ContentType::Handshake,
                    ContentType::Alert,
                ] {
                    seq += 1;
                    // Exercise high sequence bits in nonce XOR, not just short counters.
                    let sequence = (seq << 40) | seq;
                    let message = || OutboundPlainMessage {
                        typ,
                        version: ProtocolVersion::TLSv1_2,
                        payload: payload.clone(),
                    };
                    let expected = owned.encrypt(message(), sequence);
                    let mut output = vec![0xa5; expected.len() + 19];
                    // A rejected size must neither write a prefix nor consume crypto state.
                    assert!(direct
                        .encrypt(message(), sequence, &mut output[..expected.len() - 1])
                        .is_err());
                    assert!(output.iter().all(|b| *b == 0xa5));
                    let counts = allocation_counter::measure(|| {
                        let record = direct.encrypt(message(), sequence, &mut output).unwrap();
                        assert_eq!(record.typ, ContentType::ApplicationData);
                        assert_eq!(record.version, ProtocolVersion::TLSv1_2);
                        assert_eq!(record.payload, expected);
                    });
                    assert_eq!(counts.count_total, 0, "{suite:?}, {size}: {counts:?}");
                    assert!(output[expected.len()..].iter().all(|b| *b == 0xa5));
                }
            }
        }
    }
}

#[tokio::test]
async fn direct_records_interoperate_across_automatic_key_updates_for_every_cipher() {
    async fn ping(stream: &mut (impl AsyncRead + AsyncWrite + Unpin)) {
        for round in 1..=16u8 {
            stream.write_all(&[round; 2003]).await.unwrap();
            stream.flush().await.unwrap();
            let mut response = [0; 2003];
            stream.read_exact(&mut response).await.unwrap();
            assert_eq!(response, [round ^ 0x5a; 2003]);
        }
        stream.shutdown().await.unwrap();
        assert_eq!(stream.read(&mut [0; 1]).await.unwrap(), 0);
    }
    async fn pong(stream: &mut (impl AsyncRead + AsyncWrite + Unpin)) {
        for round in 1..=16u8 {
            let mut request = [0; 2003];
            stream.read_exact(&mut request).await.unwrap();
            assert_eq!(request, [round; 2003]);
            stream.write_all(&[round ^ 0x5a; 2003]).await.unwrap();
            stream.flush().await.unwrap();
        }
        assert_eq!(stream.read(&mut [0; 1]).await.unwrap(), 0);
        stream.shutdown().await.unwrap();
    }
    tokio::time::timeout(TIMEOUT, async {
        for suite in rustls::crypto::ring::ALL_CIPHER_SUITES
            .iter()
            .copied()
            .filter_map(|s| s.tls13())
        {
            // Force frequent key refreshes without changing the actual cipher.
            let limited = Box::leak(Box::new(rustls::Tls13CipherSuite {
                common: rustls::CipherSuiteCommon {
                    suite: suite.common.suite,
                    hash_provider: suite.common.hash_provider,
                    confidentiality_limit: 8,
                },
                hkdf_provider: suite.hkdf_provider,
                aead_alg: suite.aead_alg,
                quic: None,
            }));
            for unbuffered_client in [true, false] {
                let (mut client, mut server, pki) =
                    configurations(SupportedCipherSuite::Tls13(limited));
                // Force the same key limit in the independently compiled official peer.
                use tokio_rustls::rustls as reference;
                let mut provider = reference::crypto::ring::default_provider();
                let original = provider
                    .cipher_suites
                    .iter()
                    .filter_map(|s| s.tls13())
                    .find(|s| u16::from(s.common.suite) == u16::from(suite.common.suite))
                    .unwrap();
                let reference_limited = Box::leak(Box::new(reference::Tls13CipherSuite {
                    common: reference::CipherSuiteCommon {
                        suite: original.common.suite,
                        hash_provider: original.common.hash_provider,
                        confidentiality_limit: 8,
                    },
                    hkdf_provider: original.hkdf_provider,
                    aead_alg: original.aead_alg,
                    quic: None,
                }));
                provider.cipher_suites =
                    vec![reference::SupportedCipherSuite::Tls13(reference_limited)];
                let (mut reference_client, mut reference_server) =
                    pki.reference_configs(Arc::new(provider));
                reference_client.max_fragment_size = Some(256);
                reference_server.max_fragment_size = Some(256);
                client.max_fragment_size = Some(256);
                server.max_fragment_size = Some(256);
                let (client, server) = (Arc::new(client), Arc::new(server));
                let (a, b) = tokio::io::duplex(257);
                let extracted = if unbuffered_client {
                    let (_, extracted) = tokio::join!(
                        async {
                            let mut c =
                                TlsStream::connect(a, client, "localhost".try_into().unwrap())
                                    .await
                                    .unwrap();
                            ping(&mut c).await;
                        },
                        async {
                            let mut s = tokio_rustls::TlsAcceptor::from(Arc::new(reference_server))
                                .accept(b)
                                .await
                                .unwrap();
                            pong(&mut s).await;
                            s.into_inner().1.dangerous_extract_secrets().unwrap()
                        },
                    );
                    extracted
                } else {
                    let (extracted, _) = tokio::join!(
                        async {
                            let mut c =
                                tokio_rustls::TlsConnector::from(Arc::new(reference_client))
                                    .connect("localhost".try_into().unwrap(), a)
                                    .await
                                    .unwrap();
                            ping(&mut c).await;
                            c.into_inner().1.dangerous_extract_secrets().unwrap()
                        },
                        async {
                            let mut s = TlsStream::accept(b, server).await.unwrap();
                            pong(&mut s).await;
                        },
                    );
                    extracted
                };
                // 128 application records each way; low final counters prove
                // both directions actually rotated their traffic keys.
                assert!(extracted.tx.0 < 128 && extracted.rx.0 < 128);
            }
        }
    })
    .await
    .expect("direct-output TLS key updates stalled");
}
