use capntproto::storage::ObjectKey;
use capntproto::storage::Revision;
#[allow(dead_code)]
mod support;
use capnp::capability::Promise;
use capntproto::{
    authority::{Grant, ObjectGeneration, ObjectId, Rights},
    orm::{ObjectServer, ObjectState},
    storage::{Error, Retention, Store},
    store_capnp::{document, history, history_result, object},
};
use futures::FutureExt;
use std::{cell::RefCell, rc::Rc, time::Duration};
use support::Hub;
type Object = object::Client<document::Owned>;
type History = history::Client<document::Owned>;
type Read = Promise<
    capnp::capability::Response<history::next_results::Owned<document::Owned>>,
    capnp::Error,
>;

async fn drain() {
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
}
async fn open(object: &Object) -> History {
    object
        .history_request()
        .send()
        .promise
        .await
        .unwrap()
        .get()
        .unwrap()
        .get_history()
        .unwrap()
}
fn next(history: &History, after: u64) -> Read {
    let mut request = history.next_request();
    request.get().set_after(after);
    request.send().promise
}
#[derive(Debug, PartialEq, Eq)]
enum Event {
    Value(u64, String, u64),
    Gap(u64),
}
fn event(
    response: capnp::capability::Response<history::next_results::Owned<document::Owned>>,
) -> Event {
    let result = response.get().unwrap().get_result().unwrap();
    match result.which().unwrap() {
        history_result::Event(e) => Event::Value(
            e.get_revision(),
            e.get_value()
                .unwrap()
                .get_text()
                .unwrap()
                .to_str()
                .unwrap()
                .into(),
            result.get_floor(),
        ),
        history_result::Gap(()) => Event::Gap(result.get_floor()),
    }
}
async fn read(history: &History, after: u64) -> Event {
    event(
        tokio::time::timeout(Duration::from_secs(2), next(history, after))
            .await
            .unwrap()
            .unwrap(),
    )
}
async fn put(object: &Object, head: u64, text: &str) -> u64 {
    let mut r = object.put_request();
    r.get().set_expected_head(head);
    r.get().init_value().set_text(text);
    r.send()
        .promise
        .await
        .unwrap()
        .get()
        .unwrap()
        .get_revision()
}
async fn publish(object: &Object, revision: u64, expected: u64) {
    let mut r = object.publish_request();
    r.get().set_revision(revision);
    r.get().set_expected_published(expected);
    r.send().promise.await.unwrap();
}
fn typed(text: &str) -> Vec<u8> {
    use capnp::traits::HasTypeId;
    let mut message = capnp::message::Builder::new_default();
    message.init_root::<document::Builder>().set_text(text);
    let mut bytes = document::Reader::TYPE_ID.to_le_bytes().to_vec();
    bytes.extend(capnp::serialize::write_message_to_words(&message));
    bytes
}
struct Fixture {
    store: Rc<RefCell<Store>>,
    state: Rc<ObjectState>,
    grant: Grant,
    object: Object,
    tasks: Vec<tokio::task::JoinHandle<capnp::Result<()>>>,
    _dir: tempfile::TempDir,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
impl Fixture {
    fn new(wire: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("objects");
        let store = Rc::new(RefCell::new(Store::open(&path).unwrap()));
        let state = ObjectState::new(store.clone(), ObjectId::new(7).unwrap());
        let grant = Grant::root(
            ObjectId::new(7).unwrap(),
            ObjectGeneration::new(1).unwrap(),
            [1; 32],
            Rights::ALL,
        );
        let mut object = ObjectServer::client(state.clone(), grant.clone()).unwrap();
        let mut tasks = vec![];
        if wire {
            let hub = Rc::new(RefCell::new(Hub::default()));
            let a = Hub::network(&hub, 1);
            let b = Hub::network(&hub, 2);
            let mut caller = capnp_rpc::RpcSystem::new(Box::new(a), None);
            let server = capnp_rpc::RpcSystem::new(Box::new(b), Some(object.client));
            object = caller.bootstrap(2);
            tasks = [caller, server].map(tokio::task::spawn_local).into();
        }
        Self {
            store,
            state,
            grant,
            object,
            tasks,
            _dir: dir,
        }
    }
}

#[test]
fn history_checkpoint_truncation_corruption_and_later_torn_appends_fail_safely() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    let mut store = Store::open(&path).unwrap();
    for rev in 1..=3 {
        store
            .put(ObjectKey::new(7), Revision::new(rev - 1), &[rev as u8])
            .unwrap();
        store
            .publish(
                ObjectKey::new(7),
                Revision::new(rev),
                Revision::new(rev - 1),
            )
            .unwrap();
    }
    store.compact(Retention::History).unwrap();
    let checkpoint = std::fs::read(&path).unwrap();
    store
        .put(ObjectKey::new(7), Revision::new(3), b"four")
        .unwrap();
    let before_publish = std::fs::read(&path).unwrap();
    store
        .publish(ObjectKey::new(7), Revision::new(4), Revision::new(3))
        .unwrap();
    let complete = std::fs::read(&path).unwrap();
    drop(store);
    for cut in 1..checkpoint.len() {
        let p = dir.path().join("cut");
        std::fs::write(&p, &checkpoint[..cut]).unwrap();
        assert!(Store::open(&p).is_err(), "checkpoint cut {cut}");
    }
    for cut in checkpoint.len()..complete.len() {
        let p = dir.path().join("tail");
        std::fs::write(&p, &complete[..cut]).unwrap();
        let recovered = Store::open(&p).unwrap();
        assert_eq!(
            recovered.published(ObjectKey::new(7)),
            Revision::new(3),
            "append cut {cut}"
        );
        assert_eq!(
            read_publication(&recovered, ObjectKey::new(7), 1)
                .unwrap()
                .unwrap()
                .revision(),
            Revision::new(2)
        );
        assert!(read_publication(&recovered, ObjectKey::new(7), 3)
            .unwrap()
            .is_none());
        assert_eq!(
            recovered.head(ObjectKey::new(7)),
            Revision::new(if cut >= before_publish.len() { 4 } else { 3 })
        );
    }
    // Recompute record checksums after invalid metadata edits to exercise
    // semantic validation as well as bit-corruption detection.
    fn mutate(bytes: &mut [u8], start: usize, object: u64, revision: u64, kind: u64) {
        bytes[start + 8..start + 16].copy_from_slice(&kind.to_le_bytes());
        bytes[start + 16..start + 24].copy_from_slice(&object.to_le_bytes());
        bytes[start + 24..start + 32].copy_from_slice(&revision.to_le_bytes());
        let n = u64::from_le_bytes(bytes[start + 32..start + 40].try_into().unwrap()) as usize;
        let mut data = bytes[start..start + 40].to_vec();
        data.extend_from_slice(&bytes[start + 80..start + 80 + n]);
        bytes[start + 40..start + 72]
            .copy_from_slice(ring::digest::digest(&ring::digest::SHA256, &data).as_ref());
        let header = ring::digest::digest(&ring::digest::SHA256, &bytes[start..start + 72]);
        bytes[start + 72..start + 80].copy_from_slice(&header.as_ref()[..8]);
    }
    let first_event = 64 + 3 * 104;
    for (offset, object, revision, kind) in [
        (first_event, 7, 9, 5),
        (first_event + 96, 7, 1, 5),
        (first_event, 8, 1, 5),
        (first_event, 7, 1, 4),
        (first_event, 7, 1, 6),
    ] {
        let mut bytes = checkpoint.clone();
        mutate(&mut bytes, offset, object, revision, kind);
        let p = dir.path().join("bad");
        std::fs::write(&p, bytes).unwrap();
        assert!(Store::open(&p).is_err());
    }
    let mut store = Store::open(&path).unwrap();
    store.compact(Retention::Latest).unwrap();
    store.compact(Retention::History).unwrap();
    drop(store);
    let original = std::fs::read(&path).unwrap();
    let floor = original.len() - 96;
    for (object, revision) in [(7, 0), (7, 3), (8, 4)] {
        let mut bytes = original.clone();
        mutate(&mut bytes, floor, object, revision, 6);
        let p = dir.path().join("bad-floor");
        std::fs::write(&p, bytes).unwrap();
        assert!(Store::open(&p).is_err());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn caller_owned_cursor_resumes_over_rpc_after_complete_server_restart() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("objects");
            let cursor = dir.path().join("cursor");
            for restart in [false, true] {
                let store = Rc::new(RefCell::new(Store::open(&path).unwrap()));
                let state = ObjectState::new(store.clone(), ObjectId::new(7).unwrap());
                let service = ObjectServer::<document::Owned>::client(
                    state,
                    Grant::root(
                        ObjectId::new(7).unwrap(),
                        ObjectGeneration::new(1).unwrap(),
                        [1; 32],
                        Rights::ALL,
                    ),
                )
                .unwrap();
                let hub = Rc::new(RefCell::new(Hub::default()));
                let mut a = capnp_rpc::RpcSystem::new(Box::new(Hub::network(&hub, 1)), None);
                let b = capnp_rpc::RpcSystem::new(
                    Box::new(Hub::network(&hub, 2)),
                    Some(service.client),
                );
                let object: Object = a.bootstrap(2);
                let tasks = [a, b].map(tokio::task::spawn_local);
                let history = open(&object).await;
                if !restart {
                    for (head, text) in [(0, "one"), (1, "two"), (2, "three")] {
                        put(&object, head, text).await;
                        publish(&object, head + 1, head).await;
                    }
                    assert_eq!(read(&history, 0).await, Event::Value(1, "one".into(), 0));
                    std::fs::write(&cursor, 1u64.to_le_bytes()).unwrap();
                    // Observe but do not checkpoint event 2: retry after reconnect
                    // must deliver it again, not silently advance a server cursor.
                    assert_eq!(read(&history, 1).await, Event::Value(2, "two".into(), 0));
                    store.borrow_mut().compact(Retention::History).unwrap();
                } else {
                    let after =
                        u64::from_le_bytes(std::fs::read(&cursor).unwrap().try_into().unwrap());
                    assert_eq!(
                        read(&history, after).await,
                        Event::Value(2, "two".into(), 0)
                    );
                    assert_eq!(read(&history, 2).await, Event::Value(3, "three".into(), 0));
                }
                drop(history);
                drop(object);
                for task in tasks {
                    task.abort();
                    let _ = task.await;
                }
                drop(store);
                drain().await;
            }
        })
        .await;
}

#[test]
fn publication_history_recovers_skips_drafts_and_reports_retention_gaps() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    let mut store = Store::open(&path).unwrap();
    for rev in 1..=4 {
        store
            .put(ObjectKey::new(7), Revision::new(rev - 1), &[rev as u8])
            .unwrap();
    }
    store
        .publish(ObjectKey::new(7), Revision::new(1), Revision::INITIAL)
        .unwrap();
    store
        .publish(ObjectKey::new(7), Revision::new(3), Revision::new(1))
        .unwrap();
    store
        .put(ObjectKey::new(8), Revision::INITIAL, b"other")
        .unwrap();
    store
        .publish(ObjectKey::new(8), Revision::new(1), Revision::INITIAL)
        .unwrap();
    assert_eq!(
        read_publication(&store, ObjectKey::new(7), 0)
            .unwrap()
            .unwrap()
            .revision(),
        Revision::new(1)
    );
    assert_eq!(
        read_publication(&store, ObjectKey::new(7), 1)
            .unwrap()
            .unwrap()
            .revision(),
        Revision::new(3)
    );
    assert!(matches!(
        read_publication(&store, ObjectKey::new(7), 2),
        Err(Error::InvalidCursor)
    ));
    let held = read_publication(&store, ObjectKey::new(7), 0)
        .unwrap()
        .unwrap();
    store.compact(Retention::History).unwrap();
    assert_eq!(held.bytes(), &[1]);
    assert!(matches!(
        store.revision(ObjectKey::new(7), Revision::new(2)),
        Err(Error::NotFound)
    ));
    assert_eq!(
        store
            .revision(ObjectKey::new(7), Revision::new(4))
            .unwrap()
            .bytes(),
        &[4]
    );
    drop(held);
    drop(store);
    let mut store = Store::open(&path).unwrap();
    assert_eq!(
        store.history_bounds(ObjectKey::new(7)).unwrap(),
        (Revision::new(0), Revision::new(3))
    );
    assert_eq!(
        read_publication(&store, ObjectKey::new(7), 1)
            .unwrap()
            .unwrap()
            .bytes(),
        &[3]
    );
    assert_eq!(
        read_publication(&store, ObjectKey::new(8), 0)
            .unwrap()
            .unwrap()
            .bytes(),
        b"other"
    );
    store.compact(Retention::Publishable).unwrap();
    assert!(matches!(
        read_publication(&store, ObjectKey::new(7), 1),
        Err(Error::HistoryExpired { floor }) if floor == Revision::new(3)
    ));
    store
        .publish(ObjectKey::new(7), Revision::new(4), Revision::new(3))
        .unwrap();
    store.compact(Retention::History).unwrap();
    drop(store);
    let mut store = Store::open(&path).unwrap();
    assert_eq!(
        store.history_bounds(ObjectKey::new(7)).unwrap(),
        (Revision::new(3), Revision::new(4))
    );
    assert_eq!(
        read_publication(&store, ObjectKey::new(7), 3)
            .unwrap()
            .unwrap()
            .revision(),
        Revision::new(4)
    );
    store.compact(Retention::History).unwrap();
    drop(store);
    assert_eq!(
        Store::open(&path)
            .unwrap()
            .history_bounds(ObjectKey::new(7))
            .unwrap(),
        (Revision::new(3), Revision::new(4))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn history_rpc_replays_every_publication_and_retries_without_advancing_a_cursor() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for wire in [false, true] {
                let f = Fixture::new(wire);
                let history = open(&f.object).await;
                for (head, text) in [(0, "one"), (1, "draft"), (2, "three")] {
                    put(&f.object, head, text).await;
                }
                publish(&f.object, 1, 0).await;
                publish(&f.object, 3, 1).await;
                assert_eq!(read(&history, 0).await, Event::Value(1, "one".into(), 0));
                assert_eq!(read(&history, 0).await, Event::Value(1, "one".into(), 0));
                assert_eq!(read(&history, 1).await, Event::Value(3, "three".into(), 0));
                assert!(next(&history, 2).await.is_err());
                assert!(next(&history, 4).await.is_err());
                let mut wait = next(&history, 3);
                drain().await;
                assert!((&mut wait).now_or_never().is_none());
                // A separately constructed ObjectState and direct Store updates must
                // both wake this stream, without depending on ObjectFactory caching.
                let other = ObjectServer::<document::Owned>::client(
                    ObjectState::new(f.store.clone(), ObjectId::new(7).unwrap()),
                    f.grant.clone(),
                )
                .unwrap();
                put(&other, 3, "four").await;
                publish(&other, 4, 3).await;
                assert_eq!(
                    event(wait.await.unwrap()),
                    Event::Value(4, "four".into(), 0)
                );
                let wait = next(&history, 4);
                drain().await;
                f.store
                    .borrow_mut()
                    .put(ObjectKey::new(7), Revision::new(4), &typed("five"))
                    .unwrap();
                f.store
                    .borrow_mut()
                    .publish(ObjectKey::new(7), Revision::new(5), Revision::new(4))
                    .unwrap();
                assert_eq!(
                    event(wait.await.unwrap()),
                    Event::Value(5, "five".into(), 0)
                );
                f.store.borrow_mut().compact(Retention::History).unwrap();
                assert_eq!(read(&history, 0).await, Event::Value(1, "one".into(), 0));
                f.store.borrow_mut().compact(Retention::Latest).unwrap();
                assert_eq!(read(&history, 0).await, Event::Gap(5));
                assert_eq!(read(&history, 1).await, Event::Gap(5));
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn blocked_history_reads_cancel_revoke_and_release_the_pending_slot() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for wire in [false, true] {
                for revoke in [false, true] {
                    let f = Fixture::new(wire);
                    let history = open(&f.object).await;
                    let mut wait = next(&history, 0);
                    drain().await;
                    assert!((&mut wait).now_or_never().is_none());
                    assert!(next(&history, 0).await.is_err());
                    drop(wait);
                    drain().await;
                    let mut wait = next(&history, 0);
                    drain().await;
                    assert!((&mut wait).now_or_never().is_none());
                    if revoke {
                        f.grant.revoke();
                    } else {
                        history.cancel_request().send().promise.await.unwrap();
                    }
                    assert!(tokio::time::timeout(Duration::from_secs(2), wait)
                        .await
                        .unwrap()
                        .is_err());
                    assert!(next(&history, 0).await.is_err());
                    history.cancel_request().send().promise.await.unwrap();
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn history_requires_authority_and_shares_the_subscription_quota() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let f = Fixture::new(false);
            let denied = ObjectServer::<document::Owned>::client(
                f.state.clone(),
                Grant::root(
                    ObjectId::new(7).unwrap(),
                    ObjectGeneration::new(1).unwrap(),
                    [2; 32],
                    Rights::GET,
                ),
            )
            .unwrap();
            assert!(denied.history_request().send().promise.await.is_err());
            let child = f.grant.delegate([3; 32], Rights::SUBSCRIBE).unwrap();
            let allowed = ObjectServer::<document::Owned>::client(f.state.clone(), child).unwrap();
            let history = open(&allowed).await;
            let wait = next(&history, 0);
            drain().await;
            f.grant.revoke();
            assert!(tokio::time::timeout(Duration::from_secs(2), wait)
                .await
                .unwrap()
                .is_err());
            let f = Fixture::new(false);
            let mut streams = vec![];
            struct Observer;
            impl capntproto::store_capnp::observer::Server<document::Owned> for Observer {
                async fn changed(
                    self: Rc<Self>,
                    _: capntproto::store_capnp::observer::ChangedParams<document::Owned>,
                    _: capntproto::store_capnp::observer::ChangedResults<document::Owned>,
                ) -> capnp::Result<()> {
                    Ok(())
                }
            }
            let mut subscribe = f.object.subscribe_request();
            subscribe
                .get()
                .set_observer(capnp_rpc::new_client(Observer));
            let push = subscribe
                .send()
                .promise
                .await
                .unwrap()
                .get()
                .unwrap()
                .get_subscription()
                .unwrap();
            for _ in 0..63 {
                streams.push(open(&f.object).await);
            }
            assert!(f.object.history_request().send().promise.await.is_err());
            streams[0].cancel_request().send().promise.await.unwrap();
            let replacement = open(&f.object).await;
            assert!(f.object.history_request().send().promise.await.is_err());
            drop(replacement);
            drain().await;
            let _replacement = open(&f.object).await;
            assert!(f.object.history_request().send().promise.await.is_err());
            push.cancel_request().send().promise.await.unwrap();
            let _after_push = open(&f.object).await;
        })
        .await;
}

#[test]
fn history_quota_rejection_never_invents_publications_or_discards_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    let mut store = Store::open(&path).unwrap();
    store
        .put(ObjectKey::new(7), Revision::INITIAL, b"one")
        .unwrap();
    store
        .publish(ObjectKey::new(7), Revision::new(1), Revision::INITIAL)
        .unwrap();
    store.compact(Retention::Latest).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    store
        .set_limits(capntproto::storage::Limits {
            max_file_bytes: bytes.len(),
            ..store.limits()
        })
        .unwrap();
    // A history checkpoint needs a floor record when history has been trimmed.
    assert!(store.compact(Retention::History).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(
        store.history_bounds(ObjectKey::new(7)).unwrap(),
        (Revision::new(1), Revision::new(1))
    );
    assert!(store
        .put(ObjectKey::new(7), Revision::new(1), b"two")
        .is_err());
    assert_eq!(store.published(ObjectKey::new(7)), Revision::new(1));
    assert!(read_publication(&store, ObjectKey::new(7), 1)
        .unwrap()
        .is_none());

    let path = dir.path().join("failed-publication");
    let mut store = Store::open(&path).unwrap();
    store
        .put(ObjectKey::new(7), Revision::INITIAL, b"one")
        .unwrap();
    store
        .put(ObjectKey::new(7), Revision::new(1), b"two")
        .unwrap();
    store
        .publish(ObjectKey::new(7), Revision::new(1), Revision::INITIAL)
        .unwrap();
    store
        .set_limits(capntproto::storage::Limits {
            max_file_bytes: store.file_bytes(),
            ..store.limits()
        })
        .unwrap();
    assert!(matches!(
        store.publish(ObjectKey::new(7), Revision::new(2), Revision::new(1)),
        Err(Error::Limit)
    ));
    assert_eq!(
        store.history_bounds(ObjectKey::new(7)).unwrap(),
        (Revision::new(0), Revision::new(1))
    );
    assert!(read_publication(&store, ObjectKey::new(7), 1)
        .unwrap()
        .is_none());
    drop(store);
    let recovered = Store::open(&path).unwrap();
    assert_eq!(
        recovered.history_bounds(ObjectKey::new(7)).unwrap(),
        (Revision::new(0), Revision::new(1))
    );
    assert!(read_publication(&recovered, ObjectKey::new(7), 1)
        .unwrap()
        .is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn history_validates_old_payload_types_even_when_the_current_type_is_valid() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for wire in [false, true] {
                let f = Fixture::new(wire);
                f.store
                    .borrow_mut()
                    .put(
                        ObjectKey::new(7),
                        Revision::INITIAL,
                        b"wrong-type-id-and-malformed-message",
                    )
                    .unwrap();
                f.store
                    .borrow_mut()
                    .publish(ObjectKey::new(7), Revision::new(1), Revision::INITIAL)
                    .unwrap();
                f.store
                    .borrow_mut()
                    .put(ObjectKey::new(7), Revision::new(1), &typed("valid"))
                    .unwrap();
                f.store
                    .borrow_mut()
                    .publish(ObjectKey::new(7), Revision::new(2), Revision::new(1))
                    .unwrap();
                let h = open(&f.object).await;
                assert!(next(&h, 0).await.is_err());
                assert_eq!(read(&h, 1).await, Event::Value(2, "valid".into(), 0));
            }
        })
        .await;
}

#[derive(serde::Deserialize)]
struct Trace {
    model: String,
    steps: Vec<Step>,
}
#[derive(serde::Deserialize)]
struct Step {
    action: String,
    state: Vec<u64>,
}

fn replay_storage(trace: &Trace) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    let mut store = Some(Store::open(&path).unwrap());
    for rev in 1..=3 {
        store
            .as_mut()
            .unwrap()
            .put(ObjectKey::new(7), Revision::new(rev - 1), &[rev as u8])
            .unwrap();
    }
    for step in &trace.steps {
        let s = &step.state;
        match step.action.as_str() {
            "publish" => {
                let db = store.as_mut().unwrap();
                db.publish(
                    ObjectKey::new(7),
                    Revision::new(s[0]),
                    db.published(ObjectKey::new(7)),
                )
                .unwrap();
            }
            "preserve" => {
                store.as_mut().unwrap().compact(Retention::History).unwrap();
            }
            "trim" => {
                store
                    .as_mut()
                    .unwrap()
                    .compact(Retention::Publishable)
                    .unwrap();
            }
            "restart" => {
                drop(store.take());
                store = Some(Store::open(&path).unwrap());
            }
            _ => panic!("unknown history storage action"),
        }
        let db = store.as_ref().unwrap();
        assert_eq!(db.head(ObjectKey::new(7)), Revision::new(3));
        assert_eq!(
            db.history_bounds(ObjectKey::new(7)).unwrap(),
            (Revision::new(s[10]), Revision::new(s[0]))
        );
        for rev in 1..=3 {
            let entry = db.revision(ObjectKey::new(7), Revision::new(rev));
            assert_eq!(entry.is_ok(), s[rev as usize + 6] != 0);
            if let Ok(entry) = entry {
                assert_eq!(entry.bytes(), &[rev as u8]);
            }
        }
        for cursor in 0..=4 {
            let got = match read_publication(db, ObjectKey::new(7), cursor) {
                Ok(None) => 0,
                Ok(Some(snapshot)) => {
                    assert_eq!(snapshot.bytes(), &[snapshot.revision().get() as u8]);
                    snapshot.revision().get()
                }
                Err(Error::InvalidCursor) => 4,
                Err(Error::HistoryExpired { floor }) => {
                    assert_eq!(floor, Revision::new(s[10]));
                    5
                }
                Err(e) => panic!("unexpected history read error: {e}"),
            };
            assert_eq!(
                got,
                s[15 + cursor as usize],
                "{} cursor {cursor}",
                step.action
            );
        }
    }
}

async fn replay_rpc(trace: &Trace, wire: bool) {
    let f = Fixture::new(wire);
    let h = open(&f.object).await;
    let mut cursor = 0;
    let mut calls = 0;
    let mut observation = (0, 0, 0);
    let mut pending: Option<Read> = None;
    for step in &trace.steps {
        let s = &step.state;
        match step.action.as_str() {
            "start" => {
                assert!(pending.is_none());
                pending = Some(next(&h, cursor));
                observation = (1, 0, 0);
                calls += 1;
            }
            "publish" => {
                // The independent publisher retains its authority when this
                // consumer is revoked. Commit using a different local facet.
                let revision = s[0];
                f.store
                    .borrow_mut()
                    .put(
                        ObjectKey::new(7),
                        Revision::new(revision - 1),
                        &typed(&revision.to_string()),
                    )
                    .unwrap();
                f.store
                    .borrow_mut()
                    .publish(
                        ObjectKey::new(7),
                        Revision::new(revision),
                        Revision::new(revision - 1),
                    )
                    .unwrap();
            }
            "trim" => {
                f.store
                    .borrow_mut()
                    .compact(Retention::Publishable)
                    .unwrap();
            }
            "cancel" => {
                h.cancel_request().send().promise.await.unwrap();
            }
            "revoke" => f.grant.revoke(),
            "drop" => {
                assert!(pending.take().is_some());
                observation = (0, 0, 0);
            }
            "ack" => {
                assert_eq!(observation.0, 2);
                cursor = observation.1;
                observation = (0, 0, 0);
            }
            _ => panic!("unknown history RPC action"),
        }
        drain().await;
        if let Some(result) = pending.as_mut().and_then(|p| p.now_or_never()) {
            pending = None;
            observation = match result {
                Err(_) => (4, 0, 0),
                Ok(response) => match event(response) {
                    Event::Value(revision, text, floor) => {
                        assert_eq!(text, revision.to_string());
                        (2, revision, floor)
                    }
                    Event::Gap(floor) => (3, 0, floor),
                },
            };
        }
        assert_eq!(
            f.store.borrow().history_bounds(ObjectKey::new(7)).unwrap(),
            (Revision::new(s[1]), Revision::new(s[0]))
        );
        assert_eq!(u64::from(f.grant.is_live()), s[2]);
        assert_eq!(cursor, s[4], "consumer cursor, {}", step.action);
        assert_eq!(
            observation,
            (s[5], s[6], s[7]),
            "{} wire={wire}",
            step.action
        );
        assert_eq!(calls, s[8]);
        if pending.is_some() {
            // A rejected overlapping call must not release the first one's slot.
            assert!(next(&h, cursor).await.is_err());
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_orm_history_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_ORM_HISTORY_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!traces.is_empty());
    tokio::task::LocalSet::new()
        .run_until(async {
            for (index, trace) in traces.iter().enumerate() {
                match trace.model.as_str() {
                    "StorageHistory" => replay_storage(trace),
                    "RpcHistory" => {
                        replay_rpc(trace, false).await;
                        replay_rpc(trace, true).await;
                    }
                    _ => panic!("unknown model at trace {index}"),
                }
                drain().await;
            }
        })
        .await;
}

// Adapt numeric model/wire positions through the checked public API.
fn read_publication(
    store: &Store,
    object: ObjectKey,
    after: u64,
) -> capntproto::storage::Result<Option<capntproto::storage::Snapshot>> {
    let cursor = store.publication_cursor(object, after)?;
    Ok(store
        .publication_after(&cursor)?
        .map(|event| event.into_parts().0))
}
