use futures::executor::block_on;
use reproto::storage::{
    worker::{Config, Format, Options, ShutdownMode, Worker},
    ComponentId, ComponentUpdate, Limits, ObjectKey, Revision, Store,
};
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{Arc, Barrier},
    thread,
    time::Instant,
};

pub fn run(base: &Path, variant: &str, producers: usize) -> Value {
    let total = match variant {
        "status" => 16_384,
        "write" => 128,
        name => panic!("unknown worker mode {name}"),
    };
    let writing = variant == "write";
    let dir = tempfile::Builder::new()
        .prefix("concurrency-")
        .tempdir_in(base)
        .unwrap();
    let path = dir.path().join("objects");
    let worker = block_on(Worker::open(
        &path,
        Format::Components,
        Limits::default(),
        Config::default(),
    ))
    .unwrap();
    let barrier = Arc::new(Barrier::new(producers + 1));
    let mut handles = Vec::new();
    for producer in 0..producers {
        let client = worker.client(producer as u64);
        let barrier = barrier.clone();
        handles.push(thread::spawn(move || {
            let object = ObjectKey::new(producer as u64 + 1);
            let mut samples = Vec::with_capacity(total / producers);
            barrier.wait();
            for sequence in 0..total / producers {
                let started = Instant::now();
                if writing {
                    let expected = Revision::new(sequence as u64);
                    let bytes = (sequence as u64).to_le_bytes();
                    let pending = client
                        .try_edit_components(
                            object,
                            expected,
                            Some(expected),
                            &[ComponentUpdate {
                                id: ComponentId::new(1),
                                value: Some(&bytes),
                            }],
                            Options::default(),
                        )
                        .unwrap();
                    assert_eq!(
                        block_on(pending).unwrap(),
                        Revision::new(sequence as u64 + 1)
                    );
                } else {
                    let status =
                        block_on(client.try_status(object, Options::default()).unwrap()).unwrap();
                    assert_eq!(status.head, Revision::INITIAL);
                    assert_eq!(status.published, Revision::INITIAL);
                }
                samples.push(started.elapsed().as_nanos() as u64);
            }
            samples
        }));
    }
    let started = Instant::now();
    barrier.wait();
    let samples = handles
        .into_iter()
        .flat_map(|h| h.join().unwrap())
        .collect();
    let elapsed = started.elapsed();
    let diagnostics = worker.diagnostics();
    assert_eq!(diagnostics.rejected, 0);
    let report = block_on(worker.shutdown(ShutdownMode::Drain).wait());
    assert!(!report.degraded);
    assert_eq!(report.succeeded, total as u64);
    let reopened = Store::open_components(&path).unwrap();
    for producer in 0..producers {
        let object = ObjectKey::new(producer as u64 + 1);
        let revision = if writing { total / producers } else { 0 };
        assert_eq!(reopened.head(object), Revision::new(revision as u64));
        if writing {
            let snapshot = reopened.get_components(object).unwrap();
            assert_eq!(snapshot.revision(), Revision::new(revision as u64));
            assert_eq!(
                snapshot.get(ComponentId::new(1)).unwrap(),
                ((revision - 1) as u64).to_le_bytes()
            );
        }
    }
    let mut result = super::metrics(total, elapsed, samples);
    result["verified_reopen"] = json!(true);
    result["rejected"] = json!(diagnostics.rejected);
    result
}
