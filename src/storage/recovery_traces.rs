use super::{
    io::faults::{self, Action},
    *,
};
use crate::semantics::Revision;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Trace {
    steps: Vec<Step>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    action: String,
    state: State,
}
#[derive(Default, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct State {
    visible: u64,
    durable: u64,
    ack: u64,
    phase: u64,
    epoch: u64,
    durable_epoch: u64,
    damaged: u64,
    rejected: u64,
    rewritten: u64,
    failures: u64,
    crashes: u64,
    power_losses: u64,
    torn: u64,
    partials: u64,
    recovered: u64,
}
// Inspect generated test records without reopening: reopening would itself
// synchronize recovery and erase the distinction under test. This helper is
// only an observation of trusted generated frames, not an alternate parser.
fn disk_state(bytes: &[u8]) -> (u64, u64, u64) {
    let mut heads = [0, 0];
    let mut published = [0, 0];
    let mut at = HEADER;
    while bytes.len() - at >= RECORD {
        let size = u64_at(bytes, at + 32) as usize;
        let total = record_size(size).unwrap().1;
        if total > bytes.len() - at {
            break;
        }
        match u64_at(bytes, at + 8) {
            7 => {
                for e in batch_entries(&bytes[at + RECORD..at + RECORD + size]).unwrap() {
                    heads[e.object.get() as usize - 1] = e.revision.get();
                    assert!(e.publish);
                    published[e.object.get() as usize - 1] = e.revision.get();
                }
            }
            3 => heads[u64_at(bytes, at + 16) as usize - 1] = u64_at(bytes, at + 24),
            4 => published[u64_at(bytes, at + 16) as usize - 1] = u64_at(bytes, at + 24),
            kind => panic!("unexpected generated record {kind}"),
        }
        at += total;
    }
    assert_eq!(heads[0], heads[1], "partial batch head became visible");
    assert_eq!(published, heads, "receipt/publication were not atomic");
    (
        heads[0],
        u64::from(u64_at(bytes, 16) > HEADER as u64),
        u64::from(at < bytes.len()),
    )
}

#[test]
fn replay_tlc_storage_recovery_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_STORAGE_RECOVERY_TRACES")
        .expect("required trace file");
    let cases: Vec<Trace> = capntproto_test_support::traces::read(path, "StorageRecovery");
    for (case, trace) in cases.into_iter().enumerate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("objects");
        let mut store = Some(Store::open(&path).unwrap());
        let mut stable_image = fs::read(&path).unwrap();
        let mut actual = State::default();
        for step in trace.steps {
            match step.action.as_str() {
                "commit" | "appended" | "synced" | "partial" => {
                    let plan = match step.action.as_str() {
                        "appended" => vec![(Point::Appended, Action::Error(5))],
                        "synced" => vec![(Point::AppendSynced, Action::Error(5))],
                        "partial" => vec![
                            (Point::AppendWrite, Action::Short(87)),
                            (Point::AppendWrite, Action::Error(5)),
                        ],
                        _ => vec![],
                    };
                    let _guard = faults::install(plan);
                    let payload = (actual.visible + 1).to_le_bytes();
                    let result = store.as_mut().unwrap().commit(&[
                        Update {
                            object: ObjectKey::new(1),
                            expected_head: Revision::new(actual.visible),
                            expected_published: Some(Revision::new(actual.visible)),
                            value: &payload,
                        },
                        Update {
                            object: ObjectKey::new(2),
                            expected_head: Revision::new(actual.visible),
                            expected_published: Some(Revision::new(actual.visible)),
                            value: &payload,
                        },
                    ]);
                    actual.recovered = 0;
                    if step.action == "commit" {
                        actual.ack = result.unwrap()[0].get();
                        actual.durable = actual.ack;
                        stable_image = fs::read(&path).unwrap();
                    } else {
                        assert!(matches!(result, Err(Error::Io(_))));
                        actual.phase = 1;
                        assert!(store.as_ref().unwrap().poisoned);
                        if step.action == "synced" {
                            assert!(faults::visits().contains(&Point::AppendSynced));
                            actual.durable += 1;
                            stable_image = fs::read(&path).unwrap();
                        }
                        if step.action == "partial" {
                            actual.partials += 1;
                        }
                    }
                }
                "rename" => {
                    let _guard = faults::install([(Point::CompactRenamed, Action::Error(5))]);
                    assert!(store.as_mut().unwrap().compact(Retention::Latest).is_err());
                    assert!(store.as_ref().unwrap().poisoned);
                    actual.phase = 1;
                    actual.recovered = 0;
                }
                "crash" => {
                    drop(store.take());
                    actual.phase = 2;
                    actual.crashes += 1;
                    actual.recovered = 0;
                }
                "power" => {
                    assert!(store.is_none());
                    // Deterministic filesystem contract: only the last synced
                    // image/path is guaranteed to survive simulated power loss.
                    fs::write(&path, &stable_image).unwrap();
                    actual.power_losses += 1;
                    actual.recovered = 0;
                }
                "recover" => {
                    let _guard = faults::install([]);
                    store = Some(Store::open(&path).unwrap());
                    assert_eq!(
                        faults::visits(),
                        [
                            Point::RecoverySync,
                            Point::RecoveryDirectorySync,
                            Point::Recovered
                        ]
                    );
                    actual.phase = 0;
                    actual.recovered = 1;
                    stable_image = fs::read(&path).unwrap();
                    let (head, epoch, _) = disk_state(&stable_image);
                    actual.durable = head;
                    actual.durable_epoch = epoch;
                }
                "fail" => {
                    let _guard = faults::install([(Point::RecoverySync, Action::Error(5))]);
                    assert!(matches!(Store::open(&path), Err(Error::Io(_))));
                    assert!(!faults::visits().contains(&Point::Recovered));
                    actual.failures += 1;
                }
                "corrupt" => {
                    let mut bytes = fs::read(&path).unwrap();
                    bytes[HEADER + 32..HEADER + 40].copy_from_slice(&1048576u64.to_le_bytes());
                    fs::write(&path, bytes).unwrap();
                    actual.damaged = 1;
                }
                "reject" => {
                    let before = fs::read(&path).unwrap();
                    assert!(matches!(Store::open(&path), Err(Error::Corrupt(_))));
                    actual.rewritten = u64::from(fs::read(&path).unwrap() != before);
                    actual.rejected = 1;
                }
                other => panic!("unknown action {other}"),
            }
            if actual.damaged == 0 {
                (actual.visible, actual.epoch, actual.torn) = disk_state(&fs::read(&path).unwrap());
            }
            if actual.phase == 0 {
                let s = store.as_ref().unwrap();
                assert!(!s.poisoned);
                for object in [1, 2] {
                    assert_eq!(
                        s.head(ObjectKey::new(object)),
                        Revision::new(actual.visible)
                    );
                    assert_eq!(
                        s.published(ObjectKey::new(object)),
                        Revision::new(actual.visible)
                    );
                    if actual.visible > 0 {
                        assert_eq!(
                            s.get(ObjectKey::new(object)).unwrap().bytes(),
                            actual.visible.to_le_bytes()
                        );
                    }
                }
            }
            assert_eq!(actual, step.state, "case {case}: {}", step.action);
        }
    }
}
