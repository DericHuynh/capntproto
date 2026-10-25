//! A standard Waker borrowing a statically retained target. This crate owns no
//! allocation or task pointer. Targets choose notification/scheduling policy.
#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]
use core::task::{RawWaker, RawWakerVTable, Waker};
mod slots;
pub use slots::{Erased, Owned, Place, Slots};

/// A shared target. Wakers may invoke it on any thread after their task ends.
pub trait Wake: Sync + 'static {
    fn wake_by_ref(&self);
}

/// A waker can be obtained only through a static borrow. Clones require no
/// allocator or reference count, and never destroy the target.
///
/// ```compile_fail,E0597
/// use capntproto_task::{Static, Wake};
/// struct Signal;
/// impl Wake for Signal { fn wake_by_ref(&self) {} }
/// let local = Static::new(Signal);
/// let escaped = local.waker();
/// drop(local);
/// escaped.wake();
/// ```
#[derive(Clone, Copy)]
pub struct Static<T: Wake> {
    target: T,
}
impl<T: Wake> Static<T> {
    pub const fn new(target: T) -> Self {
        Self { target }
    }
    const VTABLE: RawWakerVTable = RawWakerVTable::new(clone::<T>, wake::<T>, wake::<T>, discard);
    pub fn waker(&'static self) -> Waker {
        // SAFETY: data points to this exact Static<T> for its entire static
        // lifetime. The vtable restores that type, shares only a Sync target,
        // and never frees or moves it. Clone/drop retain no ownership debt.
        unsafe { Waker::new(core::ptr::from_ref(self).cast(), &Self::VTABLE) }
    }
}
unsafe fn clone<T: Wake>(data: *const ()) -> RawWaker {
    RawWaker::new(data, &Static::<T>::VTABLE)
}
unsafe fn wake<T: Wake>(data: *const ()) {
    // SAFETY: only Static<T>::waker installs this vtable. Its static borrow
    // keeps the pointed-to Static<T> valid, including after Waker cloning.
    let target = unsafe { &*data.cast::<Static<T>>() };
    target.target.wake_by_ref();
}
unsafe fn discard(_: *const ()) {}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use core::sync::atomic::{AtomicUsize, Ordering};
    struct Count(AtomicUsize);
    impl Wake for Count {
        fn wake_by_ref(&self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    #[test]
    fn construction_clone_wake_and_drop_allocate_nothing() {
        static TARGET: Static<Count> = Static::new(Count(AtomicUsize::new(0)));
        let measured = allocation_counter::measure(|| {
            let first = TARGET.waker();
            let second = first.clone();
            first.wake_by_ref();
            second.wake();
            drop(first);
        });
        assert_eq!(TARGET.target.0.load(Ordering::Relaxed), 2);
        assert_eq!(measured.count_total, 0);
        assert_eq!(measured.bytes_total, 0);
    }
    #[test]
    fn escaped_clone_remains_valid_on_another_thread() {
        static TARGET: Static<Count> = Static::new(Count(AtomicUsize::new(0)));
        let escaped = {
            let original = TARGET.waker();
            original.clone()
        };
        std::thread::spawn(move || {
            escaped.wake_by_ref();
            escaped.wake();
        })
        .join()
        .unwrap();
        assert_eq!(TARGET.target.0.load(Ordering::Relaxed), 2);
    }
    #[test]
    fn dropping_wakers_never_destroys_the_static_target() {
        static DROPS: AtomicUsize = AtomicUsize::new(0);
        struct Target;
        impl Wake for Target {
            fn wake_by_ref(&self) {
                DROPS.fetch_add(0, Ordering::Relaxed);
            }
        }
        impl Drop for Target {
            fn drop(&mut self) {
                DROPS.fetch_add(1, Ordering::Relaxed);
            }
        }
        static TARGET: Static<Target> = Static::new(Target);
        let first = TARGET.waker();
        drop(first.clone());
        drop(first);
        TARGET.waker().wake();
        assert_eq!(DROPS.load(Ordering::Relaxed), 0);
    }
}
