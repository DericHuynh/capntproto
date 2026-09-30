//! Bounded, capability-local RDS2 reassembly. Partial data is not a snapshot.
use super::*;

pub(super) const MAGIC: &[u8; 4] = b"RDS2";
const HEADER: usize = 96;
pub(super) const CHUNK: usize = MAX_DATAGRAM_BYTES - HEADER;
pub const MAX_FRAGMENTS: usize = 64;
pub const MAX_SNAPSHOT_BYTES: usize = CHUNK * MAX_FRAGMENTS;
/// Each grant also respects its configured receiver capacity.
pub const MAX_REASSEMBLIES: usize = 8;
#[cfg(test)]
#[path = "fragment_tests.rs"]
mod tests;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Metadata {
    key: u32,
    deadline: u64,
    total: usize,
    digest: [u8; 32],
}
struct Assembly {
    metadata: Metadata,
    bytes: Vec<u8>,
    received: u64,
}
#[derive(Default)]
pub(super) struct Fragments {
    // Retain commitments even after eviction/completion: a retry cannot change
    // the payload. The capability's finite sequence namespace bounds this map.
    metadata: BTreeMap<u64, Metadata>,
    partial: BTreeMap<u64, Assembly>,
}
impl Fragments {
    pub(super) fn contains(&self, sequence: u64) -> bool {
        self.metadata.contains_key(&sequence)
    }
    pub(super) fn remove(&mut self, sequence: u64) {
        self.partial.remove(&sequence);
    }
    pub(super) fn clear(&mut self) {
        self.partial.clear();
    }
}
fn digest(bytes: &[u8]) -> [u8; 32] {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .try_into()
        .unwrap()
}
pub(super) fn packets(
    token: &[u8; 32],
    sequence: u64,
    key: u32,
    deadline: u64,
    bytes: &[u8],
) -> Vec<Vec<u8>> {
    if bytes.len() <= MAX_PAYLOAD_BYTES {
        return vec![super::packet(token, sequence, key, deadline, bytes)];
    }
    let digest = digest(bytes);
    bytes
        .chunks(CHUNK)
        .enumerate()
        .map(|(i, chunk)| {
            let mut p = super::packet(token, sequence, key, deadline, &[]);
            p[..4].copy_from_slice(MAGIC);
            p.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
            p.extend_from_slice(&((i * CHUNK) as u32).to_be_bytes());
            p.extend_from_slice(&digest);
            p.extend_from_slice(chunk);
            p
        })
        .collect()
}
impl Grant {
    pub(super) fn prune(&self) {
        // Receiver owns clock regression/overflow and publication semantics.
        // Sweep before borrowing reassembly state: clock/wakers are user code.
        if self.receiver.expire().is_err() || self.receiver.is_closed() {
            self.fragments.borrow_mut().clear();
            return;
        }
        let now = self.clock.now().checked_add(self.config.clock_skew());
        let pending = self.receiver.pending();
        self.fragments
            .borrow_mut()
            .partial
            .retain(|sequence, assembly| {
                now.is_some_and(|n| n < assembly.metadata.deadline)
                    && self.receiver.status(*sequence).is_none()
                    && !pending.contains(sequence)
            });
    }
    pub(super) fn fragment(&self, packet: &[u8]) {
        if packet.len() <= HEADER || self.receiver.is_closed() {
            return;
        }
        let sequence = u64::from_be_bytes(packet[36..44].try_into().unwrap());
        let meta = Metadata {
            key: u32::from_be_bytes(packet[44..48].try_into().unwrap()),
            deadline: u64::from_be_bytes(packet[48..56].try_into().unwrap()),
            total: u32::from_be_bytes(packet[56..60].try_into().unwrap()) as usize,
            digest: packet[64..96].try_into().unwrap(),
        };
        let offset = u32::from_be_bytes(packet[60..64].try_into().unwrap()) as usize;
        let bytes = &packet[HEADER..];
        if sequence == 0
            || sequence > self.config.max_sequence()
            || meta.key >= self.config.keys()
            || meta.total <= MAX_PAYLOAD_BYTES
            || meta.total > self.config.max_payload_bytes() as usize
            || meta.total > MAX_SNAPSHOT_BYTES
            || offset >= meta.total
            || !offset.is_multiple_of(CHUNK)
            || bytes.len() != CHUNK.min(meta.total - offset)
            || self.receiver.status(sequence).is_some()
            || self.receiver.pending().contains(&sequence)
        {
            return;
        }
        let timely = self
            .clock
            .now()
            .checked_add(self.config.clock_skew())
            .is_some_and(|n| n < meta.deadline);
        let complete = {
            let mut fragments = self.fragments.borrow_mut();
            let old = fragments.metadata.entry(sequence).or_insert(meta);
            if *old != meta || !timely {
                return;
            }
            let limit = MAX_REASSEMBLIES.min(self.config.capacity() as usize);
            if !fragments.partial.contains_key(&sequence) && fragments.partial.len() >= limit {
                return;
            }
            let assembly = fragments
                .partial
                .entry(sequence)
                .or_insert_with(|| Assembly {
                    metadata: meta,
                    bytes: vec![0; meta.total],
                    received: 0,
                });
            let bit = 1u64 << (offset / CHUNK);
            if assembly.received & bit != 0 {
                // Identical duplicates are harmless; conflicting duplicates do
                // not overwrite the first copy or advance completeness.
                return;
            }
            assembly.bytes[offset..offset + bytes.len()].copy_from_slice(bytes);
            assembly.received |= bit;
            if assembly.received.count_ones() as usize != meta.total.div_ceil(CHUNK) {
                return;
            }
            fragments.partial.remove(&sequence).unwrap().bytes
        };
        if digest(&complete) == meta.digest {
            // Only complete, committed bytes enter the snapshot receiver. Drop
            // its receipt waiter without canceling the admitted work.
            drop(
                self.receiver
                    .offer(sequence, meta.key, meta.deadline, &complete),
            );
        }
        // A bad digest releases the buffer, retaining the commitment. An exact
        // retry can recover from a corrupt first copy; mixed data never publishes.
    }
}
