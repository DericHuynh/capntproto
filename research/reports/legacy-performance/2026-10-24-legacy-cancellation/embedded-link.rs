//! Link-only core cancellation probe. No global allocator or hosted executor.
#![no_std]
#![no_main]
use capntproto_rpc_engine::completion::cancellation::Cancellation;
use core::{
    cell::{Cell, RefCell},
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

struct Borrowed<'a> {
    ready: &'a Cell<bool>,
    polls: &'a Cell<u32>,
    value: &'a u32,
}
impl Future for Borrowed<'_> {
    type Output = u32;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        self.polls.set(self.polls.get() + 1);
        if self.ready.get() {
            Poll::Ready(*self.value)
        } else {
            Poll::Pending
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let value = core::hint::black_box(41);
    let ready = Cell::new(false);
    let polls = Cell::new(0);
    let canceled = Cell::new(false);
    let waker = RefCell::new(None);
    let cancellation = Cancellation::new(
        core::hint::black_box(&canceled),
        core::hint::black_box(&waker),
    );
    let mut future = Borrowed {
        ready: &ready,
        polls: &polls,
        value: &value,
    };
    let mut cx = Context::from_waker(Waker::noop());
    assert!(cancellation
        .poll_task(Pin::new(&mut future), &mut cx)
        .is_pending());
    assert!(cancellation
        .poll_task(Pin::new(&mut future), &mut cx)
        .is_pending());
    cancellation.cancel();
    assert!(cancellation
        .poll_task(Pin::new(&mut future), &mut cx)
        .is_ready());
    assert_eq!(polls.get(), 2);
    cancellation.clear();
    canceled.set(core::hint::black_box(false));
    ready.set(core::hint::black_box(true));
    assert!(cancellation
        .poll_task(Pin::new(&mut future), &mut cx)
        .is_ready());
    assert_eq!(polls.get(), 3);
    assert!(waker.borrow().is_none());
    cancellation.clear();
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
