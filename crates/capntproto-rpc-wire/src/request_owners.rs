//! The legacy Request API transfers caller-backed publication ownership.
extern crate std;
use crate::rpc_capnp::{self, api};
use capnp::{
    capability::{PreparedRequest, Request},
    field_api::Struct,
    message,
};
use capntproto_rpc_engine::completion;
use core::{
    cell::{Cell, RefCell},
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};
const ZERO: capnp::Word = capnp::word(0, 0, 0, 0, 0, 0, 0, 0);
type Params = rpc_capnp::exception::Owned;

#[test]
fn fixed_request_transfers_its_original_buffer_to_a_borrowed_completion() {
    let allocations = allocation_counter::measure(|| {
        let mut words = [ZERO; 32];
        let mut slot = completion::Slot::new();
        let (sender, mut receiver) = slot.pair().unwrap();
        let builder = message::FixedBuilder::new(&mut words).unwrap();
        let mut request = Request::<Params, Params, _>::from_owner(PreparedRequest::new(
            builder,
            move |builder| sender.send(builder),
        ));
        request
            .try_get()
            .unwrap()
            .field(api::Exception::REASON)
            .copy_from("kept")
            .unwrap();
        assert!(request.send().is_ok());
        let Poll::Ready(Ok(received)) =
            Pin::new(&mut receiver).poll(&mut Context::from_waker(Waker::noop()))
        else {
            panic!("buffer not delivered")
        };
        assert_eq!(
            received
                .get_root_as_reader::<rpc_capnp::exception::Reader<'_>>()
                .unwrap()
                .get_reason()
                .unwrap(),
            "kept"
        );
    });
    assert_eq!(allocations.count_total, 0);
    assert_eq!(allocations.bytes_total, 0);
}

#[test]
fn rejected_publication_returns_the_intact_parameter_owner() {
    let allocations = allocation_counter::measure(|| {
        let mut words = [ZERO; 32];
        let mut slot = completion::Slot::new();
        let (sender, receiver) = slot.pair().unwrap();
        drop(receiver);
        let builder = message::FixedBuilder::new(&mut words).unwrap();
        let mut request = Request::<Params, Params, _>::from_owner(PreparedRequest::new(
            builder,
            move |builder| sender.send(builder),
        ));
        request
            .try_get()
            .unwrap()
            .field(api::Exception::REASON)
            .copy_from("rejected but retained")
            .unwrap();
        let remaining = request.hook.storage().remaining_words();
        let Err(returned) = request.send() else {
            panic!("closed receiver accepted owner")
        };
        assert_eq!(returned.remaining_words(), remaining);
        assert_eq!(
            returned
                .get_root_as_reader::<rpc_capnp::exception::Reader<'_>>()
                .unwrap()
                .get_reason()
                .unwrap(),
            "rejected but retained"
        );
    });
    assert_eq!(allocations.count_total, 0);
    assert_eq!(allocations.bytes_total, 0);
}

#[test]
fn unsent_request_releases_borrow_guard_before_canceling_its_sender() {
    struct Check<'a, T>(&'a RefCell<T>, &'a Cell<bool>);
    impl<T> Drop for Check<'_, T> {
        fn drop(&mut self) {
            assert!(self.0.try_borrow_mut().is_ok());
            self.1.set(true);
        }
    }
    let allocations = allocation_counter::measure(|| {
        let mut words = [ZERO; 32];
        let storage = RefCell::new(message::FixedBuilder::new(&mut words).unwrap());
        let canceled = Cell::new(false);
        let submitted = Cell::new(false);
        let check = Check(&storage, &canceled);
        let submitted_ref = &submitted;
        let request = Request::<Params, Params, _>::from_owner(PreparedRequest::new(
            storage.borrow_mut(),
            move |guard| {
                submitted_ref.set(true);
                drop(guard);
                drop(check);
            },
        ));
        assert!(storage.try_borrow_mut().is_err());
        drop(request);
        assert!(!submitted.get());
        assert!(canceled.get());
    });
    assert_eq!(allocations.count_total, 0);
    assert_eq!(allocations.bytes_total, 0);
}

#[test]
fn caller_backed_request_preserves_call_header_and_previous_params() {
    let allocations = allocation_counter::measure(|| {
        let mut words = [ZERO; 32];
        let mut builder = message::FixedBuilder::new(&mut words).unwrap();
        let submitted = Cell::new(0);
        {
            let root = builder.try_init_root::<api::Message>().unwrap();
            let mut call = root.into_field(api::Message::CALL).init().unwrap();
            call.scalar(api::Call::QUESTION_ID).set(37);
            call.scalar(api::Call::INTERFACE_ID).set(42);
            call.scalar(api::Call::METHOD_ID).set(3);
            let payload = call.into_field(api::Call::PARAMS).init().unwrap();
            let content = payload
                .into_field(api::Payload::CONTENT)
                .cast::<Struct<api::Exception>>();
            let mut request =
                Request::<Params, Params, _>::from_owner(PreparedRequest::new(content, |owner| {
                    submitted.set(submitted.get() + 1);
                    owner
                }));
            request
                .try_get()
                .unwrap()
                .field(api::Exception::REASON)
                .copy_from("saved")
                .unwrap();
            let failed=request.try_get().unwrap().field(api::Exception::REASON).copy_from("This deliberately exceeds the fixed request buffer capacity. The original header, data, and owner must remain unchanged when a parameter write is rejected. There is no fallback allocator.");
            assert!(failed.is_err());
            let _published_owner = request.send();
            assert_eq!(submitted.get(), 1);
        }
        let root = builder
            .get_root_as_reader::<rpc_capnp::message::Reader<'_>>()
            .unwrap();
        let rpc_capnp::message::Call(call) = root.which().unwrap() else {
            panic!("Call header lost")
        };
        let call = call.unwrap();
        assert_eq!(call.get_question_id(), 37);
        assert_eq!(call.get_interface_id(), 42);
        assert_eq!(call.get_method_id(), 3);
        assert_eq!(
            call.get_params()
                .unwrap()
                .get_content()
                .get_as::<rpc_capnp::exception::Reader<'_>>()
                .unwrap()
                .get_reason()
                .unwrap(),
            "saved"
        );
    });
    assert_eq!(allocations.count_total, 0);
    assert_eq!(allocations.bytes_total, 0);
}

#[test]
fn full_parameter_storage_fails_without_consuming_or_publishing_it() {
    let allocations = allocation_counter::measure(|| {
        let mut words = [ZERO; 1];
        let builder = message::FixedBuilder::new(&mut words).unwrap();
        let submitted = Cell::new(false);
        let mut request =
            Request::<Params, Params, _>::from_owner(PreparedRequest::new(builder, |owner| {
                submitted.set(true);
                owner
            }));
        let remaining = request.hook.storage().remaining_words();
        assert!(request.try_get().is_err());
        assert!(request.try_init().is_err());
        assert_eq!(request.hook.storage().remaining_words(), remaining);
        assert!(request
            .hook
            .storage()
            .get_root_as_reader::<capnp::any_pointer::Reader<'_>>()
            .unwrap()
            .is_null());
        drop(request);
        assert!(!submitted.get());
    });
    assert_eq!(allocations.count_total, 0);
    assert_eq!(allocations.bytes_total, 0);
}

#[test]
fn unsent_request_unlocks_storage_before_real_sender_drop_wakes_receiver() {
    use self::std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
        task::Wake,
    };
    struct Notice {
        storage: Arc<Mutex<()>>,
        woke: AtomicBool,
    }
    impl Wake for Notice {
        fn wake(self: Arc<Self>) {
            assert!(
                self.storage.try_lock().is_ok(),
                "cancellation woke with storage locked"
            );
            self.woke.store(true, Ordering::Release);
        }
    }
    let storage = Arc::new(Mutex::new(()));
    let notice = Arc::new(Notice {
        storage: storage.clone(),
        woke: AtomicBool::new(false),
    });
    let wake = Waker::from(notice.clone());
    let allocations = allocation_counter::measure(|| {
        let mut slot = completion::Slot::<()>::new();
        let (sender, mut receiver) = slot.pair().unwrap();
        assert!(Pin::new(&mut receiver)
            .poll(&mut Context::from_waker(&wake))
            .is_pending());
        let request = Request::<Params, Params, _>::from_owner(PreparedRequest::new(
            storage.lock().unwrap(),
            move |guard| {
                drop(guard);
                sender.send(())
            },
        ));
        drop(request);
        assert!(notice.woke.load(Ordering::Acquire));
        assert!(matches!(
            Pin::new(&mut receiver).poll(&mut Context::from_waker(&wake)),
            Poll::Ready(Err(completion::Error::Canceled))
        ));
    });
    // Waker and notification storage were supplied before measurement. The
    // request, borrowed completion and cancellation path add no allocations.
    assert_eq!(allocations.count_total, 0);
    assert_eq!(allocations.bytes_total, 0);
}

#[test]
fn concrete_service_promise_destroys_caller_pinned_future_before_slot_reuse() {
    use capnp::capability::Promise;
    use capntproto_rpc_engine::tasks::PinnedSlots;
    struct Guard<'a>(&'a Cell<usize>);
    impl Drop for Guard<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let allocations = allocation_counter::measure(|| {
        let storage = RefCell::new(42);
        let drops = Cell::new(0);
        let guard = storage.borrow_mut();
        let release = Guard(&drops);
        let operation = async move {
            let _release = release;
            let value = *guard;
            core::future::pending::<()>().await;
            Ok::<_, ()>(value)
        };
        let mut slots = core::pin::pin!(PinnedSlots::<_, 1>::new());
        let [place] = slots.as_mut().places();
        let owner = place.insert_erased(operation).ok().unwrap();
        let mut promise = Promise::from_owner(owner);
        assert!(Pin::new(&mut promise)
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        assert!(storage.try_borrow_mut().is_err());
        drop(promise);
        assert!(slots.is_empty());
        assert_eq!(drops.get(), 1);
        assert!(storage.try_borrow_mut().is_ok());
    });
    assert_eq!(allocations.count_total, 0);
    assert_eq!(allocations.bytes_total, 0);
}

#[test]
fn concrete_service_completion_preserves_real_wakes_and_borrowed_response_storage() {
    use self::std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use self::std::task::Wake;
    use capnp::capability::{Promise, Response};
    use capntproto_rpc_engine::tasks::PinnedSlots;
    struct Notified(AtomicBool);
    impl Wake for Notified {
        fn wake(self: Arc<Self>) {
            self.0.store(true, Ordering::Release);
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.store(true, Ordering::Release);
        }
    }
    let notified = Arc::new(Notified(AtomicBool::new(false)));
    let waker = Waker::from(notified.clone());
    let allocations = allocation_counter::measure(|| {
        let mut words = [ZERO; 8];
        let mut builder = message::FixedBuilder::new(&mut words).unwrap();
        builder.try_set_root_text("retained").unwrap();
        let segments = [builder.into_segment()];
        let mut slot = completion::Slot::new();
        let (sender, receiver) = slot.pair().unwrap();
        let mut tasks = core::pin::pin!(PinnedSlots::<_, 1>::new());
        let [place] = tasks.as_mut().places();
        let owner = place.insert_erased(receiver).ok().unwrap();
        let mut promise = Promise::from_owner(owner);
        let mut cx = Context::from_waker(&waker);
        assert!(Pin::new(&mut promise).poll(&mut cx).is_pending());
        assert!(!notified.0.load(Ordering::Acquire));
        let response = Response::<capnp::text::Owned, _>::from_owner(message::Reader::new(
            &segments[..],
            Default::default(),
        ));
        assert!(sender.send(response).is_ok());
        assert!(notified.0.swap(false, Ordering::AcqRel));
        let Poll::Ready(Ok(response)) = Pin::new(&mut promise).poll(&mut cx) else {
            panic!("completed operation was not notified")
        };
        drop(promise);
        assert!(tasks.is_empty());
        assert_eq!(response.get().unwrap(), "retained");
    });
    assert_eq!(allocations.count_total, 0);
    assert_eq!(allocations.bytes_total, 0);
}
