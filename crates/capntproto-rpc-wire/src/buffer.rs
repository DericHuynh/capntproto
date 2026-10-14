//! Explicit storage policy for per-message data. Enabling `alloc` does not
//! change the behavior of an inline provider, including a zero-sized provider.
use core::ops::{Deref, DerefMut};

/// Storage for one message's capability receipts or pipeline transform.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Capacity {
    /// Use only the array capacity in the containing type.
    #[default]
    Inline,
    /// Allocate on demand, up to this limit. Even values which fit inline obey
    /// the limit; the array size is an optimization, not a second budget.
    #[cfg(feature = "alloc")]
    Growing { maximum: usize },
}
impl Capacity {
    pub(crate) const fn maximum<const N: usize>(self) -> usize {
        match self {
            Self::Inline => N,
            #[cfg(feature = "alloc")]
            Self::Growing { maximum } => maximum,
        }
    }
    pub(crate) fn buffer<T, const N: usize>(
        self,
        length: usize,
        mut fill: impl FnMut() -> T,
    ) -> Option<Buffer<T, N>> {
        if length > self.maximum::<N>() {
            return None;
        }
        if length <= N {
            return Some(Buffer::Inline(core::array::from_fn(|_| fill())));
        }
        #[cfg(feature = "alloc")]
        {
            let mut values = alloc::vec::Vec::new();
            values.try_reserve_exact(length).ok()?;
            values.resize_with(length, fill);
            Some(Buffer::Heap(alloc::boxed::Box::new(values)))
        }
        #[cfg(not(feature = "alloc"))]
        None
    }
    pub(crate) fn copy<T: Copy + Default, const N: usize>(
        self,
        values: &[T],
    ) -> Option<Buffer<T, N>> {
        let mut buffer = self.buffer(values.len(), T::default)?;
        buffer[..values.len()].copy_from_slice(values);
        Some(buffer)
    }
}

#[derive(Debug)]
pub(crate) enum Buffer<T, const N: usize> {
    Inline([T; N]),
    #[cfg(feature = "alloc")]
    // Most per-message buffers are empty (N = 0 in the hosted driver). Keep
    // the growing representation out of every queued owner in that case.
    // The fixed provider still owns its array and never allocates.
    // Box the metadata as well: zero-inline hosted owners stay one pointer
    // wide while the backing vector grows, reducing every scheduler move.
    #[allow(clippy::box_collection)]
    Heap(alloc::boxed::Box<alloc::vec::Vec<T>>),
}
impl<T, const N: usize> Buffer<T, N> {
    pub(crate) fn ensure(
        &mut self,
        policy: Capacity,
        length: usize,
        mut fill: impl FnMut() -> T,
    ) -> bool {
        if length > policy.maximum::<N>() {
            return false;
        }
        if length <= self.len() {
            return true;
        }
        #[cfg(feature = "alloc")]
        {
            match self {
                Self::Heap(values) => {
                    if values.try_reserve_exact(length - values.len()).is_err() {
                        return false;
                    }
                    values.resize_with(length, fill);
                }
                Self::Inline(values) => {
                    let Some(Self::Heap(mut next)) = policy.buffer::<T, N>(length, &mut fill)
                    else {
                        return false;
                    };
                    for (old, new) in values.iter_mut().zip(next.iter_mut()) {
                        core::mem::swap(old, new);
                    }
                    *self = Self::Heap(next);
                }
            }
            true
        }
        #[cfg(not(feature = "alloc"))]
        {
            let _ = &mut fill;
            false
        }
    }
}
impl<T, const N: usize> Deref for Buffer<T, N> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        match self {
            Self::Inline(values) => values,
            #[cfg(feature = "alloc")]
            Self::Heap(values) => values,
        }
    }
}
impl<T, const N: usize> DerefMut for Buffer<T, N> {
    fn deref_mut(&mut self) -> &mut [T] {
        match self {
            Self::Inline(values) => values,
            #[cfg(feature = "alloc")]
            Self::Heap(values) => values,
        }
    }
}
pub(crate) enum IntoIter<T, const N: usize> {
    Inline(core::array::IntoIter<T, N>),
    #[cfg(feature = "alloc")]
    Heap(alloc::vec::IntoIter<T>),
}
impl<T, const N: usize> Iterator for IntoIter<T, N> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        match self {
            Self::Inline(iter) => iter.next(),
            #[cfg(feature = "alloc")]
            Self::Heap(iter) => iter.next(),
        }
    }
}
impl<T, const N: usize> IntoIterator for Buffer<T, N> {
    type Item = T;
    type IntoIter = IntoIter<T, N>;
    fn into_iter(self) -> Self::IntoIter {
        match self {
            Self::Inline(values) => IntoIter::Inline(values.into_iter()),
            #[cfg(feature = "alloc")]
            Self::Heap(values) => IntoIter::Heap((*values).into_iter()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_limits_never_allocate_even_when_alloc_is_enabled() {
        let usage = allocation_counter::measure(|| {
            assert!(Capacity::Inline.buffer::<u64, 0>(1, || 0).is_none());
            let mut values = Capacity::Inline.buffer::<_, 2>(2, || 7u64).unwrap();
            assert!(!values.ensure(Capacity::Inline, 3, || 0));
            assert_eq!(&*values, &[7, 7]);
            assert!(Capacity::Inline.copy::<u16, 2>(&[1, 2, 3]).is_none());
        });
        assert_eq!(usage.count_total, 0);
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn growth_moves_owners_once_and_obeys_budget_below_inline_size() {
        use core::cell::Cell;
        struct Owner<'a>(&'a Cell<usize>);
        impl Drop for Owner<'_> {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        let drops = Cell::new(0);
        let policy = Capacity::Growing { maximum: 4 };
        let mut values = policy.buffer::<_, 2>(2, || None).unwrap();
        values[0] = Some(Owner(&drops));
        assert!(values.ensure(policy, 4, || None));
        assert_eq!(drops.get(), 0);
        assert!(!values.ensure(policy, 5, || None));
        assert_eq!(drops.get(), 0);
        drop(values);
        assert_eq!(drops.get(), 1);
        assert!(Capacity::Growing { maximum: 1 }
            .buffer::<u64, 8>(2, || 0)
            .is_none());
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn allocation_failure_keeps_original_buffer_and_owners() {
        let policy = Capacity::Growing {
            maximum: usize::MAX,
        };
        let mut values = policy.buffer::<_, 1>(1, || 19u64).unwrap();
        assert!(!values.ensure(policy, usize::MAX, || 0));
        assert_eq!(&*values, &[19]);
        assert!(policy.buffer::<u64, 0>(usize::MAX, || 0).is_none());
    }
}
