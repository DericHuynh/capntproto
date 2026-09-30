use capnp::capability::FromClientHook;
use reproto::{
    authority::{Grant, ObjectGeneration, ObjectId, Rights},
    orm::{ObjectServer, ObjectState},
    persistence::{
        Descriptor, Factory, Limits, ObjectFactory, ObjectKind, Persistent, Realm, SturdyRef,
    },
    storage::Store,
    store_capnp::{document, object},
};
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc};

#[test]
fn descriptor_numeric_format_and_all_rights_roundtrip_without_losing_bindings() {
    for value in [1, 1 << 32, u64::MAX] {
        let (kind, object, generation) = (
            ObjectKind::new(value).unwrap(),
            ObjectId::new(value).unwrap(),
            ObjectGeneration::new(value).unwrap(),
        );
        assert_eq!(kind.get(), value);
        assert_eq!(object.get(), value);
        assert_eq!(generation.get(), value);
        assert_eq!(ObjectKind::try_from(value).unwrap(), kind);
        assert_eq!(ObjectId::try_from(value).unwrap(), object);
        assert_eq!(ObjectGeneration::try_from(value).unwrap(), generation);
        assert_eq!(kind.to_string(), value.to_string());
        for bits in 0..=31 {
            let rights = Rights::from_bits(bits).unwrap();
            let descriptor = Descriptor::new(kind, object, generation, rights);
            let expected = json!({"kind":value, "object":value, "generation":value, "rights":bits});
            assert_eq!(serde_json::to_value(&descriptor).unwrap(), expected);
            let decoded: Descriptor = serde_json::from_value(expected).unwrap();
            assert_eq!(decoded, descriptor.clone());
            assert_eq!(
                (
                    decoded.kind(),
                    decoded.object(),
                    decoded.generation(),
                    decoded.rights()
                ),
                (kind, object, generation, rights)
            );
        }
    }
    // Distinct values catch accidental field swaps that equal-number fixtures miss.
    let bytes = r#"{"kind":7,"object":19,"generation":23,"rights":9}"#;
    let descriptor: Descriptor = serde_json::from_str(bytes).unwrap();
    assert_eq!(
        (
            descriptor.kind().get(),
            descriptor.object().get(),
            descriptor.generation().get(),
            descriptor.rights()
        ),
        (7, 19, 23, Rights::VIEW)
    );
    assert_eq!(serde_json::to_string(&descriptor).unwrap(), bytes);
}

#[test]
fn decoding_cannot_bypass_nonzero_ids_rights_or_exact_field_set() {
    assert!(ObjectKind::new(0).is_none());
    assert!(ObjectId::new(0).is_none());
    assert!(ObjectGeneration::new(0).is_none());
    assert!(ObjectKind::try_from(0).is_err());
    assert!(ObjectId::try_from(0).is_err());
    assert!(ObjectGeneration::try_from(0).is_err());
    for invalid in [
        "0",
        "-1",
        "1.0",
        "18446744073709551616",
        "null",
        "\"1\"",
        "true",
        "[]",
        "{}",
    ] {
        assert!(serde_json::from_str::<ObjectKind>(invalid).is_err());
        assert!(serde_json::from_str::<ObjectId>(invalid).is_err());
        assert!(serde_json::from_str::<ObjectGeneration>(invalid).is_err());
    }
    let valid = json!({"kind":1, "object":2, "generation":3, "rights":31});
    for field in ["kind", "object", "generation"] {
        for invalid in [json!(0), json!(-1), json!(1.0), Value::Null, json!("1")] {
            let mut value = valid.clone();
            value[field] = invalid;
            assert!(serde_json::from_value::<Descriptor>(value).is_err());
        }
    }
    for field in ["kind", "object", "generation", "rights"] {
        let mut value = valid.clone();
        value.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<Descriptor>(value).is_err());
        let duplicate = format!("{{\"{field}\":1,{}", &valid.to_string()[1..]);
        assert!(serde_json::from_str::<Descriptor>(&duplicate).is_err());
    }
    for rights in 32..=255 {
        let mut value = valid.clone();
        value["rights"] = json!(rights);
        assert!(serde_json::from_value::<Descriptor>(value).is_err());
    }
    let mut unknown = valid;
    unknown["extra"] = json!(1);
    assert!(serde_json::from_value::<Descriptor>(unknown).is_err());
}

const OWNER: [u8; 16] = [1; 16];
const PEER: [u8; 32] = [1; 32];
type Doc = object::Client<document::Owned>;

#[test]
fn persistent_factory_state_has_stable_typed_binding() {
    let dir = tempfile::tempdir().unwrap();
    let store = Rc::new(RefCell::new(
        Store::open(dir.path().join("objects")).unwrap(),
    ));
    let factory =
        ObjectFactory::<document::Owned>::new(store.clone(), ObjectGeneration::new(23).unwrap());
    let object = ObjectId::new(19).unwrap();
    let state = factory.state(object);
    assert_eq!(state.object(), object);
    assert!(Rc::ptr_eq(state.store(), &store));
    assert!(Rc::ptr_eq(&factory.state(object), &state));
    assert!(!Rc::ptr_eq(
        &factory.state(ObjectId::new(20).unwrap()),
        &state
    ));
}

fn facet(store: &Rc<RefCell<Store>>, object: u64, generation: u64, rights: Rights) -> Doc {
    ObjectServer::<document::Owned>::client(
        ObjectState::new(store.clone(), ObjectId::new(object).unwrap()),
        Grant::root(
            ObjectId::new(object).unwrap(),
            ObjectGeneration::new(generation).unwrap(),
            PEER,
            rights,
        ),
    )
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_persistent_factory_bindings() {
    use reproto_test_support::verification::exploration;
    const MODEL: &str = "verification/PersistentFactoryBinding.tla";
    const CONFIG: &str = include_str!("../verification/PersistentFactoryBinding.cfg");
    let paths = exploration::traces(MODEL, "persistent-factory-binding", CONFIG).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for path in &paths {
                let dir = tempfile::tempdir().unwrap();
                let store = Rc::new(RefCell::new(
                    Store::open(dir.path().join("objects")).unwrap(),
                ));
                for object in 1..=2 {
                    let cap = facet(&store, object, 1, Rights::ALL);
                    let mut put = cap.put_request();
                    put.get()
                        .init_value()
                        .set_text(format!("object-{object}").as_str());
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
                }
                let (executor, driver) = capnp_rpc::new_call_executor();
                let task = tokio::task::spawn_local(driver);
                let realm = Realm::open_with_executor(
                    dir.path().join("realm"),
                    Limits::default(),
                    executor,
                )
                .unwrap();
                realm.register_owner(OWNER, PEER).unwrap();
                let calls = Rc::new(RefCell::new(Vec::new()));
                let mut registered = 0;
                let (mut stage, mut kind, mut object, mut generation, mut rights) = (0, 0, 0, 0, 0);
                let (mut outcome, mut observed, mut may_write) = (0, 0, 0);
                let mut reference = None;
                for state in path {
                    match state["event"] {
                        k @ 1..=2 => {
                            let factory = ObjectFactory::<document::Owned>::new(
                                store.clone(),
                                ObjectGeneration::new(k).unwrap(),
                            );
                            let calls = calls.clone();
                            realm
                                .register_factory(
                                    ObjectKind::new(k).unwrap(),
                                    Rc::new(move |d: &Descriptor, peer| {
                                        calls.borrow_mut().push((k, d.clone()));
                                        factory.restore(d, peer)
                                    }),
                                )
                                .unwrap();
                            registered |= 1 << (k - 1);
                        }
                        3 => {
                            // Model-selected operation arguments; outcomes below come
                            // only from the real bind/save/restore and ORM operations.
                            (kind, object, generation, rights) = (
                                state["kind"],
                                state["object"],
                                state["generation"],
                                state["rights"],
                            );
                            // Saving requires DELEGATE in both cases; only
                            // the full-rights descriptor permits PUT.
                            let rights_value = if rights == 1 {
                                Rights::ALL
                            } else {
                                Rights::from_bits(Rights::VIEW.bits() | Rights::DELEGATE.bits())
                                    .unwrap()
                            };
                            let descriptor = Descriptor::new(
                                ObjectKind::new(kind).unwrap(),
                                ObjectId::new(object).unwrap(),
                                ObjectGeneration::new(generation).unwrap(),
                                rights_value,
                            );
                            match realm.persistent(
                                facet(&store, object, generation, rights_value),
                                descriptor,
                                |_| Ok(()),
                            ) {
                                Ok(cap) => {
                                    let persistent =
                                        Persistent::new(cap.as_client_hook().add_ref());
                                    let mut save = persistent.save_request();
                                    save.get().init_seal_for().set_id(&OWNER);
                                    let response = save.send().promise.await.unwrap();
                                    reference = Some(
                                        SturdyRef::read(
                                            response.get().unwrap().get_sturdy_ref().unwrap(),
                                        )
                                        .unwrap(),
                                    );
                                    stage = 1;
                                }
                                Err(_) => stage = 3,
                            }
                            assert!(calls.borrow().is_empty());
                        }
                        4 => {
                            stage = 2;
                            match realm.restore(reference.clone().unwrap(), PEER).await {
                                Ok(client) => {
                                    outcome = 1;
                                    let cap: Doc = client.cast_to();
                                    let response = cap.get_request().send().promise.await.unwrap();
                                    let value = response.get().unwrap().get_value().unwrap();
                                    observed = value
                                        .get_text()
                                        .unwrap()
                                        .to_str()
                                        .unwrap()
                                        .strip_prefix("object-")
                                        .unwrap()
                                        .parse()
                                        .unwrap();
                                    let mut put = cap.put_request();
                                    put.get().set_expected_head(1);
                                    put.get().init_value().set_text("updated");
                                    may_write = u64::from(put.send().promise.await.is_ok());
                                }
                                Err(_) => outcome = 2,
                            }
                            let calls = calls.borrow();
                            assert_eq!(calls.len(), 1);
                            assert_eq!(calls[0].0, kind);
                            assert_eq!(
                                (
                                    calls[0].1.kind().get(),
                                    calls[0].1.object().get(),
                                    calls[0].1.generation().get()
                                ),
                                (kind, object, generation)
                            );
                            assert_eq!(calls[0].1.rights().contains(Rights::PUT), rights == 1);
                        }
                        5 => (),
                        _ => panic!("invalid event: {state:?}"),
                    }
                    for (field, actual) in [
                        ("registered", registered),
                        ("stage", stage),
                        ("kind", kind),
                        ("object", object),
                        ("generation", generation),
                        ("rights", rights),
                        ("outcome", outcome),
                        ("observedObject", observed),
                        ("mayWrite", may_write),
                    ] {
                        assert_eq!(actual, state[field], "{field}: {path:?}");
                    }
                }
                realm.close();
                task.abort();
                let _ = task.await;
            }
        })
        .await;
    exploration::controls(
        MODEL,
        "persistent-factory-binding",
        CONFIG,
        &[
            ("registration", "RegisteredBinding"),
            ("dispatch", "RestorationContract"),
            ("generation", "RestorationContract"),
            ("object", "ExactObject"),
            ("rights", "ExactRights"),
        ],
        None,
    )
    .unwrap();
    eprintln!("{} persistent factory edge-prefix replays", paths.len());
}
