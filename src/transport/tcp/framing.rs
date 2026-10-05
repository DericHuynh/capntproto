//! Keep one native frame in a vectored write, including its length header.
#![forbid(unsafe_code)]

use std::io::{self, IoSlice};
use tokio::io::{AsyncWrite, AsyncWriteExt};

pub(super) async fn write(
    output: &mut (impl AsyncWrite + Unpin),
    kind: u8,
    payload: &[u8],
) -> io::Result<()> {
    let length = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "native TCP frame too large"))?;
    let mut header = [0; 5];
    header[0] = kind;
    header[1..].copy_from_slice(&length.to_be_bytes());
    let mut pieces = [IoSlice::new(&header), IoSlice::new(payload)];
    let mut remaining = &mut pieces[..];
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
