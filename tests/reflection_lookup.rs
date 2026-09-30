use capnp::{
    introspect::{Introspect, TypeVariant},
    message,
    schema::{EnumSchema, InterfaceSchema},
    schema_capnp::{brand, node},
    schema_loader::{SchemaLoader, Type},
};
use reproto_test_support::reflection_lookup_capnp as fixture;
mod reflection_lookup {
    pub mod verification;
}
// Avoid C++'s reserved default-schema IDs (notably InterfaceSchema ID 3).
const ID_BASE: u64 = 0xa100_0000_0000_0000;

fn enum_schema() -> EnumSchema {
    let TypeVariant::Enum(raw) = fixture::Tone::introspect().which() else {
        panic!()
    };
    raw.into()
}
fn bound<'a>(loader: &'a SchemaLoader, id: u64, text: bool) -> capnp::schema_loader::Schema<'a> {
    let mut message = message::Builder::new_default();
    let mut scope = message.init_root::<brand::Builder>().init_scopes(1).get(0);
    scope.set_scope_id(id);
    let mut ty = scope.init_bind(1).get(0).init_type();
    if text {
        ty.set_text(());
    } else {
        ty.set_data(());
    }
    loader
        .get(id)
        .unwrap()
        .bind(message.get_root_as_reader().unwrap(), None)
        .unwrap()
}
#[test]
fn optional_lookup_preserves_declaring_interface_and_applied_brand() {
    let diamond = fixture::diamond::Client::<capnp::text::Owned>::schema();
    let own = fixture::override_::Client::<capnp::text::Owned>::schema();
    let base = fixture::root::Client::<capnp::text::Owned>::schema();
    let wrong = fixture::root::Client::<capnp::data::Owned>::schema();
    let mut loader = SchemaLoader::default();
    loader
        .load_compiled_type_and_dependencies::<fixture::override_::Owned<capnp::text::Owned>>()
        .unwrap();
    for (compiled, owner) in [(diamond, base), (own, own)] {
        let loaded = bound(&loader, compiled.get_proto().get_id(), true);
        for name in ["", "missing", "Match", "match\0", "☃"] {
            assert!(compiled.find_method_by_name(name).unwrap().is_none());
            assert!(compiled.get_method_by_name(name).is_err());
            assert!(loaded.find_method(name).unwrap().is_none());
            assert!(loaded.method(name).is_err());
        }
        let method = compiled.find_method_by_name("match").unwrap().unwrap();
        assert!(method.get_containing_interface().equals(owner).unwrap());
        assert!(method
            .get_param_type()
            .get_field_by_name("value")
            .unwrap()
            .get_type()
            .equals(capnp::text::Owned::introspect())
            .unwrap());
        let method = loaded.find_method("match").unwrap().unwrap();
        assert_eq!(method.parent().id(), owner.get_proto().get_id());
        assert!(matches!(
            method
                .params()
                .unwrap()
                .field("value")
                .unwrap()
                .get_type()
                .unwrap(),
            Type::Text
        ));
        assert!(loaded
            .method("match")
            .unwrap()
            .parent()
            .equals(&method.parent()));
        assert!(compiled
            .find_superclass(compiled.get_proto().get_id())
            .unwrap()
            .unwrap()
            .equals(compiled)
            .unwrap());
        assert!(loaded
            .find_superclass(loaded.id())
            .unwrap()
            .unwrap()
            .equals(&loaded));
        let inherited = loaded
            .find_superclass(base.get_proto().get_id())
            .unwrap()
            .unwrap();
        assert!(matches!(
            inherited
                .method("match")
                .unwrap()
                .params()
                .unwrap()
                .field("value")
                .unwrap()
                .get_type()
                .unwrap(),
            Type::Text
        ));
        assert!(loaded.extends(&inherited).unwrap());
        assert!(!loaded
            .extends(&bound(&loader, base.get_proto().get_id(), false))
            .unwrap());
        assert!(compiled.extends(base).unwrap());
        assert!(!compiled.extends(wrong).unwrap());
        assert!(compiled.find_superclass(u64::MAX).unwrap().is_none());
        assert!(loaded.find_superclass(u64::MAX).unwrap().is_none());
    }
}
#[test]
fn enum_lookup_is_optional_exact_and_returns_wire_ordinal() {
    let compiled = enum_schema();
    let mut loader = SchemaLoader::default();
    loader
        .load_compiled_type_and_dependencies::<fixture::Tone>()
        .unwrap();
    let loaded = loader.get(compiled.get_proto().get_id()).unwrap();
    for (ordinal, name) in ["zulu", "alpha", "middle"].into_iter().enumerate() {
        assert_eq!(
            compiled
                .find_enumerant_by_name(name)
                .unwrap()
                .unwrap()
                .get_ordinal(),
            ordinal as u16
        );
        assert_eq!(
            compiled.get_enumerant_by_name(name).unwrap().get_ordinal(),
            ordinal as u16
        );
        assert_eq!(
            loaded.find_enumerant(name).unwrap().unwrap().index(),
            ordinal as u16
        );
        assert_eq!(loaded.enumerant(name).unwrap().index(), ordinal as u16);
    }
    for name in ["", "missing", "Alpha", "alpha\0", "☃"] {
        assert!(compiled.find_enumerant_by_name(name).unwrap().is_none());
        assert!(compiled.get_enumerant_by_name(name).is_err());
        assert!(loaded.find_enumerant(name).unwrap().is_none());
        assert!(loaded.enumerant(name).is_err());
    }
    assert!(loaded.find_method("match").is_err());
    assert!(loaded.find_superclass(loaded.id()).is_err());
    assert!(loaded.extends(&loaded).is_err());
}
#[test]
fn compiled_and_loaded_searches_count_all_visits_with_exact_64_boundary() {
    let schemas = [
        fixture::limit63::Client::schema(),
        fixture::limit64::Client::schema(),
        fixture::limit65::Client::schema(),
    ];
    let other = fixture::right::Client::<capnp::text::Owned>::schema();
    let mut loader = SchemaLoader::default();
    loader
        .load_compiled_type_and_dependencies::<fixture::limit65::Owned>()
        .unwrap();
    loader
        .load_compiled_type_and_dependencies::<fixture::right::Owned<capnp::text::Owned>>()
        .unwrap();
    for (index, compiled) in schemas.into_iter().enumerate() {
        let loaded = loader.get(compiled.get_proto().get_id()).unwrap();
        let method = compiled.find_method_by_name("missing");
        let superclass = compiled.find_superclass(u64::MAX);
        let extends = compiled.extends(other);
        assert_eq!(method.is_err(), index == 2);
        assert_eq!(superclass.is_err(), index == 2);
        assert_eq!(extends.is_err(), index == 2);
        assert_eq!(loaded.find_method("missing").is_err(), index == 2);
        assert_eq!(loaded.find_superclass(u64::MAX).is_err(), index == 2);
        assert_eq!(
            loaded
                .extends(&loader.get(other.get_proto().get_id()).unwrap())
                .is_err(),
            index == 2
        );
        if index < 2 {
            assert!(method.unwrap().is_none());
            assert!(superclass.unwrap().is_none());
            assert!(!extends.unwrap());
        }
        let leaf = fixture::wide1::Client::schema();
        assert!(compiled.extends(leaf).unwrap());
        assert!(loaded
            .find_superclass(leaf.get_proto().get_id())
            .unwrap()
            .is_some());
    }
}

fn interface_node(
    id: u64,
    parents: &[u64],
    matched: bool,
) -> message::Builder<message::HeapAllocator> {
    let mut message = message::Builder::new_default();
    let mut n = message.init_root::<node::Builder>();
    n.set_id(ID_BASE + id);
    n.set_display_name("test:Interface");
    n.set_display_name_prefix_length(5);
    let mut i = n.init_interface();
    let mut supers = i.reborrow().init_superclasses(parents.len() as u32);
    for (k, id) in parents.iter().enumerate() {
        supers.reborrow().get(k as u32).set_id(ID_BASE + id);
    }
    if matched {
        let mut m = i.init_methods(1).get(0);
        m.set_name("match");
        m.set_param_struct_type(ID_BASE + 100);
        m.set_result_struct_type(ID_BASE + 100);
    }
    message
}
fn shape(graph: u64, n: u64) -> (Vec<u64>, bool) {
    match graph {
        1 | 2 => (
            match n {
                1 => vec![2, 3],
                2 => vec![4],
                _ => vec![],
            },
            n == 3 || n == 4 || (graph == 2 && n == 1),
        ),
        3 => (
            if n == 1 {
                vec![2, 8]
            } else if n < 7 {
                vec![n + 1, n + 1]
            } else {
                vec![]
            },
            n == 8,
        ),
        4 => (
            match n {
                1 => vec![2, 3],
                2 | 3 => vec![4],
                _ => vec![],
            },
            n == 4,
        ),
        5 | 6 => (
            if n < graph + 1 {
                vec![n + 1, n + 1]
            } else {
                vec![]
            },
            false,
        ),
        _ => panic!(),
    }
}
fn graph_loader(graph: u64) -> SchemaLoader {
    let nodes: Vec<_> = (1..=8)
        .map(|n| {
            let (parents, matched) = shape(graph, n);
            interface_node(n, &parents, matched)
        })
        .collect();
    let mut loader = SchemaLoader::default();
    loader
        .load_batch(
            nodes
                .iter()
                .map(|m| m.get_root_as_reader::<node::Reader>().unwrap()),
        )
        .unwrap();
    loader
}
fn query(loader: &SchemaLoader, operation: u64, needle: u64) -> u64 {
    let root = loader.get(ID_BASE + 1).unwrap();
    if operation == 0 {
        let name = if needle == 1 { "match" } else { "missing" };
        let optional = root.find_method(name);
        let required = root.method(name);
        match optional {
            Ok(Some(m)) => {
                assert_eq!(required.unwrap().parent().id(), m.parent().id());
                m.parent().id() - ID_BASE
            }
            Ok(None) => {
                assert!(required.is_err());
                0
            }
            Err(e) => {
                assert!(e.extra.contains("traversal limit"), "{e}");
                assert!(required.err().unwrap().extra.contains("traversal limit"));
                9
            }
        }
    } else {
        let target = if needle == 0 {
            u64::MAX
        } else {
            ID_BASE + needle
        };
        match root.find_superclass(target) {
            Ok(Some(s)) => {
                assert!(root.extends(&s).unwrap());
                s.id() - ID_BASE
            }
            Ok(None) => 0,
            Err(e) => {
                assert!(e.extra.contains("traversal limit"), "{e}");
                9
            }
        }
    }
}
#[test]
fn malformed_inheritance_errors_are_not_absence_or_later_branch_matches() {
    let mut loader = graph_loader(3);
    assert_eq!(query(&loader, 0, 1), 9);
    assert_eq!(query(&loader, 1, 8), 9);
    // An own member is usable without traversing the excessive branch.
    let own = interface_node(1, &[2, 8], true);
    loader.load(own.get_root_as_reader().unwrap()).unwrap();
    assert_eq!(query(&loader, 0, 1), 1);
    assert_eq!(query(&loader, 1, 1), 1);
    // Cycles are rejected during loading, before any lookup can use them.
    let cycle = interface_node(2, &[1], false);
    assert!(loader.load(cycle.get_root_as_reader().unwrap()).is_err());
    assert_eq!(query(&loader, 0, 1), 1);
}
#[test]
fn display_names_use_the_declared_byte_prefix_without_reparsing_names() {
    let schema = fixture::scope::nested::Owned::<capnp::text::Owned>::introspect()
        .as_struct_schema()
        .unwrap();
    let mut loader = SchemaLoader::default();
    loader
        .load_compiled_type_and_dependencies::<fixture::scope::nested::Owned<capnp::text::Owned>>()
        .unwrap();
    assert_eq!(
        schema.get_short_display_name().unwrap().to_str().unwrap(),
        "Nested"
    );
    assert_eq!(
        schema.get_unqualified_name().unwrap(),
        schema.get_short_display_name().unwrap()
    );
    assert_eq!(
        loader
            .get(schema.get_proto().get_id())
            .unwrap()
            .short_display_name()
            .unwrap(),
        schema.get_short_display_name().unwrap()
    );
    assert_eq!(
        enum_schema()
            .get_short_display_name()
            .unwrap()
            .to_str()
            .unwrap(),
        "Tone"
    );
    let interface = fixture::diamond::Client::<capnp::text::Owned>::schema();
    assert_eq!(
        interface.get_unqualified_name().unwrap().to_str().unwrap(),
        "Diamond"
    );
    for name in ["", "path:Outer.Inner", "p:é.Name"] {
        for prefix in 0..=name.len() + 1 {
            let mut message = interface_node(900, &[], false);
            let mut n: node::Builder = message.get_root().unwrap();
            n.set_display_name(name);
            n.set_display_name_prefix_length(prefix as u32);
            let mut loader = SchemaLoader::default();
            let loaded = loader.load(message.get_root_as_reader().unwrap()).unwrap();
            let actual = loaded.short_display_name();
            if prefix > name.len() {
                assert!(actual.is_err());
                assert!(loaded.unqualified_name().is_err());
            } else {
                let actual = actual.unwrap();
                assert_eq!(actual.as_bytes(), &name.as_bytes()[prefix..]);
                assert_eq!(loaded.unqualified_name().unwrap(), actual);
                assert_eq!(actual.to_str().is_err(), !name.is_char_boundary(prefix));
            }
        }
    }
}

#[test]
fn only_visited_superclass_brands_are_resolved_and_errors_propagate() {
    let mut root = interface_node(1, &[2, 3], false);
    let n: node::Builder = root.get_root().unwrap();
    let node::Interface(interface) = n.which().unwrap() else {
        panic!()
    };
    let mut scope = interface
        .get_superclasses()
        .unwrap()
        .get(1)
        .init_brand()
        .init_scopes(1)
        .get(0);
    scope.set_scope_id(ID_BASE + 3);
    scope.init_bind(1).get(0).init_type().set_text(());
    let first = interface_node(2, &[], true);
    let bad = interface_node(3, &[], false); // Has no generic parameters.
    let mut loader = SchemaLoader::default();
    loader
        .load_batch([
            root.get_root_as_reader::<node::Reader>().unwrap(),
            first.get_root_as_reader().unwrap(),
            bad.get_root_as_reader().unwrap(),
        ])
        .unwrap();
    let root = loader.get(ID_BASE + 1).unwrap();
    assert!(root.superclasses().is_err()); // Asking for all parents reaches the bad brand.
    assert_eq!(
        root.find_method("match").unwrap().unwrap().parent().id(),
        ID_BASE + 2
    );
    assert_eq!(
        root.find_superclass(ID_BASE + 2).unwrap().unwrap().id(),
        ID_BASE + 2
    );
    assert!(root.extends(&loader.get(ID_BASE + 2).unwrap()).unwrap());
    for error in [
        root.find_method("missing").err().unwrap(),
        root.method("missing").err().unwrap(),
        root.find_superclass(u64::MAX).err().unwrap(),
        root.extends(&loader.get(ID_BASE + 3).unwrap())
            .err()
            .unwrap(),
    ] {
        assert!(error.extra.contains("too many brand arguments"), "{error}");
    }
}

fn compiled_interface(message: &message::Builder<message::HeapAllocator>) -> InterfaceSchema {
    use capnp::{
        introspect::{Brand, RawBrandedInterfaceSchema},
        private::arena::GeneratedCodeArena,
    };
    let segments = message.get_segments_for_output();
    assert_eq!(segments.len(), 1);
    let mut words = capnp::Word::allocate_zeroed_vec(segments[0].len() / 8);
    capnp::Word::words_to_bytes_mut(&mut words).copy_from_slice(segments[0]);
    // Compiled metadata requires static storage. These few deliberately malformed
    // test nodes stand in for custom generated metadata, without unchecked reads.
    let arena = Box::leak(Box::new(GeneratedCodeArena::new(Box::leak(
        words.into_boxed_slice(),
    ))));
    InterfaceSchema::new(RawBrandedInterfaceSchema {
        arena,
        brand: Brand::EMPTY,
        method_types: |_| panic!("name inspection must not resolve methods"),
        superclass: |_| panic!("name inspection must not traverse inheritance"),
    })
}
#[test]
fn compiled_name_helpers_validate_offsets_and_leave_utf8_validation_to_reader() {
    for (name, prefix) in [
        (b"ab".as_slice(), 3),
        (b"ab", u32::MAX),
        (b"", 0),
        (b"\xffx", 0),
        (b"\xffx", 1),
    ] {
        let mut message = interface_node(900, &[], false);
        let mut n: node::Builder = message.get_root().unwrap();
        n.set_display_name(capnp::text::Reader(name));
        n.set_display_name_prefix_length(prefix);
        let schema = compiled_interface(&message);
        if let Some(expected) = name.get(prefix as usize..) {
            assert_eq!(
                schema.get_short_display_name().unwrap().as_bytes(),
                expected
            );
            assert_eq!(schema.get_unqualified_name().unwrap().as_bytes(), expected);
        } else {
            assert!(schema.get_short_display_name().is_err());
            assert!(schema.get_unqualified_name().is_err());
        }
    }
}
