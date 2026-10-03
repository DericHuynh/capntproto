#![cfg(feature = "native")]
use capntproto::transport::{self, Identity, IdentityError};

// RFC 8032 section 7.1 Ed25519 test vectors.
// https://www.rfc-editor.org/rfc/rfc8032.txt
fn key(hex: &str) -> [u8; 32] {
    assert_eq!(hex.len(), 64);
    std::array::from_fn(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap())
}
fn private(index: u64) -> [u8; 32] {
    key(match index {
        1 => "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
        2 => "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
        _ => panic!("unknown fixture"),
    })
}
fn public(index: u64) -> [u8; 32] {
    match index {
        1 => key("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"),
        2 => key("3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c"),
        3 => [0; 32],
        _ => panic!("unknown fixture"),
    }
}

#[test]
fn imported_keys_match_independent_vectors_and_reject_mismatched_pairs() {
    for index in 1..=2 {
        let identity = Identity::from_private_key(private(index)).unwrap();
        assert_eq!(identity.public_key(), public(index));
        assert_eq!(
            Identity::from_keypair(private(index), public(index))
                .unwrap()
                .public_key(),
            public(index)
        );
        for wrong in (1..=3).filter(|other| *other != index) {
            let error = Identity::from_keypair(private(index), public(wrong)).unwrap_err();
            assert_eq!(error, IdentityError::KeyMismatch);
            assert_eq!(
                error.to_string(),
                "identity public key does not match its private key"
            );
        }
        // Ed25519 seeds are exact bytes, with no X25519 scalar equivalence.
        let mut changed = private(index);
        changed[0] ^= 7;
        assert!(Identity::from_keypair(changed, public(index)).is_err());
        let mut copied_public = identity.public_key();
        copied_public[0] ^= 255;
        assert_ne!(copied_public, identity.public_key());
        assert_eq!(
            format!("{identity:?}"),
            format!("Identity {{ public_key: {:?}, .. }}", public(index))
        );
    }
}

fn pump(sender: &mut quiche::Connection, receiver: &mut quiche::Connection) {
    let mut bytes = [0; 1350];
    for _ in 0..64 {
        match sender.send(&mut bytes) {
            Ok((size, info)) => {
                receiver
                    .recv(
                        &mut bytes[..size],
                        quiche::RecvInfo {
                            from: info.from,
                            to: info.to,
                        },
                    )
                    .unwrap();
            }
            Err(quiche::Error::Done) => return,
            Err(error) => panic!("send failed: {error}"),
        }
    }
    panic!("packet budget exceeded");
}

// Prove possession of the key named by `claimed` through the real pinned
// backend. The caller's Config may outlive its source Identity.
fn authenticate(config: &mut quiche::Config, claimed: [u8; 32], psk: Option<[u8; 32]>) {
    let peer = Identity::from_private_key([4; 32]).unwrap();
    let mut peer_config = transport::config(&peer, claimed, psk, b"identity-check").unwrap();
    let left = "127.0.0.1:12001".parse().unwrap();
    let right = "127.0.0.1:12002".parse().unwrap();
    let mut client = quiche::connect(
        None,
        &quiche::ConnectionId::from_ref(&[1; 16]),
        left,
        right,
        config,
    )
    .unwrap();
    let mut server = quiche::accept(
        &quiche::ConnectionId::from_ref(&[2; 16]),
        None,
        right,
        left,
        &mut peer_config,
    )
    .unwrap();
    for _ in 0..8 {
        pump(&mut client, &mut server);
        pump(&mut server, &mut client);
    }
    assert!(client.is_established() && server.is_established());
    assert!(!client.is_in_early_data());
    client.stream_send(0, b"verified identity", true).unwrap();
    pump(&mut client, &mut server);
    let mut bytes = [0; 32];
    let (size, fin) = server.stream_recv(0, &mut bytes).unwrap();
    assert_eq!(&bytes[..size], b"verified identity");
    assert!(fin);
}

#[test]
fn configurations_retain_their_keys_after_identity_drop() {
    let peer = Identity::from_private_key([4; 32]).unwrap();
    for psk in [None, Some([7; 32])] {
        for identity in [
            Identity::generate(),
            Identity::from_private_key(private(1)).unwrap(),
            Identity::from_keypair(private(2), public(2)).unwrap(),
        ] {
            let public = identity.public_key();
            let mut config =
                transport::config(&identity, peer.public_key(), psk, b"identity-check").unwrap();
            drop(identity);
            authenticate(&mut config, public, psk);
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn imported_keys_label_authenticated_sessions_with_their_proven_peers() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for psk in [None, Some([7; 32])] {
                let alice = Identity::from_keypair(private(1), public(1)).unwrap();
                let bob = Identity::from_private_key(private(2)).unwrap();
                let left = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let right = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let address = right.local_addr().unwrap();
                let (client, server) = tokio::join!(
                    transport::connect_authenticated(
                        left,
                        address,
                        &alice,
                        bob.public_key(),
                        psk,
                        b"imported"
                    ),
                    transport::accept_authenticated(
                        right,
                        &bob,
                        alice.public_key(),
                        psk,
                        b"imported"
                    ),
                );
                assert_eq!(client.unwrap().peer(), public(2));
                assert_eq!(server.unwrap().peer(), public(1));
            }
        })
        .await;
}

#[test]
fn tlc_identity_construction_and_configuration_traces() {
    use capntproto_test_support::verification::exploration;
    const MODEL: &str = "verification/NativeIdentity.tla";
    const CONFIG: &str = include_str!("../verification/NativeIdentity.cfg");
    let paths = exploration::traces(MODEL, "native-identity", CONFIG).unwrap();
    let peer = Identity::from_private_key([4; 32]).unwrap();
    let mut handshakes = 0;
    for path in &paths {
        let mut identity: Option<Identity> = None;
        let mut configured = None;
        let mut config_label = 0;
        let mut authenticated = 0;
        for state in path {
            let result = match state["event"] {
                event @ 1..=6 => {
                    let secret = (event - 1) / 3 + 1;
                    let label = (event - 1) % 3 + 1;
                    identity = Identity::from_keypair(private(secret), public(label)).ok();
                    u64::from(identity.is_some())
                }
                event @ 7..=8 => {
                    identity = Some(Identity::from_private_key(private(event - 6)).unwrap());
                    1
                }
                9 => {
                    let owner = identity.as_ref().unwrap();
                    configured = Some(
                        transport::config(owner, peer.public_key(), None, b"identity-check")
                            .unwrap(),
                    );
                    config_label = (1..=2)
                        .find(|index| owner.public_key() == public(*index))
                        .expect("configuration must carry a known fixture identity");
                    authenticated = 0;
                    1
                }
                10 => {
                    identity.take();
                    1
                }
                11 => {
                    authenticate(configured.as_mut().unwrap(), public(config_label), None);
                    handshakes += 1;
                    authenticated = config_label;
                    1
                }
                12 => {
                    configured.take();
                    config_label = 0;
                    authenticated = 0;
                    1
                }
                _ => panic!("unknown action {state:?}"),
            };
            assert_eq!(result, state["result"], "{path:?}");
            assert_eq!(
                identity.as_ref().map(Identity::public_key),
                (state["label"] != 0).then(|| public(state["label"]))
            );
            assert_eq!(
                u64::from(configured.is_some()),
                u64::from(state["configured"] != 0)
            );
            assert_eq!(config_label, state["configuredLabel"]);
            assert_eq!(authenticated, state["authenticated"]);
        }
    }
    assert!(handshakes > 0);
    exploration::controls(
        MODEL,
        "native-identity",
        CONFIG,
        &[
            ("acceptMismatch", "LiveBinding"),
            ("rejectValid", "ExpectedOutcome"),
            ("wrongDerived", "LiveBinding"),
            ("mislabelConfig", "ConfigBinding"),
            ("forgetConfig", "ConfigOwnership"),
        ],
        None,
    )
    .unwrap();
    eprintln!(
        "{} identity edge-prefix replays, {handshakes} real Native handshakes",
        paths.len()
    );
}
