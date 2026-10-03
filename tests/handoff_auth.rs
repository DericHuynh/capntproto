#![cfg(all(feature = "native", feature = "storage"))]
use reproto::{
    authority::{Grant, ObjectGeneration, ObjectId, Rights},
    handoff::{self, Introduction, Package, Serving},
    orm::ObjectState,
    rpc,
    storage::Store,
    store_capnp::{document, handoff as wire, object},
    transport::{self, Identity},
};
use std::{cell::RefCell, net::SocketAddr, rc::Rc, time::Duration};
use tokio::{io::AsyncWriteExt, net::UdpSocket};

struct Fixture {
    _dir: tempfile::TempDir,
    object: Rc<ObjectState>,
    owner: Identity,
    recipient: Identity,
    parent: Grant,
    intro: Rc<RefCell<Introduction>>,
    package: Package,
}
impl Fixture {
    fn new(pending: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Rc::new(RefCell::new(
            Store::open(dir.path().join("objects")).unwrap(),
        ));
        let object = ObjectState::new(store, ObjectId::new(7).unwrap());
        let owner = Identity::generate();
        let recipient = Identity::generate();
        let parent = Grant::root(
            ObjectId::new(7).unwrap(),
            ObjectGeneration::new(1).unwrap(),
            owner.public_key(),
            Rights::ALL,
        );
        let mut intro =
            Introduction::new(&owner, &parent, recipient.public_key(), Rights::VIEW).unwrap();
        if pending {
            assert!(intro.enqueue_proxy());
        }
        let package = intro.provide().unwrap();
        Self {
            _dir: dir,
            object,
            owner,
            recipient,
            parent,
            intro: Rc::new(RefCell::new(intro)),
            package,
        }
    }
    fn replace(&self, pending: bool) -> Package {
        let other = Identity::generate();
        let mut intro =
            Introduction::new(&self.owner, &self.parent, other.public_key(), Rights::VIEW).unwrap();
        if pending {
            assert!(intro.enqueue_proxy());
        }
        let package = intro.provide().unwrap();
        *self.intro.borrow_mut() = intro;
        package
    }
    async fn serve(&self, socket: UdpSocket) -> capnp::Result<Serving> {
        handoff::serve::<document::Owned>(
            self.intro.clone(),
            self.object.clone(),
            &self.owner,
            socket,
        )
        .await
    }
}

// Independent wire peer: deliberately exercises the public raw transport API,
// the native prologue and stream preface, without access to session internals.
struct Peer {
    client: Option<wire::Client<document::Owned>>,
    network: tokio::task::JoinHandle<std::io::Result<()>>,
    rpc: Option<tokio::task::JoinHandle<capnp::Result<()>>>,
    _unframed: Option<tokio::io::DuplexStream>,
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.network.abort();
        if let Some(rpc) = &self.rpc {
            rpc.abort();
        }
    }
}
impl Peer {
    async fn dial(f: &Fixture, address: SocketAddr, fault: &str) -> Self {
        let wrong = Identity::generate();
        let identity = if fault == "peer" {
            &wrong
        } else {
            &f.recipient
        };
        let target = if fault == "target" {
            wrong.public_key()
        } else {
            f.package.target
        };
        let psk = match fault {
            "psk" => Some([0; 32]),
            "no-psk" => None,
            _ => Some(f.package.psk),
        };
        let mut context = b"ReProto native RPC v1\0".to_vec();
        context.extend_from_slice(&f.package.context);
        if fault == "context" {
            *context.last_mut().unwrap() ^= 1;
        }
        if fault == "domain" {
            context = f.package.context.to_vec();
        }
        let mut config = transport::config(identity, target, psk, &context).unwrap();
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (mut io, network) = transport::connect(socket, address, &mut config)
            .await
            .unwrap();
        if fault == "missing-preface" {
            return Self {
                client: None,
                network,
                rpc: None,
                _unframed: Some(io),
            };
        }
        io.write_all(if fault == "preface" { b"!" } else { b"R" })
            .await
            .unwrap();
        let (client, rpc) = rpc::client(io);
        Self {
            client: Some(client),
            network,
            rpc: Some(rpc),
            _unframed: None,
        }
    }
    async fn accept(&self, id: &[u8; 32]) -> capnp::Result<object::Client<document::Owned>> {
        let mut request = self.client.as_ref().unwrap().accept_request();
        request.get().set_id(id);
        tokio::time::timeout(Duration::from_secs(2), request.send().promise)
            .await
            .expect("RPC response deadline")?
            .get()?
            .get_object()
    }
}
async fn socket() -> UdpSocket {
    UdpSocket::bind("127.0.0.1:0").await.unwrap()
}
async fn connected(f: &Fixture) -> (Serving, Peer, SocketAddr) {
    let socket = socket().await;
    let address = socket.local_addr().unwrap();
    let peer = Peer::dial(f, address, "valid").await;
    let serving = tokio::time::timeout(Duration::from_secs(2), f.serve(socket))
        .await
        .unwrap()
        .unwrap();
    (serving, peer, address)
}

#[tokio::test(flavor = "current_thread")]
async fn rejects_unproven_peers_psks_contexts_and_stream_prefaces() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for fault in [
                "peer",
                "target",
                "psk",
                "no-psk",
                "context",
                "domain",
                "preface",
                "missing-preface",
            ] {
                let f = Fixture::new(false);
                let socket = socket().await;
                let address = socket.local_addr().unwrap();
                let before = f.intro.borrow().state();
                let _peer = Peer::dial(&f, address, fault).await;
                let result =
                    tokio::time::timeout(Duration::from_millis(250), f.serve(socket)).await;
                if fault == "missing-preface" {
                    assert!(
                        result.is_err(),
                        "handshake without stream proof must remain pending"
                    );
                }
                assert!(
                    !matches!(result, Ok(Ok(_))),
                    "published unproven bootstrap: {fault}"
                );
                // A missing proof can remain pending until cancellation; neither
                // an error nor this bounded wait may advance the introduction.
                assert_eq!(f.intro.borrow().state(), before, "{fault}");
                released(address).await;
            }
            let f = Fixture::new(false);
            let (_serving, peer, _) = connected(&f).await;
            let cap = peer.accept(&f.package.id).await.unwrap();
            assert!(
                cap.put_request().send().promise.await.is_err(),
                "delegation amplified rights"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn preflight_rejects_wrong_host_object_unprovided_and_revoked_grants() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for fault in ["host", "object", "unprovided", "revoked"] {
                let f = Fixture::new(false);
                let wrong = Identity::generate();
                let owner = if fault == "host" { &wrong } else { &f.owner };
                let object = if fault == "object" {
                    ObjectState::new(f.object.store().clone(), ObjectId::new(8).unwrap())
                } else {
                    f.object.clone()
                };
                if fault == "unprovided" {
                    *f.intro.borrow_mut() = Introduction::new(
                        &f.owner,
                        &f.parent,
                        f.recipient.public_key(),
                        Rights::VIEW,
                    )
                    .unwrap();
                }
                if fault == "revoked" {
                    f.parent.revoke();
                }
                let before = f.intro.borrow().state();
                let result = tokio::time::timeout(
                    Duration::from_secs(1),
                    handoff::serve::<document::Owned>(
                        f.intro.clone(),
                        object,
                        owner,
                        socket().await,
                    ),
                )
                .await
                .unwrap();
                assert!(result.is_err(), "{fault}");
                assert_eq!(f.intro.borrow().state(), before);
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn replacement_or_revocation_during_authentication_cannot_publish() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for revoke in [false, true] {
                let f = Fixture::new(false);
                let socket = socket().await;
                let address = socket.local_addr().unwrap();
                let mut serving = Box::pin(f.serve(socket));
                assert!(futures::poll!(serving.as_mut()).is_pending());
                if revoke {
                    f.parent.revoke();
                } else {
                    f.replace(false);
                }
                let _peer = Peer::dial(&f, address, "valid").await;
                assert!(tokio::time::timeout(Duration::from_secs(2), serving)
                    .await
                    .unwrap()
                    .is_err());
                assert!(!f.intro.borrow().state().accepted());
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn established_bootstrap_cannot_be_rebound_to_another_introduction() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let f = Fixture::new(false);
            let (_serving, peer, _) = connected(&f).await;
            let _original = peer.accept(&f.package.id).await.unwrap();
            let replacement = f.replace(false);
            for id in [f.package.id, replacement.id] {
                assert!(peer.accept(&id).await.is_err());
                assert!(!f.intro.borrow().state().accepted());
            }
        })
        .await;
}

async fn released(address: SocketAddr) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if UdpSocket::bind(address).await.is_ok() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cancelled server retained its UDP socket");
}
#[tokio::test(flavor = "current_thread")]
async fn cancelling_accept_or_serving_wait_releases_its_socket() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let f = Fixture::new(false);
            let socket = socket().await;
            let address = socket.local_addr().unwrap();
            let mut accepting = Box::pin(f.serve(socket));
            assert!(futures::poll!(accepting.as_mut()).is_pending());
            drop(accepting);
            released(address).await;
            let (serving, _peer, address) = connected(&f).await;
            let mut waiting = Box::pin(serving.wait());
            assert!(futures::poll!(waiting.as_mut()).is_pending());
            drop(waiting);
            released(address).await;
            assert!(!f.intro.borrow().state().accepted());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn tlc_authenticated_handoff_traces() {
    use reproto_test_support::verification::exploration;
    const MODEL: &str = "verification/AuthenticatedHandoff.tla";
    const CONFIG: &str = include_str!("../verification/AuthenticatedHandoff.cfg");
    let paths = exploration::traces(MODEL, "authenticated-handoff", CONFIG).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for path in &paths {
                let f = Fixture::new(true);
                let mut serving = None;
                let mut peer = None;
                let mut replacement = None;
                let mut first_cap: Option<object::Client<document::Owned>> = None;
                for state in path {
                    let result = match state["event"] {
                        event @ 1..=2 => {
                            let socket = socket().await;
                            peer = Some(
                                Peer::dial(
                                    &f,
                                    socket.local_addr().unwrap(),
                                    if event == 1 { "valid" } else { "context" },
                                )
                                .await,
                            );
                            serving = tokio::time::timeout(Duration::from_secs(2), f.serve(socket))
                                .await
                                .expect("authentication deadline")
                                .ok();
                            u64::from(serving.is_some())
                        }
                        3 => u64::from(f.intro.borrow_mut().finish_proxy()),
                        event @ 4..=5 => {
                            let mut id = replacement
                                .as_ref()
                                .map_or(f.package.id, |p: &Package| p.id);
                            if event == 5 {
                                id[0] ^= 1;
                            }
                            let result = peer.as_ref().unwrap().accept(&id).await;
                            let success = u64::from(result.is_ok());
                            if let Ok(cap) = result {
                                let resolved = capnp::capability::get_resolved_cap(cap).await;
                                if let Some(first) = &first_cap {
                                    assert_eq!(
                                        resolved.client.hook.get_ptr(),
                                        first.client.hook.get_ptr(),
                                        "replay minted another capability"
                                    );
                                } else {
                                    // Retain the proxy so equality cannot be satisfied
                                    // by a deallocated/reused capability table entry.
                                    first_cap = Some(resolved);
                                }
                            }
                            success
                        }
                        6 => {
                            f.parent.revoke();
                            1
                        }
                        7 => {
                            replacement = Some(f.replace(true));
                            1
                        }
                        8 => {
                            serving.take();
                            peer.as_ref().unwrap().network.abort();
                            tokio::task::yield_now().await;
                            1
                        }
                        _ => panic!("unknown event"),
                    };
                    assert_eq!(result, state["result"], "{path:?}");
                    let actual = f.intro.borrow().state();
                    use reproto::semantics::HandoffPhase;
                    let phase = match actual.phase() {
                        HandoffPhase::Proxying => 0,
                        HandoffPhase::Offered => 1,
                        HandoffPhase::Embargoed => 2,
                        HandoffPhase::Direct => 3,
                    };
                    assert_eq!(phase, state["phase"], "{path:?}");
                    assert_eq!(actual.pending(), state["pending"], "{path:?}");
                    assert_eq!(u64::from(actual.accepted()), state["accepted"], "{path:?}");
                    assert_eq!(u64::from(f.parent.is_live()), state["live"], "{path:?}");
                    assert_eq!(
                        u64::from(serving.is_some()),
                        u64::from(state["status"] == 1)
                    );
                }
            }
        })
        .await;
    exploration::controls(
        MODEL,
        "authenticated-handoff",
        CONFIG,
        &[
            ("falseProof", "AuthenticatedPublication"),
            ("wrongId", "CapabilitySafety"),
            ("revoked", "CapabilitySafety"),
            ("rebound", "CapabilitySafety"),
            ("earlyGrant", "CapabilitySafety"),
            ("duplicateGrant", "SingleAcceptance"),
        ],
        None,
    )
    .unwrap();
    eprintln!(
        "{} real UDP/Native handoff edge-prefix replays",
        paths.len()
    );
}
