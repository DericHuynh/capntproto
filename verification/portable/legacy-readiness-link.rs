//! Link-only caller-backed readiness and borrowed-future probe.
//! No global allocator, hosted wake owner, task box or executor dependency.
#![no_std]
#![no_main]

use capntproto_rpc_engine::readiness::{Queue, Storage};
use core::{
    cell::Cell,
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

struct Task<'a> {
    index: usize,
    progress: &'a Cell<u8>,
    queue: Queue<'a, 4>,
    notified: &'a Cell<bool>,
}
impl Future for Task<'_> {
    type Output = usize;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<usize> {
        let progress = self.progress.get();
        self.progress.set(progress + 1);
        if progress == 0 {
            self.queue.enqueue(self.index, || self.notified.set(true));
            Poll::Pending
        } else {
            Poll::Ready(self.index)
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let storage = Storage::<4>::new();
    let queue = core::hint::black_box(&storage).queue();
    let progress = [const { Cell::new(0) }; 4];
    let notified = Cell::new(false);
    let mut tasks = core::array::from_fn::<_, 4, _>(|index| Task {
        index,
        progress: &progress[index],
        queue,
        notified: &notified,
    });
    for index in 0..4 {
        queue.enqueue(index, || notified.set(true));
        queue.enqueue(index, || panic!("duplicate notification"));
    }
    let mut position = 0usize;
    let mut completed = 0usize;
    let mut context = Context::from_waker(Waker::noop());
    while let Some(index) = queue.front(position) {
        queue.consume(position, index);
        position = position.wrapping_add(1);
        if let Poll::Ready(done) = Pin::new(&mut tasks[index]).poll(&mut context) {
            assert_eq!(done, index);
            completed += 1;
        }
    }
    assert!(notified.get());
    assert_eq!(position, 8);
    assert_eq!(completed, 4);
    assert!(progress.iter().all(|p| p.get() == 2));
    // Reuse the caller's queue storage only after releasing each notification.
    queue.enqueue(3, || notified.set(true));
    assert_eq!(queue.front(position), Some(3));
    queue.consume(position, 3);
    assert_eq!(queue.front(position.wrapping_add(1)), None);
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
