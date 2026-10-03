//! Process exits exercise framing/recovery barriers, not power loss or real EIO.
use super::*;
use crate::storage::io::faults::{self, Action};

const OBJECT: ObjectKey = ObjectKey::new(7);
const HOT: ComponentId = ComponentId::new(1);
const COLD: ComponentId = ComponentId::new(2);
const SHARED: ComponentId = ComponentId::new(3);

fn edit(store: &mut Store, hot: &[u8], cold: &[u8]) {
    let mut updates = vec![
        ComponentUpdate {
            id: HOT,
            value: Some(hot),
        },
        ComponentUpdate {
            id: COLD,
            value: Some(cold),
        },
    ];
    let shared = vec![42; 65536];
    if store.head(OBJECT) == Revision::INITIAL {
        updates.push(ComponentUpdate {
            id: SHARED,
            value: Some(&shared),
        });
    }
    store
        .edit_components(
            OBJECT,
            store.head(OBJECT),
            Some(store.published(OBJECT)),
            &updates,
        )
        .unwrap();
}

fn child(path: &Path, scenario: &str) {
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "storage::components::crash_tests::crash_worker",
            "--nocapture",
        ])
        .env("REPROTO_COMPONENT_CRASH_PATH", path)
        .env("REPROTO_COMPONENT_CRASH_SCENARIO", scenario)
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(86), "{scenario}");
}

#[test]
#[ignore = "run by the storage recovery gate in isolation; fork temporarily inherits other tests' file locks"]
fn child_process_crashes_preserve_components_across_recovery_and_second_crash() {
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
        let mut store = Store::open_components(&path).unwrap();
        edit(&mut store, b"old-hot", b"old-cold");
        drop(store);
        child(&path, scenario);
        child(&path, "recovered");
        let store = Store::open_components(&path).unwrap();
        let snapshot = store.get_components(OBJECT).unwrap();
        let new = scenario != "partial";
        assert_eq!(
            snapshot.revision(),
            Revision::new(if new { 2 } else { 1 }),
            "{scenario}"
        );
        assert_eq!(store.head(OBJECT), store.published(OBJECT), "{scenario}");
        assert_eq!(
            snapshot.get(HOT).unwrap(),
            if new { b"new-hot" } else { b"old-hot" },
            "{scenario}"
        );
        assert_eq!(
            snapshot.get(COLD).unwrap(),
            if new { b"new-cold" } else { b"old-cold" },
            "{scenario}"
        );
        assert_eq!(snapshot.get(SHARED).unwrap(), vec![42; 65536], "{scenario}");
        assert_eq!(snapshot.components().len(), 3, "{scenario}");
    }
}

#[test]
fn crash_worker() {
    let Some(path) = std::env::var_os("REPROTO_COMPONENT_CRASH_PATH") else {
        return;
    };
    let scenario = std::env::var("REPROTO_COMPONENT_CRASH_SCENARIO").unwrap();
    if scenario == "recovered" {
        let _guard = faults::install([(Point::Recovered, Action::Crash)]);
        let _ = Store::open_components(path).unwrap();
        panic!("recovery fault was not reached");
    }
    let mut store = Store::open_components(path).unwrap();
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
    edit(&mut store, b"new-hot", b"new-cold");
    if scenario == "acknowledged" {
        std::process::exit(86);
    }
    store.compact(Retention::Latest).unwrap();
    panic!("crash fault was not reached");
}
