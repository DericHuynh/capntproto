//! Checked limits shared by reliable and datagram snapshot capabilities.
#![forbid(unsafe_code)]

use super::{failed, wire};

/// Immutable clock metadata and resource limits for one snapshot stream.
/// Construction validates the limits before any receiver state is allocated.
/// The clock domain and skew describe a caller-supplied clock; they do not
/// authenticate that clock or establish synchronization with a peer.
#[must_use]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    clock_domain: String,
    clock_skew: u64,
    keys: u32,
    capacity: u32,
    max_sequence: u64,
    max_payload_bytes: u32,
    max_waiters: u32,
}

impl Config {
    /// Require a 1–128 byte UTF-8 clock domain, 1–1,024 keys, and a nonzero
    /// pending capacity no larger than the key count. Sequence and waiter
    /// limits are 1–65,536; payloads are 1 byte–1 MiB. Visible plus pending
    /// payload budgets must fit within 64 MiB (metadata is separate).
    ///
    /// All `u64` clock skews are valid. Overflow in deadline comparisons is
    /// treated as expired by the receiver. Datagram adapters additionally
    /// check their smaller per-snapshot limit when binding or connecting.
    pub fn new(
        clock_domain: &str,
        clock_skew: u64,
        keys: u32,
        capacity: u32,
        max_sequence: u64,
        max_payload_bytes: u32,
        max_waiters: u32,
    ) -> capnp::Result<Self> {
        if clock_domain.is_empty()
            || clock_domain.len() > 128
            || keys == 0
            || keys > 1024
            || capacity == 0
            || capacity > keys
            || max_sequence == 0
            || max_sequence > 65536
            || max_payload_bytes == 0
            || max_payload_bytes > 1024 * 1024
            || max_waiters == 0
            || max_waiters > 65536
            || (u64::from(keys) + u64::from(capacity)) * u64::from(max_payload_bytes)
                > 64 * 1024 * 1024
        {
            return Err(failed("invalid realtime resource/clock configuration"));
        }
        Ok(Self {
            clock_domain: clock_domain.into(),
            clock_skew,
            keys,
            capacity,
            max_sequence,
            max_payload_bytes,
            max_waiters,
        })
    }

    pub fn clock_domain(&self) -> &str {
        &self.clock_domain
    }
    pub fn clock_skew(&self) -> u64 {
        self.clock_skew
    }
    pub fn keys(&self) -> u32 {
        self.keys
    }
    pub fn capacity(&self) -> u32 {
        self.capacity
    }
    pub fn max_sequence(&self) -> u64 {
        self.max_sequence
    }
    pub fn max_payload_bytes(&self) -> u32 {
        self.max_payload_bytes
    }
    pub fn max_waiters(&self) -> u32 {
        self.max_waiters
    }

    pub(crate) fn write(&self, mut b: wire::config::Builder<'_>) {
        b.set_clock_domain(self.clock_domain.as_str());
        b.set_clock_skew(self.clock_skew);
        b.set_keys(self.keys);
        b.set_capacity(self.capacity);
        b.set_max_sequence(self.max_sequence);
        b.set_max_payload_bytes(self.max_payload_bytes);
        b.set_max_waiters(self.max_waiters);
    }

    pub(crate) fn read(r: wire::config::Reader<'_>) -> capnp::Result<Self> {
        let domain = r.get_clock_domain()?;
        if domain.len() > 128 {
            return Err(failed("realtime clock domain limit"));
        }
        Self::new(
            domain.to_str()?,
            r.get_clock_skew(),
            r.get_keys(),
            r.get_capacity(),
            r.get_max_sequence(),
            r.get_max_payload_bytes(),
            r.get_max_waiters(),
        )
    }
}
