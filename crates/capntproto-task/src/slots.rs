//! Structural pin projection for caller-owned future slots. No pointers into
//! movable storage are retained. Safe callers pin the complete storage first.
use core::pin::Pin;

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
