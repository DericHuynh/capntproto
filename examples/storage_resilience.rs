//! Fixed-rate storage experiments, not a production writer or a CI timing gate.
//! Every accepted write uses the real V5 commit path and its durability barrier.
use capntproto::storage::{ComponentId, ComponentUpdate, ObjectKey, Revision, Store};
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

const OBJECT: ObjectKey = ObjectKey::new(1);
const HOT: ComponentId = ComponentId::new(1);
const COLD: ComponentId = ComponentId::new(2);

#[derive(Clone, Copy)]
struct Config {
    rate: usize,
    count: usize,
    bytes: usize,
    pause: bool,
    deadline: Option<Duration>,
    mixed: bool,
}
impl Config {
    fn named(name: &str) -> Self {
        let mut config = Self {
            rate: 1000,
            count: 64,
            bytes: 4 * 1024 * 1024,
            pause: true,
            deadline: None,
            mixed: false,
        };
        match name {
            "baseline" => {
                config.rate = 100;
                config.count = 8;
                config.pause = false;
            }
            "steady-stall" => {
                config.rate = 100;
                config.count = 8;
            }
            "overload-8" => config.count = 8,
            "overload-64" => {}
            "overload-256" => config.count = 256,
            "deadline-25ms" => config.deadline = Some(Duration::from_millis(25)),
            "mixed-4mib" => config.mixed = true,
            "mixed-128kib" => {
                config.mixed = true;
                config.bytes = 128 * 1024;
            }
            _ => panic!("unknown scenario: {name}"),
        }
        config
    }
}

#[derive(Default)]
struct Usage {
    count: usize,
    bytes: usize,
    peak_count: usize,
    peak_bytes: usize,
}
struct Budget {
    usage: Mutex<Usage>,
    config: Config,
}
enum Rejected {
    Count,
    Bytes,
}
struct Permit {
    budget: Arc<Budget>,
    bytes: usize,
}
impl Budget {
    fn acquire(self: &Arc<Self>, bytes: usize) -> Result<Permit, Rejected> {
        let mut usage = self.usage.lock().unwrap();
        if usage.count == self.config.count {
            return Err(Rejected::Count);
        }
        if bytes > self.config.bytes - usage.bytes {
            return Err(Rejected::Bytes);
        }
        usage.count += 1;
        usage.bytes += bytes;
        usage.peak_count = usage.peak_count.max(usage.count);
        usage.peak_bytes = usage.peak_bytes.max(usage.bytes);
        Ok(Permit {
            budget: self.clone(),
            bytes,
        })
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        let mut usage = self.budget.usage.lock().unwrap();
        usage.count -= 1;
        usage.bytes -= self.bytes;
    }
}
struct Command {
    id: usize,
    payload: Vec<u8>,
    scheduled: Instant,
    admitted: Instant,
    deadline: Option<Instant>,
    // Includes preparation, queueing, and execution, through the durability barrier.
    _permit: Permit,
}
#[derive(Default)]
struct Results {
    committed: usize,
    committed_large: usize,
    expired: usize,
    committed_after_deadline: usize,
    last_id: usize,
    service: Vec<f64>,
    queue_age: Vec<f64>,
    success_latency: Vec<f64>,
    expired_latency: Vec<f64>,
    pause_us: f64,
}
fn distribution(values: &[f64]) -> Value {
    if values.is_empty() {
        return json!({"count": 0, "p50_us": null, "p99_us": null, "max_us": null});
    }
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    let at = |q: f64| values[((values.len() - 1) as f64 * q).ceil() as usize];
    json!({"count": values.len(), "p50_us": at(0.5), "p99_us": at(0.99), "max_us": at(1.0)})
}
fn us(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e6
}
fn payload(id: usize, size: usize) -> Vec<u8> {
    let mut state = 0x123456789abcdefu64 ^ id as u64;
    let mut value: Vec<u8> = (0..size)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();
    value[..8].copy_from_slice(&(id as u64).to_le_bytes());
    value
}
fn seed(path: &Path) -> Store {
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
                    id: COLD,
                    value: Some(&payload(0, 65536)),
                },
            ],
        )
        .unwrap();
    store
}
fn commit(store: &mut Store, bytes: &[u8]) {
    store
        .edit_components(
            OBJECT,
            store.head(OBJECT),
            Some(store.published(OBJECT)),
            &[ComponentUpdate {
                id: HOT,
                value: Some(bytes),
            }],
        )
        .unwrap();
}
fn verify(path: &Path, committed: usize, last_id: usize) {
    let store = Store::open_components(path).unwrap();
    assert_eq!(store.head(OBJECT), Revision::new(committed as u64 + 1));
    assert_eq!(store.published(OBJECT), store.head(OBJECT));
    let snapshot = store.get_components(OBJECT).unwrap();
    assert_eq!(
        &snapshot.get(HOT).unwrap()[..8],
        (last_id as u64).to_le_bytes()
    );
    assert_eq!(snapshot.get(COLD).unwrap(), payload(0, 65536));
}

fn load(base: &Path, name: &str) -> Value {
    let config = Config::named(name);
    let dir = tempfile::tempdir_in(base).unwrap();
    let path = dir.path().join("objects");
    let writer_path = path.clone();
    let budget = Arc::new(Budget {
        usage: Mutex::new(Usage::default()),
        config,
    });
    // Admission credits bound this channel, including the item executing at the owner.
    let (tx, rx) = mpsc::channel::<Command>();
    let (ready_tx, ready_rx) = mpsc::sync_channel(0);
    let owner = thread::spawn(move || {
        let mut store = seed(&writer_path);
        let mut result = Results::default();
        ready_tx.send(()).unwrap();
        for (index, command) in rx.into_iter().enumerate() {
            if config.pause && index == 32 {
                let start = Instant::now();
                // Models unavailable service, not a kernel writeback failure.
                thread::sleep(Duration::from_millis(100));
                result.pause_us = us(start.elapsed());
            }
            let start = Instant::now();
            result
                .queue_age
                .push(us(start.duration_since(command.admitted)));
            if command.deadline.is_some_and(|deadline| start >= deadline) {
                result.expired += 1;
                result
                    .expired_latency
                    .push(us(start.duration_since(command.scheduled)));
                continue;
            }
            commit(&mut store, &command.payload);
            let done = Instant::now();
            result.committed += 1;
            result.committed_large += usize::from(command.payload.len() > 8);
            result.committed_after_deadline +=
                usize::from(command.deadline.is_some_and(|d| done > d));
            result.last_id = command.id;
            result.service.push(us(done.duration_since(start)));
            result
                .success_latency
                .push(us(done.duration_since(command.scheduled)));
        }
        result
    });
    ready_rx.recv().unwrap();
    let start = Instant::now();
    let mut rejected_count = 0;
    let mut rejected_bytes = 0;
    let mut rejected_large = 0;
    let mut admitted = 0;
    let mut generator_lag = Vec::new();
    let mut rejection_latency = Vec::new();
    for i in 0..config.rate {
        let scheduled = start + Duration::from_secs_f64(i as f64 / config.rate as f64);
        if let Some(wait) = scheduled.checked_duration_since(Instant::now()) {
            thread::sleep(wait);
        }
        generator_lag.push(us(Instant::now().duration_since(scheduled)));
        let size = if config.mixed && i % 4 == 0 { 65536 } else { 8 };
        match budget.acquire(size) {
            Ok(permit) => {
                let admitted_at = Instant::now();
                // Allocate only after credit reservation; waiting senders cannot hoard payloads.
                let command = Command {
                    id: i + 1,
                    payload: payload(i + 1, size),
                    scheduled,
                    admitted: admitted_at,
                    deadline: config.deadline.map(|d| scheduled + d),
                    _permit: permit,
                };
                tx.send(command).unwrap();
                admitted += 1;
            }
            Err(reason) => {
                match reason {
                    Rejected::Count => rejected_count += 1,
                    Rejected::Bytes => rejected_bytes += 1,
                }
                rejected_large += usize::from(size > 8);
                rejection_latency.push(us(scheduled.elapsed()));
            }
        }
    }
    drop(tx);
    let result = owner.join().unwrap();
    let finished = Instant::now();
    let usage = budget.usage.lock().unwrap();
    assert_eq!((usage.count, usage.bytes), (0, 0));
    assert!(usage.peak_count <= config.count && usage.peak_bytes <= config.bytes);
    assert_eq!(config.rate, admitted + rejected_count + rejected_bytes);
    assert_eq!(admitted, result.committed + result.expired);
    verify(&path, result.committed, result.last_id);
    json!({
        "scenario": name, "offered": config.rate, "arrival_window_ms": 1000,
        "count_limit": config.count, "payload_byte_limit": config.bytes,
        "deadline_ms": config.deadline.map(|d| d.as_millis()),
        "pause_us": result.pause_us, "admitted": admitted,
        "rejected_count": rejected_count, "rejected_bytes": rejected_bytes,
        "offered_large": if config.mixed { config.rate.div_ceil(4) } else { 0 },
        "rejected_large": rejected_large, "committed_large": result.committed_large,
        "expired_before_execution": result.expired,
        "committed": result.committed, "committed_after_deadline": result.committed_after_deadline,
        "peak_outstanding_count": usage.peak_count, "peak_outstanding_payload_bytes": usage.peak_bytes,
        "total_elapsed_us": us(finished.duration_since(start)),
        "drain_after_arrival_window_us": us(finished.saturating_duration_since(start + Duration::from_secs(1))),
        "generator_lateness": distribution(&generator_lag),
        "success_latency": distribution(&result.success_latency),
        "expired_latency": distribution(&result.expired_latency),
        "rejection_latency": distribution(&rejection_latency),
        "queue_age_at_dequeue": distribution(&result.queue_age),
        "commit_service": distribution(&result.service),
        "verified_reopen": true,
    })
}

fn lost_reply(base: &Path) -> Value {
    let dir = tempfile::tempdir_in(base).unwrap();
    let path = dir.path().join("objects");
    let writer_path = path.clone();
    let (ready_tx, ready_rx) = mpsc::sync_channel(0);
    let (go_tx, go_rx) = mpsc::sync_channel(0);
    let (reply_tx, reply_rx) = mpsc::channel();
    let owner = thread::spawn(move || {
        let mut store = seed(&writer_path);
        ready_tx.send(()).unwrap();
        go_rx.recv().unwrap();
        commit(&mut store, &1u64.to_le_bytes());
        assert!(reply_tx.send(store.head(OBJECT)).is_err());
    });
    ready_rx.recv().unwrap();
    drop(reply_rx);
    go_tx.send(()).unwrap();
    owner.join().unwrap();
    verify(&path, 1, 1);
    json!({"scenario": "lost-reply", "receiver_dropped_before_execution": true,
           "committed": 1, "verified_reopen": true})
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3, "usage: storage_resilience BASE SCENARIO");
    let base = Path::new(&args[1]);
    let result = if args[2] == "lost-reply" {
        lost_reply(base)
    } else {
        load(base, &args[2])
    };
    println!("{result}");
}
