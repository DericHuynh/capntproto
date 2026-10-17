use super::*;
use core::{cell::Cell, task::Waker};

#[derive(Default)]
struct Sink {
    buffered: Option<u8>,
    bytes: [u8; 6],
    len: usize,
    resources: u8,
    pending_flush: bool,
    writes: usize,
    flushes: usize,
    closes: usize,
    batches: usize,
    interrupt: bool,
    fail_after: Option<usize>,
}
impl Flush for Sink {
    type Error = u8;
    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), IoError<u8>>> {
        self.flushes += 1;
        if self.interrupt {
            return Poll::Ready(Err(IoError::Interrupted));
        }
        if self.fail_after == Some(self.len) {
            return Poll::Ready(Err(IoError::Failed(73)));
        }
        if self.pending_flush {
            self.pending_flush = false;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        if let Some(byte) = self.buffered.take() {
            self.bytes[self.len] = byte;
            self.len += 1;
        }
        Poll::Ready(Ok(()))
    }
}
impl Close for Sink {
    fn poll_close(&mut self, _: &mut Context<'_>) -> Poll<Result<(), IoError<u8>>> {
        assert!(self.buffered.is_none());
        self.closes += 1;
        if self.interrupt {
            Poll::Ready(Err(IoError::Interrupted))
        } else {
            Poll::Ready(Ok(()))
        }
    }
}
struct Resource<'a>(u8, &'a Cell<u8>);
impl Drop for Resource<'_> {
    fn drop(&mut self) {
        self.1.set(self.1.get() | self.0);
    }
}
impl WriteResources<Resource<'_>> for Sink {
    fn start_batch(&mut self, frames: usize) {
        assert!(
            self.buffered.is_none(),
            "a pending physical flush owns the writer"
        );
        assert!((1..=2).contains(&frames));
        self.batches += 1;
    }
    fn poll_write_resources(
        &mut self,
        _: &mut Context<'_>,
        bytes: &[u8],
        resource: Option<&Resource<'_>>,
    ) -> Poll<Result<usize, IoError<u8>>> {
        self.writes += 1;
        if self.buffered.is_some() {
            // Only flushing can free the single byte buffer; it doesn't issue
            // a separate write wakeup. The output pump must schedule progress.
            return Poll::Pending;
        }
        if let Some(resource) = resource {
            assert_eq!(self.resources & resource.0, 0, "resources sent twice");
            self.resources |= resource.0;
        }
        self.buffered = Some(bytes[0]);
        self.pending_flush = true;
        Poll::Ready(Ok(1))
    }
}

#[test]
fn partial_frames_flush_without_reordering_or_repeating_resources_and_allocate_nothing() {
    let measured = allocation_counter::measure(|| {
        let mut a = [0; 3];
        let mut b = [0; 3];
        let mut buffers = [messages::Buffer::new(&mut a), messages::Buffer::new(&mut b)];
        messages::with_messages(
            &mut buffers,
            &mut [messages::SendSlot::EMPTY; 2],
            |messages| {
                let released = Cell::new(0);
                let mut entries = [None, None];
                let mut queue = Queue::with_resources(&mut entries).unwrap();
                for (bit, bytes) in [(1, b"abc"), (2, b"def")] {
                    let writer = messages.reserve(3).unwrap();
                    messages.write(&writer).copy_from_slice(bytes);
                    let lease = messages.seal(writer, 3).unwrap();
                    let offer = messages.offer(&lease).unwrap();
                    queue
                        .reserve_with(Resource(bit, &released))
                        .unwrap()
                        .accept(messages, offer);
                    messages.release(lease);
                }
                let mut sink = Sink::default();
                let mut output = Output::new();
                let mut cx = Context::from_waker(Waker::noop());
                let budget = NonZeroUsize::new(3).unwrap();
                for _ in 0..20 {
                    let flushing = output.flushing;
                    let writes = sink.writes;
                    let batches = sink.batches;
                    let result = output.poll(&mut sink, &mut queue, messages, &mut cx, budget);
                    if flushing {
                        assert_eq!(sink.batches, batches);
                        assert_eq!(
                            sink.writes, writes,
                            "pending flush must precede further writes"
                        );
                    }
                    if result == Poll::Ready(Ok(())) {
                        break;
                    }
                    assert_eq!(result, Poll::Pending);
                }
                assert_eq!(&sink.bytes, b"abcdef");
                assert_eq!(sink.resources, 3);
                assert_eq!(released.get(), 3);
                assert!(!output.flushing);
                let counts = (sink.writes, sink.flushes, sink.batches);
                for _ in 0..3 {
                    assert_eq!(
                        output.poll(&mut sink, &mut queue, messages, &mut cx, budget),
                        Poll::Ready(Ok(()))
                    );
                }
                assert_eq!(
                    (sink.writes, sink.flushes, sink.batches),
                    counts,
                    "idle output must not touch IO"
                );
                assert_eq!(poll_close(&mut sink, &mut cx, budget), Poll::Ready(Ok(())));
                assert_eq!(sink.closes, 1);
            },
        )
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn flush_failure_at_each_offset_is_terminal_and_preserves_remaining_resources_for_cleanup() {
    for offset in 0..3 {
        let mut bytes = [0; 3];
        let mut buffers = [messages::Buffer::new(&mut bytes)];
        messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
            let released = Cell::new(0);
            let mut entries = [None];
            let mut queue = Queue::with_resources(&mut entries).unwrap();
            let writer = messages.reserve(3).unwrap();
            messages.write(&writer).copy_from_slice(b"abc");
            let lease = messages.seal(writer, 3).unwrap();
            let offer = messages.offer(&lease).unwrap();
            queue
                .reserve_with(Resource(1, &released))
                .unwrap()
                .accept(messages, offer);
            messages.release(lease);
            let mut sink = Sink {
                fail_after: Some(offset),
                ..Default::default()
            };
            let mut output = Output::new();
            let mut cx = Context::from_waker(Waker::noop());
            let budget = NonZeroUsize::new(2).unwrap();
            let mut failed = false;
            for _ in 0..10 {
                if let Poll::Ready(result) =
                    output.poll(&mut sink, &mut queue, messages, &mut cx, budget)
                {
                    assert_eq!(result, Err(SendError::Io(73)));
                    failed = true;
                    break;
                }
            }
            assert!(failed && queue.is_failed());
            assert_eq!(sink.len, offset);
            let counts = (sink.writes, sink.flushes);
            assert_eq!(
                output.poll(&mut sink, &mut queue, messages, &mut cx, budget),
                Poll::Ready(Err(SendError::State(messages::Error::QueueFailed)))
            );
            assert_eq!((sink.writes, sink.flushes), counts);
            while queue.discard_one(messages).unwrap() {}
            assert_eq!(released.get(), 1);
            let writer = messages.reserve(3).unwrap();
            messages.discard(writer);
        })
        .unwrap();
    }
}

#[test]
fn interrupted_flush_and_close_obey_the_poll_budget() {
    let mut bytes = [0];
    let mut buffers = [messages::Buffer::new(&mut bytes)];
    messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let released = Cell::new(0);
        let mut entries = [None];
        let mut queue = Queue::with_resources(&mut entries).unwrap();
        let writer = messages.reserve(1).unwrap();
        let lease = messages.seal(writer, 1).unwrap();
        let offer = messages.offer(&lease).unwrap();
        queue
            .reserve_with(Resource(1, &released))
            .unwrap()
            .accept(messages, offer);
        messages.release(lease);
        let mut sink = Sink {
            interrupt: true,
            ..Default::default()
        };
        let mut output = Output::new();
        let mut cx = Context::from_waker(Waker::noop());
        let budget = NonZeroUsize::new(3).unwrap();
        assert_eq!(
            output.poll(&mut sink, &mut queue, messages, &mut cx, budget),
            Poll::Pending
        );
        assert_eq!((sink.writes, sink.flushes), (1, 3));
        assert_eq!(
            output.poll(&mut sink, &mut queue, messages, &mut cx, budget),
            Poll::Pending
        );
        assert_eq!((sink.writes, sink.flushes), (1, 6));
        sink.interrupt = false;
        assert_eq!(
            output.poll(&mut sink, &mut queue, messages, &mut cx, budget),
            Poll::Pending
        );
        assert_eq!(
            output.poll(&mut sink, &mut queue, messages, &mut cx, budget),
            Poll::Ready(Ok(()))
        );
        sink.interrupt = true;
        assert_eq!(poll_close(&mut sink, &mut cx, budget), Poll::Pending);
        assert_eq!(sink.closes, 3);
        sink.interrupt = false;
        assert_eq!(poll_close(&mut sink, &mut cx, budget), Poll::Ready(Ok(())));
    })
    .unwrap();
}

#[test]
fn empty_frames_release_receipts_without_writing_or_flushing() {
    let mut bytes = [0; 1];
    let mut buffers = [messages::Buffer::new(&mut bytes)];
    messages::with_messages(&mut buffers, &mut [], |messages| {
        let released = Cell::new(0);
        let mut entries = [None];
        let mut queue = Queue::with_resources(&mut entries).unwrap();
        let lease = messages.copy_from(b"").unwrap();
        queue
            .reserve_with(Resource(1, &released))
            .unwrap()
            .accept_owned(lease);
        let mut sink = Sink::default();
        let mut output = Output::new();
        let mut cx = Context::from_waker(Waker::noop());
        assert_eq!(
            output.poll(
                &mut sink,
                &mut queue,
                messages,
                &mut cx,
                NonZeroUsize::new(1).unwrap()
            ),
            Poll::Ready(Ok(()))
        );
        assert!(queue.is_empty() && output.is_clean());
        assert_eq!((sink.writes, sink.flushes), (0, 0));
        assert_eq!(released.get(), 1);
        assert!(messages.can_reserve(1));
    })
    .unwrap();
}

#[cfg(feature = "alloc")]
#[test]
fn zero_progress_from_borrowed_and_shared_adapters_preserves_owners_until_abort() {
    use crate::connection::Growth;
    use alloc::rc::Rc;
    struct Parts;
    impl messages::Parts for Parts {
        fn count(&self) -> usize {
            2
        }
        fn part(&self, index: usize) -> &[u8] {
            [b"a".as_slice(), b"bc".as_slice()][index]
        }
        fn retained_capacity(&self) -> usize {
            3
        }
    }
    struct ZeroWriter {
        mode: u8,
        zero: bool,
        writes: usize,
        flushes: usize,
    }
    impl Flush for ZeroWriter {
        type Error = u8;
        fn poll_flush(&mut self, _: &mut Context<'_>) -> Poll<Result<(), IoError<u8>>> {
            self.flushes += 1;
            Poll::Ready(Ok(()))
        }
    }
    impl WriteResources<Resource<'_>> for ZeroWriter {
        fn accepts_shared(&self) -> bool {
            self.mode == 1
        }
        fn accepts_parts(&self) -> bool {
            self.mode == 2
        }
        // The default owning methods must release their temporary aliases and
        // preserve the first borrowed slice and attachment on rejection.
        fn poll_write_resources(
            &mut self,
            _: &mut Context<'_>,
            bytes: &[u8],
            resource: Option<&Resource<'_>>,
        ) -> Poll<Result<usize, IoError<u8>>> {
            let first = self.writes == 0;
            self.writes += 1;
            assert_eq!(
                bytes,
                match (self.mode, first) {
                    (2, true) => b"a".as_slice(),
                    (2, false) => b"bc".as_slice(),
                    _ => b"abc".as_slice(),
                }
            );
            assert_eq!(resource.map(|resource| resource.0), first.then_some(1));
            Poll::Ready(Ok(if self.zero { 0 } else { bytes.len() }))
        }
    }
    for (mode, zero) in [
        (0, true),
        (0, false),
        (1, true),
        (1, false),
        (2, true),
        (2, false),
    ] {
        let source = Rc::new(Parts);
        let mut pool = messages::GrowingBuffers::new(1, 32, 32);
        messages::with_growing_messages(
            &mut pool,
            Growth {
                initial: 0,
                maximum: 0,
            },
            |messages| {
                let released = Cell::new(0);
                let mut entries = [None];
                let mut queue = Queue::with_resources(&mut entries).unwrap();
                if mode == 2 {
                    let parts = messages.retain_parts(source.clone()).unwrap().unwrap();
                    queue
                        .reserve_with(Resource(1, &released))
                        .unwrap()
                        .accept_parts(parts);
                } else {
                    let lease = messages.copy_from(b"abc").unwrap();
                    queue
                        .reserve_with(Resource(1, &released))
                        .unwrap()
                        .accept_owned(lease);
                    // A byte owner cannot authorize scatter/gather progress.
                    assert_eq!(
                        queue.window(messages).unwrap().unwrap().advance_parts(1),
                        Err(messages::Error::InvalidWriteCount)
                    );
                }
                let mut writer = ZeroWriter {
                    mode,
                    zero,
                    writes: 0,
                    flushes: 0,
                };
                let mut output = Output::new();
                let mut cx = Context::from_waker(Waker::noop());
                let budget = NonZeroUsize::new(1).unwrap();
                if zero {
                    assert_eq!(
                        output.poll(&mut writer, &mut queue, messages, &mut cx, budget),
                        Poll::Ready(Err(SendError::State(messages::Error::WriteZero)))
                    );
                    assert!(queue.is_failed() && output.is_clean());
                    assert_eq!(released.get(), 0);
                    assert!(!messages.can_reserve(1));
                    assert_eq!(
                        output.poll(&mut writer, &mut queue, messages, &mut cx, budget),
                        Poll::Ready(Err(SendError::State(messages::Error::QueueFailed)))
                    );
                    assert_eq!((writer.writes, writer.flushes), (1, 0));
                    assert!(queue.discard_one(messages).unwrap());
                    assert!(!queue.discard_one(messages).unwrap());
                } else {
                    let result = output.poll(&mut writer, &mut queue, messages, &mut cx, budget);
                    if mode == 2 {
                        // The default owning adapter borrows one part at a time.
                        // A positive prefix flushes but retains the frame and
                        // its resource until the remaining bytes are accepted.
                        assert_eq!(result, Poll::Pending);
                        assert_eq!(released.get(), 0);
                        assert!(!messages.can_reserve(1));
                        assert_eq!((writer.writes, writer.flushes), (1, 1));
                        assert_eq!(
                            output.poll(&mut writer, &mut queue, messages, &mut cx, budget),
                            Poll::Ready(Ok(()))
                        );
                    } else {
                        assert_eq!(result, Poll::Ready(Ok(())));
                    }
                    assert!(queue.is_empty() && output.is_clean());
                    let expected = if mode == 2 { 2 } else { 1 };
                    assert_eq!((writer.writes, writer.flushes), (expected, expected));
                    assert_eq!(
                        output.poll(&mut writer, &mut queue, messages, &mut cx, budget),
                        Poll::Ready(Ok(()))
                    );
                    assert_eq!((writer.writes, writer.flushes), (expected, expected));
                }
                assert_eq!(released.get(), 1);
                assert!(messages.can_reserve(1));
            },
        )
        .unwrap();
        assert_eq!(Rc::strong_count(&source), 1);
    }
}
