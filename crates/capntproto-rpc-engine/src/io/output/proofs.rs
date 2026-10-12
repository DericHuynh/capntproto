use super::*;
use core::task::Waker;

struct Sink {
    mode: u8,
    writes: u8,
    flushes: u8,
    attachments: u8,
}
impl Flush for Sink {
    type Error = ();
    fn poll_flush(&mut self, _: &mut Context<'_>) -> Poll<Result<(), IoError<()>>> {
        self.flushes += 1;
        match self.mode {
            0 => Poll::Pending,
            1 => Poll::Ready(Err(IoError::Interrupted)),
            2 => Poll::Ready(Err(IoError::Failed(()))),
            _ => Poll::Ready(Ok(())),
        }
    }
}
impl WriteResources<u8> for Sink {
    fn poll_write_resources(
        &mut self,
        _: &mut Context<'_>,
        bytes: &[u8],
        resource: Option<&u8>,
    ) -> Poll<Result<usize, IoError<()>>> {
        assert_eq!(bytes.len(), 1);
        self.writes += 1;
        self.attachments += resource.copied().unwrap_or(0);
        Poll::Ready(Ok(1))
    }
}

#[kani::proof]
#[kani::unwind(4)]
fn buffered_completion_requires_flush_and_never_repeats_accepted_resources() {
    let mode: u8 = kani::any();
    kani::assume(mode <= 3);
    let mut bytes = [0];
    let mut buffers = [messages::Buffer::new(&mut bytes)];
    messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut entries = [None];
        let mut queue = Queue::with_resources(&mut entries).unwrap();
        let writer = messages.reserve(1).unwrap();
        let lease = messages.seal(writer, 1).unwrap();
        let offer = messages.offer(&lease).unwrap();
        queue.reserve_with(1u8).unwrap().accept(messages, offer);
        messages.release(lease);
        let mut sink = Sink {
            mode,
            writes: 0,
            flushes: 0,
            attachments: 0,
        };
        let mut output = Output::new();
        let mut cx = Context::from_waker(Waker::noop());
        let budget = NonZeroUsize::new(1).unwrap();
        let result = output.poll(&mut sink, &mut queue, messages, &mut cx, budget);
        match mode {
            0 | 1 => {
                assert_eq!(result, Poll::Pending);
                assert!(output.flushing);
            }
            2 => {
                assert_eq!(result, Poll::Ready(Err(SendError::Io(()))));
                assert!(queue.is_failed());
            }
            _ => assert_eq!(result, Poll::Ready(Ok(()))),
        }
        sink.mode = 3;
        let result = output.poll(&mut sink, &mut queue, messages, &mut cx, budget);
        if mode == 2 {
            assert_eq!(
                result,
                Poll::Ready(Err(SendError::State(messages::Error::QueueFailed)))
            );
        } else {
            assert_eq!(result, Poll::Ready(Ok(())));
            assert!(!output.flushing);
        }
        assert_eq!(sink.writes, 1);
        assert_eq!(sink.attachments, 1);
        assert_eq!(sink.flushes, if mode < 2 { 2 } else { 1 });
        let writer = messages.reserve(1).unwrap();
        messages.discard(writer);
    })
    .unwrap();
}
