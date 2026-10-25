use super::*;
use crate::completion::{self, direct};
use core::{
    cell::{Cell, Ref, RefCell, RefMut},
    future::{Future, Ready},
    pin::Pin,
    task::{Context, Poll, Waker},
};
struct Resource<'a>(&'a Cell<usize>);
impl Drop for Resource<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}
type Data<'a> = Reference<Resource<'a>, u32, Resource<'a>, Ready<&'a [u8]>, Resource<'a>>;
struct Slot<'a>(RefCell<Data<'a>>);
impl<'a> direct::Owner<Ready<&'a [u8]>> for Slot<'a> {
    fn reply(&self) -> Ref<'_, direct::Reply<Ready<&'a [u8]>>> {
        Ref::map(self.0.borrow(), Reference::reply)
    }
    fn reply_mut(&self) -> RefMut<'_, direct::Reply<Ready<&'a [u8]>>> {
        RefMut::map(self.0.borrow_mut(), Reference::reply_mut)
    }
}

#[test]
fn detaching_context_keeps_pending_reply_producer_pipeline_and_retained_frame_independent() {
    let measured = allocation_counter::measure(|| {
        let bytes = [17u8; 16];
        let context_drops = Cell::new(0);
        let producer_drops = Cell::new(0);
        let pipeline_drops = Cell::new(0);
        let slot = Slot(RefCell::new(Reference::new(
            Some(Resource(&context_drops)),
            37,
            Some(Resource(&producer_drops)),
        )));
        let previous = slot
            .0
            .borrow_mut()
            .replace_pipeline(Resource(&pipeline_drops));
        assert!(previous.is_none());
        let previous = slot
            .0
            .borrow_mut()
            .replace_pipeline(Resource(&pipeline_drops))
            .unwrap();
        assert_eq!(pipeline_drops.get(), 0);
        drop(previous);
        assert_eq!(pipeline_drops.get(), 1);
        let mut receiver = direct::Receiver::new(&slot);
        let mut cx = Context::from_waker(Waker::noop());
        assert!(Pin::new(&mut receiver).poll(&mut cx).is_pending());
        let context = slot.0.borrow_mut().take_context().unwrap();
        assert_eq!(context_drops.get(), 0);
        drop(context);
        assert_eq!(context_drops.get(), 1);
        assert!(slot.0.borrow().context().is_none());
        assert_eq!(*slot.0.borrow().id(), 37);
        assert!(slot.0.borrow().pipeline().is_some());
        let producer = slot.0.borrow_mut().take_producer().unwrap();
        assert!(slot.0.borrow_mut().take_producer().is_none());
        assert_eq!(producer_drops.get(), 0);
        assert!(direct::deliver(&slot, core::future::ready(&bytes[..])).is_none());
        drop(producer);
        assert_eq!(producer_drops.get(), 1);
        let Poll::Ready(response) = Pin::new(&mut receiver).poll(&mut cx) else {
            panic!("reply lost after detaching context")
        };
        drop(receiver);
        drop(slot);
        assert_eq!(pipeline_drops.get(), 2);
        assert_eq!(response, &[17u8; 16]);
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn dropping_unfulfilled_caller_record_cancels_its_borrowed_external_producer() {
    let measured = allocation_counter::measure(|| {
        let mut completion = completion::Slot::<u32>::new();
        let (producer, mut receiver) = completion.pair().unwrap();
        let mut record =
            Reference::<u32, u64, _, Ready<u32>, u32>::new(Some(7), 37, Some(producer));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(Pin::new(&mut receiver).poll(&mut cx).is_pending());
        assert_eq!(record.take_context(), Some(7));
        drop(record);
        assert_eq!(
            Pin::new(&mut receiver).poll(&mut cx),
            Poll::Ready(Err(completion::Error::Canceled))
        );
        drop(receiver);
        assert!(completion.pair().is_ok());
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}
