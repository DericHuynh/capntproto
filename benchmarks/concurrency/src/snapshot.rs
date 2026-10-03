use arc_swap::ArcSwap;
use serde_json::{json, Value};
use std::{
    hint::black_box,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Barrier, Mutex, RwLock,
    },
    thread,
    time::{Duration, Instant},
};

const TOTAL: usize = 262_144;

struct State {
    generation: u64,
    values: [u64; 8],
}

impl State {
    fn new(generation: u64) -> Self {
        Self {
            generation,
            values: [generation; 8],
        }
    }
    fn check(&self) {
        let state = black_box(self);
        assert!(state.values.iter().all(|v| *v == state.generation));
        black_box(state);
    }
}

enum Shared {
    Mutex(Mutex<Arc<State>>),
    RwLock(RwLock<Arc<State>>),
    Swap(ArcSwap<State>, bool),
}

impl Shared {
    fn read(&self) {
        match self {
            Self::Mutex(value) => {
                let state = value.lock().unwrap().clone();
                state.check();
            }
            Self::RwLock(value) => {
                let state = value.read().unwrap().clone();
                state.check();
            }
            Self::Swap(value, false) => value.load().check(),
            Self::Swap(value, true) => value.load_full().check(),
        }
    }
    fn publish(&self, generation: u64) {
        let new = Arc::new(State::new(generation));
        match self {
            Self::Mutex(value) => *value.lock().unwrap() = new,
            Self::RwLock(value) => *value.write().unwrap() = new,
            Self::Swap(value, _) => value.store(new),
        }
    }
}

pub fn run(variant: &str, readers: usize) -> Value {
    let shared = Arc::new(match variant {
        "mutex" => Shared::Mutex(Mutex::new(Arc::new(State::new(0)))),
        "rwlock" => Shared::RwLock(RwLock::new(Arc::new(State::new(0)))),
        "guard" | "owned" => Shared::Swap(ArcSwap::from_pointee(State::new(0)), variant == "owned"),
        name => panic!("unknown snapshot {name}"),
    });
    let barrier = Arc::new(Barrier::new(readers + 2));
    let stop = Arc::new(AtomicBool::new(false));
    let publisher = {
        let shared = shared.clone();
        let stop = stop.clone();
        let barrier = barrier.clone();
        thread::spawn(move || {
            let mut publications = 0;
            barrier.wait();
            while !stop.load(Ordering::Acquire) {
                publications += 1;
                shared.publish(publications);
                thread::sleep(Duration::from_micros(100));
            }
            publications
        })
    };
    let mut handles = Vec::new();
    for _ in 0..readers {
        let shared = shared.clone();
        let barrier = barrier.clone();
        handles.push(thread::spawn(move || {
            barrier.wait();
            for _ in 0..TOTAL / readers {
                shared.read();
            }
        }));
    }
    let started = Instant::now();
    barrier.wait();
    for handle in handles {
        handle.join().unwrap();
    }
    let elapsed = started.elapsed();
    stop.store(true, Ordering::Release);
    let publications = publisher.join().unwrap();
    let mut result = super::metrics(TOTAL, elapsed, Vec::new());
    result["publications"] = json!(publications);
    result
}
