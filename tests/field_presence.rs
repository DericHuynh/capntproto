use capnp::{
    dynamic_struct::{self, HasMode},
    dynamic_value as value,
    introspect::Introspect,
    message,
    schema_capnp::{field, node},
    schema_loader::{self, dynamic as loaded},
};
use capntproto_test_support::presence_capnp::{child, sample};

mod presence {
    pub mod verification;
}

const MODES: [HasMode; 2] = [HasMode::NonNull, HasMode::NonDefault];
fn schema() -> capnp::schema::StructSchema {
    sample::Owned::introspect().as_struct_schema().unwrap()
}
fn loader() -> schema_loader::SchemaLoader {
    let mut loader = schema_loader::SchemaLoader::default();
    loader
        .load_compiled_type_and_dependencies::<sample::Owned>()
        .unwrap();
    loader
}
fn loaded_schema(loader: &schema_loader::SchemaLoader) -> schema_loader::Schema<'_> {
    loader.get(schema().get_proto().get_id()).unwrap()
}
fn frame(edit: impl FnOnce(sample::Builder<'_>)) -> Vec<u8> {
    let mut message = message::Builder::new_default();
    edit(message.init_root());
    capnp::serialize::write_message_to_words(&message)
}
fn read(bytes: &[u8]) -> message::Reader<capnp::serialize::OwnedSegments> {
    capnp::serialize::read_message(bytes, Default::default()).unwrap()
}
fn compiled(
    message: &message::Reader<capnp::serialize::OwnedSegments>,
) -> dynamic_struct::Reader<'_> {
    value::Reader::from(message.get_root::<sample::Reader>().unwrap()).downcast()
}
fn slot(schema: capnp::schema::StructSchema, name: &str) -> u32 {
    let field::Slot(s) = schema
        .get_field_by_name(name)
        .unwrap()
        .get_proto()
        .which()
        .unwrap()
    else {
        panic!()
    };
    s.get_offset()
}
fn pointer_offset(schema: capnp::schema::StructSchema, name: &str) -> usize {
    let node::Struct(s) = schema.get_proto().which().unwrap() else {
        panic!()
    };
    16 + usize::from(s.get_data_word_count()) * 8 + usize::try_from(slot(schema, name)).unwrap() * 8
}
fn body_schema() -> capnp::schema::StructSchema {
    schema()
        .get_field_by_name("body")
        .unwrap()
        .get_type()
        .as_struct_schema()
        .unwrap()
}
fn tag_offset() -> usize {
    let node::Struct(s) = body_schema().get_proto().which().unwrap() else {
        panic!()
    };
    16 + usize::try_from(s.get_discriminant_offset()).unwrap() * 2
}

// Presence deliberately never reads pointer contents. These observers descend
// into active groups only, and therefore also accept malformed/capability slots.
fn observe(reader: dynamic_struct::Reader<'_>) -> Vec<(String, bool, bool)> {
    let mut result = Vec::new();
    for f in reader.get_schema().get_fields().unwrap() {
        let name = f.get_proto().get_name().unwrap().to_str().unwrap();
        let a = reader.has_with_mode(f, HasMode::NonNull).unwrap();
        let b = reader.has_with_mode(f, HasMode::NonDefault).unwrap();
        assert_eq!(reader.has(f).unwrap(), a);
        assert_eq!(reader.has_named(name).unwrap(), a);
        assert_eq!(
            reader
                .has_named_with_mode(name, HasMode::NonDefault)
                .unwrap(),
            b
        );
        result.push((name.into(), a, b));
        if a && matches!(f.get_proto().which().unwrap(), field::Group(_)) {
            result.extend(
                observe(reader.get(f).unwrap().downcast())
                    .into_iter()
                    .map(|(n, a, b)| (format!("{name}.{n}"), a, b)),
            );
        }
    }
    result
}
fn observe_loaded(reader: loaded::Reader<'_, '_>) -> Vec<(String, bool, bool)> {
    let mut result = Vec::new();
    for f in reader.schema().fields().unwrap() {
        let name = f.get_proto().get_name().unwrap().to_str().unwrap();
        let a = reader.has_with_mode(f.clone(), HasMode::NonNull).unwrap();
        let b = reader
            .has_with_mode(f.clone(), HasMode::NonDefault)
            .unwrap();
        assert_eq!(reader.has(f.clone()).unwrap(), a);
        assert_eq!(reader.has_named(name).unwrap(), a);
        assert_eq!(
            reader
                .has_named_with_mode(name, HasMode::NonDefault)
                .unwrap(),
            b
        );
        result.push((name.into(), a, b));
        if a && matches!(f.get_proto().which().unwrap(), field::Group(_)) {
            let loaded::Value::Struct(group) = reader.get(f).unwrap() else {
                panic!()
            };
            result.extend(
                observe_loaded(group)
                    .into_iter()
                    .map(|(n, a, b)| (format!("{name}.{n}"), a, b)),
            );
        }
    }
    result
}
struct Case {
    bytes: Vec<u8>,
    field: &'static str,
    expected: (bool, bool),
}
fn cases() -> Vec<Case> {
    let defaults = read(&frame(|_| {}));
    let defaults = compiled(&defaults);
    let nan32 = defaults
        .get_named("nan32")
        .unwrap()
        .downcast::<f32>()
        .to_bits();
    let nan64 = defaults
        .get_named("nan64")
        .unwrap()
        .downcast::<f64>()
        .to_bits();
    let values: Vec<(&str, value::Reader<'_>)> = vec![
        ("flag", false.into()),
        ("int8", 0i8.into()),
        ("int16", 0i16.into()),
        ("int32", i32::MAX.into()),
        ("int64", i64::MAX.into()),
        ("uint8", 0u8.into()),
        ("uint16", 0u16.into()),
        ("uint32", 0u32.into()),
        ("uint64", 0u64.into()),
        ("zero32", 0.0f32.into()),
        ("zero64", 0.0f64.into()),
        ("nan32", f32::from_bits(nan32 ^ 1).into()),
        ("nan64", f64::from_bits(nan64 ^ 1).into()),
    ];
    let mut cases = vec![Case {
        bytes: frame(|_| {}),
        field: "empty",
        expected: (true, false),
    }];
    for (name, changed) in values {
        for (v, expected) in [(defaults.get_named(name).unwrap(), false), (changed, true)] {
            cases.push(Case {
                bytes: frame(|r| {
                    value::Builder::from(r)
                        .downcast::<dynamic_struct::Builder>()
                        .set_named(name, v)
                        .unwrap();
                }),
                field: name,
                expected: (true, expected),
            });
        }
    }
    for (kind, expected) in [(sample::Kind::Other, false), (sample::Kind::Zero, true)] {
        cases.push(Case {
            bytes: frame(|mut r| r.set_kind(kind)),
            field: "kind",
            expected: (true, expected),
        });
    }
    for name in ["text", "data", "list", "child", "any", "cap"] {
        cases.push(Case {
            bytes: frame(|_| {}),
            field: name,
            expected: (false, false),
        });
        // Equal pointer defaults, empty allocations, invalid pointers and
        // capability indices are all non-null; has() must not dereference them.
        for pointer in [false, true] {
            let mut bytes = frame(|r| {
                let mut r = value::Builder::from(r).downcast::<dynamic_struct::Builder>();
                if matches!(name, "text" | "data" | "list" | "child") {
                    if pointer {
                        r.set_named(name, defaults.get_named(name).unwrap())
                            .unwrap();
                    } else if name == "child" {
                        r.reborrow().init_named(name).unwrap();
                    } else {
                        r.reborrow().initn_named(name, 0).unwrap();
                    }
                }
            });
            if name == "any" || name == "cap" {
                let offset = pointer_offset(schema(), name);
                let wire = if pointer {
                    0xffff_ffff_0000_0003u64
                } else {
                    0xffff_fffc
                };
                bytes[offset..offset + 8].copy_from_slice(&wire.to_le_bytes());
            }
            cases.push(Case {
                bytes,
                field: name,
                expected: (true, true),
            });
        }
    }
    cases
}

#[test]
fn defaults_bits_pointer_nullness_and_both_schema_paths() {
    let loader = loader();
    for case in cases() {
        let m = read(&case.bytes);
        let r = compiled(&m);
        let l = loaded::Reader::new(m.get_root().unwrap(), loaded_schema(&loader)).unwrap();
        assert_eq!(
            (
                r.has_named(case.field).unwrap(),
                r.has_named_with_mode(case.field, HasMode::NonDefault)
                    .unwrap()
            ),
            case.expected,
            "{}",
            case.field
        );
        assert_eq!(observe(r), observe_loaded(l));
        assert!(r.has_named_with_mode("body", HasMode::NonDefault).unwrap());
    }
    let m = read(&frame(|_| {}));
    assert_eq!(
        compiled(&m)
            .get_named("text")
            .unwrap()
            .downcast::<capnp::text::Reader>(),
        "default"
    );
}

#[test]
fn builders_validate_fields_and_do_not_materialize_defaults() -> capnp::Result<()> {
    let loader = loader();
    let mut m = message::Builder::new_default();
    let mut r = value::Builder::from(m.init_root::<sample::Builder>())
        .downcast::<dynamic_struct::Builder>();
    let initial_size = r.reborrow_as_reader().total_size()?;
    let foreign = child::Owned::introspect()
        .as_struct_schema()?
        .get_field_by_name("count")?;
    for mode in MODES {
        assert!(r.has_with_mode(foreign, mode).is_err());
        assert!(r.has_named_with_mode("missing", mode).is_err());
        for f in schema().get_fields()? {
            let name = f.get_proto().get_name()?.to_str()?;
            assert_eq!(
                r.has_with_mode(f, mode)?,
                r.reborrow_as_reader().has_with_mode(f, mode)?
            );
            assert_eq!(
                r.has_named_with_mode(name, mode)?,
                r.has_with_mode(f, mode)?
            );
        }
    }
    assert_eq!(initial_size, r.reborrow_as_reader().total_size()?);
    r.set_named("uint32", 0u32.into())?;
    assert!(r.has_named_with_mode("uint32", HasMode::NonDefault)?);
    r.clear_named("uint32")?;
    assert!(!r.has_named_with_mode("uint32", HasMode::NonDefault)?);
    let before = capnp::serialize::write_message_to_words(&m);
    {
        let r = loaded::Builder::new(m.get_root()?, loaded_schema(&loader))?;
        let schema_loader::Type::Struct(child) = r.schema().field("child")?.get_type()? else {
            panic!()
        };
        for mode in MODES {
            assert!(r.has_with_mode(child.field("count")?, mode).is_err());
            assert!(r.has_named_with_mode("missing", mode).is_err());
            for f in r.schema().fields()? {
                let name = f.get_proto().get_name()?.to_str()?;
                assert_eq!(
                    r.has_with_mode(f.clone(), mode)?,
                    r.as_reader().has_with_mode(f.clone(), mode)?
                );
                assert_eq!(
                    r.has_named_with_mode(name, mode)?,
                    r.has_with_mode(f.clone(), mode)?
                );
                assert_eq!(r.has_named(name)?, r.has(f)?);
            }
        }
    }
    assert_eq!(before, capnp::serialize::write_message_to_words(&m));
    Ok(())
}

#[test]
fn short_structs_and_unknown_union_tags_keep_wire_semantics() {
    // Empty struct (including zero data and pointer sections) viewed through a
    // newer schema. Missing words behave as zero, rather than decoded defaults.
    let bytes = [0u8, 0, 0, 0, 1, 0, 0, 0, 0xfc, 0xff, 0xff, 0xff, 0, 0, 0, 0];
    let m = read(&bytes);
    let loader = loader();
    let r = compiled(&m);
    assert_eq!(
        observe(r),
        observe_loaded(loaded::Reader::new(m.get_root().unwrap(), loaded_schema(&loader)).unwrap())
    );
    assert!(r.has_named("uint32").unwrap());
    assert!(!r
        .has_named_with_mode("uint32", HasMode::NonDefault)
        .unwrap());
    assert!(!r.has_named("text").unwrap());
    let mut bytes = frame(|r| {
        r.get_body().set_note("default");
    });
    bytes[tag_offset()..tag_offset() + 2].copy_from_slice(&999u16.to_le_bytes());
    let m = read(&bytes);
    let r = compiled(&m);
    let body = r
        .get_named("body")
        .unwrap()
        .downcast::<dynamic_struct::Reader>();
    assert!(body.which().unwrap().is_none());
    for mode in MODES {
        for name in ["none", "number", "note", "details"] {
            assert!(!body.has_named_with_mode(name, mode).unwrap());
        }
    }
    assert_eq!(
        observe(r),
        observe_loaded(loaded::Reader::new(m.get_root().unwrap(), loaded_schema(&loader)).unwrap())
    );
}

#[test]
fn detached_compiled_groups_compare_decoded_scalars_by_bits() -> capnp::Result<()> {
    let mut m = message::Builder::new_default();
    let (mut root, token) = value::Builder::from(m.init_root::<sample::Builder>())
        .downcast::<dynamic_struct::Builder>()
        .with_orphanage();
    let mut group = token.in_struct(&mut root)?.new_group(body_schema())?;
    let mode = HasMode::NonDefault;
    let nan = token
        .in_struct(&mut root)?
        .read_group(&mut group, |mut g| {
            for name in [
                "count", "zero", "nan", "kind", "none", "number", "note", "details", "label",
            ] {
                assert!(!g.has_named_with_mode(name, mode)?, "{name}");
            }
            assert!(g.has_named("count")?);
            assert!(g.has_named("none")?);
            assert!(g
                .has_with_mode(schema().get_field_by_name("flag")?, mode)
                .is_err());
            g.read_named("nan", |v| Ok(v.downcast::<f64>()))
        })?;
    token
        .in_struct(&mut root)?
        .edit_group(&mut group, |mut g| {
            for (name, v) in [
                ("count", 42u32.into()),
                ("zero", (-0.0f32).into()),
                ("nan", nan.into()),
            ] {
                g.set_named(name, v)?;
                assert!(!g.has_named_with_mode(name, mode)?);
            }
            for (name, v) in [
                ("count", 0u32.into()),
                ("zero", 0.0f32.into()),
                ("nan", f64::from_bits(nan.to_bits() ^ 1).into()),
            ] {
                g.set_named(name, v)?;
                let f = g.get_schema().get_field_by_name(name)?;
                assert!(g.has_with_mode(f, mode)?);
                g.clear_named(name)?;
                assert!(!g.has_named_with_mode(name, mode)?);
            }
            g.set_named("label", value::Reader::Text("default".into()))?;
            assert!(g.has_named_with_mode("label", mode)?);
            g.clear_named("label")?;
            assert!(!g.has_named_with_mode("label", mode)?);
            g.set_named("number", 42u32.into())?;
            assert!(g.has_named("number")?);
            assert!(!g.has_named_with_mode("number", mode)?);
            assert!(!g.has_named("none")?);
            g.set_named("note", value::Reader::Text("default".into()))?;
            assert!(g.has_named_with_mode("note", mode)?);
            assert!(!g.has_named("number")?);
            g.clear_named("details")?;
            assert!(g.has_named_with_mode("details", mode)?);
            g.read_group_named("details", |mut d| {
                assert!(d.has_named("flag")?);
                assert!(!d.has_named_with_mode("flag", mode)?);
                Ok(())
            })
        })?;
    root.adopt_named("body", group).unwrap();
    let body = root
        .reborrow_as_reader()
        .get_named("body")?
        .downcast::<dynamic_struct::Reader>();
    assert!(body.has_named_with_mode("details", mode)?);
    assert!(!body.has_named_with_mode("count", mode)?);
    Ok(())
}

#[test]
fn detached_loaded_groups_preserve_defaults_and_union_selection() -> capnp::Result<()> {
    use loaded::Value as V;
    let loader = loader();
    let mut m = message::Builder::new_default();
    let (mut root, token) =
        loaded::Builder::init(m.init_root(), loaded_schema(&loader))?.with_orphanage();
    let schema_loader::Type::Struct(body) = root.schema().field("body")?.get_type()? else {
        panic!()
    };
    let mut group = token.in_struct(&mut root)?.new_group(body)?;
    let mode = HasMode::NonDefault;
    let (nan, kind) = token
        .in_struct(&mut root)?
        .read_group(&mut group, |mut g| {
            for name in [
                "count", "zero", "nan", "kind", "none", "number", "note", "details", "label",
            ] {
                assert!(!g.has_named_with_mode(name, mode)?, "{name}");
            }
            assert!(g.has_named("count")?);
            assert!(g.has_named("none")?);
            assert!(g
                .has_with_mode(loaded_schema(&loader).field("flag")?, mode)
                .is_err());
            let nan = g.read_named("nan", |v| {
                let V::Float64(v) = v else { panic!() };
                Ok(v)
            })?;
            let schema_loader::Type::Enum(kind) = g.get_schema().field("kind")?.get_type()? else {
                panic!()
            };
            Ok((nan, kind))
        })?;
    token
        .in_struct(&mut root)?
        .edit_group(&mut group, |mut g| {
            for (name, v) in [
                ("count", V::UInt32(42)),
                ("zero", V::Float32(-0.0)),
                ("nan", V::Float64(nan)),
                ("kind", V::Enum(1, kind.clone())),
            ] {
                g.set_named(name, v)?;
                assert!(!g.has_named_with_mode(name, mode)?);
            }
            for (name, v) in [
                ("count", V::UInt32(0)),
                ("zero", V::Float32(0.0)),
                ("nan", V::Float64(f64::from_bits(nan.to_bits() ^ 1))),
                ("kind", V::Enum(u16::MAX, kind)),
            ] {
                g.set_named(name, v)?;
                assert!(g.has_with_mode(g.get_schema().field(name)?, mode)?);
                g.clear_named(name)?;
                assert!(!g.has_named_with_mode(name, mode)?);
            }
            g.set_named("label", V::Text("default".into()))?;
            assert!(g.has_named_with_mode("label", mode)?);
            g.clear_named("label")?;
            assert!(!g.has_named_with_mode("label", mode)?);
            g.set_named("number", V::UInt32(42))?;
            assert!(g.has_named("number")?);
            assert!(!g.has_named_with_mode("number", mode)?);
            assert!(!g.has_named("none")?);
            g.set_named("note", V::Text("default".into()))?;
            assert!(g.has_named_with_mode("note", mode)?);
            assert!(!g.has_named("number")?);
            g.clear_named("details")?;
            assert!(g.has_named_with_mode("details", mode)?);
            g.read_group_named("details", |mut d| {
                assert!(d.has_named("flag")?);
                assert!(!d.has_named_with_mode("flag", mode)?);
                Ok(())
            })
        })?;
    root.adopt_named("body", group).unwrap();
    let V::Struct(body) = root.as_reader().get_named("body")? else {
        panic!()
    };
    assert!(body.has_named_with_mode("details", mode)?);
    assert!(!body.has_named_with_mode("count", mode)?);
    Ok(())
}

#[test]
fn capability_presence_does_not_resolve_or_retain_clients() -> capnp::Result<()> {
    use capnp::{
        capability::FromClientHook,
        traits::{Imbue, ImbueMut},
    };
    use capntproto_test_support::runtime_test_capnp::harness;
    use std::{cell::Cell, rc::Rc};
    let polls = Rc::new(Cell::new(0));
    let marker = Rc::new(());
    let weak = Rc::downgrade(&marker);
    let count = polls.clone();
    let client: harness::Client = capnp_rpc::new_future_client(async move {
        count.set(count.get() + 1);
        drop(marker);
        Err(capnp::Error::failed("unresolved target".into()))
    });
    let mut caps = capnp::private::layout::CapTable::default();
    let mut m = message::Builder::new_default();
    let loader = loader();
    {
        let mut r = m.init_root::<sample::Builder>();
        r.imbue_mut(&mut caps);
        r.get_cap().set_as_capability(client.into_client_hook());
    }
    {
        let mut r = m.get_root_as_reader::<sample::Reader>()?;
        r.imbue(&caps);
        let r = value::Reader::from(r).downcast::<dynamic_struct::Reader>();
        let mut pointer = m.get_root_as_reader::<capnp::any_pointer::Reader>()?;
        pointer.imbue(&caps);
        let l = loaded::Reader::new(pointer, loaded_schema(&loader))?;
        for mode in MODES {
            assert!(r.has_named_with_mode("cap", mode)?);
            assert!(l.has_named_with_mode("cap", mode)?);
        }
    }
    assert_eq!(polls.get(), 0);
    assert_eq!(caps.iter().filter(|c| c.is_some()).count(), 1);
    drop(caps);
    assert!(weak.upgrade().is_none());
    Ok(())
}
