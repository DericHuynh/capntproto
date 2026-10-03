use super::*;
use crate::semantics::Revisions;

// A valid compacted generation at a high revision avoids billions of writes.
// Record framing comes from production encoding; counter expectations are
// supplied independently by the tests/TLC graph.
fn checkpoint(path: &Path, revision: Revision) {
    let mut bytes = vec![0; HEADER];
    bytes.extend(
        record(
            3,
            ObjectKey::new(0),
            revision,
            &revision.get().to_le_bytes(),
        )
        .unwrap(),
    );
    bytes.extend(record(4, ObjectKey::new(0), revision, &[]).unwrap());
    bytes[..8].copy_from_slice(b"RPROTO04");
    set(&mut bytes, 8, 4);
    let end = bytes.len() as u64;
    set(&mut bytes, 16, end);
    let hash = digest(&SHA256, &bytes[..32]);
    bytes[32..64].copy_from_slice(hash.as_ref());
    fs::write(path, bytes).unwrap();
}

#[test]
fn final_revision_rejects_writes_atomically_but_remains_publishable_and_readable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    let key = ObjectKey::new(0);
    let before_max = Revision::new(u64::MAX - 1);
    checkpoint(&path, before_max);
    let mut store = Store::open(&path).unwrap();
    let last = store.put(key, before_max, b"last").unwrap();
    assert_eq!(last, Revision::MAX);
    assert_eq!(store.revision(key, last).unwrap().bytes(), b"last");
    let before = fs::read(&path).unwrap();
    assert!(matches!(
        store.put(key, last, b"overflow"),
        Err(Error::Conflict)
    ));
    for exhausted_first in [false, true] {
        let mut updates = [
            Update {
                object: ObjectKey::new(1),
                expected_head: Revision::INITIAL,
                expected_published: Some(Revision::INITIAL),
                value: b"must not commit",
            },
            Update {
                object: key,
                expected_head: last,
                expected_published: Some(before_max),
                value: b"overflow",
            },
        ];
        if exhausted_first {
            updates.swap(0, 1);
        }
        assert!(matches!(store.commit(&updates), Err(Error::Conflict)));
        assert_eq!(store.head(ObjectKey::new(1)), Revision::INITIAL);
        assert_eq!(store.published(ObjectKey::new(1)), Revision::INITIAL);
        assert_eq!(fs::read(&path).unwrap(), before);
    }
    store.publish(key, last, before_max).unwrap();
    let cursor = store.publication_cursor(key, before_max.get()).unwrap();
    let event = store.publication_after(&cursor).unwrap().unwrap();
    assert_eq!(event.cursor().after(), Revision::MAX);
    assert!(store.publication_after(event.cursor()).unwrap().is_none());
    drop(event);
    for retention in [
        Retention::History,
        Retention::Publishable,
        Retention::Latest,
    ] {
        store.compact(retention).unwrap();
        drop(store);
        store = Store::open(&path).unwrap();
        assert_eq!(store.head(key), Revision::MAX);
        assert_eq!(store.published(key), Revision::MAX);
        assert_eq!(store.get(key).unwrap().bytes(), b"last");
        assert!(matches!(
            store.put(key, Revision::MAX, b"overflow"),
            Err(Error::Conflict)
        ));
        assert!(matches!(
            store.revision(key, Revision::INITIAL),
            Err(Error::NotFound)
        ));
        assert!(matches!(
            store.publish(key, Revision::INITIAL, Revision::MAX),
            Err(Error::Conflict)
        ));
    }
}

#[test]
fn recovery_rejects_wrapped_single_and_batch_records_without_modifying_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    for batch in [false, true] {
        checkpoint(&path, Revision::MAX);
        let mut bytes = fs::read(&path).unwrap();
        if batch {
            let mut payload = vec![0; 48];
            set(&mut payload, 0, 1);
            set(&mut payload, 8, 0);
            set(&mut payload, 16, 1); // Reuse first revision after MAX.
            set(&mut payload, 24, 1);
            set(&mut payload, 32, 1);
            payload[40] = 42;
            bytes.extend(record(7, ObjectKey::new(0), Revision::INITIAL, &payload).unwrap());
        } else {
            bytes.extend(record(1, ObjectKey::new(0), Revision::new(1), b"reused").unwrap());
        }
        fs::write(&path, &bytes).unwrap();
        assert!(matches!(Store::open(&path), Err(Error::Corrupt(_))));
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn replay_tlc_revision_exhaustion_and_atomic_compare() {
    use capntproto_test_support::verification::exploration;
    const MODEL: &str = "verification/RevisionBoundary.tla";
    const CONFIG: &str = include_str!("../../verification/RevisionBoundary.cfg");
    const BASE: u64 = u64::MAX - 2;
    let paths = exploration::traces(MODEL, "revision-boundary", CONFIG).unwrap();
    for path in &paths {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("objects");
        checkpoint(&file, Revision::new(BASE));
        let mut store = Store::open(&file).unwrap();
        let key = ObjectKey::new(0);
        let other = ObjectKey::new(1);
        for state in path {
            let before = fs::read(&file).unwrap();
            let guard = Revisions::new(store.head(key), store.published(key)).unwrap();
            let arg = Revision::new(BASE + state["arg"]);
            let expected = Revision::new(BASE + state["compare"]);
            let result = match state["event"] {
                1 => {
                    let prediction = guard.stage(arg);
                    let result = store.put(
                        key,
                        arg,
                        &arg.checked_next()
                            .unwrap_or(Revision::MAX)
                            .get()
                            .to_le_bytes(),
                    );
                    assert_eq!(result.is_ok(), prediction.is_some());
                    if let Some(next) = prediction {
                        assert_eq!(result.as_ref().unwrap(), &next.head());
                    }
                    result.map(|_| ())
                }
                2 => {
                    let prediction = guard.publish(arg, expected);
                    let result = store.publish(key, arg, expected);
                    assert_eq!(result.is_ok(), prediction.is_some());
                    if let Some(next) = prediction {
                        assert_eq!(result.as_ref().unwrap(), &next.published());
                    }
                    result.map(|_| ())
                }
                3 => {
                    let other_head = store.head(other);
                    let primary_bytes = arg
                        .checked_next()
                        .unwrap_or(Revision::MAX)
                        .get()
                        .to_le_bytes();
                    let other_bytes = other_head.checked_next().unwrap().get().to_le_bytes();
                    store
                        .commit(&[
                            Update {
                                object: other,
                                expected_head: Revision::new(other_head.get() + state["badSecond"]),
                                expected_published: Some(other_head),
                                value: &other_bytes,
                            },
                            Update {
                                object: key,
                                expected_head: arg,
                                expected_published: Some(store.published(key)),
                                value: &primary_bytes,
                            },
                        ])
                        .map(|_| ())
                }
                4 => {
                    drop(store);
                    store = Store::open(&file).unwrap();
                    Ok(())
                }
                5 => store.compact(Retention::History).map(|_| ()),
                _ => panic!("unknown revision event: {state:?}"),
            };
            assert_eq!(u64::from(result.is_ok()), state["result"], "{path:?}");
            if result.is_err() {
                assert!(matches!(result, Err(Error::Conflict)));
                assert_eq!(
                    fs::read(&file).unwrap(),
                    before,
                    "rejection wrote bytes: {path:?}"
                );
            }
            assert_eq!(store.head(key).get() - BASE, state["h"], "{path:?}");
            assert_eq!(store.published(key).get() - BASE, state["p"], "{path:?}");
            assert_eq!(store.head(other).get(), state["other"], "{path:?}");
            assert_eq!(store.published(other), store.head(other));
            for object in store.objects() {
                let snapshot = store.revision(object, store.head(object)).unwrap();
                assert_eq!(snapshot.bytes(), snapshot.revision().get().to_le_bytes());
            }
        }
    }
    exploration::controls(
        MODEL,
        "revision-boundary",
        CONFIG,
        &[
            ("wrap", "ExpectedOutcome"),
            ("staleStage", "ExpectedOutcome"),
            ("stalePublish", "ExpectedOutcome"),
            ("future", "ExpectedOutcome"),
            ("rewind", "ExpectedOutcome"),
            ("partial", "ExactCounters"),
            ("reuse", "ExactCounters"),
        ],
        None,
    )
    .unwrap();
    eprintln!("{} revision boundary edge-prefix replays", paths.len());
}
