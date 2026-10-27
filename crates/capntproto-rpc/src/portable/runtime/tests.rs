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
    output_yield: Option<fn(&mut Context<'_>)>,
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
    fn output_yield(&self) -> Option<fn(&mut Context<'_>)> {
        self.output_yield
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
        output_yield: None,
    }
}
#[test]
fn deferred_physical_output_admits_host_readiness_once_before_each_batch() {
    let mut runtime = Runtime::new(1.try_into().unwrap());
    let mut driver = probe();
    driver.output_yield = Some(|cx| cx.waker().wake_by_ref());
    let queued = driver.queued.clone();
    let flushed = driver.flushed.clone();
    let _completion = runtime.handle().add_driver(Box::new(driver)).unwrap();
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    let mut app = pin!(std::future::pending::<()>());
    queued.set(1);
    assert!(runtime.poll(app.as_mut(), &mut cx).is_pending());
    assert!(flushed.borrow().is_empty());
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    // Another peer becomes readable during the deferred host turn. Its
    // application continuation must run before the already-ready writer.
    let mut added = false;
    let mut ready_peer = pin!(std::future::poll_fn(|_| {
        if !added {
            queued.set(queued.get() + 1);
            added = true;
        }
        Poll::<()>::Pending
    }));
    runtime.application_waker.wake_by_ref();
    assert!(runtime.poll(ready_peer.as_mut(), &mut cx).is_pending());
    assert_eq!(&*flushed.borrow(), &[2]);
    assert_eq!(wakes.0.load(Ordering::SeqCst), 2);
    // Deferral renews only for a new output wake, and never postpones the
    // same batch twice, even when the next progress phase has more work.
    queued.set(1);
    runtime.systems[0].output_waker.wake_by_ref();
    assert!(runtime.poll(app.as_mut(), &mut cx).is_pending());
    assert_eq!(&*flushed.borrow(), &[2]);
    assert!(runtime.poll(app.as_mut(), &mut cx).is_pending());
    assert_eq!(&*flushed.borrow(), &[2, 1]);
}

#[test]
fn deferred_physical_output_cannot_be_starved_by_continuous_progress() {
    let mut runtime = Runtime::new(1.try_into().unwrap());
    let mut driver = probe();
    driver.output_yield = Some(|_| panic!("budget exhaustion must force output"));
    let queued = driver.queued.clone();
    let flushed = driver.flushed.clone();
    let _completion = runtime.handle().add_driver(Box::new(driver)).unwrap();
    let app = std::future::poll_fn(|cx| {
        queued.set(queued.get() + 1);
        cx.waker().wake_by_ref();
        Poll::<()>::Pending
    });
    assert!(poll(pin!(runtime.run(app))).is_pending());
    assert_eq!(&*flushed.borrow(), &[32]);
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
