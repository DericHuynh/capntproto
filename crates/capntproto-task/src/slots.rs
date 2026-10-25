//! Structural pin projection for caller-owned future slots. No pointers into
//! movable storage are retained. Safe callers pin the complete storage first.
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// Exclusive ownership of a pinned task value in caller storage. The handle
/// itself can move through admission and the scheduler's direct first poll;
/// its future cannot move. Dropping the handle destroys the future immediately.
pub struct Owned<'a, F> {
    value: Pin<&'a mut Option<F>>,
}

trait ErasedTask {
    type Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output>;
    fn retire(self: Pin<&mut Self>);
}
impl<F: Future> ErasedTask for Option<F> {
    type Output = F::Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.as_pin_mut().expect("owned task").poll(cx)
    }
    fn retire(mut self: Pin<&mut Self>) {
        self.set(None);
    }
}

/// A movable, allocator-free owner of a heterogeneous pinned future. The
/// normal Rust trait-object vtable provides polling and in-place retirement;
/// the two-word owner contains no heap allocation or raw ownership pointer.
pub struct Erased<'a, O> {
    value: Pin<&'a mut dyn ErasedTask<Output = O>>,
}
impl<O> Future for Erased<'_, O> {
    type Output = O;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<O> {
        self.value.as_mut().poll(cx)
    }
}
impl<O> Drop for Erased<'_, O> {
    fn drop(&mut self) {
        self.value.as_mut().retire();
    }
}
impl<F: Future> Future for Owned<'_, F> {
    type Output = F::Output;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.value
            .as_mut()
            .as_pin_mut()
            .expect("owned task")
            .poll(cx)
    }
}
impl<F> Drop for Owned<'_, F> {
    fn drop(&mut self) {
        self.value.as_mut().set(None);
    }
}

/// A distinct exclusive place from a structurally pinned array. Inserting a
/// value transfers destruction responsibility to the returned movable owner.
pub struct Place<'a, F> {
    value: Pin<&'a mut Option<F>>,
}
impl<'a, F> Place<'a, F> {
    pub fn insert(mut self, future: F) -> Result<Owned<'a, F>, F> {
        if self.value.is_some() {
            return Err(future);
        }
        self.value.as_mut().set(Some(future));
        Ok(Owned { value: self.value })
    }
    /// Erase the future's concrete type while retaining its exclusive pinned
    /// storage loan and immediate destruction responsibility.
    ///
    /// ```compile_fail,E0597
    /// use capntproto_task::Slots;
    /// let escaped;
    /// {
    ///     let mut slots = core::pin::pin!(Slots::<_, 1>::new());
    ///     let [place] = slots.as_mut().places();
    ///     escaped = place.insert_erased(async {}).ok().unwrap();
    /// }
    /// drop(escaped);
    /// ```
    pub fn insert_erased(mut self, future: F) -> Result<Erased<'a, F::Output>, F>
    where
        F: Future + 'a,
    {
        if self.value.is_some() {
            return Err(future);
        }
        self.value.as_mut().set(Some(future));
        Ok(Erased { value: self.value })
    }
}

pub struct Slots<F, const N: usize> {
    values: [Option<F>; N],
}
impl<F, const N: usize> Slots<F, N> {
    pub const fn new() -> Self {
        Self {
            values: [const { None }; N],
        }
    }
    pub fn is_empty(&self) -> bool {
        self.values.iter().all(Option::is_none)
    }
    /// Split the array into disjoint pinned ownership places. No stored future
    /// is moved, and no second mutable access to the array can coexist.
    pub fn places(self: Pin<&mut Self>) -> [Place<'_, F>; N] {
        // SAFETY: only disjoint fields are projected. Each field remains
        // structurally pinned; the private Place/Owned APIs use Pin::set for
        // replacement and never expose an unpinned reference to a stored F.
        let values = unsafe { &mut self.get_unchecked_mut().values };
        values.each_mut().map(|value| Place {
            // SAFETY: the complete array is pinned for this exclusive loan.
            value: unsafe { Pin::new_unchecked(value) },
        })
    }
    pub fn insert(self: Pin<&mut Self>, index: usize, value: F) -> Result<(), F> {
        if self.as_ref().get_ref().values[index].is_some() {
            return Err(value);
        }
        // SAFETY: this field is structurally pinned. No API moves a stored F;
        // replacement uses Pin::set, which drops old data in place first.
        let mut place = unsafe { self.map_unchecked_mut(|slots| &mut slots.values[index]) };
        place.set(Some(value));
        Ok(())
    }
    pub fn get(self: Pin<&mut Self>, index: usize) -> Option<Pin<&mut F>> {
        // SAFETY: values remain at these addresses until they are dropped.
        // The returned mutable pin is bounded by this exclusive storage loan.
        unsafe { self.map_unchecked_mut(|slots| &mut slots.values[index]) }.as_pin_mut()
    }
    pub fn clear(self: Pin<&mut Self>, index: usize) {
        // SAFETY: the projected Option is structurally pinned and Pin::set
        // destroys its existing value before reusing its memory.
        unsafe { self.map_unchecked_mut(|slots| &mut slots.values[index]) }.set(None);
    }
    pub fn clear_all(self: Pin<&mut Self>) {
        // SAFETY: the array is structurally pinned. Standard array destruction
        // also cleans remaining elements if one user destructor unwinds.
        unsafe { self.map_unchecked_mut(|slots| &mut slots.values) }.set([const { None }; N]);
    }
}
impl<F, const N: usize> Default for Slots<F, N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use core::{cell::Cell, marker::PhantomPinned};
    #[test]
    fn heterogeneous_async_owners_use_two_words_and_release_borrows_on_cancel() {
        struct Guard<'a>(&'a Cell<usize>);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        let drops = Cell::new(0);
        let measured = allocation_counter::measure(|| {
            let guard = Guard(&drops);
            let suspended = async move {
                let _guard = guard;
                core::future::pending::<()>().await;
                42u32
            };
            let immediate = core::future::ready(42u32);
            let mut first = core::pin::pin!(Slots::<_, 1>::new());
            let mut second = core::pin::pin!(Slots::<_, 1>::new());
            let [a] = first.as_mut().places();
            let [b] = second.as_mut().places();
            {
                let mut erased = [
                    a.insert_erased(suspended).ok().unwrap(),
                    b.insert_erased(immediate).ok().unwrap(),
                ];
                let mut cx = Context::from_waker(core::task::Waker::noop());
                assert!(Pin::new(&mut erased[0]).poll(&mut cx).is_pending());
                assert_eq!(Pin::new(&mut erased[1]).poll(&mut cx), Poll::Ready(42));
                let [a, b] = erased;
                drop(a);
                assert_eq!(drops.get(), 1);
                drop(b);
            }
            assert!(first.is_empty() && second.is_empty());
        });
        assert_eq!(
            core::mem::size_of::<Erased<'_, u32>>(),
            2 * core::mem::size_of::<usize>()
        );
        assert_eq!(measured.count_total, 0);
        assert_eq!(measured.bytes_total, 0);
    }
    #[test]
    fn movable_owners_pin_borrowing_futures_and_destroy_them_before_storage_ends() {
        struct Task<'a> {
            item: Item<'a>,
            ready: &'a Cell<bool>,
        }
        impl Future for Task<'_> {
            type Output = ();
            fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
                let address = core::ptr::from_ref(&self.item).addr();
                let previous = self.item.address.replace(address);
                assert!(previous == 0 || previous == address);
                if self.ready.get() {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            }
        }
        let ready = Cell::new(false);
        let drops = Cell::new(0);
        let measured = allocation_counter::measure(|| {
            let mut slots = core::pin::pin!(Slots::<Task<'_>, 2>::new());
            let [first, second] = slots.as_mut().places();
            let make = || Task {
                item: Item {
                    address: Cell::new(0),
                    drops: &drops,
                    _pin: PhantomPinned,
                },
                ready: &ready,
            };
            let mut first = first.insert(make()).ok().unwrap();
            let mut second = second.insert(make()).ok().unwrap();
            let mut cx = Context::from_waker(core::task::Waker::noop());
            assert!(Pin::new(&mut first).poll(&mut cx).is_pending());
            assert!(Pin::new(&mut second).poll(&mut cx).is_pending());
            let mut moved = first;
            ready.set(true);
            assert!(Pin::new(&mut moved).poll(&mut cx).is_ready());
            drop(moved);
            assert_eq!(drops.get(), 1);
            drop(second);
            assert_eq!(drops.get(), 2);
            assert!(slots.is_empty());
        });
        assert_eq!(measured.count_total, 0);
        assert_eq!(measured.bytes_total, 0);
    }
    struct Item<'a> {
        address: Cell<usize>,
        drops: &'a Cell<usize>,
        _pin: PhantomPinned,
    }
    impl Drop for Item<'_> {
        fn drop(&mut self) {
            assert_eq!(self.address.get(), core::ptr::from_ref(self).addr());
            self.drops.set(self.drops.get() + 1);
        }
    }
    #[test]
    fn pinned_slots_reuse_storage_and_drop_values_in_place_without_allocating() {
        let drops = Cell::new(0);
        let measured = allocation_counter::measure(|| {
            let mut slots = core::pin::pin!(Slots::<Item<'_>, 1>::new());
            for _ in 0..4 {
                assert!(slots
                    .as_mut()
                    .insert(
                        0,
                        Item {
                            address: Cell::new(0),
                            drops: &drops,
                            _pin: PhantomPinned
                        }
                    )
                    .is_ok());
                let item = slots.as_mut().get(0).unwrap();
                item.address
                    .set(core::ptr::from_ref(item.as_ref().get_ref()).addr());
                slots.as_mut().clear(0);
                assert!(slots.is_empty());
            }
        });
        assert_eq!(drops.get(), 4);
        assert_eq!(measured.count_total, 0);
        assert_eq!(measured.bytes_total, 0);
    }
    #[test]
    fn clear_all_remains_empty_and_drops_every_value_after_a_destructor_panics() {
        struct Item<'a> {
            index: usize,
            drops: &'a [Cell<usize>; 3],
        }
        impl Drop for Item<'_> {
            fn drop(&mut self) {
                self.drops[self.index].set(self.drops[self.index].get() + 1);
                if self.index == 1 {
                    panic!("test destructor");
                }
            }
        }
        let drops = [const { Cell::new(0) }; 3];
        let mut slots = core::pin::pin!(Slots::<Item<'_>, 3>::new());
        for index in 0..3 {
            assert!(slots
                .as_mut()
                .insert(
                    index,
                    Item {
                        index,
                        drops: &drops
                    }
                )
                .is_ok());
        }
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| slots.as_mut().clear_all()))
                .is_err()
        );
        assert!(slots.is_empty());
        assert!(drops.iter().all(|n| n.get() == 1));
    }
}
