//! Caller-owned pending calls, reply observation and stale-cancellation safety.
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
use capntproto_rpc_engine::{completion, pending};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

fn exercise() {
    let mut old_reply = completion::Slot::new();
    let mut new_reply = completion::Slot::new();
    {
        let (old_sender, old_receiver) = old_reply.pair().unwrap();
        let (new_sender, new_receiver) = new_reply.pair().unwrap();
        let mut backing = pending::Storage::with_map(pending::Fixed::<_, _, 0>::new());
        let queue = backing.borrow();
        let old = queue.try_push(17, old_sender).ok().unwrap();
        let old = pending::Observed::new(old_receiver, old);
        let (input, producer) = queue.drain().next().unwrap();
        assert_eq!(input, 17);
        producer.send(19).unwrap();
        let new = queue.try_push(23, new_sender).ok().unwrap();
        assert_eq!(new.identity(), 1);
        let mut new = pending::Observed::new(new_receiver, new);
        drop(old);
        let (input, producer) = queue.drain().next().unwrap();
        assert_eq!(input, 23);
        producer.send(29).unwrap();
        assert_eq!(
            Pin::new(&mut new).poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Ok(29))
        );
        assert!(queue.drain().next().is_none());
    }
    // The dropped observer's abandoned result remains explicitly recoverable.
    assert_eq!(old_reply.take_undelivered(), Some(19));
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
