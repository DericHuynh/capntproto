//! Local async schedule recording. Production futures keep their real wakers;
//! only their executor and explicit actor yield points belong to Shuttle.
use serde::{Deserialize, Serialize};
pub use shuttle::future::{block_on, spawn_local, yield_now, JoinHandle};
use shuttle::{
    scheduler::{RandomScheduler, ReplayScheduler, Schedule, Scheduler, Task, TaskId},
    Config, FailurePersistence, MaxSteps, Runner,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub const ITERATIONS: usize = 256;
pub const SEED: u64 = 0x5250_5343_4845_4431;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    pub seed: u64,
    pub tasks: Vec<usize>,
    pub events: Vec<(String, u64)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Saved {
    pub format: u32,
    pub case: String,
    pub runs: Vec<Run>,
}

pub struct Batch {
    pub runs: Vec<Run>,
    pub sampled: bool,
}

#[derive(Clone, Default)]
pub struct Trace(Arc<Mutex<Vec<Run>>>);
impl Trace {
    pub fn event(&self, name: &str, value: u64) {
        self.0
            .lock()
            .unwrap()
            .last_mut()
            .unwrap()
            .events
            .push((name.into(), value));
    }
    fn runs(&self) -> Vec<Run> {
        self.0.lock().unwrap().clone()
    }
}

struct Recording<S> {
    inner: S,
    trace: Trace,
}
impl<S: Scheduler> Scheduler for Recording<S> {
    fn new_execution(&mut self) -> Option<Schedule> {
        let schedule = self.inner.new_execution()?;
        self.trace.0.lock().unwrap().push(Run {
            seed: schedule.seed,
            ..Run::default()
        });
        Some(schedule)
    }
    fn next_task(
        &mut self,
        runnable: &[&Task],
        current: Option<TaskId>,
        yielding: bool,
    ) -> Option<TaskId> {
        let next = self.inner.next_task(runnable, current, yielding)?;
        self.trace
            .0
            .lock()
            .unwrap()
            .last_mut()
            .unwrap()
            .tasks
            .push(next.into());
        Some(next)
    }
    fn next_u64(&mut self) -> u64 {
        panic!("schedule fixture must use explicit inputs, not random data");
    }
}

fn config(directory: &std::path::Path) -> Config {
    let mut config = Config::new();
    config.max_steps = MaxSteps::FailAfter(512);
    config.max_time = None;
    config.failure_persistence = FailurePersistence::File(Some(directory.into()));
    config
}

/// Run fixed-seed schedules, replay every complete schedule and compare all
/// semantic events. Failures preserve the failing decisions and partial events.
pub fn check(name: &'static str, case: fn(Trace)) -> Batch {
    assert!(
        std::env::var_os("SHUTTLE_RANDOM_SEED").is_none(),
        "use CAPNTPROTO_SCHEDULE_REPLAY to replay saved runs"
    );
    let directory = std::env::var_os("CAPNTPROTO_SCHEDULE_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::verification::root().join("target/verification/schedules"))
        .join(name);
    std::fs::create_dir_all(&directory).unwrap();
    if let Some(path) = std::env::var_os("CAPNTPROTO_SCHEDULE_REPLAY") {
        let saved: Saved = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved.format, 1);
        assert_eq!(saved.case, name);
        assert!(!saved.runs.is_empty());
        for expected in &saved.runs {
            replay(expected, case, &directory);
        }
        return Batch {
            runs: saved.runs,
            sampled: false,
        };
    }
    let trace = Trace::default();
    let recorded = trace.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let count = Runner::new(
            Recording {
                inner: RandomScheduler::new_from_seed(SEED, ITERATIONS),
                trace: trace.clone(),
            },
            config(&directory),
        )
        .run(move || {
            case(recorded.clone());
            recorded.event("case-complete", 1);
        });
        assert_eq!(count, ITERATIONS);
    }));
    let runs = trace.runs();
    std::fs::write(
        directory.join("runs.json"),
        serde_json::to_vec_pretty(&Saved {
            format: 1,
            case: name.into(),
            runs: runs.clone(),
        })
        .unwrap(),
    )
    .unwrap();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
    assert_eq!(runs.len(), ITERATIONS);
    for expected in &runs {
        replay(expected, case, &directory);
    }
    eprintln!("{name}: {ITERATIONS} schedules and exact replays");
    Batch {
        runs,
        sampled: true,
    }
}

fn replay(expected: &Run, case: fn(Trace), directory: &std::path::Path) {
    assert!(!expected.tasks.is_empty() && expected.tasks.len() <= 512);
    let trace = Trace::default();
    let recorded = trace.clone();
    let scheduler = ReplayScheduler::new_from_schedule(Schedule::new_from_task_ids(
        expected.seed,
        expected.tasks.iter().copied(),
    ));
    assert_eq!(
        Runner::new(
            Recording {
                inner: scheduler,
                trace: trace.clone()
            },
            config(directory)
        )
        .run(move || {
            case(recorded.clone());
            recorded.event("case-complete", 1);
        }),
        1
    );
    assert_eq!(
        trace.runs().as_slice(),
        std::slice::from_ref(expected),
        "schedule/event replay diverged"
    );
}
