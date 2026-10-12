//! Optional transport resources associated with capability descriptors.
//!
//! The wire index is metadata, never capability authority. Missing resources
//! are ignored as required by the protocol. Owned admission delivers resources
//! only after all wire references commit, including aliases of existing imports.
//! The transport supplies the resources from this exact message and retains
//! them unchanged if admission rejects it. No allocation or OS type is needed.
#![forbid(unsafe_code)]

use capntproto_rpc_engine::ids::ImportId;

/// Import construction and optional post-admission resource delivery.
///
/// `attach` is infallible: protocol admission has already committed. It receives
/// the exact admitted owner, or None for null/reflected descriptors. Implementors
/// must consume each attachment at most once and ignore unavailable indices.
/// Ordinary import factory closures ignore optional transport resources.
pub trait ImportReceiver<I> {
    fn make_import(&mut self, id: ImportId) -> I;
    fn attach(&mut self, _owner: Option<&mut I>, _index: u8) {}
    /// Immutable metadata from an admitted third-party descriptor. This callback
    /// cannot grant capability authority: the owner is its exact admitted vine.
    /// Implementations may retain bounded metadata or record a local failure.
    fn introduced(&mut self, _owner: &mut I, _contact: capnp::any_pointer::Reader<'_>) {}
}
impl<I, F: FnMut(ImportId) -> I> ImportReceiver<I> for F {
    fn make_import(&mut self, id: ImportId) -> I {
        self(id)
    }
}

/// Borrow resources belonging to one incoming message. Array or slice storage
/// bounds the resource budget; a transport may truncate it all the way to zero.
/// No resource is consumed until its descriptor has been accepted. Unreferenced
/// resources remain in the caller's slots for disposal with the input message.
///
/// Delivery consumes even null/reflected descriptor attachments, ensuring a
/// later duplicate index cannot retarget the same resource. The callback should
/// preserve an import's first resource and discard subsequent deliveries.
pub struct Receiver<'a, R, Make, Attach> {
    resources: &'a mut [Option<R>],
    make: Make,
    attach: Attach,
}
impl<'a, R, Make, Attach> Receiver<'a, R, Make, Attach> {
    pub fn new<I>(resources: &'a mut [Option<R>], make: Make, attach: Attach) -> Self
    where
        Make: FnMut(ImportId) -> I,
        Attach: FnMut(&mut I, R),
    {
        Self {
            resources,
            make,
            attach,
        }
    }
}
impl<I, R, Make, Attach> ImportReceiver<I> for Receiver<'_, R, Make, Attach>
where
    Make: FnMut(ImportId) -> I,
    Attach: FnMut(&mut I, R),
{
    fn make_import(&mut self, id: ImportId) -> I {
        (self.make)(id)
    }
    fn attach(&mut self, owner: Option<&mut I>, index: u8) {
        // 255 means no attachment, even if a caller supplied a longer slice.
        if index == u8::MAX {
            return;
        }
        if let Some(resource) = self
            .resources
            .get_mut(usize::from(index))
            .and_then(Option::take)
        {
            if let Some(owner) = owner {
                (self.attach)(owner, resource);
            }
        }
    }
}

/// Dense, bounded attachment indices for one outgoing message. The bundle moves
/// into the same FIFO reservation as the frame. Missing resources or exhausted
/// transport capacity produce the protocol's absent index (255), never an index
/// for a different resource. Enabling alloc does not change the inline policy.
pub struct Sender<R, const N: usize> {
    resources: crate::buffer::Buffer<Option<R>, N>,
    capacity: crate::buffer::Capacity,
    length: usize,
}
impl<R, const N: usize> Default for Sender<R, N> {
    fn default() -> Self {
        Self::new(crate::buffer::Capacity::Inline)
    }
}
impl<R, const N: usize> Sender<R, N> {
    pub fn new(capacity: crate::buffer::Capacity) -> Self {
        Self {
            resources: crate::buffer::Buffer::Inline(core::array::from_fn(|_| None)),
            capacity,
            length: 0,
        }
    }
    pub fn push(&mut self, resource: Option<R>) -> u8 {
        let Some(resource) = resource else {
            return u8::MAX;
        };
        // 255 is the absent sentinel. Unix transports may impose a smaller
        // bound (253) through capacity, independently of the capability count.
        if self.length == usize::from(u8::MAX)
            || !self
                .resources
                .ensure(self.capacity, self.length + 1, || None)
        {
            return u8::MAX;
        }
        let index = self.length as u8;
        self.resources[self.length] = Some(resource);
        self.length += 1;
        index
    }
    pub fn len(&self) -> usize {
        self.length
    }
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &R> {
        self.resources[..self.length]
            .iter()
            .map(|item| item.as_ref().expect("dense attachments"))
    }
}

pub(crate) fn resource<'id, I, P: Copy, S, R, const PATH: usize>(
    route: &crate::clients::Route<'id, P, S, PATH>,
    imports: &capntproto_rpc_engine::imports::Imports<'id, '_, I>,
    imported: &mut impl FnMut(&I) -> Option<R>,
    local: &mut impl FnMut(&S) -> Option<R>,
) -> Option<R> {
    use crate::clients::{Endpoint, Route};
    match route {
        Route::Capability(Endpoint::Imported(remote)) => imported(imports.get(remote.reference())),
        Route::Capability(Endpoint::Local(server)) | Route::Vine(server) => local(server),
        Route::Capability(Endpoint::Broken(_) | Endpoint::Failed { .. }) | Route::Question(_) => {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;
    struct Owner<'a>(&'a Cell<usize>);
    impl Drop for Owner<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    #[test]
    fn bounded_sender_truncates_without_allocating_or_retargeting_indices() {
        let usage = allocation_counter::measure(|| {
            let dropped = Cell::new(0);
            let mut sender = Sender::<_, 256>::default();
            assert_eq!(sender.push(None), 255);
            for n in 0..255 {
                assert_eq!(sender.push(Some(Owner(&dropped))), n);
            }
            assert_eq!(sender.push(Some(Owner(&dropped))), 255);
            assert_eq!(sender.len(), 255);
            assert_eq!(sender.iter().len(), 255);
            assert_eq!(dropped.get(), 1);
            drop(sender);
            assert_eq!(dropped.get(), 256);
            let mut empty = Sender::<_, 0>::default();
            assert_eq!(empty.push(Some(Owner(&dropped))), 255);
            assert!(empty.is_empty());
            assert_eq!(dropped.get(), 257);
        });
        assert_eq!(usage.count_total, 0);
    }
    #[cfg(feature = "alloc")]
    #[test]
    fn sender_growth_and_small_explicit_limits_preserve_owners() {
        for inline in [false, true] {
            let dropped = Cell::new(0);
            if inline {
                let mut sender =
                    Sender::<_, 4>::new(crate::buffer::Capacity::Growing { maximum: 1 });
                assert_eq!(sender.push(Some(Owner(&dropped))), 0);
                assert_eq!(sender.push(Some(Owner(&dropped))), 255);
                assert_eq!(sender.iter().len(), 1);
            } else {
                let mut sender =
                    Sender::<_, 0>::new(crate::buffer::Capacity::Growing { maximum: 1 });
                assert_eq!(sender.push(Some(Owner(&dropped))), 0);
                assert_eq!(sender.push(Some(Owner(&dropped))), 255);
                assert_eq!(sender.iter().len(), 1);
            }
            assert_eq!(dropped.get(), 2);
        }
    }
}
