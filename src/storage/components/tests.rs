use super::*;
use crate::storage::io::faults::{self, Action};

const OBJECT: ObjectKey = ObjectKey::new(7);
const HOT: ComponentId = ComponentId::new(1);
const COLD: ComponentId = ComponentId::new(2);

fn disk_bytes(store: &Store) -> Vec<u8> {
    use std::io::Read;
    // Inspect through the lock-owning handle: Windows byte-range locks also
    // block reads through a separately opened handle in the same process.
    let mut file = &*store.file;
    let position = file.stream_position().unwrap();
    file.rewind().unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    file.seek(SeekFrom::Start(position)).unwrap();
    bytes
}

#[test]
fn component_count_and_file_quotas_are_checked_before_io() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    let mut store = Store::open_components(&path).unwrap();
    let mut updates: Vec<_> = (0..256)
        .map(|id| ComponentUpdate {
            id: ComponentId::new(id),
            value: Some(&b"x"[..]),
        })
        .collect();
    let guard = faults::install([]);
    store
        .edit_components(OBJECT, Revision::INITIAL, Some(Revision::INITIAL), &updates)
        .unwrap();
    assert_eq!(
        faults::visits()
            .iter()
            .filter(|&&p| p == Point::AppendSync)
            .count(),
        1
    );
    drop(guard);
    let before = disk_bytes(&store);
    updates.push(ComponentUpdate {
        id: ComponentId::new(256),
        value: Some(b"x"),
    });
    assert!(store
        .edit_components(OBJECT, Revision::new(1), None, &updates)
        .is_err());
    assert!(store
        .edit_components(OBJECT, Revision::new(1), None, &updates[256..])
        .is_err());
    store
        .set_limits(Limits {
            max_file_bytes: store.file_bytes(),
            ..Limits::default()
        })
        .unwrap();
    assert!(matches!(edit(&mut store, b"new", None), Err(Error::Limit)));
    assert!(!store.poisoned);
    assert_eq!(disk_bytes(&store), before);
    assert_eq!(
        store.get_components(OBJECT).unwrap().components().len(),
        256
    );
}

fn edit(store: &mut Store, hot: &[u8], cold: Option<&[u8]>) -> Result<Revision> {
    let mut updates = vec![ComponentUpdate {
        id: HOT,
        value: Some(hot),
    }];
    if let Some(cold) = cold {
        updates.push(ComponentUpdate {
            id: COLD,
            value: Some(cold),
        });
    }
    store.edit_components(
        OBJECT,
        store.head(OBJECT),
        Some(store.published(OBJECT)),
        &updates,
    )
}

#[test]
fn small_edits_share_bytes_and_recover_one_atomic_publication() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    let mut store = Store::open_components(&path).unwrap();
    edit(&mut store, b"12345678", Some(&vec![42; 65536])).unwrap();
    let old = store.get_components(OBJECT).unwrap();
    let before = store.file_bytes();
    edit(&mut store, b"abcdefgh", None).unwrap();
    assert_eq!(store.file_bytes() - before, 176);
    let a = &store.components[&(OBJECT, Revision::new(1))][&COLD];
    let b = &store.components[&(OBJECT, Revision::new(2))][&COLD];
    assert_eq!(a.offset, b.offset);
    assert_eq!(old.get(HOT).unwrap(), b"12345678");
    assert!(matches!(store.get(OBJECT), Err(Error::Layout)));
    drop(old);
    drop(store);
    let store = Store::open_components(&path).unwrap();
    let new = store.get_components(OBJECT).unwrap();
    assert_eq!(new.get(HOT).unwrap(), b"abcdefgh");
    assert_eq!(new.get(COLD).unwrap(), vec![42; 65536]);
    assert_eq!(new.revision(), Revision::new(2));
}

#[test]
fn limits_cas_and_layout_errors_do_not_mutate_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_components(dir.path().join("objects")).unwrap();
    edit(&mut store, b"old", Some(b"cold")).unwrap();
    let before = store.file_bytes();
    let update = [ComponentUpdate {
        id: HOT,
        value: Some(b"new"),
    }];
    for (head, published) in [(0, 1), (1, 0)] {
        assert!(matches!(
            store.edit_components(
                OBJECT,
                Revision::new(head),
                Some(Revision::new(published)),
                &update
            ),
            Err(Error::Conflict)
        ));
    }
    assert!(store
        .edit_components(OBJECT, Revision::new(1), None, &[])
        .is_err());
    assert!(store
        .edit_components(
            OBJECT,
            Revision::new(1),
            None,
            &[
                ComponentUpdate {
                    id: HOT,
                    value: Some(b"a")
                },
                ComponentUpdate {
                    id: HOT,
                    value: None
                },
            ]
        )
        .is_err());
    assert!(store
        .edit_components(
            OBJECT,
            Revision::new(1),
            None,
            &[ComponentUpdate {
                id: ComponentId::new(99),
                value: None
            },]
        )
        .is_err());
    assert!(matches!(
        store.put(OBJECT, Revision::new(1), b"whole"),
        Err(Error::Layout)
    ));
    store
        .set_limits(Limits {
            max_entry_bytes: 88,
            ..Limits::default()
        })
        .unwrap();
    assert!(matches!(
        edit(&mut store, b"larger than eight", None),
        Err(Error::Limit)
    ));
    assert!(store
        .set_limits(Limits {
            max_entry_bytes: 87,
            ..Limits::default()
        })
        .is_err());
    assert_eq!(store.file_bytes(), before);
    let unchanged = edit(&mut store, b"old", None).unwrap();
    assert_eq!(
        store.file_bytes() - before,
        RECORD + FOOTER + PREFIX + 2 * DESCRIPTOR
    );
    let draft = store
        .edit_components(OBJECT, unchanged, None, &update)
        .unwrap();
    assert_eq!(
        store.get_components(OBJECT).unwrap().get(HOT).unwrap(),
        b"old"
    );
    store.publish(OBJECT, draft, unchanged).unwrap();
    assert_eq!(
        store.get_components(OBJECT).unwrap().get(HOT).unwrap(),
        b"new"
    );
}

#[test]
fn formats_are_explicit_and_whole_objects_can_coexist() {
    let dir = tempfile::tempdir().unwrap();
    for version in [4, 5] {
        let path = dir.path().join(version.to_string());
        let mut store = if version == 4 {
            Store::open(&path)
        } else {
            Store::open_components(&path)
        }
        .unwrap();
        assert_eq!(store.format_version(), version);
        store.put(OBJECT, Revision::INITIAL, b"whole").unwrap();
        assert!(matches!(edit(&mut store, b"hot", None), Err(Error::Layout)));
        if version == 5 {
            store
                .edit_components(
                    ObjectKey::new(8),
                    Revision::INITIAL,
                    Some(Revision::INITIAL),
                    &[ComponentUpdate {
                        id: HOT,
                        value: Some(b"component"),
                    }],
                )
                .unwrap();
            store.compact(Retention::History).unwrap();
        }
        drop(store);
        let before = fs::read(&path).unwrap();
        assert!(if version == 4 {
            Store::open_components(&path)
        } else {
            Store::open(&path)
        }
        .is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        let store = if version == 4 {
            Store::open(&path)
        } else {
            Store::open_components(&path)
        }
        .unwrap();
        assert_eq!(
            store.revision(OBJECT, Revision::new(1)).unwrap().bytes(),
            b"whole"
        );
    }
}

#[test]
fn compaction_rebases_deleted_origins_and_deduplicates_retained_history() {
    for retention in [
        Retention::Latest,
        Retention::Publishable,
        Retention::History,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("objects");
        let mut store = Store::open_components(&path).unwrap();
        edit(&mut store, b"1", Some(&vec![42; 65536])).unwrap();
        let old = store.get_components(OBJECT).unwrap();
        let cursor = store.publication_cursor(OBJECT, 0).unwrap();
        edit(&mut store, b"2", None).unwrap();
        edit(&mut store, b"3", None).unwrap();
        store
            .edit_components(
                OBJECT,
                Revision::new(3),
                None,
                &[ComponentUpdate {
                    id: HOT,
                    value: Some(b"4"),
                }],
            )
            .unwrap();
        store
            .edit_components(
                OBJECT,
                Revision::new(4),
                None,
                &[ComponentUpdate {
                    id: HOT,
                    value: Some(b"5"),
                }],
            )
            .unwrap();
        let result = store.compact(retention).unwrap();
        assert_eq!(
            result.after_bytes,
            fs::metadata(&path).unwrap().len() as usize
        );
        assert!(result.after_bytes < 65536 + 2000); // Cold bytes appear once, even with history.
        assert_eq!(
            result.removed_revisions,
            match retention {
                Retention::Latest => 3,
                Retention::Publishable => 2,
                Retention::History => 0,
            }
        );
        if retention == Retention::History {
            let event = store.component_publication_after(&cursor).unwrap().unwrap();
            assert_eq!(event.snapshot.get(HOT).unwrap(), b"1");
            assert_eq!(
                store
                    .component_publication_after(&event.cursor)
                    .unwrap()
                    .unwrap()
                    .snapshot
                    .get(HOT)
                    .unwrap(),
                b"2"
            );
        } else {
            assert!(matches!(
                store.component_publication_after(&cursor),
                Err(Error::HistoryExpired { .. })
            ));
        }
        assert_eq!(old.get(HOT).unwrap(), b"1");
        assert_eq!(old.get(COLD).unwrap(), vec![42; 65536]);
        drop(store);
        assert!(Store::open_components(&path).is_err());
        drop(old);
        let mut store = Store::open_components(&path).unwrap();
        assert!(matches!(
            store.component_publication_after(&cursor),
            Err(Error::ForeignCursor)
        ));
        assert_eq!(
            store.get_components(OBJECT).unwrap().get(HOT).unwrap(),
            b"3"
        );
        assert_eq!(
            store
                .component_revision(OBJECT, Revision::new(5))
                .unwrap()
                .get(COLD)
                .unwrap(),
            vec![42; 65536]
        );
        edit(&mut store, b"6", None).unwrap();
        store.compact(retention).unwrap();
        drop(store);
        assert_eq!(
            Store::open_components(&path)
                .unwrap()
                .get_components(OBJECT)
                .unwrap()
                .get(HOT)
                .unwrap(),
            b"6"
        );
    }
}

#[test]
fn deleting_all_components_and_readding_empty_values_survives_compaction() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    let mut store = Store::open_components(&path).unwrap();
    edit(&mut store, b"", Some(b"")).unwrap();
    store
        .edit_components(
            OBJECT,
            Revision::new(1),
            Some(Revision::new(1)),
            &[
                ComponentUpdate {
                    id: HOT,
                    value: None,
                },
                ComponentUpdate {
                    id: COLD,
                    value: None,
                },
            ],
        )
        .unwrap();
    assert_eq!(store.get_components(OBJECT).unwrap().components().len(), 0);
    edit(&mut store, b"", Some(b"")).unwrap();
    for retention in [Retention::History, Retention::Latest] {
        store.compact(retention).unwrap();
        drop(store);
        store = Store::open_components(&path).unwrap();
        assert_eq!(
            store.get_components(OBJECT).unwrap().get(COLD),
            Some(&b""[..])
        );
    }
}

#[test]
fn every_torn_component_commit_recovers_all_old_components() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source");
    let mut store = Store::open_components(&path).unwrap();
    edit(&mut store, b"old", Some(b"cold")).unwrap();
    let start = store.file_bytes();
    edit(&mut store, b"new", Some(b"different")).unwrap();
    drop(store);
    let complete = fs::read(&path).unwrap();
    let cut = dir.path().join("cut");
    for end in start..complete.len() {
        fs::write(&cut, &complete[..end]).unwrap();
        let store = Store::open_components(&cut).unwrap();
        assert_eq!(store.head(OBJECT), Revision::new(1), "cut {end}");
        assert_eq!(
            store.get_components(OBJECT).unwrap().get(HOT).unwrap(),
            b"old"
        );
        assert_eq!(
            store.get_components(OBJECT).unwrap().get(COLD).unwrap(),
            b"cold"
        );
    }
    let mut corrupt = complete;
    corrupt[start + RECORD + PREFIX + DESCRIPTOR] ^= 1;
    fs::write(&cut, &corrupt).unwrap();
    assert!(matches!(
        Store::open_components(&cut),
        Err(Error::Corrupt(_))
    ));
    assert_eq!(fs::read(&cut).unwrap(), corrupt);
}

#[test]
fn valid_checksums_do_not_bypass_manifest_validation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    let mut store = Store::open_components(&path).unwrap();
    edit(&mut store, b"a", Some(b"cold")).unwrap();
    drop(store);
    let prefix = fs::read(&path).unwrap();
    let good = encode(
        [
            (HOT, Revision::new(1), &b"a"[..]),
            (COLD, Revision::new(1), &b"cold"[..]),
        ]
        .into_iter(),
        true,
    )
    .unwrap();
    for (offset, word) in [
        (0, 2),
        (8, 257),
        (16, 2),
        (24 + 8, 2),
        (24 + 16, 2),
        (48, 1),
        (48, 99),
    ] {
        let mut payload = good.clone();
        set(&mut payload, offset, word);
        let mut bad = prefix.clone();
        bad.extend(record(8, OBJECT, Revision::new(2), &payload).unwrap());
        fs::write(&path, &bad).unwrap();
        assert!(
            Store::open_components(&path).is_err(),
            "offset {offset} word {word}"
        );
        assert_eq!(fs::read(&path).unwrap(), bad);
    }
    for payload in [
        {
            let mut b = good.clone();
            b.extend_from_slice(&[0; 8]);
            b
        },
        {
            let mut b = encode([(HOT, Revision::INITIAL, &b"a"[..])].into_iter(), true).unwrap();
            *b.last_mut().unwrap() = 1;
            b
        },
    ] {
        let mut bad = prefix.clone();
        bad.extend(record(8, OBJECT, Revision::new(2), &payload).unwrap());
        fs::write(&path, &bad).unwrap();
        assert!(Store::open_components(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), bad);
    }
}

#[test]
fn component_io_faults_quarantine_and_recover_complete_revisions() {
    for (plan, complete) in [
        (
            vec![
                (Point::AppendWrite, Action::Short(87)),
                (Point::AppendWrite, Action::Error(28)),
            ],
            false,
        ),
        (vec![(Point::AppendSync, Action::Error(5))], true),
        (vec![(Point::AppendSynced, Action::Error(5))], true),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("objects");
        let mut store = Store::open_components(&path).unwrap();
        edit(&mut store, b"old", Some(b"cold")).unwrap();
        let old = store.get_components(OBJECT).unwrap();
        let guard = faults::install(plan);
        assert!(edit(&mut store, b"new", Some(b"new cold")).is_err());
        assert!(store.poisoned);
        assert!(store.get_components(OBJECT).is_err());
        assert!(edit(&mut store, b"retry", None).is_err());
        assert_eq!(old.get(HOT).unwrap(), b"old");
        drop(guard);
        drop(store);
        drop(old);
        let store = Store::open_components(&path).unwrap();
        assert_eq!(
            store.get_components(OBJECT).unwrap().get(HOT).unwrap(),
            if complete { &b"new"[..] } else { &b"old"[..] }
        );
        assert_eq!(
            store.get_components(OBJECT).unwrap().get(COLD).unwrap(),
            if complete {
                &b"new cold"[..]
            } else {
                &b"cold"[..]
            }
        );
    }
}

#[test]
fn component_compaction_faults_preserve_references() {
    for point in [
        Point::CompactWrite,
        Point::CompactSync,
        Point::CompactRename,
        Point::CompactDirectorySync,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("objects");
        let mut store = Store::open_components(&path).unwrap();
        edit(&mut store, b"old", Some(b"cold")).unwrap();
        edit(&mut store, b"new", None).unwrap();
        let guard = faults::install([(point, Action::Error(5))]);
        assert!(store.compact(Retention::Latest).is_err());
        drop(guard);
        drop(store);
        let store = Store::open_components(&path).unwrap();
        assert_eq!(
            store.get_components(OBJECT).unwrap().get(COLD).unwrap(),
            b"cold"
        );
        assert_eq!(
            store.get_components(OBJECT).unwrap().get(HOT).unwrap(),
            b"new"
        );
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(32))]
    #[test]
    fn mixed_edits_publications_retention_and_recovery_match_materialized_values(
        actions in proptest::collection::vec((0u8..8, 0u64..4, proptest::collection::vec(proptest::num::u8::ANY, 0..24)), 20..80)
    ) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("objects");
        let mut store = Store::open_components(&path).unwrap();
        let mut head = Revision::INITIAL;
        let mut published = Revision::INITIAL;
        let mut revisions = BTreeMap::<Revision, BTreeMap<ComponentId, Vec<u8>>>::new();
        let mut publications = BTreeSet::new();
        for (action, id, bytes) in actions {
            if action < 4 || head == Revision::INITIAL {
                let id = ComponentId::new(id);
                let mut value = revisions.get(&head).cloned().unwrap_or_default();
                let remove = action == 3 && value.contains_key(&id);
                let update = ComponentUpdate { id, value: if remove { None } else { Some(&bytes) } };
                let publish = action == 0;
                head = store.edit_components(OBJECT, head, publish.then_some(published), &[update]).unwrap();
                if remove { value.remove(&id); } else { value.insert(id, bytes); }
                revisions.insert(head, value);
                if publish { published = head; publications.insert(head); }
            } else if action == 4 && head > published {
                // Publish an intermediate retained draft, not necessarily head.
                let draft = *revisions.range((Excluded(published), std::ops::Bound::Unbounded)).next().unwrap().0;
                store.publish(OBJECT, draft, published).unwrap();
                published = draft;
                publications.insert(draft);
            } else if action >= 5 {
                let retention = match action { 5 => Retention::Latest, 6 => Retention::Publishable, _ => Retention::History };
                store.compact(retention).unwrap();
                revisions.retain(|r, _| *r == head || *r == published || (retention != Retention::Latest && *r > published) || (retention == Retention::History && publications.contains(r)));
                if retention != Retention::History { publications.retain(|r| *r == published); }
            }
            drop(store);
            store = Store::open_components(&path).unwrap();
            proptest::prop_assert_eq!(store.head(OBJECT), head);
            proptest::prop_assert_eq!(store.published(OBJECT), published);
            proptest::prop_assert_eq!(store.entries.len(), revisions.len());
            for (&revision, expected) in &revisions {
                let actual = store.component_revision(OBJECT, revision).unwrap().components().map(|(id, bytes)| (id, bytes.to_vec())).collect::<BTreeMap<_, _>>();
                proptest::prop_assert_eq!(&actual, expected);
            }
        }
    }
}
