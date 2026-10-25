//! Link-only caller-owned admission lifecycle, without an allocator.
#![no_std]
#![no_main]
use capntproto_rpc_engine::admission::Slot;
use core::task::{Context, Poll, Waker};

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut slot = Slot::<u32, 2>::new();
    let slot = core::hint::black_box(&mut slot);
    for round in 0..3 {
        let (sender, mut receiver) = slot.pair();
        let second = sender.clone();
        assert_eq!(sender.send(round), Ok(()));
        assert_eq!(second.send(round + 1), Ok(()));
        assert_eq!(sender.send(99), Err(99));
        let mut context = Context::from_waker(Waker::noop());
        assert_eq!(receiver.poll_next(&mut context), Poll::Ready(Some(round)));
        assert_eq!(sender.send(7), Ok(()));
        drop(sender);
        assert_eq!(
            receiver.poll_next(&mut context),
            Poll::Ready(Some(round + 1))
        );
        assert_eq!(receiver.poll_next(&mut context), Poll::Ready(Some(7)));
        assert!(receiver.poll_next(&mut context).is_pending());
        drop(second);
        assert_eq!(receiver.poll_next(&mut context), Poll::Ready(None));
    }
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
