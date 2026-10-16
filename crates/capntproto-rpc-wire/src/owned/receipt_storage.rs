//! Keep the three receipt arrays behind one storage discriminator. In the
//! zero-inline-capacity hosted driver an empty receipt set occupies one word,
//! instead of moving three empty vector headers with every Call/Return.
use super::*;

pub(super) enum ReceiptStorage<'i, 'e, 'a, I, const N: usize> {
    Inline {
        imports: [imports::ReceiveSlot<'i, I>; N],
        exports: [exports::LeaseSlot<'e>; N],
        answers: [answers::PipelineSlot<'a>; N],
    },
    #[cfg(feature = "alloc")]
    Heap(alloc::boxed::Box<Heap<'i, 'e, 'a, I>>),
}
#[cfg(feature = "alloc")]
pub(super) enum Heap<'i, 'e, 'a, I> {
    // A single result capability is common. Keep its parallel receipts in
    // the existing box, avoiding three separate vector allocations.
    // The outer zero-inline empty representation remains one pointer wide.
    One {
        imports: [imports::ReceiveSlot<'i, I>; 1],
        exports: [exports::LeaseSlot<'e>; 1],
        answers: [answers::PipelineSlot<'a>; 1],
    },
    Many {
        imports: alloc::vec::Vec<imports::ReceiveSlot<'i, I>>,
        exports: alloc::vec::Vec<exports::LeaseSlot<'e>>,
        answers: alloc::vec::Vec<answers::PipelineSlot<'a>>,
    },
}
impl<'i, 'e, 'a, I, const N: usize> ReceiptStorage<'i, 'e, 'a, I, N> {
    pub(super) fn new() -> Self {
        Self::Inline {
            imports: core::array::from_fn(|_| imports::ReceiveSlot::none()),
            exports: core::array::from_fn(|_| exports::LeaseSlot::none()),
            answers: core::array::from_fn(|_| answers::PipelineSlot::none()),
        }
    }
    pub(super) fn imports(&self) -> &[imports::ReceiveSlot<'i, I>] {
        match self {
            Self::Inline { imports, .. } => imports,
            #[cfg(feature = "alloc")]
            Self::Heap(heap) => heap.imports(),
        }
    }
    pub(super) fn exports(&self) -> &[exports::LeaseSlot<'e>] {
        match self {
            Self::Inline { exports, .. } => exports,
            #[cfg(feature = "alloc")]
            Self::Heap(heap) => heap.exports(),
        }
    }
    pub(super) fn answers(&self) -> &[answers::PipelineSlot<'a>] {
        match self {
            Self::Inline { answers, .. } => answers,
            #[cfg(feature = "alloc")]
            Self::Heap(heap) => heap.answers(),
        }
    }
    pub(super) fn slots(&mut self) -> reception::full::Slots<'_, 'i, 'e, 'a, I> {
        let (imports, exports, answers) = match self {
            Self::Inline {
                imports,
                exports,
                answers,
            } => (
                imports.as_mut_slice(),
                exports.as_mut_slice(),
                answers.as_mut_slice(),
            ),
            #[cfg(feature = "alloc")]
            Self::Heap(heap) => return heap.slots(),
        };
        reception::full::Slots {
            imports,
            exports,
            answers,
        }
    }
    pub(super) fn reserve(&mut self, capacity: Capacity, count: usize) -> Result<(), Error> {
        if count > capacity.maximum::<N>() {
            return Err(Error::Capacity);
        }
        if count <= self.imports().len() {
            return Ok(());
        }
        #[cfg(feature = "alloc")]
        {
            // Grow all arrays before replacing the inline owner. On rejection
            // every existing receipt stays in its original slot.
            match self {
                Self::Heap(heap) => heap.reserve(count),
                Self::Inline {
                    imports,
                    exports,
                    answers,
                } => {
                    let mut heap = Heap::new(count)?;
                    let slots = heap.slots();
                    for (old, new) in imports.iter_mut().zip(slots.imports) {
                        core::mem::swap(old, new);
                    }
                    for (old, new) in exports.iter_mut().zip(slots.exports) {
                        core::mem::swap(old, new);
                    }
                    for (old, new) in answers.iter_mut().zip(slots.answers) {
                        core::mem::swap(old, new);
                    }
                    *self = Self::Heap(alloc::boxed::Box::new(heap));
                    Ok(())
                }
            }
        }
        #[cfg(not(feature = "alloc"))]
        Err(Error::Capacity)
    }
}
#[cfg(feature = "alloc")]
impl<'i, 'e, 'a, I> Heap<'i, 'e, 'a, I> {
    fn new(count: usize) -> Result<Self, Error> {
        if count == 1 {
            return Ok(Self::One {
                imports: core::array::from_fn(|_| imports::ReceiveSlot::none()),
                exports: core::array::from_fn(|_| exports::LeaseSlot::none()),
                answers: core::array::from_fn(|_| answers::PipelineSlot::none()),
            });
        }
        let mut heap = Self::Many {
            imports: alloc::vec::Vec::new(),
            exports: alloc::vec::Vec::new(),
            answers: alloc::vec::Vec::new(),
        };
        heap.reserve(count)?;
        Ok(heap)
    }
    fn imports(&self) -> &[imports::ReceiveSlot<'i, I>] {
        match self {
            Self::One { imports, .. } => imports,
            Self::Many { imports, .. } => imports,
        }
    }
    fn exports(&self) -> &[exports::LeaseSlot<'e>] {
        match self {
            Self::One { exports, .. } => exports,
            Self::Many { exports, .. } => exports,
        }
    }
    fn answers(&self) -> &[answers::PipelineSlot<'a>] {
        match self {
            Self::One { answers, .. } => answers,
            Self::Many { answers, .. } => answers,
        }
    }
    fn slots(&mut self) -> reception::full::Slots<'_, 'i, 'e, 'a, I> {
        let (imports, exports, answers) = match self {
            Self::One {
                imports,
                exports,
                answers,
            } => (
                imports.as_mut_slice(),
                exports.as_mut_slice(),
                answers.as_mut_slice(),
            ),
            Self::Many {
                imports,
                exports,
                answers,
            } => (
                imports.as_mut_slice(),
                exports.as_mut_slice(),
                answers.as_mut_slice(),
            ),
        };
        reception::full::Slots {
            imports,
            exports,
            answers,
        }
    }
    fn reserve(&mut self, count: usize) -> Result<(), Error> {
        if let Self::One { .. } = self {
            if count <= 1 {
                return Ok(());
            }
            // Complete every fallible allocation before moving any owner.
            let mut grown = Self::new(count)?;
            let old = self.slots();
            let new = grown.slots();
            for (old, new) in old.imports.iter_mut().zip(new.imports) {
                core::mem::swap(old, new);
            }
            for (old, new) in old.exports.iter_mut().zip(new.exports) {
                core::mem::swap(old, new);
            }
            for (old, new) in old.answers.iter_mut().zip(new.answers) {
                core::mem::swap(old, new);
            }
            *self = grown;
            return Ok(());
        }
        let Self::Many {
            imports,
            exports,
            answers,
        } = self
        else {
            unreachable!()
        };
        imports
            .try_reserve_exact(count - imports.len())
            .map_err(|_| Error::Capacity)?;
        exports
            .try_reserve_exact(count - exports.len())
            .map_err(|_| Error::Capacity)?;
        answers
            .try_reserve_exact(count - answers.len())
            .map_err(|_| Error::Capacity)?;
        imports.resize_with(count, imports::ReceiveSlot::none);
        exports.resize_with(count, exports::LeaseSlot::none);
        answers.resize_with(count, answers::PipelineSlot::none);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(feature = "alloc", not(miri)))]
    #[test]
    fn single_growing_receipt_uses_one_allocation_and_keeps_the_capacity_limit() {
        let mut storage = ReceiptStorage::<(), 0>::new();
        let policy = Capacity::Growing { maximum: 2 };
        let first = allocation_counter::measure(|| storage.reserve(policy, 1).unwrap());
        assert_eq!(first.count_total, 1);
        assert_eq!(storage.imports().len(), 1);
        let repeated = allocation_counter::measure(|| storage.reserve(policy, 1).unwrap());
        assert_eq!(repeated.count_total, 0);
        storage.reserve(policy, 2).unwrap();
        assert_eq!(storage.imports().len(), 2);
        assert_eq!(storage.reserve(policy, 3), Err(Error::Capacity));
        assert_eq!(storage.reserve(Capacity::Inline, 1), Err(Error::Capacity));
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn small_receipt_promotion_and_rejection_preserve_factory_owners() {
        use core::cell::Cell;
        struct Owner<'a>(&'a Cell<usize>);
        impl Drop for Owner<'_> {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        let drops = Cell::new(0);
        let mut storage = ReceiptStorage::<Owner<'_>, 0>::new();
        let small = Capacity::Growing { maximum: 2 };
        storage.reserve(small, 1).unwrap();
        let slots = storage.slots();
        slots.imports[0] = imports::ReceiveSlot::new(ImportId::from_wire(7), Owner(&drops));
        slots.exports[0] = exports::LeaseSlot::new(ExportId::from_wire(8));
        slots.answers[0] = answers::PipelineSlot::new(AnswerId::from_wire(9));
        assert_eq!(
            storage.reserve(Capacity::Growing { maximum: 1 }, 2),
            Err(Error::Capacity)
        );
        assert_eq!(storage.imports()[0].id(), Some(ImportId::from_wire(7)));
        assert!(storage.imports()[0].owner().is_some());
        assert_eq!(drops.get(), 0);
        storage.reserve(small, 2).unwrap();
        assert_eq!(storage.reserve(small, 3), Err(Error::Capacity));
        assert_eq!(drops.get(), 0);
        for count in [3, 5] {
            storage
                .reserve(Capacity::Growing { maximum: 5 }, count)
                .unwrap();
            assert_eq!(storage.imports().len(), count);
            assert_eq!(storage.imports()[0].id(), Some(ImportId::from_wire(7)));
            assert!(storage.imports()[0].owner().is_some());
            assert_eq!(storage.exports()[0].id(), Some(ExportId::from_wire(8)));
            assert_eq!(storage.answers()[0].id(), Some(AnswerId::from_wire(9)));
            assert_eq!(drops.get(), 0);
        }
        drop(storage);
        assert_eq!(drops.get(), 1);
    }

    #[test]
    fn inline_capacity_remains_fixed_with_allocation_enabled() {
        let mut slots = ReceiptStorage::<(), 1>::new();
        slots.reserve(Capacity::Inline, 1).unwrap();
        assert_eq!(slots.reserve(Capacity::Inline, 2), Err(Error::Capacity));
        assert_eq!(slots.imports().len(), 1);
        let mut empty = ReceiptStorage::<(), 0>::new();
        empty.reserve(Capacity::Inline, 0).unwrap();
        assert_eq!(empty.reserve(Capacity::Inline, 1), Err(Error::Capacity));
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn promotion_growth_and_rejection_preserve_every_receipt() {
        let mut storage = ReceiptStorage::<(), 1>::new();
        let slots = storage.slots();
        slots.imports[0] = imports::ReceiveSlot::alias(ImportId::from_wire(7));
        slots.exports[0] = exports::LeaseSlot::new(ExportId::from_wire(8));
        slots.answers[0] = answers::PipelineSlot::new(AnswerId::from_wire(9));
        let policy = Capacity::Growing { maximum: 4 };
        for count in [2, 4] {
            storage.reserve(policy, count).unwrap();
            assert_eq!(storage.imports().len(), count);
            assert_eq!(storage.exports().len(), count);
            assert_eq!(storage.answers().len(), count);
            assert_eq!(storage.imports()[0].id(), Some(ImportId::from_wire(7)));
            assert_eq!(storage.exports()[0].id(), Some(ExportId::from_wire(8)));
            assert_eq!(storage.answers()[0].id(), Some(AnswerId::from_wire(9)));
        }
        assert_eq!(storage.reserve(policy, 5), Err(Error::Capacity));
        assert_eq!(storage.imports()[0].id(), Some(ImportId::from_wire(7)));
        assert_eq!(storage.exports()[0].id(), Some(ExportId::from_wire(8)));
        assert_eq!(storage.answers()[0].id(), Some(AnswerId::from_wire(9)));
        assert_eq!(
            core::mem::size_of::<ReceiptStorage<'_, '_, '_, (), 0>>(),
            core::mem::size_of::<usize>()
        );
    }
}
