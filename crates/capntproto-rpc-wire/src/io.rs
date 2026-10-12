//! Allocation-free framed receive polling over the caller's IO implementation.
#![forbid(unsafe_code)]
use capntproto_rpc_engine::io::{IoError, Read};
use core::{
    num::NonZeroUsize,
    task::{Context, Poll},
};

#[derive(Debug, PartialEq, Eq)]
pub enum ReceiveError<E> {
    Io(E),
    Wire(capnp::ErrorKind),
}
/// Ready(true) lends a complete frame through receiver.message(); Ready(false)
/// is clean EOF. The receiver owns progress across cancelled/dropped futures.
/// The explicit read-attempt budget includes Interrupted and self-wakes on
/// exhaustion. It never prefetches the next frame or runs application dispatch.
pub fn poll_receive<R: Read>(
    reader: &mut R,
    receiver: &mut capnp::serialize::FrameReceiver<'_>,
    cx: &mut Context<'_>,
    budget: NonZeroUsize,
) -> Poll<Result<bool, ReceiveError<R::Error>>> {
    for _ in 0..budget.get() {
        let mut input = match receiver.input() {
            Ok(Some(input)) => input,
            Ok(None) => {
                return Poll::Ready(
                    receiver
                        .message()
                        .map(|m| m.is_some())
                        .map_err(|e| ReceiveError::Wire(e.kind)),
                )
            }
            Err(e) => return Poll::Ready(Err(ReceiveError::Wire(e.kind))),
        };
        match reader.poll_read(cx, input.bytes()) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(count)) => {
                if let Err(e) = input.commit(count) {
                    return Poll::Ready(Err(ReceiveError::Wire(e.kind)));
                }
            }
            Poll::Ready(Err(IoError::Interrupted)) => (),
            Poll::Ready(Err(IoError::Failed(e))) => {
                receiver.abort(capnp::ErrorKind::Disconnected);
                return Poll::Ready(Err(ReceiveError::Io(e)));
            }
        }
    }
    // Avoid an unnecessary extra wakeup when the last allowed read completed
    // the frame or observed clean EOF.
    match receiver.message() {
        Ok(frame) => Poll::Ready(Ok(frame.is_some())),
        Err(e) if e.kind == capnp::ErrorKind::IncompleteFill => {
            cx.waker().wake_by_ref();
            Poll::Pending
        }
        Err(e) => Poll::Ready(Err(ReceiveError::Wire(e.kind))),
    }
}
