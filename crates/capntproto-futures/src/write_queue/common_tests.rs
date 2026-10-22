use super::*;
use capnp::message::{Builder, HeapAllocator};
use futures::FutureExt;
use std::sync::atomic::AtomicU64;

fn message() -> Builder<HeapAllocator> {
    let mut message = Builder::new_default();
    message.initn_root::<capnp::data::Builder>(0);
    message
}

#[test]
fn cumulative_flush_observers_fail_on_driver_drop_and_never_wrap_sequence_numbers() {
    let (sender, driver) = write_queue(futures::io::sink());
    let mut observed = sender.observe().unwrap();
    assert!(sender.observe().is_none());
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    sender.shared.queue.lock().unwrap().accepted = u64::MAX - 1;
    observed.send(message()).unwrap();
    assert!(observed.send(message()).is_err());
    assert_eq!(
        sender.len(),
        1,
        "overflow cannot append or change the previous barrier"
    );
    assert!(observed.poll_flush(&mut cx).is_pending());
    drop(driver);
    assert!(matches!(observed.poll_flush(&mut cx), Poll::Ready(Err(_))));
    assert!(observed.send(message()).is_err());
    drop(observed);
    assert!(
        sender.observe().is_some(),
        "observer ownership is released independently of IO"
    );
}

#[test]
fn cooperative_publication_only_schedules_unconsumed_work_and_releases_ready_waiters() {
    use std::{
        pin::Pin,
        sync::atomic::AtomicUsize,
        task::{Context, Wake, Waker},
    };
    struct Count(AtomicUsize);
    impl Wake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let (sender, driver) = write_queue(futures::io::sink());
    let mut driver = Box::pin(driver);
    let mut observed = sender.observe().unwrap();
    let count = Arc::new(Count(AtomicUsize::new(0)));
    let wake = Waker::from(count.clone());
    let mut cx = Context::from_waker(&wake);
    assert!(driver.as_mut().poll(&mut cx).is_pending());
    for _ in 0..3 {
        observed
            .send_and_drive(message(), || {
                assert!(driver.as_mut().poll(&mut cx).is_pending());
            })
            .unwrap();
        assert!(matches!(observed.poll_flush(&mut cx), Poll::Ready(Ok(()))));
        assert_eq!(
            count.0.load(Ordering::SeqCst),
            0,
            "synchronous completion must not schedule a redundant task"
        );
    }
    observed.send_and_drive(message(), || {}).unwrap();
    assert_eq!(count.0.load(Ordering::SeqCst), 1);
    assert!(driver.as_mut().poll(&mut cx).is_pending());
    assert!(matches!(observed.poll_flush(&mut cx), Poll::Ready(Ok(()))));
    let before = count.0.load(Ordering::SeqCst);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        observed
            .send_and_drive(message(), || panic!("driver callback panic"))
            .unwrap();
    }))
    .is_err());
    assert_eq!(
        count.0.load(Ordering::SeqCst),
        before + 1,
        "unwind must still notify accepted output"
    );
    assert!(observed.poll_flush(&mut cx).is_pending());
    assert!(Pin::new(&mut driver).poll(&mut cx).is_pending());
    assert!(matches!(observed.poll_flush(&mut cx), Poll::Ready(Ok(()))));
}

#[test]
fn buffered_batches_preserve_output_and_notify_on_flush_drop_and_unwind() {
    use std::{
        sync::atomic::AtomicUsize,
        task::{Context, Wake, Waker},
    };
    struct Count(AtomicUsize);
    impl Wake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let (sender, driver) = write_queue(futures::io::sink());
    let mut driver = Box::pin(driver);
    let count = Arc::new(Count(AtomicUsize::new(0)));
    let wake = Waker::from(count.clone());
    let mut cx = Context::from_waker(&wake);
    assert!(driver.as_mut().poll(&mut cx).is_pending());
    let mut observed = sender.observe().unwrap();
    for _ in 0..3 {
        observed.send_buffered(message()).unwrap();
    }
    assert_eq!(sender.len(), 3);
    assert_eq!(count.0.load(Ordering::SeqCst), 0);
    assert!(driver.as_mut().poll(&mut cx).is_pending());
    assert!(matches!(observed.poll_flush(&mut cx), Poll::Ready(Ok(()))));
    assert_eq!(count.0.load(Ordering::SeqCst), 0);

    observed.send_buffered(message()).unwrap();
    assert!(observed.poll_flush(&mut cx).is_pending());
    assert_eq!(count.0.load(Ordering::SeqCst), 1);
    assert!(driver.as_mut().poll(&mut cx).is_pending());
    assert!(matches!(observed.poll_flush(&mut cx), Poll::Ready(Ok(()))));
    assert_eq!(sender.len(), 0);

    let before = count.0.load(Ordering::SeqCst);
    observed.send_buffered(message()).unwrap();
    drop(observed);
    assert_eq!(count.0.load(Ordering::SeqCst), before + 1);
    assert_eq!(
        sender.len(),
        1,
        "dropping the observer retains accepted output"
    );
    assert!(driver.as_mut().poll(&mut cx).is_pending());
    assert_eq!(sender.len(), 0);

    let before = count.0.load(Ordering::SeqCst);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut observed = sender.observe().unwrap();
        observed.send_buffered(message()).unwrap();
        panic!("publication unwound before flush");
    }))
    .is_err());
    assert_eq!(count.0.load(Ordering::SeqCst), before + 1);
    assert_eq!(sender.len(), 1);
    assert!(driver.as_mut().poll(&mut cx).is_pending());
    assert_eq!(sender.len(), 0);
    let mut observed = sender.observe().unwrap();
    drop(driver);
    assert!(observed.send_buffered(message()).is_err());
}

#[test]
fn buffered_output_notifies_when_a_flush_waker_panics_during_observer_drop() {
    use std::{
        sync::atomic::AtomicUsize,
        task::{Context, Wake, Waker},
    };
    struct Count(AtomicUsize);
    impl Wake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    struct PanickingDrop;
    // This waker deliberately owns a panicking destructor; Waker::noop cannot
    // exercise unwinding while the queue releases a caller-owned flush waker.
    #[allow(clippy::manual_noop_waker)]
    impl Wake for PanickingDrop {
        fn wake(self: Arc<Self>) {}
    }
    impl Drop for PanickingDrop {
        fn drop(&mut self) {
            panic!("flush waker destructor");
        }
    }
    let (sender, driver) = write_queue(futures::io::sink());
    let mut driver = Box::pin(driver);
    let count = Arc::new(Count(AtomicUsize::new(0)));
    let wake = Waker::from(count.clone());
    let mut cx = Context::from_waker(&wake);
    assert!(driver.as_mut().poll(&mut cx).is_pending());
    let mut observed = sender.observe().unwrap();
    // A pending flush observer can retain an arbitrary caller-owned waker.
    let caller = Waker::from(Arc::new(PanickingDrop));
    sender.shared.flushed.register(&caller);
    drop(caller);
    observed.send_buffered(message()).unwrap();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(observed))).is_err());
    assert_eq!(count.0.load(Ordering::SeqCst), 1);
    assert_eq!(sender.len(), 1);
    assert!(
        sender.observe().is_some(),
        "observer ownership was released"
    );
    assert!(driver.as_mut().poll(&mut cx).is_pending());
    assert_eq!(sender.len(), 0, "accepted output still reaches its driver");
}

#[test]
fn stopped_queue_discards_new_messages_and_fails_outstanding_receipts() {
    let (sender, driver) = write_queue(futures::io::sink());
    let diagnostics = sender.outgoing_queue();
    let accepted = sender.send(message());
    assert_eq!(sender.len(), 1);
    drop(driver);
    assert!(accepted.now_or_never().unwrap().is_err());

    sender.send_detached(message());
    assert!(sender.send(message()).now_or_never().unwrap().is_err());
    assert!(sender.is_empty());
    assert_eq!(diagnostics.output_snapshot(), OutputSnapshot::default());
}

#[test]
fn oversized_batch_flushes_every_receipt_and_releases_capacity_for_later_work() {
    let (sender, driver) = write_queue(futures::io::sink());
    let receipts: Vec<_> = (0..1025).map(|_| sender.send(message())).collect();
    futures::pin_mut!(driver);
    assert!(driver.as_mut().now_or_never().is_none());
    for receipt in receipts {
        receipt.now_or_never().unwrap().unwrap();
    }
    let later = sender.send(message());
    assert!(driver.as_mut().now_or_never().is_none());
    later.now_or_never().unwrap().unwrap();
    assert_eq!(sender.outgoing_queue().snapshot().message_count, 0);
    assert!(sender.shared.queue.lock().unwrap().messages.capacity() <= 1024);
    drop(sender);
    driver.now_or_never().unwrap().unwrap();
}

#[test]
fn one_enqueue_clock_sample_per_batch_preserves_oldest_age() {
    let ticks = Arc::new(AtomicU64::new(0));
    let clock = ticks.clone();
    let (sender, driver) = write_queue_with_clock(futures::io::sink(), move || {
        Duration::from_secs(clock.fetch_add(1, Ordering::Relaxed))
    });
    futures::pin_mut!(driver);
    for _ in 0..3 {
        sender.send_detached(message());
    }
    assert_eq!(ticks.load(Ordering::Relaxed), 1);
    let snapshot = sender.outgoing_queue().snapshot();
    assert_eq!(snapshot.message_count, 3);
    assert_eq!(snapshot.wait_time, Duration::from_secs(1));
    assert!(driver.as_mut().now_or_never().is_none());
    sender.send_detached(message());
    assert_eq!(ticks.load(Ordering::Relaxed), 3);
    let snapshot = sender.outgoing_queue().snapshot();
    assert_eq!(snapshot.message_count, 1);
    assert_eq!(snapshot.wait_time, Duration::from_secs(1));
}

#[test]
fn contiguous_frame_retains_its_owner_until_partial_write_and_flush_finish() {
    use std::{
        cell::RefCell,
        pin::Pin,
        rc::Rc,
        task::{Context, Waker},
    };
    #[derive(Default)]
    struct Output {
        bytes: Vec<u8>,
        writable: bool,
        flushed: bool,
    }
    struct Writer(Rc<RefCell<Output>>);
    impl AsyncWrite for Writer {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            let mut state = self.0.borrow_mut();
            if !state.writable {
                return Poll::Pending;
            }
            state.writable = false;
            let n = bytes.len().min(3);
            state.bytes.extend_from_slice(&bytes[..n]);
            Poll::Ready(Ok(n))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            if self.0.borrow().flushed {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        }
        fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            self.poll_flush(cx)
        }
    }
    struct Frame([u8; 16]);
    impl AsOutputSegments for Frame {
        fn as_output_segments(&self) -> capnp::OutputSegments<'_> {
            panic!("contiguous frame must not rebuild segment metadata")
        }
        fn framed_bytes(&self) -> Option<&[u8]> {
            Some(&self.0)
        }
        fn size_in_bytes(&self) -> usize {
            8
        }
    }
    let output = Rc::new(RefCell::new(Output::default()));
    let (sender, driver) = write_queue(Writer(output.clone()));
    let expected = [0, 0, 0, 0, 1, 0, 0, 0, 8, 7, 6, 5, 4, 3, 2, 1];
    let receipt = sender.send(Frame(expected));
    futures::pin_mut!(driver, receipt);
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..6 {
        output.borrow_mut().writable = true;
        assert!(driver.as_mut().poll(&mut cx).is_pending());
        assert!(receipt.as_mut().poll(&mut cx).is_pending());
        assert_eq!(
            sender
                .outgoing_queue()
                .output_snapshot()
                .active
                .message_count,
            1
        );
    }
    assert_eq!(output.borrow().bytes, expected);
    output.borrow_mut().flushed = true;
    assert!(driver.as_mut().poll(&mut cx).is_pending());
    let Poll::Ready(Ok(frame)) = receipt.as_mut().poll(&mut cx) else {
        panic!("flushed receipt")
    };
    assert_eq!(frame.0, expected);
    assert_eq!(
        sender.outgoing_queue().output_snapshot(),
        OutputSnapshot::default()
    );
}

#[test]
fn contiguous_frame_io_failures_release_owners_and_fail_all_receipts() {
    use std::sync::atomic::AtomicUsize;
    struct Frame {
        bytes: [u8; 16],
        drops: Arc<AtomicUsize>,
    }
    impl AsOutputSegments for Frame {
        fn as_output_segments(&self) -> capnp::OutputSegments<'_> {
            capnp::OutputSegments::SingleSegment([&self.bytes[8..]])
        }
        fn framed_bytes(&self) -> Option<&[u8]> {
            Some(&self.bytes)
        }
    }
    impl Drop for Frame {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[derive(Clone, Copy)]
    enum Fault {
        Write,
        Zero,
        Flush,
    }
    struct Writer(Fault);
    impl std::io::Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            match self.0 {
                Fault::Write => Err(std::io::ErrorKind::BrokenPipe.into()),
                Fault::Zero => Ok(0),
                Fault::Flush => Ok(bytes.len()),
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
    }
    for fault in [Fault::Write, Fault::Zero, Fault::Flush] {
        for count in [1, 2] {
            let drops = Arc::new(AtomicUsize::new(0));
            let (sender, driver) = write_queue(futures::io::AllowStdIo::new(Writer(fault)));
            let receipts: Vec<_> = (0..count)
                .map(|_| {
                    sender.send(Frame {
                        bytes: [0, 0, 0, 0, 1, 0, 0, 0, 8, 7, 6, 5, 4, 3, 2, 1],
                        drops: drops.clone(),
                    })
                })
                .collect();
            assert!(driver.now_or_never().unwrap().is_err());
            for receipt in receipts {
                assert!(receipt.now_or_never().unwrap().is_err());
            }
            assert_eq!(drops.load(Ordering::SeqCst), count);
            assert_eq!(
                sender.outgoing_queue().output_snapshot(),
                OutputSnapshot::default()
            );
            assert!(sender.is_empty());
        }
    }
}

#[test]
fn physical_reservation_survives_reentrant_driver_poll_and_shutdown() {
    let (sender, driver) = write_queue(futures::io::sink());
    let mut driver = Box::pin(driver);
    let mut observed = sender.observe().unwrap();
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    observed.reserve().unwrap();
    sender.send_detached(message());
    let capacity = sender.shared.queue.lock().unwrap().messages.capacity();
    assert!(driver.as_mut().poll(&mut cx).is_pending());
    assert_eq!(sender.len(), 1, "reentry cannot take a reserved deque");
    assert_eq!(
        sender.shared.queue.lock().unwrap().messages.capacity(),
        capacity
    );
    observed.publish_reserved(message()).unwrap();
    assert_eq!(sender.len(), 2);
    assert!(driver.as_mut().poll(&mut cx).is_pending());
    assert!(matches!(observed.poll_flush(&mut cx), Poll::Ready(Ok(()))));
    observed.reserve().unwrap();
    let closed = sender.terminate(Ok(()));
    assert!(
        driver.as_mut().poll(&mut cx).is_pending(),
        "shutdown waits for a live reservation"
    );
    observed.cancel_reservation();
    assert!(matches!(driver.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    assert!(closed.now_or_never().unwrap().is_ok());
    assert!(observed.reserve().is_err());
}

#[test]
fn reservation_releases_on_clock_unwind_and_physical_failure() {
    use std::sync::atomic::AtomicBool;
    let fail = Arc::new(AtomicBool::new(true));
    let clock = fail.clone();
    let (sender, driver) = write_queue_with_clock(futures::io::sink(), move || {
        assert!(!clock.swap(false, Ordering::SeqCst), "clock panic");
        Duration::ZERO
    });
    let mut observed = sender.observe().unwrap();
    observed.reserve().unwrap();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || observed.publish_reserved(message())
    ))
    .is_err());
    assert_eq!(sender.shared.queue.lock().unwrap().reserved, 0);
    assert!(sender.is_empty());
    observed.reserve().unwrap();
    drop(driver);
    assert!(observed.publish_reserved(message()).is_err());
    assert_eq!(sender.shared.queue.lock().unwrap().reserved, 0);
}
