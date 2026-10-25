//! Hash only trusted, runtime-generated keys. Full key equality (including the
//! generation) remains the authority check; wire IDs never enter this index.
use core::hash::{BuildHasherDefault, Hasher};
#[derive(Default)]
pub(super) struct KeyHasher(u64);
impl Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_u64(u64::from(byte));
        }
    }
    fn write_usize(&mut self, value: usize) {
        self.write_u64(value as u64);
    }
    fn write_u64(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(0x517cc1b727220a95);
    }
}
pub(super) type Bindings<'id> = crate::collections::HashMap<
    capntproto_rpc_engine::clients::Key<'id>,
    usize,
    BuildHasherDefault<KeyHasher>,
>;
