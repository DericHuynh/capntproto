use capnp::{
    message,
    schema_capnp::{brand, code_generator_request, node},
    schema_loader::{
        dynamic::{self, Value},
        PointerKind, Schema, SchemaLoader, Type,
    },
};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn request() -> message::Reader<capnp::serialize::OwnedSegments> {
    let output = Command::new("capnp")
        .args([
            "compile",
            "-o-",
            "-Ivendor/capnproto/c++/src",
            "--src-prefix=schemas",
            "schemas/reflection-inspection.capnp",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    capnp::serialize::read_message(output.stdout.as_slice(), Default::default()).unwrap()
}
fn loaded() -> SchemaLoader {
    let mut loader = SchemaLoader::default();
    loader
        .load_request(
            request()
                .get_root::<code_generator_request::Reader>()
                .unwrap(),
        )
        .unwrap();
    loader
}
fn schema<'a>(loader: &'a SchemaLoader, name: &str) -> Schema<'a> {
    loader
        .get_all_loaded()
        .find(|s| {
            s.get_proto()
                .get_display_name()
                .unwrap()
                .to_str()
                .unwrap()
                .ends_with(&format!(":{name}"))
        })
        .unwrap()
}
fn brand(id: u64, mode: u8) -> message::Builder<message::HeapAllocator> {
    let mut message = message::Builder::new_default();
    let mut scope = message.init_root::<brand::Builder>().init_scopes(1).get(0);
    scope.set_scope_id(id);
    match mode {
        2 => {
            scope.init_bind(0);
        }
        3 => {
            scope.init_bind(1).get(0).set_unbound(());
        }
        4 => {
            scope.init_bind(1).get(0).init_type().set_text(());
        }
        5 | 6 => scope.set_inherit(()),
        _ => panic!(),
    }
    message
}
fn variant<'a>(base: &Schema<'a>, loader: &'a SchemaLoader, mode: u8) -> Schema<'a> {
    match mode {
        0 => base.generic(),
        1 => loader.get_unbound(base.id()).unwrap(),
        2..=6 => {
            let scope = if mode == 6 {
                loader.get_unbound(base.id()).unwrap()
            } else {
                base.generic()
            };
            base.bind(
                brand(base.id(), mode).get_root_as_reader().unwrap(),
                Some(&scope),
            )
            .unwrap()
        }
        _ => panic!(),
    }
}
fn type_name(ty: Type<'_>) -> String {
    match ty {
        Type::AnyPointer(PointerKind::Any) => "any".into(),
        Type::Parameter(id, index) => format!("p{id}:{index}"),
        Type::Text => "text".into(),
        Type::Data => "data".into(),
        Type::List(ty) => format!("list({})", type_name(*ty)),
        Type::Struct(s) => format!("struct{}", s.id()),
        Type::Interface(s) => format!("interface{}", s.id()),
        _ => panic!("unexpected fixture type"),
    }
}

#[test]
fn brand_inspection_distinguishes_defaults_explicit_arguments_and_symbolic_scopes() {
    let loader = loaded();
    let base = schema(&loader, "Scope");
    for mode in 0..7 {
        let s = variant(&base, &loader, mode);
        assert_eq!(s.is_branded(), mode != 0);
        assert!(s.generic().equals(&base));
        assert!(s.generic().generic().equals(&base));
        assert_eq!(
            s.generic_scope_ids(),
            if mode >= 2 { vec![base.id()] } else { vec![] }
        );
        let args = s.brand_arguments_at_scope(base.id()).unwrap();
        assert_eq!(args.len(), u32::from(mode == 3 || mode == 4));
        assert_eq!(args.iter().len(), args.len() as usize);
        assert_eq!(args.is_empty(), !matches!(mode, 3 | 4));
        assert_eq!(
            type_name(args.get(0)),
            match mode {
                1 | 6 => format!("p{}:0", base.id()),
                4 => "text".into(),
                _ => "any".into(),
            }
        );
        let beyond = args.get(u16::MAX);
        assert_eq!(
            type_name(beyond),
            if mode == 1 || mode == 6 {
                format!("p{}:{}", base.id(), u16::MAX)
            } else {
                "any".into()
            }
        );
        let absent = s.brand_arguments_at_scope(999).unwrap();
        assert_eq!(absent.len(), 0);
        assert_eq!(
            type_name(absent.get(2)),
            if mode == 1 { "p999:2" } else { "any" }
        );
        if mode != 0 {
            assert!(!s.equals(&base));
        }
    }
    let ordinary = schema(&loader, "Cases");
    assert!(!loader.get_unbound(ordinary.id()).unwrap().is_branded());
    assert!(ordinary.brand_arguments_at_scope(base.id()).is_err());
    let Type::Struct(inner) = ordinary.field("nested").unwrap().get_type().unwrap() else {
        panic!()
    };
    let mut scopes = vec![base.id(), inner.id()];
    scopes.sort();
    assert_eq!(inner.generic_scope_ids(), scopes);
    assert_eq!(
        type_name(inner.brand_arguments_at_scope(base.id()).unwrap().get(0)),
        "text"
    );
    assert_eq!(
        type_name(inner.brand_arguments_at_scope(base.id()).unwrap().get(1)),
        "data"
    );
    assert_eq!(
        type_name(inner.brand_arguments_at_scope(inner.id()).unwrap().get(0)),
        "list(text)"
    );
    assert_eq!(
        type_name(inner.field("outer").unwrap().get_type().unwrap()),
        "text"
    );
    assert_eq!(
        type_name(inner.field("local").unwrap().get_type().unwrap()),
        "list(text)"
    );
    let other = loader.clone();
    assert!(!inner.generic().equals(&other.get(inner.id()).unwrap()));

    // Rebinding an unbound receiver must not retain its fallback symbolic scope.
    let unbound = loader.get_unbound(base.id()).unwrap();
    let any = unbound
        .bind(brand(base.id(), 3).get_root_as_reader().unwrap(), None)
        .unwrap();
    assert_eq!(
        type_name(any.field("second").unwrap().get_type().unwrap()),
        "any"
    );
    let empty = message::Builder::new_default();
    assert!(unbound
        .bind(empty.get_root_as_reader().unwrap(), None)
        .unwrap()
        .equals(&base));
    let mut duplicates = message::Builder::new_default();
    let mut scopes = duplicates.init_root::<brand::Builder>().init_scopes(2);
    for i in 0..2 {
        let mut s = scopes.reborrow().get(i);
        s.set_scope_id(base.id());
        s.set_inherit(());
    }
    assert!(base
        .bind(duplicates.get_root_as_reader().unwrap(), None)
        .is_err());
}

fn fields(fields: Vec<capnp::schema_loader::Field<'_>>) -> String {
    fields
        .into_iter()
        .map(|f| f.index().to_string())
        .collect::<Vec<_>>()
        .join(",")
}
fn choice_reader<'a>(
    message: &'a message::Reader<capnp::serialize::OwnedSegments>,
    schema: Schema<'a>,
) -> dynamic::Reader<'a, 'a> {
    dynamic::Reader::new(message.get_root().unwrap(), schema).unwrap()
}
fn choice_message(
    schema: Schema<'_>,
    tag: u16,
) -> message::Reader<capnp::serialize::OwnedSegments> {
    let node::Struct(s) = schema.get_proto().which().unwrap() else {
        panic!()
    };
    let mut message = message::Builder::new_default();
    let mut builder = dynamic::Builder::init(message.init_root(), schema).unwrap();
    builder.set_named("serial", Value::UInt32(123)).unwrap();
    let mut bytes = capnp::serialize::write_message_to_words(&message);
    // The small fixture occupies one segment: framing, root pointer, data.
    let offset = 16 + 2 * s.get_discriminant_offset() as usize;
    bytes[offset..offset + 2].copy_from_slice(&tag.to_le_bytes());
    capnp::serialize::read_message(bytes.as_slice(), Default::default()).unwrap()
}

#[test]
fn union_subsets_keep_context_and_unknown_tags_never_select_non_union_fields() {
    let loader = loaded();
    let Type::Struct(choice) = schema(&loader, "Cases")
        .field("choice")
        .unwrap()
        .get_type()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(fields(choice.union_fields().unwrap()), "1,2,3");
    assert_eq!(fields(choice.non_union_fields().unwrap()), "0,4,5");
    assert!(choice.find_field("absent").unwrap().is_none());
    assert!(schema(&loader, "Service").find_field("call").is_err());
    for f in choice.union_fields().unwrap() {
        assert!(f.parent().equals(&choice));
    }
    assert_eq!(
        type_name(
            choice
                .field_by_discriminant(1)
                .unwrap()
                .unwrap()
                .get_type()
                .unwrap()
        ),
        "text"
    );
    let Type::Struct(named) = choice.field("named").unwrap().get_type().unwrap() else {
        panic!()
    };
    assert_eq!(named.union_fields().unwrap().len(), 2);
    assert!(named.non_union_fields().unwrap().is_empty());
    assert!(choice.find_field("left").unwrap().is_none());
    for tag in [0, 1, 2, 3, 42, u16::MAX] {
        let m = choice_message(choice.clone(), tag);
        let r = choice_reader(&m, choice.clone());
        let expected = if tag < 3 { Some(tag + 1) } else { None };
        assert_eq!(
            choice
                .field_by_discriminant(tag)
                .unwrap()
                .map(|f| f.index()),
            expected
        );
        assert_eq!(r.which().unwrap().map(|f| f.index()), expected);
        assert!(matches!(r.get_named("serial").unwrap(), Value::UInt32(123)));
        if tag >= 3 {
            for f in choice.union_fields().unwrap() {
                assert!(!r.has(f.clone()).unwrap());
                assert!(r.get(f).is_err());
            }
        }
    }
    let ordinary = schema(&loader, "Cases");
    assert!(ordinary.union_fields().unwrap().is_empty());
    assert!(ordinary.field_by_discriminant(0).unwrap().is_none());
    assert!(ordinary.field_by_discriminant(u16::MAX).unwrap().is_none());
}

fn inspect_schema(lines: &mut Vec<String>, label: &str, s: Schema<'_>, loader: &SchemaLoader) {
    let ids = s.generic_scope_ids();
    let list = |values: Vec<String>| {
        if values.is_empty() {
            String::new()
        } else {
            format!(" {}", values.join(" "))
        }
    };
    lines.push(format!(
        "s {label} {} {} scopes{}",
        u8::from(s.is_branded()),
        u8::from(s.generic().equals(&loader.get(s.id()).unwrap())),
        list(ids.iter().map(u64::to_string).collect())
    ));
    if s.get_proto().get_is_generic() {
        let mut scopes = vec![s.id(), 999];
        scopes.extend(ids.into_iter().filter(|id| *id != s.id()));
        for id in scopes {
            let args = s.brand_arguments_at_scope(id).unwrap();
            lines.push(format!(
                "a {label} {id} {}{}",
                args.len(),
                list(
                    [0, 1, 2, u16::MAX]
                        .into_iter()
                        .map(|i| type_name(args.get(i)))
                        .collect()
                )
            ));
        }
    }
    if s.kind() == capnp::schema_loader::Kind::Struct {
        lines.push(format!(
            "u {label} union{} other{} tags{}",
            list(
                s.union_fields()
                    .unwrap()
                    .iter()
                    .map(|f| f.index().to_string())
                    .collect()
            ),
            list(
                s.non_union_fields()
                    .unwrap()
                    .iter()
                    .map(|f| f.index().to_string())
                    .collect()
            ),
            list(
                [0, 1, 2, 3, 42, u16::MAX]
                    .into_iter()
                    .map(|tag| s
                        .field_by_discriminant(tag)
                        .unwrap()
                        .map_or("-".into(), |f| f.index().to_string()))
                    .collect()
            )
        ));
    }
}

#[test]
fn pinned_cpp_agrees_on_brands_scopes_arguments_and_union_subsets() {
    let executable = capntproto_test_support::verification::cpp::loader().unwrap();
    let request = request();
    let mut loader = SchemaLoader::default();
    loader
        .load_request(
            request
                .get_root::<code_generator_request::Reader>()
                .unwrap(),
        )
        .unwrap();
    let mut expected = Vec::new();
    for s in loader.get_all_loaded() {
        let count = if s.get_proto().get_parameters().unwrap().is_empty() {
            2
        } else {
            7
        };
        for mode in 0..count {
            inspect_schema(
                &mut expected,
                &format!("{}/{mode}", s.id()),
                variant(&s, &loader, mode),
                &loader,
            );
        }
        if s.kind() == capnp::schema_loader::Kind::Struct {
            for f in s.fields().unwrap() {
                if let Type::Struct(t) | Type::Interface(t) = f.get_type().unwrap() {
                    inspect_schema(
                        &mut expected,
                        &format!("{}/field{}", s.id(), f.index()),
                        t,
                        &loader,
                    );
                }
            }
        }
    }
    let mut child = Command::new(executable)
        .arg("introspection")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut message = message::Builder::new_default();
    message
        .set_root(
            request
                .get_root::<code_generator_request::Reader>()
                .unwrap(),
        )
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&capnp::serialize::write_message_to_words(&message))
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut actual: Vec<_> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected);
    eprintln!("{} introspection records matched pinned C++", actual.len());
}

fn compiled_union(tag: u16) {
    use capnp::introspect::Introspect;
    use capntproto_test_support::dynamic_test_capnp::brand_envelope;
    let schema = brand_envelope::Owned::introspect()
        .as_struct_schema()
        .unwrap();
    let node::Struct(s) = schema.get_proto().which().unwrap() else {
        panic!()
    };
    let mut message = message::Builder::new_default();
    message.init_root::<brand_envelope::Builder>();
    let mut bytes = capnp::serialize::write_message_to_words(&message);
    let offset = 16 + 2 * s.get_discriminant_offset() as usize;
    bytes[offset..offset + 2].copy_from_slice(&tag.to_le_bytes());
    let input = capnp::serialize::read_message(bytes.as_slice(), Default::default()).unwrap();
    let native = input.get_root::<brand_envelope::Reader>().unwrap();
    let reader =
        capnp::dynamic_value::Reader::from(native).downcast::<capnp::dynamic_struct::Reader>();
    let cap = schema.get_field_by_name("cap").unwrap();
    assert_eq!(reader.get(cap).is_ok(), tag == 1);
    assert_eq!(
        reader.which().unwrap().map(|f| f.get_index()),
        if tag < 2 { Some(tag) } else { None }
    );
    let mut copy = message::Builder::new_default();
    copy.set_root(native).unwrap();
    let builder =
        capnp::dynamic_value::Builder::from(copy.get_root::<brand_envelope::Builder>().unwrap())
            .downcast::<capnp::dynamic_struct::Builder>();
    assert_eq!(builder.get(cap).is_ok(), tag == 1);
}

#[test]
fn inactive_union_arms_cannot_expose_retained_capability_pointers() {
    use capnp::traits::{HasTypeId, Imbue, ImbueMut};
    use capntproto_test_support::dynamic_test_capnp::{base, brand_envelope};
    struct Server;
    impl base::Server<capnp::text::Owned> for Server {}
    let client: base::Client<capnp::text::Owned> = capnp_rpc::new_client(Server);
    let mut loader = SchemaLoader::default();
    loader
        .load_compiled_type_and_dependencies::<brand_envelope::Owned>()
        .unwrap();
    let schema = loader.get(brand_envelope::Reader::TYPE_ID).unwrap();
    let mut message = message::Builder::new_default();
    let mut caps = capnp::private::layout::CapTable::default();
    {
        let mut root = message.init_root::<brand_envelope::Builder>();
        root.imbue_mut(&mut caps);
        root.set_cap(client);
    }
    let held = {
        let mut pointer = message
            .get_root_as_reader::<capnp::any_pointer::Reader>()
            .unwrap();
        pointer.imbue(&caps);
        let Value::Capability(cap) = dynamic::Reader::new(pointer, schema.clone())
            .unwrap()
            .get_named("cap")
            .unwrap()
        else {
            panic!()
        };
        cap
    };
    assert!(held.as_client().is_ok());
    message
        .get_root::<brand_envelope::Builder>()
        .unwrap()
        .set_sentinel(42);
    // Scalar selection leaves the capability pointer physically present. Both
    // reflection APIs must respect the union tag before reading that pointer.
    assert!(caps.iter().any(Option::is_some));
    let mut pointer = message
        .get_root_as_reader::<capnp::any_pointer::Reader>()
        .unwrap();
    pointer.imbue(&caps);
    let reader = dynamic::Reader::new(pointer, schema).unwrap();
    assert!(reader.get_named("cap").is_err());
    assert!(matches!(
        reader.get_named("sentinel").unwrap(),
        Value::UInt32(42)
    ));
    let mut native = message
        .get_root_as_reader::<brand_envelope::Reader>()
        .unwrap();
    native.imbue(&caps);
    let compiled =
        capnp::dynamic_value::Reader::from(native).downcast::<capnp::dynamic_struct::Reader>();
    assert!(compiled.get_named("cap").is_err());
    assert!(held.as_client().is_ok());
}

#[test]
fn tlc_introspection_model_replays_brand_transitions_and_union_reads() {
    use capntproto_test_support::verification::exploration::{controls, traces};
    const MODEL: &str = "verification/RpcSchemaIntrospection.tla";
    const CFG: &str = include_str!("../verification/RpcSchemaIntrospection.cfg");
    controls(
        MODEL,
        "schema-introspection",
        CFG,
        &[
            ("dropExplicitDefault", "ExplicitScopes"),
            ("keepSymbolicOnErase", "ResolvedArguments"),
            ("selectNonUnion", "UnknownUnion"),
            ("readInactive", "ActiveRead"),
        ],
        None,
    )
    .unwrap();
    let loader = loaded();
    let base = schema(&loader, "Scope");
    let Type::Struct(choice) = schema(&loader, "Cases")
        .field("choice")
        .unwrap()
        .get_type()
        .unwrap()
    else {
        panic!()
    };
    let mut coverage = std::collections::BTreeSet::new();
    for trace in traces(MODEL, "schema-introspection", CFG).unwrap() {
        let mut current = base.clone();
        for state in trace {
            match state["event"] {
                1 => current = current.generic(),
                2 | 3 => {
                    let b = brand(base.id(), if state["event"] == 2 { 3 } else { 4 });
                    current = current.bind(b.get_root_as_reader().unwrap(), None).unwrap();
                }
                4 => current = loader.get_unbound(base.id()).unwrap(),
                5 => {
                    let b = brand(base.id(), 5);
                    current = base
                        .bind(b.get_root_as_reader().unwrap(), Some(&current))
                        .unwrap();
                }
                6 => {
                    let tag = if state["tag"] == 2 {
                        u16::MAX
                    } else {
                        state["tag"] as u16
                    };
                    let m = choice_message(choice.clone(), tag);
                    let reader = choice_reader(&m, choice.clone());
                    assert_eq!(
                        u64::from(reader.which().unwrap().map_or(0, |f| f.index())),
                        state["selected"]
                    );
                    assert_eq!(
                        u64::from(reader.get_named("value").is_ok()),
                        state["readable"]
                    );
                    compiled_union(tag);
                }
                _ => panic!(),
            }
            assert_eq!(u64::from(current.is_branded()), state["branded"]);
            assert_eq!(current.generic_scope_ids().len() as u64, state["scopes"]);
            let args = current.brand_arguments_at_scope(base.id()).unwrap();
            assert_eq!(u64::from(args.len()), state["count"]);
            let argument = match args.get(0) {
                Type::AnyPointer(PointerKind::Any) => 0,
                Type::Text => 1,
                Type::Parameter(id, 0) => {
                    assert_eq!(id, base.id());
                    2
                }
                _ => panic!(),
            };
            assert_eq!(argument, state["argument"]);
            assert!(current.generic().equals(&base));
            coverage.insert((state["mode"], state["tag"]));
        }
    }
    for mode in 0..6 {
        for tag in 0..3 {
            assert!(coverage.contains(&(mode, tag)));
        }
    }
}
