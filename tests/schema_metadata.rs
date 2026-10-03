use capnp::{
    message,
    schema_capnp::{code_generator_request, node},
    schema_loader::{
        dynamic::{self, Value},
        AnnotationList, Kind, Schema, SchemaLoader, Type,
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
            "schemas/reflection-metadata.capnp",
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
fn describe(value: Value<'_, '_>) -> String {
    match value {
        Value::Unknown(tag) => format!("unknown:{tag}"),
        Value::Void => "void".into(),
        Value::Bool(v) => format!("b{}", u8::from(v)),
        Value::Int8(v) => format!("i{v}"),
        Value::Int16(v) => format!("i{v}"),
        Value::Int32(v) => format!("i{v}"),
        Value::Int64(v) => format!("i{v}"),
        Value::UInt8(v) => format!("u{v}"),
        Value::UInt16(v) => format!("u{v}"),
        Value::UInt32(v) => format!("u{v}"),
        Value::UInt64(v) => format!("u{v}"),
        Value::Float32(v) => format!("f{v}"),
        Value::Float64(v) => format!("f{v}"),
        Value::Text(v) => format!("t:{}", v.to_str().unwrap()),
        Value::Data(v) => format!("d:{v:?}"),
        Value::Enum(v, s) => format!("e{}:{v}", s.id()),
        Value::Struct(v) => format!(
            "s{}:[{}]",
            v.schema().id(),
            v.schema()
                .fields()
                .unwrap()
                .into_iter()
                .map(|f| describe(v.get(f).unwrap()))
                .collect::<Vec<_>>()
                .join(";")
        ),
        Value::List(v) => format!(
            "[{}]",
            (0..v.len())
                .map(|i| describe(v.get(i).unwrap()))
                .collect::<Vec<_>>()
                .join(";")
        ),
        Value::AnyPointer(v) => {
            assert!(v.is_null());
            "null".into()
        }
        Value::Capability(v) => {
            assert!(v.as_client().is_err());
            "null-cap".into()
        }
    }
}

#[test]
fn constants_and_annotations_keep_values_brands_and_member_context() {
    let loader = loaded();
    for (name, expected) in [
        ("answer", "u42"),
        ("greeting", "t:hello"),
        ("bytes", "d:[0, 255, 16]"),
        ("words", "[t:first;t:second]"),
        ("nothing", "void"),
        ("yes", "b1"),
        ("i8", "i-8"),
        ("i16", "i-160"),
        ("i32", "i-320"),
        ("i64", "i-640"),
        ("u8", "u8"),
        ("u16", "u160"),
        ("u64", "u640"),
        ("f32", "f1.5"),
        ("f64", "f-2.25"),
    ] {
        assert_eq!(
            describe(schema(&loader, name).constant_value().unwrap()),
            expected
        );
    }
    let record = schema(&loader, "Record");
    assert!(record.constant_type().is_err());
    assert!(record.constant_value().is_err());
    assert!(record.enumerants().is_err());
    let note = schema(&loader, "note").id();
    let annotations = record.annotations().unwrap();
    assert_eq!(annotations.len(), 2);
    assert!(annotations.get(2).is_err());
    assert!(annotations.find(0).unwrap().is_none());
    assert_eq!(
        describe(
            annotations
                .find(note)
                .unwrap()
                .unwrap()
                .get_value()
                .unwrap()
        ),
        "t:record"
    );
    let generic = annotations.get(1).unwrap();
    assert_eq!(generic.id(), schema(&loader, "Box.label").id());
    assert_eq!(generic.declaration().kind(), Kind::Annotation);
    assert_eq!(generic.get_proto().get_id(), generic.id());
    assert!(matches!(generic.get_type().unwrap(), Type::Text));
    assert_eq!(describe(generic.get_value().unwrap()), "t:generic");
    for (annotations, expected) in [
        (
            record.field("name").unwrap().annotations().unwrap(),
            "t:field",
        ),
        (record.field("count").unwrap().annotations().unwrap(), "u9"),
        (
            schema(&loader, "Service").annotations().unwrap(),
            "t:interface",
        ),
        (
            schema(&loader, "Service")
                .method("call")
                .unwrap()
                .annotations()
                .unwrap(),
            "t:method",
        ),
        (schema(&loader, "Color").annotations().unwrap(), "t:palette"),
        (
            schema(&loader, "Color")
                .enumerant("red")
                .unwrap()
                .annotations()
                .unwrap(),
            "t:warm",
        ),
        (
            schema(&loader, "Color")
                .enumerant("blue")
                .unwrap()
                .annotations()
                .unwrap(),
            "u7",
        ),
    ] {
        assert_eq!(
            describe(annotations.get(0).unwrap().get_value().unwrap()),
            expected
        );
    }
    let colors = schema(&loader, "Color");
    let members = colors.enumerants().unwrap();
    assert_eq!(members.len(), 2);
    assert_eq!(members[1].index(), 1);
    assert!(members[0].parent().equals(&colors));
    assert!(colors.enumerant("missing").is_err());
    assert!(schema(&loader, "answer").annotations().unwrap().is_empty());

    let mut message = message::Builder::new_default();
    let mut destination =
        dynamic::Builder::init(message.init_root(), schema(&loader, "Destination")).unwrap();
    for field in ["record", "records", "color", "boxed"] {
        let source = schema(&loader, field).constant_value().unwrap();
        destination.set_named(field, source.clone()).unwrap();
        assert_eq!(
            describe(destination.as_reader().get_named(field).unwrap()),
            describe(source)
        );
    }
    let Value::Struct(boxed) = destination.as_reader().get_named("boxed").unwrap() else {
        panic!()
    };
    assert!(matches!(
        boxed.schema().field("value").unwrap().get_type().unwrap(),
        Type::Text
    ));
    assert_eq!(describe(boxed.get_named("value").unwrap()), "t:inside");
    let annotation = schema(&loader, "Destination")
        .annotations()
        .unwrap()
        .get(0)
        .unwrap();
    destination
        .set_named("record", annotation.get_value().unwrap())
        .unwrap();
    let Value::Struct(record) = destination.as_reader().get_named("record").unwrap() else {
        panic!()
    };
    assert_eq!(describe(record.get_named("name").unwrap()), "t:annotated");
}

#[test]
fn metadata_copy_rejects_foreign_schema_authority_without_changing_destination() {
    let loader = loaded();
    let foreign = loaded();
    let mut message = message::Builder::new_default();
    let mut destination =
        dynamic::Builder::init(message.init_root(), schema(&loader, "Destination")).unwrap();
    for field in ["record", "records", "color", "boxed"] {
        let value = schema(&loader, field).constant_value().unwrap();
        destination.set_named(field, value.clone()).unwrap();
        assert!(destination
            .set_named(field, schema(&foreign, field).constant_value().unwrap())
            .is_err());
        assert_eq!(
            describe(destination.as_reader().get_named(field).unwrap()),
            describe(value)
        );
    }
}

fn annotation_lines(output: &mut Vec<String>, parent: u64, member: &str, list: AnnotationList<'_>) {
    for a in list.iter() {
        let a = a.unwrap();
        output.push(format!(
            "a {parent} {member} {} {}",
            a.id(),
            describe(a.get_value().unwrap())
        ));
    }
}

#[test]
fn pinned_cpp_agrees_on_constant_and_annotation_metadata() {
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
        if s.kind() == Kind::Const {
            expected.push(format!(
                "c {} {}",
                s.id(),
                describe(s.constant_value().unwrap())
            ));
        }
        annotation_lines(&mut expected, s.id(), "node", s.annotations().unwrap());
        match s.kind() {
            Kind::Struct => {
                for f in s.fields().unwrap() {
                    annotation_lines(
                        &mut expected,
                        s.id(),
                        &format!("field{}", f.index()),
                        f.annotations().unwrap(),
                    );
                }
            }
            Kind::Enum => {
                for e in s.enumerants().unwrap() {
                    annotation_lines(
                        &mut expected,
                        s.id(),
                        &format!("enumerant{}", e.index()),
                        e.annotations().unwrap(),
                    );
                }
            }
            Kind::Interface => {
                for m in s.methods().unwrap() {
                    annotation_lines(
                        &mut expected,
                        s.id(),
                        &format!("method{}", m.index()),
                        m.annotations().unwrap(),
                    );
                }
            }
            _ => {}
        }
    }
    let mut child = Command::new(executable)
        .arg("metadata")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut wire = Vec::new();
    let mut copy = message::Builder::new_default();
    copy.set_root(
        request
            .get_root::<code_generator_request::Reader>()
            .unwrap(),
    )
    .unwrap();
    capnp::serialize::write_message(&mut wire, &copy).unwrap();
    child.stdin.take().unwrap().write_all(&wire).unwrap();
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
    expected.sort();
    actual.sort();
    assert_eq!(actual, expected);
    eprintln!(
        "{} constant/member annotation records matched pinned C++",
        actual.len()
    );
}

fn metadata_case(base: &SchemaLoader, payload: u64, bound: u64) -> SchemaLoader {
    let declaration = schema(base, "Box.label").id();
    let destination = schema(base, "Destination").id();
    let mut nodes = Vec::new();
    for s in base.get_all_loaded().filter(|s| s.id() != declaration) {
        let mut node = message::Builder::new_default();
        node.set_root(s.get_proto()).unwrap();
        if s.id() == destination {
            let mut a = node
                .get_root::<node::Builder>()
                .unwrap()
                .get_annotations()
                .unwrap()
                .get(0);
            let node::Const(c) = schema(base, if payload == 1 { "greeting" } else { "record" })
                .get_proto()
                .which()
                .unwrap()
            else {
                panic!()
            };
            a.set_value(c.get_value().unwrap()).unwrap();
            let capnp::schema_capnp::brand::scope::Bind(bindings) = a
                .get_brand()
                .unwrap()
                .get_scopes()
                .unwrap()
                .get(0)
                .which()
                .unwrap()
            else {
                panic!()
            };
            let mut ty = bindings.unwrap().get(0).init_type();
            if bound == 1 {
                ty.set_text(());
            } else {
                ty.init_struct().set_type_id(schema(base, "Record").id());
            }
        }
        nodes.push(node);
    }
    let mut loader = SchemaLoader::default();
    loader
        .load_batch(nodes.iter().map(|m| m.get_root_as_reader().unwrap()))
        .unwrap();
    loader
}

#[test]
fn tlc_metadata_model_replays_lazy_resolution_type_checks_and_authority() {
    use capntproto_test_support::verification::exploration::{controls, traces};
    const MODEL: &str = "verification/RpcSchemaMetadata.tla";
    const CFG: &str = include_str!("../verification/RpcSchemaMetadata.cfg");
    controls(
        MODEL,
        "schema-metadata",
        CFG,
        &[
            ("missingDeclaration", "DeclarationRequired"),
            ("wrongType", "TypeAgreement"),
            ("foreignAggregate", "AggregateAuthority"),
        ],
        None,
    )
    .unwrap();
    let base = loaded();
    let mut coverage = std::collections::BTreeSet::new();
    for trace in traces(MODEL, "schema-metadata", CFG).unwrap() {
        let mut loader = SchemaLoader::default();
        let mut writes = 0;
        for state in trace {
            match state["event"] {
                1 => loader = metadata_case(&base, state["payload"], state["bound"]),
                2 => {
                    let mut declaration = message::Builder::new_default();
                    if state["known"] == 1 {
                        declaration
                            .set_root(schema(&base, "Box.label").get_proto())
                            .unwrap();
                    } else {
                        let mut n = declaration.init_root::<node::Builder>();
                        n.set_id(schema(&base, "Box.label").id());
                        n.init_enum();
                    }
                    loader
                        .load(declaration.get_root_as_reader().unwrap())
                        .unwrap();
                }
                3 | 4 => {
                    let value = schema(&loader, "Destination")
                        .annotations()
                        .unwrap()
                        .get(0)
                        .and_then(|a| a.get_value());
                    let success = if state["event"] == 3 {
                        value.is_ok()
                    } else {
                        let other = loader.clone();
                        let target = if state["foreign"] == 1 {
                            &other
                        } else {
                            &loader
                        };
                        let field = if state["bound"] == 1 {
                            "text"
                        } else {
                            "record"
                        };
                        let mut message = message::Builder::new_default();
                        let mut builder = dynamic::Builder::init(
                            message.init_root(),
                            schema(target, "Destination"),
                        )
                        .unwrap();
                        let before = describe(builder.as_reader().get_named(field).unwrap());
                        let success = value.and_then(|v| builder.set_named(field, v)).is_ok();
                        if success {
                            writes += 1;
                            assert_eq!(
                                describe(builder.as_reader().get_named(field).unwrap()),
                                describe(
                                    schema(
                                        target,
                                        if state["bound"] == 1 {
                                            "greeting"
                                        } else {
                                            "record"
                                        }
                                    )
                                    .constant_value()
                                    .unwrap()
                                )
                            );
                        } else {
                            assert_eq!(
                                describe(builder.as_reader().get_named(field).unwrap()),
                                before
                            );
                        }
                        success
                    };
                    assert_eq!(u64::from(success), state["accepted"], "{state:?}");
                    coverage.insert((
                        state["event"],
                        state["known"],
                        state["payload"],
                        state["bound"],
                        success,
                    ));
                }
                _ => panic!("unknown model event"),
            }
            assert_eq!(writes, state["writes"]);
        }
    }
    for event in [3, 4] {
        assert!(coverage.contains(&(event, 0, 1, 1, false)));
        assert!(coverage.contains(&(event, 2, 1, 1, false)));
        assert!(coverage.contains(&(event, 1, 1, 2, false)));
        assert!(coverage.contains(&(event, 1, 2, 1, false)));
        assert!(coverage.contains(&(event, 1, 1, 1, true)));
        assert!(coverage.contains(&(event, 1, 2, 2, true)));
    }
    assert!(coverage.contains(&(4, 1, 2, 2, false)));
}

#[test]
fn missing_annotation_declarations_are_lazy_and_do_not_hide_known_metadata() {
    let base = loaded();
    let mut loader = metadata_case(&base, 1, 1);
    let note = schema(&base, "note").id();
    let label = schema(&base, "Box.label");
    let size = loader.len();
    {
        let list = schema(&loader, "Record").annotations().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(
            describe(list.find(note).unwrap().unwrap().get_value().unwrap()),
            "t:record"
        );
        assert!(list.find(label.id()).is_err());
        assert!(list.find(0).unwrap().is_none());
        assert_eq!(
            list.iter().map(|a| a.is_ok()).collect::<Vec<_>>(),
            [true, false]
        );
    }
    assert_eq!(loader.len(), size);
    loader.load(label.get_proto()).unwrap();
    assert_eq!(
        describe(
            schema(&loader, "Record")
                .annotations()
                .unwrap()
                .find(label.id())
                .unwrap()
                .unwrap()
                .get_value()
                .unwrap()
        ),
        "t:generic"
    );
}

#[test]
fn method_annotation_brands_resolve_implicit_arguments() {
    let base = loaded();
    let service = schema(&base, "Service");
    let mut definition = message::Builder::new_default();
    definition.set_root(service.get_proto()).unwrap();
    let node::Interface(interface) = definition
        .get_root::<node::Builder>()
        .unwrap()
        .which()
        .unwrap()
    else {
        panic!()
    };
    let mut method = interface.get_methods().unwrap().get(0);
    method
        .reborrow()
        .init_implicit_parameters(1)
        .get(0)
        .set_name("T");
    let mut a = method.init_annotations(1).get(0);
    a.set_id(schema(&base, "Box.label").id());
    a.reborrow().init_value().set_text("bound");
    let mut scope = a.init_brand().init_scopes(1).get(0);
    scope.set_scope_id(schema(&base, "Box").id());
    scope
        .init_bind(1)
        .get(0)
        .init_type()
        .init_any_pointer()
        .init_implicit_method_parameter()
        .set_parameter_index(0);
    let mut loader = SchemaLoader::default();
    loader
        .load_batch(
            base.get_all_loaded()
                .filter(|s| s.id() != service.id())
                .map(|s| s.get_proto()),
        )
        .unwrap();
    loader
        .load(definition.get_root_as_reader().unwrap())
        .unwrap();
    let method = schema(&loader, "Service").method("call").unwrap();
    let bound = method.bind_implicit(&[Type::Text]).unwrap();
    let a = bound.annotations().unwrap().get(0).unwrap();
    assert!(matches!(a.get_type().unwrap(), Type::Text));
    assert_eq!(describe(a.get_value().unwrap()), "t:bound");
    assert!(method
        .bind_implicit(&[Type::Data])
        .unwrap()
        .annotations()
        .unwrap()
        .get(0)
        .unwrap()
        .get_value()
        .is_err());
    assert!(method
        .annotations()
        .unwrap()
        .get(0)
        .unwrap()
        .get_value()
        .is_err());
}

#[test]
fn null_and_future_constants_never_create_live_capabilities_or_assume_layout() {
    let mut loader = loaded();
    let mut constant = message::Builder::new_default();
    let mut n = constant.init_root::<node::Builder>();
    n.set_id(123);
    let mut c = n.init_const();
    c.reborrow()
        .init_type()
        .init_interface()
        .set_type_id(schema(&loader, "Service").id());
    c.init_value().set_interface(());
    loader.load(constant.get_root_as_reader().unwrap()).unwrap();
    {
        let Value::Capability(cap) = loader.get(123).unwrap().constant_value().unwrap() else {
            panic!()
        };
        assert!(cap.schema().unwrap().equals(&schema(&loader, "Service")));
        assert!(cap.as_client().is_err());
        assert!(cap.new_request("call").is_err());
        let mut message = message::Builder::new_default();
        let mut target =
            dynamic::Builder::init(message.init_root(), schema(&loader, "Destination")).unwrap();
        target.set_named("service", Value::Capability(cap)).unwrap();
        assert_eq!(
            describe(target.as_reader().get_named("service").unwrap()),
            "null-cap"
        );
    }
    let mut pointer = message::Builder::new_default();
    let mut n = pointer.init_root::<node::Builder>();
    n.set_id(124);
    let mut c = n.init_const();
    c.reborrow()
        .init_type()
        .init_any_pointer()
        .init_unconstrained()
        .set_any_kind(());
    c.init_value().init_any_pointer();
    assert_eq!(
        describe(
            loader
                .load(pointer.get_root_as_reader().unwrap())
                .unwrap()
                .constant_value()
                .unwrap()
        ),
        "null"
    );

    let mut ty = message::Builder::new_default();
    ty.init_root::<capnp::schema_capnp::type_::Builder>()
        .set_void(());
    let mut bytes = capnp::serialize::write_message_to_words(&ty);
    bytes[16..18].copy_from_slice(&999u16.to_le_bytes());
    let ty = capnp::serialize::read_message(bytes.as_slice(), Default::default()).unwrap();
    let mut future = message::Builder::new_default();
    let mut n = future.init_root::<node::Builder>();
    n.set_id(125);
    let mut c = n.init_const();
    c.reborrow().set_type(ty.get_root().unwrap()).unwrap();
    c.init_value().set_uint32(42);
    let s = loader.load(future.get_root_as_reader().unwrap()).unwrap();
    assert!(matches!(s.constant_type().unwrap(), Type::Unknown(999)));
    assert!(matches!(s.constant_value().unwrap(), Value::Unknown(999)));
}

#[test]
fn malformed_metadata_pointer_payloads_and_unbound_parameters_fail_on_access() {
    let base = loaded();
    let mut broken = message::Builder::new_default();
    let mut n = broken.init_root::<node::Builder>();
    n.set_id(126);
    let mut c = n.init_const();
    c.reborrow()
        .init_type()
        .init_struct()
        .set_type_id(schema(&base, "Record").id());
    c.init_value()
        .init_struct()
        .set_as::<capnp::text::Owned>("not a struct")
        .unwrap();
    let mut loader = base.clone();
    let s = loader.load(broken.get_root_as_reader().unwrap()).unwrap();
    assert!(s.constant_value().is_err());

    // An unbound generic scope can be inspected symbolically, but cannot be
    // used to decode a concrete value as though its parameter were known.
    let mut declaration = message::Builder::new_default();
    declaration
        .set_root(schema(&base, "Box").get_proto())
        .unwrap();
    let mut annotation = declaration
        .get_root::<node::Builder>()
        .unwrap()
        .init_annotations(1)
        .get(0);
    annotation.set_id(schema(&base, "Box.label").id());
    annotation.reborrow().init_value().init_any_pointer();
    let mut scope = annotation.init_brand().init_scopes(1).get(0);
    scope.set_scope_id(schema(&base, "Box").id());
    scope.set_inherit(());
    let mut loader = SchemaLoader::default();
    // Load the changed node first so compatible version selection cannot retain
    // the original node with its intentionally empty annotations.
    loader
        .load(declaration.get_root_as_reader().unwrap())
        .unwrap();
    loader
        .load_batch(base.get_all_loaded().map(|s| s.get_proto()))
        .unwrap();
    let a = loader
        .get_unbound(schema(&base, "Box").id())
        .unwrap()
        .annotations()
        .unwrap()
        .get(0)
        .unwrap();
    assert!(matches!(a.get_type().unwrap(), Type::Parameter(_, 0)));
    assert!(a.get_value().is_err());
}
