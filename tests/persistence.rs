use capnp::capability::{Client, FromClientHook, Promise};
use capntproto::storage::ObjectKey;
use capntproto::storage::Revision;
use capntproto::{
    authority::{Grant, ObjectGeneration, ObjectId, Rights},
    persistence::{
        Descriptor, Limits, ObjectFactory, ObjectKind, OwnerId, Persistent, Realm, SturdyRef,
    },
    persistence_capnp as wire,
    storage::Store,
    store_capnp::{document, object},
};
use capntproto_test_support::runtime_test_capnp::harness;
use futures::{channel::oneshot, FutureExt};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

const KIND: ObjectKind = ObjectKind::new(1).unwrap();
const OBJECT: ObjectId = ObjectId::new(42).unwrap();
const OWNER: OwnerId = [1; 16];
const PEER: [u8; 32] = [1; 32];
struct Echo;
impl harness::Server for Echo {
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
        results.get().set_cap(params.get()?.get_cap()?);
        Ok(())
    }
    async fn stream(self: Rc<Self>, _: harness::StreamParams) -> capnp::Result<()> {
        Ok(())
    }
}
fn client() -> harness::Client {
    capnp_rpc::new_client(Echo)
}
fn descriptor() -> Descriptor {
    Descriptor::new(KIND, OBJECT, ObjectGeneration::new(1).unwrap(), Rights::ALL)
}
fn factory(realm: &Realm) {
    realm
        .register_factory(
            KIND,
            Rc::new(|_: &Descriptor, _| Promise::ok(client().client)),
        )
        .unwrap();
}
fn bound(realm: &Realm) -> harness::Client {
    realm
        .persistent(client(), descriptor(), |_| Ok(()))
        .unwrap()
}
async fn save<C: FromClientHook>(cap: &C, owner: OwnerId) -> capnp::Result<SturdyRef> {
    let persistent = Persistent::new(cap.as_client_hook().add_ref());
    let mut request = persistent.save_request();
    request.get().init_seal_for().set_id(&owner);
    SturdyRef::read(request.send().promise.await?.get()?.get_sturdy_ref()?)
}
async fn echo(cap: &Client) {
    let cap = harness::Client::new(cap.hook.add_ref());
    let mut call = cap.echo_request();
    call.get().set_value(123);
    assert_eq!(
        call.send()
            .promise
            .await
            .unwrap()
            .get()
            .unwrap()
            .get_value(),
        123
    );
}
fn open_limits(path: &std::path::Path, limits: Limits) -> Realm {
    let (executor, driver) = capnp_rpc::new_call_executor();
    tokio::task::spawn_local(driver);
    Realm::open_with_executor(path, limits, executor).unwrap()
}
fn open(path: &std::path::Path) -> Realm {
    open_limits(path, Limits::default())
}

#[tokio::test(flavor = "current_thread")]
async fn sealed_save_restore_rotation_and_restart() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("realm");
            let realm = open(&path);
            let realm_id = realm.id();
            realm.register_owner(OWNER, PEER).unwrap();
            factory(&realm);
            let cap = bound(&realm);
            assert!(cap.debug_info().starts_with("persistent:local:"));
            echo(&cap.client).await;
            cap.stream_request().send().await.unwrap();
            let mut bounce = cap.bounce_request();
            bounce.get().set_cap(client());
            let bounce = bounce.send();
            let mut pipeline = bounce.pipeline.get_cap().echo_request();
            pipeline.get().set_value(99);
            assert_eq!(
                pipeline
                    .send()
                    .promise
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_value(),
                99
            );
            bounce.promise.await.unwrap();
            let reference = save(&cap, OWNER).await.unwrap();
            let second = save(&cap, OWNER).await.unwrap();
            assert_ne!(reference, second);
            assert!(!format!("{reference:?}").contains("token"));
            assert!(save(&cap, [2; 16]).await.is_err());
            assert!(realm.restore(reference.clone(), [2; 32]).await.is_err());
            let live = realm.restore(reference.clone(), PEER).await.unwrap();
            echo(&live).await;
            let renewed = save(&live, OWNER).await.unwrap();
            realm.register_owner([2; 16], [3; 32]).unwrap();
            assert!(save(&live, [2; 16]).await.is_err()); // No broader sealing policy on restored facets.
            assert_eq!(realm.rotate_owner(OWNER, 1, [2; 32]).unwrap(), 3);
            assert!(realm.rotate_owner(OWNER, 1, [4; 32]).is_err());
            assert!(realm.restore(reference.clone(), PEER).await.is_err());
            assert!(save(&live, OWNER).await.is_err());
            echo(&live).await; // Rotation changes restoration, not existing live authority.
            realm.close();
            drop(realm);
            assert!(save(&cap, OWNER).await.is_err());
            let realm = open(&path);
            assert_eq!(realm.id(), realm_id);
            factory(&realm);
            echo(&realm.restore(reference.clone(), [2; 32]).await.unwrap()).await;
            realm.revoke(&reference).unwrap();
            realm.revoke(&reference).unwrap();
            assert!(realm.restore(reference.clone(), [2; 32]).await.is_err());
            echo(&realm.restore(renewed, [2; 32]).await.unwrap()).await;
            realm.close();
            let realm = open(&path);
            factory(&realm);
            assert!(realm.restore(reference, [2; 32]).await.is_err());
            echo(&realm.restore(second, [2; 32]).await.unwrap()).await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn validation_quotas_and_save_authority_are_atomic() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("realm");
            let realm = open_limits(
                &path,
                Limits {
                    owners: 2,
                    references: 1,
                },
            );
            assert!(realm.register_owner([0; 16], PEER).is_err());
            assert!(realm.register_owner(OWNER, [0; 32]).is_err());
            realm.register_owner(OWNER, PEER).unwrap();
            assert!(realm.register_owner([2; 16], PEER).is_err());
            realm.register_owner([2; 16], [2; 32]).unwrap();
            assert!(realm.rotate_owner(OWNER, 1, [2; 32]).is_err());
            factory(&realm);
            assert!(realm
                .register_factory(
                    KIND,
                    Rc::new(|_: &Descriptor, _| Promise::ok(client().client))
                )
                .is_err());
            let permitted = Rc::new(Cell::new(false));
            let flag = permitted.clone();
            let cap = realm
                .persistent(client(), descriptor(), move |_| {
                    if flag.get() {
                        Ok(())
                    } else {
                        Err(capnp::Error::failed("revoked".into()))
                    }
                })
                .unwrap();
            assert!(save(&cap, OWNER).await.is_err());
            let no_delegate = realm
                .persistent(
                    client(),
                    Descriptor::new(KIND, OBJECT, ObjectGeneration::new(1).unwrap(), Rights::GET),
                    |_| Ok(()),
                )
                .unwrap();
            assert!(save(&no_delegate, OWNER).await.is_err());
            let mut bad = Persistent::new(cap.as_client_hook().add_ref()).save_request();
            bad.get().init_seal_for().set_id(&[1; 15]);
            assert!(bad.send().promise.await.is_err());
            permitted.set(true);
            let reference = save(&cap, OWNER).await.unwrap();
            assert!(save(&cap, OWNER).await.is_err());
            realm.revoke(&reference).unwrap();
            assert!(save(&cap, OWNER).await.is_err()); // Tombstones consume quota.
            realm.close();
            let bytes = std::fs::read(&path).unwrap();
            assert!(Realm::open(
                &path,
                Limits {
                    owners: 1,
                    references: 1
                }
            )
            .is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            let other = dir.path().join("orm");
            {
                let mut store = Store::open(&other).unwrap();
                store
                    .put(ObjectKey::new(5), Revision::INITIAL, b"object")
                    .unwrap();
            }
            assert!(Realm::open(&other, Limits::default()).is_err());
        })
        .await;
}

struct Dropped(Rc<Cell<usize>>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}
struct DropEcho {
    _guard: Dropped,
}
impl harness::Server for DropEcho {
    async fn echo(
        self: Rc<Self>,
        params: harness::EchoParams,
        mut results: harness::EchoResults,
    ) -> capnp::Result<()> {
        results.get().set_value(params.get()?.get_value());
        Ok(())
    }
}
type Gate = Rc<RefCell<Option<oneshot::Sender<()>>>>;
fn gated_factory(realm: &Realm, gate: &Gate, tasks: &Rc<Cell<usize>>, caps: &Rc<Cell<usize>>) {
    let gate = gate.clone();
    let tasks = tasks.clone();
    let caps = caps.clone();
    realm
        .register_factory(
            KIND,
            Rc::new(move |_: &Descriptor, _| {
                let (send, recv) = oneshot::channel();
                *gate.borrow_mut() = Some(send);
                let task = Dropped(tasks.clone());
                let caps = caps.clone();
                Promise::from_future(async move {
                    let _task = task;
                    recv.await
                        .map_err(|_| capnp::Error::failed("canceled".into()))?;
                    let cap: harness::Client = capnp_rpc::new_client(DropEcho {
                        _guard: Dropped(caps),
                    });
                    Ok(cap.client)
                })
            }),
        )
        .unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn pending_factory_rechecks_seal_epoch_lifetime_and_cancellation() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for action in [
                "revoke", "rotate", "aba", "close", "drop", "cancel", "success",
            ] {
                let dir = tempfile::tempdir().unwrap();
                let realm = open(&dir.path().join("realm"));
                realm.register_owner(OWNER, PEER).unwrap();
                let gate = Rc::new(RefCell::new(None));
                let tasks = Rc::new(Cell::new(0));
                let caps = Rc::new(Cell::new(0));
                gated_factory(&realm, &gate, &tasks, &caps);
                let reference = save(&bound(&realm), OWNER).await.unwrap();
                let pending = realm.restore(reference.clone(), PEER);
                match action {
                    "revoke" => realm.revoke(&reference).unwrap(),
                    "rotate" => {
                        realm.rotate_owner(OWNER, 1, [2; 32]).unwrap();
                    }
                    "aba" => {
                        realm.rotate_owner(OWNER, 1, [2; 32]).unwrap();
                        realm.rotate_owner(OWNER, 2, PEER).unwrap();
                    }
                    "close" => realm.close(),
                    _ => (),
                }
                let retained = if action == "drop" {
                    drop(realm);
                    None
                } else {
                    Some(realm)
                };
                if action == "cancel" {
                    drop(pending);
                    assert_eq!(tasks.get(), 1);
                    assert!(gate.borrow_mut().take().unwrap().send(()).is_err());
                    assert_eq!(caps.get(), 0);
                } else {
                    gate.borrow_mut().take().unwrap().send(()).unwrap();
                    let result = pending.await;
                    assert_eq!(result.is_ok(), action == "success", "{action}");
                    assert_eq!(tasks.get(), 1);
                    drop(result);
                    assert_eq!(caps.get(), 1, "factory result leaked: {action}");
                }
                drop(retained);
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn application_factory_can_reenter_and_errors_release_borrows() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let realm = open(&dir.path().join("realm"));
            realm.register_owner(OWNER, PEER).unwrap();
            // Avoid a factory/realm cycle: take the temporary administrative handle on invocation.
            let slot = Rc::new(RefCell::new(Some(realm.clone())));
            let state = slot.clone();
            realm
                .register_factory(
                    KIND,
                    Rc::new(move |_: &Descriptor, _| {
                        let realm = state.borrow_mut().take().unwrap();
                        realm.rotate_owner(OWNER, 1, [2; 32]).unwrap();
                        Promise::ok(client().client)
                    }),
                )
                .unwrap();
            let reference = save(&bound(&realm), OWNER).await.unwrap();
            assert!(realm.restore(reference, PEER).await.is_err());
            assert!(slot.borrow().is_none());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn orm_restart_restores_exact_rights_and_generation() {
    tokio::task::LocalSet::new()
        .run_until(async {
            type Doc = object::Client<document::Owned>;
            let dir = tempfile::tempdir().unwrap();
            let rp = dir.path().join("realm");
            let dp = dir.path().join("objects");
            let reference = {
                let realm = open(&rp);
                realm.register_owner(OWNER, PEER).unwrap();
                let store = Rc::new(RefCell::new(Store::open(&dp).unwrap()));
                let factory = Rc::new(ObjectFactory::<document::Owned>::new(
                    store,
                    ObjectGeneration::new(7).unwrap(),
                ));
                realm.register_factory(KIND, factory.clone()).unwrap();
                let root = Grant::root(
                    ObjectId::new(42).unwrap(),
                    ObjectGeneration::new(7).unwrap(),
                    PEER,
                    Rights::ALL,
                );
                let cap: Doc = realm
                    .persistent_object(factory.state(OBJECT), root.clone(), KIND)
                    .unwrap();
                let mut put = cap.put_request();
                put.get().init_value().set_text("durable");
                let revision = put
                    .send()
                    .promise
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_revision();
                let mut publish = cap.publish_request();
                publish.get().set_revision(revision);
                publish.send().promise.await.unwrap();
                let rights =
                    Rights::from_bits(Rights::VIEW.bits() | Rights::DELEGATE.bits()).unwrap();
                let limited: Doc = realm
                    .persistent_object(
                        factory.state(OBJECT),
                        root.delegate(PEER, rights).unwrap(),
                        KIND,
                    )
                    .unwrap();
                let reference = save(&limited, OWNER).await.unwrap();
                root.revoke();
                assert!(save(&limited, OWNER).await.is_err());
                let restored: Doc = realm
                    .restore(reference.clone(), PEER)
                    .await
                    .unwrap()
                    .cast_to();
                assert!(restored.get_request().send().promise.await.is_ok());
                reference
            };
            for generation in [8, 7] {
                let realm = open(&rp);
                let factory = Rc::new(ObjectFactory::<document::Owned>::new(
                    Rc::new(RefCell::new(Store::open(&dp).unwrap())),
                    ObjectGeneration::new(generation).unwrap(),
                ));
                realm.register_factory(KIND, factory.clone()).unwrap();
                let result = realm.restore(reference.clone(), PEER).await;
                if generation == 8 {
                    assert!(result.is_err());
                    continue;
                }
                let restored: Doc = result.unwrap().cast_to();
                let get = restored.get_request().send().promise.await.unwrap();
                assert_eq!(
                    get.get().unwrap().get_value().unwrap().get_text().unwrap(),
                    "durable"
                );
                assert!(restored.put_request().send().promise.await.is_err());
                assert!(restored.publish_request().send().promise.await.is_err());
                let a = factory.state(OBJECT);
                let b = factory.state(OBJECT);
                assert!(Rc::ptr_eq(&a, &b));
                save(&restored, OWNER).await.unwrap();
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn every_torn_ledger_transaction_recovers_last_committed_authority() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("realm");
            let trial = dir.path().join("trial");
            let realm = open(&path);
            realm.register_owner(OWNER, PEER).unwrap();
            factory(&realm);
            let before_save = std::fs::read(&path).unwrap();
            let reference = save(&bound(&realm), OWNER).await.unwrap();
            let after_save = std::fs::read(&path).unwrap();
            realm.rotate_owner(OWNER, 1, [2; 32]).unwrap();
            let after_rotate = std::fs::read(&path).unwrap();
            realm.revoke(&reference).unwrap();
            let after_revoke = std::fs::read(&path).unwrap();
            realm.close();
            for (old, new, old_valid, new_valid, peer) in [
                (&before_save, &after_save, false, true, PEER),
                (&after_save, &after_rotate, true, false, PEER),
                (&after_rotate, &after_revoke, true, false, [2; 32]),
            ] {
                for cut in old.len()..=new.len() {
                    std::fs::write(&trial, &new[..cut]).unwrap();
                    let realm = open(&trial);
                    factory(&realm);
                    assert_eq!(
                        realm.restore(reference.clone(), peer).await.is_ok(),
                        if cut == new.len() {
                            new_valid
                        } else {
                            old_valid
                        },
                        "cut {cut}/{}",
                        new.len()
                    );
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn realm_bootstrap_uses_authenticated_native_identity() {
    tokio::task::LocalSet::new()
        .run_until(async {
            use capntproto::{
                native_rpc::{Handle, Network},
                transport::{self, Identity},
            };
            async fn pair(a: &Identity, b: &Identity, ah: &Handle, bh: &Handle) {
                let left = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let right = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let address = right.local_addr().unwrap();
                let (a, b) = tokio::join!(
                    transport::connect_authenticated(
                        left,
                        address,
                        a,
                        b.public_key(),
                        None,
                        b"realm"
                    ),
                    transport::accept_authenticated(right, b, a.public_key(), None, b"realm")
                );
                ah.attach(a.unwrap()).unwrap();
                bh.attach(b.unwrap()).unwrap();
            }
            struct Tasks(Vec<tokio::task::JoinHandle<capnp::Result<()>>>);
            impl Drop for Tasks {
                fn drop(&mut self) {
                    for t in &self.0 {
                        t.abort();
                    }
                }
            }
            async fn restore(
                cap: &wire::restorer::Client,
                reference: &SturdyRef,
            ) -> capnp::Result<Client> {
                let mut call = cap.restore_request();
                reference.write(call.get().init_reference());
                call.send()
                    .promise
                    .await?
                    .get()?
                    .get_cap()
                    .get_as_capability()
            }
            tokio::task::LocalSet::new()
                .run_until(async {
                    tokio::time::timeout(std::time::Duration::from_secs(10), async {
                        let dir = tempfile::tempdir().unwrap();
                        let realm = open(&dir.path().join("realm"));
                        let server = Identity::generate();
                        let owner = Identity::generate();
                        let replacement = Identity::generate();
                        realm.register_owner(OWNER, owner.public_key()).unwrap();
                        factory(&realm);
                        let reference = save(&bound(&realm), OWNER).await.unwrap();
                        let (sn, sh) = Network::new(server.public_key());
                        let (an, ah) = Network::new(owner.public_key());
                        let (bn, bh) = Network::new(replacement.public_key());
                        pair(&owner, &server, &ah, &sh).await;
                        pair(&replacement, &server, &bh, &sh).await;
                        let s = capnp_rpc::RpcSystem::new_with_bootstrap_factory(
                            Box::new(sn),
                            server.public_key(),
                            realm.bootstrap_factory(),
                        );
                        let mut a = capnp_rpc::RpcSystem::new(Box::new(an), None);
                        let mut b = capnp_rpc::RpcSystem::new(Box::new(bn), None);
                        let ac: wire::restorer::Client = a.bootstrap(server.public_key());
                        let bc: wire::restorer::Client = b.bootstrap(server.public_key());
                        let _tasks = Tasks(vec![
                            tokio::task::spawn_local(s),
                            tokio::task::spawn_local(a),
                            tokio::task::spawn_local(b),
                        ]);
                        let live = restore(&ac, &reference).await.unwrap();
                        echo(&live).await;
                        assert!(restore(&bc, &reference).await.is_err());
                        let renewal = save(&live, OWNER).await.unwrap();
                        realm
                            .rotate_owner(OWNER, 1, replacement.public_key())
                            .unwrap();
                        assert!(restore(&ac, &reference).await.is_err());
                        assert!(save(&live, OWNER).await.is_err());
                        echo(&restore(&bc, &reference).await.unwrap()).await;
                        realm.revoke(&reference).unwrap();
                        assert!(restore(&bc, &reference).await.is_err());
                        echo(&restore(&bc, &renewal).await.unwrap()).await;
                    })
                    .await
                    .unwrap();
                })
                .await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_persistence_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_PERSISTENCE_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for (index, case) in cases.iter().enumerate() {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("realm");
                let mut realm = open(&path);
                let id = realm.id();
                realm.register_owner(OWNER, PEER).unwrap();
                let gate = Rc::new(RefCell::new(None));
                let tasks = Rc::new(Cell::new(0));
                let caps = Rc::new(Cell::new(0));
                gated_factory(&realm, &gate, &tasks, &caps);
                let reference = save(&bound(&realm), OWNER).await.unwrap();
                let mut pending = None;
                let mut live: Option<Client> = None;
                let mut renewed = None;
                let mut epoch = 1u64;
                for step in case["steps"].as_array().unwrap() {
                    let state: Vec<u64> = step["state"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_u64().unwrap())
                        .collect();
                    match step["action"].as_str().unwrap() {
                        "begin" => {
                            let peer = if epoch == 2 { [2; 32] } else { PEER };
                            pending = Some(realm.restore(reference.clone(), peer));
                            assert!(pending.as_mut().unwrap().now_or_never().is_none());
                        }
                        "finish" => {
                            gate.borrow_mut().take().unwrap().send(()).unwrap();
                            let result = pending.take().unwrap().await;
                            assert_eq!(result.is_ok(), state[3] == 2, "trace {index}: {step}");
                            live = result.ok();
                        }
                        "cancel" => {
                            drop(pending.take().unwrap());
                            assert!(gate.borrow_mut().take().unwrap().send(()).is_err());
                        }
                        "revoke" => realm.revoke(&reference).unwrap(),
                        "rotate" => {
                            epoch = realm
                                .rotate_owner(OWNER, epoch, if epoch == 1 { [2; 32] } else { PEER })
                                .unwrap();
                        }
                        "close" => realm.close(),
                        "reopen" => {
                            realm = open(&path);
                            assert_eq!(realm.id(), id);
                            gated_factory(&realm, &gate, &tasks, &caps);
                        }
                        "intruder" => {
                            assert!(realm.restore(reference.clone(), [3; 32]).await.is_err())
                        }
                        "renew" => {
                            let result = save(live.as_ref().unwrap(), OWNER).await;
                            assert_eq!(result.is_ok(), state[12] == 1, "trace {index}: {step}");
                            renewed = result.ok();
                        }
                        action => panic!("unknown action {action}"),
                    }
                    assert_eq!(epoch, state[2]);
                    assert_eq!(pending.is_some(), state[3] == 1);
                    assert_eq!(live.is_some(), state[3] == 2);
                    assert_eq!(renewed.is_some(), state[12] == 1);
                    assert_eq!(tasks.get(), usize::from(state[3] >= 2));
                    assert_eq!(caps.get(), usize::from(state[3] == 3));
                    if let Some(cap) = &live {
                        echo(cap).await;
                    }
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn malformed_references_and_committed_metadata_fail_closed() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("realm");
            let realm = open(&path);
            realm.register_owner(OWNER, PEER).unwrap();
            factory(&realm);
            let reference = save(&bound(&realm), OWNER).await.unwrap();
            let mut message = capnp::message::Builder::new_default();
            reference.write(message.init_root::<wire::sturdy_ref::Builder>());
            let token = message
                .get_root_as_reader::<wire::sturdy_ref::Reader>()
                .unwrap()
                .get_token()
                .unwrap()
                .to_vec();
            message
                .get_root::<wire::sturdy_ref::Builder>()
                .unwrap()
                .set_token(&[0; 32]);
            let forged = SturdyRef::read(message.get_root_as_reader().unwrap()).unwrap();
            assert!(realm.restore(forged, PEER).await.is_err());
            reference.write(message.get_root().unwrap());
            message
                .get_root::<wire::sturdy_ref::Builder>()
                .unwrap()
                .set_realm(&[0; 16]);
            let foreign = SturdyRef::read(message.get_root_as_reader().unwrap()).unwrap();
            assert!(realm.restore(foreign, PEER).await.is_err());
            for size in [0, 15, 17, 31, 33] {
                reference.write(message.get_root().unwrap());
                message
                    .get_root::<wire::sturdy_ref::Builder>()
                    .unwrap()
                    .set_token(&vec![0; size]);
                assert!(SturdyRef::read(message.get_root_as_reader().unwrap()).is_err());
            }
            realm.close();
            let ledger: serde_json::Value = {
                let store = Store::open(&path).unwrap();
                let id = store.objects().next().unwrap();
                serde_json::from_slice(store.revision(id, store.head(id)).unwrap().bytes()).unwrap()
            };
            assert_ne!(ledger["references"][0]["hash"], serde_json::json!(token));
            let original = std::fs::read(&path).unwrap();
            for mutation in [
                "rights",
                "missingOwner",
                "duplicateOwner",
                "duplicateReference",
                "format",
                "unknownField",
                "published",
            ] {
                let trial = dir.path().join("bad");
                std::fs::write(&trial, &original).unwrap();
                let mut changed = ledger.clone();
                match mutation {
                    "rights" => changed["references"][0]["descriptor"]["rights"] = 255.into(),
                    "missingOwner" => changed["owners"] = serde_json::json!([]),
                    "duplicateOwner" => {
                        let owner = changed["owners"][0].clone();
                        changed["owners"].as_array_mut().unwrap().push(owner);
                    }
                    "duplicateReference" => {
                        let reference = changed["references"][0].clone();
                        changed["references"]
                            .as_array_mut()
                            .unwrap()
                            .push(reference);
                    }
                    "format" => changed["format"] = "unknown".into(),
                    "unknownField" => changed["extra"] = true.into(),
                    "published" => (),
                    _ => unreachable!(),
                }
                {
                    let mut store = Store::open(&trial).unwrap();
                    let id = store.objects().next().unwrap();
                    let head = store.head(id);
                    if mutation == "published" {
                        store.publish(id, head, Revision::INITIAL).unwrap();
                    } else {
                        store
                            .put(id, head, &serde_json::to_vec(&changed).unwrap())
                            .unwrap();
                    }
                }
                assert!(
                    Realm::open(&trial, Limits::default()).is_err(),
                    "{mutation}"
                );
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn restored_orm_facets_share_publication_notifications() {
    use capntproto::store_capnp::observer;
    struct Observer(Rc<Cell<u64>>);
    impl observer::Server<document::Owned> for Observer {
        async fn changed(
            self: Rc<Self>,
            params: observer::ChangedParams<document::Owned>,
            _: observer::ChangedResults<document::Owned>,
        ) -> capnp::Result<()> {
            self.0.set(params.get()?.get_revision());
            Ok(())
        }
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let realm = open(&dir.path().join("realm"));
            realm.register_owner(OWNER, PEER).unwrap();
            let factory = Rc::new(ObjectFactory::<document::Owned>::new(
                Rc::new(RefCell::new(
                    Store::open(dir.path().join("objects")).unwrap(),
                )),
                ObjectGeneration::new(1).unwrap(),
            ));
            realm.register_factory(KIND, factory.clone()).unwrap();
            let original = realm
                .persistent_object::<document::Owned>(
                    factory.state(OBJECT),
                    Grant::root(
                        ObjectId::new(42).unwrap(),
                        ObjectGeneration::new(1).unwrap(),
                        PEER,
                        Rights::ALL,
                    ),
                    KIND,
                )
                .unwrap();
            let reference = save(&original, OWNER).await.unwrap();
            drop(original);
            let a: object::Client<document::Owned> = realm
                .restore(reference.clone(), PEER)
                .await
                .unwrap()
                .cast_to();
            let b: object::Client<document::Owned> =
                realm.restore(reference, PEER).await.unwrap().cast_to();
            let observed = Rc::new(Cell::new(0));
            let observer = capnp_rpc::new_client(Observer(observed.clone()));
            let mut sub = a.subscribe_request();
            sub.get().set_observer(observer);
            let response = sub.send().promise.await.unwrap();
            let subscription = response.get().unwrap().get_subscription().unwrap();
            let mut put = b.put_request();
            put.get().init_value().set_text("shared update");
            let revision = put
                .send()
                .promise
                .await
                .unwrap()
                .get()
                .unwrap()
                .get_revision();
            let mut publish = b.publish_request();
            publish.get().set_revision(revision);
            publish.send().promise.await.unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(1), async {
                while observed.get() != revision {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            subscription.cancel_request().send().promise.await.unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn protected_save_waits_for_earlier_local_stream_policy() {
    struct Streaming {
        gate: RefCell<Option<oneshot::Receiver<()>>>,
        permitted: Rc<Cell<bool>>,
        deny: bool,
    }
    impl harness::Server for Streaming {
        async fn stream(self: Rc<Self>, _: harness::StreamParams) -> capnp::Result<()> {
            let gate = self.gate.borrow_mut().take().unwrap();
            gate.await.unwrap();
            self.permitted.set(!self.deny);
            Ok(())
        }
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            for deny in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("realm");
                let realm = open(&path);
                realm.register_owner(OWNER, PEER).unwrap();
                factory(&realm);
                let permitted = Rc::new(Cell::new(true));
                let flag = permitted.clone();
                let (send, recv) = oneshot::channel();
                let application: harness::Client = capnp_rpc::new_client(Streaming {
                    gate: RefCell::new(Some(recv)),
                    permitted,
                    deny,
                });
                let cap = realm
                    .persistent(application, descriptor(), move |_| {
                        if flag.get() {
                            Ok(())
                        } else {
                            Err(capnp::Error::failed("revoked by stream".into()))
                        }
                    })
                    .unwrap();
                let mut stream = cap.stream_request().send();
                assert!(futures::poll!(&mut stream).is_pending());
                let persistent = Persistent::new(cap.as_client_hook().add_ref());
                let mut request = persistent.save_request();
                request.get().init_seal_for().set_id(&OWNER);
                let mut save = request.send().promise;
                let before = std::fs::metadata(&path).unwrap().len();
                assert!(futures::poll!(&mut save).is_pending());
                drop(save); // Dispatched standard save stays owned by its executor.
                for _ in 0..8 {
                    tokio::task::yield_now().await;
                }
                assert_eq!(std::fs::metadata(&path).unwrap().len(), before);
                send.send(()).unwrap();
                stream.await.unwrap();
                for _ in 0..8 {
                    tokio::task::yield_now().await;
                }
                assert_eq!(std::fs::metadata(&path).unwrap().len() > before, !deny);
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_persistent_save_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_PERSISTENT_SAVE_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    struct Streaming {
        gate: RefCell<Option<oneshot::Receiver<()>>>,
        permitted: Rc<Cell<bool>>,
        deny: bool,
    }
    impl harness::Server for Streaming {
        async fn stream(self: Rc<Self>, _: harness::StreamParams) -> capnp::Result<()> {
            let gate = self.gate.borrow_mut().take().unwrap();
            gate.await.unwrap();
            self.permitted.set(!self.deny);
            Ok(())
        }
    }
    for (index, case) in cases.iter().enumerate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("realm");
        let (executor, mut driver) = capnp_rpc::new_call_executor();
        let realm = Realm::open_with_executor(&path, Limits::default(), executor).unwrap();
        realm.register_owner(OWNER, PEER).unwrap();
        factory(&realm);
        let permitted = Rc::new(Cell::new(true));
        let flag = permitted.clone();
        let checks = Rc::new(Cell::new(0));
        let checked = checks.clone();
        let (send, recv) = oneshot::channel();
        let mut send = Some(send);
        let application: harness::Client = capnp_rpc::new_client(Streaming {
            gate: RefCell::new(Some(recv)),
            permitted,
            deny: case["deny"].as_bool().unwrap(),
        });
        let cap = realm
            .persistent(application, descriptor(), move |_| {
                checked.set(checked.get() + 1);
                if flag.get() {
                    Ok(())
                } else {
                    Err(capnp::Error::failed("revoked by stream".into()))
                }
            })
            .unwrap();
        let mut stream = Some(cap.stream_request().send());
        assert!(futures::poll!(stream.as_mut().unwrap()).is_pending());
        let mut save = None;
        let before = std::fs::metadata(&path).unwrap().len();
        for step in case["steps"].as_array().unwrap() {
            let state: Vec<u64> = step["state"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap())
                .collect();
            match step["action"].as_str().unwrap() {
                "start" => {
                    let persistent = Persistent::new(cap.as_client_hook().add_ref());
                    let mut request = persistent.save_request();
                    request.get().init_seal_for().set_id(&OWNER);
                    let mut promise = request.send().promise;
                    match futures::poll!(&mut promise) {
                        std::task::Poll::Pending => {
                            assert_eq!(state[2], 1);
                            save = Some(promise);
                        }
                        std::task::Poll::Ready(result) => {
                            assert_eq!(state[2], 2);
                            assert_eq!(result.is_ok(), state[5] == 1);
                        }
                    }
                }
                "stream" => {
                    send.take().unwrap().send(()).unwrap();
                    stream.take().unwrap().await.unwrap();
                }
                "drop" => {
                    save.take();
                }
                "close" => realm.close(),
                "finish" => {
                    // Drive only the explicitly owned protected-call task.
                    let _ = futures::poll!(&mut driver);
                    if let Some(promise) = save.take() {
                        assert_eq!(
                            promise.await.is_ok(),
                            state[5] == 1,
                            "trace {index}: {step}"
                        );
                    }
                }
                _ => panic!(),
            }
            assert_eq!(
                std::fs::metadata(&path).unwrap().len() > before,
                state[5] == 1,
                "trace {index}: {step}"
            );
            assert_eq!(
                checks.get(),
                usize::from(state[2] == 2),
                "trace {index}: {step}"
            );
        }
    }
}

#[test]
fn descriptor_decoding_enforces_public_rights_invariant() {
    for value in [
        serde_json::json!({"kind":1,"object":42,"generation":1,"rights":255}),
        serde_json::json!({"kind":0,"object":42,"generation":1,"rights":31}),
    ] {
        assert!(serde_json::from_value::<Descriptor>(value).is_err());
    }
    let d: Descriptor = serde_json::from_value(
        serde_json::json!({"kind":1,"object":42,"generation":1,"rights":17}),
    )
    .unwrap();
    assert!(d.rights().contains(Rights::GET));
    assert!(!d.rights().contains(Rights::PUT));
}

fn token_serial(reference: &SturdyRef) -> u64 {
    let mut message = capnp::message::Builder::new_default();
    reference.write(message.init_root());
    let token = message
        .get_root_as_reader::<wire::sturdy_ref::Reader>()
        .unwrap()
        .get_token()
        .unwrap();
    u64::from_be_bytes(token[..8].try_into().unwrap())
}
fn ledger(path: &std::path::Path) -> serde_json::Value {
    let store = Store::open(path).unwrap();
    let id = store.objects().next().unwrap();
    serde_json::from_slice(store.revision(id, store.head(id)).unwrap().bytes()).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn expiration_deletion_and_collection_preserve_live_caps_and_never_reuse_authority() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("realm");
            let limits = Limits {
                owners: 1,
                references: 2,
            };
            let realm = open_limits(&path, limits);
            factory(&realm);
            assert_eq!(realm.register_owner(OWNER, PEER).unwrap(), 1);
            realm.advance_clock(10).unwrap();
            let original = realm
                .persistent_until(client(), descriptor(), 20, |_| Ok(()))
                .unwrap();
            let reference = save(&original, OWNER).await.unwrap();
            let live = realm.restore(reference.clone(), PEER).await.unwrap();
            let child = save(&live, OWNER).await.unwrap();
            assert_eq!(token_serial(&reference), 1);
            assert_eq!(token_serial(&child), 2);
            assert!(save(&original, OWNER).await.is_err());
            let before = std::fs::read(&path).unwrap();
            assert!(realm.advance_clock(9).is_err());
            assert!(realm.expire_at(&reference, 21).is_err());
            assert!(realm.delete_owner(OWNER, 2).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
            realm.expire_at(&reference, 15).unwrap();
            assert_eq!(realm.advance_clock(15).unwrap(), 1);
            assert!(realm.restore(reference.clone(), PEER).await.is_err());
            assert!(save(&live, OWNER).await.is_err());
            echo(&live).await;
            echo(&realm.restore(child.clone(), PEER).await.unwrap()).await;
            assert_eq!(realm.advance_clock(20).unwrap(), 1);
            assert!(realm.restore(child.clone(), PEER).await.is_err());
            assert!(save(&original, OWNER).await.is_err());
            // Revoked references can be collected without advancing time.
            let fresh = save(&bound(&realm), OWNER).await.unwrap();
            assert_eq!(token_serial(&fresh), 3);
            realm.revoke(&fresh).unwrap();
            assert_eq!(realm.advance_clock(20).unwrap(), 1);
            let deleted = save(&bound(&realm), OWNER).await.unwrap();
            assert_eq!(realm.delete_owner(OWNER, 1).unwrap(), 1);
            assert_eq!(realm.register_owner(OWNER, PEER).unwrap(), 2);
            assert!(realm.rotate_owner(OWNER, 1, [2; 32]).is_err());
            assert!(realm.delete_owner(OWNER, 1).is_err());
            assert!(realm.restore(deleted.clone(), PEER).await.is_err());
            realm.close();
            let realm = open_limits(&path, limits);
            factory(&realm);
            assert_eq!(realm.clock(), 20);
            assert!(realm.advance_clock(19).is_err());
            let replacement = save(&bound(&realm), OWNER).await.unwrap();
            assert_eq!(token_serial(&replacement), 5);
            for old in [reference, child, fresh, deleted] {
                assert_ne!(old, replacement);
                assert!(realm.restore(old, PEER).await.is_err());
            }
            assert_eq!(realm.rotate_owner(OWNER, 2, [2; 32]).unwrap(), 3);
            echo(&realm.restore(replacement, [2; 32]).await.unwrap()).await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn reject_obsolete_ledgers_and_invalid_lifecycle_metadata() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("realm");
            let realm = open(&path);
            realm.register_owner(OWNER, PEER).unwrap();
            factory(&realm);
            let reference = save(&bound(&realm), OWNER).await.unwrap();
            realm.close();
            let mut old = ledger(&path);
            old["format"] = "capntproto-realm/1".into();
            for name in ["clock", "last_epoch", "last_token"] {
                old.as_object_mut().unwrap().remove(name);
            }
            for r in old["references"].as_array_mut().unwrap() {
                r.as_object_mut().unwrap().remove("serial");
                r.as_object_mut().unwrap().remove("expires_at");
            }
            let legacy = dir.path().join("legacy");
            {
                let mut s = Store::open(&legacy).unwrap();
                s.put(
                    ObjectKey::new(0x52505245414c4d01),
                    Revision::INITIAL,
                    &serde_json::to_vec(&old).unwrap(),
                )
                .unwrap();
            }
            let before = std::fs::read(&legacy).unwrap();
            assert!(Realm::open(&legacy, Limits::default()).is_err());
            assert_eq!(std::fs::read(&legacy).unwrap(), before);
            let valid = ledger(&path);
            for fault in [
                "missingClock",
                "missingExpiry",
                "zeroSerial",
                "epochFloor",
                "serialFloor",
                "duplicateSerial",
                "legacyNewFields",
            ] {
                let mut invalid = valid.clone();
                match fault {
                    "missingClock" => {
                        invalid.as_object_mut().unwrap().remove("clock");
                    }
                    "missingExpiry" => {
                        invalid["references"][0]
                            .as_object_mut()
                            .unwrap()
                            .remove("expires_at");
                    }
                    "zeroSerial" => invalid["references"][0]["serial"] = 0.into(),
                    "epochFloor" => invalid["last_epoch"] = 0.into(),
                    "serialFloor" => invalid["last_token"] = 0.into(),
                    "duplicateSerial" => {
                        let mut r = invalid["references"][0].clone();
                        r["hash"] = serde_json::json!(vec![0u8; 32]);
                        invalid["references"].as_array_mut().unwrap().push(r);
                    }
                    "legacyNewFields" => invalid["format"] = "capntproto-realm/1".into(),
                    _ => unreachable!(),
                }
                let trial = dir.path().join(fault);
                {
                    let mut s = Store::open(&trial).unwrap();
                    s.put(
                        ObjectKey::new(0x52505245414c4d01),
                        Revision::INITIAL,
                        &serde_json::to_vec(&invalid).unwrap(),
                    )
                    .unwrap();
                }
                let bytes = std::fs::read(&trial).unwrap();
                assert!(Realm::open(&trial, Limits::default()).is_err(), "{fault}");
                assert_eq!(std::fs::read(&trial).unwrap(), bytes);
            }
            for counter in ["last_epoch", "last_token"] {
                let mut exhausted = valid.clone();
                exhausted[counter] = u64::MAX.into();
                let trial = dir.path().join(counter);
                {
                    let mut s = Store::open(&trial).unwrap();
                    s.put(
                        ObjectKey::new(0x52505245414c4d01),
                        Revision::INITIAL,
                        &serde_json::to_vec(&exhausted).unwrap(),
                    )
                    .unwrap();
                }
                let r = open(&trial);
                factory(&r);
                let before = std::fs::read(&trial).unwrap();
                if counter == "last_epoch" {
                    assert!(r.register_owner([2; 16], [2; 32]).is_err());
                    assert!(r.rotate_owner(OWNER, 1, [2; 32]).is_err());
                } else {
                    assert!(save(&bound(&r), OWNER).await.is_err());
                }
                assert_eq!(std::fs::read(&trial).unwrap(), before);
                r.expire_at(&reference, u64::MAX).unwrap();
                assert_eq!(r.advance_clock(u64::MAX).unwrap(), 1);
                assert!(r.restore(reference.clone(), PEER).await.is_err());
                assert!(r
                    .persistent_until(client(), descriptor(), u64::MAX, |_| Ok(()))
                    .is_err());
                assert_eq!(r.advance_clock(u64::MAX).unwrap(), 0);
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn lifecycle_commits_recover_atomically_at_every_truncated_byte() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("realm");
            let realm = open(&path);
            realm.register_owner(OWNER, PEER).unwrap();
            factory(&realm);
            let reference = save(&bound(&realm), OWNER).await.unwrap();
            let initial = std::fs::read(&path).unwrap();
            realm.expire_at(&reference, 2).unwrap();
            let deadline = std::fs::read(&path).unwrap();
            realm.advance_clock(2).unwrap();
            let expired = std::fs::read(&path).unwrap();
            let second = save(&bound(&realm), OWNER).await.unwrap();
            let saved = std::fs::read(&path).unwrap();
            realm.delete_owner(OWNER, 1).unwrap();
            let deleted = std::fs::read(&path).unwrap();
            assert_eq!(realm.register_owner(OWNER, PEER).unwrap(), 2);
            let recreated = std::fs::read(&path).unwrap();
            realm.close();
            for (old, new, clock, owners, refs, expiry, epoch) in [
                (&initial, &deadline, 0, 1, 1, Some(2), 1),
                (&deadline, &expired, 2, 1, 0, None, 1),
                (&expired, &saved, 2, 1, 1, None, 1),
                (&saved, &deleted, 2, 0, 0, None, 1),
                (&deleted, &recreated, 2, 1, 0, None, 2),
            ] {
                let old_path = dir.path().join("old");
                std::fs::write(&old_path, old).unwrap();
                let old_ledger = ledger(&old_path);
                for cut in old.len()..=new.len() {
                    let trial = dir.path().join("cut");
                    std::fs::write(&trial, &new[..cut]).unwrap();
                    let r = open(&trial);
                    r.close();
                    let recovered = ledger(&trial);
                    if cut < new.len() {
                        assert_eq!(recovered, old_ledger);
                    } else {
                        assert_eq!(recovered["clock"], clock);
                        assert_eq!(recovered["owners"].as_array().unwrap().len(), owners);
                        assert_eq!(recovered["references"].as_array().unwrap().len(), refs);
                        assert_eq!(recovered["last_epoch"], epoch);
                        if refs != 0 {
                            assert_eq!(
                                recovered["references"][0]["expires_at"],
                                serde_json::json!(expiry)
                            );
                        }
                    }
                    let r = open(&trial);
                    factory(&r);
                    let valid = recovered["references"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|x| x["serial"] == token_serial(&second));
                    assert_eq!(r.restore(second.clone(), PEER).await.is_ok(), valid);
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_persistence_expiry_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_PERSISTENCE_EXPIRY_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!cases.is_empty());
    tokio::task::LocalSet::new()
        .run_until(async {
            for (index, case) in cases.iter().enumerate() {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("realm");
                let mut realm = open(&path);
                assert_eq!(realm.register_owner(OWNER, PEER).unwrap(), 1);
                let gate = Rc::new(RefCell::new(None));
                let tasks = Rc::new(Cell::new(0));
                let caps = Rc::new(Cell::new(0));
                gated_factory(&realm, &gate, &tasks, &caps);
                let original = realm
                    .persistent_until(client(), descriptor(), 2, |_| Ok(()))
                    .unwrap();
                let reference = save(&original, OWNER).await.unwrap();
                drop(original);
                let mut pending = None;
                let mut live = None;
                let mut child = None;
                let mut fresh = None;
                let mut previous = vec![0, 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 0];
                for step in case["steps"].as_array().unwrap() {
                    let s: Vec<u64> = step["state"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|x| x.as_u64().unwrap())
                        .collect();
                    match step["action"].as_str().unwrap() {
                        "compact" => {
                            realm.compact().unwrap();
                        }
                        "begin" => {
                            pending = Some(realm.restore(reference.clone(), PEER));
                            assert!(pending.as_mut().unwrap().now_or_never().is_none());
                        }
                        "finish" => {
                            gate.borrow_mut().take().unwrap().send(()).unwrap();
                            let result = pending.take().unwrap().await;
                            assert_eq!(result.is_ok(), s[4] == 2, "trace {index}: {step}");
                            live = result.ok();
                        }
                        "cancel" => {
                            drop(pending.take().unwrap());
                            assert!(gate.borrow_mut().take().unwrap().send(()).is_err());
                        }
                        "shorten" => realm.expire_at(&reference, 1).unwrap(),
                        "revoke" => realm.revoke(&reference).unwrap(),
                        "collect" => assert_eq!(realm.advance_clock(realm.clock()).unwrap(), 1),
                        "tick" => {
                            let expected =
                                (previous[3] - s[3]) + u64::from(previous[9] == 1 && s[9] == 2);
                            assert_eq!(
                                realm.advance_clock(s[0]).unwrap() as u64,
                                expected,
                                "trace {index}: {step}"
                            );
                        }
                        "delete" => {
                            assert_eq!(
                                realm.delete_owner(OWNER, 1).unwrap() as u64,
                                previous[3] + u64::from(previous[9] == 1)
                            );
                        }
                        "recreate" => assert_eq!(realm.register_owner(OWNER, PEER).unwrap(), s[2]),
                        "restart" => {
                            realm.close();
                            realm = open(&path);
                            gated_factory(&realm, &gate, &tasks, &caps);
                        }
                        "renew" => {
                            let result = save(live.as_ref().unwrap(), OWNER).await;
                            assert_eq!(result.is_ok(), s[9] == 1, "trace {index}: {step}");
                            child = result.ok();
                        }
                        "fresh" => fresh = Some(save(&bound(&realm), OWNER).await.unwrap()),
                        _ => panic!("unknown action"),
                    }
                    assert_eq!(realm.clock(), s[0]);
                    assert_eq!(pending.is_some(), s[4] == 1);
                    assert_eq!(live.is_some(), s[4] == 2);
                    assert_eq!(tasks.get(), usize::from(s[4] >= 2));
                    assert_eq!(caps.get(), usize::from(s[4] == 3));
                    if let Some(c) = &live {
                        echo(c).await;
                    }
                    if let Some(r) = &child {
                        assert_eq!(token_serial(r), s[13]);
                    }
                    if let Some(r) = &fresh {
                        assert_eq!(token_serial(r), s[11]);
                    }
                    previous = s;
                }
                // Compare the committed metadata at each graph edge endpoint, not
                // merely the temporary in-memory API results along its prefix.
                realm.close();
                if previous[19] == 1 {
                    assert_eq!(&std::fs::read(&path).unwrap()[..8], b"RPROTO04");
                }
                let l = ledger(&path);
                let s = previous;
                assert_eq!(l["clock"], s[0]);
                assert_eq!(l["last_token"], s[12]);
                assert_eq!(l["last_epoch"], if s[2] == 2 { 2 } else { 1 });
                let owners = l["owners"].as_array().unwrap();
                assert_eq!(owners.len(), usize::from(s[2] != 0));
                if !owners.is_empty() {
                    assert_eq!(owners[0]["epoch"], s[2]);
                }
                let refs = l["references"].as_array().unwrap();
                assert_eq!(
                    refs.len(),
                    s[3] as usize + usize::from(s[9] == 1) + usize::from(s[11] != 0)
                );
                for r in refs {
                    let id = r["serial"].as_u64().unwrap();
                    let expected = if id == 1 {
                        assert_eq!(s[3], 1);
                        Some(s[1])
                    } else if id == s[13] {
                        assert_eq!(s[9], 1);
                        Some(s[10])
                    } else {
                        assert_eq!(id, s[11]);
                        None
                    };
                    assert_eq!(r["expires_at"], serde_json::json!(expected));
                    assert_eq!(r["revoked"], id == 1 && s[17] == 1);
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn configured_realm_compacts_authority_without_reissuing_or_rewinding() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("realm");
            let copy = dir.path().join("committed");
            let storage_limits = capntproto::storage::Limits {
                max_entry_bytes: 32 * 1024,
                max_file_bytes: 64 * 1024,
            };
            let store = Store::open_with_limits(&path, storage_limits).unwrap();
            let (executor, driver) = capnp_rpc::new_call_executor();
            tokio::task::spawn_local(driver);
            let realm = Realm::from_store(store, Limits::default(), Some(executor)).unwrap();
            realm.register_owner(OWNER, PEER).unwrap();
            factory(&realm);
            let cap = bound(&realm);
            let live_ref = save(&cap, OWNER).await.unwrap();
            let revoked = save(&cap, OWNER).await.unwrap();
            realm.revoke(&revoked).unwrap();
            let expired = save(&cap, OWNER).await.unwrap();
            realm.expire_at(&expired, 10).unwrap();
            for now in 1..=10 {
                realm.advance_clock(now).unwrap();
            }
            let dead_owner = [9; 16];
            assert_eq!(realm.register_owner(dead_owner, [9; 32]).unwrap(), 2);
            realm.delete_owner(dead_owner, 2).unwrap();
            let live = realm.restore(live_ref.clone(), PEER).await.unwrap();
            let pending = realm.restore(live_ref.clone(), PEER);
            std::fs::copy(&path, &copy).unwrap();
            let before = ledger(&copy);
            let result = realm.compact().unwrap();
            assert!(result.after_bytes < result.before_bytes);
            assert!(result.removed_revisions > 10);
            std::fs::copy(&path, &copy).unwrap();
            assert_eq!(ledger(&copy), before);
            echo(&pending.await.unwrap()).await;
            echo(&live).await;
            assert!(realm.restore(revoked.clone(), PEER).await.is_err());
            assert!(realm.restore(expired.clone(), PEER).await.is_err());
            assert!(realm
                .set_storage_limits(capntproto::storage::Limits {
                    max_file_bytes: 64,
                    ..storage_limits
                })
                .is_err());
            realm
                .set_storage_limits(capntproto::storage::Limits {
                    max_file_bytes: 128 * 1024,
                    ..storage_limits
                })
                .unwrap();
            let renewed = save(&live, OWNER).await.unwrap();
            assert_eq!(token_serial(&renewed), 4);
            realm.close();
            let reopened = open(&path);
            factory(&reopened);
            assert_eq!(reopened.clock(), 10);
            assert_eq!(reopened.register_owner(dead_owner, [9; 32]).unwrap(), 3);
            echo(&reopened.restore(renewed, PEER).await.unwrap()).await;
            assert!(reopened.restore(revoked, PEER).await.is_err());
            assert!(reopened.restore(expired, PEER).await.is_err());
            reopened.compact().unwrap();
        })
        .await;
}
