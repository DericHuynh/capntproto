//! Heap-owned endpoints over the same reply transitions as caller-owned slots.
//! This optional adapter is single-threaded and needs `alloc`, but not `std`.
//! Values use normal Rust destruction after abandoned endpoints are gone.
//! Convert the sender into the scoped Sender and track it when values need
//! explicit RPC receipt recovery; this moves ownership without allocating.
use super::*;
use alloc::rc::Rc;

/// One allocation, no executor and no implicit background task. Neither endpoint
/// can be cloned or reused for another reply.
pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let mut slot = Slot::new();
    slot.endpoints.set(2);
    slot.state.get_mut().phase = Phase::Pending;
    let slot = Rc::new(slot);
    (
        Sender {
            slot: Some(slot.clone()),
        },
        Receiver { slot: Some(slot) },
    )
}

#[must_use = "send once or drop to report cancellation"]
pub struct Sender<T> {
    slot: Option<Rc<Slot<T>>>,
}
impl<T> Sender<T> {
    pub fn is_closed(&self) -> bool {
        observed(&self.slot.as_ref().unwrap().state.borrow().phase)
    }
    /// A canceled receiver returns the original value without destroying it.
    pub fn send(self, value: T) -> Result<(), T> {
        send(self.slot.as_ref().unwrap(), value)
    }
}
impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        if let Some(slot) = self.slot.take() {
            cancel_pending(&slot, false);
        }
    }
}

#[must_use = "poll the reply or drop to cancel interest"]
pub struct Receiver<T> {
    slot: Option<Rc<Slot<T>>>,
}
impl<T> Future for Receiver<T> {
    type Output = Result<T, Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        poll_receiver(self.slot.as_ref().unwrap(), cx)
    }
}
impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        if let Some(slot) = self.slot.take() {
            drop_receiver(&slot);
        }
    }
}

/// Transfer the unique producer into the driver's tracked completion interface.
/// The contained value keeps its original lifetimes; no static lifetime is
/// required and the conversion performs no allocation or wake callback.
impl<'a, T> From<Sender<T>> for super::Sender<'a, T> {
    fn from(mut sender: Sender<T>) -> Self {
        Self {
            slot: Some(SlotRef::Shared(sender.slot.take().unwrap())),
        }
    }
}
/// Transfer the receiver without consuming, canceling or duplicating its reply.
impl<'a, T> From<Receiver<T>> for super::Receiver<'a, T> {
    fn from(mut receiver: Receiver<T>) -> Self {
        Self {
            slot: SlotRef::Shared(receiver.slot.take().unwrap()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::sync::Arc;
    use core::sync::atomic::{AtomicUsize, Ordering};
    struct Wakes(AtomicUsize);
    impl alloc::task::Wake for Wakes {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    struct Value(Rc<Cell<usize>>);
    impl Drop for Value {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn shared_completion_wakes_once_and_transfers_value() {
        let wakes = Arc::new(Wakes(AtomicUsize::new(0)));
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        let drops = Rc::new(Cell::new(0));
        let (sender, mut receiver) = channel();
        assert!(Pin::new(&mut receiver).poll(&mut cx).is_pending());
        assert!(Pin::new(&mut receiver).poll(&mut cx).is_pending());
        assert!(sender.send(Value(drops.clone())).is_ok());
        assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
        let Poll::Ready(Ok(value)) = Pin::new(&mut receiver).poll(&mut cx) else {
            panic!("reply")
        };
        assert_eq!(drops.get(), 0);
        assert!(matches!(
            Pin::new(&mut receiver).poll(&mut cx),
            Poll::Ready(Err(Error::Consumed))
        ));
        drop(receiver);
        assert_eq!(drops.get(), 0);
        drop(value);
        assert_eq!(drops.get(), 1);
    }
    #[test]
    fn shared_completion_cancellation_preserves_rejected_value_and_drops_abandoned_reply() {
        let drops = Rc::new(Cell::new(0));
        let (sender, receiver) = channel();
        drop(receiver);
        assert!(sender.is_closed());
        let value = sender.send(Value(drops.clone())).err().unwrap();
        assert_eq!(drops.get(), 0);
        drop(value);
        let (sender, receiver) = channel();
        assert!(sender.send(Value(drops.clone())).is_ok());
        assert_eq!(drops.get(), 1);
        drop(receiver);
        assert_eq!(drops.get(), 2);
    }
    #[test]
    fn shared_completion_sender_drop_wakes_pending_receiver() {
        let wakes = Arc::new(Wakes(AtomicUsize::new(0)));
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        let (sender, mut receiver) = channel::<()>();
        assert!(Pin::new(&mut receiver).poll(&mut cx).is_pending());
        drop(sender);
        assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
        assert!(matches!(
            Pin::new(&mut receiver).poll(&mut cx),
            Poll::Ready(Err(Error::Canceled))
        ));
    }

    #[test]
    fn converted_shared_endpoints_preserve_wakes_and_nonstatic_owners() {
        // The owned slot must not impose 'static on values with scoped receipts.
        struct ScopedValue<'a>(&'a Cell<usize>);
        impl Drop for ScopedValue<'_> {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        for mode in 0..5 {
            let drops = Cell::new(0);
            let wakes = Arc::new(Wakes(AtomicUsize::new(0)));
            let waker = Waker::from(wakes.clone());
            let mut cx = Context::from_waker(&waker);
            let (sender, mut receiver) = channel();
            let weak = Rc::downgrade(sender.slot.as_ref().unwrap());
            assert!(Pin::new(&mut receiver).poll(&mut cx).is_pending());
            let mut endpoints = None;
            let measured = allocation_counter::measure(|| {
                let sender: super::super::Sender<'_, ScopedValue<'_>> = sender.into();
                let receiver: super::super::Receiver<'_, ScopedValue<'_>> = receiver.into();
                endpoints = Some((sender.track(), receiver));
            });
            assert_eq!(measured.count_total, 0);
            let (mut delivery, mut receiver) = endpoints.take().unwrap();
            assert_eq!(wakes.0.load(Ordering::SeqCst), 0, "conversion is silent");
            assert!(delivery.poll_observed(&mut cx).is_pending());
            if mode == 0 {
                drop(receiver);
                assert!(delivery.is_observed());
                let value = delivery.try_send(ScopedValue(&drops)).err().unwrap();
                assert_eq!(drops.get(), 0);
                drop(value);
                assert!(delivery.cancel().is_none());
            } else {
                assert!(delivery.try_send(ScopedValue(&drops)).is_ok());
                assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
                assert!(delivery.take_abandoned().is_none());
                match mode {
                    1 => {
                        let value = receiver.try_take().unwrap().unwrap();
                        assert!(delivery.is_observed());
                        assert!(delivery.cancel().is_none());
                        assert_eq!(drops.get(), 0);
                        drop(value);
                        drop(receiver);
                    }
                    2 => {
                        drop(receiver);
                        assert_eq!(drops.get(), 0, "observer retains abandoned receipts");
                        let value = delivery.take_abandoned().unwrap();
                        assert!(delivery.take_abandoned().is_none());
                        drop(value);
                    }
                    3 => {
                        let value = delivery.cancel().unwrap();
                        assert!(matches!(receiver.try_take(), Err(Error::Canceled)));
                        assert!(delivery.cancel().is_none());
                        drop(value);
                        drop(receiver);
                    }
                    4 => {
                        // Ordinary destruction remains the terminal fallback.
                        drop(receiver);
                        assert_eq!(drops.get(), 0);
                    }
                    _ => unreachable!(),
                }
            }
            drop(delivery);
            assert_eq!(drops.get(), 1);
            assert!(weak.upgrade().is_none(), "no leaked completion allocation");
        }
    }
}
