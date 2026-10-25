//! Last local import ownership, independently of heap allocation or transport.
use super::ImportRefs;

/// Effects of retiring an admitted import. Keys belong to this owner and must
/// identify the same import generation; providers validate that authority.
/// Each callback returns before the next one starts. In particular, `retire`
/// must release table borrows before `release` enters output or user code.
pub trait Owner {
    type Key;
    /// Remove the local import and its lookup registrations exactly once.
    fn retire(&self, key: &Self::Key);
    /// Publish the outstanding remote reference count. If output is full, move
    /// that obligation into caller-owned pending storage before committing its
    /// ticket, or terminate the connection. Returning with an unretained count
    /// would lose cleanup when the final local owner is destroyed.
    fn release(&self, key: &Self::Key, references: &mut ImportRefs);
    fn idle(&self);
}

/// Shared by flat hosted import records and caller-backed finalizers. This
/// function neither creates an owner nor changes reference-count policy.
#[inline(always)]
pub fn run<O: Owner + ?Sized>(owner: &O, key: &O::Key, references: &mut ImportRefs) {
    owner.retire(key);
    owner.release(key, references);
    owner.idle();
}

/// Last local interest borrowing stable connection context. Its wire reference
/// count moves with the owner, including through a suspended application task.
/// The connection must admit a unique key before constructing this finalizer.
/// Store keys in connection records, not finalizers borrowing those records.
///
/// ```compile_fail
/// use capntproto_rpc_engine::references::{import::{Last, Owner}, ImportRefs};
/// fn cannot_escape<O: Owner + 'static>(owner: &O, key: O::Key) -> Last<'static, O> {
///     Last::new(owner, key, ImportRefs::new())
/// }
/// ```
pub struct Last<'a, O: Owner + ?Sized> {
    owner: &'a O,
    key: Option<O::Key>,
    references: ImportRefs,
}
impl<'a, O: Owner + ?Sized> Last<'a, O> {
    pub fn new(owner: &'a O, key: O::Key, references: ImportRefs) -> Self {
        Self {
            owner,
            key: Some(key),
            references,
        }
    }
    pub fn owner(&self) -> &O {
        self.owner
    }
    /// Account only descriptors admitted from the peer, not local clones.
    pub fn references(&mut self) -> &mut ImportRefs {
        &mut self.references
    }
    /// Transfer the entire cleanup obligation, for example to terminal teardown.
    /// The caller must retain both the key and the remote reference count.
    pub fn into_parts(mut self) -> (O::Key, ImportRefs) {
        (
            self.key.take().expect("live import owner"),
            core::mem::take(&mut self.references),
        )
    }
}
impl<O: Owner + ?Sized> Drop for Last<'_, O> {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            run(self.owner, &key, &mut self.references);
        }
    }
}

#[cfg(test)]
mod tests;
