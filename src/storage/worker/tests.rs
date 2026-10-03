use super::*;
use crate::storage::io::{
    faults::{self, Action},
    Point,
};
use std::{sync::mpsc, time::Duration};

const OBJECT: ObjectKey = ObjectKey::new(1);
const OPTIONS: Options = Options { deadline: None };

struct Release(Option<mpsc::Sender<()>>);
impl Drop for Release {
    fn drop(&mut self) {
        if let Some(tx) = self.0.take() {
            let _ = tx.send(());
        }
    }
}
async fn pause(client: &Client) -> (Pending<()>, Release) {
    let (go_tx, go_rx) = mpsc::channel();
    let (started_tx, started_rx) = oneshot::channel();
    let request = client
        .submit(0, OPTIONS, || {
            move |_| {
                started_tx.send(()).unwrap();
                go_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                Ok(())
            }
        })
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), started_rx)
        .await
        .unwrap()
        .unwrap();
    (request, Release(Some(go_tx)))
}
async fn worker(config: Config) -> (tempfile::TempDir, Worker) {
    let dir = tempfile::tempdir().unwrap();
    let worker = Worker::open(
        dir.path().join("objects"),
        Format::WholeEntry,
        Limits::default(),
        config,
    )
    .await
    .unwrap();
    (dir, worker)
}
fn tiny() -> Config {
    Config {
        small_payload_bytes: 4,
        small: Budget {
            requests: 4,
            bytes: 8,
        },
        large: Budget {
            requests: 2,
            bytes: 32,
        },
        per_principal: Budget {
            requests: 2,
            bytes: 32,
        },
    }
}

#[tokio::test(flavor = "current_thread")]
async fn admission_bounds_preparation_bytes_principals_and_both_size_classes() {
    let (_dir, owner) = worker(tiny()).await;
    let (blocked, release) = pause(&owner.client(99)).await;
    let a = owner.client(1);
    let b = owner.client(2);
    let first = a
        .try_put(OBJECT, Revision::INITIAL, b"abcd", OPTIONS)
        .unwrap();
    let second = a
        .clone()
        .try_put(OBJECT, Revision::new(1), b"efgh", OPTIONS)
        .unwrap();
    assert!(matches!(
        owner.client(1).try_status(OBJECT, OPTIONS),
        Err(AdmissionError::Overloaded(Resource::PrincipalRequests))
    ));
    assert!(matches!(
        b.try_put(OBJECT, Revision::new(2), b"x", OPTIONS),
        Err(AdmissionError::Overloaded(Resource::Bytes))
    ));
    // Exhausted small bytes cannot take the large class's reserved budget.
    let large = b
        .try_put(OBJECT, Revision::new(2), &[7; 24], OPTIONS)
        .unwrap();
    assert!(matches!(
        b.try_put(OBJECT, Revision::new(3), &[8; 16], OPTIONS),
        Err(AdmissionError::Overloaded(Resource::PrincipalBytes))
    ));
    assert!(matches!(
        owner
            .client(3)
            .try_put(OBJECT, Revision::new(3), &[8; 16], OPTIONS),
        Err(AdmissionError::Overloaded(Resource::Bytes))
    ));
    assert!(matches!(
        a.try_put(OBJECT, Revision::INITIAL, &[0; 33], OPTIONS),
        Err(AdmissionError::TooLarge)
    ));
    assert!(matches!(
        a.submit::<(), _>(
            1,
            OPTIONS,
            || -> fn(&mut Store) -> super::super::Result<()> {
                panic!("preparation must not run without credit")
            }
        ),
        Err(AdmissionError::Overloaded(_))
    ));
    let stats = owner.diagnostics();
    assert_eq!(
        stats.small,
        Usage {
            requests: 3,
            bytes: 8
        }
    );
    assert_eq!(
        stats.large,
        Usage {
            requests: 1,
            bytes: 24
        }
    );
    drop(release);
    blocked.await.unwrap();
    assert_eq!(first.await.unwrap(), Revision::new(1));
    assert_eq!(second.await.unwrap(), Revision::new(2));
    assert_eq!(large.await.unwrap(), Revision::new(3));
    let report = owner.shutdown(ShutdownMode::Drain).wait().await;
    assert_eq!(report.succeeded, 4);
    assert_eq!(owner.diagnostics().small, Usage::default());
    assert_eq!(owner.diagnostics().large, Usage::default());
}

#[tokio::test]
async fn unread_replies_keep_credit_and_dropping_them_releases_it() {
    let mut config = tiny();
    config.small.requests = 2;
    let (_dir, owner) = worker(config).await;
    let client = owner.client(1);
    let first = client
        .try_put(OBJECT, Revision::INITIAL, b"data", OPTIONS)
        .unwrap();
    // FIFO status observation proves the first operation completed without consuming its reply.
    let status = client.try_status(OBJECT, OPTIONS).unwrap().await.unwrap();
    assert_eq!(status.head, Revision::new(1));
    assert_eq!(
        client.diagnostics().small,
        Usage {
            requests: 1,
            bytes: 4
        }
    );
    let second = client.try_status(OBJECT, OPTIONS).unwrap();
    assert!(matches!(
        client.try_status(OBJECT, OPTIONS),
        Err(AdmissionError::Overloaded(_))
    ));
    drop(first);
    let third = client.try_status(OBJECT, OPTIONS).unwrap();
    second.await.unwrap();
    third.await.unwrap();
    owner.shutdown(ShutdownMode::Drain).wait().await;
    assert_eq!(client.diagnostics().small, Usage::default());
}

#[tokio::test]
async fn large_request_saturation_preserves_small_slots_and_global_count_limits() {
    let mut config = tiny();
    config.small.requests = 2;
    let (_dir, owner) = worker(config).await;
    let (blocked, release) = pause(&owner.client(99)).await;
    let first = owner
        .client(1)
        .try_put(OBJECT, Revision::INITIAL, &[1; 16], OPTIONS)
        .unwrap();
    let second = owner
        .client(2)
        .try_put(OBJECT, Revision::new(1), &[2; 16], OPTIONS)
        .unwrap();
    assert!(matches!(
        owner
            .client(3)
            .try_put(OBJECT, Revision::new(2), &[3; 16], OPTIONS),
        Err(AdmissionError::Overloaded(Resource::Requests))
    ));
    let small = owner
        .client(3)
        .try_put(OBJECT, Revision::new(2), b"tiny", OPTIONS)
        .unwrap();
    assert!(matches!(
        owner.client(4).try_status(OBJECT, OPTIONS),
        Err(AdmissionError::Overloaded(Resource::Requests))
    ));
    drop(release);
    blocked.await.unwrap();
    first.await.unwrap();
    second.await.unwrap();
    small.await.unwrap();
    owner.shutdown(ShutdownMode::Drain).wait().await;
}

#[tokio::test]
async fn shutdown_during_preparation_rejects_enqueue_and_reclaims_reservation() {
    let (_dir, owner) = worker(tiny()).await;
    let client = owner.client(1);
    let (started_tx, started_rx) = oneshot::channel();
    let (go_tx, go_rx) = mpsc::channel();
    let release = Release(Some(go_tx));
    let (result_tx, result_rx) = oneshot::channel();
    thread::spawn(move || {
        let result = client.submit(4, OPTIONS, || {
            started_tx.send(()).unwrap();
            go_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            |store: &mut Store| store.put(OBJECT, Revision::INITIAL, b"data")
        });
        assert!(result_tx.send(result).is_ok());
    });
    started_rx.await.unwrap();
    assert_eq!(owner.diagnostics().small.bytes, 4);
    assert_eq!(
        owner.shutdown(ShutdownMode::Drain).wait().await.succeeded,
        0
    );
    drop(release);
    assert!(matches!(
        result_rx.await.unwrap(),
        Err(AdmissionError::Closed)
    ));
    assert_eq!(owner.diagnostics().small, Usage::default());
}

#[tokio::test]
async fn queued_cancellation_expiry_and_drop_never_execute() {
    let (_dir, owner) = worker(Config::default()).await;
    let client = owner.client(1);
    let (blocked, release) = pause(&client).await;
    let cancelled = client
        .try_put(OBJECT, Revision::INITIAL, b"cancel", OPTIONS)
        .unwrap();
    assert_eq!(cancelled.cancel(), Cancellation::Cancelled);
    assert_eq!(cancelled.cancel(), Cancellation::Cancelled);
    let expired = client
        .try_put(
            OBJECT,
            Revision::INITIAL,
            b"expire",
            Options {
                deadline: Some(Instant::now()),
            },
        )
        .unwrap();
    let dropped = client
        .try_put(OBJECT, Revision::INITIAL, b"drop", OPTIONS)
        .unwrap();
    drop(dropped);
    drop(release);
    blocked.await.unwrap();
    assert!(matches!(cancelled.await, Err(Error::Cancelled)));
    assert!(matches!(expired.await, Err(Error::Expired)));
    assert_eq!(
        client
            .try_status(OBJECT, OPTIONS)
            .unwrap()
            .await
            .unwrap()
            .head,
        Revision::INITIAL
    );
    let report = owner.shutdown(ShutdownMode::Drain).wait().await;
    assert_eq!(report.cancelled, 2);
    assert_eq!(report.expired, 1);
    assert_eq!(client.diagnostics().small, Usage::default());
}

#[tokio::test(flavor = "current_thread")]
async fn started_work_survives_reply_drop_without_blocking_the_executor() {
    let (dir, owner) = worker(Config::default()).await;
    let client = owner.client(1);
    let (go_tx, go_rx) = mpsc::channel();
    let release = Release(Some(go_tx));
    let (started_tx, started_rx) = oneshot::channel();
    let request = client
        .submit(4, OPTIONS, || {
            move |store| {
                started_tx.send(()).unwrap();
                go_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                store.put(OBJECT, Revision::INITIAL, b"data")
            }
        })
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), started_rx)
        .await
        .unwrap()
        .unwrap();
    // The current-thread runtime can still schedule timers while the owner is blocked.
    tokio::time::sleep(Duration::from_millis(1)).await;
    assert_eq!(request.cancel(), Cancellation::TooLate);
    drop(request);
    drop(release);
    let status = client.try_status(OBJECT, OPTIONS).unwrap().await.unwrap();
    assert_eq!(status.head, Revision::new(1));
    assert_eq!(client.diagnostics().lost_replies, 1);
    owner.shutdown(ShutdownMode::Drain).wait().await;
    assert_eq!(
        Store::open(dir.path().join("objects"))
            .unwrap()
            .head(OBJECT),
        Revision::new(1)
    );
}

#[tokio::test]
async fn drain_closes_admission_and_outlives_a_timed_out_observer() {
    let (dir, owner) = worker(Config::default()).await;
    let client = owner.client(1);
    let (blocked, release) = pause(&client).await;
    let first = client
        .try_put(OBJECT, Revision::INITIAL, b"one", OPTIONS)
        .unwrap();
    let second = client
        .try_put(OBJECT, Revision::new(1), b"two", OPTIONS)
        .unwrap();
    let shutdown = owner.shutdown(ShutdownMode::Drain);
    assert!(matches!(
        client.try_status(OBJECT, OPTIONS),
        Err(AdmissionError::Closed)
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(1), shutdown.wait())
            .await
            .is_err()
    );
    let done = owner.shutdown(ShutdownMode::Drain);
    drop(owner); // A requested drain is not changed into cancellation by Drop.
    drop(release);
    let report = done.wait().await;
    assert_eq!(report.succeeded, 3);
    assert!(!report.degraded);
    // Shutdown does not wait for the application to consume successful replies.
    blocked.await.unwrap();
    assert_eq!(first.await.unwrap(), Revision::new(1));
    assert_eq!(second.await.unwrap(), Revision::new(2));
    assert_eq!(
        Store::open(dir.path().join("objects"))
            .unwrap()
            .head(OBJECT),
        Revision::new(2)
    );
}

#[tokio::test]
async fn cancel_shutdown_escalates_drain_and_reports_every_queued_outcome() {
    let (_dir, owner) = worker(Config::default()).await;
    let client = owner.client(1);
    let (blocked, release) = pause(&client).await;
    let first = client
        .try_put(OBJECT, Revision::INITIAL, b"one", OPTIONS)
        .unwrap();
    let second = client
        .try_put(OBJECT, Revision::INITIAL, b"two", OPTIONS)
        .unwrap();
    assert_eq!(second.cancel(), Cancellation::Cancelled);
    let _drain = owner.shutdown(ShutdownMode::Drain);
    let done = owner.shutdown(ShutdownMode::CancelQueued);
    drop(release);
    let report = done.wait().await;
    blocked.await.unwrap();
    assert!(matches!(first.await, Err(Error::Unavailable)));
    assert!(matches!(second.await, Err(Error::Cancelled)));
    assert_eq!(
        (report.succeeded, report.not_applied, report.cancelled),
        (1, 1, 1)
    );
}

#[tokio::test]
async fn dropping_owner_closes_admission_despite_retained_clients() {
    let (_dir, owner) = worker(Config::default()).await;
    let client = owner.client(1);
    let (blocked, release) = pause(&client).await;
    let queued = client
        .try_put(OBJECT, Revision::INITIAL, b"data", OPTIONS)
        .unwrap();
    let done = Shutdown {
        done: client.shared.done.subscribe(),
    };
    drop(owner);
    assert!(matches!(
        client.try_status(OBJECT, OPTIONS),
        Err(AdmissionError::Closed)
    ));
    drop(release);
    blocked.await.unwrap();
    assert!(matches!(queued.await, Err(Error::Unavailable)));
    done.wait().await;
}

#[tokio::test]
async fn io_failure_quarantines_before_reply_and_does_not_execute_queued_work() {
    for point in [Point::AppendSync, Point::CompactSync] {
        let (dir, owner) = worker(Config::default()).await;
        let client = owner.client(1);
        client
            .try_put(OBJECT, Revision::INITIAL, b"one", OPTIONS)
            .unwrap()
            .await
            .unwrap();
        let (blocked, release) = pause(&client).await;
        let failed = client
            .submit(0, OPTIONS, || {
                move |store| {
                    let _guard = faults::install([(point, Action::Error(5))]);
                    if point == Point::CompactSync {
                        store.compact(Retention::Latest).map(|_| ())
                    } else {
                        store.put(OBJECT, Revision::new(1), b"two").map(|_| ())
                    }
                }
            })
            .unwrap();
        let queued = client
            .try_put(OBJECT, Revision::new(2), b"must-not-run", OPTIONS)
            .unwrap();
        drop(release);
        blocked.await.unwrap();
        assert!(matches!(
            failed.await,
            Err(Error::Uncertain(super::super::Error::Io(_)))
        ));
        assert!(matches!(
            client.try_status(OBJECT, OPTIONS),
            Err(AdmissionError::Degraded)
        ));
        assert!(matches!(queued.await, Err(Error::Unavailable)));
        let report = owner.shutdown(ShutdownMode::Drain).wait().await;
        assert!(report.degraded);
        assert_eq!(report.uncertain, 1);
        assert_eq!(report.not_applied, 1);
        // This is an application seam, not a real writeback failure: the complete
        // unsynced append may recover, but compaction never changed the head.
        let store = Store::open(dir.path().join("objects")).unwrap();
        assert_eq!(
            store.head(OBJECT),
            Revision::new(if point == Point::AppendSync { 2 } else { 1 })
        );
    }
}

#[tokio::test]
async fn owner_panic_is_uncertain_and_waiting_requests_are_not_applied() {
    let (dir, owner) = worker(Config::default()).await;
    let client = owner.client(1);
    let (blocked, release) = pause(&client).await;
    let failed = client
        .submit::<(), _>(4, OPTIONS, || {
            |store| {
                store.put(OBJECT, Revision::INITIAL, b"data").unwrap();
                panic!("injected owner panic after durable mutation")
            }
        })
        .unwrap();
    let queued = client
        .try_put(OBJECT, Revision::INITIAL, b"no", OPTIONS)
        .unwrap();
    drop(release);
    blocked.await.unwrap();
    assert!(matches!(failed.await, Err(Error::OwnerLost)));
    assert!(matches!(queued.await, Err(Error::Unavailable)));
    let report = owner.shutdown(ShutdownMode::Drain).wait().await;
    assert!(report.owner_panicked && report.degraded);
    assert_eq!(report.uncertain, 1);
    assert_eq!(client.diagnostics().small, Usage::default());
    assert_eq!(
        Store::open(dir.path().join("objects"))
            .unwrap()
            .head(OBJECT),
        Revision::new(1)
    );
}

#[tokio::test]
async fn cancellation_races_never_report_not_applied_for_a_committed_write() {
    let (_dir, owner) = worker(Config::default()).await;
    let client = owner.client(1);
    for id in 1..=64 {
        let object = ObjectKey::new(id);
        let pending = client
            .try_put(object, Revision::INITIAL, b"race", OPTIONS)
            .unwrap();
        let cancelled = pending.cancel();
        let result = pending.await;
        let head = client
            .try_status(object, OPTIONS)
            .unwrap()
            .await
            .unwrap()
            .head;
        match cancelled {
            Cancellation::Cancelled => {
                assert!(matches!(result, Err(Error::Cancelled)));
                assert_eq!(head, Revision::INITIAL);
            }
            Cancellation::TooLate => {
                assert_eq!(result.unwrap(), Revision::new(1));
                assert_eq!(head, Revision::new(1));
            }
        }
    }
    owner.shutdown(ShutdownMode::Drain).wait().await;
}

#[tokio::test]
async fn startup_errors_and_cancelled_startup_do_not_leave_a_serving_owner() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    let mut config = tiny();
    config.small.requests = 0;
    assert!(matches!(
        Worker::open(&path, Format::WholeEntry, Limits::default(), config).await,
        Err(StartError::Config)
    ));
    assert!(!path.exists());
    let store = Store::open(&path).unwrap();
    assert!(matches!(
        Worker::open(&path, Format::WholeEntry, Limits::default(), tiny()).await,
        Err(StartError::Storage(_))
    ));
    drop(store);
    let (go_tx, go_rx) = mpsc::channel();
    let release = Release(Some(go_tx));
    let open_path = path.clone();
    let (owner, ready) = Worker::spawn(tiny(), move || {
        go_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        Store::open(open_path)
    })
    .unwrap();
    let done = Shutdown {
        done: owner.shared.done.subscribe(),
    };
    drop(ready);
    drop(owner);
    drop(release);
    done.wait().await;
    assert!(Store::open(&path).is_ok());
}
