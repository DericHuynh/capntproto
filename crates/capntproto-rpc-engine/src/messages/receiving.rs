//! Receive-only ownership: previous contents never become a builder or lease.
use super::{Error, Lease, Messages, Writing};
use crate::io::{IoError, Read};
use core::task::{Context, Poll};

#[derive(Debug, PartialEq, Eq)]
pub enum ReceiveReadError<E> {
    Io(IoError<E>),
    InvalidCount { count: usize, available: usize },
    InvalidEnd,
}

/// Exclusive receive storage, with an independently tracked committed prefix.
/// The IO adapter must initialize every byte it reports, as for ordinary reads.
/// No API exposes previous contents to a decoder, builder, or outgoing send.
#[must_use = "seal or discard the receive, or terminate the pool scope"]
pub struct Receiving<'id, 'storage> {
    writing: Writing<'id, 'storage>,
    position: usize,
}

impl<'id, 'storage> Receiving<'id, 'storage> {
    pub(super) fn new(writing: Writing<'id, 'storage>) -> Self {
        Self {
            writing,
            position: 0,
        }
    }
    /// Start receiving into an existing cleared builder reservation.
    pub fn from_writing(writing: Writing<'id, 'storage>) -> Self {
        Self::new(writing)
    }
    pub fn capacity(&self) -> usize {
        self.writing.view.len()
    }
    /// Only the bytes this IO operation has initialized, even after reuse.
    pub fn received(&self) -> &[u8] {
        &self.writing.view[..self.position]
    }
    /// Read up to the decoder's next boundary, without prefetching beyond it.
    /// Pending and Interrupted preserve progress; an invalid count is rejected
    /// before changing the committed prefix. Zero denotes EOF, not completion.
    pub fn poll_read<R: Read>(
        &mut self,
        reader: &mut R,
        end: usize,
        cx: &mut Context<'_>,
    ) -> Poll<Result<usize, ReceiveReadError<R::Error>>> {
        if end <= self.position || end > self.capacity() {
            return Poll::Ready(Err(ReceiveReadError::InvalidEnd));
        }
        let available = end - self.position;
        match reader.poll_read(cx, &mut self.writing.view[self.position..end]) {
            Poll::Ready(Ok(count)) if count <= available => {
                self.position += count;
                Poll::Ready(Ok(count))
            }
            Poll::Ready(Ok(count)) => {
                Poll::Ready(Err(ReceiveReadError::InvalidCount { count, available }))
            }
            result => result.map_err(ReceiveReadError::Io),
        }
    }
    /// Publish exactly the received prefix. Framing/semantic validation belongs
    /// to the decoder, which must call this only after a complete valid frame.
    pub fn seal(self, messages: &mut Messages<'id, '_, '_>) -> Result<Lease<'id>, Error> {
        match messages.seal(self.writing.into_writer(), self.position) {
            Ok(lease) => Ok(lease),
            Err(rejected) => {
                messages.discard(rejected.writer);
                Err(rejected.error)
            }
        }
    }
    pub fn discard(self, messages: &mut Messages<'id, '_, '_>) {
        messages.discard(self.writing.into_writer());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::{with_messages, Buffer};
    use core::task::Waker;

    struct Source;
    impl Read for Source {
        type Error = ();
        fn poll_read(
            &mut self,
            _: &mut Context<'_>,
            bytes: &mut [u8],
        ) -> Poll<Result<usize, IoError<()>>> {
            // The adapter alone sees retained storage; no clearing preceded IO.
            assert_eq!(bytes, &[0xa5; 4]);
            bytes[..2].copy_from_slice(b"ok");
            Poll::Ready(Ok(2))
        }
    }
    #[test]
    fn reused_storage_exposes_only_committed_prefix_and_builders_still_clear() {
        let usage = allocation_counter::measure(|| {
            let mut bytes = [0xa5; 16];
            let mut buffers = [Buffer::new(&mut bytes)];
            with_messages(&mut buffers, &mut [], |messages| {
                let mut receiving = messages.receive(8).unwrap();
                assert!(receiving.received().is_empty());
                assert_eq!(receiving.capacity(), 8);
                assert!(!messages.can_reserve(0));
                assert_eq!(
                    receiving.poll_read(&mut Source, 4, &mut Context::from_waker(Waker::noop())),
                    Poll::Ready(Ok(2))
                );
                assert_eq!(receiving.received(), b"ok");
                let lease = receiving.seal(messages).unwrap();
                assert_eq!(&*messages.read(&lease), b"ok");
                messages.release(lease);
                let receiving = messages.receive(8).unwrap();
                assert!(receiving.received().is_empty());
                receiving.discard(messages);
                let writer = messages.reserve(8).unwrap();
                assert_eq!(&*messages.write(&writer), &[0; 16]);
                messages.discard(writer);
            })
            .unwrap();
        });
        assert_eq!(usage.count_total, 0);
    }
    #[test]
    fn invalid_adapter_count_cannot_expose_previous_contents_and_drop_pins_storage() {
        struct Invalid;
        impl Read for Invalid {
            type Error = ();
            fn poll_read(
                &mut self,
                _: &mut Context<'_>,
                bytes: &mut [u8],
            ) -> Poll<Result<usize, IoError<()>>> {
                Poll::Ready(Ok(bytes.len() + 1))
            }
        }
        let mut bytes = [0xa5; 16];
        let mut buffers = [Buffer::new(&mut bytes)];
        with_messages(&mut buffers, &mut [], |messages| {
            let mut receiving = messages.receive(8).unwrap();
            let mut cx = Context::from_waker(Waker::noop());
            assert_eq!(
                receiving.poll_read(&mut Invalid, 8, &mut cx),
                Poll::Ready(Err(ReceiveReadError::InvalidCount {
                    count: 9,
                    available: 8
                }))
            );
            assert_eq!(
                receiving.poll_read(&mut Invalid, 9, &mut cx),
                Poll::Ready(Err(ReceiveReadError::InvalidEnd))
            );
            assert!(receiving.received().is_empty());
            drop(receiving);
            assert!(!messages.can_reserve(0));
        })
        .unwrap();
    }
}
