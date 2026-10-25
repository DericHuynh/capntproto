//! Link-only probe of legacy's extracted reply path. No global allocator.
#![no_std]
#![no_main]
use capntproto_rpc_engine::completion::direct;
use core::{cell::{Cell, RefCell}, future::Future, pin::Pin, task::{Context, Poll, Waker}};
struct Borrowed<'a> { ready: &'a Cell<bool>, value: &'a u32 }
impl<'a> Future for Borrowed<'a> {
    type Output = &'a u32;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        if self.ready.get() { Poll::Ready(self.value) } else { Poll::Pending }
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let value = core::hint::black_box(41);
    let ready = Cell::new(false);
    let owner = RefCell::new(direct::Reply::new());
    let mut cx = Context::from_waker(Waker::noop());
    assert!(direct::poll(core::hint::black_box(&owner), &mut cx).is_pending());
    assert!(direct::deliver(core::hint::black_box(&owner), Borrowed { ready: &ready, value: &value }).is_none());
    assert!(direct::poll(core::hint::black_box(&owner), &mut cx).is_pending());
    ready.set(core::hint::black_box(true));
    assert_eq!(direct::poll(core::hint::black_box(&owner), &mut cx), Poll::Ready(&value));
    direct::release(core::hint::black_box(&owner));
    assert!(direct::deliver(core::hint::black_box(&owner), Borrowed { ready: &ready, value: &value }).is_some());
    loop { core::hint::spin_loop(); }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! { loop { core::hint::spin_loop(); } }
