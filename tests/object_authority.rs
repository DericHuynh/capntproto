use capntproto::storage::ObjectKey;
use capntproto::storage::Revision;
use capntproto::{
    authority::{Grant, ObjectGeneration, ObjectId, Rights},
    orm::{ObjectServer, ObjectState},
    storage::Store,
    store_capnp::{document, object},
};
use std::{cell::RefCell, rc::Rc};

type Doc = object::Client<document::Owned>;

#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_object_authority_bindings() {
    use capntproto_test_support::verification::exploration;
    const MODEL: &str = "verification/ObjectAuthorityBinding.tla";
    const CONFIG: &str = include_str!("../verification/ObjectAuthorityBinding.cfg");
    let paths = exploration::traces(MODEL, "object-authority-binding", CONFIG).unwrap();
    for path in &paths {
        let dir = tempfile::tempdir().unwrap();
        let store = Rc::new(RefCell::new(
            Store::open(dir.path().join("objects")).unwrap(),
        ));
        let states: Vec<_> = (1..=2)
            .map(|id| ObjectState::new(store.clone(), ObjectId::new(id).unwrap()))
            .collect();
        for state in &states {
            let cap = ObjectServer::<document::Owned>::client(
                state.clone(),
                Grant::root(
                    state.object(),
                    ObjectGeneration::new(1).unwrap(),
                    [1; 32],
                    Rights::ALL,
                ),
            )
            .unwrap();
            let mut put = cap.put_request();
            put.get()
                .init_value()
                .set_text(state.object().to_string().as_str());
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
        let (mut host_object, mut host_generation, mut requested, mut target) = (0, 0, 0, 0);
        let (mut stage, mut read, mut write, mut used_live) = (0, 0, 0, 0);
        let mut root: Option<Grant> = None;
        let mut child: Option<Grant> = None;
        let mut cap: Option<Doc> = None;
        for state in path {
            match state["event"] {
                1 => {
                    (host_object, host_generation) = (state["hostObject"], state["hostGeneration"]);
                    root = Some(Grant::root(
                        ObjectId::new(host_object).unwrap(),
                        ObjectGeneration::new(host_generation).unwrap(),
                        [1; 32],
                        Rights::ALL,
                    ));
                    stage = 1;
                }
                2 => {
                    requested = state["requested"];
                    child = Some(
                        root.as_ref()
                            .unwrap()
                            .delegate(
                                [2; 32],
                                if requested == 1 {
                                    Rights::ALL
                                } else {
                                    Rights::VIEW
                                },
                            )
                            .unwrap(),
                    );
                    stage = 2;
                }
                3 => {
                    target = state["target"];
                    match ObjectServer::<document::Owned>::client(
                        states[target as usize - 1].clone(),
                        child.as_ref().unwrap().clone(),
                    ) {
                        Ok(client) => {
                            cap = Some(client);
                            stage = 3;
                        }
                        Err(_) => stage = 4,
                    }
                }
                4 => {
                    stage = 5;
                    used_live = u64::from(child.as_ref().unwrap().is_live());
                    let client = cap.as_ref().unwrap();
                    read = match client.get_request().send().promise.await {
                        Ok(response) => response
                            .get()
                            .unwrap()
                            .get_value()
                            .unwrap()
                            .get_text()
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .parse()
                            .unwrap(),
                        Err(_) => 0,
                    };
                    let mut put = client.put_request();
                    put.get().set_expected_head(1);
                    put.get().init_value().set_text("written");
                    write = if put.send().promise.await.is_ok() {
                        target
                    } else {
                        0
                    };
                }
                5 => root.as_ref().unwrap().revoke(),
                6 => (),
                _ => panic!("unknown event: {state:?}"),
            }
            let grant = child.as_ref().or(root.as_ref()).unwrap();
            for (field, actual) in [
                ("stage", stage),
                ("hostObject", host_object),
                ("hostGeneration", host_generation),
                ("object", grant.object().get()),
                ("generation", grant.generation().get()),
                ("requested", requested),
                ("rights", u64::from(grant.rights().contains(Rights::PUT))),
                ("live", u64::from(grant.is_live())),
                ("target", target),
                ("read", read),
                ("write", write),
                ("usedLive", used_live),
            ] {
                assert_eq!(actual, state[field], "{field}: {path:?}");
            }
            assert_eq!(
                grant.holder(),
                if child.is_some() { [2; 32] } else { [1; 32] }
            );
            assert_eq!(
                grant.rights(),
                if child.is_none() || requested == 1 {
                    Rights::ALL
                } else {
                    Rights::VIEW
                }
            );
            for (index, object_state) in states.iter().enumerate() {
                let id = index as u64 + 1;
                assert_eq!(object_state.object().get(), id);
                assert!(Rc::ptr_eq(object_state.store(), &store));
                assert_eq!(
                    store.borrow().head(ObjectKey::new(id)),
                    Revision::new(1 + u64::from(write == id))
                );
                assert_eq!(
                    store.borrow().published(ObjectKey::new(id)),
                    Revision::new(1)
                );
            }
        }
    }
    exploration::controls(
        MODEL,
        "object-authority-binding",
        CONFIG,
        &[
            ("object", "GrantBindings"),
            ("generation", "GrantBindings"),
            ("rights", "Attenuation"),
            ("bind", "Binding"),
            ("revocation", "Invocation"),
        ],
        None,
    )
    .unwrap();
    eprintln!("{} object authority edge-prefix replays", paths.len());
}

#[test]
fn full_width_bindings_survive_delegation_and_serialization() {
    // Object IDs and generations stay distinct even when their numbers overlap.
    for (object, generation) in [(1, u64::MAX), (u64::MAX, 1), (u64::MAX, u64::MAX)] {
        let object = ObjectId::try_from(object).unwrap();
        let generation = ObjectGeneration::try_from(generation).unwrap();
        let root = Grant::root(object, generation, [1; 32], Rights::ALL);
        let child = root.delegate([2; 32], Rights::VIEW).unwrap();
        let clone = child.clone();
        assert_eq!((clone.object(), clone.generation()), (object, generation));
        let descriptor = capntproto::persistence::Descriptor::new(
            capntproto::persistence::ObjectKind::new(7).unwrap(),
            clone.object(),
            clone.generation(),
            clone.rights(),
        );
        let decoded: capntproto::persistence::Descriptor =
            serde_json::from_str(&serde_json::to_string(&descriptor).unwrap()).unwrap();
        assert_eq!(
            (decoded.object(), decoded.generation()),
            (object, generation)
        );
        root.revoke();
        assert!(!clone.is_live());
        assert_eq!((clone.object(), clone.generation()), (object, generation));
    }
}
