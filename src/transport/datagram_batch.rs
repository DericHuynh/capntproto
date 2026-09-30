//! Packet-bound queue capacity. Reserve before committing protocol state, then
//! publish only after releasing state borrows: sending may invoke user wakers.
#![forbid(unsafe_code)]

use super::{DATAGRAM_QUEUE, MAX_DATAGRAM_BYTES};
use std::io;
use tokio::sync::mpsc::{PermitIterator, Sender};

/// A complete validated batch and the capacity to enqueue it once. Both packet
/// contents and the queue remain borrowed until submission or cancellation.
/// Dropping a reservation sends nothing and releases its capacity. Sending can
/// wake arbitrary code; callers must commit their local state before `send`.
#[must_use = "send the reserved batch after committing local state, or drop it to release capacity"]
pub(crate) struct Batch<'a> {
    permits: PermitIterator<'a, Vec<u8>>,
    packets: &'a [Vec<u8>],
}

pub(super) fn reserve<'a>(
    sender: &'a Sender<Vec<u8>>,
    packets: &'a [Vec<u8>],
) -> io::Result<Batch<'a>> {
    if packets.is_empty()
        || packets.len() > DATAGRAM_QUEUE
        || packets.iter().any(|p| p.len() > MAX_DATAGRAM_BYTES)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid datagram batch",
        ));
    }
    let permits = sender
        .try_reserve_many(packets.len())
        .map_err(|e| match e {
            tokio::sync::mpsc::error::TrySendError::Full(_) => {
                io::Error::new(io::ErrorKind::WouldBlock, "datagram queue full")
            }
            tokio::sync::mpsc::error::TrySendError::Closed(_) => {
                io::Error::new(io::ErrorKind::BrokenPipe, "datagram session closed")
            }
        })?;
    Ok(Batch { permits, packets })
}

impl Batch<'_> {
    /// Uses exactly the reserved packets/slots. Queue closure after reservation
    /// does not revoke permits; submission still establishes no remote delivery.
    pub(crate) fn send(self) {
        for (permit, packet) in self.permits.zip(self.packets) {
            permit.send(packet.clone());
        }
    }
}
