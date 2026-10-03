use capnp::{
    introspect::Introspect,
    message,
    schema_loader::{dynamic, SchemaLoader, Type},
};
use capntproto_test_support::native_list_capnp::{self as fixture, item, lists, other, service};
mod native_list {
    pub mod capabilities;
    pub mod verification;
}
type Numbers = capnp::primitive_list::Owned<u32>;
type Choices = capnp::enum_list::Owned<fixture::Choice>;
type Records = capnp::struct_list::Owned<item::Owned<capnp::data::Owned>>;
type Nested = capnp::list_list::Owned<Records>;
type Caps = capnp::capability_list::Owned<service::Client>;
type WrongCaps = capnp::capability_list::Owned<other::Client>;
type Message = message::Builder<message::HeapAllocator>;
fn root_id() -> u64 {
    lists::Owned::introspect()
        .as_struct_schema()
        .unwrap()
        .get_proto()
        .get_id()
}
fn unregistered() -> SchemaLoader {
    let mut native = SchemaLoader::default();
    native
        .load_compiled_type_and_dependencies::<lists::Owned>()
        .unwrap();
    let mut loader = SchemaLoader::default();
    loader
        .load_batch(native.get_all_loaded().map(|s| s.get_proto()))
        .unwrap();
    loader
}
fn register(loader: &mut SchemaLoader, kind: u64) {
    match kind {
        0 => loader
            .load_compiled_type_and_dependencies::<fixture::Choice>()
            .unwrap(),
        1 => loader
            .load_compiled_type_and_dependencies::<item::Owned<capnp::text::Owned>>()
            .unwrap(),
        2 => loader
            .load_compiled_type_and_dependencies::<service::Owned>()
            .unwrap(),
        _ => panic!(),
    }
}
fn field(case: u64) -> &'static str {
    match case {
        0 => "numbers",
        1 => "choices",
        2 | 4 => "records",
        3 => "nested",
        5 | 6 => "caps",
        7 => "deepNumbers",
        _ => panic!(),
    }
}
fn make_message(loader: &SchemaLoader, case: u64) -> Message {
    let mut message = message::Builder::new_default();
    let root = dynamic::Builder::init(message.init_root(), loader.get(root_id()).unwrap()).unwrap();
    let list = root.init_list(field(case), 1).unwrap();
    if case == 3 || case == 7 {
        list.init_list(0, 1).unwrap();
    }
    message
}
fn cast_reader(message: &Message, loader: &SchemaLoader, case: u64) -> bool {
    let root = dynamic::Reader::new(
        message.get_root_as_reader().unwrap(),
        loader.get(root_id()).unwrap(),
    )
    .unwrap();
    let dynamic::Value::List(list) = root.get_named(field(case)).unwrap() else {
        panic!()
    };
    macro_rules! cast {
        ($t:ty) => {
            list.downcast_native::<$t>()
                .map(|v| {
                    assert_eq!(v.len(), 1);
                })
                .is_ok()
        };
    }
    match case {
        0 | 4 | 7 => cast!(Numbers),
        1 => cast!(Choices),
        2 => cast!(Records),
        3 => cast!(Nested),
        5 => cast!(Caps),
        6 => cast!(WrongCaps),
        _ => panic!(),
    }
}
fn cast_builder(message: &mut Message, loader: &SchemaLoader, case: u64, write: bool) -> bool {
    let root =
        dynamic::Builder::new(message.get_root().unwrap(), loader.get(root_id()).unwrap()).unwrap();
    let list = root.get_list(field(case)).unwrap();
    match case {
        0 | 4 | 7 => list
            .downcast_native::<Numbers>()
            .map(|mut v| {
                assert_eq!(v.len(), 1);
                if write {
                    v.set(0, 1);
                }
            })
            .is_ok(),
        1 => list
            .downcast_native::<Choices>()
            .map(|mut v| {
                assert_eq!(v.len(), 1);
                if write {
                    v.set(0, fixture::Choice::One);
                }
            })
            .is_ok(),
        2 => list
            .downcast_native::<Records>()
            .map(|v| {
                assert_eq!(v.len(), 1);
                if write {
                    v.get(0).set_number(1);
                }
            })
            .is_ok(),
        3 => list
            .downcast_native::<Nested>()
            .map(|v| {
                assert_eq!(v.len(), 1);
                if write {
                    v.get(0).unwrap().get(0).set_number(1);
                }
            })
            .is_ok(),
        5 => list
            .downcast_native::<Caps>()
            .map(|v| {
                assert_eq!(v.len(), 1);
            })
            .is_ok(),
        6 => list
            .downcast_native::<WrongCaps>()
            .map(|v| {
                assert_eq!(v.len(), 1);
            })
            .is_ok(),
        _ => panic!(),
    }
}
fn value(message: &Message, case: u64) -> u64 {
    let root = message.get_root_as_reader::<lists::Reader>().unwrap();
    match case {
        0 => root.get_numbers().unwrap().get(0) as u64,
        1 => u16::from(root.get_choices().unwrap().get(0).unwrap()) as u64,
        2 | 4 => root.get_records().unwrap().get(0).get_number() as u64,
        3 => root
            .get_nested()
            .unwrap()
            .get(0)
            .unwrap()
            .get(0)
            .get_number() as u64,
        5 | 6 => 0,
        7 => root.get_deep_numbers().unwrap().get(0).unwrap().get(0) as u64,
        _ => panic!(),
    }
}
#[test]
fn casts_check_registration_kind_id_and_depth_and_erase_generic_arguments() {
    for registered in [false, true] {
        let mut loader = unregistered();
        if registered {
            for kind in 0..3 {
                register(&mut loader, kind);
            }
        }
        for case in 0..8 {
            let expected = case == 0 || (registered && matches!(case, 1 | 2 | 3 | 5));
            let mut message = make_message(&loader, case);
            assert_eq!(
                cast_reader(&message, &loader, case),
                expected,
                "reader {registered}/{case}"
            );
            assert_eq!(
                cast_builder(&mut message, &loader, case, true),
                expected,
                "builder {registered}/{case}"
            );
            assert_eq!(value(&message, case), u64::from(expected && case < 4));
        }
    }
}
#[test]
fn native_type_checks_handle_pointer_constraints_and_deep_lists_without_recursion() {
    use capnp::schema_loader::PointerKind;
    for kind in [
        PointerKind::Any,
        PointerKind::Struct,
        PointerKind::List,
        PointerKind::Capability,
    ] {
        assert!(Type::AnyPointer(kind)
            .require_usable_as::<capnp::any_pointer::Owned>()
            .is_ok());
        assert!(Type::AnyPointer(kind)
            .require_usable_as::<capnp::text::Owned>()
            .is_err());
    }
    assert!(Type::Parameter(91, 0)
        .require_usable_as::<capnp::any_pointer::Owned>()
        .is_ok());
    assert!(Type::Unknown(65535)
        .require_usable_as::<capnp::any_pointer::Owned>()
        .is_err());
    struct Deep;
    impl Introspect for Deep {
        fn introspect() -> capnp::introspect::Type {
            (0..512).fold(u32::introspect(), |ty, _| {
                capnp::introspect::Type::list_of(ty)
            })
        }
    }
    let mut deep = (0..512).fold(Type::UInt32, |ty, _| Type::List(Box::new(ty)));
    assert!(deep.require_usable_as::<Deep>().is_ok());
    assert!(deep.require_usable_as::<Numbers>().is_err());
    // Destruction is separate from the iterative compatibility check.
    while let Type::List(inner) = deep {
        deep = *inner;
    }
}

#[test]
fn blob_bit_and_empty_list_casts_borrow_the_original_storage() {
    let loader = unregistered();
    let schema = loader.get(root_id()).unwrap();
    let mut message = message::Builder::new_default();
    {
        let mut root = dynamic::Builder::init(message.init_root(), schema.clone()).unwrap();
        root.reborrow()
            .init_list("bytes", 2)
            .unwrap()
            .downcast_native::<capnp::primitive_list::Owned<u8>>()
            .unwrap()
            .set(1, 37);
        root.reborrow()
            .init_list("flags", 9)
            .unwrap()
            .downcast_native::<capnp::primitive_list::Owned<bool>>()
            .unwrap()
            .set(8, true);
        root.reborrow()
            .init_list("texts", 1)
            .unwrap()
            .downcast_native::<capnp::text_list::Owned>()
            .unwrap()
            .set(0, "same storage");
        root.reborrow()
            .init_list("blobs", 1)
            .unwrap()
            .downcast_native::<capnp::data_list::Owned>()
            .unwrap()
            .set(0, b"bytes");
        let mut empty = root.get_list("numbers").unwrap();
        assert!(empty
            .reborrow()
            .downcast_native::<Numbers>()
            .unwrap()
            .is_empty());
        assert!(empty
            .reborrow()
            .downcast_native::<capnp::text::Owned>()
            .is_err());
        assert!(empty
            .into_reader()
            .downcast_native::<capnp::text::Owned>()
            .is_err());
    }
    let loaded = dynamic::Reader::new(message.get_root_as_reader().unwrap(), schema).unwrap();
    let typed = message.get_root_as_reader::<lists::Reader>().unwrap();
    let list = |name| {
        let dynamic::Value::List(list) = loaded.get_named(name).unwrap() else {
            panic!()
        };
        list
    };
    let bytes = list("bytes")
        .downcast_native::<capnp::primitive_list::Owned<u8>>()
        .unwrap();
    assert_eq!(bytes.get(1), 37);
    assert_eq!(
        bytes.as_slice().unwrap().as_ptr(),
        typed.get_bytes().unwrap().as_slice().unwrap().as_ptr()
    );
    let flags = list("flags")
        .downcast_native::<capnp::primitive_list::Owned<bool>>()
        .unwrap();
    assert!(flags.get(8));
    assert!(!flags.get(7));
    assert!(typed.get_flags().unwrap().get(8));
    let text = list("texts")
        .downcast_native::<capnp::text_list::Owned>()
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(text, "same storage");
    assert_eq!(
        text.as_bytes().as_ptr(),
        typed
            .get_texts()
            .unwrap()
            .get(0)
            .unwrap()
            .as_bytes()
            .as_ptr()
    );
    let data = list("blobs")
        .downcast_native::<capnp::data_list::Owned>()
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(data, b"bytes");
    assert_eq!(
        data.as_ptr(),
        typed.get_blobs().unwrap().get(0).unwrap().as_ptr()
    );
    assert!(list("numbers")
        .downcast_native::<Numbers>()
        .unwrap()
        .is_empty());
    // Empty lists still require native registration for named element types.
    assert!(list("choices").downcast_native::<Choices>().is_err());
}

#[test]
fn native_registration_is_loader_local_and_does_not_relax_dynamic_assignment() {
    let plain = unregistered();
    let mut native = plain.clone();
    register(&mut native, 1);
    let source = make_message(&plain, 2);
    assert!(!cast_reader(&source, &plain, 2));
    assert!(cast_reader(&source, &native, 2));
    let mut source = message::Builder::new_default();
    let id = item::Owned::<capnp::data::Owned>::introspect()
        .as_struct_schema()
        .unwrap()
        .get_proto()
        .get_id();
    let mut brand = message::Builder::new_default();
    let mut scope = brand
        .init_root::<capnp::schema_capnp::brand::Builder>()
        .init_scopes(1)
        .get(0);
    scope.set_scope_id(id);
    scope.init_bind(1).get(0).init_type().set_data(());
    let data = native
        .get(id)
        .unwrap()
        .bind(brand.get_root_as_reader().unwrap(), None)
        .unwrap();
    let mut src = dynamic::Builder::init(source.init_root(), data).unwrap();
    src.set_named("number", dynamic::Value::UInt32(77)).unwrap();
    let mut destination = make_message(&native, 2);
    let mut dst = dynamic::Builder::new(
        destination.get_root().unwrap(),
        native.get(root_id()).unwrap(),
    )
    .unwrap()
    .get_list("records")
    .unwrap();
    assert!(dst.set(0, dynamic::Value::Struct(src.as_reader())).is_err());
    assert_eq!(value(&destination, 2), 0);
}
