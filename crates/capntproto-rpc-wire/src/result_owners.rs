//! The legacy Results API with real generated fixed schema and RPC envelopes.
use crate::rpc_capnp::{self, api};
use capnp::{capability::Results, field_api::Struct, message, ErrorKind};
const ZERO: capnp::Word = capnp::word(0, 0, 0, 0, 0, 0, 0, 0);

#[test]
fn fixed_results_retain_payload_edits_and_ignore_unbounded_allocation_hints() {
    let measured = allocation_counter::measure(|| {
        let mut words = [ZERO; 32];
        let builder = message::FixedBuilder::new(&mut words).unwrap();
        let mut results = Results::<rpc_capnp::exception::Owned, _>::from_owner(builder);
        results
            .try_get()
            .unwrap()
            .field(api::Exception::REASON)
            .copy_from("kept")
            .unwrap();
        let hint = capnp::MessageSize {
            word_count: 1_000_000,
            cap_count: 0,
        };
        assert!(results.try_get_with_size_hint(Some(hint)).is_ok());
        let value = results
            .hook
            .get_root_as_reader::<rpc_capnp::exception::Reader<'_>>()
            .unwrap();
        assert_eq!(value.get_reason().unwrap(), "kept");
        results.try_init().unwrap();
        assert!(results
            .hook
            .get_root_as_reader::<rpc_capnp::exception::Reader<'_>>()
            .unwrap()
            .get_reason()
            .unwrap()
            .is_empty());
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn a_full_fixed_result_returns_error_and_preserves_the_previous_root() {
    let measured = allocation_counter::measure(|| {
        let mut words = [ZERO; 1];
        let builder = message::FixedBuilder::new(&mut words).unwrap();
        let mut results = Results::<rpc_capnp::exception::Owned, _>::from_owner(builder);
        let available = results.hook.remaining_words();
        assert!(results.try_get().is_err());
        assert_eq!(results.hook.remaining_words(), available);
        assert!(results
            .hook
            .get_root_as_reader::<capnp::any_pointer::Reader<'_>>()
            .unwrap()
            .is_null());
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn a_return_content_owner_preserves_header_and_previous_data_on_growth_failure() {
    let measured = allocation_counter::measure(|| {
        let mut words = [ZERO; 24];
        let mut builder = message::FixedBuilder::new(&mut words).unwrap();
        {
            let root = builder.try_init_root::<api::Message>().unwrap();
            let mut returned = root.into_field(api::Message::RETURN).init().unwrap();
            returned.scalar(api::Return::ANSWER_ID).set(73);
            returned.scalar(api::Return::RELEASE_PARAM_CAPS).set(false);
            let payload = returned.into_field(api::Return::RESULTS).init().unwrap();
            let content = payload
                .into_field(api::Payload::CONTENT)
                .cast::<Struct<api::Exception>>();
            let mut results = Results::<rpc_capnp::exception::Owned, _>::from_owner(content);
            results
                .try_get()
                .unwrap()
                .field(api::Exception::REASON)
                .copy_from("kept")
                .unwrap();
            let failed = results.try_get().unwrap().field(api::Exception::REASON).copy_from("This string deliberately exceeds the remaining fixed response capacity by a substantial margin. No fallback allocator may replace the original payload.");
            assert!(failed.is_err());
        }
        let root = builder
            .get_root_as_reader::<rpc_capnp::message::Reader<'_>>()
            .unwrap();
        let rpc_capnp::message::Return(returned) = root.which().unwrap() else {
            panic!("lost Return envelope")
        };
        let returned = returned.unwrap();
        assert_eq!(returned.get_answer_id(), 73);
        assert!(!returned.get_release_param_caps());
        let rpc_capnp::return_::Results(payload) = returned.which().unwrap() else {
            panic!("lost result payload")
        };
        let content = payload
            .unwrap()
            .get_content()
            .get_as::<rpc_capnp::exception::Reader<'_>>()
            .unwrap();
        assert_eq!(content.get_reason().unwrap(), "kept");
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn an_unselected_result_field_is_rejected_without_changing_the_return_variant() {
    let measured = allocation_counter::measure(|| {
        let mut words = [ZERO; 24];
        let mut builder = message::FixedBuilder::new(&mut words).unwrap();
        {
            let root = builder.try_init_root::<api::Message>().unwrap();
            let mut returned = root.into_field(api::Message::RETURN).init().unwrap();
            returned.void(api::Return::CANCELED).set();
            let field = returned.into_field(api::Return::RESULTS);
            let mut results = Results::<rpc_capnp::payload::Owned, _>::from_owner(field);
            let error = results.try_get().err().unwrap();
            assert_eq!(error.kind, ErrorKind::NotPresent);
        }
        let root = builder
            .get_root_as_reader::<rpc_capnp::message::Reader<'_>>()
            .unwrap();
        let rpc_capnp::message::Return(returned) = root.which().unwrap() else {
            panic!("lost Return")
        };
        assert!(matches!(
            returned.unwrap().which().unwrap(),
            rpc_capnp::return_::Canceled(())
        ));
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn fixed_result_guards_keep_pooled_storage_exclusive_until_owner_release() {
    let measured = allocation_counter::measure(|| {
        let mut words = [ZERO; 32];
        let cell = core::cell::RefCell::new(message::FixedBuilder::new(&mut words).unwrap());
        let guard = cell.borrow_mut();
        let mut results = Results::<rpc_capnp::exception::Owned, _>::from_owner(guard);
        results
            .try_get()
            .unwrap()
            .field(api::Exception::REASON)
            .copy_from("guarded")
            .unwrap();
        assert!(cell.try_borrow_mut().is_err());
        drop(results);
        let guard = cell.borrow();
        let root = guard
            .get_root_as_reader::<rpc_capnp::exception::Reader<'_>>()
            .unwrap();
        assert_eq!(root.get_reason().unwrap(), "guarded");
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn legacy_owned_schema_markers_select_the_same_no_allocator_read_and_write_views() {
    use capnp::capability::{Params, Response};
    let measured = allocation_counter::measure(|| {
        let mut words = [ZERO; 32];
        let builder = message::FixedBuilder::new(&mut words).unwrap();
        let mut results = Results::<rpc_capnp::exception::Owned, _>::from_owner(builder);
        results
            .try_get()
            .unwrap()
            .field(api::Exception::REASON)
            .copy_from("typed")
            .unwrap();
        let segment = results.hook.into_segment();
        let segments = [segment];
        let reader = message::Reader::new(&segments[..], Default::default());
        let params = Params::<rpc_capnp::exception::Owned, _>::from_owner(reader);
        assert_eq!(
            params
                .view()
                .unwrap()
                .field(api::Exception::REASON)
                .wire_text()
                .unwrap(),
            "typed"
        );
        let response = Response::<rpc_capnp::exception::Owned, _>::from_owner(params.hook);
        assert_eq!(
            response
                .view()
                .unwrap()
                .field(api::Exception::REASON)
                .wire_text()
                .unwrap(),
            "typed"
        );
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}
