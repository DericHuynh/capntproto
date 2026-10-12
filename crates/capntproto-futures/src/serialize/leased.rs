//! AsyncWrite adapter for allocation-free message/send ownership.
#![forbid(unsafe_code)]
use capntproto_rpc_engine::messages::{Error, Messages, SendKey};
use core::{
    fmt,
    num::NonZeroUsize,
    task::{Context, Poll},
};
use futures_util::AsyncWrite;

/// Compact failures without formatted/allocated diagnostic strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendError {
    Io(std::io::ErrorKind),
    State(Error),
}
impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(kind) => kind.fmt(f),
            Self::State(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for SendError {}

/// Advance an accepted, already-framed message with an explicit IO-attempt
/// budget. Every completed write commits progress immediately; Pending retains
/// the unsent suffix. Budget exhaustion self-wakes, and Interrupted consumes one
/// attempt. This function never flushes, closes or acknowledges an RPC.
///
/// An IO/count failure releases only this send's ownership after the IO borrow
/// ends. The caller must terminate a partially written framed stream before
/// sending another frame. Stale keys do not touch IO or a replacement send.
/// The writer and waker retain their own allocation requirements.
pub fn poll_queued_message<'id, W: AsyncWrite + Unpin>(
    writer: &mut W,
    messages: &mut Messages<'id, '_, '_>,
    key: SendKey<'id>,
    cx: &mut Context<'_>,
    writes_per_poll: NonZeroUsize,
) -> Poll<Result<(), SendError>> {
    capntproto_rpc_engine::io::poll_send(
        &mut crate::io::Adapter(writer),
        messages,
        key,
        cx,
        writes_per_poll,
    )
    .map_err(|error| match error {
        capntproto_rpc_engine::io::SendError::Io(kind) => SendError::Io(kind),
        capntproto_rpc_engine::io::SendError::State(error) => SendError::State(error),
    })
}

/// Drive [`poll_queued_message`] without allocating a future. Canceling the
/// future preserves completed writes and the pending send reference. Resume on
/// the same stream/key, or explicitly fail the send after stopping IO.
/// This convenience future borrows the pool exclusively; a multiplexing driver
/// can call the poll function for separate keys within its own work budget.
pub async fn write_queued_message<'id, W: AsyncWrite + Unpin>(
    writer: &mut W,
    messages: &mut Messages<'id, '_, '_>,
    key: SendKey<'id>,
    writes_per_poll: NonZeroUsize,
) -> Result<(), SendError> {
    futures_util::future::poll_fn(|cx| {
        poll_queued_message(writer, messages, key, cx, writes_per_poll)
    })
    .await
}

#[cfg(test)]
mod tests;
