//! Fixed and optional growing slot storage behind the same transitions.
//! Relocation changes no slot indices or generations. Borrowed storage never
//! allocates, including when the crate's alloc feature is enabled.
use core::ops::{Deref, DerefMut};
#[cfg(any(feature = "alloc", kani))]
mod limits;
#[cfg(any(feature = "alloc", kani))]
pub(crate) use limits::growth_target;

/// Slot backing shared by the engine and its protocol adapters. Growing backing
/// preserves indices; views borrow the provider and prevent relocation.
pub struct Storage<'s, T> {
    backing: Backing<'s, T>,
}
enum Backing<'s, T> {
    Borrowed(&'s mut [T]),
    #[cfg(feature = "alloc")]
    Growing {
        slots: alloc::vec::Vec<T>,
        maximum: usize,
    },
}

impl<T> Deref for Storage<'_, T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        match &self.backing {
            Backing::Borrowed(slots) => slots,
            #[cfg(feature = "alloc")]
            Backing::Growing { slots, .. } => slots,
        }
    }
}
impl<T> DerefMut for Storage<'_, T> {
    fn deref_mut(&mut self) -> &mut [T] {
        match &mut self.backing {
            Backing::Borrowed(slots) => slots,
            #[cfg(feature = "alloc")]
            Backing::Growing { slots, .. } => slots,
        }
    }
}
impl<'s, T> Storage<'s, T> {
    pub fn borrowed(slots: &'s mut [T]) -> Self {
        Self {
            backing: Backing::Borrowed(slots),
        }
    }
    /// Configured slot bound, independent of currently allocated capacity.
    pub fn maximum(&self) -> usize {
        match &self.backing {
            Backing::Borrowed(slots) => slots.len(),
            #[cfg(feature = "alloc")]
            Backing::Growing { maximum, .. } => *maximum,
        }
    }
    /// Reserve a reusable slot, retaining only empty capacity on later rejection.
    pub fn vacant_mut(
        &mut self,
        reusable: impl FnMut(usize, &T) -> bool,
        empty: impl FnMut() -> T,
    ) -> Option<&mut T> {
        let index = self.vacant(reusable, empty)?;
        Some(&mut self[index])
    }
    pub fn vacant(
        &mut self,
        mut reusable: impl FnMut(usize, &T) -> bool,
        empty: impl FnMut() -> T,
    ) -> Option<usize> {
        if let Some(index) = self
            .iter()
            .enumerate()
            .position(|(i, value)| reusable(i, value))
        {
            return Some(index);
        }
        let index = self.len();
        self.grow(1, empty).then_some(index)
    }
    #[cfg(feature = "alloc")]
    pub fn growing(initial: usize, maximum: usize, empty: impl FnMut() -> T) -> Option<Self> {
        if initial > maximum {
            return None;
        }
        let mut slots = alloc::vec::Vec::new();
        slots.try_reserve_exact(initial).ok()?;
        slots.resize_with(initial, empty);
        Some(Self {
            backing: Backing::Growing { slots, maximum },
        })
    }
    /// Add at least `additional` slots or leave every logical slot unchanged.
    /// Only internal empty-slot constructors may be passed as `empty`.
    pub fn grow(&mut self, additional: usize, empty: impl FnMut() -> T) -> bool {
        match &mut self.backing {
            Backing::Borrowed(_) => {
                let _ = (additional, empty);
                false
            }
            #[cfg(feature = "alloc")]
            Backing::Growing { slots, maximum } => {
                let Some(target) = growth_target(slots.len(), additional, *maximum) else {
                    return false;
                };
                if slots.try_reserve_exact(target - slots.len()).is_err() {
                    return false;
                }
                slots.resize_with(target, empty);
                true
            }
        }
    }
}

#[cfg(kani)]
mod proofs {
    #[kani::proof]
    fn growing_capacity_preserves_full_width_bounds() {
        let current: usize = kani::any();
        let additional: usize = kani::any();
        let maximum: usize = kani::any();
        match super::growth_target(current, additional, maximum) {
            Some(next) => {
                kani::assert(
                    next >= current,
                    "growth preserves every existing slot index",
                );
                kani::assert(
                    next - current >= additional,
                    "growth admits the complete requested reservation",
                );
                kani::assert(
                    next <= maximum,
                    "growth cannot exceed its configured maximum",
                );
            }
            None => {
                kani::assert(
                    current > maximum || additional > maximum - current,
                    "growth rejects only impossible reservations",
                );
            }
        }
    }
}
