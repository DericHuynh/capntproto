use capnp::{
    dynamic_struct,
    dynamic_value::{self, Reader as R},
    introspect::{Introspect, Type},
    message,
    schema_loader::{
        dynamic::{self, Value as V},
        Schema, SchemaLoader, Type as L,
    },
    ErrorKind,
};
use capntproto_test_support::conversion_capnp::{self, target};

mod conversion {
    pub mod verification;
}

const TARGETS: [&str; 15] = [
    "empty", "flag", "int8", "int16", "int32", "int64", "uint8", "uint16", "uint32", "uint64",
    "float32", "float64", "kind", "text", "data",
];
fn schema() -> capnp::schema::StructSchema {
    target::Owned::introspect().as_struct_schema().unwrap()
}
fn ty(index: usize) -> Type {
    schema()
        .get_field_by_name(TARGETS[index])
        .unwrap()
        .get_type()
}
fn loader() -> SchemaLoader {
    let mut loader = SchemaLoader::default();
    loader
        .load_compiled_type_and_dependencies::<target::Owned>()
        .unwrap();
    loader
        .load_compiled_type_and_dependencies::<conversion_capnp::Foreign>()
        .unwrap();
    loader
}
fn loaded_schema(loader: &SchemaLoader) -> Schema<'_> {
    loader.get(schema().get_proto().get_id()).unwrap()
}
fn enum_schema(ty: Type) -> capnp::schema::EnumSchema {
    let capnp::introspect::TypeVariant::Enum(s) = ty.which() else {
        panic!()
    };
    s.into()
}

#[derive(Clone, Copy, Debug)]
enum Input {
    Signed(i64),
    Unsigned(u64),
    Float(f64),
    Float32(f32),
    Bool(bool),
    Void,
    Text(&'static str),
    Data(&'static [u8]),
    Enum(u16, bool),
}
impl Input {
    fn compiled(self) -> R<'static> {
        match self {
            Self::Signed(v) => R::Int64(v),
            Self::Unsigned(v) => R::UInt64(v),
            Self::Float(v) => R::Float64(v),
            Self::Float32(v) => R::Float32(v),
            Self::Bool(v) => R::Bool(v),
            Self::Void => R::Void,
            Self::Text(v) => R::Text(v.into()),
            Self::Data(v) => R::Data(v),
            Self::Enum(v, foreign) => R::Enum(dynamic_value::Enum::new(
                v,
                enum_schema(if foreign {
                    conversion_capnp::Foreign::introspect()
                } else {
                    ty(12)
                }),
            )),
        }
    }
    fn loaded(self, loader: &SchemaLoader) -> V<'_, '_> {
        match self {
            Self::Signed(v) => V::Int64(v),
            Self::Unsigned(v) => V::UInt64(v),
            Self::Float(v) => V::Float64(v),
            Self::Float32(v) => V::Float32(v),
            Self::Bool(v) => V::Bool(v),
            Self::Void => V::Void,
            Self::Text(v) => V::Text(v.into()),
            Self::Data(v) => V::Data(v),
            Self::Enum(v, foreign) => {
                let id = enum_schema(if foreign {
                    conversion_capnp::Foreign::introspect()
                } else {
                    ty(12)
                })
                .get_proto()
                .get_id();
                V::Enum(v, loader.get(id).unwrap())
            }
        }
    }
}
fn normalize(value: capnp::Result<R<'_>>) -> String {
    match value {
        Err(_) => "error".into(),
        Ok(v) => match v {
            R::Void => "void".into(),
            R::Bool(v) => format!("bool{}", u8::from(v)),
            R::Int8(v) => format!("i{v}"),
            R::Int16(v) => format!("i{v}"),
            R::Int32(v) => format!("i{v}"),
            R::Int64(v) => format!("i{v}"),
            R::UInt8(v) => format!("u{v}"),
            R::UInt16(v) => format!("u{v}"),
            R::UInt32(v) => format!("u{v}"),
            R::UInt64(v) => format!("u{v}"),
            R::Float32(v) if v.is_nan() => "nan32".into(),
            R::Float64(v) if v.is_nan() => "nan64".into(),
            R::Float32(v) => format!("f32:{}", v.to_bits()),
            R::Float64(v) => format!("f64:{}", v.to_bits()),
            R::Enum(v) => format!("enum{}", v.get_value()),
            R::Text(v) => format!("text{}", hex(v.as_bytes())),
            R::Data(v) => format!("data{}", hex(v)),
            _ => panic!("unexpected non-scalar result"),
        },
    }
}
fn normalize_loaded(value: capnp::Result<V<'_, '_>>) -> String {
    normalize(value.map(|v| match v {
        V::Void => R::Void,
        V::Bool(v) => R::Bool(v),
        V::Int8(v) => R::Int8(v),
        V::Int16(v) => R::Int16(v),
        V::Int32(v) => R::Int32(v),
        V::Int64(v) => R::Int64(v),
        V::UInt8(v) => R::UInt8(v),
        V::UInt16(v) => R::UInt16(v),
        V::UInt32(v) => R::UInt32(v),
        V::UInt64(v) => R::UInt64(v),
        V::Float32(v) => R::Float32(v),
        V::Float64(v) => R::Float64(v),
        V::Enum(v, _) => R::Enum(dynamic_value::Enum::new(v, enum_schema(ty(12)))),
        V::Text(v) => R::Text(v),
        V::Data(v) => R::Data(v),
        _ => panic!(),
    }))
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn sources() -> Vec<Input> {
    let mut result = vec![
        Input::Void,
        Input::Bool(false),
        Input::Bool(true),
        Input::Text(""),
        Input::Text("zero"),
        Input::Text("other"),
        Input::Text("Other"),
        Input::Text("1"),
        Input::Text("other\0"),
        Input::Text("héllo"),
        Input::Data(b"other"),
        Input::Data(b"\xff\0"),
        Input::Enum(0, false),
        Input::Enum(1, false),
        Input::Enum(65535, false),
        Input::Enum(1, true),
    ];
    for v in [
        i64::MIN,
        i64::MIN + 1,
        -2147483649,
        -2147483648,
        -32769,
        -32768,
        -129,
        -128,
        -1,
        0,
        1,
        127,
        128,
        255,
        256,
        32767,
        32768,
        65535,
        65536,
        2147483647,
        2147483648,
        4294967295,
        4294967296,
        i64::MAX - 1,
        i64::MAX,
    ] {
        result.push(Input::Signed(v));
    }
    for v in [
        0,
        1,
        127,
        128,
        255,
        256,
        32767,
        32768,
        65535,
        65536,
        2147483647,
        2147483648,
        4294967295,
        4294967296,
        1 << 53,
        (1 << 53) + 1,
        (1 << 63) - 1,
        1 << 63,
        (1 << 63) + (1 << 39) + 1,
        u64::MAX - 1,
        u64::MAX,
    ] {
        result.push(Input::Unsigned(v));
    }
    for v in [
        -f64::MAX,
        -1.0e30,
        -9_223_372_036_854_775_808.0,
        -2147483649.0,
        -32769.0,
        -129.0,
        -128.0,
        -1.5,
        -1.0,
        -0.5,
        -0.0,
        0.0,
        0.5,
        1.0,
        1.5,
        127.0,
        127.5,
        128.0,
        255.0,
        255.5,
        256.0,
        32767.0,
        65535.0,
        65536.0,
        2147483647.0,
        4294967295.0,
        4294967296.0,
        9_223_372_036_854_775_808.0,
        18_446_744_073_709_551_616.0,
        f64::MAX,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        f64::from_bits(1),
    ] {
        result.push(Input::Float(v));
    }
    // Adjacent representable doubles around the signed/unsigned 64-bit bounds.
    for boundary in [
        -9_223_372_036_854_775_808.0f64,
        9_223_372_036_854_775_808.0,
        18_446_744_073_709_551_616.0,
    ] {
        for bits in [boundary.to_bits() - 1, boundary.to_bits() + 1] {
            result.push(Input::Float(f64::from_bits(bits)));
        }
    }
    for v in [
        -0.0,
        0.0,
        0.5,
        1.0,
        127.0,
        128.0,
        255.0,
        65535.0,
        f32::MAX,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
        f32::from_bits(1),
    ] {
        result.push(Input::Float32(v));
    }
    result
}

#[test]
fn numeric_bounds_and_fractional_values_fail_without_saturation() {
    for (input, target) in [
        (Input::Signed(-1), 9),
        (Input::Unsigned(u64::MAX), 5),
        (Input::Float(1.5), 5),
        (Input::Float(f64::NAN), 9),
        (Input::Float(f64::INFINITY), 5),
        (Input::Float(9_223_372_036_854_775_808.0), 5),
        (Input::Float(18_446_744_073_709_551_616.0), 9),
    ] {
        assert_eq!(
            input.compiled().try_convert(ty(target)).err().unwrap().kind,
            ErrorKind::NumericConversionOutOfRange
        );
    }
    assert_eq!(
        R::Float64(-0.0)
            .try_convert(u64::introspect())
            .unwrap()
            .downcast::<u64>(),
        0
    );
    assert_eq!(
        R::Float64(-9_223_372_036_854_775_808.0)
            .try_convert(i64::introspect())
            .unwrap()
            .downcast::<i64>(),
        i64::MIN
    );
    let just_below = f64::from_bits(18_446_744_073_709_551_616.0f64.to_bits() - 1);
    assert_eq!(
        R::Float64(just_below)
            .try_convert(u64::introspect())
            .unwrap()
            .downcast::<u64>(),
        u64::MAX - 2047
    );
    let n = (1u64 << 63) + (1u64 << 39) + 1;
    let f = R::UInt64(n)
        .try_convert(f32::introspect())
        .unwrap()
        .downcast::<f32>();
    assert_eq!(f.to_bits(), (9_223_372_036_854_775_808.0f32).to_bits() + 1);
    assert!(R::Float64(f64::MAX)
        .try_convert(f32::introspect())
        .unwrap()
        .downcast::<f32>()
        .is_infinite());
}

#[test]
fn every_numeric_source_width_uses_the_same_checked_rules() {
    let values = vec![
        R::Int8(42),
        R::Int16(42),
        R::Int32(42),
        R::Int64(42),
        R::UInt8(42),
        R::UInt16(42),
        R::UInt32(42),
        R::UInt64(42),
        R::Float32(42.0),
        R::Float64(42.0),
    ];
    for v in values {
        for target in 2..12 {
            let actual = normalize(v.clone().try_convert(ty(target)));
            assert_eq!(actual, normalize(R::Int64(42).try_convert(ty(target))));
        }
    }
    let values = vec![
        V::Int8(42),
        V::Int16(42),
        V::Int32(42),
        V::Int64(42),
        V::UInt8(42),
        V::UInt16(42),
        V::UInt32(42),
        V::UInt64(42),
        V::Float32(42.0),
        V::Float64(42.0),
    ];
    let loader = loader();
    for v in values {
        for (target, name) in TARGETS.iter().enumerate().take(12).skip(2) {
            let t = loaded_schema(&loader)
                .field(name)
                .unwrap()
                .get_type()
                .unwrap();
            assert_eq!(
                normalize_loaded(v.clone().try_convert(&t)),
                normalize(R::Int64(42).try_convert(ty(target)))
            );
        }
    }
}

#[test]
fn enum_names_ordinals_and_blob_conversions_are_directional() {
    for (v, expected) in [
        (Input::Text("other"), "enum1"),
        (Input::Text("Other"), "error"),
        (Input::Text("1"), "error"),
        (Input::Unsigned(65535), "enum65535"),
        (Input::Unsigned(65536), "error"),
        (Input::Float(1.0), "error"),
        (Input::Enum(65535, false), "enum65535"),
        (Input::Enum(1, true), "error"),
    ] {
        assert_eq!(normalize(v.compiled().try_convert(ty(12))), expected);
    }
    let text: capnp::text::Reader = "data".into();
    let data = R::Text(text)
        .try_convert(capnp::data::Owned::introspect())
        .unwrap()
        .downcast::<capnp::data::Reader>();
    assert_eq!(data.as_ptr(), text.as_bytes().as_ptr());
    assert!(R::Data(b"data")
        .try_convert(capnp::text::Owned::introspect())
        .is_err());
    assert!(R::Bool(true).try_convert(u8::introspect()).is_err());
    assert!(R::UInt8(1).try_convert(bool::introspect()).is_err());
    assert!(R::UInt8(1).try_convert(<()>::introspect()).is_err());
    assert!(Input::Enum(1, false)
        .compiled()
        .try_convert(u16::introspect())
        .is_err());
    let loader = loader();
    for input in sources() {
        for (index, name) in TARGETS.iter().enumerate() {
            let t = loaded_schema(&loader)
                .field(name)
                .unwrap()
                .get_type()
                .unwrap();
            assert_eq!(
                normalize(input.compiled().try_convert(ty(index))),
                normalize_loaded(input.loaded(&loader).try_convert(&t)),
                "{input:?} -> {name}"
            );
        }
    }
    assert!(V::Unknown(999).try_convert(&L::Unknown(999)).is_err());
    assert!(V::UInt8(1).try_convert(&L::Parameter(1, 0)).is_err());
}

#[test]
fn conversion_errors_preserve_union_bytes_and_capability_ownership() -> capnp::Result<()> {
    use capnp::{capability::FromClientHook, traits::ImbueMut};
    use capntproto_test_support::runtime_test_capnp::harness;
    use std::{cell::Cell, rc::Rc};
    let loader = loader();
    for runtime_loaded in [false, true] {
        let polls = Rc::new(Cell::new(0));
        let count = polls.clone();
        let marker = Rc::new(());
        let weak = Rc::downgrade(&marker);
        let client: harness::Client = capnp_rpc::new_future_client(async move {
            count.set(count.get() + 1);
            drop(marker);
            Err(capnp::Error::failed("unexpected resolution".into()))
        });
        let mut m = message::Builder::new_default();
        let mut caps = capnp::private::layout::CapTable::default();
        {
            let mut r = m.init_root::<target::Builder>();
            r.imbue_mut(&mut caps);
            r.init_kept().set_as_capability(client.into_client_hook());
        }
        let before = capnp::serialize::write_message_to_words(&m);
        for (input, index) in [
            (Input::Float(1.5), 2),
            (Input::Signed(-1), 9),
            (Input::Unsigned(256), 6),
            (Input::Text("missing"), 12),
            (Input::Enum(1, true), 12),
            (Input::Bool(true), 2),
        ] {
            if runtime_loaded {
                let mut pointer = m.get_root::<capnp::any_pointer::Builder>()?;
                pointer.imbue_mut(&mut caps);
                let mut r = dynamic::Builder::new(pointer, loaded_schema(&loader))?;
                let field = r.schema().field(TARGETS[index])?;
                let result = input
                    .loaded(&loader)
                    .try_convert(&field.get_type()?)
                    .and_then(|v| r.set(field, v));
                assert!(result.is_err());
            } else {
                let mut r = m.get_root::<target::Builder>()?;
                r.imbue_mut(&mut caps);
                let mut r = dynamic_value::Builder::from(r).downcast::<dynamic_struct::Builder>();
                let field = r.get_schema().get_field_by_name(TARGETS[index])?;
                let result = input
                    .compiled()
                    .try_convert(field.get_type())
                    .and_then(|v| r.set(field, v));
                assert!(result.is_err());
            }
            assert_eq!(before, capnp::serialize::write_message_to_words(&m));
            assert_eq!(polls.get(), 0);
            assert!(weak.upgrade().is_some());
        }
        assert_eq!(caps.iter().filter(|c| c.is_some()).count(), 1);
        drop(caps);
        assert!(weak.upgrade().is_none());
    }
    Ok(())
}

#[test]
fn aggregate_brands_and_capability_upcasts_remain_checked() -> capnp::Result<()> {
    use capnp::{
        capability::FromClientHook,
        traits::{Imbue, ImbueMut},
    };
    use capntproto_test_support::dynamic_test_capnp::{derived, orphan_brands};
    use std::{cell::Cell, rc::Rc};
    let polls = Rc::new(Cell::new(0));
    let count = polls.clone();
    let marker = Rc::new(());
    let weak = Rc::downgrade(&marker);
    {
        let client: derived::Client<capnp::text::Owned> =
            capnp_rpc::new_future_client(async move {
                count.set(count.get() + 1);
                drop(marker);
                Err(capnp::Error::failed("unexpected resolution".into()))
            });
        let identity = client.as_client_hook().get_ptr();
        let mut m = message::Builder::new_default();
        let mut caps = capnp::private::layout::CapTable::default();
        {
            let mut r = m.init_root::<orphan_brands::Builder>();
            r.imbue_mut(&mut caps);
            r.reborrow().init_text().set_value("borrowed")?;
            r.set_derived(client);
        }
        let mut r = m.get_root_as_reader::<orphan_brands::Reader>()?;
        r.imbue(&caps);
        let r = R::from(r).downcast::<dynamic_struct::Reader>();
        let ty = |name| Ok::<_, capnp::Error>(r.get_schema().get_field_by_name(name)?.get_type());
        let text = r.get_named("text")?;
        assert!(text.clone().try_convert(ty("text")?).is_ok());
        assert!(text.try_convert(ty("data")?).is_err());
        let cap = r.get_named("derived")?;
        assert!(cap.clone().try_convert(ty("wrong")?).is_err());
        assert!(cap.clone().try_convert(u64::introspect()).is_err());
        let base = cap.try_convert(ty("base")?)?;
        assert!(base.clone().try_convert(ty("derived")?).is_err());
        assert_eq!(
            base.downcast::<dynamic_value::Capability>()
                .as_client()?
                .hook
                .get_ptr(),
            identity
        );

        let mut loader = SchemaLoader::default();
        loader.load_compiled_type_and_dependencies::<orphan_brands::Owned>()?;
        let schema = loader.get(r.get_schema().get_proto().get_id())?;
        let mut pointer = m.get_root_as_reader::<capnp::any_pointer::Reader>()?;
        pointer.imbue(&caps);
        let r = dynamic::Reader::new(pointer, schema.clone())?;
        let ty = |name| schema.field(name)?.get_type();
        let text = r.get_named("text")?;
        assert!(text.clone().try_convert(&ty("text")?).is_ok());
        assert!(text.try_convert(&ty("data")?).is_err());
        let cap = r.get_named("derived")?;
        assert!(cap.clone().try_convert(&ty("wrong")?).is_err());
        assert!(cap.clone().try_convert(&L::UInt64).is_err());
        let base = cap.try_convert(&ty("base")?)?;
        assert!(base.clone().try_convert(&ty("derived")?).is_err());
        let V::Capability(base) = base else { panic!() };
        assert_eq!(base.as_client()?.hook.get_ptr(), identity);
        assert_eq!(polls.get(), 0);
    }
    assert!(weak.upgrade().is_none());
    Ok(())
}

#[test]
fn converted_values_work_in_lists_and_detached_group_setters() -> capnp::Result<()> {
    use capntproto_test_support::presence_capnp::sample;
    let mut list_message = message::Builder::new_default();
    let list = list_message.initn_root::<capnp::primitive_list::Builder<u8>>(2);
    let mut list = dynamic_value::Builder::from(list).downcast::<capnp::dynamic_list::Builder>();
    list.set(0, R::Int32(42).try_convert(u8::introspect())?)?;
    assert!(R::Int32(256)
        .try_convert(u8::introspect())
        .and_then(|v| list.set(0, v))
        .is_err());
    let reader = list.into_reader();
    assert_eq!(reader.get(0)?.downcast::<u8>(), 42);
    assert!(R::List(reader)
        .try_convert(Type::list_of(u8::introspect()))
        .is_ok());
    assert!(R::List(reader)
        .try_convert(Type::list_of(u16::introspect()))
        .is_err());

    let mut m = message::Builder::new_default();
    let (mut r, token) = dynamic_value::Builder::from(m.init_root::<sample::Builder>())
        .downcast::<dynamic_struct::Builder>()
        .with_orphanage();
    let body_schema = r
        .get_schema()
        .get_field_by_name("body")?
        .get_type()
        .as_struct_schema()?;
    let mut body = token.in_struct(&mut r)?.new_group(body_schema)?;
    token.in_struct(&mut r)?.edit_group(&mut body, |mut g| {
        let number = g.get_schema().get_field_by_name("number")?;
        let converted = R::Float64(73.0).try_convert(number.get_type())?;
        g.set(number, converted)?;
        assert!(R::Float64(1.5)
            .try_convert(number.get_type())
            .and_then(|v| g.set(number, v))
            .is_err());
        g.read(number, |v| {
            assert_eq!(v.downcast::<u32>(), 73);
            Ok(())
        })
    })?;
    r.adopt_named("body", body).unwrap();
    Ok(())
}
