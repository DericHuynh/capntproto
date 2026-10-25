//! Actual caller-pinned futures, bounded admission and static standard wakers.
//! No allocator, reference-counted wake owner or hosted executor.
#![no_std]
#![no_main]
use capntproto_rpc_engine::{admission, readiness, tasks, wake};
use core::{
    cell::Cell,
    future::Future,
    marker::PhantomPinned,
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll},
};

static READY: readiness::Storage<4> = readiness::Storage::new();
static NOTIFIED: AtomicBool = AtomicBool::new(false);
fn notify() {
    NOTIFIED.store(true, Ordering::Release);
}
static SIGNALS: [tasks::Signal<4>; 4] = tasks::signals(&READY, notify);
struct Parent;
impl wake::Wake for Parent {
    fn wake_by_ref(&self) {
        notify();
    }
}
static PARENT: wake::Static<Parent> = wake::Static::new(Parent);

struct Task<'a> {
    index: u32,
    progress: &'a Cell<u8>,
    dropped: &'a Cell<u8>,
    _pin: PhantomPinned,
}
impl Future for Task<'_> {
    type Output = u32;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<u32> {
        let progress = self.progress.get();
        self.progress.set(progress + 1);
        if progress == 0 {
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(self.index)
        }
    }
}
impl Drop for Task<'_> {
    fn drop(&mut self) {
        self.dropped.set(self.dropped.get() + 1);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let progress = [const { Cell::new(0) }; 4];
    let dropped = Cell::new(0);
    let mut cursor = 0usize;
    let escaped = {
        let wakes = core::array::from_fn(|index| SIGNALS[index].waker());
        let mut inbox = admission::Slot::<Task<'_>, 4>::new();
        let (sender, mut receiver) = inbox.pair();
        for index in 0..4 {
            assert!(sender
                .send(Task {
                    index: index as u32,
                    progress: &progress[index],
                    dropped: &dropped,
                    _pin: PhantomPinned
                })
                .is_ok());
        }
        drop(sender);
        let parent = PARENT.waker();
        let mut cx = Context::from_waker(&parent);
        let mut pinned = core::pin::pin!(tasks::PinnedSlots::new());
        let mut set = tasks::Set::new(
            pinned.as_mut(),
            READY.queue(),
            &wakes,
            core::hint::black_box(&mut cursor),
        );
        while let Poll::Ready(Some(future)) = receiver.poll_next(&mut cx) {
            assert!(set.try_push(future, notify).is_ok());
        }
        let mut completed = 0;
        let mut total = 0;
        for _ in 0..16 {
            if !NOTIFIED.swap(false, Ordering::AcqRel) {
                continue;
            }
            while let Poll::Ready(Some(value)) = set.poll_next(&mut cx) {
                completed += 1;
                total += value;
            }
            if completed == 4 {
                break;
            }
        }
        assert_eq!(completed, 4);
        assert_eq!(total, 6);
        assert!(progress.iter().all(|p| p.get() == 2));
        assert_eq!(dropped.get(), 4);
        drop(set);
        wakes[0].clone()
    };
    assert_eq!(dropped.get(), 4);
    // This wake runs after both the task set and the actual future owners end.
    escaped.wake();
    assert_eq!(READY.queue().front(cursor), Some(0));
    READY.queue().consume(cursor, 0);
    loop {
        core::hint::spin_loop();
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
