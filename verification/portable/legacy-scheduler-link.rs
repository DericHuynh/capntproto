//! The shared legacy scheduler with owned !Unpin futures, two fixed ready
//! lanes, direct connection polling, deferred publication and borrowed inbox.
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
use capntproto_rpc_engine::{
    admission, completion, readiness,
    scheduler::{self, bounded, Command},
    tasks, wake,
};
use core::{
    cell::Cell,
    future::Future,
    marker::PhantomPinned,
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll},
};

static READY: readiness::Storage<4> = readiness::Storage::new();
static LAST: readiness::Storage<4> = readiness::Storage::new();
static REGULAR: AtomicBool = AtomicBool::new(false);
static DEFERRED: AtomicBool = AtomicBool::new(false);
static NOTIFIED: AtomicBool = AtomicBool::new(true);
fn regular() {
    REGULAR.store(true, Ordering::Release);
    notify();
}
fn deferred() {
    DEFERRED.store(true, Ordering::Release);
    notify();
}
fn notify() {
    NOTIFIED.store(true, Ordering::Release);
}
static SIGNALS: [tasks::Signal<4>; 4] = tasks::signals(&READY, regular);
static LAST_SIGNALS: [tasks::Signal<4>; 4] = tasks::signals(&LAST, deferred);
struct Parent(fn());
impl wake::Wake for Parent {
    fn wake_by_ref(&self) {
        (self.0)();
    }
}
static PARENT: wake::Static<Parent> = wake::Static::new(Parent(notify));
static ACTIVITY: wake::Static<Parent> = wake::Static::new(Parent(regular));
static LAST_ACTIVITY: wake::Static<Parent> = wake::Static::new(Parent(deferred));

struct Task<'a> {
    progress: &'a Cell<u8>,
    dropped: &'a Cell<u8>,
    _pin: PhantomPinned,
}
impl Future for Task<'_> {
    type Output = Result<(), u8>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let previous = self.progress.get();
        self.progress.set(previous + 1);
        if previous == 0 {
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }
}
impl Drop for Task<'_> {
    fn drop(&mut self) {
        self.dropped.set(self.dropped.get() + 1);
    }
}
struct Reaper<'a>(&'a Cell<u8>);
impl scheduler::Reaper<u8> for Reaper<'_> {
    fn task_succeeded(&mut self) {
        self.0.set(self.0.get() + 1);
    }
    fn task_failed(&mut self, _: u8) {
        panic!("unexpected task failure");
    }
}
fn exercise() {
    let progress = [const { Cell::new(0) }; 4];
    let dropped = Cell::new(0);
    let completed = Cell::new(0);
    let (mut cursor, mut last_cursor) = (0, 0);
    {
        let mut owners = core::pin::pin!(wake::Slots::<_, 4>::new());
        let places = owners.as_mut().places();
        let mut stop = completion::Slot::new();
        let (_stopper, stopped) = stop.pair().unwrap();
        let mut channel = admission::Slot::<_, 4>::new();
        let (sender, receiver) = channel.pair();
        for (index, place) in places.into_iter().enumerate() {
            let task = place
                .insert_erased(Task {
                    progress: &progress[index],
                    dropped: &dropped,
                    _pin: PhantomPinned,
                })
                .ok()
                .unwrap();
            let command = match index {
                0 => Command::Task(task),
                3 => Command::Last(task),
                _ => Command::Connection(task),
            };
            assert!(sender.send(command).is_ok());
        }
        drop(sender);
        let mut source = bounded::Inbox::new(receiver, stopped);
        let wakes = core::array::from_fn(|i| SIGNALS[i].waker());
        let last_wakes = core::array::from_fn(|i| LAST_SIGNALS[i].waker());
        let mut slots = core::pin::pin!(tasks::PinnedSlots::new());
        let mut last_slots = core::pin::pin!(tasks::PinnedSlots::new());
        let mut regular = bounded::Tasks::new(tasks::Set::new(
            slots.as_mut(),
            READY.queue(),
            &wakes,
            &mut cursor,
        ));
        let mut last = bounded::Tasks::new(tasks::Set::new(
            last_slots.as_mut(),
            LAST.queue(),
            &last_wakes,
            &mut last_cursor,
        ));
        let parent = PARENT.waker();
        let activity = ACTIVITY.waker();
        let last_activity = LAST_ACTIVITY.waker();
        let mut connection = None;
        let mut publication = scheduler::Publication::new();
        let mut empty = bounded::Notifications::<1>::new();
        let mut reaper = Reaper(&completed);
        let mut runtime = scheduler::Parts {
            enqueued: &mut source,
            connection: &mut connection,
            in_progress: &mut regular,
            last: &mut last,
            activity: scheduler::Signal {
                ready: &REGULAR,
                parent: &bounded::StaticParent::new(&activity, |_| {}),
            },
            last_activity: scheduler::Signal {
                ready: &DEFERRED,
                parent: &bounded::StaticParent::new(&last_activity, |_| {}),
            },
            publication: &mut publication,
            defer: |cx| cx.waker().wake_by_ref(),
            on_empty_fulfillers: &mut empty,
            reaper: &mut reaper,
            idle_regular: 0,
        };
        let mut finished = false;
        for _ in 0..128 {
            if NOTIFIED.swap(false, Ordering::AcqRel) {
                match runtime.poll(&mut Context::from_waker(&parent)) {
                    Poll::Ready(Ok(())) => {
                        finished = true;
                        break;
                    }
                    Poll::Ready(Err(_)) => panic!("unexpected scheduler failure"),
                    Poll::Pending => {}
                }
            }
        }
        assert!(finished);
        assert_eq!(completed.get(), 4);
        assert_eq!(dropped.get(), 4);
        assert!(progress.iter().all(|value| value.get() == 2));
    }
    SIGNALS[0].waker().wake();
}
#[cfg(not(target_os = "none"))]
fn main() {
    exercise();
}
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    exercise();
    loop {
        core::hint::spin_loop();
    }
}
#[cfg(target_os = "none")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
