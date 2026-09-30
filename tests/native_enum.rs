use capnp::{
    dynamic_value,
    introspect::{Introspect, TypeVariant},
    message,
    schema::EnumSchema,
    schema_capnp::{brand, node},
    schema_loader::{dynamic, Schema, SchemaLoader, Type},
};
use fixture::{api::scope::Tone as OpenTone, scope::Tone};
use reproto_test_support::enum_brand_capnp as fixture;
mod native_enum {
    pub mod verification;
}

fn enum_schema<E: Introspect>() -> EnumSchema {
    let TypeVariant::Enum(raw) = E::introspect().which() else {
        panic!()
    };
    raw.into()
}
fn scope_id() -> u64 {
    fixture::scope::Owned::<capnp::text::Owned>::introspect()
        .as_struct_schema()
        .unwrap()
        .get_proto()
        .get_id()
}
fn branded(loader: &SchemaLoader, text: bool) -> Schema<'_> {
    let mut message = message::Builder::new_default();
    let mut scope = message.init_root::<brand::Builder>().init_scopes(1).get(0);
    scope.set_scope_id(scope_id());
    let mut ty = scope.init_bind(1).get(0).init_type();
    if text {
        ty.set_text(());
    } else {
        ty.set_data(());
    }
    let Type::Enum(schema) = loader
        .get(scope_id())
        .unwrap()
        .bind(message.get_root_as_reader().unwrap(), None)
        .unwrap()
        .field("tone")
        .unwrap()
        .get_type()
        .unwrap()
    else {
        panic!()
    };
    schema
}
struct Catalog {
    registered: SchemaLoader,
    unregistered: SchemaLoader,
    foreign: SchemaLoader,
    extended: SchemaLoader,
}
impl Catalog {
    fn new() -> Self {
        let mut registered = SchemaLoader::default();
        registered
            .load_compiled_type_and_dependencies::<fixture::record::Owned>()
            .unwrap();
        registered.load_compiled_type_and_dependencies::<fixture::scope::inner::Owned<capnp::text::Owned,capnp::data::Owned>>().unwrap();
        let mut unregistered = SchemaLoader::default();
        unregistered
            .load_batch(registered.get_all_loaded().map(|s| s.get_proto()))
            .unwrap();
        let foreign = unregistered.clone();
        let mut extended = unregistered.clone();
        let original = enum_schema::<Tone>().get_proto();
        let mut message = message::Builder::new_default();
        message.set_root(original).unwrap();
        let node::Enum(mut e) = message
            .get_root::<node::Builder>()
            .unwrap()
            .which()
            .unwrap()
        else {
            panic!()
        };
        let node::Enum(old) = original.which().unwrap() else {
            panic!()
        };
        let mut members = e.reborrow().init_enumerants(3);
        for i in 0..2 {
            members
                .set_with_caveats(i, old.get_enumerants().unwrap().get(i))
                .unwrap();
        }
        let mut member = members.get(2);
        member.set_name("future");
        member.set_code_order(2);
        extended
            .load(message.get_root_as_reader().unwrap())
            .unwrap();
        Self {
            registered,
            unregistered,
            foreign,
            extended,
        }
    }
    fn value(&self, selected: u64, ordinal: u16) -> dynamic::Value<'_, '_> {
        let schema = match selected {
            1 => branded(&self.unregistered, true),
            2 => branded(&self.registered, false),
            3 => branded(&self.foreign, true),
            4 => branded(&self.extended, true),
            5 => self
                .registered
                .get(
                    enum_schema::<fixture::scope::inner::State>()
                        .get_proto()
                        .get_id(),
                )
                .unwrap(),
            6 => return dynamic::Value::UInt16(ordinal),
            _ => panic!(),
        };
        dynamic::Value::Enum(ordinal, schema)
    }
    fn cast(&self, selected: u64, ordinal: u16) -> capnp::Result<OpenTone> {
        if selected == 0 {
            dynamic_value::Enum::new(ordinal, enum_schema::<Tone>()).cast_native()
        } else {
            self.value(selected, ordinal).cast_enum()
        }
    }
    fn inspect(&self, selected: u64, ordinal: u16) -> capnp::Result<Option<u16>> {
        if selected == 0 {
            Ok(dynamic_value::Enum::new(ordinal, enum_schema::<Tone>())
                .get_enumerant()?
                .map(|m| m.get_ordinal()))
        } else {
            Ok(self
                .value(selected, ordinal)
                .get_enumerant()?
                .map(|m| m.index()))
        }
    }
}

fn stored(message: &message::Builder<message::HeapAllocator>) -> (u64, u64) {
    match message
        .get_root_as_reader::<fixture::record::Reader>()
        .unwrap()
        .which()
        .unwrap()
    {
        fixture::record::Other(n) => (0, n.into()),
        fixture::record::Tone(Ok(n)) => (1, u16::from(n).into()),
        fixture::record::Tone(Err(capnp::NotInSchema(n))) => (1, n.into()),
    }
}
fn write(message: &mut message::Builder<message::HeapAllocator>, value: OpenTone) {
    let dynamic_value::Builder::Struct(mut target) = message
        .get_root::<fixture::record::Builder>()
        .unwrap()
        .into()
    else {
        panic!()
    };
    target.set_named("tone", value.into()).unwrap();
}
fn mismatch<T>(result: capnp::Result<T>) {
    assert!(matches!(
        result,
        Err(capnp::Error {
            kind: capnp::ErrorKind::TypeMismatch,
            ..
        })
    ));
}

#[test]
fn every_uint16_roundtrips_through_open_native_enums_and_closed_enums_fail_safely() {
    let schema = enum_schema::<Tone>();
    assert_eq!(
        schema.identity().unwrap(),
        enum_schema::<OpenTone>().identity().unwrap()
    );
    for raw in 0..=u16::MAX {
        let value = dynamic_value::Enum::new(raw, schema);
        let open = value.cast_native::<OpenTone>().unwrap();
        assert_eq!(u16::from(open), raw);
        let dynamic_value::Reader::Enum(roundtrip) = open.into() else {
            panic!()
        };
        assert_eq!(roundtrip.get_value(), raw);
        assert_eq!(
            roundtrip.get_schema().identity().unwrap(),
            schema.identity().unwrap()
        );
        match value.cast_native::<Tone>() {
            Ok(value) => {
                assert!(raw < 2);
                assert_eq!(u16::from(value), raw);
            }
            Err(error) => assert!(
                matches!(error.kind, capnp::ErrorKind::EnumValueOrUnionDiscriminantNotPresent(capnp::NotInSchema(n)) if n == raw && raw >= 2)
            ),
        }
    }
}

#[test]
fn individual_enum_casts_ignore_registration_owner_and_brand_but_aggregate_casts_do_not() {
    let catalog = Catalog::new();
    assert_ne!(
        branded(&catalog.unregistered, true).identity().unwrap(),
        branded(&catalog.foreign, true).identity().unwrap()
    );
    assert!(Type::Enum(branded(&catalog.unregistered, true))
        .require_usable_as::<Tone>()
        .is_err());
    for selected in 0..=4 {
        for raw in [0, 1, 2, u16::MAX] {
            assert_eq!(u16::from(catalog.cast(selected, raw).unwrap()), raw);
        }
    }
    for selected in 5..=6 {
        mismatch(catalog.cast(selected, 1));
    }
    mismatch(dynamic_value::Enum::new(1, enum_schema::<Tone>()).cast_native::<u16>());
    mismatch(catalog.value(1, 1).cast_enum::<u16>());
    mismatch(
        dynamic::Value::Enum(0, catalog.registered.get(scope_id()).unwrap())
            .cast_enum::<OpenTone>(),
    );

    let mut message = message::Builder::new_default();
    message.init_root::<fixture::scope::Builder<capnp::text::Owned>>();
    let id = scope_id();
    let mut brand_message = message::Builder::new_default();
    let mut scope = brand_message
        .init_root::<brand::Builder>()
        .init_scopes(1)
        .get(0);
    scope.set_scope_id(id);
    scope.init_bind(1).get(0).init_type().set_text(());
    let target_schema = catalog
        .registered
        .get(id)
        .unwrap()
        .bind(brand_message.get_root_as_reader().unwrap(), None)
        .unwrap();
    let mut target = dynamic::Builder::new(message.get_root().unwrap(), target_schema).unwrap();
    assert!(target.set_named("tone", catalog.value(2, 1)).is_err());
    let native = catalog.value(2, 1).cast_enum::<Tone>().unwrap();
    drop(target);
    message
        .get_root::<fixture::scope::Builder<capnp::text::Owned>>()
        .unwrap()
        .set_tone(native);
    assert_eq!(
        message
            .get_root_as_reader::<fixture::scope::Reader<capnp::text::Owned>>()
            .unwrap()
            .get_tone()
            .unwrap(),
        Tone::Loud
    );
}

#[test]
fn member_lookup_uses_the_source_schema_version_and_survives_message_drop() {
    let catalog = Catalog::new();
    assert_eq!(catalog.inspect(1, 2).unwrap(), None);
    assert_eq!(catalog.inspect(4, 2).unwrap(), Some(2));
    assert_eq!(catalog.inspect(4, u16::MAX).unwrap(), None);
    assert!(catalog.inspect(6, 0).is_err());
    assert!(
        dynamic::Value::Enum(0, catalog.registered.get(scope_id()).unwrap())
            .get_enumerant()
            .is_err()
    );
    let member = {
        let mut message = message::Builder::new_default();
        message
            .init_root::<fixture::scope::Builder<capnp::text::Owned>>()
            .set_tone(Tone::Quiet);
        let reader = dynamic::Reader::new(
            message.get_root_as_reader().unwrap(),
            catalog.registered.get(scope_id()).unwrap(),
        )
        .unwrap();
        reader
            .get_named("tone")
            .unwrap()
            .get_enumerant()
            .unwrap()
            .unwrap()
    };
    assert_eq!(
        member.get_proto().get_name().unwrap().to_str().unwrap(),
        "quiet"
    );
    let member = catalog.value(4, 2).get_enumerant().unwrap().unwrap();
    assert_eq!(
        member.get_proto().get_name().unwrap().to_str().unwrap(),
        "future"
    );
    assert_eq!(
        member.parent().identity().unwrap(),
        branded(&catalog.extended, true).identity().unwrap()
    );
    assert!(matches!(
        catalog.value(4, 2).cast_enum::<Tone>().unwrap_err().kind,
        capnp::ErrorKind::EnumValueOrUnionDiscriminantNotPresent(capnp::NotInSchema(2))
    ));
    let native = {
        let catalog = Catalog::new();
        catalog.value(4, 2).cast_enum::<OpenTone>().unwrap()
    };
    assert_eq!(native, OpenTone::Unknown(2));
}

#[test]
fn wrong_id_is_checked_before_conversion_and_rejected_cast_does_not_change_union() {
    struct Unreachable;
    impl Introspect for Unreachable {
        fn introspect() -> capnp::introspect::Type {
            fixture::scope::inner::State::introspect()
        }
    }
    impl TryFrom<u16> for Unreachable {
        type Error = capnp::NotInSchema;
        fn try_from(_: u16) -> Result<Self, Self::Error> {
            panic!("wrong-ID conversion was invoked")
        }
    }
    mismatch(dynamic_value::Enum::new(1, enum_schema::<Tone>()).cast_native::<Unreachable>());
    let catalog = Catalog::new();
    mismatch(catalog.value(1, 1).cast_enum::<Unreachable>());
    let mut message = message::Builder::new_default();
    message
        .init_root::<fixture::record::Builder>()
        .set_other(77);
    for selected in 5..=6 {
        if let Ok(value) = catalog.cast(selected, u16::MAX) {
            write(&mut message, value);
        }
        assert_eq!(stored(&message), (0, 77));
    }
    write(&mut message, catalog.cast(2, u16::MAX).unwrap());
    assert_eq!(stored(&message), (1, u16::MAX.into()));
    for selected in 5..=6 {
        if let Ok(value) = catalog.cast(selected, 0) {
            write(&mut message, value);
        }
        assert_eq!(stored(&message), (1, u16::MAX.into()));
    }
    let nested = fixture::api::scope::inner::State::Unknown(u16::MAX);
    let dynamic_value::Reader::Enum(value) = nested.into() else {
        panic!()
    };
    assert_eq!(
        value
            .cast_native::<fixture::api::scope::inner::State>()
            .unwrap(),
        nested
    );
    mismatch(value.cast_native::<OpenTone>());
}
