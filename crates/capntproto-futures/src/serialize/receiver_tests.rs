use super::{try_read_message_into, try_read_message_no_alloc};
use capnp::{message, serialize::FrameReceiver, ErrorKind, Word};
use futures::{io::AsyncRead, Future, FutureExt};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

const ZERO: Word = capnp::word(0, 0, 0, 0, 0, 0, 0, 0);

#[derive(Clone, Copy)]
enum Fault {
    Pending,
    Eof,
    Error(io::ErrorKind),
    Overreport,
}

/// One-byte reads, with one injected fault at an exact byte boundary.
struct Source<'a> {
    bytes: &'a [u8],
    position: usize,
    fault: Option<(usize, Fault)>,
    polls: usize,
}

impl AsyncRead for Source<'_> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        self.polls += 1;
        assert!(!out.is_empty(), "framing must not issue empty reads");
        if self.fault.is_some_and(|(at, _)| at == self.position) {
            let (_, fault) = self.fault.take().unwrap();
            return match fault {
                Fault::Pending => {
                    // Uncommitted writes must never be treated as received bytes.
                    out.fill(0xff);
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Fault::Eof => Poll::Ready(Ok(0)),
                Fault::Error(kind) => Poll::Ready(Err(kind.into())),
                Fault::Overreport => Poll::Ready(Ok(out.len() + 1)),
            };
        }
        if self.position == self.bytes.len() {
            return Poll::Ready(Ok(0));
        }
        out[0] = self.bytes[self.position];
        self.position += 1;
        Poll::Ready(Ok(1))
    }
}

fn frame() -> Vec<u8> {
    let mut builder = message::Builder::new(message::HeapAllocator::new().first_segment_words(1));
    builder
        .set_root::<capnp::text::Owned>("borrowed across segments")
        .unwrap();
    assert_eq!(builder.get_segments_for_output().len(), 2);
    capnp::serialize::write_message_to_words(&builder)
}

#[test]
fn cancelling_at_every_byte_resumes_without_losing_or_prefetching_frames() {
    let mut wire = frame();
    let size = wire.len();
    wire.extend_from_within(..);
    for at in 0..size {
        let mut words = [ZERO; 64];
        let mut receiver =
            FrameReceiver::new(Word::words_to_bytes_mut(&mut words), Default::default()).unwrap();
        let mut source = Source {
            bytes: &wire,
            position: 0,
            fault: Some((at, Fault::Pending)),
            polls: 0,
        };
        {
            let mut future = std::pin::pin!(try_read_message_into(&mut source, &mut receiver));
            let mut cx = Context::from_waker(futures::task::noop_waker_ref());
            assert!(future.as_mut().poll(&mut cx).is_pending());
        }
        assert_eq!(receiver.received_bytes(), at);
        assert_eq!(source.position, at);
        for end in [size, size * 2] {
            let message = try_read_message_into(&mut source, &mut receiver)
                .now_or_never()
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(
                message.get_root::<capnp::text::Reader<'_>>().unwrap(),
                "borrowed across segments"
            );
            assert_eq!(source.position, end);
            let polls = source.polls;
            // Re-observing the current frame does not consume the next one.
            assert!(try_read_message_into(&mut source, &mut receiver)
                .now_or_never()
                .unwrap()
                .unwrap()
                .is_some());
            assert_eq!(source.polls, polls);
            receiver.next_message().unwrap();
        }
        assert!(try_read_message_into(&mut source, &mut receiver)
            .now_or_never()
            .unwrap()
            .unwrap()
            .is_none());
    }
}

#[test]
fn read_failures_at_every_byte_are_terminal_but_interruption_is_retried() {
    let wire = frame();
    for at in 0..wire.len() {
        for fault in [
            Fault::Eof,
            Fault::Error(io::ErrorKind::ConnectionReset),
            Fault::Overreport,
            Fault::Error(io::ErrorKind::Interrupted),
        ] {
            let mut words = [ZERO; 64];
            let mut receiver =
                FrameReceiver::new(Word::words_to_bytes_mut(&mut words), Default::default())
                    .unwrap();
            let mut source = Source {
                bytes: &wire,
                position: 0,
                fault: Some((at, fault)),
                polls: 0,
            };
            let result = try_read_message_into(&mut source, &mut receiver)
                .now_or_never()
                .unwrap();
            if matches!(fault, Fault::Error(io::ErrorKind::Interrupted)) {
                assert!(result.unwrap().is_some());
                assert_eq!(source.position, wire.len());
                continue;
            }
            if at == 0 && matches!(fault, Fault::Eof) {
                assert!(result.unwrap().is_none());
                continue;
            }
            let error = result.err().unwrap();
            match fault {
                Fault::Eof => assert_eq!(error.kind, ErrorKind::PrematureEndOfFile),
                Fault::Error(_) => assert_eq!(error.kind, ErrorKind::Disconnected),
                Fault::Overreport => assert!(
                    matches!(error.kind, ErrorKind::InvalidReadCount(n, available) if n == available + 1)
                ),
                _ => unreachable!(),
            }
            assert!(error.extra.is_empty());
            let polls = source.polls;
            assert_eq!(
                try_read_message_into(&mut source, &mut receiver)
                    .now_or_never()
                    .unwrap()
                    .err()
                    .unwrap()
                    .kind,
                error.kind
            );
            assert_eq!(source.polls, polls);
            assert_eq!(source.position, at);
        }
    }
}

#[test]
fn convenience_reader_borrows_storage_and_rejects_capacity_before_body_io() {
    let wire = frame();
    let mut source = Source {
        bytes: &wire,
        position: 0,
        fault: None,
        polls: 0,
    };
    let mut words = [ZERO; 64];
    let reader = try_read_message_no_alloc(
        &mut source,
        Word::words_to_bytes_mut(&mut words),
        Default::default(),
    )
    .now_or_never()
    .unwrap()
    .unwrap()
    .unwrap();
    assert_eq!(
        reader.get_root::<capnp::text::Reader<'_>>().unwrap(),
        "borrowed across segments"
    );
    assert!(try_read_message_no_alloc(
        &mut source,
        Word::words_to_bytes_mut(&mut words),
        Default::default()
    )
    .now_or_never()
    .unwrap()
    .unwrap()
    .is_none());

    source.position = 0;
    let error = try_read_message_no_alloc(
        &mut source,
        Word::words_to_bytes_mut(&mut words[..2]),
        Default::default(),
    )
    .now_or_never()
    .unwrap()
    .err()
    .unwrap();
    assert_eq!(error.kind, ErrorKind::BufferNotLargeEnough);
    assert_eq!(
        source.position, 16,
        "reject after the table, before body reads"
    );
}
