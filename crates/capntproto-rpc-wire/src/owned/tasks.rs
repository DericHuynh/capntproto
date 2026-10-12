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
pub struct Received<'pool, E> {
    writer: messages::Writer<'pool>,
    outcome: Result<Option<usize>, crate::io::ReceiveError<E>>,
    options: capnp::message::ReaderOptions,
}
impl<'pool, E> Received<'pool, E> {
    /// Only a complete frame can be sealed. EOF and every error recover the
    /// writer. A framing/transport error still requires connection teardown.
    pub fn finish(
        self,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> Result<Option<Frame<'pool>>, ReceiveError<E>> {
        match self.outcome {
            Ok(Some(length)) => match messages.seal(self.writer, length) {
                Ok(lease) => Ok(Some(Frame::new(lease, self.options))),
                Err(rejected) => {
                    messages.discard(rejected.writer);
                    Err(ReceiveError::Storage(rejected.error))
                }
            },
            outcome => {
                messages.discard(self.writer);
                outcome.map(|_| None).map_err(ReceiveError::Input)
            }
        }
    }
}

/// Read one exact standard frame directly into its final pool buffer. Keep this
/// future alive across Pending; dropping it mid-frame is terminal connection
/// teardown, and its reservation remains unavailable until the pool scope ends.
/// Other input/output/application tasks may continue using the pool meanwhile.
pub async fn receive<'pool, R: Read>(
    writing: messages::Writing<'pool, '_>,
    reader: &mut R,
    options: capnp::message::ReaderOptions,
    budget: NonZeroUsize,
) -> Received<'pool, R::Error> {
    receive_until(writing, reader, options, budget, |_| {
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
pub async fn receive_until<'pool, R: Read>(
    mut writing: messages::Writing<'pool, '_>,
    reader: &mut R,
    options: capnp::message::ReaderOptions,
    budget: NonZeroUsize,
    mut stop: impl FnMut(&mut core::task::Context<'_>) -> core::task::Poll<capnp::ErrorKind>,
) -> Received<'pool, R::Error> {
    let outcome = async {
        let mut receiver = capnp::serialize::FrameReceiver::new(writing.bytes(), options)
            .map_err(|e| crate::io::ReceiveError::Wire(e.kind))?;
        let complete = core::future::poll_fn(|cx| {
            if let core::task::Poll::Ready(kind) = stop(cx) {
                return core::task::Poll::Ready(Err(crate::io::ReceiveError::Wire(kind)));
            }
            crate::io::poll_receive(reader, &mut receiver, cx, budget)
        })
        .await?;
        Ok(complete.then_some(receiver.received_bytes()))
    }
    .await;
    Received {
        writer: writing.into_writer(),
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
                let mut bytes = [word(0, 0, 0, 0, 0, 0, 0, 0); 8];
                let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut bytes))];
                let mut sends = [];
                messages::with_messages(&mut buffers, &mut sends, |messages| {
                    let writer = messages.reserve(64).unwrap();
                    let writing = messages
                        .write_owned(writer)
                        .unwrap_or_else(|_| panic!("fresh writer"));
                    let mut source = Source {
                        bytes: &frame[..prefix],
                        calls: 0,
                    };
                    let stop = Cell::new(prefix == 0);
                    {
                        let mut receive = core::pin::pin!(receive_until(
                            writing,
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
                    messages.discard(writer);
                })
                .unwrap();
            }
        });
        assert_eq!(measured.count_total, 0);
    }
}
