//! Caller-polled IO; no executor, socket type, allocation or global clock.
#![forbid(unsafe_code)]
use crate::messages::{self, Messages, Queue, SendKey};
use core::{
    num::NonZeroUsize,
    task::{Context, Poll},
};

#[derive(Debug, PartialEq, Eq)]
pub enum IoError<E> {
    Interrupted,
    Failed(E),
}
pub trait Read {
    type Error;
    /// Pending must arrange a wakeup and must not retain the buffer borrow.
    fn poll_read(
        &mut self,
        cx: &mut Context<'_>,
        bytes: &mut [u8],
    ) -> Poll<Result<usize, IoError<Self::Error>>>;
}
/// Flush accepted bytes to the underlying transport. This is local completion,
/// not peer receipt. Unbuffered adapters explicitly return Ready(Ok(())).
/// Pending must register a wakeup; Interrupted is retried with a caller budget.
pub trait Flush {
    type Error;
    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), IoError<Self::Error>>>;
}
/// Optional ownership of the transport's write half. Close is polled only after
/// queued output has flushed. Completion means the write half is closed.
pub trait Close: Flush {
    fn poll_close(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), IoError<Self::Error>>>;
}
pub trait Write: Flush {
    /// The returned count is consumed before the next call. Pending must arrange
    /// a wakeup and must not retain the buffer borrow. No hidden flush is implied.
    fn poll_write(
        &mut self,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<Result<usize, IoError<Self::Error>>>;
}
/// Stream IO with optional resources attached to the first byte of a frame.
///
/// Pending, Interrupted and errors MUST NOT transfer bytes or resources. A
/// positive count transfers all supplied resources with the first accepted byte.
/// Implementations must not retain either borrow, and must arrange wakeups on
/// Pending. A transport without attachment support may discard the metadata.
/// Async completion belongs inside this adapter; report bytes only when IO has
/// accepted them, never merely because an operation has been started.
pub trait WriteResources<R>: Flush {
    /// Permit new writes while a previous physical flush is pending. This is
    /// only valid for adapters that independently own accepted bytes, resources
    /// and admission credit until completion, with bounded queue admission.
    /// A successful flush must still cover ALL accepted writes, including those
    /// added since the first flush poll. Ordinary borrowed buffers keep the
    /// pending-flush barrier by returning false.
    fn can_write_while_flushing(&self) -> bool {
        false
    }
    /// Hint the bounded number of queued frames before the next flush poll.
    /// The actual writes can stop early on partial IO or backpressure. This is
    /// not acceptance: all byte, resource and flush obligations remain in force.
    fn start_batch(&mut self, _frames: usize) {}
    /// Opt into retaining allocating message buffers through asynchronous IO.
    /// Fixed providers continue to use borrowed writes. Retained bytes consume
    /// the pool's capacity until this adapter releases them.
    #[cfg(feature = "alloc")]
    fn accepts_shared(&self) -> bool {
        false
    }
    /// Same acceptance/attachment contract as a borrowed write. Pending and
    /// errors must release this owner; successful IO may retain it until flush.
    #[cfg(feature = "alloc")]
    fn poll_write_shared(
        &mut self,
        cx: &mut Context<'_>,
        bytes: messages::SharedBytes,
        resources: Option<&R>,
    ) -> Poll<Result<usize, IoError<Self::Error>>> {
        self.poll_write_resources(cx, &bytes, resources)
    }
    /// Consume some or all remaining portions. The default borrows just the
    /// first remaining part; retaining adapters may accept the complete owner.
    /// Pending/error must drop the supplied alias, just like poll_write_shared.
    #[cfg(feature = "alloc")]
    fn accepts_parts(&self) -> bool {
        false
    }
    #[cfg(feature = "alloc")]
    fn poll_write_parts(
        &mut self,
        cx: &mut Context<'_>,
        parts: messages::SharedParts,
        resources: Option<&R>,
    ) -> Poll<Result<usize, IoError<Self::Error>>> {
        self.poll_write_resources(cx, parts.first_remaining(), resources)
    }
    fn poll_write_resources(
        &mut self,
        cx: &mut Context<'_>,
        bytes: &[u8],
        resources: Option<&R>,
    ) -> Poll<Result<usize, IoError<Self::Error>>>;
}

mod output;
pub use output::{poll_close, Output};

#[derive(Debug, PartialEq, Eq)]
pub enum SendError<E> {
    Io(E),
    State(messages::Error),
}

/// Poll FIFO output with an explicit attempt budget. Partial progress is committed
/// before yielding. An error permanently fails the queue; terminate the connection
/// and quiesce IO before releasing its tables, buffers and remote grants.
pub fn poll_queue<'pool, W: Write>(
    writer: &mut W,
    queue: &mut Queue<'pool, '_>,
    messages: &mut Messages<'pool, '_, '_>,
    cx: &mut Context<'_>,
    budget: NonZeroUsize,
) -> Poll<Result<(), SendError<W::Error>>> {
    struct Bytes<'a, W>(&'a mut W);
    impl<W: Write> Flush for Bytes<'_, W> {
        type Error = W::Error;
        fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), IoError<Self::Error>>> {
            self.0.poll_flush(cx)
        }
    }
    impl<W: Write> WriteResources<()> for Bytes<'_, W> {
        fn poll_write_resources(
            &mut self,
            cx: &mut Context<'_>,
            bytes: &[u8],
            _: Option<&()>,
        ) -> Poll<Result<usize, IoError<Self::Error>>> {
            self.0.poll_write(cx, bytes)
        }
    }
    poll_queue_resources(&mut Bytes(writer), queue, messages, cx, budget)
}

/// Drive bytes and resources from the same accepted FIFO entry. Attachments
/// survive retries, are not offered again after the first positive write, and
/// remain owned until completion or terminal cleanup after IO is stopped.
pub fn poll_queue_resources<'pool, R, W: WriteResources<R>>(
    writer: &mut W,
    queue: &mut Queue<'pool, '_, R>,
    messages: &mut Messages<'pool, '_, '_>,
    cx: &mut Context<'_>,
    budget: NonZeroUsize,
) -> Poll<Result<(), SendError<W::Error>>> {
    for _ in 0..budget.get() {
        let progress = {
            let window = match queue.window(messages) {
                Ok(Some(window)) => window,
                Ok(None) => return Poll::Ready(Ok(())),
                Err(e) => {
                    queue.fail();
                    return Poll::Ready(Err(SendError::State(e)));
                }
            };
            if window.bytes().is_empty() {
                window.advance(0).map_err(SendError::State)
            } else {
                #[cfg(feature = "alloc")]
                let shared = writer.accepts_shared().then(|| window.share()).flatten();
                #[cfg(feature = "alloc")]
                let parts = writer
                    .accepts_parts()
                    .then(|| window.share_parts())
                    .flatten();
                #[cfg(feature = "alloc")]
                let accepts_parts = parts.is_some();
                #[cfg(feature = "alloc")]
                let result = match parts {
                    Some(parts) => writer.poll_write_parts(cx, parts, window.resources()),
                    None => match shared {
                        Some(bytes) => writer.poll_write_shared(cx, bytes, window.resources()),
                        None => writer.poll_write_resources(cx, window.bytes(), window.resources()),
                    },
                };
                #[cfg(not(feature = "alloc"))]
                let result = writer.poll_write_resources(cx, window.bytes(), window.resources());
                match result {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Ok(count)) => {
                        #[cfg(feature = "alloc")]
                        if accepts_parts {
                            window.advance_parts(count).map_err(SendError::State)
                        } else {
                            window.advance(count).map_err(SendError::State)
                        }
                        #[cfg(not(feature = "alloc"))]
                        window.advance(count).map_err(SendError::State)
                    }
                    Poll::Ready(Err(IoError::Interrupted)) => continue,
                    Poll::Ready(Err(IoError::Failed(e))) => Err(SendError::Io(e)),
                }
            }
        };
        if let Err(e) = progress {
            queue.fail();
            return Poll::Ready(Err(e));
        }
        if queue.is_empty() {
            return Poll::Ready(Ok(()));
        }
    }
    cx.waker().wake_by_ref();
    Poll::Pending
}

/// Single accepted message adapter, shared by the hosted futures bridge. A
/// failure releases this send only; the caller must terminate its framed stream.
pub fn poll_send<'pool, W: Write>(
    writer: &mut W,
    messages: &mut Messages<'pool, '_, '_>,
    key: SendKey<'pool>,
    cx: &mut Context<'_>,
    budget: NonZeroUsize,
) -> Poll<Result<(), SendError<W::Error>>> {
    for _ in 0..budget.get() {
        let progress = {
            let window = match messages.send_window(key) {
                Ok(window) => window,
                Err(e) => return Poll::Ready(Err(SendError::State(e))),
            };
            if window.bytes().is_empty() {
                window.advance(0).map_err(SendError::State)
            } else {
                match writer.poll_write(cx, window.bytes()) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Ok(count)) => window.advance(count).map_err(SendError::State),
                    Poll::Ready(Err(IoError::Interrupted)) => continue,
                    Poll::Ready(Err(IoError::Failed(e))) => Err(SendError::Io(e)),
                }
            }
        };
        match progress {
            Ok(true) => return Poll::Ready(Ok(())),
            Ok(false) => (),
            Err(e) => {
                let _ = messages.fail_send(key);
                return Poll::Ready(Err(e));
            }
        }
    }
    cx.waker().wake_by_ref();
    Poll::Pending
}
