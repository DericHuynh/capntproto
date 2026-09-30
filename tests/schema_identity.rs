use capnp::{
    introspect::{Brand, Introspect, RawBrandedStructSchema, TypeVariant},
    message,
    schema::{MemberIdentity, StructSchema},
    schema_capnp::brand,
    schema_loader::{Schema, SchemaLoader, Type},
};
use reproto_test_support::reflection_lookup_capnp as fixture;
use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{BuildHasherDefault, Hash, Hasher},
};
mod schema_identity {
    pub mod verification;
}
fn hash(value: &impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}
fn cache<T: capnp::traits::Owned>() -> StructSchema {
    fixture::cache::Owned::<T>::introspect()
        .as_struct_schema()
        .unwrap()
}
fn loader() -> SchemaLoader {
    let mut loader = SchemaLoader::default();
    loader
        .load_compiled_type_and_dependencies::<fixture::cache::Owned<capnp::text::Owned>>()
        .unwrap();
    loader
        .load_compiled_type_and_dependencies::<fixture::diamond::Owned<capnp::text::Owned>>()
        .unwrap();
    loader
        .load_compiled_type_and_dependencies::<fixture::Tone>()
        .unwrap();
    loader
        .load_compiled_type_and_dependencies::<fixture::implicit::Owned>()
        .unwrap();
    loader
}
fn bound<'a>(loader: &'a SchemaLoader, id: u64, mode: u32) -> Schema<'a> {
    if mode == 0 {
        return loader.get(id).unwrap();
    }
    if mode == 1 {
        return loader.get_unbound(id).unwrap();
    }
    let mut message = message::Builder::new_default();
    let mut scope = message.init_root::<brand::Builder>().init_scopes(1).get(0);
    scope.set_scope_id(id);
    match mode {
        2 => {
            scope.init_bind(0);
        }
        3 => scope.init_bind(1).get(0).set_unbound(()),
        4 => scope.init_bind(1).get(0).init_type().set_text(()),
        5 => scope.init_bind(1).get(0).init_type().set_data(()),
        6 => scope
            .init_bind(1)
            .get(0)
            .init_type()
            .init_list()
            .init_element_type()
            .set_text(()),
        _ => panic!(),
    }
    loader
        .get(id)
        .unwrap()
        .bind(message.get_root_as_reader().unwrap(), None)
        .unwrap()
}
fn foreign_cache() -> StructSchema {
    let TypeVariant::Struct(raw) =
        fixture::cache::Owned::<capnp::text::Owned>::introspect().which()
    else {
        panic!()
    };
    // A second compiled metadata owner with the same schema ID and bytes.
    TypeVariant::Struct(RawBrandedStructSchema {
        generic: Box::leak(Box::new(*raw.generic)),
        ..raw
    })
    .into_type_with_brand(Brand::new(1, |_| capnp::text::Owned::introspect()))
    .as_struct_schema()
    .unwrap()
}
struct Catalog<'a> {
    a: &'a SchemaLoader,
    b: &'a SchemaLoader,
    foreign: StructSchema,
}
impl<'a> Catalog<'a> {
    fn key(&self, index: u64, compiled: bool) -> MemberIdentity<'a> {
        let TypeVariant::Enum(en) = fixture::Tone::introspect().which() else {
            panic!()
        };
        let en = capnp::schema::EnumSchema::from(en);
        if compiled {
            match index {
                1 => cache::<capnp::text::Owned>()
                    .get_field_by_name("first")
                    .unwrap()
                    .identity()
                    .unwrap(),
                2 => cache::<capnp::text::Owned>()
                    .get_fields()
                    .unwrap()
                    .get(0)
                    .identity()
                    .unwrap(),
                3 => cache::<capnp::text::Owned>()
                    .get_fields()
                    .unwrap()
                    .get(1)
                    .identity()
                    .unwrap(),
                4 => cache::<capnp::data::Owned>()
                    .get_fields()
                    .unwrap()
                    .get(0)
                    .identity()
                    .unwrap(),
                5 => self
                    .foreign
                    .get_fields()
                    .unwrap()
                    .get(0)
                    .identity()
                    .unwrap(),
                6 => fixture::root::Client::<capnp::text::Owned>::schema()
                    .get_method_by_name("match")
                    .unwrap()
                    .identity()
                    .unwrap(),
                7 => fixture::diamond::Client::<capnp::text::Owned>::schema()
                    .get_method_by_name("match")
                    .unwrap()
                    .identity()
                    .unwrap(),
                8 | 9 => en
                    .get_enumerants()
                    .unwrap()
                    .get((index - 8) as u16)
                    .identity()
                    .unwrap(),
                _ => panic!(),
            }
        } else {
            match index {
                1..=5 => {
                    let s = bound(
                        if index == 5 { self.b } else { self.a },
                        cache::<capnp::text::Owned>().get_proto().get_id(),
                        if index == 4 { 5 } else { 4 },
                    );
                    if index == 1 {
                        s.field("first").unwrap().identity().unwrap()
                    } else {
                        s.fields().unwrap()[usize::from(index == 3)]
                            .identity()
                            .unwrap()
                    }
                }
                6 | 7 => {
                    let id = if index == 6 {
                        fixture::root::Client::<capnp::text::Owned>::schema()
                            .get_proto()
                            .get_id()
                    } else {
                        fixture::diamond::Client::<capnp::text::Owned>::schema()
                            .get_proto()
                            .get_id()
                    };
                    bound(self.a, id, 4)
                        .method("match")
                        .unwrap()
                        .identity()
                        .unwrap()
                }
                8 | 9 => self
                    .a
                    .get(en.get_proto().get_id())
                    .unwrap()
                    .enumerants()
                    .unwrap()[(index - 8) as usize]
                    .identity()
                    .unwrap(),
                _ => panic!(),
            }
        }
    }
}
fn canonical(index: u64) -> u64 {
    match index {
        2 => 1,
        7 => 6,
        _ => index,
    }
}
#[derive(Default)]
struct CollisionHasher;
impl Hasher for CollisionHasher {
    fn finish(&self) -> u64 {
        0
    }
    fn write(&mut self, _: &[u8]) {}
}
type CollisionMap<K, V> = HashMap<K, V, BuildHasherDefault<CollisionHasher>>;
#[test]
fn member_keys_distinguish_owner_brand_and_index_and_survive_hash_collisions() {
    let a = loader();
    let b = a.clone();
    let catalog = Catalog {
        a: &a,
        b: &b,
        foreign: foreign_cache(),
    };
    for compiled in [false, true] {
        for i in 1..=9 {
            for j in 1..=9 {
                let left = catalog.key(i, compiled);
                let right = catalog.key(j, compiled);
                let equal = canonical(i) == canonical(j);
                assert_eq!(left == right, equal, "compiled={compiled}, {i}/{j}");
                if equal {
                    assert_eq!(hash(&left), hash(&right));
                }
                let mut map = CollisionMap::default();
                map.insert(left, 73);
                assert_eq!(map.get(&right).copied(), equal.then_some(73));
                map.insert(right, 91);
                assert_eq!(map.len(), if equal { 1 } else { 2 });
            }
        }
    }
    assert!(catalog
        .foreign
        .equals(cache::<capnp::text::Owned>())
        .unwrap());
    assert_ne!(
        catalog.foreign.identity().unwrap(),
        cache::<capnp::text::Owned>().identity().unwrap()
    );
    let field = cache::<capnp::text::Owned>()
        .get_field_by_name("first")
        .unwrap();
    assert_eq!(
        field.identity().unwrap().parent(),
        &field.get_containing_struct().identity().unwrap()
    );
    assert_eq!(
        field.identity().unwrap().kind(),
        capnp::schema::MemberKind::Field
    );
    assert_eq!(field.identity().unwrap().index(), 0);
    // Registering compiled types never aliases mutable loader storage with static metadata.
    assert_ne!(catalog.key(1, true), catalog.key(1, false));
}
#[test]
fn loaded_identity_preserves_explicit_defaults_unbound_scopes_and_nested_arguments() {
    let loader = loader();
    let id = cache::<capnp::text::Owned>().get_proto().get_id();
    for a in 0..7 {
        for b in 0..7 {
            let left = bound(&loader, id, a);
            let right = bound(&loader, id, b);
            let l = left.identity().unwrap();
            let r = right.identity().unwrap();
            assert_eq!(l == r, left.equals(&right), "{a}/{b}");
            if l == r {
                assert_eq!(hash(&l), hash(&r));
            }
            assert_eq!(l.node_id(), id);
            assert_eq!(
                left.generic().identity().unwrap(),
                loader.get(id).unwrap().identity().unwrap()
            );
        }
    }
    assert_ne!(
        cache::<capnp::text::Owned>().identity().unwrap(),
        cache::<capnp::data::Owned>().identity().unwrap()
    );
    assert_ne!(
        cache::<capnp::list_list::Owned<capnp::text_list::Owned>>()
            .identity()
            .unwrap(),
        cache::<capnp::text_list::Owned>().identity().unwrap()
    );
    assert_ne!(
        cache::<fixture::root::Owned<capnp::text::Owned>>()
            .identity()
            .unwrap(),
        cache::<fixture::root::Owned<capnp::data::Owned>>()
            .identity()
            .unwrap()
    );
}
#[test]
fn implicit_method_arguments_specialize_types_without_changing_declaration_identity() {
    let loader = loader();
    let s = loader
        .get(fixture::implicit::Client::schema().get_proto().get_id())
        .unwrap();
    let method = s.method("call").unwrap();
    let text = method.bind_implicit(&[Type::Text]).unwrap();
    let data = method.bind_implicit(&[Type::Data]).unwrap();
    assert_eq!(method.identity().unwrap(), text.identity().unwrap());
    assert_eq!(text.identity().unwrap(), data.identity().unwrap());
    assert!(!text.params().unwrap().equals(&data.params().unwrap()));
    assert!(matches!(
        text.params()
            .unwrap()
            .field("value")
            .unwrap()
            .get_type()
            .unwrap(),
        Type::Text
    ));
}

fn branded_cache(get: fn(u16) -> capnp::introspect::Type) -> StructSchema {
    let TypeVariant::Struct(raw) =
        fixture::cache::Owned::<capnp::text::Owned>::introspect().which()
    else {
        panic!()
    };
    TypeVariant::Struct(raw)
        .into_type_with_brand(Brand::new(1, get))
        .as_struct_schema()
        .unwrap()
}
fn recursive_argument(_: u16) -> capnp::introspect::Type {
    let TypeVariant::Struct(raw) =
        fixture::cache::Owned::<capnp::text::Owned>::introspect().which()
    else {
        panic!()
    };
    TypeVariant::Struct(raw).into_type_with_brand(Brand::new(1, recursive_argument))
}
thread_local! {
    static USE_DATA: std::cell::Cell<bool> = const {std::cell::Cell::new(false)};
    static CALLBACKS: std::cell::Cell<u32> = const {std::cell::Cell::new(0)};
}
fn changing_argument(_: u16) -> capnp::introspect::Type {
    CALLBACKS.set(CALLBACKS.get() + 1);
    if USE_DATA.get() {
        capnp::data::Owned::introspect()
    } else {
        capnp::text::Owned::introspect()
    }
}
#[test]
fn compiled_keys_snapshot_values_and_reject_incomplete_or_cyclic_metadata() {
    let original = cache::<capnp::text::Owned>();
    let alternate = branded_cache(|_| capnp::text::Owned::introspect());
    assert_eq!(original.identity().unwrap(), alternate.identity().unwrap());
    let TypeVariant::Struct(raw) =
        fixture::cache::Owned::<capnp::text::Owned>::introspect().which()
    else {
        panic!()
    };
    assert!(StructSchema::new(raw).identity().is_err());
    assert!(branded_cache(recursive_argument).identity().is_err());

    USE_DATA.set(false);
    CALLBACKS.set(0);
    let changing = branded_cache(changing_argument);
    let text = changing.identity().unwrap();
    assert_eq!(CALLBACKS.get(), 1);
    let initial_hash = hash(&text);
    USE_DATA.set(true);
    assert_eq!(hash(&text), initial_hash);
    assert_eq!(text, original.identity().unwrap());
    assert_eq!(
        CALLBACKS.get(),
        1,
        "equality and hashing must not execute callbacks"
    );
    let data = changing.identity().unwrap();
    assert_ne!(text, data);
    assert_eq!(CALLBACKS.get(), 2);
    assert_eq!(data, cache::<capnp::data::Owned>().identity().unwrap());
}

#[test]
fn loaded_key_construction_has_an_exact_metadata_budget() {
    const ID: u64 = 0xa200_0000_0000_0000;
    let mut node = message::Builder::new_default();
    let mut n = node.init_root::<capnp::schema_capnp::node::Builder>();
    n.set_id(ID);
    n.set_is_generic(true);
    let mut params = n.reborrow().init_parameters(127);
    for i in 0..127 {
        params.reborrow().get(i).set_name(format!("T{i}"));
    }
    n.init_struct();
    let mut loader = SchemaLoader::default();
    loader.load(node.get_root_as_reader().unwrap()).unwrap();
    for count in [125, 126, 127] {
        let mut message = message::Builder::new_default();
        let mut scope = message.init_root::<brand::Builder>().init_scopes(1).get(0);
        scope.set_scope_id(ID);
        let mut values = scope.init_bind(count);
        for i in 0..count {
            values.reborrow().get(i).init_type().set_text(());
        }
        let schema = loader
            .get(ID)
            .unwrap()
            .bind(message.get_root_as_reader().unwrap(), None)
            .unwrap();
        // One schema, one scope and one visit per argument: 128 is accepted.
        assert_eq!(schema.identity().is_ok(), count <= 126);
    }
}

#[test]
fn generic_enum_keys_use_the_native_erased_brand() {
    let mut message = message::Builder::new_default();
    let mut node = message.init_root::<capnp::schema_capnp::node::Builder>();
    node.set_id(0xa200_0000_0000_0001);
    node.set_is_generic(true);
    node.init_enum().init_enumerants(1).get(0).set_name("value");
    let segments = message.get_segments_for_output();
    assert_eq!(segments.len(), 1);
    let mut words = capnp::Word::allocate_zeroed_vec(segments[0].len() / 8);
    capnp::Word::words_to_bytes_mut(&mut words).copy_from_slice(segments[0]);
    let raw = capnp::introspect::RawEnumSchema {
        encoded_node: Box::leak(words.into_boxed_slice()),
        annotation_types: |_, _| panic!("no annotations"),
    };
    let schema = capnp::schema::EnumSchema::new(raw);
    let key = schema.identity().unwrap();
    assert_eq!(
        schema
            .get_enumerants()
            .unwrap()
            .get(0)
            .identity()
            .unwrap()
            .parent(),
        &key
    );
}
