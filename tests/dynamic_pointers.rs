use capnp::{
    any_list::{self, ElementSize},
    any_pointer, any_struct, dynamic_struct, dynamic_value,
    introspect::Introspect,
    message,
    schema_capnp::{field, node},
    schema_loader::{dynamic, Schema, SchemaLoader},
};
use reproto_test_support::presence_capnp::{access_capability, pointer_access};
type Message = message::Builder<message::HeapAllocator>;
fn native_schema() -> capnp::schema::StructSchema {
    pointer_access::Owned::introspect()
        .as_struct_schema()
        .unwrap()
}
fn loader() -> SchemaLoader {
    let mut native = SchemaLoader::default();
    native
        .load_compiled_type_and_dependencies::<pointer_access::Owned>()
        .unwrap();
    let mut loader = SchemaLoader::default();
    loader
        .load_batch(native.get_all_loaded().map(|s| s.get_proto()))
        .unwrap();
    loader
}
fn schema(loader: &SchemaLoader) -> Schema<'_> {
    loader.get(native_schema().get_proto().get_id()).unwrap()
}
fn tag(name: &str) -> u16 {
    native_schema()
        .get_field_by_name(name)
        .unwrap()
        .get_proto()
        .get_discriminant_value()
}
fn set_tag(message: &mut Message, tag: u16) {
    let node::Struct(layout) = native_schema().get_proto().which().unwrap() else {
        panic!()
    };
    let offset = layout.get_discriminant_offset() as usize * 2;
    any_struct::Builder::from_builder(message.get_root::<pointer_access::Builder>().unwrap())
        .unwrap()
        .get_data_section()[offset..offset + 2]
        .copy_from_slice(&tag.to_le_bytes());
}
#[derive(Clone, Copy, Debug)]
enum Kind {
    Any,
    Struct,
    List,
}
impl Kind {
    fn field(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Struct => "structure",
            Self::List => "list",
        }
    }
    fn paths(self) -> [&'static str; 2] {
        match self {
            Self::Any => ["plainAny", "boundAny.body.value"],
            Self::Struct => ["plainStruct", "details.structure"],
            Self::List => ["plainList", "details.list"],
        }
    }
}
#[derive(Clone, Copy, Debug)]
enum Seed {
    Null,
    Struct,
    EmptyStruct,
    Numbers,
    Pointers,
    Structs,
    Bits,
    Voids,
    Text,
    EmptyList,
}
impl Seed {
    fn compatible(self, kind: Kind) -> bool {
        match kind {
            Kind::Any => true,
            Kind::Struct => matches!(self, Self::Null | Self::Struct | Self::EmptyStruct),
            Kind::List => !matches!(self, Self::Struct | Self::EmptyStruct),
        }
    }
}
#[derive(Clone, Copy, Debug)]
enum Action {
    Get,
    Edit,
    Init { encoding: u8, count: u32 },
}
#[derive(Debug)]
struct Case {
    kind: Kind,
    seed: Seed,
    path: String,
    tag: u16,
    action: Action,
    accepts: bool,
}
fn cases() -> Vec<Case> {
    let mut cases = vec![];
    for kind in [Kind::Any, Kind::Struct, Kind::List] {
        let mut add = |seed: Seed, path: &str, tag, action, accepts| {
            cases.push(Case {
                kind,
                seed,
                path: path.into(),
                tag,
                action,
                accepts,
            })
        };
        for mode in [
            Seed::Null,
            Seed::Struct,
            Seed::EmptyStruct,
            Seed::Numbers,
            Seed::Pointers,
            Seed::Structs,
            Seed::Bits,
            Seed::Voids,
            Seed::Text,
            Seed::EmptyList,
        ] {
            add(
                mode,
                kind.field(),
                tag(kind.field()),
                Action::Get,
                mode.compatible(kind),
            );
        }
        for selection in [0, u16::MAX] {
            for mode in [Seed::Null, Seed::Struct] {
                add(mode, kind.field(), selection, Action::Get, false);
            }
        }
        let populated = match kind {
            Kind::Any | Kind::Struct => Seed::Struct,
            Kind::List => Seed::Numbers,
        };
        for path in kind.paths() {
            for mode in [Seed::Null, populated] {
                add(mode, path, u16::MAX, Action::Get, true);
            }
        }
        add(
            populated,
            kind.field(),
            tag(kind.field()),
            Action::Edit,
            true,
        );
        for selection in [0, tag(kind.field()), u16::MAX] {
            add(
                Seed::Struct,
                kind.field(),
                selection,
                Action::Init {
                    encoding: 4,
                    count: 2,
                },
                true,
            );
        }
    }
    for seed in [Seed::Pointers, Seed::Structs] {
        cases.push(Case {
            kind: Kind::List,
            seed,
            path: "list".into(),
            tag: tag("list"),
            action: Action::Edit,
            accepts: true,
        });
    }
    for encoding in 0..8 {
        for count in [0, 3] {
            cases.push(Case {
                kind: Kind::List,
                seed: Seed::Struct,
                path: "list".into(),
                tag: 0,
                action: Action::Init { encoding, count },
                accepts: true,
            });
        }
    }
    cases.push(Case {
        kind: Kind::Struct,
        seed: Seed::Struct,
        path: "structure".into(),
        tag: 0,
        action: Action::Init {
            encoding: 0,
            count: 0,
        },
        accepts: true,
    });
    cases
}
fn compiled_parent<'a, 'p>(
    message: &'a mut Message,
    path: &'p str,
) -> capnp::Result<(dynamic_struct::Builder<'a>, &'p str)> {
    let mut root = dynamic_value::Builder::from(message.get_root::<pointer_access::Builder>()?)
        .downcast::<dynamic_struct::Builder>();
    let mut parts = path.split('.').peekable();
    loop {
        let part = parts.next().unwrap();
        if parts.peek().is_none() {
            return Ok((root, part));
        }
        root = root.get_named(part)?.downcast();
    }
}
fn loaded_parent<'a, 's: 'a, 'p>(
    message: &'a mut Message,
    loader: &'s SchemaLoader,
    path: &'p str,
) -> capnp::Result<(dynamic::Builder<'a, 's>, &'p str)> {
    let mut root = dynamic::Builder::new(message.get_root()?, schema(loader))?;
    let mut parts = path.split('.').peekable();
    loop {
        let part = parts.next().unwrap();
        if parts.peek().is_none() {
            return Ok((root, part));
        }
        root = root.get_struct(part)?;
    }
}
fn fill_struct(mut value: any_struct::Builder<'_>) {
    value.get_data_section()[0] = 11;
    value.get_data_section()[8] = 77;
    value
        .get_pointer_section()
        .get(0)
        .set_as::<capnp::text::Owned>("known")
        .unwrap();
    value
        .get_pointer_section()
        .get(1)
        .set_as::<capnp::text::Owned>("unknown")
        .unwrap();
}
fn seed(case: &Case, small: bool) -> Message {
    let allocator = message::HeapAllocator::new()
        .first_segment_words(if small { 1 } else { 1024 })
        .allocation_strategy(message::AllocationStrategy::FixedSize);
    let mut message = Message::new(allocator);
    message
        .init_root::<pointer_access::Builder>()
        .set_marker(73);
    // Bypass the requested selection while seeding physical storage.
    set_tag(
        &mut message,
        if case.path.contains('.') {
            0
        } else {
            let t = tag(&case.path);
            if t == field::NO_DISCRIMINANT {
                0
            } else {
                t
            }
        },
    );
    let (root, field) = compiled_parent(&mut message, &case.path).unwrap();
    let mut pointer = root
        .get_named(field)
        .unwrap()
        .downcast::<any_pointer::Builder>();
    match case.seed {
        Seed::Null => (),
        Seed::Struct => fill_struct(pointer.init_as_any_struct(2, 2)),
        Seed::EmptyStruct => {
            pointer.init_as_any_struct(0, 0);
        }
        Seed::Numbers => {
            let mut list = pointer.initn_as::<capnp::primitive_list::Builder<u32>>(3);
            list.set(0, 11);
            list.set(1, 22);
            list.set(2, 33);
        }
        Seed::Pointers => {
            let mut list = pointer.initn_as::<capnp::any_pointer_list::Builder>(2);
            list.reborrow()
                .get(0)
                .set_as::<capnp::text::Owned>("known")
                .unwrap();
            list.get(1).set_as::<capnp::text::Owned>("unknown").unwrap();
        }
        Seed::Structs => {
            let mut list = pointer.init_as_list_of_any_struct(2, 2, 2).unwrap();
            for i in 0..2 {
                fill_struct(list.reborrow().get(i));
            }
        }
        Seed::Bits => {
            pointer
                .initn_as::<capnp::primitive_list::Builder<bool>>(9)
                .set(8, true);
        }
        Seed::Voids => {
            pointer.initn_as::<capnp::primitive_list::Builder<()>>(4);
        }
        Seed::Text => {
            pointer.set_as::<capnp::text::Owned>("hello").unwrap();
        }
        Seed::EmptyList => {
            pointer.initn_as::<capnp::primitive_list::Builder<u32>>(0);
        }
    }
    set_tag(&mut message, case.tag);
    message
}
fn encoding(n: u8) -> ElementSize {
    [
        ElementSize::Void,
        ElementSize::Bit,
        ElementSize::Byte,
        ElementSize::TwoBytes,
        ElementSize::FourBytes,
        ElementSize::EightBytes,
        ElementSize::Pointer,
        ElementSize::InlineComposite,
    ][n as usize]
}
fn edit_struct(value: &mut any_struct::Builder<'_>) {
    value.get_data_section()[0] = 99;
    value
        .get_pointer_section()
        .get(0)
        .set_as::<capnp::text::Owned>("changed")
        .unwrap();
}
fn observe_any(mut value: any_pointer::Builder<'_>, action: Action) -> capnp::Result<String> {
    if matches!(action, Action::Edit) {
        value.set_as::<capnp::text::Owned>("changed")?;
    }
    Ok(match value.get_pointer_type()? {
        any_pointer::PointerType::Null => "null",
        any_pointer::PointerType::Struct => "struct",
        any_pointer::PointerType::List => "list",
        any_pointer::PointerType::Capability => "cap",
    }
    .into())
}
fn observe_struct(mut value: any_struct::Builder<'_>, action: Action) -> String {
    if matches!(action, Action::Edit) {
        edit_struct(&mut value);
    }
    format!(
        "struct {} {}",
        value.as_reader().get_data_section().len(),
        value.as_reader().get_pointer_section().len()
    )
}
fn observe_list(mut value: any_list::Builder<'_>, action: Action) -> capnp::Result<String> {
    if matches!(action, Action::Edit) {
        match value.get_element_size() {
            ElementSize::FourBytes => value
                .reborrow()
                .get_as::<capnp::primitive_list::Owned<u32>>()?
                .set(0, 99),
            ElementSize::Pointer => value
                .reborrow()
                .get_as::<capnp::any_pointer_list::Owned>()?
                .get(0)
                .set_as::<capnp::text::Owned>("changed")?,
            ElementSize::InlineComposite => {
                edit_struct(&mut value.reborrow().get_as_struct_list()?.get(0))
            }
            _ => panic!(),
        }
    }
    Ok(format!(
        "list {} {}",
        value.get_element_size() as u8,
        value.len()
    ))
}
fn apply(
    message: &mut Message,
    loader: &SchemaLoader,
    case: &Case,
    compiled: bool,
) -> capnp::Result<String> {
    if compiled {
        let (root, field) = compiled_parent(message, &case.path)?;
        let pointer = match case.action {
            Action::Init { .. } => root.init_named(field)?,
            _ => root.get_named(field)?,
        }
        .downcast::<any_pointer::Builder>();
        return match (case.kind, case.action) {
            (Kind::Any, action) => observe_any(pointer, action),
            (Kind::Struct, Action::Init { count, .. }) => Ok(observe_struct(
                pointer.init_as_any_struct(count as u16, count as u16),
                case.action,
            )),
            (Kind::Struct, action) => Ok(observe_struct(pointer.get_as()?, action)),
            (Kind::List, Action::Init { encoding: 7, count }) => {
                pointer.init_as_list_of_any_struct(2, 2, count)?;
                Ok(format!("list 7 {count}"))
            }
            (Kind::List, Action::Init { encoding: e, count }) => {
                observe_list(pointer.init_as_any_list(encoding(e), count)?, case.action)
            }
            (Kind::List, action) => observe_list(pointer.get_as()?, action),
        };
    }
    let (root, field) = loaded_parent(message, loader, &case.path)?;
    match (case.kind, case.action) {
        (Kind::Any, Action::Init { .. }) => observe_any(root.init_any_pointer(field)?, case.action),
        (Kind::Any, action) => observe_any(root.get_any_pointer(field)?, action),
        (Kind::Struct, Action::Init { count, .. }) => Ok(observe_struct(
            root.init_any_struct(field, count as u16, count as u16)?,
            case.action,
        )),
        (Kind::Struct, action) => Ok(observe_struct(root.get_any_struct(field)?, action)),
        (Kind::List, Action::Init { encoding: 7, count }) => {
            let list = root.init_any_struct_list(field, 2, 2, count)?;
            assert_eq!(list.len(), count);
            Ok(format!("list 7 {count}"))
        }
        (Kind::List, Action::Init { encoding: e, count }) => {
            observe_list(root.init_any_list(field, encoding(e), count)?, case.action)
        }
        (Kind::List, action) => observe_list(root.get_any_list(field)?, action),
    }
}
fn wire(message: &Message) -> Vec<u8> {
    capnp::Word::words_to_bytes(
        &any_struct::Reader::from_reader(
            message
                .get_root_as_reader::<pointer_access::Reader>()
                .unwrap(),
        )
        .canonicalize()
        .unwrap(),
    )
    .to_vec()
}
#[test]
fn constrained_pointer_views_match_compiled_and_preserve_layouts() {
    let loader = loader();
    for (i, case) in cases().iter().enumerate() {
        let mut loaded = seed(case, i % 2 == 0);
        let before = capnp::serialize::write_message_to_words(&loaded);
        let result = apply(&mut loaded, &loader, case, false);
        assert_eq!(result.is_ok(), case.accepts, "{case:?}: {result:?}");
        if !case.accepts
            || (matches!(case.action, Action::Get)
                && !(matches!(case.kind, Kind::Struct) && matches!(case.seed, Seed::Null)))
        {
            assert_eq!(
                capnp::serialize::write_message_to_words(&loaded),
                before,
                "{case:?}"
            );
        }
        let mut native = seed(case, i % 2 == 0);
        let result_native = apply(&mut native, &loader, case, true);
        assert_eq!(
            result_native.is_ok(),
            case.accepts,
            "{case:?}: {result_native:?}"
        );
        assert_eq!(result.ok(), result_native.ok(), "{case:?}");
        assert_eq!(wire(&loaded), wire(&native), "{case:?}");
    }
}
#[test]
fn constrained_views_retain_unknown_fields_and_borrow_original_storage() {
    let loader = loader();
    for (kind, mode) in [(Kind::Struct, Seed::Struct), (Kind::List, Seed::Structs)] {
        let case = Case {
            kind,
            seed: mode,
            path: kind.field().into(),
            tag: tag(kind.field()),
            action: Action::Edit,
            accepts: true,
        };
        let mut message = seed(&case, true);
        let address = {
            let (root, field) = compiled_parent(&mut message, &case.path).unwrap();
            let pointer = root
                .get_named(field)
                .unwrap()
                .downcast::<any_pointer::Builder>();
            match kind {
                Kind::Struct => pointer
                    .get_as::<any_struct::Builder>()
                    .unwrap()
                    .as_reader()
                    .get_data_section()
                    .as_ptr(),
                Kind::List => pointer
                    .get_as::<capnp::any_struct_list::Builder>()
                    .unwrap()
                    .get(0)
                    .as_reader()
                    .get_data_section()
                    .as_ptr(),
                _ => unreachable!(),
            }
        };
        apply(&mut message, &loader, &case, false).unwrap();
        let (root, field) = compiled_parent(&mut message, &case.path).unwrap();
        let pointer = root
            .get_named(field)
            .unwrap()
            .downcast::<any_pointer::Builder>();
        let value = match kind {
            Kind::Struct => pointer.get_as::<any_struct::Builder>().unwrap(),
            Kind::List => pointer
                .get_as::<capnp::any_struct_list::Builder>()
                .unwrap()
                .get(0),
            _ => unreachable!(),
        };
        assert_eq!(value.as_reader().get_data_section().as_ptr(), address);
        assert_eq!(value.as_reader().get_data_section()[0], 99);
        assert_eq!(value.as_reader().get_data_section()[8], 77);
        assert_eq!(
            value
                .as_reader()
                .get_pointer_section()
                .get(1)
                .get_as::<capnp::text::Reader>()
                .unwrap(),
            "unknown"
        );
    }
}
#[test]
fn pointer_constraints_and_init_bounds_preserve_capability_owners() {
    use capnp::traits::ImbueMut;
    use std::{cell::Cell, rc::Rc};
    struct Server(Rc<Cell<u32>>);
    impl access_capability::Server for Server {}
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let loader = loader();
    for kind in [Kind::Any, Kind::Struct, Kind::List] {
        let drops = Rc::new(Cell::new(0));
        let client: access_capability::Client = capnp_rpc::new_client(Server(drops.clone()));
        let identity = client.client.hook.get_ptr();
        let case = Case {
            kind,
            seed: Seed::Null,
            path: kind.field().into(),
            tag: tag(kind.field()),
            action: Action::Get,
            accepts: true,
        };
        let mut message = seed(&case, true);
        let mut table = vec![];
        {
            let mut root = message.get_root::<pointer_access::Builder>().unwrap();
            root.imbue_mut(&mut table);
            root.init_any().set_as_capability(client.client.hook);
        }
        set_tag(&mut message, tag(kind.field()));
        let before = capnp::serialize::write_message_to_words(&message);
        {
            let mut pointer = message.get_root::<any_pointer::Builder>().unwrap();
            pointer.imbue_mut(&mut table);
            let mut root = dynamic::Builder::new(pointer, schema(&loader)).unwrap();
            // Even unconstrained APIs cannot bypass the field's declared constraint.
            for name in [
                "any",
                "structure",
                "list",
                "cap",
                "typed",
                "text",
                "marker",
                "missing",
            ] {
                if name != "any" {
                    assert!(root.reborrow().get_any_pointer(name).is_err());
                    assert!(root.reborrow().init_any_pointer(name).is_err());
                }
                if name != "structure" {
                    assert!(root.reborrow().get_any_struct(name).is_err());
                    assert!(root.reborrow().init_any_struct(name, 1, 1).is_err());
                }
                if name != "list" {
                    assert!(root.reborrow().get_any_list(name).is_err());
                    assert!(root
                        .reborrow()
                        .init_any_list(name, ElementSize::Byte, 1)
                        .is_err());
                    assert!(root.reborrow().init_any_struct_list(name, 1, 1, 1).is_err());
                }
            }
            assert!(root
                .reborrow()
                .init_any_list("list", ElementSize::InlineComposite, 1)
                .is_err());
            assert!(root
                .reborrow()
                .init_any_list("list", ElementSize::Byte, 1 << 29)
                .is_err());
            assert!(root
                .reborrow()
                .init_any_list("list", ElementSize::Void, u32::MAX)
                .is_err());
            assert!(root
                .reborrow()
                .init_any_struct_list("list", 0, 0, 1 << 30)
                .is_err());
            assert!(root
                .reborrow()
                .init_any_struct_list("list", 2, 0, 1 << 28)
                .is_err());
            assert!(root
                .reborrow()
                .init_any_struct_list("list", u16::MAX, u16::MAX, u32::MAX)
                .is_err());
            match kind {
                Kind::Any => assert_eq!(
                    root.get_any_pointer("any")
                        .unwrap()
                        .get_pointer_type()
                        .unwrap(),
                    any_pointer::PointerType::Capability
                ),
                Kind::Struct => assert!(root.get_any_struct("structure").is_err()),
                Kind::List => assert!(root.get_any_list("list").is_err()),
            }
        }
        assert_eq!(capnp::serialize::write_message_to_words(&message), before);
        assert_eq!(table[0].as_ref().unwrap().get_ptr(), identity);
        assert_eq!(drops.get(), 0);
        // Inactive/unknown selections reject before exposing or replacing a hook.
        for selection in [0, u16::MAX] {
            set_tag(&mut message, selection);
            let before = capnp::serialize::write_message_to_words(&message);
            let mut pointer = message.get_root::<any_pointer::Builder>().unwrap();
            pointer.imbue_mut(&mut table);
            let root = dynamic::Builder::new(pointer, schema(&loader)).unwrap();
            assert!(match kind {
                Kind::Any => root.get_any_pointer("any").is_err(),
                Kind::Struct => root.get_any_struct("structure").is_err(),
                Kind::List => root.get_any_list("list").is_err(),
            });
            assert_eq!(capnp::serialize::write_message_to_words(&message), before);
            assert_eq!(drops.get(), 0);
        }
        let mut pointer = message.get_root::<any_pointer::Builder>().unwrap();
        pointer.imbue_mut(&mut table);
        let root = dynamic::Builder::new(pointer, schema(&loader)).unwrap();
        match kind {
            Kind::Any => {
                assert!(root.init_any_pointer("any").unwrap().is_null());
            }
            Kind::Struct => {
                assert_eq!(
                    root.init_any_struct("structure", 1, 1)
                        .unwrap()
                        .as_reader()
                        .get_data_section(),
                    &[0; 8]
                );
            }
            Kind::List => {
                assert_eq!(
                    root.init_any_list("list", ElementSize::FourBytes, 2)
                        .unwrap()
                        .get_as::<capnp::primitive_list::Owned<u32>>()
                        .unwrap()
                        .get(0),
                    0
                );
            }
        }
        assert!(table[0].is_none());
        assert_eq!(drops.get(), 1);
    }
    // A concrete generic binding cannot be treated as its unbound AnyPointer slot.
    let mut message = Message::new_default();
    message
        .init_root::<pointer_access::Builder>()
        .init_bound_text();
    let (mut root, field) = loaded_parent(&mut message, &loader, "boundText.body.value").unwrap();
    assert!(root.reborrow().get_any_pointer(field).is_err());
    assert!(root.init_any_pointer(field).is_err());
}

#[test]
fn constrained_views_keep_descendant_capabilities_and_owned_extractions() {
    use capnp::traits::ImbueMut;
    use std::{cell::Cell, rc::Rc};
    struct Server(Rc<Cell<u32>>);
    impl access_capability::Server for Server {}
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let loader = loader();
    for kind in [Kind::Struct, Kind::List] {
        let case = Case {
            kind,
            seed: match kind {
                Kind::Struct => Seed::Struct,
                _ => Seed::Structs,
            },
            path: kind.field().into(),
            tag: tag(kind.field()),
            action: Action::Get,
            accepts: true,
        };
        let mut message = seed(&case, true);
        let drops = Rc::new(Cell::new(0));
        let client: access_capability::Client = capnp_rpc::new_client(Server(drops.clone()));
        let identity = client.client.hook.get_ptr();
        let mut table = vec![];
        {
            let mut root = message.get_root::<pointer_access::Builder>().unwrap();
            root.imbue_mut(&mut table);
            let root = dynamic_value::Builder::from(root).downcast::<dynamic_struct::Builder>();
            let pointer = root
                .get_named(kind.field())
                .unwrap()
                .downcast::<any_pointer::Builder>();
            let mut value = match kind {
                Kind::Struct => pointer.get_as::<any_struct::Builder>().unwrap(),
                _ => pointer
                    .get_as::<capnp::any_struct_list::Builder>()
                    .unwrap()
                    .get(0),
            };
            value
                .get_pointer_section()
                .get(1)
                .set_as_capability(client.client.hook);
        }
        let before = capnp::serialize::write_message_to_words(&message);
        let retained: access_capability::Client = {
            let mut pointer = message.get_root::<any_pointer::Builder>().unwrap();
            pointer.imbue_mut(&mut table);
            let root = dynamic::Builder::new(pointer, schema(&loader)).unwrap();
            let value = match kind {
                Kind::Struct => root.get_any_struct("structure").unwrap(),
                _ => root
                    .get_any_list("list")
                    .unwrap()
                    .get_as_struct_list()
                    .unwrap()
                    .get(0),
            };
            value
                .as_reader()
                .get_pointer_section()
                .get(1)
                .get_as_capability()
                .unwrap()
        };
        assert_eq!(retained.client.hook.get_ptr(), identity);
        assert_eq!(table[0].as_ref().unwrap().get_ptr(), identity);
        assert_eq!(capnp::serialize::write_message_to_words(&message), before);
        {
            let mut pointer = message.get_root::<any_pointer::Builder>().unwrap();
            pointer.imbue_mut(&mut table);
            let root = dynamic::Builder::new(pointer, schema(&loader)).unwrap();
            match kind {
                Kind::Struct => {
                    root.init_any_struct("structure", 0, 0).unwrap();
                }
                _ => {
                    root.init_any_struct_list("list", 0, 0, 0).unwrap();
                }
            }
        }
        assert!(table[0].is_none());
        assert_eq!(drops.get(), 0);
        drop(retained);
        assert_eq!(drops.get(), 1);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn pointer_views_match_pinned_cpp() {
    use reproto_test_support::verification::{command, cpp, root, run};
    let build = cpp::build(&["capnpc", "capnp_tool"]).unwrap();
    let logs = root().join("target/verification/dynamic-pointers");
    std::fs::create_dir_all(&logs).unwrap();
    let binary = logs.join("pointers");
    run(
        command("g++")
            .args([
                "-std=c++23",
                "-Ivendor/capnproto/c++/src",
                "tests/cpp/dynamic-pointers.c++",
            ])
            .arg(build.join("c++/src/capnp/libcapnpc.a"))
            .arg(build.join("c++/src/capnp/libcapnp.a"))
            .arg(build.join("c++/src/kj/libkj.a"))
            .args(["-pthread", "-o"])
            .arg(&binary),
        &logs.join("build.log"),
        0,
    )
    .unwrap();
    let request = command(build.join("c++/src/capnp/capnp"))
        .args([
            "compile",
            "-o-",
            "-Ivendor/capnproto/c++/src",
            "--src-prefix=schemas",
            "schemas/presence.capnp",
        ])
        .output()
        .unwrap();
    assert!(
        request.status.success(),
        "{}",
        String::from_utf8_lossy(&request.stderr)
    );
    let schema_path = logs.join("schema.bin");
    std::fs::write(&schema_path, request.stdout).unwrap();
    let loader = loader();
    let cases = cases();
    for (i, case) in cases.iter().enumerate() {
        let mut message = seed(case, i % 2 == 0);
        let seed_path = logs.join(format!("{i}.seed"));
        std::fs::write(
            &seed_path,
            capnp::serialize::write_message_to_words(&message),
        )
        .unwrap();
        let (action, encoding, count) = match case.action {
            Action::Get => ("get", 0, 0),
            Action::Edit => ("edit", 0, 0),
            Action::Init { encoding, count } => ("init", encoding, count),
        };
        let expected = run(
            command(&binary)
                .arg(&schema_path)
                .arg(schema(&loader).id().to_string())
                .arg(&seed_path)
                .arg(action)
                .arg(case.kind.field())
                .arg(&case.path)
                .arg(encoding.to_string())
                .arg(count.to_string()),
            &logs.join(format!("{i}.out")),
            0,
        )
        .unwrap();
        let result = apply(&mut message, &loader, case, false);
        let status = match result {
            Ok(value) => format!("ok {value}"),
            Err(_) => "error".into(),
        };
        let bytes: String = wire(&message).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(format!("{status} {bytes}\n"), expected, "{case:?}");
    }
    std::fs::write(logs.join("summary.txt"), format!("{} pointer view/init results, physical layouts and canonical wire states match pinned C++ ({} rejected).\n", cases.len(), cases.iter().filter(|c| !c.accepts).count())).unwrap();
}
