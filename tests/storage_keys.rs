use reproto::storage::Revision;
use reproto::{
    authority::{Grant, ObjectGeneration, ObjectId, Rights},
    bulk::{Config, Status},
    durable_bulk::{JournalId, Receiver},
    orm::ObjectState,
    storage::{ObjectKey, Retention, Snapshot, Store, Update},
    store_capnp::document,
};
use std::{cell::RefCell, rc::Rc};

#[test]
fn all_key_bits_and_snapshot_identity_survive_compaction_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    let mut store = Store::open(&path).unwrap();
    let keys = [0, 1, 1 << 32, u64::MAX].map(ObjectKey::new);
    let mut held = Vec::new();
    for key in keys {
        assert_eq!(key.to_string(), key.get().to_string());
        assert_eq!(
            store
                .put(key, Revision::INITIAL, &key.get().to_le_bytes())
                .unwrap(),
            Revision::new(1)
        );
        store
            .publish(key, Revision::new(1), Revision::INITIAL)
            .unwrap();
        let snapshot = store.get(key).unwrap();
        held.push(snapshot.clone());
        assert_eq!(
            (snapshot.object(), snapshot.revision()),
            (key, Revision::new(1))
        );
        assert_eq!(
            store.put(key, Revision::new(1), b"new draft").unwrap(),
            Revision::new(2)
        );
    }
    assert_eq!(store.objects().collect::<Vec<_>>(), keys);
    store.compact(Retention::Latest).unwrap();
    drop(store);
    assert!(Store::open(&path).is_err());
    for (snapshot, key) in held.iter().zip(keys) {
        assert_eq!(
            (snapshot.object(), snapshot.revision()),
            (key, Revision::new(1))
        );
        assert_eq!(snapshot.bytes(), key.get().to_le_bytes());
    }
    drop(held);
    let store = Store::open(&path).unwrap();
    for key in keys {
        assert_eq!(store.head(key), Revision::new(2));
        assert_eq!(store.get(key).unwrap().bytes(), key.get().to_le_bytes());
        let draft = store.revision(key, Revision::new(2)).unwrap();
        assert_eq!((draft.object(), draft.revision()), (key, Revision::new(2)));
        assert_eq!(draft.bytes(), b"new draft");
    }
    assert_eq!(
        ObjectKey::from(ObjectId::new(u64::MAX).unwrap()),
        ObjectKey::new(u64::MAX)
    );
}

#[test]
fn journal_domain_preserves_zero_and_max_and_rejects_target_collision() {
    let dir = tempfile::tempdir().unwrap();
    let store = Rc::new(RefCell::new(
        Store::open(dir.path().join("uploads")).unwrap(),
    ));
    let target = ObjectId::new(7).unwrap();
    let state = ObjectState::new(store.clone(), target);
    let grant = Grant::root(
        target,
        ObjectGeneration::new(1).unwrap(),
        [1; 32],
        Rights::ALL,
    );
    let config = Config::new(1, 1, 1, 1).unwrap();
    let before = store.borrow().file_bytes();
    assert!(Receiver::<document::Owned>::create(
        state.clone(),
        JournalId::new(7),
        grant.clone(),
        config.clone(),
        [0; 32]
    )
    .is_err());
    assert_eq!(store.borrow().file_bytes(), before);
    for value in [0, u64::MAX] {
        let journal = JournalId::new(value);
        assert_eq!(journal.get(), value);
        assert_eq!(journal.to_string(), value.to_string());
        assert_eq!(journal.key(), ObjectKey::new(value));
        let receiver = Receiver::<document::Owned>::create(
            state.clone(),
            journal,
            grant.clone(),
            config.clone(),
            [0; 32],
        )
        .unwrap();
        assert_eq!(receiver.checkpoint().unwrap().status, Status::Receiving);
        assert_eq!(store.borrow().head(journal.key()), Revision::new(1));
        drop(receiver);
        assert!(Receiver::<document::Owned>::resume(state.clone(), journal, grant.clone()).is_ok());
    }
    assert_eq!(
        store.borrow().head(ObjectKey::from(target)),
        Revision::INITIAL
    );
}

#[test]
fn replay_tlc_storage_key_and_snapshot_bindings() {
    use reproto_test_support::verification::exploration;
    const MODEL: &str = "verification/StorageObjectKeys.tla";
    const CONFIG: &str = include_str!("../verification/StorageObjectKeys.cfg");
    let paths = exploration::traces(MODEL, "storage-object-keys", CONFIG).unwrap();
    for path in &paths {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("objects");
        let mut store = Store::open(&file).unwrap();
        let mut snapshot: Option<Snapshot> = None;
        let mut writes = [0, 0];
        let (mut source_key, mut source_revision, mut compact, mut reopened) = (0, 0, 0, 0);
        for state in path {
            let key = ObjectKey::new(state["key"]);
            match state["event"] {
                1 => {
                    let expected = writes[key.get() as usize];
                    let bytes = (10 * key.get() + expected + 1).to_le_bytes();
                    // Both record encodings must preserve zero as an actual object key.
                    let revision = if key.get() == 0 {
                        store
                            .commit(&[Update {
                                object: key,
                                expected_head: Revision::new(expected),
                                expected_published: None,
                                value: &bytes,
                            }])
                            .unwrap()[0]
                    } else {
                        store.put(key, Revision::new(expected), &bytes).unwrap()
                    };
                    assert_eq!(revision, Revision::new(expected + 1));
                    writes[key.get() as usize] += 1;
                }
                2 => {
                    store
                        .publish(key, store.head(key), store.published(key))
                        .unwrap();
                }
                3 => {
                    source_key = key.get();
                    source_revision = store.published(key).get();
                    let view = store.get(key).unwrap();
                    snapshot = Some(view.clone());
                }
                4 => {
                    snapshot = None;
                    source_key = 0;
                    source_revision = 0;
                }
                5 => {
                    store.compact(Retention::Latest).unwrap();
                    compact = 1;
                }
                6 => {
                    drop(store);
                    store = Store::open(&file).unwrap();
                    reopened = 1;
                }
                7 => (),
                _ => panic!("invalid event: {state:?}"),
            }
            for (field, actual) in [
                ("h0", store.head(ObjectKey::new(0)).get()),
                ("h1", store.head(ObjectKey::new(1)).get()),
                ("p0", store.published(ObjectKey::new(0)).get()),
                ("p1", store.published(ObjectKey::new(1)).get()),
                ("w0", writes[0]),
                ("w1", writes[1]),
                ("snap", u64::from(snapshot.is_some())),
                ("sourceKey", source_key),
                ("sourceRevision", source_revision),
                ("object", snapshot.as_ref().map_or(0, |s| s.object().get())),
                (
                    "revision",
                    snapshot.as_ref().map_or(0, |s| s.revision().get()),
                ),
                (
                    "value",
                    snapshot
                        .as_ref()
                        .map_or(0, |s| u64::from_le_bytes(s.bytes().try_into().unwrap())),
                ),
                ("compact", compact),
                ("reopened", reopened),
            ] {
                assert_eq!(actual, state[field], "{field}: {path:?}");
            }
            assert_eq!(
                store.objects().collect::<Vec<_>>(),
                (0..=1)
                    .filter(|&k| writes[k as usize] > 0)
                    .map(ObjectKey::new)
                    .collect::<Vec<_>>()
            );
        }
    }
    exploration::controls(
        MODEL,
        "storage-object-keys",
        CONFIG,
        &[
            ("crossKey", "ExactHeads"),
            ("relabel", "SnapshotIdentity"),
            ("label", "SnapshotIdentity"),
            ("payload", "SnapshotData"),
            ("mapping", "SnapshotData"),
            ("recovery", "ExactHeads"),
        ],
        None,
    )
    .unwrap();
    eprintln!("{} storage key/snapshot edge-prefix replays", paths.len());
}
