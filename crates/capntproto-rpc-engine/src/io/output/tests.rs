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
