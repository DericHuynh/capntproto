#![cfg(feature = "storage")]
use capntproto::storage::{
    worker::{AdmissionError, Client, Config, Error, Format, Options, ShutdownMode, Worker},
    ComponentId, ComponentUpdate, Limits, ObjectKey, Retention, Revision, Store, Update,
};

const A: ObjectKey = ObjectKey::new(1);
const B: ObjectKey = ObjectKey::new(2);
const OPTIONS: Options = Options { deadline: None };

#[tokio::test]
async fn whole_entry_batches_conflicts_history_and_snapshot_lifetimes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    let worker = Worker::open(
        &path,
        Format::WholeEntry,
        Limits::default(),
        Config::default(),
    )
    .await
    .unwrap();
    let client = worker.client(1);
    let batch = |head: Revision| {
        [
            Update {
                object: A,
                expected_head: head,
                expected_published: Some(head),
                value: b"a",
            },
            Update {
                object: B,
                expected_head: head,
                expected_published: Some(head),
                value: b"b",
            },
        ]
    };
    assert_eq!(
        client
            .try_commit(&batch(Revision::INITIAL), OPTIONS)
            .unwrap()
            .await
            .unwrap(),
        [Revision::new(1); 2]
    );
    let old = client.try_get(A, OPTIONS).unwrap().await.unwrap();
    assert_eq!(old.bytes(), b"a");
    assert!(matches!(
        client
            .try_commit(&batch(Revision::INITIAL), OPTIONS)
            .unwrap()
            .await,
        Err(Error::NotApplied(capntproto::storage::Error::Conflict))
    ));
    client
        .try_put(A, Revision::new(1), b"new", OPTIONS)
        .unwrap()
        .await
        .unwrap();
    client
        .try_publish(A, Revision::new(2), Revision::new(1), OPTIONS)
        .unwrap()
        .await
        .unwrap();
    let cursor = client
        .try_publication_cursor(A, Revision::INITIAL, OPTIONS)
        .unwrap()
        .await
        .unwrap();
    let first = client
        .try_publication_after(&cursor, OPTIONS)
        .unwrap()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.snapshot().bytes(), b"a");
    let next = client
        .try_publication_after(first.cursor(), OPTIONS)
        .unwrap()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next.snapshot().bytes(), b"new");
    drop(first);
    drop(next);
    client
        .try_compact(Retention::Latest, OPTIONS)
        .unwrap()
        .await
        .unwrap();
    assert!(matches!(
        client
            .try_publication_after(&cursor, OPTIONS)
            .unwrap()
            .await,
        Err(Error::NotApplied(
            capntproto::storage::Error::HistoryExpired { .. }
        ))
    ));
    assert!(matches!(
        client
            .try_revision(A, Revision::new(1), OPTIONS)
            .unwrap()
            .await,
        Err(Error::NotApplied(capntproto::storage::Error::NotFound))
    ));
    assert_eq!(old.bytes(), b"a");
    assert_eq!(
        client.try_status(B, OPTIONS).unwrap().await.unwrap().head,
        Revision::new(1)
    );
    let report = worker.shutdown(ShutdownMode::Drain).wait().await;
    assert!(!report.degraded);
    assert!(Store::open(&path).is_err()); // The caller's snapshot still owns its lock.
    drop(old);
    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.get(A).unwrap().bytes(), b"new");
    assert_eq!(reopened.get(B).unwrap().bytes(), b"b");
}

#[tokio::test]
async fn components_preserve_atomic_cas_references_retention_and_delete_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    let worker = Worker::open(
        &path,
        Format::Components,
        Limits::default(),
        Config::default(),
    )
    .await
    .unwrap();
    let client = worker.client(7);
    let hot = ComponentId::new(1);
    let cold = ComponentId::new(2);
    let seed = [
        ComponentUpdate {
            id: hot,
            value: Some(b"old"),
        },
        ComponentUpdate {
            id: cold,
            value: Some(b"shared"),
        },
    ];
    client
        .try_edit_components(
            A,
            Revision::INITIAL,
            Some(Revision::INITIAL),
            &seed,
            OPTIONS,
        )
        .unwrap()
        .await
        .unwrap();
    let old = client
        .try_component_revision(A, Revision::new(1), OPTIONS)
        .unwrap()
        .await
        .unwrap();
    let edit = [ComponentUpdate {
        id: hot,
        value: Some(b"new"),
    }];
    let first = client
        .try_edit_components(A, Revision::new(1), Some(Revision::new(1)), &edit, OPTIONS)
        .unwrap();
    let conflict = client
        .try_edit_components(A, Revision::new(1), Some(Revision::new(1)), &edit, OPTIONS)
        .unwrap();
    assert_eq!(first.await.unwrap(), Revision::new(2));
    assert!(matches!(
        conflict.await,
        Err(Error::NotApplied(capntproto::storage::Error::Conflict))
    ));
    let current = client
        .try_get_components(A, OPTIONS)
        .unwrap()
        .await
        .unwrap();
    assert_eq!(current.get(hot).unwrap(), b"new");
    assert_eq!(current.get(cold).unwrap(), b"shared");
    let cursor = client
        .try_publication_cursor(A, Revision::new(1), OPTIONS)
        .unwrap()
        .await
        .unwrap();
    let next = client
        .try_component_publication_after(&cursor, OPTIONS)
        .unwrap()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next.snapshot().revision(), Revision::new(2));
    assert!(matches!(
        client.try_get(A, OPTIONS).unwrap().await,
        Err(Error::NotApplied(capntproto::storage::Error::Layout))
    ));
    client
        .try_edit_components(
            A,
            Revision::new(2),
            Some(Revision::new(2)),
            &[ComponentUpdate {
                id: hot,
                value: None,
            }],
            OPTIONS,
        )
        .unwrap()
        .await
        .unwrap();
    client
        .try_compact(Retention::Latest, OPTIONS)
        .unwrap()
        .await
        .unwrap();
    assert_eq!(old.get(hot).unwrap(), b"old");
    assert_eq!(old.get(cold).unwrap(), b"shared");
    drop((old, current, next));
    worker.shutdown(ShutdownMode::Drain).wait().await;
    let store = Store::open_components(&path).unwrap();
    let current = store.get_components(A).unwrap();
    assert!(current.get(hot).is_none());
    assert_eq!(current.get(cold).unwrap(), b"shared");
}

#[tokio::test]
async fn storage_limits_are_execution_failures_without_quarantining_admission() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("objects")).unwrap();
    let worker = Worker::from_store(store, Config::default()).await.unwrap();
    let client = worker.client(1);
    let limits = Limits {
        max_entry_bytes: 1,
        ..Limits::default()
    };
    client
        .try_set_limits(limits, OPTIONS)
        .unwrap()
        .await
        .unwrap();
    assert!(matches!(
        client
            .try_put(A, Revision::INITIAL, b"too long", OPTIONS)
            .unwrap()
            .await,
        Err(Error::NotApplied(capntproto::storage::Error::Limit))
    ));
    assert!(matches!(
        client.try_commit(&[], OPTIONS),
        Err(AdmissionError::InvalidRequest)
    ));
    client
        .try_put(A, Revision::INITIAL, b"x", OPTIONS)
        .unwrap()
        .await
        .unwrap();
    worker.shutdown(ShutdownMode::Drain).wait().await;
    assert!(!worker.diagnostics().degraded);
}

#[test]
fn public_handles_and_outcomes_can_cross_thread_boundaries() {
    fn send_sync<T: Send + Sync>() {}
    fn send<T: Send>() {}
    send_sync::<Client>();
    send_sync::<Worker>();
    send::<capntproto::storage::worker::Pending<Revision>>();
    send::<Error>();
}
