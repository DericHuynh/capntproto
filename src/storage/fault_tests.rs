use super::{
    io::faults::{self, Action},
    *,
};
use crate::semantics::Revision;

fn seed(path: &Path) {
    let mut store = Store::open(path).unwrap();
    store
        .put(ObjectKey::new(1), Revision::INITIAL, b"old")
        .unwrap();
    store
        .publish(ObjectKey::new(1), Revision::new(1), Revision::INITIAL)
        .unwrap();
}
fn batch(store: &mut Store) -> Result<Vec<Revision>> {
    store.commit(&[
        Update {
            object: ObjectKey::new(1),
            expected_head: Revision::new(1),
            expected_published: Some(Revision::new(1)),
            value: b"new",
        },
        Update {
            object: ObjectKey::new(2),
            expected_head: Revision::INITIAL,
            expected_published: Some(Revision::INITIAL),
            value: b"receipt",
        },
    ])
}
fn assert_batch(store: &Store, complete: bool) {
    assert_eq!(
        store.head(ObjectKey::new(1)),
        Revision::new(if complete { 2 } else { 1 })
    );
    assert_eq!(
        store.published(ObjectKey::new(1)),
        Revision::new(if complete { 2 } else { 1 })
    );
    assert_eq!(
        store.head(ObjectKey::new(2)),
        Revision::new(u64::from(complete))
    );
    assert_eq!(
        store.published(ObjectKey::new(2)),
        Revision::new(u64::from(complete))
    );
    assert_eq!(
        store.get(ObjectKey::new(1)).unwrap().bytes(),
        if complete { b"new" } else { b"old" }
    );
    if complete {
        assert_eq!(store.get(ObjectKey::new(2)).unwrap().bytes(), b"receipt");
    }
}

#[test]
fn every_record_header_bit_is_checked_before_any_tail_repair() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("original");
    seed(&original);
    let mut store = Store::open(&original).unwrap();
    batch(&mut store).unwrap();
    drop(store);
    let good = fs::read(&original).unwrap();
    let mut offsets = Vec::new();
    let mut start = HEADER;
    while start < good.len() {
        offsets.push(start);
        start += record_size(u64_at(&good, start + 32) as usize).unwrap().1;
    }
    assert_eq!(offsets.len(), 3); // entry, publication, atomic batch
    let path = dir.path().join("corrupt");
    for start in offsets {
        for byte in 0..RECORD {
            for bit in 0..8 {
                let mut bad = good.clone();
                bad[start + byte] ^= 1 << bit;
                fs::write(&path, &bad).unwrap();
                assert!(
                    matches!(Store::open(&path), Err(Error::Corrupt(_))),
                    "header {start} byte {byte} bit {bit}"
                );
                assert_eq!(fs::read(&path).unwrap(), bad, "corrupt input was rewritten");
            }
        }
    }
}

#[test]
fn recovery_syncs_data_then_directory_even_without_a_torn_tail() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    seed(&path);
    let _guard = faults::install([]);
    let store = Store::open(&path).unwrap();
    assert_eq!(
        faults::visits(),
        vec![
            Point::RecoverySync,
            Point::RecoveryDirectorySync,
            Point::Recovered
        ]
    );
    assert_batch(&store, false);
}

#[test]
fn failed_recovery_barriers_never_return_a_serving_store_and_release_locks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    seed(&path);
    for point in [Point::RecoverySync, Point::RecoveryDirectorySync] {
        let guard = faults::install([(point, Action::Error(5))]); // EIO on Linux
        assert!(matches!(Store::open(&path), Err(Error::Io(_))));
        assert!(!faults::visits().contains(&Point::Recovered));
        drop(guard);
        assert_batch(&Store::open(&path).unwrap(), false);
    }
}

#[test]
fn short_and_interrupted_writes_retry_and_publish_one_atomic_batch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    seed(&path);
    let mut store = Store::open(&path).unwrap();
    let guard = faults::install([
        (Point::AppendWrite, Action::Short(9)),
        (Point::AppendWrite, Action::Error(4)), // EINTR
        (Point::AppendWrite, Action::Short(3)),
    ]);
    batch(&mut store).unwrap();
    assert_batch(&store, true);
    drop(guard);
    drop(store);
    assert_batch(&Store::open(&path).unwrap(), true);
}

#[test]
fn partial_write_and_sync_errors_quarantine_until_all_or_none_recovery() {
    for (plan, complete) in [
        (vec![(Point::AppendWrite, Action::Short(0))], false),
        (
            vec![
                (Point::AppendWrite, Action::Short(87)),
                (Point::AppendWrite, Action::Error(28)),
            ],
            false,
        ), // ENOSPC
        (
            vec![
                (Point::AppendWrite, Action::Short(87)),
                (Point::AppendWrite, Action::Error(5)),
            ],
            false,
        ),
        (vec![(Point::AppendSync, Action::Error(5))], true),
        (vec![(Point::AppendSynced, Action::Error(5))], true),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("objects");
        seed(&path);
        let mut store = Store::open(&path).unwrap();
        let cursor = store.publication_cursor(ObjectKey::new(1), 0).unwrap();
        let guard = faults::install(plan);
        assert!(matches!(batch(&mut store), Err(Error::Io(_))));
        assert!(store.poisoned);
        assert!(store.get(ObjectKey::new(1)).is_err());
        assert!(matches!(
            store.publication_after(&cursor),
            Err(Error::Corrupt(_))
        ));
        assert!(matches!(
            store.publication_cursor(ObjectKey::new(1), 0),
            Err(Error::Corrupt(_))
        ));
        assert!(batch(&mut store).is_err());
        drop(guard);
        drop(store);
        let recovered = Store::open(&path).unwrap();
        assert!(matches!(
            recovered.publication_after(&cursor),
            Err(Error::ForeignCursor)
        ));
        assert_batch(&recovered, complete);
        drop(recovered);
        assert_batch(&Store::open(&path).unwrap(), complete);
    }
}

#[test]
fn actual_compaction_io_errors_preserve_a_complete_generation() {
    for point in [
        Point::CompactWrite,
        Point::CompactSync,
        Point::CompactRename,
        Point::CompactDirectorySync,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("objects");
        seed(&path);
        let mut store = Store::open(&path).unwrap();
        batch(&mut store).unwrap();
        let held = store.get(ObjectKey::new(1)).unwrap();
        let guard = faults::install([(point, Action::Error(5))]);
        assert!(matches!(
            store.compact(Retention::Latest),
            Err(Error::Io(_))
        ));
        assert_eq!(held.bytes(), b"new");
        if matches!(point, Point::CompactRename | Point::CompactDirectorySync) {
            assert!(store.poisoned);
        }
        drop(guard);
        drop(store);
        drop(held);
        assert_batch(&Store::open(&path).unwrap(), true);
    }
}

fn child(path: &Path, scenario: &str) {
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "storage::fault_tests::crash_worker",
            "--nocapture",
        ])
        .env("CAPNTPROTO_STORAGE_CRASH_PATH", path)
        .env("CAPNTPROTO_STORAGE_CRASH_SCENARIO", scenario)
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(86), "{scenario}");
}

#[test]
#[ignore = "run by the storage recovery gate in isolation; fork temporarily inherits other tests' file locks"]
fn child_process_crashes_preserve_batch_atomicity_across_recovery_and_second_crash() {
    for scenario in [
        "partial",
        "appended",
        "synced",
        "checkpoint",
        "renamed",
        "directory",
        "acknowledged",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("objects");
        seed(&path);
        child(&path, scenario);
        // Crash again immediately after recovery's durability barriers, before
        // a caller sees a Store or any further mutation is issued.
        child(&path, "recovered");
        assert_batch(&Store::open(&path).unwrap(), scenario != "partial");
    }
}

#[test]
fn crash_worker() {
    let Some(path) = std::env::var_os("CAPNTPROTO_STORAGE_CRASH_PATH") else {
        return;
    };
    let scenario = std::env::var("CAPNTPROTO_STORAGE_CRASH_SCENARIO").unwrap();
    if scenario == "recovered" {
        let _guard = faults::install([(Point::Recovered, Action::Crash)]);
        let _ = Store::open(path).unwrap();
        panic!("recovery fault was not reached");
    }
    let mut store = Store::open(path).unwrap();
    let plan = match scenario.as_str() {
        "partial" => vec![
            (Point::AppendWrite, Action::Short(87)),
            (Point::AppendWrite, Action::Crash),
        ],
        "appended" => vec![(Point::Appended, Action::Crash)],
        "synced" => vec![(Point::AppendSynced, Action::Crash)],
        "checkpoint" => vec![(Point::CompactSynced, Action::Crash)],
        "renamed" => vec![(Point::CompactRenamed, Action::Crash)],
        "directory" => vec![(Point::CompactComplete, Action::Crash)],
        "acknowledged" => vec![],
        _ => panic!("unknown crash scenario"),
    };
    let _guard = faults::install(plan);
    batch(&mut store).unwrap();
    if scenario == "acknowledged" {
        std::process::exit(86);
    }
    store.compact(Retention::Latest).unwrap();
    panic!("crash fault was not reached");
}

#[tokio::test(flavor = "current_thread")]
async fn uncertain_and_acknowledged_revocations_survive_recovery_without_new_authority() {
    use crate::{
        authority::{ObjectGeneration, ObjectId, Rights},
        persistence::{Descriptor, ObjectKind, Persistent, Realm, SturdyRef},
    };
    use capnp::capability::{FromClientHook, Promise};
    use capntproto_test_support::runtime_test_capnp::harness;
    use std::rc::Rc;
    struct Target;
    impl harness::Server for Target {}
    fn target() -> harness::Client {
        capnp_rpc::new_client(Target)
    }
    fn factory(realm: &Realm) {
        realm
            .register_factory(
                ObjectKind::new(1).unwrap(),
                Rc::new(|_: &Descriptor, _| Promise::ok(target().client)),
            )
            .unwrap();
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            for (plan, revoked) in [
                (
                    vec![
                        (Point::AppendWrite, Action::Short(87)),
                        (Point::AppendWrite, Action::Error(28)),
                    ],
                    false,
                ),
                (vec![(Point::AppendSync, Action::Error(5))], true),
                (vec![(Point::AppendSynced, Action::Error(5))], true),
                (vec![], true),
            ] {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("realm");
                let (executor, driver) = capnp_rpc::new_call_executor();
                let task = tokio::task::spawn_local(driver);
                let realm = Realm::open_with_executor(
                    &path,
                    crate::persistence::Limits::default(),
                    executor.clone(),
                )
                .unwrap();
                realm.register_owner([1; 16], [2; 32]).unwrap();
                factory(&realm);
                let cap = realm
                    .persistent(
                        target(),
                        Descriptor::new(
                            ObjectKind::new(1).unwrap(),
                            ObjectId::new(1).unwrap(),
                            ObjectGeneration::new(1).unwrap(),
                            Rights::ALL,
                        ),
                        |_| Ok(()),
                    )
                    .unwrap();
                let persistent = Persistent::new(cap.as_client_hook().add_ref());
                let mut request = persistent.save_request();
                request.get().init_seal_for().set_id(&[1; 16]);
                let reference = SturdyRef::read(
                    request
                        .send()
                        .promise
                        .await
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_sturdy_ref()
                        .unwrap(),
                )
                .unwrap();
                let fails = !plan.is_empty();
                let guard = faults::install(plan);
                assert_eq!(realm.revoke(&reference).is_err(), fails);
                assert!(realm.restore(reference.clone(), [2; 32]).await.is_err());
                drop(guard);
                realm.close();
                let recovered = Realm::open_with_executor(
                    &path,
                    crate::persistence::Limits::default(),
                    executor,
                )
                .unwrap();
                factory(&recovered);
                assert_eq!(
                    recovered.restore(reference, [2; 32]).await.is_err(),
                    revoked
                );
                recovered.close();
                task.abort();
            }
        })
        .await;
}
