//! Owner lookup for locally allocated outgoing question IDs. Peers cannot
//! insert keys: the branded question ledger admits every indexed publication.
//! Keep sparse storage (including high ID ranges) without hashing these trusted
//! integers with SipHash. Equality and the full generational key still decide
//! ownership; this index grants no protocol authority.
use std::{
    collections::HashMap,
    hash::{BuildHasherDefault, Hasher},
};

pub(super) type ReplyOwners = HashMap<u32, usize, BuildHasherDefault<LocalIdHasher>>;

#[derive(Default)]
pub(super) struct LocalIdHasher(u64);
impl Hasher for LocalIdHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write_u32(&mut self, id: u32) {
        // Spread consecutive IDs into both bucket and fingerprint bits. Unlike
        // identity hashing, small IDs do not all have the same high fingerprint.
        self.0 = u64::from(id).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
    fn write(&mut self, bytes: &[u8]) {
        self.write_u32(u32::from_ne_bytes(bytes.try_into().expect("u32 reply ID")));
    }
}
