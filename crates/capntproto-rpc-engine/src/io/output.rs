//! Buffered output completion is separate from byte/attachment acceptance.
use super::*;

/// Caller-owned, allocation-free output progress. Flush partial accepted writes
/// even when the FIFO writer is Pending: waiting for an entire frame can deadlock
/// an adapter whose bounded buffer needs flushing before accepting more bytes.
/// Keep this state and the writer together until completion or terminal abort.
#[derive(Default)]
pub struct Output {
    flushing: bool,
}
impl Output {
    pub const fn new() -> Self {
        Self { flushing: false }
    }
    /// No accepted bytes remain to flush. A caller must also check its FIFO
    /// and failure state before using this as an idle certificate.
    pub fn is_clean(&self) -> bool {
        !self.flushing
    }

    /// Poll writes and flushes with bounded work. Ready(Ok) means both the FIFO
    /// and transport buffer are empty. The queue's individual receipts still
    /// indicate byte acceptance only. A pending flush precedes any later write,
    /// and an idle, clean output does not flush or schedule itself.
    #[inline(always)]
    pub fn poll<'pool, R, W: WriteResources<R>>(
        &mut self,
        writer: &mut W,
        queue: &mut Queue<'pool, '_, R>,
        messages: &mut Messages<'pool, '_, '_>,
        cx: &mut Context<'_>,
        budget: NonZeroUsize,
    ) -> Poll<Result<(), SendError<W::Error>>> {
        if queue.is_failed() {
            return Poll::Ready(Err(SendError::State(messages::Error::QueueFailed)));
        }
        if !self.flushing && queue.is_empty() {
            return Poll::Ready(Ok(()));
        }
        if !self.flushing {
            let mut tracked = Tracking {
                writer,
                accepted: &mut self.flushing,
            };
            if let Poll::Ready(Err(error)) =
                poll_queue_resources(&mut tracked, queue, messages, cx, budget)
            {
                return Poll::Ready(Err(error));
            }
        }
        if self.flushing {
            for attempt in 0..budget.get() {
                match writer.poll_flush(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Err(IoError::Failed(error))) => {
                        queue.fail();
                        return Poll::Ready(Err(SendError::Io(error)));
                    }
                    Poll::Ready(Err(IoError::Interrupted)) => {
                        if attempt + 1 == budget.get() {
                            cx.waker().wake_by_ref();
                            return Poll::Pending;
                        }
                    }
                    Poll::Ready(Ok(())) => {
                        self.flushing = false;
                        // Flushing can make a previously blocked writer ready
                        // synchronously, without another readiness notification.
                        if !queue.is_empty() {
                            cx.waker().wake_by_ref();
                        }
                        break;
                    }
                }
            }
        }
        if queue.is_empty() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
}

struct Tracking<'a, W> {
    writer: &'a mut W,
    accepted: &'a mut bool,
}
impl<W: Flush> Flush for Tracking<'_, W> {
    type Error = W::Error;
    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), IoError<Self::Error>>> {
        self.writer.poll_flush(cx)
    }
}
impl<R, W: WriteResources<R>> WriteResources<R> for Tracking<'_, W> {
    #[cfg(feature = "alloc")]
    fn accepts_shared(&self) -> bool {
        self.writer.accepts_shared()
    }
    #[cfg(feature = "alloc")]
    fn poll_write_shared(
        &mut self,
        cx: &mut Context<'_>,
        bytes: messages::SharedBytes,
        resources: Option<&R>,
    ) -> Poll<Result<usize, IoError<Self::Error>>> {
        let result = self.writer.poll_write_shared(cx, bytes, resources);
        if matches!(result, Poll::Ready(Ok(count)) if count > 0) {
            *self.accepted = true;
        }
        result
    }
    fn poll_write_resources(
        &mut self,
        cx: &mut Context<'_>,
        bytes: &[u8],
        resources: Option<&R>,
    ) -> Poll<Result<usize, IoError<Self::Error>>> {
        let result = self.writer.poll_write_resources(cx, bytes, resources);
        if matches!(result, Poll::Ready(Ok(count)) if count > 0) {
            *self.accepted = true;
        }
        result
    }
}

/// Close an already-drained writer, bounding repeated Interrupted responses.
/// The caller supplies cancellation/deadlines and must stop all subsequent IO.
pub fn poll_close<W: Close>(
    writer: &mut W,
    cx: &mut Context<'_>,
    budget: NonZeroUsize,
) -> Poll<Result<(), W::Error>> {
    for _ in 0..budget.get() {
        match writer.poll_close(cx) {
            Poll::Ready(Err(IoError::Interrupted)) => (),
            Poll::Ready(Err(IoError::Failed(error))) => return Poll::Ready(Err(error)),
            Poll::Ready(Ok(())) => return Poll::Ready(Ok(())),
            Poll::Pending => return Poll::Pending,
        }
    }
    cx.waker().wake_by_ref();
    Poll::Pending
}

#[cfg(kani)]
mod proofs;
#[cfg(test)]
mod tests;
