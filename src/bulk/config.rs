//! Validated bulk limits shared by local, wire and durable metadata imports.
#![forbid(unsafe_code)]

use super::{failed, wire};

/// Immutable transfer limits. Construction checks that the chunk budget can
/// carry the declared length and that each chunk fits the byte-credit window.
/// A zero-length transfer is valid; chunk size and count must remain nonzero.
/// These limits describe a transfer, not authority to access one.
#[must_use]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Config {
    length: u64,
    max_chunk_bytes: u32,
    window_bytes: u32,
    max_chunks: u32,
}

impl Config {
    /// Accept at most 64 MiB total, 1 MiB per chunk, 16 MiB in flight and
    /// 65,536 chunks. Rejected imports never produce a `Config`.
    pub fn new(
        length: u64,
        max_chunk_bytes: u32,
        window_bytes: u32,
        max_chunks: u32,
    ) -> capnp::Result<Self> {
        if length > 64 * 1024 * 1024
            || max_chunk_bytes == 0
            || max_chunk_bytes > 1024 * 1024
            || window_bytes < max_chunk_bytes
            || window_bytes > 16 * 1024 * 1024
            || max_chunks == 0
            || max_chunks > 65536
            || length > u64::from(max_chunks) * u64::from(max_chunk_bytes)
        {
            return Err(failed("invalid bulk limits"));
        }
        Ok(Self {
            length,
            max_chunk_bytes,
            window_bytes,
            max_chunks,
        })
    }

    pub fn length(&self) -> u64 {
        self.length
    }
    pub fn max_chunk_bytes(&self) -> u32 {
        self.max_chunk_bytes
    }
    pub fn window_bytes(&self) -> u32 {
        self.window_bytes
    }
    pub fn max_chunks(&self) -> u32 {
        self.max_chunks
    }

    pub(crate) fn write(&self, mut out: wire::config::Builder<'_>) {
        out.set_length(self.length);
        out.set_max_chunk_bytes(self.max_chunk_bytes);
        out.set_window_bytes(self.window_bytes);
        out.set_max_chunks(self.max_chunks);
    }
    pub(crate) fn read(r: wire::config::Reader<'_>) -> capnp::Result<Self> {
        Self::new(
            r.get_length(),
            r.get_max_chunk_bytes(),
            r.get_window_bytes(),
            r.get_max_chunks(),
        )
    }
}

impl<'de> serde::Deserialize<'de> for Config {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Import {
            length: u64,
            max_chunk_bytes: u32,
            window_bytes: u32,
            max_chunks: u32,
        }
        let raw = Import::deserialize(deserializer)?;
        Self::new(
            raw.length,
            raw.max_chunk_bytes,
            raw.window_bytes,
            raw.max_chunks,
        )
        .map_err(serde::de::Error::custom)
    }
}
