//! The legacy invocation context keeps concrete parameters/results across calls.
use crate::rpc_capnp::{self, api};
use capnp::{
    capability::{CallContext, PayloadStorage, PreparedCall, Promise},
    field_api::{FixedStruct, Struct},
    message,
    private::capability::ParamsHook,
};
use core::{
    cell::{Cell, RefCell, RefMut},
    future::Future,
    pin::Pin,
    task::{Context, Waker},
};
const ZERO: capnp::Word = capnp::word(0, 0, 0, 0, 0, 0, 0, 0);
type Output = rpc_capnp::exception::Owned;

#[test]
fn fixed_invocation_completes_in_a_return_envelope_without_boxed_hooks() {
    let measured = allocation_counter::measure(|| {
        let mut input_words = [ZERO; 8];
        let mut input = message::FixedBuilder::new(&mut input_words).unwrap();
        input.try_set_root_text("kept").unwrap();
        let segments = [input.into_segment()];
        let reader = message::Reader::new(&segments[..], Default::default());
        let mut output_words = [ZERO; 24];
        let mut output = message::FixedBuilder::new(&mut output_words).unwrap();
        {
            let root = output.try_init_root::<api::Message>().unwrap();
            let mut returned = root.into_field(api::Message::RETURN).init().unwrap();
            returned.scalar(api::Return::ANSWER_ID).set(73);
            returned.scalar(api::Return::RELEASE_PARAM_CAPS).set(false);
            let content = returned
                .into_field(api::Return::RESULTS)
                .init()
                .unwrap()
                .into_field(api::Payload::CONTENT)
                .cast::<Struct<api::Exception>>();
            let call = CallContext::<capnp::text::Owned, Output, _>::from_owner(PreparedCall::new(
                reader, content,
            ));
            call.try_complete(|call| {
                let (params, mut results) = call.get()?;
                results
                    .field(api::Exception::REASON)
                    .copy_from(params.to_str()?)
            })
            .unwrap();
        }
        let root = output
            .get_root_as_reader::<rpc_capnp::message::Reader<'_>>()
            .unwrap();
        let rpc_capnp::message::Return(returned) = root.which().unwrap() else {
            panic!("Return lost")
        };
        let returned = returned.unwrap();
        assert_eq!(returned.get_answer_id(), 73);
        assert!(!returned.get_release_param_caps());
        let rpc_capnp::return_::Results(payload) = returned.which().unwrap() else {
            panic!("results lost")
        };
        assert_eq!(
            payload
                .unwrap()
                .get_content()
                .get_as::<rpc_capnp::exception::Reader<'_>>()
                .unwrap()
                .get_reason()
                .unwrap(),
            "kept"
        );
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn fixed_invocation_reports_growth_failure_and_retains_previous_results() {
    let measured = allocation_counter::measure(|| {
        let mut input_words = [ZERO; 8];
        let mut input = message::FixedBuilder::new(&mut input_words).unwrap();
        input.try_set_root_text("kept").unwrap();
        let segments = [input.into_segment()];
        let reader = message::Reader::new(&segments[..], Default::default());
        let mut output_words = [ZERO; 8];
        let mut output = message::FixedBuilder::new(&mut output_words).unwrap();
        let call = CallContext::<capnp::text::Owned, Output, _>::from_owner(PreparedCall::new(
            reader,
            &mut output,
        ));
        let result = call.try_complete(|call| {
            let (params, mut results) = call.get()?;
            results.field(api::Exception::REASON).copy_from(params.to_str()?)?;
            results.field(api::Exception::REASON).copy_from("This exceeds the fixed result capacity and must reach the completion as an error without replacing the previous message or using a fallback allocator.")
        });
        assert!(matches!(
            result.unwrap_err().kind,
            capnp::ErrorKind::InsufficientCapacity { .. }
        ));
        assert_eq!(
            output
                .get_root_as_reader::<rpc_capnp::exception::Reader<'_>>()
                .unwrap()
                .get_reason()
                .unwrap(),
            "kept"
        );
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn concrete_invocation_owners_move_into_a_suspended_service_and_cancel_in_place() {
    let measured = allocation_counter::measure(|| {
        let mut input_words = [ZERO; 8];
        let mut input = message::FixedBuilder::new(&mut input_words).unwrap();
        input.try_set_root_text("retained").unwrap();
        let segments = [input.into_segment()];
        let reader = message::Reader::new(&segments[..], Default::default());
        let mut output_words = [ZERO; 16];
        let output = RefCell::new(message::FixedBuilder::new(&mut output_words).unwrap());
        let call = CallContext::<capnp::text::Owned, Output, _>::from_owner(PreparedCall::new(
            reader,
            output.borrow_mut(),
        ));
        let (params, mut results) = call.into_owned();
        let operation = async move {
            results
                .try_get()?
                .field(api::Exception::REASON)
                .copy_from(params.get()?.to_str()?)?;
            core::future::pending::<()>().await;
            Ok::<(), capnp::Error>(())
        };
        let mut slots = core::pin::pin!(capntproto_rpc_engine::tasks::PinnedSlots::<_, 1>::new());
        let [place] = slots.as_mut().places();
        let mut completion = Promise::from_owner(place.insert_erased(operation).ok().unwrap());
        assert!(Pin::new(&mut completion)
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        assert!(output.try_borrow_mut().is_err());
        drop(completion);
        assert!(slots.is_empty());
        let output = output.try_borrow_mut().unwrap();
        assert_eq!(
            output
                .get_root_as_reader::<rpc_capnp::exception::Reader<'_>>()
                .unwrap()
                .get_reason()
                .unwrap(),
            "retained"
        );
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn dropping_an_unentered_invocation_releases_params_before_result_cleanup() {
    struct Input<'a> {
        _guard: RefMut<'a, ()>,
        dropped: &'a Cell<bool>,
    }
    impl ParamsHook for Input<'_> {
        fn get(&self) -> capnp::Result<capnp::any_pointer::Reader<'_>> {
            panic!("unentered call read params")
        }
    }
    impl Drop for Input<'_> {
        fn drop(&mut self) {
            self.dropped.set(true);
        }
    }
    struct Sink<'a, 'b> {
        builder: message::FixedBuilder<'b>,
        params: &'a RefCell<()>,
        input_dropped: &'a Cell<bool>,
        dropped: &'a Cell<bool>,
    }
    impl PayloadStorage<Output> for Sink<'_, '_> {
        type Builder<'a>
            = FixedStruct<'a, api::Exception>
        where
            Self: 'a;
        fn try_get(
            &mut self,
            hint: Option<capnp::MessageSize>,
        ) -> capnp::Result<Self::Builder<'_>> {
            PayloadStorage::<Output>::try_get(&mut self.builder, hint)
        }
        fn try_init(
            &mut self,
            hint: Option<capnp::MessageSize>,
        ) -> capnp::Result<Self::Builder<'_>> {
            PayloadStorage::<Output>::try_init(&mut self.builder, hint)
        }
    }
    impl Drop for Sink<'_, '_> {
        fn drop(&mut self) {
            assert!(self.input_dropped.get());
            assert!(self.params.try_borrow_mut().is_ok());
            self.dropped.set(true);
        }
    }
    let measured = allocation_counter::measure(|| {
        let params = RefCell::new(());
        let input_dropped = Cell::new(false);
        let result_dropped = Cell::new(false);
        let mut words = [ZERO; 8];
        let call = CallContext::<capnp::text::Owned, Output, _>::from_owner(PreparedCall::new(
            Input {
                _guard: params.borrow_mut(),
                dropped: &input_dropped,
            },
            Sink {
                builder: message::FixedBuilder::new(&mut words).unwrap(),
                params: &params,
                input_dropped: &input_dropped,
                dropped: &result_dropped,
            },
        ));
        drop(call);
        assert!(result_dropped.get());
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn rejected_parameter_read_does_not_initialize_result_storage() {
    let measured = allocation_counter::measure(|| {
        let mut input_words = [ZERO; 8];
        let mut input = message::FixedBuilder::new(&mut input_words).unwrap();
        input.try_set_root_text("unread").unwrap();
        let segments = [input.into_segment()];
        let mut options = message::ReaderOptions::new();
        options.traversal_limit_in_words(Some(0));
        let reader = message::Reader::new(&segments[..], options);
        let mut output_words = [ZERO; 8];
        let mut output = message::FixedBuilder::new(&mut output_words).unwrap();
        let call = CallContext::<capnp::text::Owned, Output, _>::from_owner(PreparedCall::new(
            reader,
            &mut output,
        ));
        let error = call
            .try_complete(|call| {
                let _ = call.get()?;
                Ok(())
            })
            .unwrap_err();
        assert_eq!(error.kind, capnp::ErrorKind::ReadLimitExceeded);
        assert!(output
            .get_root_as_reader::<capnp::any_pointer::Reader<'_>>()
            .unwrap()
            .is_null());
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}
