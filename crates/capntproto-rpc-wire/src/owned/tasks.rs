//! Caller-pinned receive tasks over owned message reservations. No pool-wide
//! borrow, temporary frame copy, executor or socket type is required.
use super::{Error, Frame};
use capntproto_rpc_engine::{io::Read, messages};
use core::num::NonZeroUsize;

#[derive(Debug, PartialEq, Eq)]
pub enum ReceiveError<E> {
    Input(crate::io::ReceiveError<E>),
    Storage(messages::Error),
}
#[must_use = "finish this receive to recover its reserved buffer"]
pub struct Received<'pool, 'storage, E> {
    receiving: messages::Receiving<'pool, 'storage>,
    outcome: Result<bool, crate::io::ReceiveError<E>>,
    options: capnp::message::ReaderOptions,
}
impl<'pool, E> Received<'pool, '_, E> {
    /// Only a complete frame can be sealed. EOF, cancellation and every error
    /// discard the reservation, including bytes read before the failure.
    pub fn finish(
        self,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> Result<Option<Frame<'pool>>, ReceiveError<E>> {
        match self.outcome {
            Ok(true) => self
                .receiving
                .seal(messages)
                .map(|lease| Some(Frame::new(lease, self.options)))
                .map_err(ReceiveError::Storage),
            outcome => {
                self.receiving.discard(messages);
                outcome.map(|_| None).map_err(ReceiveError::Input)
            }
        }
    }
}

/// Read one exact standard frame directly into its final pool buffer. Keep this
/// future alive across Pending; dropping it mid-frame is terminal connection
/// teardown, and its reservation remains unavailable until the pool scope ends.
/// Other input/output/application tasks may continue using the pool meanwhile.
pub async fn receive<'pool, 'storage, R: Read>(
    receiving: messages::Receiving<'pool, 'storage>,
    reader: &mut R,
    options: capnp::message::ReaderOptions,
    budget: NonZeroUsize,
) -> Received<'pool, 'storage, R::Error> {
    receive_until(receiving, reader, options, budget, |_| {
        core::task::Poll::Pending
    })
    .await
}

/// Cancel a pending receive without losing its reservation. The stop callback
/// must register the caller's waker when pending. Cancellation is terminal for
/// this byte stream: some header/body bytes may already have been consumed.
/// Always call `finish` on the result before reclaiming the connection's pool.
/// Cancellation wins over readable input in the same poll; no further IO is
/// performed after stop becomes ready. This function needs neither allocation
/// nor an executor, and the receive future can stay pinned on the caller's stack.
pub async fn receive_until<'pool, 'storage, R: Read>(
    mut receiving: messages::Receiving<'pool, 'storage>,
    reader: &mut R,
    options: capnp::message::ReaderOptions,
    budget: NonZeroUsize,
    mut stop: impl FnMut(&mut core::task::Context<'_>) -> core::task::Poll<capnp::ErrorKind>,
) -> Received<'pool, 'storage, R::Error> {
    use crate::io::ReceiveError as InputError;
    use capntproto_rpc_engine::{io::IoError, messages::ReceiveReadError};
    use core::task::Poll;
    let outcome = async {
        let mut cursor = capnp::serialize::FrameCursor::new(receiving.capacity(), options)
            .map_err(|e| InputError::Wire(e.kind))?;
        cursor
            .check_alignment(receiving.received())
            .map_err(|e| InputError::Wire(e.kind))?;
        core::future::poll_fn(|cx| {
            if let Poll::Ready(kind) = stop(cx) {
                return Poll::Ready(Err(InputError::Wire(kind)));
            }
            for _ in 0..budget.get() {
                let end = match cursor.next_read_end() {
                    Ok(Some(end)) => end,
                    Ok(None) => {
                        return Poll::Ready(cursor.complete().map_err(|e| InputError::Wire(e.kind)))
                    }
                    Err(error) => return Poll::Ready(Err(InputError::Wire(error.kind))),
                };
                match receiving.poll_read(reader, end, cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Ok(count)) => {
                        if let Err(error) = cursor.advance(receiving.received(), count) {
                            return Poll::Ready(Err(InputError::Wire(error.kind)));
                        }
                    }
                    Poll::Ready(Err(ReceiveReadError::Io(IoError::Interrupted))) => (),
                    Poll::Ready(Err(ReceiveReadError::Io(IoError::Failed(error)))) => {
                        return Poll::Ready(Err(InputError::Io(error)));
                    }
                    Poll::Ready(Err(ReceiveReadError::InvalidCount { count, available })) => {
                        return Poll::Ready(Err(InputError::Wire(
                            capnp::ErrorKind::InvalidReadCount(count, available),
                        )));
                    }
                    Poll::Ready(Err(ReceiveReadError::InvalidEnd)) => {
                        return Poll::Ready(Err(InputError::Wire(
                            capnp::ErrorKind::BufferNotLargeEnough,
                        )));
                    }
                }
            }
            match cursor.complete() {
                Ok(complete) => Poll::Ready(Ok(complete)),
                Err(error) if error.kind == capnp::ErrorKind::IncompleteFill => {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Err(error) => Poll::Ready(Err(InputError::Wire(error.kind))),
            }
        })
        .await
    }
    .await;
    Received {
        receiving,
        outcome,
        options,
    }
}

/// A completed async builder is sealed only after the caller regains its pool.
/// Both failure and success recover the mutable reservation exactly once.
#[must_use = "finish this build to recover or seal its reserved buffer"]
pub struct Built<'pool, T> {
    writer: messages::Writer<'pool>,
    outcome: capnp::Result<(T, usize)>,
    options: capnp::message::ReaderOptions,
}
impl<'pool, T> Built<'pool, T> {
    pub fn finish(
        self,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> Result<(T, Frame<'pool>), Error> {
        let (value, length) = match self.outcome {
            Ok(value) => value,
            Err(error) => {
                messages.discard(self.writer);
                return Err(error.into());
            }
        };
        match messages.seal(self.writer, length) {
            Ok(lease) => Ok((value, Frame::new(lease, self.options))),
            Err(rejected) => {
                messages.discard(rejected.writer);
                Err(Error::Storage(rejected.error))
            }
        }
    }
}

/// Run a borrowed async builder directly in reserved wire storage. The closure
/// can suspend without keeping the message pool borrowed. The resulting frame
/// still needs protocol/output admission; this task publishes no capabilities.
pub async fn build<'pool, T>(
    mut writing: messages::Writing<'pool, '_>,
    options: capnp::message::ReaderOptions,
    fill: impl for<'b, 'buffer> core::ops::AsyncFnOnce(
        &'b mut capnp::message::FixedBuilder<'buffer>,
    ) -> capnp::Result<T>,
) -> Built<'pool, T> {
    let outcome = async {
        let bytes = writing.bytes();
        if bytes.len() < 16 {
            return Err(capnp::Error::from_kind(
                capnp::ErrorKind::BufferNotLargeEnough,
            ));
        }
        let (value, length) = {
            let mut builder = capnp::message::FixedBuilder::from_bytes(&mut bytes[8..])?;
            let value = fill(&mut builder).await?;
            (value, builder.into_segment().len())
        };
        let words = u32::try_from(length / 8)
            .map_err(|_| capnp::Error::from_kind(capnp::ErrorKind::MessageSizeOverflow))?;
        bytes[..4].copy_from_slice(&0u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&words.to_le_bytes());
        Ok((value, length + 8))
    }
    .await;
    Built {
        writer: writing.into_writer(),
        outcome,
        options,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use capnp::{word, Word};
    use capntproto_rpc_engine::io::IoError;
    use core::{
        cell::Cell,
        future::Future,
        task::{Context, Poll, Waker},
    };

    struct Source<'a> {
        bytes: &'a [u8],
        calls: usize,
    }
    impl Read for Source<'_> {
        type Error = ();
        fn poll_read(
            &mut self,
            _: &mut Context<'_>,
            output: &mut [u8],
        ) -> Poll<Result<usize, IoError<()>>> {
            self.calls += 1;
            if self.bytes.is_empty() {
                return Poll::Pending;
            }
            let count = self.bytes.len().min(output.len());
            output[..count].copy_from_slice(&self.bytes[..count]);
            self.bytes = &self.bytes[count..];
            Poll::Ready(Ok(count))
        }
    }
    #[test]
    fn cancel_before_read_and_during_header_or_body_recovers_fixed_storage_without_allocation() {
        let measured = allocation_counter::measure(|| {
            // One-segment frame, one word, suspended at every incomplete prefix.
            let frame = [0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            for prefix in 0..frame.len() {
                let mut bytes = [word(0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5); 8];
                let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut bytes))];
                let mut sends = [];
                messages::with_messages(&mut buffers, &mut sends, |messages| {
                    let receiving = messages.receive(64).unwrap();
                    let mut source = Source {
                        bytes: &frame[..prefix],
                        calls: 0,
                    };
                    let stop = Cell::new(prefix == 0);
                    {
                        let mut receive = core::pin::pin!(receive_until(
                            receiving,
                            &mut source,
                            Default::default(),
                            NonZeroUsize::new(8).unwrap(),
                            |_| {
                                if stop.get() {
                                    Poll::Ready(capnp::ErrorKind::Disconnected)
                                } else {
                                    Poll::Pending
                                }
                            }
                        ));
                        let mut cx = Context::from_waker(Waker::noop());
                        if prefix != 0 {
                            assert!(receive.as_mut().poll(&mut cx).is_pending());
                        }
                        stop.set(true);
                        let Poll::Ready(received) = receive.as_mut().poll(&mut cx) else {
                            panic!("cancel must complete")
                        };
                        assert!(matches!(
                            received.finish(messages),
                            Err(ReceiveError::Input(crate::io::ReceiveError::Wire(
                                capnp::ErrorKind::Disconnected
                            )))
                        ));
                    }
                    if prefix == 0 {
                        assert_eq!(source.calls, 0);
                    }
                    let writer = messages.reserve(64).unwrap();
                    assert!(messages.write(&writer).iter().all(|byte| *byte == 0));
                    messages.discard(writer);
                })
                .unwrap();
            }
        });
        assert_eq!(measured.count_total, 0);
    }

    #[test]
    fn receive_alignment_uses_the_core_feature_even_when_enabled_by_another_dependency() {
        let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 4];
        let bytes = &mut Word::words_to_bytes_mut(&mut words)[1..17];
        let expected = capnp::serialize::FrameReceiver::new(bytes, Default::default())
            .err()
            .map(|error| error.kind);
        let mut buffers = [messages::Buffer::new(bytes)];
        messages::with_messages(&mut buffers, &mut [], |messages| {
            let mut source = Source { bytes: &[0; 8], calls: 0 };
            let outcome = {
                let mut task = core::pin::pin!(receive(messages.receive(16).unwrap(), &mut source, Default::default(), NonZeroUsize::new(2).unwrap()));
                let Poll::Ready(received) = task.as_mut().poll(&mut Context::from_waker(Waker::noop())) else { panic!("complete buffered header") };
                received.finish(messages)
            };
            if let Some(kind) = expected {
                assert!(matches!(outcome, Err(ReceiveError::Input(crate::io::ReceiveError::Wire(actual))) if actual == kind));
                assert_eq!(source.calls, 0);
            } else {
                outcome.unwrap().unwrap().release(messages);
                assert_eq!(source.calls, 1);
            }
            messages.receive(16).unwrap().discard(messages);
        }).unwrap();
    }

    #[test]
    fn receive_reuse_handles_fragmented_tables_eof_and_never_prefetches() {
        struct Fragments<'a> {
            bytes: &'a [u8],
            chunk: usize,
            polls: usize,
        }
        impl Read for Fragments<'_> {
            type Error = ();
            fn poll_read(
                &mut self,
                cx: &mut Context<'_>,
                output: &mut [u8],
            ) -> Poll<Result<usize, IoError<()>>> {
                self.polls += 1;
                if self.polls.is_multiple_of(7) {
                    return Poll::Ready(Err(IoError::Interrupted));
                }
                if self.polls.is_multiple_of(3) {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                let count = self.chunk.min(output.len()).min(self.bytes.len());
                output[..count].copy_from_slice(&self.bytes[..count]);
                self.bytes = &self.bytes[count..];
                Poll::Ready(Ok(count))
            }
        }
        // Two segments, nonzero padding, followed by an empty frame. Every
        // truncated prefix must discard rather than publishing old pool bytes.
        let frame = [
            word(1, 0, 0, 0, 1, 0, 0, 0),
            word(1, 0, 0, 0, 9, 8, 7, 6),
            word(0, 0, 0, 0, 0, 0, 0, 0),
            word(0, 0, 0, 0, 0, 0, 0, 0),
            word(0, 0, 0, 0, 0, 0, 0, 0),
        ];
        let frame = Word::words_to_bytes(&frame);
        for chunk in 1..=16 {
            for length in 0..=frame.len() {
                let mut bytes = [word(0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5); 8];
                let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut bytes))];
                messages::with_messages(&mut buffers, &mut [], |messages| {
                    let mut source = Fragments {
                        bytes: &frame[..length],
                        chunk,
                        polls: 0,
                    };
                    let outcome = {
                        let mut task = core::pin::pin!(receive(
                            messages.receive(64).unwrap(),
                            &mut source,
                            Default::default(),
                            NonZeroUsize::new(2).unwrap()
                        ));
                        let mut cx = Context::from_waker(Waker::noop());
                        let mut ready = None;
                        for _ in 0..128 {
                            if let Poll::Ready(received) = task.as_mut().poll(&mut cx) {
                                ready = Some(received.finish(messages));
                                break;
                            }
                        }
                        ready.expect("bounded fragmented receive")
                    };
                    if length == 0 {
                        assert!(outcome.unwrap().is_none());
                    } else if length < 32 {
                        assert!(matches!(
                            outcome,
                            Err(ReceiveError::Input(crate::io::ReceiveError::Wire(
                                capnp::ErrorKind::PrematureEndOfFile
                            )))
                        ));
                    } else {
                        let received = outcome.unwrap().unwrap();
                        assert_eq!(source.bytes, &frame[32..length]);
                        assert_eq!(received.size_in_words(messages).unwrap(), 2);
                        received.release(messages);
                    }
                    let receiving = messages.receive(64).unwrap();
                    assert!(receiving.received().is_empty());
                    receiving.discard(messages);
                })
                .unwrap();
            }
        }
    }
}
