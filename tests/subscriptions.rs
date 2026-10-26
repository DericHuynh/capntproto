#![cfg(feature = "storage")]

use capnp::{capability::Promise, traits::HasTypeId};
use capntproto::{
    authority::{Grant, ObjectGeneration, ObjectId, Rights},
    orm::{ObjectServer, ObjectState},
    storage::{ObjectKey, Revision, Store, Update},
    store_capnp::{document, history, object, observer, subscription},
};
use std::{cell::RefCell, rc::Rc, time::Duration};
use tokio::sync::mpsc;

type Object = object::Client<document::Owned>;
type History = history::Client<document::Owned>;

struct Fixture {
    store: Rc<RefCell<Store>>,
    state: Rc<ObjectState>,
    grant: Grant,
    object: Object,
    _directory: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let store = Rc::new(RefCell::new(
            Store::open(directory.path().join("objects")).unwrap(),
        ));
        let state = ObjectState::new(store.clone(), ObjectId::new(7).unwrap());
        let grant = Grant::root(
            state.object(),
            ObjectGeneration::new(1).unwrap(),
            [1; 32],
            Rights::ALL,
        );
        let object = ObjectServer::client(state.clone(), grant.clone()).unwrap();
        Self {
            store,
            state,
            grant,
            object,
            _directory: directory,
        }
    }
    fn other_client(&self) -> Object {
        ObjectServer::client(
            ObjectState::new(self.store.clone(), self.state.object()),
            self.grant.clone(),
        )
        .unwrap()
    }
    fn publish_directly(&self, commit: bool) {
        let mut message = capnp::message::Builder::new_default();
        message.init_root::<document::Builder>().set_text("updated");
        let mut bytes = document::Reader::TYPE_ID.to_le_bytes().to_vec();
        bytes.extend(capnp::serialize::write_message_to_words(&message));
        let mut store = self.store.borrow_mut();
        if commit {
            store
                .commit(&[Update {
                    object: ObjectKey::new(7),
                    expected_head: Revision::INITIAL,
                    expected_published: Some(Revision::INITIAL),
                    value: &bytes,
                }])
                .unwrap();
        } else {
            let revision = store
                .put(ObjectKey::new(7), Revision::INITIAL, &bytes)
                .unwrap();
            store
                .publish(ObjectKey::new(7), revision, Revision::INITIAL)
                .unwrap();
        }
    }
}

#[derive(Clone, Copy)]
enum Reply {
    Ok,
    Error,
    Pending,
}
struct Observer {
    events: mpsc::UnboundedSender<(u64, String)>,
    reply: Reply,
}
impl observer::Server<document::Owned> for Observer {
    fn changed(
        self: Rc<Self>,
        context: observer::ChangedContext<'_, document::Owned>,
    ) -> Promise<(), capnp::Error> {
        let (params, _) = context.into_owned();
        Promise::from_future(async move {
            let value = params.get()?;
            let _ = self.events.send((
                value.get_revision(),
                value.get_value()?.get_text()?.to_str()?.into(),
            ));
            match self.reply {
                Reply::Ok => Ok(()),
                Reply::Error => Err(capnp::Error::failed("observer stopped".into())),
                Reply::Pending => std::future::pending().await,
            }
        })
    }
}
async fn subscribe(
    object: &Object,
    reply: Reply,
) -> (subscription::Client, mpsc::UnboundedReceiver<(u64, String)>) {
    let (events, received) = mpsc::unbounded_channel();
    let mut request = object.subscribe_request();
    request
        .get()
        .set_observer(capnp_rpc::new_client(Observer { events, reply }));
    let subscription = request
        .send()
        .promise
        .await
        .unwrap()
        .get()
        .unwrap()
        .get_subscription()
        .unwrap();
    (subscription, received)
}
async fn open_history(object: &Object) -> capnp::Result<History> {
    object
        .history_request()
        .send()
        .promise
        .await?
        .get()?
        .get_history()
}
async fn publish(object: &Object) {
    let mut put = object.put_request();
    put.get().init_value().set_text("updated");
    put.send().promise.await.unwrap();
    let mut publish = object.publish_request();
    publish.get().set_revision(1);
    publish.send().promise.await.unwrap();
}
async fn drain() {
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn subscriptions_follow_publications_from_every_store_writer() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for writer in 0..3 {
                for publish_first in [false, true] {
                    let f = Fixture::new();
                    let publication = async {
                        if writer == 0 {
                            publish(&f.other_client()).await;
                        } else {
                            f.publish_directly(writer == 2);
                        }
                    };
                    tokio::pin!(publication);
                    if publish_first {
                        (&mut publication).await;
                    }
                    let (subscription, mut events) = subscribe(&f.object, Reply::Ok).await;
                    drain().await;
                    if !publish_first {
                        assert!(events.try_recv().is_err());
                        publication.await;
                    }
                    assert_eq!(
                        tokio::time::timeout(Duration::from_secs(2), events.recv())
                            .await
                            .expect("subscription missed a store publication"),
                        Some((1, "updated".into())),
                    );
                    subscription.cancel_request().send().promise.await.unwrap();
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn revoked_subscriptions_stop_idle_and_blocked_observers() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for blocked in [false, true] {
                let f = Fixture::new();
                let child = f.grant.delegate([2; 32], Rights::SUBSCRIBE).unwrap();
                let subscriber = ObjectServer::client(f.state.clone(), child.clone()).unwrap();
                let (subscription, mut events) = subscribe(&subscriber, Reply::Pending).await;
                if blocked {
                    publish(&f.object).await;
                    tokio::time::timeout(Duration::from_secs(2), events.recv())
                        .await
                        .unwrap()
                        .unwrap();
                } else {
                    drain().await;
                }
                child.revoke();
                assert!(tokio::time::timeout(Duration::from_secs(2), events.recv())
                    .await
                    .expect("revocation left the observer retained")
                    .is_none());
                assert_slot_released(&f.object, &subscription).await;
            }
        })
        .await;
}

async fn assert_slot_released(object: &Object, subscription: &subscription::Client) {
    drain().await;
    let mut histories = Vec::new();
    for _ in 0..64 {
        histories.push(
            open_history(object)
                .await
                .expect("finished subscription kept its admission slot"),
        );
    }
    assert!(open_history(object).await.is_err());
    // Canceling a completed subscription must not release another user's slot.
    subscription.cancel_request().send().promise.await.unwrap();
    subscription.cancel_request().send().promise.await.unwrap();
    assert!(open_history(object).await.is_err());
    histories.pop();
    drain().await;
    histories.push(open_history(object).await.unwrap());
    assert!(open_history(object).await.is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn failed_observers_release_their_subscription_slot() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let f = Fixture::new();
            let (subscription, mut events) = subscribe(&f.object, Reply::Error).await;
            publish(&f.object).await;
            tokio::time::timeout(Duration::from_secs(2), events.recv())
                .await
                .unwrap()
                .unwrap();
            assert_slot_released(&f.object, &subscription).await;
        })
        .await;
}
