//! Research harness, not a runtime API. Run compiled release binaries serially
//! on an explicitly selected filesystem; never use these results as CI gates.
use capnp::traits::HasTypeId;
use reproto::{
    authority::ObjectId,
    orm::components::{ComponentState, Edit, TypedComponent},
    storage::{
        ComponentId, ComponentSnapshot, ComponentUpdate, ObjectKey, Retention, Revision, Snapshot,
        Store, Update,
    },
    store_capnp::document,
};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    hint::black_box,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const OBJECT: ObjectKey = ObjectKey::new(1);
const HOT: ComponentId = ComponentId::new(0);
const READS: usize = 10_000;

fn distribution(values: &[f64]) -> Value {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q).ceil() as usize];
    json!({"count": sorted.len(), "p50_us": at(0.5), "p99_us": at(0.99), "max_us": at(1.0)})
}
fn micros(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1e6
}
fn payload() -> String {
    let mut state = 0x123456789abcdefu64;
    (0..65536)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (33 + state % 94) as u8 as char
        })
        .collect()
}
fn message(value: &str) -> capnp::message::Builder<capnp::message::HeapAllocator> {
    let mut message = capnp::message::Builder::new_default();
    message.init_root::<document::Builder<'_>>().set_text(value);
    message
}
fn edit(id: usize, value: &str) -> capnp::Result<Edit> {
    Edit::set::<document::Owned>(
        ComponentId::new(id as u64),
        message(value).get_root_as_reader()?,
    )
}
fn mappings(path: &Path) -> Value {
    let Ok(maps) = std::fs::read_to_string("/proc/self/maps") else {
        return Value::Null;
    };
    let mut count = 0;
    let mut bytes = 0u64;
    for line in maps
        .lines()
        .filter(|line| line.contains(path.to_str().unwrap()))
    {
        let (start, end) = line
            .split_whitespace()
            .next()
            .unwrap()
            .split_once('-')
            .unwrap();
        bytes += u64::from_str_radix(end, 16).unwrap() - u64::from_str_radix(start, 16).unwrap();
        count += 1;
    }
    json!({"mappings": count, "virtual_bytes": bytes})
}
fn read_whole(snapshot: &Snapshot) -> capnp::Result<()> {
    let mut bytes = &snapshot.bytes()[8..];
    let reader = capnp::serialize::read_message_from_flat_slice(&mut bytes, Default::default())?;
    black_box(
        &reader
            .get_root::<document::Reader<'_>>()?
            .get_text()?
            .as_bytes()[..8],
    );
    Ok(())
}
fn read_component(snapshot: ComponentSnapshot) -> capnp::Result<()> {
    TypedComponent::<document::Owned>::new(snapshot, HOT)?.with_reader(|r| {
        black_box(r.get_text()?.as_bytes());
        Ok(())
    })
}

fn layout(base: &Path, components: bool, parts: usize, writes: u64, hold: bool) -> Result<Value> {
    let dir = tempfile::tempdir_in(base)?;
    let path = dir.path().join("objects");
    let open = |path: &Path| {
        if components {
            Store::open_components(path)
        } else {
            Store::open(path)
        }
    };
    let store = Rc::new(RefCell::new(open(&path)?));
    let state = if components {
        Some(ComponentState::new(
            store.clone(),
            ObjectId::new(1).unwrap(),
        )?)
    } else {
        None
    };
    let cold = payload();
    let mut timings = Vec::new();
    let mut whole_held = Vec::new();
    let mut component_held = Vec::new();
    let mut seed_bytes = 0;
    for i in 0..=writes {
        let start = Instant::now();
        let hot = format!("{i:08x}");
        if let Some(state) = &state {
            let mut edits = vec![edit(0, &hot)?];
            if i == 0 {
                for part in 1..parts {
                    let from = (part - 1) * cold.len() / (parts - 1);
                    let to = part * cold.len() / (parts - 1);
                    edits.push(edit(part, &cold[from..to])?);
                }
            }
            state.edit(Revision::new(i), Some(Revision::new(i)), &edits)?;
        } else {
            let msg = message(&format!("{hot}{cold}"));
            let mut bytes = <document::Reader<'_> as HasTypeId>::TYPE_ID
                .to_le_bytes()
                .to_vec();
            bytes.extend(capnp::serialize::write_message_to_words(&msg));
            store.borrow_mut().commit(&[Update {
                object: OBJECT,
                expected_head: Revision::new(i),
                expected_published: Some(Revision::new(i)),
                value: &bytes,
            }])?;
        }
        if i == 0 {
            seed_bytes = store.borrow().file_bytes();
        } else {
            timings.push(micros(start));
        }
        if hold {
            if components {
                component_held.push(store.borrow().get_components(OBJECT)?);
            } else {
                whole_held.push(store.borrow().get(OBJECT)?);
            }
        }
    }
    let append_bytes = store.borrow().file_bytes() - seed_bytes;
    let mapped_before = mappings(&path);
    let start = Instant::now();
    for _ in 0..READS {
        if components {
            read_component(store.borrow().get_components(OBJECT)?)?;
        } else {
            read_whole(&store.borrow().get(OBJECT)?)?;
        }
    }
    let read_us = micros(start) / READS as f64;
    let start = Instant::now();
    let checkpoint = store.borrow_mut().compact(Retention::History)?;
    let compact_us = micros(start);
    let mapped_after_compact = mappings(&path);
    // Verify old snapshots after replacement and the latest complete typed value.
    if components {
        if let Some(first) = component_held.first() {
            TypedComponent::<document::Owned>::new(first.clone(), HOT)?.with_reader(|r| {
                assert_eq!(r.get_text()?, "00000000");
                Ok(())
            })?;
        }
        let snap = store.borrow().get_components(OBJECT)?;
        TypedComponent::<document::Owned>::new(snap.clone(), HOT)?.with_reader(|r| {
            assert_eq!(r.get_text()?.to_str()?, format!("{writes:08x}"));
            Ok(())
        })?;
        let mut joined = String::new();
        for id in 1..parts {
            TypedComponent::<document::Owned>::new(snap.clone(), ComponentId::new(id as u64))?
                .with_reader(|r| {
                    joined.push_str(r.get_text()?.to_str()?);
                    Ok(())
                })?;
        }
        assert_eq!(joined, cold);
    } else {
        let snapshot = store.borrow().get(OBJECT)?;
        let mut bytes = &snapshot.bytes()[8..];
        let msg = capnp::serialize::read_message_from_flat_slice(&mut bytes, Default::default())?;
        assert_eq!(
            msg.get_root::<document::Reader<'_>>()?
                .get_text()?
                .to_str()?,
            format!("{writes:08x}{cold}")
        );
    }
    drop(component_held);
    drop(whole_held);
    let mapped_after_release = mappings(&path);
    drop(state);
    drop(store);
    let start = Instant::now();
    let reopened = open(&path)?;
    let reopen_us = micros(start);
    assert_eq!(reopened.published(OBJECT), Revision::new(writes + 1));
    if components {
        read_component(reopened.get_components(OBJECT)?)?;
    } else {
        read_whole(&reopened.get(OBJECT)?)?;
    }
    Ok(
        json!({"mode": if components { "components" } else { "whole" }, "components": parts, "writes": writes, "held_snapshots": hold,
        "commit_including_encode": distribution(&timings), "seed_bytes": seed_bytes, "update_bytes": append_bytes,
        "bytes_per_update": append_bytes / writes as usize, "warm_acquire_and_hot_read_mean_us": read_us,
        "warm_read_iterations": READS, "history_compact_us": compact_us, "history_checkpoint_bytes": checkpoint.after_bytes,
        "warm_reopen_us": reopen_us, "mapped_before": mapped_before, "mapped_after_compact": mapped_after_compact, "mapped_after_release": mapped_after_release }),
    )
}

fn raw_seed(path: &Path) -> Store {
    let mut store = Store::open_components(path).unwrap();
    store
        .edit_components(
            OBJECT,
            Revision::INITIAL,
            Some(Revision::INITIAL),
            &[
                ComponentUpdate {
                    id: HOT,
                    value: Some(&0u64.to_le_bytes()),
                },
                ComponentUpdate {
                    id: ComponentId::new(1),
                    value: Some(payload().as_bytes()),
                },
            ],
        )
        .unwrap();
    store
}
fn raw_commit(store: &mut Store, value: u64) {
    store
        .edit_components(
            OBJECT,
            Revision::new(value),
            Some(Revision::new(value)),
            &[ComponentUpdate {
                id: HOT,
                value: Some(&value.to_le_bytes()),
            }],
        )
        .unwrap();
}

async fn executor(base: &Path, worker: bool, writes: u64) -> Result<Value> {
    let dir = tempfile::tempdir_in(base)?;
    let path = dir.path().join("objects");
    let mut direct = if worker { None } else { Some(raw_seed(&path)) };
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(u64, tokio::sync::oneshot::Sender<()>)>(8);
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let thread = if worker {
        Some(std::thread::spawn(move || {
            let mut store = raw_seed(&path);
            ready_tx.send(()).unwrap();
            while let Some((value, reply)) = rx.blocking_recv() {
                raw_commit(&mut store, value);
                let _ = reply.send(());
            }
        }))
    } else {
        None
    };
    if worker {
        ready_rx.await?;
    }
    let stop = Rc::new(Cell::new(false));
    let probe_stop = stop.clone();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let probe = tokio::task::spawn_local(async move {
        let mut timer = tokio::time::interval(Duration::from_millis(1));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        timer.tick().await;
        started_tx.send(()).unwrap();
        let mut late = Vec::new();
        while !probe_stop.get() {
            let due = timer.tick().await;
            late.push(due.elapsed().as_secs_f64() * 1e6);
        }
        late
    });
    started_rx.await?;
    let mut timings = Vec::new();
    let start_all = Instant::now();
    for value in 1..=writes {
        let start = Instant::now();
        if let Some(store) = direct.as_mut() {
            raw_commit(store, value);
        } else {
            let (reply, done) = tokio::sync::oneshot::channel();
            tx.send((value, reply)).await?;
            done.await?;
        }
        timings.push(micros(start));
        tokio::task::yield_now().await;
    }
    let elapsed_us = micros(start_all);
    stop.set(true);
    let lateness = probe.await?;
    drop(tx);
    if let Some(thread) = thread {
        thread.join().unwrap();
    }
    Ok(
        json!({"mode": if worker { "worker" } else { "direct" }, "writes": writes, "raw_component_commit": distribution(&timings),
        "elapsed_us": elapsed_us, "timer_lateness": distribution(&lateness), "timer_period_us": 1000,
        "max_queued_requests": 8, "concurrent_requests": 1}),
    )
}

fn batch(base: &Path, size: u64, writes: u64) -> Result<Value> {
    let dir = tempfile::tempdir_in(base)?;
    let mut store = Store::open(dir.path().join("objects"))?;
    let mut timings = Vec::new();
    let mut operations = 0;
    let start_all = Instant::now();
    for revision in 0..writes / size {
        let value = revision.to_le_bytes();
        let updates: Vec<_> = (1..=size)
            .map(|id| Update {
                object: ObjectKey::new(id),
                expected_head: Revision::new(revision),
                expected_published: Some(Revision::new(revision)),
                value: &value,
            })
            .collect();
        let start = Instant::now();
        store.commit(&updates)?;
        timings.push(micros(start));
        operations += size;
    }
    let elapsed_us = micros(start_all);
    Ok(
        json!({"mode": "batch", "batch_size": size, "object_updates": operations, "syncs": writes / size,
        "elapsed_us": elapsed_us, "mean_us_per_object_update": elapsed_us / operations as f64, "batch_commit": distribution(&timings)}),
    )
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 6 {
        return Err("usage: storage_probe BASE MODE SIZE WRITES HOLD; MODE=whole|components|direct|worker|batch".into());
    }
    let base = PathBuf::from(&args[1]).canonicalize()?;
    let size = args[3].parse::<usize>()?;
    let writes = args[4].parse::<u64>()?;
    let hold = args[5].parse::<bool>()?;
    if !(1..=256).contains(&size) || !(16..=4096).contains(&writes) {
        return Err("SIZE must be 1..256 and WRITES 16..4096".into());
    }
    let mut result = match args[2].as_str() {
        "whole" => layout(&base, false, size, writes, hold)?,
        "components" if size >= 2 => layout(&base, true, size, writes, hold)?,
        "direct" | "worker" => {
            tokio::task::LocalSet::new()
                .run_until(executor(&base, args[2] == "worker", writes))
                .await?
        }
        "batch" if size <= 16 && writes.is_multiple_of(size as u64) => {
            batch(&base, size as u64, writes)?
        }
        _ => return Err("invalid mode or batch size".into()),
    };
    result["base"] = json!(base);
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}
