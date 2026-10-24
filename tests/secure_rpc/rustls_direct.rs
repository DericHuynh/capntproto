//! Differential record tests use the unchanged owned encrypter as the oracle.
use super::common::{Pki, TIMEOUT};
use capntproto::rpc::tls::{rustls, TlsStream};
use rustls::{
    crypto::cipher::{
        MessageEncrypter, OutboundChunks, OutboundOpaqueMessage, OutboundPlainMessage,
    },
    ClientConfig, ConnectionTrafficSecrets, ContentType, ProtocolVersion, ServerConfig,
    SupportedCipherSuite,
};
use std::{io::Cursor, sync::Arc};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

fn configurations(suite: SupportedCipherSuite) -> (ClientConfig, ServerConfig) {
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
    (client, server)
}

fn matching_encrypters(
    suite: SupportedCipherSuite,
) -> (Box<dyn MessageEncrypter>, Box<dyn MessageEncrypter>) {
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
    let (client, server) = configurations(suite);
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
        make(server.dangerous_extract_secrets().unwrap().rx.1),
    )
}

struct OwnedOnly<'a>(&'a mut dyn MessageEncrypter);
impl MessageEncrypter for OwnedOnly<'_> {
    fn encrypt(
        &mut self,
        msg: OutboundPlainMessage<'_>,
        seq: u64,
    ) -> Result<OutboundOpaqueMessage, rustls::Error> {
        self.0.encrypt(msg, seq)
    }
    fn encrypted_payload_len(&self, size: usize) -> usize {
        self.0.encrypted_payload_len(size)
    }
}

#[test]
fn direct_records_match_owned_records_without_allocating() {
    for suite in rustls::crypto::ring::ALL_CIPHER_SUITES
        .iter()
        .copied()
        .filter(|s| s.tls13().is_some())
    {
        let (mut direct, mut owned) = matching_encrypters(suite);
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
                    let expected = owned.encrypt(message(), sequence).unwrap().encode();
                    let mut output = vec![0xa5; expected.len() + 19];
                    // A rejected size must neither write a prefix nor consume crypto state.
                    assert!(direct
                        .encrypt_into(message(), sequence, &mut output[..expected.len() - 1])
                        .is_err());
                    assert!(output.iter().all(|b| *b == 0xa5));
                    let counts = allocation_counter::measure(|| {
                        assert_eq!(
                            direct
                                .encrypt_into(message(), sequence, &mut output)
                                .unwrap(),
                            expected.len()
                        );
                    });
                    assert_eq!(counts.count_total, 0, "{suite:?}, {size}: {counts:?}");
                    assert_eq!(&output[..expected.len()], expected);
                    assert!(output[expected.len()..].iter().all(|b| *b == 0xa5));
                    // Providers without an override retain the exact original behavior.
                    output.fill(0xa5);
                    let mut fallback = OwnedOnly(&mut *owned);
                    assert!(fallback
                        .encrypt_into(message(), sequence, &mut output[..expected.len() - 1])
                        .is_err());
                    assert!(output.iter().all(|b| *b == 0xa5));
                    let counts = allocation_counter::measure(|| {
                        assert_eq!(
                            fallback
                                .encrypt_into(message(), sequence, &mut output)
                                .unwrap(),
                            expected.len()
                        );
                    });
                    assert!(counts.count_total > 0);
                    assert_eq!(&output[..expected.len()], expected);
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
                let (mut client, mut server) = configurations(SupportedCipherSuite::Tls13(limited));
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
                            let mut s = tokio_rustls::TlsAcceptor::from(server)
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
                            let mut c = tokio_rustls::TlsConnector::from(client)
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
