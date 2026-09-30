use capnp::{
    dynamic_value,
    introspect::{Introspect, TypeVariant},
    message,
    schema::{EnumSchema, SchemaIdentity, StructSchema},
    schema_capnp::brand,
    schema_loader::{dynamic, Schema, SchemaLoader, Type},
};
use reproto_test_support::enum_brand_capnp as fixture;
mod enum_brand {
    pub mod verification;
}
fn enum_schema(ty: capnp::introspect::Type) -> EnumSchema {
    let TypeVariant::Enum(raw) = ty.which() else {
        panic!()
    };
    raw.into()
}
fn scope<T: capnp::traits::Owned>() -> StructSchema {
    fixture::scope::Owned::<T>::introspect()
        .as_struct_schema()
        .unwrap()
}
fn compiled(index: u64) -> EnumSchema {
    match index {
        0 => enum_schema(fixture::scope::Tone::introspect()),
        1 => enum_schema(
            scope::<capnp::text::Owned>()
                .get_field_by_name("tone")
                .unwrap()
                .get_type(),
        ),
        2 => enum_schema(
            scope::<capnp::data::Owned>()
                .get_field_by_name("tone")
                .unwrap()
                .get_type(),
        ),
        3 => {
            let TypeVariant::List(element) = scope::<capnp::text::Owned>()
                .get_field_by_name("tones")
                .unwrap()
                .get_type()
                .which()
            else {
                panic!()
            };
            enum_schema(element)
        }
        4 => enum_schema(fixture::scope::inner::State::introspect()),
        _ => panic!(),
    }
}
fn loader() -> SchemaLoader {
    let mut loader = SchemaLoader::default();
    loader
        .load_compiled_type_and_dependencies::<fixture::scope::Owned<capnp::text::Owned>>()
        .unwrap();
    loader.load_compiled_type_and_dependencies::<fixture::scope::inner::Owned<capnp::text::Owned,capnp::data::Owned>>().unwrap();
    loader
}
fn bound_scope(loader: &SchemaLoader, text: bool) -> Schema<'_> {
    let id = scope::<capnp::text::Owned>().get_proto().get_id();
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
fn loaded(loader: &SchemaLoader, index: u64) -> Schema<'_> {
    if index == 0 || index == 4 {
        return loader.get(compiled(index).get_proto().get_id()).unwrap();
    }
    let ty = bound_scope(loader, index != 2)
        .field(if index == 3 { "tones" } else { "tone" })
        .unwrap()
        .get_type()
        .unwrap();
    let ty = if let Type::List(element) = ty {
        *element
    } else {
        ty
    };
    let Type::Enum(schema) = ty else { panic!() };
    schema
}
fn key(loader: &SchemaLoader, mode: u64, index: u64) -> SchemaIdentity<'_> {
    if mode == 0 {
        compiled(index).identity().unwrap()
    } else {
        loaded(loader, index).identity().unwrap()
    }
}
fn canonical(mode: u64, index: u64) -> u64 {
    if index == 4 {
        4
    } else if mode == 0 {
        0
    } else if index == 3 {
        1
    } else {
        index
    }
}
fn write(
    message: &mut message::Builder<message::HeapAllocator>,
    loader: &SchemaLoader,
    mode: u64,
    source: u64,
    ordinal: u16,
) -> bool {
    if mode == 0 {
        let dynamic_value::Builder::Struct(mut target) = message
            .get_root::<fixture::scope::Builder<capnp::text::Owned>>()
            .unwrap()
            .into()
        else {
            panic!()
        };
        target
            .set_named(
                "tone",
                dynamic_value::Enum::new(ordinal, compiled(source)).into(),
            )
            .is_ok()
    } else {
        let mut target =
            dynamic::Builder::new(message.get_root().unwrap(), bound_scope(loader, true)).unwrap();
        target
            .set_named(
                "tone",
                dynamic::Value::Enum(ordinal, loaded(loader, source)),
            )
            .is_ok()
    }
}
fn read(message: &message::Builder<message::HeapAllocator>) -> u16 {
    match message
        .get_root_as_reader::<fixture::scope::Reader<capnp::text::Owned>>()
        .unwrap()
        .get_tone()
    {
        Ok(value) => value.into(),
        Err(capnp::NotInSchema(value)) => value,
    }
}
#[test]
fn compiled_enum_identity_erases_enclosing_brands_while_loaded_identity_preserves_them() {
    let loader = loader();
    for mode in 0..=1 {
        for left in 0..5 {
            for right in 0..5 {
                assert_eq!(
                    key(&loader, mode, left) == key(&loader, mode, right),
                    canonical(mode, left) == canonical(mode, right),
                    "{mode}/{left}/{right}"
                );
            }
        }
    }
    assert!(compiled(0).get_proto().get_is_generic());
    assert_ne!(
        scope::<capnp::text::Owned>().identity().unwrap(),
        scope::<capnp::data::Owned>().identity().unwrap()
    );
    assert_ne!(
        scope::<capnp::text::Owned>()
            .get_field_by_name("tone")
            .unwrap()
            .identity()
            .unwrap(),
        scope::<capnp::data::Owned>()
            .get_field_by_name("tone")
            .unwrap()
            .identity()
            .unwrap()
    );
    for i in 0..5 {
        let e = compiled(i).get_enumerants().unwrap().get(0);
        assert_eq!(
            e.identity().unwrap().parent(),
            &compiled(i).identity().unwrap()
        );
        let foreign = loader.clone();
        assert_ne!(
            loaded(&loader, i).identity().unwrap(),
            loaded(&foreign, i).identity().unwrap()
        );
    }
    let first =
        fixture::scope::inner::Owned::<capnp::text::Owned, capnp::data::Owned>::introspect()
            .as_struct_schema()
            .unwrap();
    let second =
        fixture::scope::inner::Owned::<capnp::data::Owned, capnp::text::Owned>::introspect()
            .as_struct_schema()
            .unwrap();
    for field in ["state", "outer"] {
        let first = enum_schema(first.get_field_by_name(field).unwrap().get_type());
        let second = enum_schema(second.get_field_by_name(field).unwrap().get_type());
        assert_eq!(first.identity().unwrap(), second.identity().unwrap());
    }
}
#[test]
fn nested_enum_annotation_callbacks_and_native_values_compile_with_erased_parameters() {
    for (schema, label, ordinal, member_label) in [
        (compiled(0), "tone", 0, "quiet"),
        (compiled(4), "state", 1, "busy"),
    ] {
        let dynamic_value::Reader::Text(value) = schema
            .get_annotations()
            .unwrap()
            .get(0)
            .get_value()
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(value.to_str().unwrap(), label);
        let dynamic_value::Reader::Text(value) = schema
            .get_enumerants()
            .unwrap()
            .get(ordinal)
            .get_annotations()
            .unwrap()
            .get(0)
            .get_value()
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(value.to_str().unwrap(), member_label);
    }
    for (value, schema) in [
        (
            dynamic_value::Reader::from(fixture::scope::Tone::Loud),
            compiled(0),
        ),
        (
            dynamic_value::Reader::from(fixture::scope::inner::State::Busy),
            compiled(4),
        ),
    ] {
        let dynamic_value::Reader::Enum(value) = value else {
            panic!()
        };
        assert_eq!(
            value.get_schema().identity().unwrap(),
            schema.identity().unwrap()
        );
        assert_eq!(value.get_value(), 1);
    }
}
#[test]
fn enum_list_roundtrip_keeps_unknown_ordinals_and_the_correct_identity_domain() {
    let loader = loader();
    let mut message = message::Builder::new_default();
    {
        let mut root = message.init_root::<fixture::scope::Builder<capnp::text::Owned>>();
        let mut list = root.reborrow().init_tones(2);
        list.set(0, fixture::scope::Tone::Loud);
        let dynamic_value::Builder::List(mut dynamic) = list.into() else {
            panic!()
        };
        dynamic
            .set(1, dynamic_value::Enum::new(u16::MAX, compiled(0)).into())
            .unwrap();
    }
    let dynamic_value::Reader::Struct(compiled_root) = message
        .get_root_as_reader::<fixture::scope::Reader<capnp::text::Owned>>()
        .unwrap()
        .into()
    else {
        panic!()
    };
    let dynamic_value::Reader::List(list) = compiled_root.get_named("tones").unwrap() else {
        panic!()
    };
    let loaded_root = dynamic::Reader::new(
        message.get_root_as_reader().unwrap(),
        bound_scope(&loader, true),
    )
    .unwrap();
    let dynamic::Value::List(loaded_list) = loaded_root.get_named("tones").unwrap() else {
        panic!()
    };
    for (index, expected) in [1, u16::MAX].into_iter().enumerate() {
        let dynamic_value::Reader::Enum(value) = list.get(index as u32).unwrap() else {
            panic!()
        };
        assert_eq!(value.get_value(), expected);
        assert_eq!(
            value.get_schema().identity().unwrap(),
            compiled(0).identity().unwrap()
        );
        let dynamic::Value::Enum(value, schema) = loaded_list.get(index as u32).unwrap() else {
            panic!()
        };
        assert_eq!(value, expected);
        assert_eq!(
            schema.identity().unwrap(),
            loaded(&loader, 1).identity().unwrap()
        );
        assert_ne!(
            schema.identity().unwrap(),
            loaded(&loader, 0).identity().unwrap()
        );
    }
}

#[test]
fn loaded_enum_resolution_preserves_nested_and_symbolic_scopes_and_rejects_bad_bindings() {
    let loader = loader();
    let outer_id = scope::<capnp::text::Owned>().get_proto().get_id();
    let inner_id =
        fixture::scope::inner::Owned::<capnp::text::Owned, capnp::data::Owned>::introspect()
            .as_struct_schema()
            .unwrap()
            .get_proto()
            .get_id();
    let mut brand_message = message::Builder::new_default();
    let mut scopes = brand_message.init_root::<brand::Builder>().init_scopes(2);
    for (i, id) in [outer_id, inner_id].into_iter().enumerate() {
        let mut s = scopes.reborrow().get(i as u32);
        s.set_scope_id(id);
        let mut t = s.init_bind(1).get(0).init_type();
        if i == 0 {
            t.set_text(());
        } else {
            t.set_data(());
        }
    }
    let inner = loader
        .get(inner_id)
        .unwrap()
        .bind(brand_message.get_root_as_reader().unwrap(), None)
        .unwrap();
    let Type::Enum(state) = inner.field("state").unwrap().get_type().unwrap() else {
        panic!()
    };
    assert!(matches!(
        state.brand_arguments_at_scope(outer_id).unwrap().get(0),
        Type::Text
    ));
    assert!(matches!(
        state.brand_arguments_at_scope(inner_id).unwrap().get(0),
        Type::Data
    ));
    assert_eq!(state.generic_scope_ids().len(), 2);
    let Type::Enum(tone) = inner.field("outer").unwrap().get_type().unwrap() else {
        panic!()
    };
    assert_eq!(
        tone.identity().unwrap(),
        loaded(&loader, 1).identity().unwrap()
    );

    let unbound = loader.get_unbound(outer_id).unwrap();
    let Type::Enum(symbolic) = unbound.field("tone").unwrap().get_type().unwrap() else {
        panic!()
    };
    assert!(
        matches!(symbolic.brand_arguments_at_scope(outer_id).unwrap().get(0),Type::Parameter(id,0) if id==outer_id)
    );
    assert_ne!(
        symbolic.identity().unwrap(),
        loaded(&loader, 0).identity().unwrap()
    );

    for kind in 0..3 {
        let mut message = message::Builder::new_default();
        let mut en = message
            .init_root::<capnp::schema_capnp::type_::Builder>()
            .init_enum();
        en.set_type_id(compiled(0).get_proto().get_id());
        let mut s = en.init_brand().init_scopes(1).get(0);
        s.set_scope_id(if kind == 2 { u64::MAX } else { outer_id });
        let mut args = s.init_bind(if kind == 1 { 2 } else { 1 });
        for i in 0..args.len() {
            args.reborrow().get(i).init_type().set_bool(());
        }
        assert!(unbound
            .get_type(message.get_root_as_reader().unwrap())
            .is_err());
    }
}

#[test]
fn field_api_nested_enums_roundtrip_known_and_unknown_values() -> capnp::Result<()> {
    use capnp::field_api::{Message, MessageView};
    use fixture::api::{scope::Tone, Scope};
    let mut message = Message::<Scope<capnp::text::Owned>>::new()?;
    {
        let mut root = message.edit();
        root.tone().set(Tone::Unknown(u16::MAX));
        root.tones().init_with(2, |i, item| {
            item.set(if i == 0 {
                Tone::Quiet
            } else {
                Tone::Unknown(40000)
            });
            Ok(())
        })?;
    }
    let bytes = message.freeze().to_vec();
    let view = MessageView::<Scope<capnp::text::Owned>>::from_unpacked(&bytes, Default::default())?;
    assert_eq!(view.read().tone(), Tone::Unknown(u16::MAX));
    assert_eq!(view.read().tones()?.get(0), Some(Tone::Quiet));
    assert_eq!(view.read().tones()?.get(1), Some(Tone::Unknown(40000)));
    Ok(())
}
