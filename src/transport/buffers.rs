//! Owned stream buffers through upstream quiche's zero-copy send API.
#![forbid(unsafe_code)]

#[derive(Clone, Debug, Default)]
pub(crate) struct Factory;

pub(crate) type Connection = quiche::Connection<Factory>;

impl quiche::BufFactory for Factory {
    type Buf = bytes::Bytes;
    type DgramBuf = Vec<u8>;

    fn buf_from_slice(input: &[u8]) -> Self::Buf {
        bytes::Bytes::copy_from_slice(input)
    }

    fn dgram_buf_from_slice(input: &[u8]) -> Self::DgramBuf {
        input.into()
    }
}
