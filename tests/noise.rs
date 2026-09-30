use reproto::transport::{config, Identity};
use std::net::SocketAddr;

#[test]
fn snow_features_enable_the_blake3_profile() {
    use snow::params::{CipherChoice, HashChoice, NoiseParams};
    use snow::resolvers::{CryptoResolver, DefaultResolver};

    for name in [
        "Noise_IK_25519_ChaChaPoly_BLAKE3",
        "Noise_IKpsk2_25519_ChaChaPoly_BLAKE3",
    ] {
        let params: NoiseParams = name.parse().unwrap();
        assert_eq!(params.hash, HashChoice::Blake3);
        let mut hash = DefaultResolver.resolve_hash(&params.hash).unwrap();
        assert_eq!(hash.name(), "BLAKE3");
        assert_eq!((hash.hash_len(), hash.block_len()), (32, 64));
        hash.input(b"abc");
        let mut result = [0; 32];
        hash.result(&mut result);
        // BLAKE3's standard unkeyed 256-bit digest of "abc".
        assert_eq!(
            result,
            [
                0x64, 0x37, 0xb3, 0xac, 0x38, 0x46, 0x51, 0x33, 0xff, 0xb6, 0x3b, 0x75, 0x27, 0x3a,
                0x8d, 0xb5, 0x48, 0xc5, 0x58, 0x46, 0x5d, 0x79, 0xdb, 0x03, 0xfd, 0x35, 0x9c, 0x6c,
                0xd5, 0xbd, 0x9d, 0x85,
            ]
        );
    }
    // Catch default-feature unification accidentally adding legacy suites.
    for hash in [
        HashChoice::SHA256,
        HashChoice::SHA512,
        HashChoice::Blake2s,
        HashChoice::Blake2b,
    ] {
        assert!(DefaultResolver.resolve_hash(&hash).is_none());
    }
    assert!(DefaultResolver
        .resolve_cipher(&CipherChoice::AESGCM)
        .is_none());
}

fn pair(psk: Option<[u8; 32]>, bad: bool) -> (quiche::Connection, quiche::Connection) {
    let a = Identity::generate();
    let b = Identity::generate();
    let mut ac = config(&a, b.public_key(), psk, b"test").unwrap();
    let mut bc = config(
        &b,
        if bad {
            Identity::generate().public_key()
        } else {
            a.public_key()
        },
        psk,
        b"test",
    )
    .unwrap();
    let aa: SocketAddr = "127.0.0.1:1234".parse().unwrap();
    let ba = "127.0.0.1:4321".parse().unwrap();
    (
        quiche::connect(
            None,
            &quiche::ConnectionId::from_ref(&[1; 16]),
            aa,
            ba,
            &mut ac,
        )
        .unwrap(),
        quiche::accept(
            &quiche::ConnectionId::from_ref(&[2; 16]),
            None,
            ba,
            aa,
            &mut bc,
        )
        .unwrap(),
    )
}
fn pump(a: &mut quiche::Connection, b: &mut quiche::Connection) -> quiche::Result<usize> {
    let mut buf = [0; 1350];
    let mut count = 0;
    loop {
        match a.send(&mut buf) {
            Ok((n, info)) => {
                b.recv(
                    &mut buf[..n],
                    quiche::RecvInfo {
                        from: info.from,
                        to: info.to,
                    },
                )?;
                count += 1;
            }
            Err(quiche::Error::Done) => break,
            Err(e) => return Err(e),
        }
    }
    Ok(count)
}
#[test]
fn noise_stream_and_datagram() {
    for psk in [None, Some([7; 32])] {
        let (mut a, mut b) = pair(psk, false);
        for _ in 0..20 {
            pump(&mut a, &mut b).unwrap();
            pump(&mut b, &mut a).unwrap();
        }
        assert!(a.is_established() && b.is_established());
        assert!(!a.is_in_early_data());
        a.stream_send(0, b"capability rpc", true).unwrap();
        a.dgram_send(b"fresh").unwrap();
        pump(&mut a, &mut b).unwrap();
        let mut out = [0; 128];
        let (n, fin) = b.stream_recv(0, &mut out).unwrap();
        assert_eq!(&out[..n], b"capability rpc");
        assert!(fin);
        let n = b.dgram_recv(&mut out).unwrap();
        assert_eq!(&out[..n], b"fresh");
    }
}
#[test]
fn established_server_response_exceeds_initial_amplification_budget() {
    let (mut client, mut server) = pair(None, false);
    for _ in 0..4 {
        pump(&mut client, &mut server).unwrap();
        pump(&mut server, &mut client).unwrap();
    }
    assert!(client.is_established() && server.is_established());
    client.stream_send(0, b"?", true).unwrap();
    pump(&mut client, &mut server).unwrap();
    assert_eq!(server.stream_recv(0, &mut [0; 1]), Ok((1, true)));

    // A tiny request must not leave the authenticated server permanently
    // limited to three times the client's handshake/request byte count.
    let response = vec![0x5a; 32 * 1024];
    let mut queued = 0;
    let mut received = Vec::new();
    let mut finished = false;
    for _ in 0..64 {
        if queued < response.len() {
            match server.stream_send(0, &response[queued..], true) {
                Ok(n) => queued += n,
                Err(quiche::Error::Done) => (),
                other => panic!("response send failed: {other:?}"),
            }
        }
        pump(&mut server, &mut client).unwrap();
        let mut chunk = [0; 4096];
        loop {
            match client.stream_recv(0, &mut chunk) {
                Ok((n, fin)) => {
                    received.extend_from_slice(&chunk[..n]);
                    if fin {
                        finished = true;
                        break;
                    }
                }
                Err(quiche::Error::Done) => break,
                other => panic!("response receive failed: {other:?}"),
            }
        }
        pump(&mut client, &mut server).unwrap();
        if finished {
            break;
        }
    }
    assert!(finished, "response stalled after {} bytes", received.len());
    assert_eq!(queued, response.len());
    assert_eq!(received, response);
}

#[test]
fn wrong_peer_rejected() {
    let (mut a, mut b) = pair(None, true);
    assert!(pump(&mut a, &mut b).is_err());
    assert!(!b.is_established());
}

#[test]
fn incompatible_context_and_psk_fail_closed() {
    for wrong_psk in [false, true] {
        let a = Identity::generate();
        let b = Identity::generate();
        let mut ac = config(&a, b.public_key(), Some([1; 32]), b"grant one").unwrap();
        let mut bc = config(
            &b,
            a.public_key(),
            Some(if wrong_psk { [2; 32] } else { [1; 32] }),
            if wrong_psk {
                b"grant one"
            } else {
                b"grant two"
            },
        )
        .unwrap();
        let aa = "127.0.0.1:1234".parse().unwrap();
        let ba = "127.0.0.1:4321".parse().unwrap();
        let mut a = quiche::connect(
            None,
            &quiche::ConnectionId::from_ref(&[1; 16]),
            aa,
            ba,
            &mut ac,
        )
        .unwrap();
        let mut b = quiche::accept(
            &quiche::ConnectionId::from_ref(&[2; 16]),
            None,
            ba,
            aa,
            &mut bc,
        )
        .unwrap();
        let result = pump(&mut a, &mut b).and_then(|_| pump(&mut b, &mut a));
        assert!(result.is_err());
        assert!(!a.is_established());
    }
}
#[test]
fn duplicate_and_tampered_application_packets_do_not_deliver_twice() {
    let (mut a, mut b) = pair(None, false);
    for _ in 0..10 {
        pump(&mut a, &mut b).unwrap();
        pump(&mut b, &mut a).unwrap();
    }
    a.dgram_send(b"once").unwrap();
    let mut packet = [0; 1350];
    let (n, info) = a.send(&mut packet).unwrap();
    let original = packet[..n].to_vec();
    let mut tampered = original.clone();
    tampered[n - 1] ^= 1;
    let _ = b.recv(
        &mut tampered,
        quiche::RecvInfo {
            from: info.from,
            to: info.to,
        },
    );
    let mut out = [0; 100];
    assert_eq!(b.dgram_recv(&mut out), Err(quiche::Error::Done));
    b.recv(
        &mut packet[..n],
        quiche::RecvInfo {
            from: info.from,
            to: info.to,
        },
    )
    .unwrap();
    assert_eq!(b.dgram_recv(&mut out).unwrap(), 4);
    let mut replay = original;
    let _ = b.recv(
        &mut replay,
        quiche::RecvInfo {
            from: info.from,
            to: info.to,
        },
    );
    assert_eq!(b.dgram_recv(&mut out), Err(quiche::Error::Done));
}
#[test]
fn lost_first_flight_is_retransmitted() {
    let (mut a, mut b) = pair(None, false);
    let mut packet = [0; 1350];
    a.send(&mut packet).unwrap();
    std::thread::sleep(a.timeout().unwrap() + std::time::Duration::from_millis(5));
    a.on_timeout();
    for _ in 0..20 {
        pump(&mut a, &mut b).unwrap();
        pump(&mut b, &mut a).unwrap();
    }
    assert!(a.is_established() && b.is_established());
}
#[test]
fn lost_responder_flight_is_retransmitted() {
    let (mut a, mut b) = pair(Some([9; 32]), false);
    pump(&mut a, &mut b).unwrap();
    let mut packet = [0; 1350];
    while b.send(&mut packet).is_ok() {}
    std::thread::sleep(b.timeout().unwrap() + std::time::Duration::from_millis(5));
    b.on_timeout();
    for _ in 0..20 {
        pump(&mut b, &mut a).unwrap();
        pump(&mut a, &mut b).unwrap();
    }
    assert!(a.is_established() && b.is_established());
}
