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
pub(super) struct Heap<'i, 'e, 'a, I> {
    imports: alloc::vec::Vec<imports::ReceiveSlot<'i, I>>,
    exports: alloc::vec::Vec<exports::LeaseSlot<'e>>,
    answers: alloc::vec::Vec<answers::PipelineSlot<'a>>,
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
            Self::Heap(heap) => &heap.imports,
        }
    }
    pub(super) fn exports(&self) -> &[exports::LeaseSlot<'e>] {
        match self {
            Self::Inline { exports, .. } => exports,
            #[cfg(feature = "alloc")]
            Self::Heap(heap) => &heap.exports,
        }
    }
    pub(super) fn answers(&self) -> &[answers::PipelineSlot<'a>] {
        match self {
            Self::Inline { answers, .. } => answers,
            #[cfg(feature = "alloc")]
            Self::Heap(heap) => &heap.answers,
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
            Self::Heap(heap) => (
                heap.imports.as_mut_slice(),
                heap.exports.as_mut_slice(),
                heap.answers.as_mut_slice(),
            ),
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
                    let mut heap = Heap {
                        imports: alloc::vec::Vec::new(),
                        exports: alloc::vec::Vec::new(),
                        answers: alloc::vec::Vec::new(),
                    };
                    heap.reserve(count)?;
                    for (old, new) in imports.iter_mut().zip(&mut heap.imports) {
                        core::mem::swap(old, new);
                    }
                    for (old, new) in exports.iter_mut().zip(&mut heap.exports) {
                        core::mem::swap(old, new);
                    }
                    for (old, new) in answers.iter_mut().zip(&mut heap.answers) {
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
impl<I> Heap<'_, '_, '_, I> {
    fn reserve(&mut self, count: usize) -> Result<(), Error> {
        self.imports
            .try_reserve_exact(count - self.imports.len())
            .map_err(|_| Error::Capacity)?;
        self.exports
            .try_reserve_exact(count - self.exports.len())
            .map_err(|_| Error::Capacity)?;
        self.answers
            .try_reserve_exact(count - self.answers.len())
            .map_err(|_| Error::Capacity)?;
        self.imports.resize_with(count, imports::ReceiveSlot::none);
        self.exports.resize_with(count, exports::LeaseSlot::none);
        self.answers.resize_with(count, answers::PipelineSlot::none);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
