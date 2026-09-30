mod common;
#[path = "common/text_assignments.rs"]
mod text_assignments;
#[path = "common/text_scalars.rs"]
mod text_scalars;
#[path = "common/text_wrapping.rs"]
mod text_wrapping;
use capnp::{
    message,
    schema_loader::{
        dynamic::{
            self,
            orphan::{Access, Orphan},
        },
        Type,
    },
};
use capnp_compat::{
    json::{Handler, JsonCodec, Value},
    text::TextCodec,
};
use common::*;
use std::rc::Rc;
const TEXT: &str = r#"(flag = true, i8 = -128, i16 = -32768, i32 = -2147483648, i64 = -9223372036854775808, u8 = 255, u16 = 65535, u32 = 4294967295, u64 = 18446744073709551615, f32 = 1.5, f64 = -inf, text = "a\n\x00b", data = 0x"00ff80", nums = [-1, 9223372036854775807], child = (text = "nested"), children = [(i8 = 7), (flag = true)], lists = [[1, 2], []], choice = second, some = "chosen", details = (amount = 10))"#;

fn decode_text(
    input: &str,
    schema: capnp::schema_loader::Schema<'_>,
    orphan: bool,
    message: &mut message::Builder<message::HeapAllocator>,
) -> capnp::Result<()> {
    let root = dynamic::Builder::init(message.init_root(), schema.clone())?;
    if orphan {
        let (mut root, token) = root.with_orphanage();
        let value = TextCodec::new().decode_orphan(
            input,
            Type::Struct(schema),
            &mut token.in_struct(&mut root)?,
        )?;
        root.adopt_content(value).map_err(|e| e.error)
    } else {
        TextCodec::new().decode(input, root)
    }
}

#[test]
fn text_assignments_apply_in_order_and_preserve_documented_failure_state() {
    let schemas = schemas();
    let text = TextCodec::new();
    let json = JsonCodec::new();
    for case in text_assignments::cases() {
        let schema = schema(&schemas, case.schema);
        let mut message = message::Builder::new_default();
        let mut root = dynamic::Builder::init(message.init_root(), schema.clone()).unwrap();
        text.decode(case.seed, root.reborrow()).unwrap();
        let result = if case.orphan {
            let (mut root, token) = root.reborrow().with_orphanage();
            text.decode_orphan(
                case.input,
                Type::Struct(schema.clone()),
                &mut token.in_struct(&mut root).unwrap(),
            )
            .and_then(|value| root.adopt_content(value).map_err(|e| e.error))
        } else {
            text.decode(case.input, root.reborrow())
        };
        assert_eq!(result.is_err(), case.error, "{}: {result:?}", case.input);
        let mut expected = message::Builder::new_default();
        decode_text(case.expected, schema.clone(), false, &mut expected).unwrap();
        let expected = dynamic::Value::Struct(
            dynamic::Reader::new(expected.get_root_as_reader().unwrap(), schema).unwrap(),
        );
        let actual = dynamic::Value::Struct(root.as_reader());
        assert_eq!(
            text.encode(actual.clone()).unwrap(),
            text.encode(expected.clone()).unwrap(),
            "{} after {}",
            case.input,
            case.seed
        );
        assert_eq!(
            json.encode(actual).unwrap(),
            json.encode(expected).unwrap(),
            "{} after {}",
            case.input,
            case.seed
        );
    }
}

#[test]
fn text_implicit_wrapping_matches_explicit_values_and_defaults() {
    let schemas = schemas();
    for case in text_wrapping::accepted() {
        let schema = schema(&schemas, case.schema);
        let mut implicit = message::Builder::new_default();
        decode_text(case.input, schema.clone(), case.orphan, &mut implicit)
            .unwrap_or_else(|e| panic!("{}: {e}", case.input));
        let mut explicit = message::Builder::new_default();
        decode_text(case.explicit, schema.clone(), false, &mut explicit).unwrap();
        assert_eq!(
            wire(&implicit, schema.clone()),
            wire(&explicit, schema),
            "{}",
            case.input
        );
    }
}

#[test]
fn text_implicit_wrapping_is_one_level_and_requires_an_unambiguous_scalar() {
    let schemas = schemas();
    for case in text_wrapping::rejected() {
        let mut message = message::Builder::new_default();
        assert!(
            decode_text(
                case.input,
                schema(&schemas, case.schema),
                case.orphan,
                &mut message
            )
            .is_err(),
            "{}",
            case.input
        );
    }
}

#[test]
fn text_implicit_wrapping_replaces_values_preserves_bytes_and_resets_group_defaults() {
    let schemas = schemas();
    let schema = schema(&schemas, "Wrapping");
    let mut message = message::Builder::new_default();
    let mut root = dynamic::Builder::init(message.init_root(), schema.clone()).unwrap();
    let codec = TextCodec::new();
    codec.decode(r#"(flag = (value = false, spare = "old"), details = (value = 1, spare = "old"), text = "\xff\x00", picked = false)"#, root.reborrow()).unwrap();
    codec
        .decode("(flag = true, details = 23, group = true)", root.reborrow())
        .unwrap();
    let reader = root.as_reader();
    let dynamic::Value::Struct(text) = reader.get_named("text").unwrap() else {
        panic!()
    };
    let dynamic::Value::Text(text) = text.get_named("value").unwrap() else {
        panic!()
    };
    assert_eq!(text.as_bytes(), &[255, 0]);
    let dynamic::Value::Struct(flag) = reader.get_named("flag").unwrap() else {
        panic!()
    };
    assert!(matches!(
        flag.get_named("value").unwrap(),
        dynamic::Value::Bool(true)
    ));
    let dynamic::Value::Text(spare) = flag.get_named("spare").unwrap() else {
        panic!()
    };
    assert_eq!(spare, "kept default");
    let dynamic::Value::Struct(details) = reader.get_named("details").unwrap() else {
        panic!()
    };
    let dynamic::Value::Text(spare) = details.get_named("spare").unwrap() else {
        panic!()
    };
    assert_eq!(spare, "group default");

    let mut expected = message::Builder::new_default();
    decode_text(r#"(flag = (value = true), details = (value = 23), text = (value = "\xff\x00"), group = (value = true))"#, schema.clone(), false, &mut expected).unwrap();
    assert_eq!(wire(&message, schema.clone()), wire(&expected, schema));
}

#[test]
fn text_quotes_match_cpp_for_unicode_controls_and_binary_bytes() {
    let codec = TextCodec::new();
    let input = "\x07\x08\x0c\n\r\t\x0b'\"\\\0\x7fé🦀";
    assert_eq!(
        codec.encode(dynamic::Value::Text(input.into())).unwrap(),
        r#""\a\b\f\n\r\t\v\'\"\\\000\177é🦀""#
    );
    assert_eq!(
        codec.encode(dynamic::Value::Data("é".as_bytes())).unwrap(),
        r#""\303\251""#
    );
    // C++ may emit non-UTF-8 bytes directly; the String API uses reversible
    // octal escapes instead, without replacing or dropping malformed bytes.
    let bytes = &[255, 0, 0xc3, 0xa9][..];
    let encoded = codec.encode(dynamic::Value::Text(bytes.into())).unwrap();
    assert_eq!(encoded, r#""\377\000\303\251""#);
    assert_eq!(
        capnp_compiler::literal::parse_literal(&encoded).unwrap(),
        capnp_compiler::literal::Literal::Text(bytes.to_vec())
    );
}

#[test]
fn text_numeric_literals_enforce_type_and_range_boundaries() {
    let schemas = schemas();
    let schema = schema(&schemas, "Record");
    for (input, accepted) in text_scalars::accepted()
        .into_iter()
        .map(|s| (s, true))
        .chain(text_scalars::rejected().into_iter().map(|s| (s, false)))
        // Deliberate checked-integer boundary: C++ wraps positive overflow.
        .chain(
            [
                "(f64 = 18446744073709551616)",
                "(f32 = 0x10000000000000000)",
            ]
            .map(|s| (s.into(), false)),
        )
    {
        let mut message = message::Builder::new_default();
        let result = TextCodec::new().decode(
            &input,
            dynamic::Builder::init(message.init_root(), schema.clone()).unwrap(),
        );
        assert_eq!(result.is_ok(), accepted, "{input}: {result:?}");
    }
}

#[test]
fn text_integer_float_rounding_and_signed_zero_preserve_wire_bits() {
    let schemas = schemas();
    let schema = schema(&schemas, "Record");
    for (input, f32_bits, f64_bits) in [
        ("(f32 = 4611686293305294849, f64 = -0)", 0x5e80_0001, 0),
        (
            "(f32 = -4611686293305294849, f64 = -0.0)",
            0xde80_0001,
            1_u64 << 63,
        ),
        ("(f32 = -0, f64 = 0)", 0, 0),
        ("(f32 = -0.0, f64 = -0e0)", 1_u32 << 31, 1_u64 << 63),
    ] {
        let mut message = message::Builder::new_default();
        TextCodec::new()
            .decode(
                input,
                dynamic::Builder::init(message.init_root(), schema.clone()).unwrap(),
            )
            .unwrap();
        let reader =
            dynamic::Reader::new(message.get_root_as_reader().unwrap(), schema.clone()).unwrap();
        let dynamic::Value::Float32(actual) = reader.get_named("f32").unwrap() else {
            panic!()
        };
        assert_eq!(actual.to_bits(), f32_bits, "{input}");
        let dynamic::Value::Float64(actual) = reader.get_named("f64").unwrap() else {
            panic!()
        };
        assert_eq!(actual.to_bits(), f64_bits, "{input}");
    }
}
#[test]
fn text_json_roundtrip_all_types_preserves_canonical_wire() {
    let schemas = schemas();
    let schema = schema(&schemas, "Record");
    let mut m = message::Builder::new_default();
    let text = TextCodec::new();
    text.decode(
        TEXT,
        dynamic::Builder::init(m.init_root(), schema.clone()).unwrap(),
    )
    .unwrap();
    let original = wire(&m, schema.clone());
    let json = JsonCodec::new()
        .encode(dynamic::Value::Struct(
            dynamic::Reader::new(m.get_root_as_reader().unwrap(), schema.clone()).unwrap(),
        ))
        .unwrap();
    assert!(json.contains(r#""i64":"-9223372036854775808""#));
    assert!(json.contains(r#""u64":"18446744073709551615""#));
    assert!(json.contains(r#""f64":"-Infinity""#));
    let mut n = message::Builder::new_default();
    JsonCodec::new()
        .decode(
            &json,
            dynamic::Builder::init(n.init_root(), schema.clone()).unwrap(),
        )
        .unwrap();
    assert_eq!(original, wire(&n, schema.clone()));
    let encoded = text
        .encode(dynamic::Value::Struct(
            dynamic::Reader::new(n.get_root_as_reader().unwrap(), schema.clone()).unwrap(),
        ))
        .unwrap();
    let mut n = message::Builder::new_default();
    text.decode(
        &encoded,
        dynamic::Builder::init(n.init_root(), schema.clone()).unwrap(),
    )
    .unwrap();
    assert_eq!(original, wire(&n, schema));
}
#[test]
fn annotations_flatten_discriminator_blobs_enums_and_embedded_json() {
    let schemas = schemas();
    let schema = schema(&schemas, "Annotated");
    let mut codec = JsonCodec::new();
    codec.handle_by_annotation(schema.clone()).unwrap();
    codec.reject_unknown_fields = true;
    let input = r#"{"value":42,"renamed_field":"hello","p_count":9,"p_flag":true,"data":"AP+A","hex":"00ff80","choices":["SECOND","first"],"embedded":{"a":[1,true,null],"b":"now"},"kind":"number"}"#;
    let mut m = message::Builder::new_default();
    codec
        .decode(
            input,
            dynamic::Builder::init(m.init_root(), schema.clone()).unwrap(),
        )
        .unwrap();
    let value = codec
        .encode_value(dynamic::Value::Struct(
            dynamic::Reader::new(m.get_root_as_reader().unwrap(), schema.clone()).unwrap(),
        ))
        .unwrap();
    for (k, v) in match codec.parse(input).unwrap() {
        Value::Object(v) => v,
        _ => unreachable!(),
    } {
        assert_eq!(value.get(&k), Some(&v), "{k}");
    }
    let mut n = message::Builder::new_default();
    codec
        .decode_value(
            &value,
            dynamic::Builder::init(n.init_root(), schema.clone()).unwrap(),
        )
        .unwrap();
    assert_eq!(wire(&m, schema.clone()), wire(&n, schema));
}
#[test]
fn flattened_union_selects_by_discriminator_after_payload() {
    let schemas = schemas();
    let schema = schema(&schemas, "FlatUnion");
    let mut codec = JsonCodec::new();
    codec.handle_by_annotation(schema.clone()).unwrap();
    codec.reject_unknown_fields = true;
    let mut m = message::Builder::new_default();
    codec
        .decode(
            r#"{"b_text":"hello","type":"b","b_i32":12}"#,
            dynamic::Builder::init(m.init_root(), schema.clone()).unwrap(),
        )
        .unwrap();
    let v = codec
        .encode_value(dynamic::Value::Struct(
            dynamic::Reader::new(m.get_root_as_reader().unwrap(), schema).unwrap(),
        ))
        .unwrap();
    assert_eq!(v.get("b_text"), Some(&Value::String("hello".into())));
    assert!(v.get("x").is_none());
}
#[test]
fn custom_field_overrides_type_handler_and_orphan_decode() {
    struct Reverse;
    impl Handler for Reverse {
        fn encode(&self, _: &JsonCodec, value: dynamic::Value<'_, '_>) -> capnp::Result<Value> {
            let dynamic::Value::Text(t) = value else {
                panic!()
            };
            Ok(Value::String(t.to_str()?.chars().rev().collect()))
        }
        fn decode<'m, 's>(
            &self,
            _: &JsonCodec,
            input: &Value,
            _: Type<'s>,
            access: &mut Access<'_, 'm>,
        ) -> capnp::Result<Orphan<'m, 's>> {
            let s: String = input.as_str().unwrap().chars().rev().collect();
            access.copy(dynamic::Value::Text(s.as_str().into()))
        }
    }
    struct Upper;
    impl Handler for Upper {
        fn encode(&self, _: &JsonCodec, value: dynamic::Value<'_, '_>) -> capnp::Result<Value> {
            let dynamic::Value::Text(t) = value else {
                panic!()
            };
            Ok(Value::String(t.to_str()?.to_uppercase()))
        }
        fn decode<'m, 's>(
            &self,
            _: &JsonCodec,
            input: &Value,
            _: Type<'s>,
            access: &mut Access<'_, 'm>,
        ) -> capnp::Result<Orphan<'m, 's>> {
            access.copy(dynamic::Value::Text(input.as_str().unwrap().into()))
        }
    }
    let schemas = schemas();
    let schema = schema(&schemas, "Record");
    let mut codec = JsonCodec::new();
    codec.add_type_handler(|t| *t == Type::Text, Rc::new(Reverse));
    codec.add_field_handler(schema.field("some").unwrap(), Rc::new(Upper));
    let mut m = message::Builder::new_default();
    codec
        .decode(
            r#"{"text":"olleh","some":"field"}"#,
            dynamic::Builder::init(m.init_root(), schema.clone()).unwrap(),
        )
        .unwrap();
    let v = codec
        .encode_value(dynamic::Value::Struct(
            dynamic::Reader::new(m.get_root_as_reader().unwrap(), schema).unwrap(),
        ))
        .unwrap();
    assert_eq!(v.get("text"), Some(&Value::String("olleh".into())));
    assert_eq!(v.get("some"), Some(&Value::String("FIELD".into())));
}
#[test]
fn malformed_input_type_ranges_unknowns_and_limits() {
    let schemas = schemas();
    let schema = schema(&schemas, "Record");
    let mut codec = JsonCodec::new();
    codec.reject_unknown_fields = true;
    for input in [
        r#"{"i8":128}"#,
        r#"{"u64":"18446744073709551616"}"#,
        r#"{"u32":-1}"#,
        r#"{"flag":1}"#,
        r#"{"nums":[1.5]}"#,
        r#"{"data":[256]}"#,
        r#"{"unknown":1}"#,
        r#"{"choice":"unknown"}"#,
    ] {
        let mut m = message::Builder::new_default();
        assert!(
            codec
                .decode(
                    input,
                    dynamic::Builder::init(m.init_root(), schema.clone()).unwrap()
                )
                .is_err(),
            "{input}"
        );
    }
    for input in [
        "[1,]",
        "{\"a\":1,}",
        "01",
        "1e",
        "1.",
        "-",
        "true false",
        "\"\\ud800\"",
        "[",
        "NaN",
    ] {
        assert!(codec.parse(input).is_err(), "{input}");
    }
    codec.max_nesting_depth = 3;
    assert!(codec.parse("[[[[[0]]]]]").is_err());
    codec.max_input_bytes = 1;
    assert!(codec.parse("[]").is_err());
    codec.max_output_bytes = 1;
    assert!(codec.stringify(&Value::String("x".into())).is_err());
    for input in [
        "(i8 = 128)",
        "(text = embed \"file\")",
        "(i32 = Other.constant)",
        "() junk",
    ] {
        let mut m = message::Builder::new_default();
        assert!(
            TextCodec::new()
                .decode(
                    input,
                    dynamic::Builder::init(m.init_root(), schema.clone()).unwrap()
                )
                .is_err(),
            "{input}"
        );
    }
}
#[test]
fn byte_strings_integer_radices_pretty_and_standalone_values() {
    let schemas = schemas();
    let schema = schema(&schemas, "Record");
    let mut m = message::Builder::new_default();
    let mut codec = TextCodec::new();
    codec.pretty_print = true;
    let builder = dynamic::Builder::init(m.init_root(), schema.clone()).unwrap();
    let (mut builder, orphanage) = builder.with_orphanage();
    let o = codec
        .decode_orphan(
            "[0xFFFF, 077]",
            Type::List(Box::new(Type::UInt16)),
            &mut orphanage.in_struct(&mut builder).unwrap(),
        )
        .unwrap();
    let mut o = o;
    orphanage
        .in_struct(&mut builder)
        .unwrap()
        .read(&mut o, |v| {
            let dynamic::Value::List(v) = v else { panic!() };
            assert!(matches!(v.get(0)?, dynamic::Value::UInt16(65535)));
            assert!(matches!(v.get(1)?, dynamic::Value::UInt16(63)));
            Ok(())
        })
        .unwrap();
    codec
        .decode(r#"(text = "\xff\x00", i32 = -0x80, u32 = 077)"#, builder)
        .unwrap();
    let reader = dynamic::Reader::new(m.get_root_as_reader().unwrap(), schema.clone()).unwrap();
    let output = codec.encode(dynamic::Value::Struct(reader)).unwrap();
    assert!(output.contains("\\377\\000"));
    assert!(output.contains('\n'));
    let mut n = message::Builder::new_default();
    codec
        .decode(
            &output,
            dynamic::Builder::init(n.init_root(), schema.clone()).unwrap(),
        )
        .unwrap();
    assert_eq!(wire(&m, schema.clone()), wire(&n, schema));
}
#[test]
fn raw_call_encoding_and_duplicate_json_fields_are_preserved() {
    let codec = JsonCodec::new();
    let input = r#"{"x":1,"x":2,"y":null}"#;
    assert_eq!(
        codec.stringify(&codec.parse(input).unwrap()).unwrap(),
        input
    );
    assert_eq!(
        codec.stringify(&Value::Raw("[1,2]".into())).unwrap(),
        "[1,2]"
    );
    assert_eq!(
        codec
            .stringify(&Value::Call {
                function: "BinData".into(),
                params: vec![Value::Number(0.0), Value::String("a".into())]
            })
            .unwrap(),
        r#"BinData(0,"a")"#
    );
    assert!(codec.parse(r#"BinData(0,"a")"#).is_err());
}

#[test]
fn custom_struct_handler_decodes_into_existing_builder_and_work_limit_is_per_call() {
    struct Pair;
    impl Handler for Pair {
        fn encode(&self, _: &JsonCodec, input: dynamic::Value<'_, '_>) -> capnp::Result<Value> {
            let dynamic::Value::Struct(s) = input else {
                panic!()
            };
            let dynamic::Value::Int32(n) = s.get_named("i32")? else {
                panic!()
            };
            Ok(Value::Array(vec![Value::Number(n.into())]))
        }
        fn decode<'m, 's>(
            &self,
            _: &JsonCodec,
            input: &Value,
            ty: Type<'s>,
            access: &mut Access<'_, 'm>,
        ) -> capnp::Result<Orphan<'m, 's>> {
            let Type::Struct(s) = ty else { panic!() };
            let Value::Array(v) = input else { panic!() };
            let Value::Number(n) = v[0] else { panic!() };
            let mut owner = access.new_struct(s)?;
            access.edit(&mut owner, |editor| {
                let capnp::schema_loader::dynamic::orphan::Editor::Struct(mut b) = editor else {
                    panic!()
                };
                b.set_named("i32", dynamic::Value::Int32(n as i32))
            })?;
            Ok(owner)
        }
    }
    let schemas = schemas();
    let schema = schema(&schemas, "Record");
    let id = schema.id();
    let mut codec = JsonCodec::new();
    codec.add_type_handler(
        move |t| matches!(t,Type::Struct(s)if s.id()==id),
        Rc::new(Pair),
    );
    let mut m = message::Builder::new_default();
    let mut b = dynamic::Builder::init(m.init_root(), schema.clone()).unwrap();
    b.set_named("text", dynamic::Value::Text("discard".into()))
        .unwrap();
    codec.decode("[42]", b).unwrap();
    let reader = dynamic::Reader::new(m.get_root_as_reader().unwrap(), schema.clone()).unwrap();
    assert!(!reader.has_named("text").unwrap());
    assert_eq!(
        codec.encode(dynamic::Value::Struct(reader)).unwrap(),
        "[42]"
    );
    let mut codec = JsonCodec::new();
    codec.max_value_visits = 5;
    let reader = dynamic::Reader::new(m.get_root_as_reader().unwrap(), schema).unwrap();
    assert!(codec.encode(dynamic::Value::Struct(reader)).is_err());
    assert_eq!(codec.encode(dynamic::Value::Bool(true)).unwrap(), "true");
}

#[test]
fn field_handlers_preserve_loader_and_generic_brand_identity() {
    struct Tag;
    impl Handler for Tag {
        fn encode(&self, _: &JsonCodec, _: dynamic::Value<'_, '_>) -> capnp::Result<Value> {
            Ok(Value::String("handled text".into()))
        }
        fn decode<'m, 's>(
            &self,
            _: &JsonCodec,
            _: &Value,
            ty: Type<'s>,
            access: &mut Access<'_, 'm>,
        ) -> capnp::Result<Orphan<'m, 's>> {
            assert!(ty == Type::Text);
            access.copy(dynamic::Value::Text("handled text".into()))
        }
    }
    let schemas = schemas();
    let schema = schema(&schemas, "Branded");
    let Type::Struct(text) = schema.field("text").unwrap().get_type().unwrap() else {
        panic!()
    };
    let mut codec = JsonCodec::new();
    codec.add_field_handler(text.field("value").unwrap(), Rc::new(Tag));
    let mut message = message::Builder::new_default();
    codec
        .decode(
            r#"{"text":{"value":"ignored"},"data":{"value":[1,255]}}"#,
            dynamic::Builder::init(message.init_root(), schema.clone()).unwrap(),
        )
        .unwrap();
    let json = codec
        .encode_value(dynamic::Value::Struct(
            dynamic::Reader::new(message.get_root_as_reader().unwrap(), schema).unwrap(),
        ))
        .unwrap();
    assert_eq!(
        json.get("text").unwrap().get("value"),
        Some(&Value::String("handled text".into()))
    );
    assert_eq!(
        json.get("data").unwrap().get("value"),
        Some(&Value::Array(vec![
            Value::Number(1.0),
            Value::Number(255.0)
        ]))
    );
}

#[test]
fn root_orphan_adoption_rejects_foreign_arena_and_loader_without_losing_ownership() {
    let schemas = schemas();
    let other_schemas = common::schemas();
    let record = schema(&schemas, "Record");
    let mut message = message::Builder::new_default();
    let (mut root, token) = dynamic::Builder::init(message.init_root(), record.clone())
        .unwrap()
        .with_orphanage();
    let owner = TextCodec::new()
        .decode_orphan(
            "(i32 = 42)",
            Type::Struct(record.clone()),
            &mut token.in_struct(&mut root).unwrap(),
        )
        .unwrap();
    let mut foreign_message = message::Builder::new_default();
    let mut foreign = dynamic::Builder::init(foreign_message.init_root(), record).unwrap();
    let owner = foreign.adopt_content(owner).unwrap_err().orphan;
    root.adopt_content(owner).unwrap();
    assert!(matches!(
        root.as_reader().get_named("i32").unwrap(),
        dynamic::Value::Int32(42)
    ));

    let mut owner = token
        .in_struct(&mut root)
        .unwrap()
        .new_struct(schema(&other_schemas, "Record"))
        .unwrap();
    owner = root.adopt_content(owner).unwrap_err().orphan;
    token
        .in_struct(&mut root)
        .unwrap()
        .read(&mut owner, |value| {
            let dynamic::Value::Struct(value) = value else {
                panic!()
            };
            assert!(matches!(value.get_named("i32")?, dynamic::Value::Int32(0)));
            Ok(())
        })
        .unwrap();
    assert!(matches!(
        root.as_reader().get_named("i32").unwrap(),
        dynamic::Value::Int32(42)
    ));
}
