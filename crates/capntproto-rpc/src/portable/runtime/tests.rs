use super::*;
use std::{cell::Cell, sync::atomic::AtomicUsize};

#[derive(Default)]
struct Wakes(AtomicUsize);
impl Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct Probe {
    queued: Rc<Cell<usize>>,
    flushed: Rc<RefCell<Vec<usize>>>,
    done: Rc<Cell<bool>>,
    on_drop: Option<Box<dyn FnOnce()>>,
    complete_on_output: bool,
    progress_waker: Option<Waker>,
    progress_polls: Rc<Cell<usize>>,
}
impl Driver for Probe {
    fn progress(&mut self, cx: &mut Context<'_>) -> Poll<capnp::Result<()>> {
        self.progress_polls.set(self.progress_polls.get() + 1);
        self.progress_waker = Some(cx.waker().clone());
        if self.done.get() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
    fn output(&mut self, _: &mut Context<'_>) {
        if std::mem::take(&mut self.complete_on_output) {
            self.done.set(true);
            self.progress_waker.as_ref().unwrap().wake_by_ref();
        }
        let queued = self.queued.replace(0);
        if queued > 0 {
            self.flushed.borrow_mut().push(queued);
        }
    }
}
impl Drop for Probe {
    fn drop(&mut self) {
        if let Some(callback) = self.on_drop.take() {
            callback();
        }
    }
}
fn probe() -> Probe {
    Probe {
        queued: Rc::new(Cell::new(0)),
        flushed: Rc::default(),
        done: Rc::default(),
        on_drop: None,
        complete_on_output: false,
        progress_waker: None,
        progress_polls: Rc::default(),
    }
}
fn poll<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}
#[test]
fn related_application_work_flushes_once_at_end_of_turn() {
    let mut runtime = Runtime::new(1.try_into().unwrap());
    let driver = probe();
    let progress_polls = driver.progress_polls.clone();
    let queued = driver.queued.clone();
    let flushed = driver.flushed.clone();
    let _completion = runtime.handle().add_driver(Box::new(driver)).unwrap();
    let mut rounds = 0;
    let app = std::future::poll_fn(|cx| {
        rounds += 1;
        queued.set(queued.get() + 1);
        if rounds < 3 {
            cx.waker().wake_by_ref();
        }
        Poll::<()>::Pending
    });
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    assert!(pin!(runtime.run(app))
        .poll(&mut Context::from_waker(&waker))
        .is_pending());
    assert_eq!(&*flushed.borrow(), &[3]);
    assert_eq!(
        progress_polls.get(),
        1,
        "application wakes do not poll a sleeping driver"
    );
    assert_eq!(
        wakes.0.load(Ordering::SeqCst),
        0,
        "same-turn work does not enqueue another executor turn"
    );
}
#[test]
fn continuously_ready_application_cannot_starve_output() {
    let mut runtime = Runtime::new(1.try_into().unwrap());
    let driver = probe();
    let queued = driver.queued.clone();
    let flushed = driver.flushed.clone();
    let _completion = runtime.handle().add_driver(Box::new(driver)).unwrap();
    let app = std::future::poll_fn(|cx| {
        queued.set(queued.get() + 1);
        cx.waker().wake_by_ref();
        Poll::<()>::Pending
    });
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    assert!(pin!(runtime.run(app))
        .poll(&mut Context::from_waker(&waker))
        .is_pending());
    assert_eq!(&*flushed.borrow(), &[32]);
    assert_eq!(
        wakes.0.load(Ordering::SeqCst),
        1,
        "budget exhaustion schedules one continuation"
    );
}
#[test]
fn completion_reclaims_capacity_and_drop_closes_handles_without_borrowing() {
    let mut runtime = Runtime::new(1.try_into().unwrap());
    let handle = runtime.handle();
    let driver = probe();
    driver.done.set(true);
    let completion = handle.add_driver(Box::new(driver)).unwrap();
    assert_eq!(
        handle.add_driver(Box::new(probe())).err().unwrap().kind,
        capnp::ErrorKind::Overloaded
    );
    assert!(matches!(
        poll(pin!(runtime.run(completion))),
        Poll::Ready(Ok(()))
    ));
    let mut driver = probe();
    let observer = handle.clone();
    driver.on_drop = Some(Box::new(move || {
        assert_eq!(
            observer.add_driver(Box::new(probe())).err().unwrap().kind,
            capnp::ErrorKind::Disconnected
        );
    }));
    let mut completion = handle.add_driver(Box::new(driver)).unwrap();
    drop(runtime);
    assert!(matches!(
        poll(Pin::new(&mut completion)),
        Poll::Ready(Err(_))
    ));
    assert_eq!(
        handle.add_driver(Box::new(probe())).err().unwrap().kind,
        capnp::ErrorKind::Disconnected
    );
}

#[test]
fn output_completion_starts_the_next_turn_without_an_executor_round_trip() {
    let mut runtime = Runtime::new(1.try_into().unwrap());
    let mut driver = probe();
    driver.complete_on_output = true;
    let app = runtime.handle().add_driver(Box::new(driver)).unwrap();
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    assert!(pin!(runtime.run(app))
        .poll(&mut Context::from_waker(&waker))
        .is_ready());
    assert_eq!(wakes.0.load(Ordering::SeqCst), 0);
}

#[test]
fn ready_protocol_work_batches_within_one_bounded_output_turn() {
    struct Buffered {
        remaining: usize,
        queued: usize,
        batches: Rc<RefCell<Vec<usize>>>,
        output: Option<Waker>,
    }
    impl Driver for Buffered {
        fn progress(&mut self, cx: &mut Context<'_>) -> Poll<capnp::Result<()>> {
            if self.remaining > 0 {
                self.remaining -= 1;
                self.queued += 1;
                if let Some(output) = &self.output {
                    output.wake_by_ref();
                }
                cx.waker().wake_by_ref();
            }
            Poll::Pending
        }
        fn output(&mut self, cx: &mut Context<'_>) {
            self.output = Some(cx.waker().clone());
            if self.queued > 0 {
                self.batches
                    .borrow_mut()
                    .push(std::mem::take(&mut self.queued));
            }
        }
    }
    for (remaining, expected) in [(3, 3), (100, 32)] {
        let mut runtime = Runtime::new(1.try_into().unwrap());
        let batches = Rc::new(RefCell::new(Vec::new()));
        let _completion = runtime
            .handle()
            .add_driver(Box::new(Buffered {
                remaining,
                queued: 0,
                batches: batches.clone(),
                output: None,
            }))
            .unwrap();
        assert!(poll(pin!(runtime.run(std::future::pending::<()>()))).is_pending());
        assert_eq!(&*batches.borrow(), &[expected]);
    }
}

#[test]
fn application_completion_does_not_flush_or_implicitly_close_connections() {
    let mut runtime = Runtime::new(1.try_into().unwrap());
    let driver = probe();
    driver.queued.set(1);
    let flushed = driver.flushed.clone();
    let mut completion = runtime.handle().add_driver(Box::new(driver)).unwrap();
    assert!(poll(pin!(runtime.run(std::future::ready(())))).is_ready());
    assert!(flushed.borrow().is_empty());
    assert!(poll(Pin::new(&mut completion)).is_pending());
    drop(runtime);
    assert!(matches!(
        poll(Pin::new(&mut completion)),
        Poll::Ready(Err(_))
    ));
}

#[derive(Default)]
struct ReadyState {
    progress: Cell<usize>,
    output: Cell<usize>,
    progress_wake: RefCell<Option<Waker>>,
    output_wake: RefCell<Option<Waker>>,
    done: Cell<bool>,
    self_waking_output: Cell<bool>,
    self_waking_progress: Cell<bool>,
}
struct ReadyDriver(Rc<ReadyState>);
impl Driver for ReadyDriver {
    fn progress(&mut self, cx: &mut Context<'_>) -> Poll<capnp::Result<()>> {
        self.0.progress.set(self.0.progress.get() + 1);
        *self.0.progress_wake.borrow_mut() = Some(cx.waker().clone());
        if self.0.self_waking_progress.get() {
            cx.waker().wake_by_ref();
        }
        if self.0.done.get() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
    fn output(&mut self, cx: &mut Context<'_>) {
        self.0.output.set(self.0.output.get() + 1);
        *self.0.output_wake.borrow_mut() = Some(cx.waker().clone());
        if self.0.self_waking_output.get() {
            cx.waker().wake_by_ref();
        }
    }
}
fn park(runtime: &mut Runtime) {
    assert!(poll(pin!(runtime.run(std::future::pending::<()>()))).is_pending());
}
#[test]
fn multiple_systems_keep_phase_wakes_generation_ownership_and_bounded_admission() {
    let counts: &[usize] = if cfg!(miri) { &[3] } else { &[32, 129, 257] };
    for &count in counts {
        let mut runtime = Runtime::new(count.try_into().unwrap());
        let handle = runtime.handle();
        let states: Vec<_> = (0..count).map(|_| Rc::new(ReadyState::default())).collect();
        let completions: Vec<_> = states
            .iter()
            .map(|state| {
                handle
                    .add_driver(Box::new(ReadyDriver(state.clone())))
                    .unwrap()
            })
            .collect();
        park(&mut runtime);
        assert!(states
            .iter()
            .all(|state| state.progress.get() == 1 && state.output.get() == 1));
        let first = states[0].progress_wake.borrow().as_ref().unwrap().clone();
        first.wake_by_ref();
        park(&mut runtime);
        assert_eq!(states[0].progress.get(), 2);
        assert!(states[1..].iter().all(|state| state.progress.get() == 1));
        assert!(states.iter().all(|state| state.output.get() == 1));
        states[count - 1]
            .output_wake
            .borrow()
            .as_ref()
            .unwrap()
            .wake_by_ref();
        park(&mut runtime);
        assert_eq!(states[count - 1].output.get(), 2);
        assert_eq!(states[0].progress.get(), 2, "output woke unrelated input");
        assert_eq!(
            handle.add_driver(Box::new(probe())).err().unwrap().kind,
            capnp::ErrorKind::Overloaded
        );
        let old_output = states[0].output_wake.borrow().as_ref().unwrap().clone();
        states[0].done.set(true);
        first.wake_by_ref();
        park(&mut runtime);
        let replacement = Rc::new(ReadyState::default());
        let completion = handle
            .add_driver(Box::new(ReadyDriver(replacement.clone())))
            .unwrap();
        // Completion interest does not own the driver or its capacity.
        drop(completion);
        park(&mut runtime);
        assert_eq!(replacement.progress.get(), 1);
        assert_eq!(replacement.output.get(), 1);
        first.wake_by_ref();
        old_output.wake_by_ref();
        park(&mut runtime);
        assert_eq!(
            replacement.progress.get(),
            1,
            "old input wake reached replacement"
        );
        assert_eq!(
            replacement.output.get(),
            1,
            "old output wake reached replacement"
        );
        drop(runtime);
        first.wake();
        old_output.wake();
        assert_eq!(
            handle.add_driver(Box::new(probe())).err().unwrap().kind,
            capnp::ErrorKind::Disconnected
        );
        drop(completions);
    }
}

#[test]
fn multiple_system_completion_reclaims_slot_before_reentrant_destructor_admission() {
    let mut runtime = Runtime::new(2.try_into().unwrap());
    let handle = runtime.handle();
    let observer = handle.clone();
    let dropped = Rc::new(Cell::new(false));
    let flag = dropped.clone();
    let mut first = probe();
    first.done.set(true);
    first.on_drop = Some(Box::new(move || {
        flag.set(true);
        let closing = observer.clone();
        let mut replacement = probe();
        replacement.on_drop = Some(Box::new(move || {
            assert_eq!(
                closing.add_driver(Box::new(probe())).err().unwrap().kind,
                capnp::ErrorKind::Disconnected
            );
        }));
        drop(observer.add_driver(Box::new(replacement)).unwrap());
    }));
    let first = handle.add_driver(Box::new(first)).unwrap();
    let _second = handle.add_driver(Box::new(probe())).unwrap();
    assert!(matches!(
        poll(pin!(runtime.run(first))),
        Poll::Ready(Ok(()))
    ));
    assert!(dropped.get());
    assert_eq!(
        handle.add_driver(Box::new(probe())).err().unwrap().kind,
        capnp::ErrorKind::Overloaded
    );
    park(&mut runtime);
    drop(runtime);
}

#[test]
fn multiple_output_only_self_wakes_are_bounded_and_never_repoll_input() {
    let mut runtime = Runtime::new(2.try_into().unwrap());
    let state = Rc::new(ReadyState::default());
    state.self_waking_output.set(true);
    let _done = runtime
        .handle()
        .add_driver(Box::new(ReadyDriver(state.clone())))
        .unwrap();
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    assert!(pin!(runtime.run(std::future::pending::<()>()))
        .poll(&mut Context::from_waker(&waker))
        .is_pending());
    assert_eq!(state.progress.get(), 1);
    assert_eq!(state.output.get(), 32);
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
}

#[test]
fn retired_self_wake_does_not_keep_other_systems_or_the_executor_busy() {
    let mut runtime = Runtime::new(2.try_into().unwrap());
    let state = Rc::new(ReadyState::default());
    let _first = runtime
        .handle()
        .add_driver(Box::new(ReadyDriver(state.clone())))
        .unwrap();
    let _second = runtime.handle().add_driver(Box::new(probe())).unwrap();
    park(&mut runtime);
    state.done.set(true);
    state.self_waking_progress.set(true);
    state.progress_wake.borrow().as_ref().unwrap().wake_by_ref();
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    assert!(pin!(runtime.run(std::future::pending::<()>()))
        .poll(&mut Context::from_waker(&waker))
        .is_pending());
    assert_eq!(
        wakes.0.load(Ordering::SeqCst),
        0,
        "a retired bit consumed every rotation"
    );
}

fn run_dense_ready_rounds(runtime: &mut Runtime, states: &[Rc<ReadyState>], rounds: usize) {
    for _ in 0..rounds {
        for state in states {
            for wake in [&state.progress_wake, &state.output_wake] {
                let wake = wake.borrow();
                let wake = wake.as_ref().unwrap();
                wake.wake_by_ref();
                wake.wake_by_ref();
            }
        }
        park(runtime);
    }
}

#[test]
fn dense_ready_systems_coalesce_duplicate_wakes_without_allocating() {
    let count = if cfg!(miri) { 3 } else { 32 };
    let mut runtime = Runtime::new(count.try_into().unwrap());
    let states: Vec<_> = (0..count).map(|_| Rc::new(ReadyState::default())).collect();
    let completions: Vec<_> = states
        .iter()
        .map(|state| {
            runtime
                .handle()
                .add_driver(Box::new(ReadyDriver(state.clone())))
                .unwrap()
        })
        .collect();
    park(&mut runtime);
    let rounds = if cfg!(miri) { 2 } else { 128 };
    let allocations = allocation_counter::measure(|| {
        run_dense_ready_rounds(&mut runtime, &states, rounds);
    });
    assert_eq!(allocations.count_total, 0);
    assert!(states
        .iter()
        .all(|state| state.progress.get() == rounds + 1 && state.output.get() == rounds + 1));
    drop(completions);
}
