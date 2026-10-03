use capntproto::storage::Revision;
use capntproto::storage::*;
use std::io::Write;
#[test]
fn revisions_snapshots_recovery() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("objects");
    let mut s = Store::open(&p).unwrap();
    assert!(Store::open(&p).is_err());
    let r = s
        .put(ObjectKey::new(7), Revision::INITIAL, b"first")
        .unwrap();
    assert!(s.get(ObjectKey::new(7)).is_err());
    s.publish(ObjectKey::new(7), r, Revision::INITIAL).unwrap();
    let old = s.get(ObjectKey::new(7)).unwrap();
    let r = s
        .put(ObjectKey::new(7), Revision::new(1), b"replacement")
        .unwrap();
    s.publish(ObjectKey::new(7), r, Revision::new(1)).unwrap();
    assert_eq!(old.bytes(), b"first");
    assert_eq!(s.get(ObjectKey::new(7)).unwrap().bytes(), b"replacement");
    assert!(s
        .put(ObjectKey::new(7), Revision::INITIAL, b"stale")
        .is_err());
    drop(s);
    assert!(Store::open(&p).is_err());
    drop(old);
    let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
    f.write_all(b"RPENT").unwrap();
    drop(f);
    let s = Store::open(&p).unwrap();
    assert_eq!(s.get(ObjectKey::new(7)).unwrap().bytes(), b"replacement");
    assert_eq!(s.head(ObjectKey::new(7)), Revision::new(2));
}
#[test]
fn every_torn_tail_recovers_previous_publication() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("source");
    let mut s = Store::open(&p).unwrap();
    s.put(ObjectKey::new(1), Revision::INITIAL, b"old").unwrap();
    s.publish(ObjectKey::new(1), Revision::new(1), Revision::INITIAL)
        .unwrap();
    let before = std::fs::metadata(&p).unwrap().len() as usize;
    s.put(ObjectKey::new(1), Revision::new(1), b"new version")
        .unwrap();
    s.publish(ObjectKey::new(1), Revision::new(2), Revision::new(1))
        .unwrap();
    drop(s);
    let bytes = std::fs::read(&p).unwrap();
    for n in before..bytes.len() {
        let q = d.path().join("cut");
        std::fs::write(&q, &bytes[..n]).unwrap();
        let s = Store::open(q).unwrap();
        assert_eq!(s.get(ObjectKey::new(1)).unwrap().bytes(), b"old", "cut {n}");
    }
}
#[test]
fn committed_corruption_fails_closed() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("objects");
    let mut s = Store::open(&p).unwrap();
    s.put(ObjectKey::new(1), Revision::INITIAL, b"value")
        .unwrap();
    drop(s);
    let mut b = std::fs::read(&p).unwrap();
    b[144] ^= 1;
    std::fs::write(&p, b).unwrap();
    assert!(matches!(Store::open(p), Err(Error::Corrupt(_))));
}

#[test]
fn compaction_retains_publication_drafts_and_mapped_generations() {
    for retention in [Retention::Publishable, Retention::Latest] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("objects");
        let mut store = Store::open(&path).unwrap();
        for revision in 1..=4 {
            store
                .put(
                    ObjectKey::new(7),
                    Revision::new(revision - 1),
                    &[revision as u8; 8],
                )
                .unwrap();
        }
        store
            .publish(ObjectKey::new(7), Revision::new(2), Revision::INITIAL)
            .unwrap();
        store
            .put(ObjectKey::new(9), Revision::INITIAL, b"other")
            .unwrap();
        let retired = store.revision(ObjectKey::new(7), Revision::new(1)).unwrap();
        let published = store.get(ObjectKey::new(7)).unwrap();
        let before = store.file_bytes();
        let result = store.compact(retention).unwrap();
        assert_eq!(result.before_bytes, before);
        assert!(result.after_bytes < before);
        assert_eq!(
            result.removed_revisions,
            if retention == Retention::Latest { 2 } else { 1 }
        );
        assert_eq!(store.head(ObjectKey::new(7)), Revision::new(4));
        assert_eq!(store.published(ObjectKey::new(7)), Revision::new(2));
        assert!(matches!(
            store.revision(ObjectKey::new(7), Revision::new(1)),
            Err(Error::NotFound)
        ));
        assert_eq!(retired.bytes(), &[1; 8]);
        assert_eq!(published.bytes(), &[2; 8]);
        assert_eq!(
            store.get(ObjectKey::new(7)).unwrap().bytes(),
            published.bytes()
        );
        assert_eq!(
            store
                .revision(ObjectKey::new(9), Revision::new(1))
                .unwrap()
                .bytes(),
            b"other"
        );
        assert!(Store::open(&path).is_err());
        if retention == Retention::Latest {
            assert!(matches!(
                store.publish(ObjectKey::new(7), Revision::new(3), Revision::new(2)),
                Err(Error::NotFound)
            ));
        } else {
            store
                .publish(ObjectKey::new(7), Revision::new(3), Revision::new(2))
                .unwrap();
        }
        store
            .publish(
                ObjectKey::new(7),
                Revision::new(4),
                store.published(ObjectKey::new(7)),
            )
            .unwrap();
        assert_eq!(
            store
                .put(ObjectKey::new(7), Revision::new(4), b"next")
                .unwrap(),
            Revision::new(5)
        );
        store.compact(retention).unwrap();
        drop(store);
        assert!(Store::open(&path).is_err()); // Stable lock spans every old generation.
        assert_eq!(retired.bytes(), &[1; 8]);
        drop(retired);
        assert!(Store::open(&path).is_err());
        drop(published);
        let mut store = Store::open(&path).unwrap();
        assert_eq!(store.head(ObjectKey::new(7)), Revision::new(5));
        assert_eq!(store.published(ObjectKey::new(7)), Revision::new(4));
        assert_eq!(store.get(ObjectKey::new(7)).unwrap().bytes(), &[4; 8]);
        assert_eq!(
            store
                .revision(ObjectKey::new(7), Revision::new(5))
                .unwrap()
                .bytes(),
            b"next"
        );
        assert_eq!(
            store
                .put(ObjectKey::new(7), Revision::new(5), b"after reopen")
                .unwrap(),
            Revision::new(6)
        );
    }
}

#[test]
fn quota_rejection_growth_and_compaction_are_recoverable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    let limits = Limits {
        max_entry_bytes: 8,
        max_file_bytes: 264,
    };
    let mut store = Store::open_with_limits(&path, limits).unwrap();
    store
        .put(ObjectKey::new(1), Revision::INITIAL, b"12345678")
        .unwrap();
    store
        .publish(ObjectKey::new(1), Revision::new(1), Revision::INITIAL)
        .unwrap();
    assert_eq!(store.file_bytes(), limits.max_file_bytes);
    let before = std::fs::read(&path).unwrap();
    assert!(matches!(
        store.put(ObjectKey::new(1), Revision::new(1), b"value"),
        Err(Error::Limit)
    ));
    assert!(matches!(
        store.put(ObjectKey::new(1), Revision::new(1), b"123456789"),
        Err(Error::Limit)
    ));
    assert!(store
        .set_limits(Limits {
            max_file_bytes: 263,
            ..limits
        })
        .is_err());
    assert!(store
        .set_limits(Limits {
            max_entry_bytes: 7,
            ..limits
        })
        .is_err());
    assert_eq!(store.limits(), limits);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let larger = Limits {
        max_file_bytes: 1024,
        ..limits
    };
    store.set_limits(larger).unwrap();
    for revision in 2..=4 {
        store
            .put(ObjectKey::new(1), Revision::new(revision - 1), b"updated")
            .unwrap();
        store
            .publish(
                ObjectKey::new(1),
                Revision::new(revision),
                Revision::new(revision - 1),
            )
            .unwrap();
    }
    assert!(store.file_bytes() > limits.max_file_bytes);
    drop(store);
    assert!(matches!(
        Store::open_with_limits(&path, limits),
        Err(Error::Limit)
    ));
    let mut store = Store::open_with_limits(&path, larger).unwrap();
    store.compact(Retention::Publishable).unwrap();
    assert_eq!(store.file_bytes(), 264);
    store.set_limits(limits).unwrap();
    drop(store);
    assert_eq!(
        Store::open_with_limits(&path, limits)
            .unwrap()
            .head(ObjectKey::new(1)),
        Revision::new(4)
    );
    assert!(Store::open_with_limits(
        dir.path().join("invalid"),
        Limits {
            max_file_bytes: usize::MAX,
            ..limits
        }
    )
    .is_err());
    assert!(!dir.path().join("invalid").exists());
}

#[test]
fn checkpoints_fail_closed_when_truncated_and_recover_later_torn_appends() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    let mut store = Store::open(&path).unwrap();
    for revision in 1..=3 {
        store
            .put(
                ObjectKey::new(1),
                Revision::new(revision - 1),
                &[revision as u8; 8],
            )
            .unwrap();
    }
    store
        .publish(ObjectKey::new(1), Revision::new(2), Revision::INITIAL)
        .unwrap();
    store.compact(Retention::Latest).unwrap();
    let checkpoint = store.file_bytes();
    store
        .put(ObjectKey::new(1), Revision::new(3), b"append")
        .unwrap();
    store
        .publish(ObjectKey::new(1), Revision::new(4), Revision::new(2))
        .unwrap();
    drop(store);
    let bytes = std::fs::read(&path).unwrap();
    let cut = dir.path().join("cut");
    for n in 1..checkpoint {
        std::fs::write(&cut, &bytes[..n]).unwrap();
        assert!(
            Store::open(&cut).is_err(),
            "accepted incomplete checkpoint {n}"
        );
        assert_eq!(std::fs::read(&cut).unwrap(), bytes[..n]);
    }
    for n in checkpoint..bytes.len() {
        std::fs::write(&cut, &bytes[..n]).unwrap();
        let recovered = Store::open(&cut).unwrap();
        assert_eq!(
            recovered.published(ObjectKey::new(1)),
            Revision::new(2),
            "cut {n}"
        );
        assert_eq!(recovered.get(ObjectKey::new(1)).unwrap().bytes(), &[2; 8]);
        assert!(
            recovered.head(ObjectKey::new(1)) == Revision::new(3)
                || recovered.head(ObjectKey::new(1)) == Revision::new(4)
        );
    }
    for i in [0, 8, 16, 24, 32, 64, 104, 144, checkpoint - 1] {
        let mut corrupt = bytes[..checkpoint].to_vec();
        corrupt[i] ^= 1;
        std::fs::write(&cut, &corrupt).unwrap();
        assert!(
            Store::open(&cut).is_err(),
            "accepted corrupt checkpoint byte {i}"
        );
        assert_eq!(std::fs::read(&cut).unwrap(), corrupt);
    }
}

#[cfg(unix)]
#[test]
fn compaction_resolves_symlinks_and_refuses_hard_link_aliases() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    let alias = dir.path().join("alias");
    let mut store = Store::open(&path).unwrap();
    store
        .put(ObjectKey::new(1), Revision::INITIAL, b"original")
        .unwrap();
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    assert!(Store::open(&alias).is_err());
    drop(store);
    let mut store = Store::open(&alias).unwrap();
    store.compact(Retention::Latest).unwrap();
    assert!(alias.is_symlink());
    assert!(Store::open(&path).is_err());
    std::fs::hard_link(&path, dir.path().join("hard")).unwrap();
    assert!(matches!(
        store.compact(Retention::Latest),
        Err(Error::LinkedFile)
    ));
    // Rejected before replacement; the writer remains usable.
    store
        .put(ObjectKey::new(1), Revision::new(1), b"still usable")
        .unwrap();
}

#[test]
fn replay_tlc_storage_compaction_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_STORAGE_COMPACTION_TRACES")
        .expect("prepare verified trace corpus");
    #[derive(serde::Deserialize)]
    struct Step {
        action: String,
        state: Vec<u64>,
    }
    #[derive(serde::Deserialize)]
    struct Case {
        latest: bool,
        steps: Vec<Step>,
    }
    let cases: Vec<Case> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for (index, case) in cases.iter().enumerate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("objects");
        let mut limits = Limits {
            max_entry_bytes: 8,
            max_file_bytes: 464,
        };
        let mut store = Some(Store::open_with_limits(&path, limits).unwrap());
        let mut snapshot: Option<Snapshot> = None;
        for step in &case.steps {
            let s = &step.state;
            match step.action.as_str() {
                "put" => {
                    store
                        .as_mut()
                        .unwrap()
                        .put(
                            ObjectKey::new(1),
                            Revision::new(s[0] - 1),
                            &s[0].to_le_bytes(),
                        )
                        .unwrap();
                }
                "publish" => {
                    let store = store.as_mut().unwrap();
                    store
                        .publish(
                            ObjectKey::new(1),
                            Revision::new(s[1]),
                            store.published(ObjectKey::new(1)),
                        )
                        .unwrap();
                }
                "compact" => {
                    let store = store.as_mut().unwrap();
                    let before = store.file_bytes();
                    let result = store
                        .compact(if case.latest {
                            Retention::Latest
                        } else {
                            Retention::Publishable
                        })
                        .unwrap();
                    assert_eq!(result.before_bytes, before);
                    assert_eq!(result.after_bytes as u64, s[5]);
                }
                "capture" => {
                    snapshot = Some(store.as_ref().unwrap().get(ObjectKey::new(1)).unwrap())
                }
                "drop" => snapshot = None,
                "close" => store = None,
                "reopen" => store = Some(Store::open_with_limits(&path, limits).unwrap()),
                "grow" => {
                    limits.max_file_bytes = s[6] as usize;
                    store.as_mut().unwrap().set_limits(limits).unwrap();
                }
                "reject" => {
                    let before = std::fs::read(&path).unwrap();
                    let result = store.as_mut().unwrap().put(
                        ObjectKey::new(1),
                        Revision::new(s[0]),
                        &(s[0] + 1).to_le_bytes(),
                    );
                    assert!(matches!(result, Err(Error::Limit)));
                    assert_eq!(std::fs::read(&path).unwrap(), before);
                }
                action => panic!("unknown action {action}"),
            }
            assert_eq!(u64::from(store.is_some()), s[7]);
            assert_eq!(snapshot.as_ref().map_or(0, |v| v.revision().get()), s[8]);
            if let Some(snapshot) = &snapshot {
                assert_eq!(snapshot.bytes(), &s[9].to_le_bytes());
            }
            assert_eq!(std::fs::metadata(&path).unwrap().len(), s[5]);
            if store.is_some() || snapshot.is_some() {
                assert!(Store::open_with_limits(&path, limits).is_err());
            }
            if let Some(store) = &store {
                assert_eq!(
                    store.head(ObjectKey::new(1)),
                    Revision::new(s[0]),
                    "case {index} {}",
                    step.action
                );
                assert_eq!(store.published(ObjectKey::new(1)), Revision::new(s[1]));
                assert_eq!(store.file_bytes() as u64, s[5]);
                assert_eq!(store.limits().max_file_bytes as u64, s[6]);
                for r in 1..=3 {
                    match store.revision(ObjectKey::new(1), Revision::new(r)) {
                        Ok(value) => {
                            assert_eq!(s[r as usize + 1], 1);
                            assert_eq!(value.bytes(), &r.to_le_bytes());
                        }
                        Err(Error::NotFound) => assert_eq!(s[r as usize + 1], 0),
                        Err(e) => panic!("case {index}: {e}"),
                    }
                }
            }
        }
        drop(snapshot);
        drop(store);
        let recovered = Store::open_with_limits(&path, limits).unwrap();
        let s = &case.steps.last().unwrap().state;
        assert_eq!(recovered.head(ObjectKey::new(1)), Revision::new(s[0]));
        assert_eq!(recovered.published(ObjectKey::new(1)), Revision::new(s[1]));
        for r in 1..=3 {
            assert_eq!(
                recovered
                    .revision(ObjectKey::new(1), Revision::new(r))
                    .is_ok(),
                s[r as usize + 1] == 1
            );
        }
    }
}

#[test]
fn configured_store_grows_beyond_original_entry_and_file_limits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large");
    let limits = Limits {
        max_entry_bytes: 18 * 1024 * 1024,
        max_file_bytes: 320 * 1024 * 1024,
    };
    let mut store = Store::open_with_limits(&path, limits).unwrap();
    let payload = vec![0x5a; 17 * 1024 * 1024];
    for revision in 1..=16 {
        store
            .put(ObjectKey::new(1), Revision::new(revision - 1), &payload)
            .unwrap();
    }
    assert!(store.file_bytes() > 256 * 1024 * 1024);
    drop(store);
    assert!(matches!(Store::open(&path), Err(Error::Limit)));
    let mut store = Store::open_with_limits(&path, limits).unwrap();
    assert_eq!(store.head(ObjectKey::new(1)), Revision::new(16));
    assert_eq!(
        store
            .revision(ObjectKey::new(1), Revision::new(16))
            .unwrap()
            .bytes(),
        payload
    );
    let result = store.compact(Retention::Latest).unwrap();
    assert_eq!(result.removed_revisions, 15);
    assert!(result.after_bytes < result.before_bytes / 8);
    drop(store);
    let store = Store::open_with_limits(&path, limits).unwrap();
    assert_eq!(store.head(ObjectKey::new(1)), Revision::new(16));
    assert_eq!(
        store
            .revision(ObjectKey::new(1), Revision::new(16))
            .unwrap()
            .bytes(),
        payload
    );
}

#[test]
fn obsolete_storage_headers_are_rejected_without_rewriting() {
    let dir = tempfile::tempdir().unwrap();
    for version in [1u64, 2, 3] {
        let path = dir.path().join(format!("obsolete-{version}"));
        let mut store = Store::open(&path).unwrap();
        store
            .put(ObjectKey::new(1), Revision::INITIAL, b"payload")
            .unwrap();
        drop(store);
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[..8].copy_from_slice(format!("RPROTO{version:02}").as_bytes());
        bytes[8..16].copy_from_slice(&version.to_le_bytes());
        let checksum = ring::digest::digest(&ring::digest::SHA256, &bytes[..32]);
        bytes[32..64].copy_from_slice(checksum.as_ref());
        std::fs::write(&path, &bytes).unwrap();
        assert!(matches!(
            Store::open(&path),
            Err(Error::Corrupt("header/version"))
        ));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
