//! Keep one native frame in a vectored write, including its length header.
#![forbid(unsafe_code)]

use bytes::{BufMut, BytesMut};
use std::io::{self, IoSlice};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(super) const BATCH_FRAMES: usize = 8;

/// Read only this validated frame's payload into reusable, initialized storage.
/// Limiting spare capacity prevents consuming the next frame's header.
pub(super) async fn read_payload(
    input: &mut (impl AsyncRead + Unpin),
    bytes: &mut BytesMut,
    length: usize,
) -> io::Result<()> {
    bytes.clear();
    bytes.reserve(length);
    while bytes.len() < length {
        let remaining = length - bytes.len();
        if input.read_buf(&mut (&mut *bytes).limit(remaining)).await? == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) async fn write(
    output: &mut (impl AsyncWrite + Unpin),
    kind: u8,
    payload: &[u8],
) -> io::Result<()> {
    write_batch(output, &[(kind, payload)]).await
}

/// Keep queued frames together, like C++ TwoPartyVatNetwork's queued
/// MessageStream::writeMessages call.
/// TLS can pack their headers and payloads into full records instead of making
/// a tiny extra record for each 16 KiB payload's five-byte frame header.
pub(super) async fn write_batch(
    output: &mut (impl AsyncWrite + Unpin),
    frames: &[(u8, &[u8])],
) -> io::Result<()> {
    if frames.len() > BATCH_FRAMES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "native TCP batch too large",
        ));
    }
    if frames.is_empty() {
        return Ok(());
    }
    let mut headers = [[0; 5]; BATCH_FRAMES];
    for (header, (kind, payload)) in headers.iter_mut().zip(frames) {
        let length = u32::try_from(payload.len()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "native TCP frame too large")
        })?;
        header[0] = *kind;
        header[1..].copy_from_slice(&length.to_be_bytes());
    }
    let mut pieces = [IoSlice::new(&[]); BATCH_FRAMES * 2];
    for (index, (header, (_, payload))) in headers.iter().zip(frames).enumerate() {
        pieces[index * 2] = IoSlice::new(header);
        pieces[index * 2 + 1] = IoSlice::new(payload);
    }
    let mut remaining = &mut pieces[..frames.len() * 2];
    // Separate writes would make rustls encrypt and submit the kind, length,
    // and payload independently. Keep them together without copying payloads.
    // Partial writers may stop anywhere, including inside either header field.
    while !remaining.is_empty() {
        let written = output.write_vectored(remaining).await?;
        if written == 0 {
            return Err(io::ErrorKind::WriteZero.into());
        }
        IoSlice::advance_slices(&mut remaining, written);
    }
    // Receipt acknowledgement is published by the caller only after this fence.
    output.flush().await
}

#[cfg(test)]
mod tests;
