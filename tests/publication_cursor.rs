use reproto::storage::Revision;
use reproto::storage::{Error, ObjectKey, PublicationCursor, Retention, Store};

fn seed(path: &std::path::Path) -> Store {
    let mut store = Store::open(path).unwrap();
    for k in 0..=1 {
        let key = ObjectKey::new(k);
        for rev in 1..=3 {
            store
                .put(key, Revision::new(rev - 1), &[10 * k as u8 + rev as u8])
                .unwrap();
        }
        store
            .publish(key, Revision::new(1), Revision::INITIAL)
            .unwrap();
        if k == 1 {
            store
                .publish(key, Revision::new(3), Revision::new(1))
                .unwrap();
        }
    }
    store
}

#[test]
fn import_checks_drafts_future_positions_and_full_width_keys() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = seed(&dir.path().join("objects"));
    for key in [ObjectKey::new(0), ObjectKey::new(1)] {
        for after in [2, 4, u64::MAX] {
            assert!(matches!(
                store.publication_cursor(key, after),
                Err(Error::InvalidCursor)
            ));
        }
    }
    assert!(matches!(
        store.publication_cursor(ObjectKey::new(0), 3),
        Err(Error::InvalidCursor)
    ));
    let max = ObjectKey::new(u64::MAX);
    let cursor = store.publication_cursor(max, 0).unwrap();
    assert_eq!((cursor.object(), cursor.after()), (max, Revision::new(0)));
    assert!(store.publication_after(&cursor).unwrap().is_none());
    store.put(max, Revision::INITIAL, b"later").unwrap();
    assert!(store.publication_after(&cursor).unwrap().is_none());
    store
        .publish(max, Revision::new(1), Revision::INITIAL)
        .unwrap();
    let event = store.publication_after(&cursor).unwrap().unwrap();
    assert_eq!(event.snapshot().bytes(), b"later");
    assert_eq!(event.cursor().object(), max);
    assert_eq!(event.cursor().after(), Revision::new(1));
    assert_eq!(cursor.after(), Revision::INITIAL);
}

#[test]
fn cursor_ownership_survives_moves_but_not_another_store_or_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    let store = seed(&path);
    let other = seed(&dir.path().join("other"));
    let key = ObjectKey::new(0);
    let cursor = store.publication_cursor(key, 0).unwrap();
    let foreign = other.publication_cursor(key, 0).unwrap();
    let mut moved = Box::new(store);
    assert!(matches!(
        moved.publication_after(&foreign),
        Err(Error::ForeignCursor)
    ));
    assert!(matches!(
        other.publication_after(&cursor),
        Err(Error::ForeignCursor)
    ));
    moved.compact(Retention::History).unwrap();
    assert_eq!(
        moved
            .publication_after(&cursor.clone())
            .unwrap()
            .unwrap()
            .snapshot()
            .bytes(),
        &[1]
    );
    drop(moved);
    // The retained cursor carries only an owner token, not a mapping/file lock.
    let reopened = Store::open(&path).unwrap();
    assert!(matches!(
        reopened.publication_after(&cursor),
        Err(Error::ForeignCursor)
    ));
    let resumed = reopened
        .publication_cursor(cursor.object(), cursor.after().get())
        .unwrap();
    assert_eq!(
        reopened
            .publication_after(&resumed)
            .unwrap()
            .unwrap()
            .snapshot()
            .bytes(),
        &[1]
    );
}

#[test]
fn retries_skip_drafts_and_retention_revalidates_issued_cursors() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = seed(&dir.path().join("objects"));
    let key = ObjectKey::new(1);
    let start = store.publication_cursor(key, 0).unwrap();
    let first = store.publication_after(&start).unwrap().unwrap();
    let (snapshot, after_one) = first.into_parts();
    assert_eq!(snapshot.revision(), Revision::new(1));
    let last = store.publication_after(&after_one).unwrap().unwrap();
    assert_eq!(
        (last.snapshot().object(), last.snapshot().revision()),
        (key, Revision::new(3))
    );
    assert_eq!(last.cursor().object(), key);
    assert_eq!(last.cursor().after(), Revision::new(3));
    assert!(store.publication_after(last.cursor()).unwrap().is_none());
    for retention in [
        Retention::History,
        Retention::Publishable,
        Retention::Latest,
    ] {
        store.compact(retention).unwrap();
        if retention == Retention::History {
            assert_eq!(
                store
                    .publication_after(&start.clone())
                    .unwrap()
                    .unwrap()
                    .snapshot()
                    .revision(),
                Revision::new(1)
            );
            assert_eq!(
                store
                    .publication_after(&after_one)
                    .unwrap()
                    .unwrap()
                    .snapshot()
                    .revision(),
                Revision::new(3)
            );
        } else {
            for cursor in [&start, &after_one] {
                assert!(matches!(
                    store.publication_after(cursor),
                    Err(Error::HistoryExpired { floor }) if floor == Revision::new(3)
                ));
            }
            assert!(matches!(
                store.publication_cursor(key, 0),
                Err(Error::HistoryExpired { floor }) if floor == Revision::new(3)
            ));
        }
        assert!(store.publication_after(last.cursor()).unwrap().is_none());
        assert_eq!(snapshot.bytes(), &[11]);
        assert_eq!(last.snapshot().bytes(), &[13]);
        assert_eq!(start.after(), Revision::INITIAL);
        assert_eq!(after_one.after(), Revision::new(1));
    }
}

fn error_code(error: Error) -> u64 {
    match error {
        Error::InvalidCursor => 4,
        Error::HistoryExpired { .. } => 5,
        Error::ForeignCursor => 6,
        other => panic!("unexpected cursor error: {other}"),
    }
}

#[test]
fn replay_tlc_cursor_ownership_retention_and_retry() {
    use reproto_test_support::verification::exploration;
    const MODEL: &str = "verification/PublicationCursor.tla";
    const CONFIG: &str = include_str!("../verification/PublicationCursor.cfg");
    let paths = exploration::traces(MODEL, "publication-cursor", CONFIG).unwrap();
    let template_dir = tempfile::tempdir().unwrap();
    let template_path = template_dir.path().join("template");
    let foreign = seed(&template_path);
    let template = std::fs::read(&template_path).unwrap();
    for path in &paths {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("objects");
        std::fs::write(&file, &template).unwrap();
        let mut store = Store::open(&file).unwrap();
        let mut cursor: Option<PublicationCursor> = None;
        let mut next: Option<PublicationCursor> = None;
        let (mut compact, mut epoch, mut owner, mut checkpoint) = (0, 0, 0, 0);
        for state in path {
            let previous_next = next.take();
            let (mut result, mut value, mut result_key, mut next_after) = (0, 0, 0, 0);
            match state["event"] {
                1 => match store
                    .publication_cursor(ObjectKey::new(state["key"]), state["requested"])
                {
                    Ok(issued) => {
                        checkpoint = state["requested"];
                        owner = epoch;
                        cursor = Some(issued);
                        result = 1;
                    }
                    Err(error) => result = error_code(error),
                },
                2 => {
                    let db = if state["target"] == 0 {
                        &store
                    } else {
                        &foreign
                    };
                    let input = cursor.as_ref().unwrap().clone();
                    match db.publication_after(&input) {
                        Ok(None) => result = 3,
                        Ok(Some(event)) => {
                            let (snapshot, advanced) = event.into_parts();
                            result = 2;
                            value = snapshot.revision().get();
                            result_key = snapshot.object().get();
                            assert_eq!(snapshot.bytes(), &[10 * result_key as u8 + value as u8]);
                            assert_eq!(advanced.object(), snapshot.object());
                            next_after = advanced.after().get();
                            next = Some(advanced);
                        }
                        Err(error) => {
                            if let Error::HistoryExpired { floor } = &error {
                                assert_eq!(*floor, store.history_bounds(input.object()).unwrap().0);
                            }
                            result = error_code(error);
                        }
                    }
                    assert_eq!(input.after(), Revision::new(checkpoint));
                }
                3 => {
                    cursor = previous_next;
                    checkpoint = cursor.as_ref().unwrap().after().get();
                }
                4 => {
                    cursor = None;
                    owner = 0;
                    checkpoint = 0;
                }
                5 => {
                    store
                        .publish(ObjectKey::new(0), Revision::new(3), Revision::new(1))
                        .unwrap();
                }
                6 => {
                    store.compact(Retention::Publishable).unwrap();
                    compact = 1;
                }
                7 => {
                    drop(store);
                    store = Store::open(&file).unwrap();
                    epoch = 1;
                }
                8 => (),
                _ => panic!("unknown cursor event: {state:?}"),
            }
            for (field, actual) in [
                ("latest", store.published(ObjectKey::new(0)).get()),
                (
                    "floor",
                    store.history_bounds(ObjectKey::new(0)).unwrap().0.get(),
                ),
                ("compact", compact),
                ("epoch", epoch),
                ("owner", owner),
                ("held", u64::from(cursor.is_some())),
                ("object", cursor.as_ref().map_or(0, |c| c.object().get())),
                ("after", cursor.as_ref().map_or(0, |c| c.after().get())),
                ("checkpoint", checkpoint),
                ("result", result),
                ("value", value),
                ("resultKey", result_key),
                ("nextAfter", next_after),
            ] {
                assert_eq!(actual, state[field], "{field}: {path:?}");
            }
        }
    }
    exploration::controls(
        MODEL,
        "publication-cursor",
        CONFIG,
        &[
            ("draft", "ImportValidation"),
            ("owner", "OwnerSeparation"),
            ("floor", "ReadValidation"),
            ("advance", "CursorImmutable"),
            ("object", "EventBinding"),
            ("skip", "EventBinding"),
            ("next", "NextPosition"),
        ],
        None,
    )
    .unwrap();
    eprintln!("{} publication cursor edge-prefix replays", paths.len());
}
