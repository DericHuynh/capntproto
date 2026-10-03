#![cfg(feature = "storage")]
use capnp_rpc::rpc_twoparty_capnp::Side;
use reproto::{
    authority::{Grant, ObjectGeneration, ObjectId, Rights},
    orm::components::{ComponentServer, ComponentState, Edit, TypedComponent},
    rpc::Connection,
    storage::{ComponentId, ComponentUpdate, ObjectKey, Retention, Revision, Store},
    store_capnp::{component, document},
};
use reproto_test_support::runtime_test_capnp::harness;
use std::{cell::RefCell, rc::Rc, time::Duration};

const HOT: ComponentId = ComponentId::new(1);
const COLD: ComponentId = ComponentId::new(2);
fn object() -> ObjectId {
    ObjectId::new(7).unwrap()
}
fn grant(rights: Rights) -> Grant {
    Grant::root(object(), ObjectGeneration::new(1).unwrap(), [0; 32], rights)
}
fn doc(id: ComponentId, text: &str) -> Edit {
    let mut message = capnp::message::Builder::new_default();
    message.init_root::<document::Builder<'_>>().set_text(text);
    Edit::set::<document::Owned>(id, message.get_root_as_reader().unwrap()).unwrap()
}
fn number(id: ComponentId) -> Edit {
    let mut message = capnp::message::Builder::new_default();
    message
        .init_root::<harness::value::Builder<'_>>()
        .set_value(42);
    Edit::set::<harness::value::Owned>(id, message.get_root_as_reader().unwrap()).unwrap()
}
fn text(value: &TypedComponent<document::Owned>) -> String {
    value
        .with_reader(|r| Ok(r.get_text()?.to_str()?.to_owned()))
        .unwrap()
}

#[test]
fn typed_multicomponent_edits_validate_schemas_and_hold_old_mappings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    let store = Rc::new(RefCell::new(Store::open_components(&path).unwrap()));
    let state = ComponentState::new(store.clone(), object()).unwrap();
    state
        .edit(
            Revision::INITIAL,
            Some(Revision::INITIAL),
            &[doc(HOT, "old"), number(COLD)],
        )
        .unwrap();
    let old = state.get::<document::Owned>(HOT).unwrap();
    assert_eq!(
        state
            .get::<harness::value::Owned>(COLD)
            .unwrap()
            .with_reader(|r| Ok(r.get_value()))
            .unwrap(),
        42
    );
    let before = store.borrow().file_bytes();
    assert!(state
        .edit(
            Revision::new(1),
            Some(Revision::new(1)),
            &[doc(HOT, "new"), doc(COLD, "wrong schema")]
        )
        .is_err());
    assert!(state.get::<document::Owned>(COLD).is_err());
    assert_eq!(store.borrow().file_bytes(), before);
    state
        .edit(Revision::new(1), Some(Revision::new(1)), &[doc(HOT, "new")])
        .unwrap();
    store.borrow_mut().compact(Retention::Latest).unwrap();
    assert_eq!(text(&old), "old");
    assert_eq!(text(&state.get(HOT).unwrap()), "new");
    drop(state);
    drop(store);
    assert!(Store::open_components(&path).is_err());
    drop(old);
    let store = Rc::new(RefCell::new(Store::open_components(&path).unwrap()));
    let state = ComponentState::new(store, object()).unwrap();
    assert!(state.edit(Revision::new(2), None, &[number(HOT)]).is_err());
    assert!(ComponentServer::<harness::value::Owned>::client(
        state.clone(),
        grant(Rights::ALL),
        HOT
    )
    .is_err());
    assert_eq!(text(&state.get(HOT).unwrap()), "new");
}

#[test]
fn facet_reservations_and_failed_edits_cannot_change_schemas() {
    let dir = tempfile::tempdir().unwrap();
    let store = Rc::new(RefCell::new(
        Store::open_components(dir.path().join("objects")).unwrap(),
    ));
    let state = ComponentState::new(store.clone(), object()).unwrap();
    let _facet =
        ComponentServer::<document::Owned>::client(state.clone(), grant(Rights::ALL), HOT).unwrap();
    assert!(ComponentServer::<harness::value::Owned>::client(
        state.clone(),
        grant(Rights::ALL),
        HOT
    )
    .is_err());
    assert!(state.edit(Revision::INITIAL, None, &[number(HOT)]).is_err());
    assert!(state
        .edit(Revision::new(99), None, &[number(COLD)])
        .is_err());
    // A rejected transaction must not reserve a type on another component.
    state
        .edit(
            Revision::INITIAL,
            Some(Revision::INITIAL),
            &[doc(HOT, "a"), doc(COLD, "b")],
        )
        .unwrap();
    assert_eq!(text(&state.get(COLD).unwrap()), "b");
    // Independent host states must still respect the current persisted type.
    let other = ComponentState::new(store, object()).unwrap();
    assert!(other.edit(Revision::new(1), None, &[number(HOT)]).is_err());
}

#[test]
fn rejects_live_capabilities_and_trailing_message_bytes() {
    struct Harness;
    impl harness::Server for Harness {}
    let mut message = capnp::message::Builder::new_default();
    let mut caps = capnp::private::layout::CapTable::new();
    let mut value = message.init_root::<harness::cap_cycle::Builder<'_>>();
    capnp::traits::ImbueMut::imbue_mut(&mut value, &mut caps);
    value.set_cap(capnp_rpc::new_client(Harness));
    assert!(Edit::set::<harness::cap_cycle::Owned>(HOT, value.into_reader()).is_err());

    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_components(dir.path().join("objects")).unwrap();
    let mut message = capnp::message::Builder::new_default();
    message.init_root::<document::Builder<'_>>().set_text("a");
    let mut bytes = <document::Reader<'_> as capnp::traits::HasTypeId>::TYPE_ID
        .to_le_bytes()
        .to_vec();
    bytes.extend(capnp::serialize::write_message_to_words(&message));
    bytes.extend_from_slice(&[0; 8]);
    store
        .edit_components(
            ObjectKey::from(object()),
            Revision::INITIAL,
            Some(Revision::INITIAL),
            &[ComponentUpdate {
                id: HOT,
                value: Some(&bytes),
            }],
        )
        .unwrap();
    let view = TypedComponent::<document::Owned>::new(
        store.get_components(ObjectKey::from(object())).unwrap(),
        HOT,
    )
    .unwrap();
    assert!(view.with_reader(|r| Ok(r.get_text()?.len())).is_err());
}

async fn commit(
    client: &component::Client<document::Owned>,
    head: u64,
    published: u64,
    value: &str,
) -> capnp::Result<u64> {
    let mut request = client.commit_request();
    request.get().set_expected_head(head);
    request.get().set_expected_published(published);
    request.get().init_value().set_text(value);
    Ok(request.send().promise.await?.get()?.get_revision())
}

#[tokio::test(flavor = "current_thread")]
async fn both_orm_layouts_reject_live_rpc_capabilities_without_panicking_or_writing() {
    use reproto::orm::{ObjectServer, ObjectState};
    struct Harness;
    impl harness::Server for Harness {}
    tokio::task::LocalSet::new()
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let whole = Rc::new(RefCell::new(Store::open(dir.path().join("whole")).unwrap()));
            let client = ObjectServer::<harness::cap_cycle::Owned>::client(
                ObjectState::new(whole.clone(), object()),
                grant(Rights::ALL),
            )
            .unwrap();
            let mut request = client.put_request();
            request
                .get()
                .init_value()
                .set_cap(capnp_rpc::new_client(Harness));
            let before = whole.borrow().file_bytes();
            assert!(request
                .send()
                .promise
                .await
                .err()
                .unwrap()
                .extra
                .contains("live connection capabilities"));
            assert_eq!(whole.borrow().file_bytes(), before);
            assert_eq!(
                whole.borrow().head(ObjectKey::from(object())),
                Revision::INITIAL
            );

            let parts = Rc::new(RefCell::new(
                Store::open_components(dir.path().join("parts")).unwrap(),
            ));
            let state = ComponentState::new(parts.clone(), object()).unwrap();
            let client = ComponentServer::<harness::cap_cycle::Owned>::client(
                state,
                grant(Rights::ALL),
                HOT,
            )
            .unwrap();
            let mut request = client.commit_request();
            request
                .get()
                .init_value()
                .set_cap(capnp_rpc::new_client(Harness));
            let before = parts.borrow().file_bytes();
            assert!(request
                .send()
                .promise
                .await
                .err()
                .unwrap()
                .extra
                .contains("live connection capabilities"));
            assert_eq!(parts.borrow().file_bytes(), before);
            assert_eq!(
                parts.borrow().head(ObjectKey::from(object())),
                Revision::INITIAL
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn tcp_component_facets_preserve_cas_permissions_revocation_and_cold_data() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let dir = tempfile::tempdir().unwrap();
                let store = Rc::new(RefCell::new(
                    Store::open_components(dir.path().join("objects")).unwrap(),
                ));
                let state = ComponentState::new(store.clone(), object()).unwrap();
                state
                    .edit(
                        Revision::INITIAL,
                        Some(Revision::INITIAL),
                        &[doc(HOT, "old"), doc(COLD, &"c".repeat(65536))],
                    )
                    .unwrap();
                let root = grant(Rights::ALL);
                let facet =
                    ComponentServer::<document::Owned>::client(state.clone(), root.clone(), HOT)
                        .unwrap();
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let (client, server) = tokio::join!(
                    tokio::net::TcpStream::connect(listener.local_addr().unwrap()),
                    listener.accept()
                );
                let a = Connection::new(client.unwrap(), None, Side::Client);
                let b = Connection::new(server.unwrap().0, Some(facet.client), Side::Server);
                let remote: component::Client<document::Owned> = a.bootstrap();
                let before = store.borrow().file_bytes();
                assert_eq!(commit(&remote, 1, 1, "new").await.unwrap(), 2);
                assert!(store.borrow().file_bytes() - before < 256);
                assert!(commit(&remote, 1, 1, "stale").await.is_err());
                let response = remote.get_request().send().promise.await.unwrap();
                assert_eq!(
                    response
                        .get()
                        .unwrap()
                        .get_value()
                        .unwrap()
                        .get_text()
                        .unwrap(),
                    "new"
                );
                assert_eq!(response.get().unwrap().get_revision(), 2);
                assert_eq!(text(&state.get(COLD).unwrap()).len(), 65536);
                let read_only = ComponentServer::<document::Owned>::client(
                    state.clone(),
                    grant(Rights::GET),
                    HOT,
                )
                .unwrap();
                assert!(commit(&read_only, 2, 2, "denied").await.is_err());
                let put_only = ComponentServer::<document::Owned>::client(
                    state.clone(),
                    grant(Rights::PUT),
                    HOT,
                )
                .unwrap();
                assert!(put_only.get_request().send().promise.await.is_err());
                assert!(commit(&put_only, 2, 2, "denied").await.is_err());
                let mut draft = put_only.put_request();
                draft.get().set_expected_head(2);
                draft.get().init_value().set_text("draft");
                assert_eq!(
                    draft
                        .send()
                        .promise
                        .await
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_revision(),
                    3
                );
                assert_eq!(text(&state.get(HOT).unwrap()), "new");
                assert_eq!(commit(&remote, 3, 2, "published").await.unwrap(), 4);
                root.revoke();
                assert!(remote.get_request().send().promise.await.is_err());
                assert!(commit(&remote, 4, 4, "revoked").await.is_err());
                assert_eq!(
                    store.borrow().head(ObjectKey::from(object())),
                    Revision::new(4)
                );
                a.shutdown(Duration::from_secs(1)).await.unwrap();
                b.on_disconnect().await.unwrap();
            })
            .await
            .unwrap();
        })
        .await;
}
