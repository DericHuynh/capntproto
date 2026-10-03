use super::*;

fn edit_raw(mut raw: any_struct::Builder<'_>, data: bool) {
    // CastItem.number has an XOR default of 17.
    raw.get_data_section()[..4].copy_from_slice(&(99u32 ^ 17).to_le_bytes());
    let mut pointer = raw.get_pointer_section().get(0);
    if data {
        pointer
            .set_as::<capnp::data::Owned>(b"new".as_slice())
            .unwrap();
    } else {
        pointer.set_as::<capnp::text::Owned>("new").unwrap();
    }
}
fn list_address(raw: any_list::Reader<'_>) -> *const u8 {
    match raw.get_raw_bytes() {
        Ok(bytes) => bytes.as_ptr(),
        Err(_) => raw
            .get_as_struct_list()
            .unwrap()
            .get(0)
            .unwrap()
            .get_data_section()
            .as_ptr(),
    }
}

pub(super) fn apply_erased(message: &mut Message, loader: &SchemaLoader, case: &Case) -> String {
    let before = wire(message);
    match case.shape {
        Shape::Struct(..) => {
            let address = message
                .get_root_as_reader::<any_struct::Reader>()
                .unwrap()
                .get_data_section()
                .as_ptr();
            let loaded =
                dynamic::Builder::new(message.get_root().unwrap(), structure(loader, case.target))
                    .unwrap();
            let raw = loaded.into_any_struct();
            assert_eq!(raw.as_reader().get_data_section().as_ptr(), address);
            let mut copy = Message::new_default();
            copy.set_root::<any_pointer::Owned>(raw.as_reader())
                .unwrap();
            // Erasure itself does not modify or narrow the physical object.
            assert_eq!(wire(&copy), before);
            edit_raw(raw, case.target == "data");
        }
        Shape::List(encoding, count, _, _) => {
            let address = list_address(message.get_root_as_reader::<any_list::Reader>().unwrap());
            let loaded = message
                .get_root::<any_list::Builder>()
                .unwrap()
                .get_as_loaded(element(loader, case.target))
                .unwrap();
            let mut raw = loaded.into_any_list();
            assert_eq!(raw.len(), count);
            assert_eq!(raw.get_element_size(), encoding);
            assert_eq!(list_address(raw.as_reader()), address);
            let mut copy = Message::new_default();
            copy.set_root::<any_pointer::Owned>(raw.as_reader())
                .unwrap();
            assert_eq!(wire(&copy), before);
            match case.target {
                "records" => edit_raw(raw.get_as_struct_list().unwrap().get(0), false),
                "numbers" => raw
                    .get_as::<capnp::primitive_list::Owned<u32>>()
                    .unwrap()
                    .set(0, 99),
                "choices" => raw
                    .get_as::<capnp::primitive_list::Owned<u16>>()
                    .unwrap()
                    .set(0, 65535),
                "flags" => raw
                    .get_as::<capnp::primitive_list::Owned<bool>>()
                    .unwrap()
                    .set(0, false),
                "texts" => raw
                    .get_as::<capnp::text_list::Owned>()
                    .unwrap()
                    .set(0, "new"),
                "blobs" => raw
                    .get_as::<capnp::data_list::Owned>()
                    .unwrap()
                    .set(0, b"new"),
                "nested" => {
                    let mut outer = raw
                        .reborrow()
                        .get_as_loaded(element(loader, "nested"))
                        .unwrap();
                    let child = outer.reborrow().get_list(0).unwrap().into_any_list();
                    child
                        .get_as::<capnp::primitive_list::Owned<u32>>()
                        .unwrap()
                        .set(0, 99);
                }
                _ => panic!(),
            }
        }
    }
    apply(
        message,
        loader,
        &Case {
            mutable: false,
            ..*case
        },
    )
    .unwrap()
}

#[test]
fn loaded_builder_erasure_preserves_physical_layouts_and_edits_original_storage() {
    let loader = loader();
    for case in cases().iter().filter(|c| c.accepts && c.mutable) {
        let mut message = seed(case);
        let mut compiled = seed(case);
        let observed = apply_erased(&mut message, &loader, case);
        assert_eq!(
            observed,
            apply_native(&mut compiled, case).unwrap(),
            "{case:?}"
        );
        assert_eq!(wire(&message), wire(&compiled), "{case:?}");
    }
}

#[test]
fn erasure_preserves_empty_encodings_and_null_children() {
    let loader = loader();
    for (encoding, ty) in [
        (E::Void, Type::Void),
        (E::Byte, Type::UInt8),
        (E::EightBytes, Type::UInt64),
    ] {
        for count in [0, 3] {
            let case = Case {
                shape: Shape::List(encoding, count, 0, 0),
                target: "numbers",
                mutable: false,
                accepts: true,
            };
            let mut message = seed(&case);
            let before = capnp::serialize::write_message_to_words(&message);
            let loaded = message
                .get_root::<any_list::Builder>()
                .unwrap()
                .get_as_loaded(ty.clone())
                .unwrap();
            let raw = loaded.into_any_list();
            assert_eq!(raw.get_element_size(), encoding);
            assert_eq!(raw.len(), count);
            assert_eq!(capnp::serialize::write_message_to_words(&message), before);
        }
    }
    let mut message = Message::new_default();
    message.init_root::<cast_types::Builder>().init_nested(1);
    let before = capnp::serialize::write_message_to_words(&message);
    let mut loaded =
        dynamic::Builder::new(message.get_root().unwrap(), root_schema(&loader)).unwrap();
    assert!(loaded
        .reborrow()
        .get_list("numbers")
        .unwrap()
        .into_any_list()
        .is_empty());
    let mut outer = loaded.reborrow().get_list("nested").unwrap();
    assert!(outer
        .reborrow()
        .get_list(0)
        .unwrap()
        .into_any_list()
        .is_empty());
    let raw = outer.into_any_list();
    assert!(raw
        .as_reader()
        .get_as::<capnp::any_pointer_list::Owned>()
        .unwrap()
        .get(0)
        .is_null());
    assert_eq!(capnp::serialize::write_message_to_words(&message), before);
}

#[test]
fn group_erasure_exposes_containing_storage_and_retains_reborrow_exclusivity() {
    use capntproto_test_support::presence_capnp::group_reset;
    let mut native = SchemaLoader::default();
    native
        .load_compiled_type_and_dependencies::<group_reset::Owned>()
        .unwrap();
    let mut loader = SchemaLoader::default();
    loader
        .load_batch(native.get_all_loaded().map(|s| s.get_proto()))
        .unwrap();
    let schema = loader
        .get(
            group_reset::Owned::introspect()
                .as_struct_schema()
                .unwrap()
                .get_proto()
                .get_id(),
        )
        .unwrap();
    let mut message = Message::new_default();
    let mut root = message.init_root::<group_reset::Builder>();
    root.set_marker(17);
    root.init_body().set_count(23);
    let before = message.get_root_as_reader::<any_struct::Reader>().unwrap();
    let address = before.get_data_section().as_ptr();
    let size = before.get_data_section().len();
    let pointers = before.get_pointer_section().len();
    let mut group = dynamic::Builder::new(message.get_root().unwrap(), schema)
        .unwrap()
        .group("body")
        .unwrap();
    {
        let raw = group.reborrow().into_any_struct();
        assert_eq!(raw.as_reader().get_data_section().as_ptr(), address);
        assert_eq!(raw.as_reader().get_data_section().len(), size);
        assert_eq!(raw.as_reader().get_pointer_section().len(), pointers);
        raw.get_as::<group_reset::Owned>().unwrap().set_marker(99);
    }
    assert!(matches!(
        group.as_reader().get_named("count").unwrap(),
        dynamic::Value::UInt32(23)
    ));
    group
        .set_named("count", dynamic::Value::UInt32(42))
        .unwrap();
    let native = message.get_root_as_reader::<group_reset::Reader>().unwrap();
    assert_eq!(native.get_marker(), 99);
    assert_eq!(native.get_body().get_count(), 42);
}
