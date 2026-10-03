//! Same fixed-slot, latest-publication workload on both storage engines.
//! This is a comparison adapter, not the complete ORM/history/capability API.
use capntproto::storage::{Limits, ObjectKey, Retention, Revision, Snapshot, Store, Update};
use eae_pages::durable::{Arena, ArenaSnapshot, Durability, Limits as ArenaLimits};
use serde_json::json;
use std::{fs, hint::black_box, os::unix::fs::MetadataExt, path::Path, time::Instant};

const SCHEMA: u64 = 0x7270726f62656e63;
const META: usize = 32;
enum Backend {
    Store(Store),
    Arena(Arena),
}
enum Held {
    Store(Snapshot),
    Arena(ArenaSnapshot, usize, usize),
}
impl Held {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Store(s) => s.bytes(),
            Self::Arena(s, offset, len) => &s.segments()[0][*offset..*offset + *len],
        }
    }
}
fn number(bytes: &[u8]) -> u64 {
    u64::from_le_bytes(bytes.try_into().unwrap())
}
fn arena_limits() -> ArenaLimits {
    ArenaLimits {
        max_log_bytes: 1024 * 1024 * 1024,
        ..ArenaLimits::default()
    }
}
fn store_limits() -> Limits {
    Limits {
        max_entry_bytes: 32 * 1024 * 1024,
        max_file_bytes: 1024 * 1024 * 1024,
    }
}
impl Backend {
    fn create(name: &str, path: &Path, values: &[Vec<u8>], capacity: usize) -> Self {
        if name == "store" {
            let mut store = Store::open_with_limits(path, store_limits()).unwrap();
            for (start, group) in values.chunks(16).enumerate() {
                let updates: Vec<_> = group
                    .iter()
                    .enumerate()
                    .map(|(i, value)| Update {
                        object: ObjectKey::new((start * 16 + i) as u64),
                        expected_head: Revision::INITIAL,
                        expected_published: Some(Revision::INITIAL),
                        value,
                    })
                    .collect();
                store.commit(&updates).unwrap();
            }
            Self::Store(store)
        } else {
            assert_eq!(name, "eae");
            let mut arena = Arena::create(path, &[capacity], SCHEMA, arena_limits()).unwrap();
            let mut tx = arena.begin().unwrap();
            for (i, value) in values.iter().enumerate() {
                let start = i * (META + value.len());
                tx.write(0, start, &(i as u64).to_le_bytes()).unwrap();
                tx.write(0, start + 8, &1u64.to_le_bytes()).unwrap();
                tx.write(0, start + 16, &1u64.to_le_bytes()).unwrap();
                tx.write(0, start + META, value).unwrap();
            }
            tx.commit(Durability::Durable).unwrap();
            Self::Arena(arena)
        }
    }
    fn open(name: &str, path: &Path) -> Self {
        if name == "store" {
            Self::Store(Store::open_with_limits(path, store_limits()).unwrap())
        } else {
            Self::Arena(Arena::open(path, SCHEMA, arena_limits()).unwrap())
        }
    }
    fn commit(&mut self, id: usize, expected: u64, value: &[u8], offset: usize, len: usize) -> u64 {
        match self {
            Self::Store(store) => {
                let before = store.file_bytes();
                let revisions = store
                    .commit(&[Update {
                        object: ObjectKey::new(id as u64),
                        expected_head: Revision::new(expected),
                        expected_published: Some(Revision::new(expected)),
                        value,
                    }])
                    .unwrap();
                assert_eq!(revisions, [Revision::new(expected + 1)]);
                (store.file_bytes() - before) as u64
            }
            Self::Arena(arena) => {
                let start = id * (META + value.len());
                {
                    let segments = arena.segments().unwrap();
                    assert_eq!(number(&segments[0][start + 8..start + 16]), expected);
                    assert_eq!(number(&segments[0][start + 16..start + 24]), expected);
                }
                let generation = arena.generation();
                let mut tx = arena.begin_at(generation).unwrap();
                tx.write(0, start + 8, &(expected + 1).to_le_bytes())
                    .unwrap();
                tx.write(0, start + 16, &(expected + 1).to_le_bytes())
                    .unwrap();
                tx.write(0, start + META + offset, &value[offset..offset + len])
                    .unwrap();
                tx.commit(Durability::Durable).unwrap().file_bytes as u64
            }
        }
    }
    fn snapshot(&self, id: usize, len: usize) -> Held {
        match self {
            Self::Store(store) => Held::Store(store.get(ObjectKey::new(id as u64)).unwrap()),
            Self::Arena(arena) => {
                Held::Arena(arena.snapshot().unwrap(), id * (META + len) + META, len)
            }
        }
    }
    fn checkpoint(&mut self) -> u64 {
        match self {
            Self::Store(store) => store.compact(Retention::Latest).unwrap().after_bytes as u64,
            Self::Arena(arena) => arena.checkpoint().unwrap().file_bytes,
        }
    }
    fn verify(&self, values: &[Vec<u8>], revisions: &[u64], capacity: usize) {
        match self {
            Self::Store(store) => {
                for (i, value) in values.iter().enumerate() {
                    assert_eq!(store.head(ObjectKey::new(i as u64)).get(), revisions[i]);
                    assert_eq!(
                        store.published(ObjectKey::new(i as u64)).get(),
                        revisions[i]
                    );
                    assert_eq!(store.get(ObjectKey::new(i as u64)).unwrap().bytes(), value);
                }
            }
            Self::Arena(arena) => {
                let segments = arena.segments().unwrap();
                for (i, value) in values.iter().enumerate() {
                    let start = i * (META + value.len());
                    assert_eq!(number(&segments[0][start..start + 8]), i as u64);
                    assert_eq!(number(&segments[0][start + 8..start + 16]), revisions[i]);
                    assert_eq!(number(&segments[0][start + 16..start + 24]), revisions[i]);
                    assert_eq!(&segments[0][start + 24..start + META], &[0; 8]);
                    assert_eq!(
                        &segments[0][start + META..start + META + value.len()],
                        value
                    );
                }
                let used = values.len() * (META + values[0].len());
                assert!(segments[0][used..capacity].iter().all(|v| *v == 0));
            }
        }
    }
}
fn elapsed_ns(start: Instant) -> u64 {
    start.elapsed().as_nanos().try_into().unwrap()
}
fn stats(samples: &[u64]) -> serde_json::Value {
    if samples.is_empty() {
        return json!({"count":0});
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    json!({"count": sorted.len(), "total_ns": sorted.iter().sum::<u64>(),
        "p50_ns": sorted[sorted.len() / 2], "p99_ns": sorted[(sorted.len() * 99 / 100).min(sorted.len()-1)]})
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 10, "backend base-dir objects entry-bytes edit-bytes iterations checkpoint-every snapshot-every capacity-factor");
    let name = &args[1];
    let base = Path::new(&args[2]);
    let numbers: Vec<usize> = args[3..].iter().map(|v| v.parse().unwrap()).collect();
    let [objects, entry, edit, iterations, every, snapshots, factor]: [usize; 7] =
        numbers.try_into().unwrap();
    assert!(
        objects > 0
            && entry >= 8
            && entry % 8 == 0
            && edit > 0
            && edit <= entry
            && iterations > 0
            && factor > 0
    );
    let dir = tempfile::tempdir_in(base).unwrap();
    let path = dir.path().join("store");
    let data_path = if name == "store" {
        path.clone()
    } else {
        path.join("DATA")
    };
    let capacity = objects * (META + entry) * factor;
    let mut values = vec![vec![1; entry]; objects];
    let mut revisions = vec![1; objects];
    let initial_start = Instant::now();
    let mut backend = Backend::create(name, &path, &values, capacity);
    let initialize_ns = elapsed_ns(initial_start);
    let initial_file_bytes = fs::metadata(&data_path).unwrap().len();
    let mut commits = Vec::new();
    let mut checkpoints = Vec::new();
    let mut first_snapshots = Vec::new();
    let mut repeated_snapshots = Vec::new();
    let mut log_written = 0;
    let mut checkpoint_written = 0;
    let mut coexist_bytes = 0;
    let mut held: Option<(Held, Vec<u8>)> = None;
    let wall = Instant::now();
    for n in 0..iterations {
        let id = n % objects;
        let offset = if edit == entry {
            0
        } else {
            ((n * 53) % ((entry - edit) / 8 + 1)) * 8
        };
        values[id][offset..offset + edit].fill(((n + 2) % 251 + 1) as u8);
        let start = Instant::now();
        log_written += backend.commit(id, revisions[id], &values[id], offset, edit);
        commits.push(elapsed_ns(start));
        revisions[id] += 1;
        if snapshots != 0 && (n + 1) % snapshots == 0 {
            if let Some((old, bytes)) = &held {
                assert_eq!(old.bytes(), bytes);
            }
            let start = Instant::now();
            let snapshot = backend.snapshot(id, entry);
            first_snapshots.push(elapsed_ns(start));
            assert_eq!(snapshot.bytes(), values[id]);
            let start = Instant::now();
            for _ in 0..32 {
                black_box(backend.snapshot(id, entry));
            }
            repeated_snapshots.push(elapsed_ns(start) / 32);
            held = Some((snapshot, values[id].clone()));
        }
        if every != 0 && (n + 1) % every == 0 {
            let before = fs::metadata(&data_path).unwrap().len();
            let start = Instant::now();
            let written = backend.checkpoint();
            checkpoints.push(elapsed_ns(start));
            checkpoint_written += written;
            coexist_bytes = coexist_bytes.max(before + written);
        }
    }
    let workload_wall_ns = elapsed_ns(wall);
    backend.verify(&values, &revisions, capacity);
    if let Some((old, bytes)) = &held {
        assert_eq!(old.bytes(), bytes);
    }
    drop(held);
    let before_normalize = fs::metadata(&data_path).unwrap();
    drop(backend);
    let start = Instant::now();
    let mut backend = Backend::open(name, &path);
    let reopen_with_log_ns = elapsed_ns(start);
    backend.verify(&values, &revisions, capacity);
    let start = Instant::now();
    let final_checkpoint_bytes = backend.checkpoint();
    let final_checkpoint_ns = elapsed_ns(start);
    coexist_bytes = coexist_bytes.max(before_normalize.len() + final_checkpoint_bytes);
    drop(backend);
    let start = Instant::now();
    let backend = Backend::open(name, &path);
    let reopen_checkpoint_ns = elapsed_ns(start);
    backend.verify(&values, &revisions, capacity);
    let final_file = fs::metadata(&data_path).unwrap();
    let high_water_kib = fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find(|line| line.starts_with("VmHWM:"))
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse::<u64>()
        .unwrap();
    println!(
        "{}",
        json!({"backend":name,"objects":objects,"entry_bytes":entry,"edit_bytes":edit,
        "iterations":iterations,"checkpoint_every":every,"snapshot_every":snapshots,"capacity_factor":factor,
        "arena_capacity_bytes":capacity,"initialize_ns":initialize_ns,"initial_file_bytes":initial_file_bytes,
        "commits":stats(&commits),"checkpoints":stats(&checkpoints),"first_snapshots":stats(&first_snapshots),
        "repeated_snapshots":stats(&repeated_snapshots),"workload_wall_ns":workload_wall_ns,
        "logical_changed_bytes":iterations*edit,"log_written_bytes":log_written,"checkpoint_written_bytes":checkpoint_written,
        "final_checkpoint_bytes":final_checkpoint_bytes,"final_checkpoint_ns":final_checkpoint_ns,
        "total_written_bytes":initial_file_bytes+log_written+checkpoint_written+final_checkpoint_bytes,
        "end_file_bytes":before_normalize.len(),"end_allocated_bytes":before_normalize.blocks()*512,
        "final_file_bytes":final_file.len(),"final_allocated_bytes":final_file.blocks()*512,
        "checkpoint_coexist_logical_bytes":coexist_bytes,"process_peak_rss_kib":high_water_kib,
        "reopen_with_log_ns":reopen_with_log_ns,"reopen_checkpoint_ns":reopen_checkpoint_ns,
        "validation":"all object bytes/revisions, unused arena capacity, held snapshots and both reopens verified"})
    );
}
