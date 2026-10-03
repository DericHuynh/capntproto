use reproto::{
    authority::{Grant, ObjectGeneration, ObjectId, Rights},
    orm::{ObjectServer, ObjectState},
    rpc,
    storage::Store,
    store_capnp::{document, object, observer},
    transport::{self, Identity},
};
use std::{cell::RefCell, rc::Rc, time::Duration};
use tokio::net::UdpSocket;
type Doc = object::Client<document::Owned>;
struct Observer(Rc<RefCell<Vec<(u64, String)>>>);
impl observer::Server<document::Owned> for Observer {
    async fn changed(
        self: Rc<Self>,
        p: observer::ChangedParams<document::Owned>,
        _: observer::ChangedResults<document::Owned>,
    ) -> capnp::Result<()> {
        let p = p.get()?;
        self.0.borrow_mut().push((
            p.get_revision(),
            p.get_value()?.get_text()?.to_str()?.into(),
        ));
        Ok(())
    }
}
async fn network(
    state: Rc<ObjectState>,
    grant: Grant,
    client: &Identity,
    server: &Identity,
    psk: Option<[u8; 32]>,
    context: &[u8],
) -> (Doc, Vec<tokio::task::AbortHandle>) {
    let a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let b = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let remote = b.local_addr().unwrap();
    let mut ac = transport::config(client, server.public_key(), psk, context).unwrap();
    let mut bc = transport::config(server, client.public_key(), psk, context).unwrap();
    let (a, at) = transport::connect(a, remote, &mut ac).await.unwrap();
    let (b, bt) = transport::accept(b, &mut bc).await.unwrap();
    let service = ObjectServer::<document::Owned>::client(state, grant).unwrap();
    let st = rpc::serve(b, service.client);
    let (client, ct) = rpc::client(a);
    (
        client,
        vec![
            at.abort_handle(),
            bt.abort_handle(),
            st.abort_handle(),
            ct.abort_handle(),
        ],
    )
}
#[tokio::test(flavor = "current_thread")]
async fn typed_orm_over_native_udp() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(10), async {
                let dir = tempfile::tempdir().unwrap();
                let store = Rc::new(RefCell::new(Store::open(dir.path().join("db")).unwrap()));
                let state = ObjectState::new(store, ObjectId::new(42).unwrap());
                let a = Identity::generate();
                let b = Identity::generate();
                let root = Grant::root(
                    ObjectId::new(42).unwrap(),
                    ObjectGeneration::new(1).unwrap(),
                    a.public_key(),
                    Rights::ALL,
                );
                let (client, tasks) =
                    network(state.clone(), root.clone(), &a, &b, None, b"orm-test").await;
                let observed = Rc::new(RefCell::new(vec![]));
                let observer: observer::Client<document::Owned> =
                    capnp_rpc::new_client(Observer(observed.clone()));
                let mut sub = client.subscribe_request();
                sub.get().set_observer(observer);
                let sub = sub.send().promise.await.unwrap();
                let subscription = sub.get().unwrap().get_subscription().unwrap();
                let history = client
                    .history_request()
                    .send()
                    .promise
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_history()
                    .unwrap();
                let first_event = history.next_request().send().promise;
                let mut put = client.put_request();
                put.get().set_expected_head(0);
                put.get().init_value().set_text("stored over Native");
                let rev = put
                    .send()
                    .promise
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_revision();
                assert_eq!(rev, 1);
                assert!(
                    ObjectServer::<reproto::store_capnp::introduction_ticket::Owned>::client(
                        ObjectState::new(state.store().clone(), ObjectId::new(42).unwrap()),
                        root.clone()
                    )
                    .is_err()
                );

                assert!(client.get_request().send().promise.await.is_err());
                let mut pubreq = client.publish_request();
                pubreq.get().set_revision(rev);
                assert_eq!(
                    pubreq
                        .send()
                        .promise
                        .await
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_revision(),
                    rev
                );
                let get = client.get_request().send().promise.await.unwrap();
                assert_eq!(
                    get.get().unwrap().get_value().unwrap().get_text().unwrap(),
                    "stored over Native"
                );
                while observed.borrow().is_empty() {
                    tokio::task::yield_now().await;
                }
                assert_eq!(observed.borrow()[0], (1, "stored over Native".into()));
                let response = first_event.await.unwrap();
                let result = response.get().unwrap().get_result().unwrap();
                assert_eq!(result.get_floor(), 0);
                match result.which().unwrap() {
                    reproto::store_capnp::history_result::Event(event) => {
                        assert_eq!(event.get_revision(), 1);
                        assert_eq!(
                            event.get_value().unwrap().get_text().unwrap(),
                            "stored over Native"
                        );
                    }
                    _ => panic!("initial Native history read lost its event"),
                }
                let mut pending = history.next_request();
                pending.get().set_after(1);
                let pending = pending.send().promise;
                subscription.cancel_request().send().promise.await.unwrap();
                root.revoke();
                assert!(pending.await.is_err());
                assert!(client.get_request().send().promise.await.is_err());
                for t in tasks {
                    t.abort();
                }
            })
            .await
            .unwrap();
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn three_party_introduction_preserves_rights_and_order() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(10), async {
                let dir = tempfile::tempdir().unwrap();
                let store = Rc::new(RefCell::new(Store::open(dir.path().join("db")).unwrap()));
                let state = ObjectState::new(store, ObjectId::new(7).unwrap());
                let broker = Identity::generate();
                let owner = Identity::generate();
                let recipient = Identity::generate();
                let parent = Grant::root(
                    ObjectId::new(7).unwrap(),
                    ObjectGeneration::new(1).unwrap(),
                    broker.public_key(),
                    Rights::ALL,
                );
                let (proxy, tasks) = network(
                    state.clone(),
                    parent.clone(),
                    &broker,
                    &owner,
                    None,
                    b"broker-owner",
                )
                .await;
                let mut intro = reproto::handoff::Introduction::new(
                    &owner,
                    &parent,
                    recipient.public_key(),
                    Rights::VIEW,
                )
                .unwrap();
                assert!(intro.enqueue_proxy());
                let package = intro.provide().unwrap();
                let mut p = proxy.put_request();
                p.get().init_value().set_text("before handoff");
                p.send().promise.await.unwrap();
                let mut p = proxy.publish_request();
                p.get().set_revision(1);
                p.send().promise.await.unwrap();
                let intro = Rc::new(RefCell::new(intro));
                let a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let b = UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let remote = b.local_addr().unwrap();
                // Independent wire peer: the authenticated serving API requires
                // the native Native prologue and stream-opening preface.
                let mut context = b"ReProto native RPC v1\0".to_vec();
                context.extend_from_slice(&package.context);
                let mut ac =
                    transport::config(&recipient, owner.public_key(), Some(package.psk), &context)
                        .unwrap();
                let (mut a, at) = transport::connect(a, remote, &mut ac).await.unwrap();
                use tokio::io::AsyncWriteExt;
                a.write_all(b"R").await.unwrap();
                let serving =
                    reproto::handoff::serve::<document::Owned>(intro.clone(), state, &owner, b)
                        .await
                        .unwrap();
                let (handoff, ct) =
                    rpc::client::<reproto::store_capnp::handoff::Client<document::Owned>>(a);
                let more = vec![at.abort_handle(), ct.abort_handle()];
                let mut invalid = handoff.accept_request();
                invalid.get().set_id(&[0; 32]);
                assert!(invalid.send().promise.await.is_err());
                let mut early = handoff.accept_request();
                early.get().set_id(&package.id);
                assert!(early.send().promise.await.is_err());
                assert!(intro.borrow().state().accepted());
                assert!(intro.borrow_mut().finish_proxy());
                let mut accept = handoff.accept_request();
                accept.get().set_id(&package.id);
                let accepted = accept.send();
                // Pipeline a get before the handoff response arrives.
                let response = accepted
                    .pipeline
                    .get_object()
                    .get_request()
                    .send()
                    .promise
                    .await
                    .unwrap();
                assert_eq!(response.get().unwrap().get_revision(), 1);
                let direct = accepted
                    .promise
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_object()
                    .unwrap();
                let mut replay = handoff.accept_request();
                replay.get().set_id(&package.id);
                let again = replay
                    .send()
                    .promise
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_object()
                    .unwrap();
                let first = capnp::capability::get_resolved_cap(direct.clone()).await;
                let second = capnp::capability::get_resolved_cap(again).await;
                assert_eq!(first.client.hook.get_ptr(), second.client.hook.get_ptr());
                assert!(intro.borrow().state().accepted());
                let response = direct.get_request().send().promise.await.unwrap();
                assert_eq!(
                    response
                        .get()
                        .unwrap()
                        .get_value()
                        .unwrap()
                        .get_text()
                        .unwrap(),
                    "before handoff"
                );
                assert!(direct.put_request().send().promise.await.is_err());
                parent.revoke();
                assert!(direct.get_request().send().promise.await.is_err());
                drop(serving);
                for t in tasks.into_iter().chain(more) {
                    t.abort();
                }
            })
            .await
            .unwrap();
        })
        .await;
}

async fn bootstrap<C: capnp::capability::FromClientHook>(
    client: &Identity,
    server: &Identity,
    service: capnp::capability::Client,
) -> (C, Vec<tokio::task::AbortHandle>) {
    let a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let b = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = b.local_addr().unwrap();
    let mut ac = transport::config(client, server.public_key(), None, b"bootstrap").unwrap();
    let mut bc = transport::config(server, client.public_key(), None, b"bootstrap").unwrap();
    let (a, at) = transport::connect(a, addr, &mut ac).await.unwrap();
    let (b, bt) = transport::accept(b, &mut bc).await.unwrap();
    let st = rpc::serve(b, service);
    let (c, ct) = rpc::client(a);
    (
        c,
        vec![
            at.abort_handle(),
            bt.abort_handle(),
            st.abort_handle(),
            ct.abort_handle(),
        ],
    )
}
#[tokio::test(flavor = "current_thread")]
async fn automatic_three_party_ticket_delivery_and_direct_connection() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(10), async {
                use reproto::{
                    introduction::{Introducer, Receiver},
                    store_capnp::{introducer, introduction_receiver},
                };
                let dir = tempfile::tempdir().unwrap();
                let store = Rc::new(RefCell::new(Store::open(dir.path().join("db")).unwrap()));
                let state = ObjectState::new(store, ObjectId::new(99).unwrap());
                let broker = Identity::generate();
                let owner = Rc::new(Identity::generate());
                let recipient = Rc::new(Identity::generate());
                let parent = Grant::root(
                    ObjectId::new(99).unwrap(),
                    ObjectGeneration::new(1).unwrap(),
                    broker.public_key(),
                    Rights::ALL,
                );
                let (proxy, mut tasks) = network(
                    state.clone(),
                    parent.clone(),
                    &broker,
                    &owner,
                    None,
                    b"original route",
                )
                .await;
                let mut put = proxy.put_request();
                put.get().init_value().set_text("three-party object");
                put.send().promise.await.unwrap();
                let mut pubreq = proxy.publish_request();
                pubreq.get().set_revision(1);
                pubreq.send().promise.await.unwrap();
                let source = Introducer::<document::Owned>::client(
                    owner.clone(),
                    parent.clone(),
                    state,
                    Rights::VIEW,
                    "127.0.0.1:0".parse().unwrap(),
                );
                let (source, more) = bootstrap::<introducer::Client<document::Owned>>(
                    &broker,
                    &owner,
                    source.client,
                )
                .await;
                tasks.extend(more);
                let (tx, mut rx) = tokio::sync::mpsc::channel(1);
                let sink: introduction_receiver::Client<document::Owned> =
                    capnp_rpc::new_client(Receiver {
                        identity: recipient.clone(),
                        local: "127.0.0.1:0".parse().unwrap(),
                        accepted: tx,
                    });
                let (sink, more) = bootstrap::<introduction_receiver::Client<document::Owned>>(
                    &broker,
                    &recipient,
                    sink.client,
                )
                .await;
                tasks.extend(more);
                let mut provide = source.provide_request();
                provide.get().set_recipient(&recipient.public_key());
                let offered = provide.send().promise.await.unwrap();
                let mut introduce = sink.introduce_request();
                introduce
                    .get()
                    .set_ticket(offered.get().unwrap().get_ticket().unwrap())
                    .unwrap();
                introduce.send().promise.await.unwrap();
                let direct = rx.recv().await.unwrap();
                let get = direct.object.get_request().send().promise.await.unwrap();
                assert_eq!(
                    get.get().unwrap().get_value().unwrap().get_text().unwrap(),
                    "three-party object"
                );
                assert!(direct.object.put_request().send().promise.await.is_err());
                parent.revoke();
                assert!(direct.object.get_request().send().promise.await.is_err());
                drop(direct);
                for t in tasks {
                    t.abort();
                }
            })
            .await
            .unwrap();
        })
        .await;
}
