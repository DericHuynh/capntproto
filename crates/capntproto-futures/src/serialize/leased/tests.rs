use super::*;
use capntproto_rpc_engine::messages::{with_messages, Buffer, SendSlot};
use core::future::Future;
use core::pin::Pin;
use std::io;

#[derive(Clone, Copy)]
enum Fault {
    Pending,
    Interrupted,
    Reset,
    Zero,
    Overreport,
}
struct Sink {
    bytes: [u8; 32],
    used: usize,
    polls: usize,
    fault: Option<(usize, Fault)>,
}
impl Sink {
    fn new(fault: Option<(usize, Fault)>) -> Self {
        Self {
            bytes: [0; 32],
            used: 0,
            polls: 0,
            fault,
        }
    }
}
impl AsyncWrite for Sink {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        assert!(!bytes.is_empty());
        self.polls += 1;
        if self.fault.is_some_and(|(at, _)| at == self.used) {
            match self.fault.take().unwrap().1 {
                Fault::Pending => {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                Fault::Interrupted => return Poll::Ready(Err(io::ErrorKind::Interrupted.into())),
                Fault::Reset => return Poll::Ready(Err(io::ErrorKind::ConnectionReset.into())),
                Fault::Zero => return Poll::Ready(Ok(0)),
                Fault::Overreport => return Poll::Ready(Ok(bytes.len() + 1)),
            }
        }
        let index = self.used;
        self.bytes[index] = bytes[0];
        self.used += 1;
        Poll::Ready(Ok(1))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        panic!("byte completion must not flush")
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        panic!("byte completion must not close")
    }
}

#[test]
fn cancellation_at_every_byte_resumes_the_exact_suffix() {
    let input = b"leased RPC message";
    for at in 0..input.len() {
        let mut bytes = [0; 32];
        let mut buffers = [Buffer::new(&mut bytes)];
        let mut sends = [SendSlot::EMPTY];
        with_messages(&mut buffers, &mut sends, |messages| {
            let writer = messages.reserve(input.len()).unwrap();
            messages.write(&writer)[..input.len()].copy_from_slice(input);
            let lease = messages.seal(writer, input.len()).unwrap();
            let offer = messages.offer(&lease).unwrap();
            let key = messages.accept(offer);
            messages.release(lease);
            let mut sink = Sink::new(Some((at, Fault::Pending)));
            let mut cx = Context::from_waker(futures::task::noop_waker_ref());
            {
                let mut future = std::pin::pin!(write_queued_message(
                    &mut sink,
                    messages,
                    key,
                    NonZeroUsize::new(64).unwrap()
                ));
                assert!(future.as_mut().poll(&mut cx).is_pending());
            }
            assert_eq!(sink.used, at);
            assert_eq!(messages.send_window(key).unwrap().bytes(), &input[at..]);
            assert_eq!(messages.reserve(1).unwrap_err(), Error::NoBuffer);
            {
                let mut future = std::pin::pin!(write_queued_message(
                    &mut sink,
                    messages,
                    key,
                    NonZeroUsize::new(64).unwrap()
                ));
                assert_eq!(future.as_mut().poll(&mut cx), Poll::Ready(Ok(())));
            }
            assert_eq!(&sink.bytes[..sink.used], input);
            let writer = messages.reserve(32).unwrap();
            messages.discard(writer);
        })
        .unwrap();
    }
}

#[test]
fn write_errors_at_every_byte_release_only_the_send_and_interruption_retries() {
    let input = b"faulty writes";
    for at in 0..input.len() {
        for fault in [
            Fault::Interrupted,
            Fault::Reset,
            Fault::Zero,
            Fault::Overreport,
        ] {
            let mut bytes = [0; 32];
            let mut buffers = [Buffer::new(&mut bytes)];
            let mut sends = [SendSlot::EMPTY];
            with_messages(&mut buffers, &mut sends, |messages| {
                let writer = messages.reserve(input.len()).unwrap();
                messages.write(&writer)[..input.len()].copy_from_slice(input);
                let lease = messages.seal(writer, input.len()).unwrap();
                let offer = messages.offer(&lease).unwrap();
                let key = messages.accept(offer);
                let mut sink = Sink::new(Some((at, fault)));
                let mut cx = Context::from_waker(futures::task::noop_waker_ref());
                let result = poll_queued_message(
                    &mut sink,
                    messages,
                    key,
                    &mut cx,
                    NonZeroUsize::new(64).unwrap(),
                );
                let expected = match fault {
                    Fault::Interrupted => Ok(()),
                    Fault::Reset => Err(SendError::Io(io::ErrorKind::ConnectionReset)),
                    Fault::Zero => Err(SendError::State(Error::WriteZero)),
                    Fault::Overreport => Err(SendError::State(Error::InvalidWriteCount)),
                    Fault::Pending => unreachable!(),
                };
                assert_eq!(result, Poll::Ready(expected));
                let written = if matches!(fault, Fault::Interrupted) {
                    input.len()
                } else {
                    at
                };
                assert_eq!(&sink.bytes[..sink.used], &input[..written]);
                assert_eq!(&*messages.read(&lease), input);
                let polls = sink.polls;
                assert_eq!(
                    poll_queued_message(
                        &mut sink,
                        messages,
                        key,
                        &mut cx,
                        NonZeroUsize::new(1).unwrap()
                    ),
                    Poll::Ready(Err(SendError::State(Error::StaleSend)))
                );
                assert_eq!(sink.polls, polls);
                messages.release(lease);
                let writer = messages.reserve(32).unwrap();
                messages.discard(writer);
            })
            .unwrap();
        }
    }
}

#[test]
fn budgets_self_wake_and_execution_never_allocates() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    struct Wake(AtomicUsize);
    impl futures::task::ArcWake for Wake {
        fn wake_by_ref(this: &Arc<Self>) {
            this.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    let wake = Arc::new(Wake(AtomicUsize::new(0)));
    let waker = futures::task::waker(wake.clone());
    let mut cx = Context::from_waker(&waker);
    let counts = allocation_counter::measure(|| {
        let mut bytes = [0; 4];
        let mut buffers = [Buffer::new(&mut bytes)];
        let mut sends = [SendSlot::EMPTY];
        with_messages(&mut buffers, &mut sends, |messages| {
            let writer = messages.reserve(4).unwrap();
            messages.write(&writer).copy_from_slice(b"rust");
            let lease = messages.seal(writer, 4).unwrap();
            let offer = messages.offer(&lease).unwrap();
            let key = messages.accept(offer);
            messages.release(lease);
            let mut sink = Sink::new(Some((0, Fault::Interrupted)));
            for expected_polls in 1..=5 {
                let result = poll_queued_message(
                    &mut sink,
                    messages,
                    key,
                    &mut cx,
                    NonZeroUsize::new(1).unwrap(),
                );
                assert_eq!(sink.polls, expected_polls);
                assert_eq!(
                    result,
                    if expected_polls == 5 {
                        Poll::Ready(Ok(()))
                    } else {
                        Poll::Pending
                    }
                );
            }
            assert_eq!(&sink.bytes[..sink.used], b"rust");
            assert_eq!(wake.0.load(Ordering::Relaxed), 4);
            let writer = messages.reserve(0).unwrap();
            let lease = messages.seal(writer, 0).unwrap();
            let offer = messages.offer(&lease).unwrap();
            let key = messages.accept(offer);
            messages.release(lease);
            assert_eq!(
                poll_queued_message(
                    &mut sink,
                    messages,
                    key,
                    &mut cx,
                    NonZeroUsize::new(1).unwrap()
                ),
                Poll::Ready(Ok(()))
            );
            assert_eq!(sink.polls, 5); // Empty messages never poll IO.
        })
        .unwrap();
    });
    assert_eq!(counts.count_total, 0);
}
