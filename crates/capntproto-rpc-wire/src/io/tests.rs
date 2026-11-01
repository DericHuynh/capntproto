extern crate std;

use super::*;
use capnp::{serialize::FrameReceiver, word, Word};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Wake, Waker},
};

#[derive(Clone, Copy)]
enum Action {
    Read(usize),
    Pending,
    Interrupted,
    Failed,
    Oversized,
}
struct Source<'a> {
    bytes: &'a [u8],
    actions: VecDeque<Action>,
    reads: usize,
}
impl<'a> Source<'a> {
    fn new(bytes: &'a [u8], actions: &[Action]) -> Self {
        Self {
            bytes,
            actions: actions.iter().copied().collect(),
            reads: 0,
        }
    }
}
impl Read for Source<'_> {
    type Error = u8;
    fn poll_read(
        &mut self,
        _: &mut Context<'_>,
        bytes: &mut [u8],
    ) -> Poll<Result<usize, IoError<u8>>> {
        self.reads += 1;
        match self.actions.pop_front().expect("unexpected read") {
            Action::Read(limit) => {
                let count = bytes.len().min(self.bytes.len()).min(limit);
                bytes[..count].copy_from_slice(&self.bytes[..count]);
                self.bytes = &self.bytes[count..];
                Poll::Ready(Ok(count))
            }
            Action::Pending => Poll::Pending,
            Action::Interrupted => Poll::Ready(Err(IoError::Interrupted)),
            Action::Failed => Poll::Ready(Err(IoError::Failed(7))),
            Action::Oversized => Poll::Ready(Ok(bytes.len() + 1)),
        }
    }
}
#[derive(Default)]
struct Signal(AtomicUsize);
impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}
const ZERO: Word = word(0, 0, 0, 0, 0, 0, 0, 0);
fn budget(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).unwrap()
}

#[test]
fn completion_at_the_read_budget_does_not_wake_or_prefetch_the_next_frame() {
    let wire = [word(0, 0, 0, 0, 1, 0, 0, 0), ZERO, ZERO];
    let mut source = Source::new(Word::words_to_bytes(&wire), &[Action::Read(8); 3]);
    let mut storage = [ZERO; 3];
    let mut receiver =
        FrameReceiver::new(Word::words_to_bytes_mut(&mut storage), Default::default()).unwrap();
    let signal = Arc::new(Signal::default());
    let waker = Waker::from(signal.clone());
    let mut cx = Context::from_waker(&waker);
    let measured = allocation_counter::measure(|| {
        assert_eq!(
            poll_receive(&mut source, &mut receiver, &mut cx, budget(2)),
            Poll::Ready(Ok(true))
        );
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(source.reads, 2);
    assert_eq!(source.bytes.len(), 8);
    assert_eq!(receiver.received_bytes(), 16);
    assert_eq!(signal.0.load(Ordering::Relaxed), 0);
    assert_eq!(
        poll_receive(&mut source, &mut receiver, &mut cx, budget(1)),
        Poll::Ready(Ok(true))
    );
    assert_eq!(source.reads, 2);
    receiver.next_message().unwrap();
    assert_eq!(
        poll_receive(&mut source, &mut receiver, &mut cx, budget(1)),
        Poll::Ready(Ok(true))
    );
    assert!(source.bytes.is_empty());
}

#[test]
fn pending_preserves_partial_header_progress_for_the_next_poll() {
    let wire = [ZERO];
    let mut source = Source::new(
        Word::words_to_bytes(&wire),
        &[Action::Read(3), Action::Pending, Action::Read(5)],
    );
    let mut storage = [ZERO; 1];
    let mut receiver =
        FrameReceiver::new(Word::words_to_bytes_mut(&mut storage), Default::default()).unwrap();
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    assert_eq!(
        poll_receive(&mut source, &mut receiver, &mut cx, budget(8)),
        Poll::Pending
    );
    assert_eq!(receiver.received_bytes(), 3);
    assert_eq!(
        poll_receive(&mut source, &mut receiver, &mut cx, budget(1)),
        Poll::Ready(Ok(true))
    );
    assert_eq!(receiver.received_bytes(), 8);
}

#[test]
fn interrupted_reads_consume_the_budget_and_wake_once_for_more_work() {
    let wire = [ZERO];
    let mut source = Source::new(
        Word::words_to_bytes(&wire),
        &[Action::Interrupted, Action::Interrupted, Action::Read(8)],
    );
    let mut storage = [ZERO; 1];
    let mut receiver =
        FrameReceiver::new(Word::words_to_bytes_mut(&mut storage), Default::default()).unwrap();
    let signal = Arc::new(Signal::default());
    let waker = Waker::from(signal.clone());
    let mut cx = Context::from_waker(&waker);
    assert_eq!(
        poll_receive(&mut source, &mut receiver, &mut cx, budget(2)),
        Poll::Pending
    );
    assert_eq!(source.reads, 2);
    assert_eq!(signal.0.load(Ordering::Relaxed), 1);
    assert_eq!(
        poll_receive(&mut source, &mut receiver, &mut cx, budget(1)),
        Poll::Ready(Ok(true))
    );
    assert_eq!(signal.0.load(Ordering::Relaxed), 1);
}

#[test]
fn io_failure_aborts_the_receiver_and_cannot_read_or_reuse_its_prefix() {
    let wire = [ZERO];
    let mut source = Source::new(
        Word::words_to_bytes(&wire),
        &[Action::Read(3), Action::Failed],
    );
    let mut storage = [ZERO; 1];
    let mut receiver =
        FrameReceiver::new(Word::words_to_bytes_mut(&mut storage), Default::default()).unwrap();
    let mut cx = Context::from_waker(Waker::noop());
    assert_eq!(
        poll_receive(&mut source, &mut receiver, &mut cx, budget(3)),
        Poll::Ready(Err(ReceiveError::Io(7)))
    );
    assert_eq!(
        poll_receive(&mut source, &mut receiver, &mut cx, budget(3)),
        Poll::Ready(Err(ReceiveError::Wire(capnp::ErrorKind::Disconnected)))
    );
    assert_eq!(source.reads, 2);
}

#[test]
fn malformed_truncated_and_oversized_input_fail_closed() {
    for (wire, actions) in [
        (
            [word(255, 255, 255, 255, 0, 0, 0, 0)],
            &[Action::Read(8)][..],
        ),
        ([ZERO], &[Action::Read(3), Action::Read(0)][..]),
        ([ZERO], &[Action::Oversized][..]),
    ] {
        let mut source = Source::new(Word::words_to_bytes(&wire), actions);
        let mut storage = [ZERO; 1];
        let mut receiver =
            FrameReceiver::new(Word::words_to_bytes_mut(&mut storage), Default::default()).unwrap();
        let mut cx = Context::from_waker(Waker::noop());
        assert!(matches!(
            poll_receive(&mut source, &mut receiver, &mut cx, budget(3)),
            Poll::Ready(Err(ReceiveError::Wire(_)))
        ));
        assert!(receiver.message().is_err());
    }
}

#[test]
fn clean_eof_is_sticky_and_does_not_schedule_an_extra_poll() {
    let mut source = Source::new(&[], &[Action::Read(0)]);
    let mut storage = [ZERO; 1];
    let mut receiver =
        FrameReceiver::new(Word::words_to_bytes_mut(&mut storage), Default::default()).unwrap();
    let signal = Arc::new(Signal::default());
    let waker = Waker::from(signal.clone());
    let mut cx = Context::from_waker(&waker);
    for _ in 0..2 {
        assert_eq!(
            poll_receive(&mut source, &mut receiver, &mut cx, budget(1)),
            Poll::Ready(Ok(false))
        );
    }
    assert_eq!(source.reads, 1);
    assert_eq!(signal.0.load(Ordering::Relaxed), 0);
}
