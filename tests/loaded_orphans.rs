use capnp::{
    any_pointer,
    capability::FromClientHook,
    message,
    schema_loader::{
        dynamic::{
            self,
            orphan::{self, Editor},
            Value,
        },
        Schema, SchemaLoader,
    },
    traits::ImbueMut,
    ErrorKind,
};
use capntproto_test_support::{
    dynamic_test_capnp::{orphan_case, orphan_payload},
    runtime_test_capnp::harness,
};
use std::{cell::Cell, rc::Rc};

fn loader() -> SchemaLoader {
    let output = std::process::Command::new("capnp")
        .args([
            "compile",
            "-o-",
            "-Ivendor/capnproto/c++/src",
            "--src-prefix=schemas",
            "schemas/dynamic-test.capnp",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let request =
        capnp::serialize::read_message(output.stdout.as_slice(), Default::default()).unwrap();
    let mut loader = SchemaLoader::default();
    loader.load_request(request.get_root().unwrap()).unwrap();
    loader
}
fn schema<'s>(loader: &'s SchemaLoader, name: &str) -> Schema<'s> {
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
fn allocator(small: bool) -> message::HeapAllocator {
    let a = message::HeapAllocator::new();
    if small {
        a.first_segment_words(1)
            .allocation_strategy(message::AllocationStrategy::FixedSize)
    } else {
        a
    }
}
fn root<'a, 's: 'a>(
    m: &'a mut message::Builder<message::HeapAllocator>,
    schema: Schema<'s>,
    caps: &'a mut capnp::private::layout::CapTable,
) -> capnp::Result<dynamic::Builder<'a, 's>> {
    let mut p = m.get_root::<any_pointer::Builder>()?;
    p.imbue_mut(caps);
    dynamic::Builder::new(p, schema)
}
fn structure<'a, 's: 'a>(v: Value<'a, 's>) -> dynamic::Reader<'a, 's> {
    let Value::Struct(s) = v else {
        panic!("not a struct")
    };
    s
}
fn list<'a, 's: 'a>(v: Value<'a, 's>) -> dynamic::ListReader<'a, 's> {
    let Value::List(s) = v else {
        panic!("not a list")
    };
    s
}
fn text(v: Value<'_, '_>) -> String {
    let Value::Text(s) = v else {
        panic!("not text")
    };
    s.to_str().unwrap().to_owned()
}
fn uint(v: Value<'_, '_>) -> u64 {
    match v {
        Value::UInt32(v) => u64::from(v),
        Value::UInt64(v) => v,
        _ => panic!("not an integer"),
    }
}
struct Server(Rc<Cell<usize>>);
impl harness::Server for Server {
    async fn echo(
        self: Rc<Self>,
        _: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        r.get().set_value(123);
        Ok(())
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}
fn client(drops: &Rc<Cell<usize>>) -> harness::Client {
    capnp_rpc::new_client(Server(drops.clone()))
}

#[test]
fn loaded_orphans_move_defaults_and_preserve_failed_adoption() -> capnp::Result<()> {
    let loader = loader();
    let other_loader = crate::loader();
    for small in [false, true] {
        let mut m = message::Builder::new(allocator(small));
        m.init_root::<orphan_case::Builder>()
            .init_source()
            .set_text("stable payload");
        let mut caps = Vec::new();
        let (mut r, token) =
            root(&mut m, schema(&loader, "OrphanCase"), &mut caps)?.with_orphanage();
        let Value::Text(before) =
            structure(r.as_reader().get_named("source")?).get_named("text")?
        else {
            panic!()
        };
        let address = before.as_bytes().as_ptr();
        assert!(r.disown_named("arm", &token).is_err());
        let o = r.disown_named("source", &token)?;
        // Matching nominal IDs do not grant a native cast before registration.
        let o = o
            .release_as::<orphan_payload::Owned>()
            .err()
            .unwrap()
            .orphan;
        let error = r.adopt_named("sentinel", o).unwrap_err();
        assert_eq!(error.error.kind, ErrorKind::TypeMismatch);
        assert_eq!(
            r.as_reader().which()?.unwrap().get_proto().get_name()?,
            "sentinel"
        );
        let o = r.adopt_named("missing", error.orphan).unwrap_err().orphan;
        // Same arena and ID, but a distinct schema authority is not interchangeable.
        let other_schema = schema(&other_loader, "OrphanCase");
        let wrong_field = other_schema.field("target")?;
        let o = r.adopt(wrong_field, o).unwrap_err().orphan;
        r.adopt_named("arm", o).unwrap();
        let Value::Text(after) = structure(r.as_reader().get_named("arm")?).get_named("text")?
        else {
            panic!()
        };
        assert_eq!(after.as_bytes().as_ptr(), address);
        let o = r.disown_named("arm", &token)?;
        r.adopt_named("any", o).unwrap();
        let o = r.disown_named("description", &token)?;
        assert!(!r.as_reader().has(r.schema().field("description")?)?);
        r.adopt_named("any", o).unwrap();
        let Value::AnyPointer(p) = r.as_reader().get_named("any")? else {
            panic!()
        };
        assert_eq!(p.get_as::<capnp::text::Reader>()?, "default");
        for (name, value) in [
            ("flag", Value::Bool(false)),
            ("float", Value::Float64(3.25)),
            ("sentinel", Value::UInt32(91)),
        ] {
            r.set_named(name, value)?;
            let o = r.disown_named(name, &token)?;
            r.adopt_named(name, o).unwrap();
        }
        assert!(matches!(
            r.as_reader().get_named("flag")?,
            Value::Bool(false)
        ));
        assert!(matches!(r.as_reader().get_named("float")?,Value::Float64(v) if v==3.25));
        assert_eq!(uint(r.as_reader().get_named("sentinel")?), 91);
        let mut l = r.reborrow().init_list("numbers", 2)?;
        l.set(0, Value::UInt32(99))?;
        let o = l.disown(0, &token)?;
        assert_eq!(uint(l.as_reader().get(0)?), 0);
        let o = l.adopt(2, o).unwrap_err().orphan;
        l.adopt(1, o).unwrap();
        assert_eq!(uint(l.as_reader().get(1)?), 99);
    }
    Ok(())
}

#[test]
fn loaded_allocation_copy_group_access_resize_and_native_brands() -> capnp::Result<()> {
    use capntproto_test_support::dynamic_test_capnp::{group_defaults, orphan_brands, parcel};
    let mut loader = loader();
    loader.load_compiled_type_and_dependencies::<orphan_case::Owned>()?;
    loader.load_compiled_type_and_dependencies::<orphan_brands::Owned>()?;
    loader.load_compiled_type_and_dependencies::<group_defaults::Owned<capnp::text::Owned>>()?;
    for small in [false, true] {
        let mut m = message::Builder::new(allocator(small));
        let mut caps = Vec::new();
        let (mut r, token) =
            root(&mut m, schema(&loader, "OrphanCase"), &mut caps)?.with_orphanage();
        let mut t = token.in_struct(&mut r)?.new_text(3)?;
        token.in_struct(&mut r)?.edit(&mut t, |v| {
            let Editor::Text(t) = v else { panic!() };
            t.as_bytes_mut().copy_from_slice(b"abc");
            Ok(())
        })?;
        let mut t = t.release_as::<capnp::text::Owned>().unwrap();
        token.in_struct(&mut r)?.resize_typed(&mut t, 5)?;
        token.in_struct(&mut r)?.edit_typed(&mut t, |t| {
            t.as_bytes_mut()[3..].copy_from_slice(b"de");
            Ok(())
        })?;
        assert_eq!(
            token
                .in_struct(&mut r)?
                .read_typed(&mut t, |t| Ok(t.to_str()?.to_owned()))?,
            "abcde"
        );
        r.adopt_named("description", t.into_dynamic()).unwrap();
        assert_eq!(text(r.as_reader().get_named("description")?), "abcde");
        let mut fresh = token
            .in_struct(&mut r)?
            .new_struct(schema(&loader, "OrphanPayload"))?;
        token.in_struct(&mut r)?.edit(&mut fresh, |v| {
            let Editor::Struct(mut b) = v else { panic!() };
            b.set_named("text", Value::Text("new".into()))
        })?;
        let mut fresh = fresh.release_as::<orphan_payload::Owned>().unwrap();
        token.in_struct(&mut r)?.edit_typed(&mut fresh, |mut b| {
            b.set_number(7);
            Ok(())
        })?;
        r.adopt_named("source", fresh.into_dynamic()).unwrap();
        assert_eq!(
            uint(structure(r.as_reader().get_named("source")?).get_named("number")?),
            7
        );

        // Copy and concat retain physical dimensions and isolate source edits.
        let mut source = message::Builder::new_default();
        let mut scaps = Vec::new();
        let mut source = root(&mut source, schema(&loader, "OrphanCase"), &mut scaps)?;
        source
            .reborrow()
            .init_list("values", 1)?
            .get_struct(0)?
            .set_named("text", Value::Text("foreign".into()))?;
        let input = list(source.as_reader().get_named("values")?);
        let mut joined = token
            .in_struct(&mut r)?
            .concat(input.element_type(), &[input.clone(), input])?;
        token.in_struct(&mut r)?.read(&mut joined, |v| {
            let l = list(v);
            assert_eq!(l.len(), 2);
            assert_eq!(text(structure(l.get(1)?).get_named("text")?), "foreign");
            Ok(())
        })?;
        source
            .reborrow()
            .get_list("values")?
            .get_struct(0)?
            .set_named("text", Value::Text("changed".into()))?;
        token.in_struct(&mut r)?.resize(&mut joined, 1)?;
        r.adopt_named("values", joined).unwrap();
        assert_eq!(
            text(structure(list(r.as_reader().get_named("values")?).get(0)?).get_named("text")?),
            "foreign"
        );
        let mut copied = token
            .in_struct(&mut r)?
            .copy(source.as_reader().get_named("values")?)?;
        assert!(!token.in_struct(&mut r)?.is_null(&mut copied)?);
        let mut empty = token
            .in_struct(&mut r)?
            .null(capnp::schema_loader::Type::Data)?;
        assert!(token.in_struct(&mut r)?.is_null(&mut empty)?);
        token.in_struct(&mut r)?.resize(&mut empty, 8)?;
        token.in_struct(&mut r)?.edit(&mut empty, |v| {
            let Editor::Data(d) = v else { panic!() };
            assert_eq!(d, &[0; 8]);
            d[0] = 9;
            Ok(())
        })?;
        let mut data = token.in_struct(&mut r)?.new_data(0)?;
        token.in_struct(&mut r)?.resize(&mut data, 1)?;
        let words: std::sync::Arc<[capnp::Word]> = vec![capnp::word(1, 2, 3, 0, 0, 0, 0, 0)].into();
        let mut external = token
            .in_struct(&mut r)?
            .reference_external_data(orphan::ExternalData::new(words, 3)?)?;
        token.in_struct(&mut r)?.read(&mut external, |v| {
            let Value::Data(d) = v else { panic!() };
            assert_eq!(d, b"\x01\x02\x03");
            Ok(())
        })?;
        assert!(token
            .in_struct(&mut r)?
            .edit(&mut external, |_| Ok(()))
            .is_err());
        assert!(token.in_struct(&mut r)?.resize(&mut external, 2).is_err());

        let mut brands = message::Builder::new(allocator(small));
        let mut bcaps = Vec::new();
        let (mut b, bt) =
            root(&mut brands, schema(&loader, "OrphanBrands"), &mut bcaps)?.with_orphanage();
        b.reborrow()
            .init_struct("text")?
            .set_named("value", Value::Text("generic".into()))?;
        let o = b.disown_named("text", &bt)?;
        let o = b.adopt_named("data", o).unwrap_err().orphan;
        let o = o
            .release_as::<parcel::Owned<capnp::data::Owned>>()
            .err()
            .unwrap()
            .orphan;
        let mut o = o.release_as::<parcel::Owned<capnp::text::Owned>>().unwrap();
        assert_eq!(
            bt.in_struct(&mut b)?
                .read_typed(&mut o, |p| Ok(p.get_value()?.to_str()?.to_owned()))?,
            "generic"
        );
        b.adopt_named("text", o.into_dynamic()).unwrap();
        // Nested defaults and parent siblings survive fieldwise/contiguous conversion.
        let mut groups = message::Builder::new(allocator(small));
        let mut gcaps = Vec::new();
        let bound = schema(&loader, "GroupDefaults");
        let (mut g, gt) = root(&mut groups, bound, &mut gcaps)?.with_orphanage();
        g.set_named("sibling", Value::UInt64(81))?;
        let mut o = g.disown_named("body", &gt)?;
        gt.in_struct(&mut g)?.read(&mut o, |v| {
            let v = structure(v);
            assert_eq!(text(v.get_named("label")?), "default");
            assert_eq!(uint(v.get_named("count")?), 42);
            Ok(())
        })?;
        gt.in_struct(&mut g)?.edit(&mut o, |v| {
            let Editor::Struct(mut v) = v else { panic!() };
            v.set_named("note", Value::Text("changed".into()))
        })?;
        g.adopt_named("body", o).unwrap();
        assert_eq!(uint(g.as_reader().get_named("sibling")?), 81);
        assert_eq!(
            text(structure(g.as_reader().get_named("body")?).get_named("note")?),
            "changed"
        );
        let cap_group_schema = match r.schema().field("groups")?.get_type()? {
            capnp::schema_loader::Type::List(t) => match *t {
                capnp::schema_loader::Type::Struct(s) => match s.field("body")?.get_type()? {
                    capnp::schema_loader::Type::Struct(g) => g,
                    _ => panic!(),
                },
                _ => panic!(),
            },
            _ => panic!(),
        };
        let mut o = token.in_struct(&mut r)?.new_group(cap_group_schema)?;
        token.in_struct(&mut r)?.materialize_group(&mut o)?;
        token.in_struct(&mut r)?.edit(&mut o, |v| {
            let Editor::Struct(mut v) = v else { panic!() };
            v.set_named("label", Value::Text("allocated group".into()))
        })?;
        r.reborrow()
            .init_list("groups", 1)?
            .get_struct(0)?
            .adopt_named("body", o)
            .unwrap();
        assert_eq!(
            text(
                structure(
                    structure(list(r.as_reader().get_named("groups")?).get(0)?)
                        .get_named("body")?
                )
                .get_named("label")?
            ),
            "allocated group"
        );
    }
    Ok(())
}

#[test]
fn loaded_fieldwise_group_defaults_copy_and_nested_transfer() -> capnp::Result<()> {
    use capnp::schema_loader::Type;
    let loader = loader();
    for small in [false, true] {
        let mut m = message::Builder::new(allocator(small));
        let mut caps = Vec::new();
        let (mut r, token) =
            root(&mut m, schema(&loader, "GroupDefaults"), &mut caps)?.with_orphanage();
        r.set_named("sibling", Value::UInt64(81))?;
        let Type::Struct(body_schema) = r.schema().field("body")?.get_type()? else {
            panic!()
        };
        let mut owner = token.in_struct(&mut r)?.new_group(body_schema.clone())?;
        token.in_struct(&mut r)?.read_group(&mut owner, |mut g| {
            assert!(g.get_schema().equals(&body_schema));
            assert_eq!(g.which()?.unwrap().get_proto().get_name()?, "nested");
            assert!(!g.has_named("label")?);
            assert!(!g.has_named("note")?);
            g.read_named("label", |v| {
                assert_eq!(text(v), "default");
                Ok(())
            })?;
            g.read_group_named("nested", |mut n| {
                n.read_named("text", |v| {
                    assert_eq!(text(v), "nested");
                    Ok(())
                })?;
                n.read_named("flag", |v| {
                    assert!(matches!(v, Value::Bool(true)));
                    Ok(())
                })
            })
        })?;
        // Default values detach as materialized owners. The source remains at its default.
        let mut label = token
            .in_struct(&mut r)?
            .edit_group(&mut owner, |mut g| g.disown_named("label"))?;
        token.in_struct(&mut r)?.read(&mut label, |v| {
            assert_eq!(text(v), "default");
            Ok(())
        })?;
        token.in_struct(&mut r)?.edit_group(&mut owner, |mut g| {
            g.adopt_named("label", label).unwrap();
            g.edit_named("label", |v| {
                let Editor::Text(t) = v else { panic!() };
                t.as_bytes_mut().copy_from_slice(b"changed");
                Ok(())
            })?;
            g.set_named("count", Value::UInt32(90))?;
            g.clear_named("count")?;
            g.read_named("count", |v| {
                assert_eq!(uint(v), 42);
                Ok(())
            })?;
            g.edit_group_named("nested", |mut n| {
                n.set_named("text", Value::Text("moved nested".into()))
            })
        })?;
        let mut nested = token
            .in_struct(&mut r)?
            .edit_group(&mut owner, |mut g| g.disown_named("nested"))?;
        token.in_struct(&mut r)?.read_group(&mut nested, |mut n| {
            n.read_named("text", |v| {
                assert_eq!(text(v), "moved nested");
                Ok(())
            })
        })?;
        token.in_struct(&mut r)?.edit_group(&mut owner, |mut g| {
            g.read_group_named("nested", |mut n| {
                n.read_named("text", |v| {
                    assert_eq!(text(v), "nested");
                    Ok(())
                })
            })?;
            g.set_named("note", Value::Text("selected".into()))?;
            assert!(g.disown_named("nested").is_err());
            g.adopt_named("nested", nested).unwrap();
            Ok(())
        })?;
        r.adopt_named("body", owner).unwrap();
        let mut foreign = message::Builder::new(allocator(small));
        let mut other_caps = Vec::new();
        let (mut target, t) = root(
            &mut foreign,
            schema(&loader, "GroupDefaults"),
            &mut other_caps,
        )?
        .with_orphanage();
        let copy = t
            .in_struct(&mut target)?
            .copy_group(structure(r.as_reader().get_named("body")?))?;
        drop(r.disown_named("body", &token)?);
        target.adopt_named("body", copy).unwrap();
        let copied = structure(target.as_reader().get_named("body")?);
        assert_eq!(text(copied.get_named("label")?), "changed");
        assert_eq!(
            text(structure(copied.get_named("nested")?).get_named("text")?),
            "moved nested"
        );
        assert_eq!(uint(r.as_reader().get_named("sibling")?), 81);

        // Copy assignment validates actual pointer kinds and gives the new
        // owner the destination type. This differs from narrowing by adoption.
        let drops = Rc::new(Cell::new(0));
        let mut source = message::Builder::new(allocator(small));
        let mut source_caps = Vec::new();
        {
            let mut s =
                source.init_root::<capntproto_test_support::dynamic_test_capnp::orphan_pointer_kinds::Builder>();
            s.imbue_mut(&mut source_caps);
            let mut body = s.init_body();
            body.reborrow()
                .get_structure()
                .init_as::<orphan_payload::Builder>()
                .set_text("structure");
            body.reborrow()
                .get_list()
                .set_as::<capnp::text::Owned>("list")?;
            body.get_cap().set_as_capability(client(&drops).client.hook);
        }
        use capnp::traits::Imbue;
        let mut pointer = source.get_root_as_reader::<any_pointer::Reader>()?;
        pointer.imbue(&source_caps);
        let input = structure(
            dynamic::Reader::new(pointer, schema(&loader, "OrphanPointerKinds"))?
                .get_named("body")?,
        );
        let mut destination = message::Builder::new(allocator(small));
        let mut destination_caps = Vec::new();
        let (mut d, dt) = root(
            &mut destination,
            schema(&loader, "OrphanPointerKinds"),
            &mut destination_caps,
        )?
        .with_orphanage();
        let mut group = d.disown_named("body", &dt)?;
        dt.in_struct(&mut d)?.edit_group(&mut group, |mut g| {
            g.set_named("structure", input.get_named("structure")?)?;
            g.set_named("list", input.get_named("list")?)?;
            g.set_named("cap", input.get_named("cap")?)?;
            g.set_named("typed", input.get_named("cap")?)?;
            assert!(g.set_named("structure", input.get_named("list")?).is_err());
            g.read_named("structure", |v| {
                let Value::AnyPointer(p) = v else { panic!() };
                assert_eq!(
                    p.get_as::<orphan_payload::Reader>()?.get_text()?,
                    "structure"
                );
                Ok(())
            })?;
            g.read_named("list", |v| {
                let Value::AnyPointer(p) = v else { panic!() };
                assert_eq!(p.get_as::<capnp::text::Reader>()?, "list");
                Ok(())
            })?;
            g.set_named("typed", Value::Capability(dynamic::Client::null(None)))?;
            assert!(!g.has_named("typed")?);
            assert!(g.has_named("cap")?);
            Ok(())
        })?;
        d.adopt_named("body", group).unwrap();
        assert!(structure(d.as_reader().get_named("body")?).has(input.schema().field("cap")?)?);
    }
    Ok(())
}

#[test]
fn loaded_unknown_fields_survive_rejected_inline_adoption() -> capnp::Result<()> {
    use capnp::traits::IntoInternalStructReader;
    use capntproto_test_support::dynamic_test_capnp::{
        large_orphan_case, small_orphan, small_orphan_case,
    };
    let mut loader = loader();
    loader.load_compiled_type_and_dependencies::<small_orphan_case::Owned>()?;
    for small in [false, true] {
        let drops = Rc::new(Cell::new(0));
        let mut m = message::Builder::new(allocator(small));
        let mut caps = Vec::new();
        let mut future = m.init_root::<large_orphan_case::Builder>();
        future.imbue_mut(&mut caps);
        future.reborrow().init_source().set_cap(client(&drops));
        future.reborrow().get_source()?.set_text("unknown child");
        let address = future
            .get_source()?
            .get_text()?
            .into_reader()
            .as_bytes()
            .as_ptr();
        let (mut r, token) =
            root(&mut m, schema(&loader, "SmallOrphanCase"), &mut caps)?.with_orphanage();
        r.reborrow()
            .init_list("targets", 1)?
            .get_struct(0)?
            .set_named("number", Value::UInt32(73))?;
        let owner = r.disown_named("source", &token)?;
        let error = r
            .reborrow()
            .get_list("targets")?
            .adopt(0, owner)
            .unwrap_err();
        assert_eq!(error.error.kind, ErrorKind::WouldTruncate);
        assert_eq!(
            uint(structure(list(r.as_reader().get_named("targets")?).get(0)?).get_named("number")?),
            73
        );
        assert_eq!(drops.get(), 0);
        r.adopt_named("source", error.orphan).unwrap();
        let value = structure(r.as_reader().get_named("source")?)
            .downcast_native::<small_orphan::Owned>()?;
        let value = orphan_payload::Reader::from(value.into_internal_struct_reader());
        assert_eq!(value.get_text()?.as_bytes().as_ptr(), address);
        assert!(value.has_cap());
        drop(r.disown_named("source", &token)?);
        assert_eq!(drops.get(), 1);
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn loaded_capability_subtyping_preserves_brands_and_owned_hooks() -> capnp::Result<()> {
    use capnp::schema_loader::Type;
    let loader = loader();
    let drops = Rc::new(Cell::new(0));
    let mut m = message::Builder::new_default();
    let mut caps = Vec::new();
    let (mut r, token) = root(&mut m, schema(&loader, "OrphanBrands"), &mut caps)?.with_orphanage();
    let Type::Interface(derived) = r.schema().field("derived")?.get_type()? else {
        panic!()
    };
    r.set_named(
        "derived",
        Value::Capability(dynamic::Client::new(client(&drops), derived)?),
    )?;
    let owner = r.disown_named("derived", &token)?;
    let error = r.adopt_named("wrong", owner).unwrap_err();
    assert_eq!(error.error.kind, ErrorKind::TypeMismatch);
    r.adopt_named("base", error.orphan).unwrap();
    let mut owner = r.disown_named("base", &token)?;
    let held = token.in_struct(&mut r)?.read(&mut owner, |v| {
        let Value::Capability(c) = v else { panic!() };
        Ok(c.as_client()?.clone().cast_to::<harness::Client>())
    })?;
    drop(owner);
    drop(r);
    drop(m);
    drop(caps);
    assert_eq!(drops.get(), 0);
    assert_eq!(
        held.echo_request().send().promise.await?.get()?.get_value(),
        123
    );
    drop(held);
    assert_eq!(drops.get(), 1);
    Ok(())
}

fn slot_cap<'s>(
    r: dynamic::Reader<'_, 's>,
    mode: u8,
    index: u32,
) -> capnp::Result<dynamic::Client<'s>> {
    let v = match mode {
        0 => structure(r.get_named(if index == 0 { "source" } else { "target" })?)
            .get_named("cap")?,
        1 => list(r.get_named("tokens")?).get(index)?,
        2 => structure(list(r.get_named("values")?).get(index)?).get_named("cap")?,
        3 => structure(structure(list(r.get_named("groups")?).get(index)?).get_named("body")?)
            .get_named("cap")?,
        _ => unreachable!(),
    };
    let Value::Capability(c) = v else {
        panic!("not a capability")
    };
    Ok(c)
}
fn take_slot<'m, 's>(
    r: &mut dynamic::Builder<'_, 's>,
    mode: u8,
    index: u32,
    token: &orphan::Orphanage<'m>,
) -> capnp::Result<orphan::Orphan<'m, 's>> {
    match mode {
        0 => r.disown_named(if index == 0 { "source" } else { "target" }, token),
        1 => r.reborrow().get_list("tokens")?.disown(index, token),
        2 => r.reborrow().get_list("values")?.disown(index, token),
        3 => r
            .reborrow()
            .get_list("groups")?
            .get_struct(index)?
            .disown_named("body", token),
        _ => unreachable!(),
    }
}
fn adopt_slot<'m, 's>(
    r: &mut dynamic::Builder<'_, 's>,
    mode: u8,
    o: orphan::Orphan<'m, 's>,
) -> Result<(), orphan::AdoptError<orphan::Orphan<'m, 's>>> {
    match mode {
        0 => r.adopt_named("target", o),
        1 => r.reborrow().get_list("tokens").unwrap().adopt(1, o),
        2 => r.reborrow().get_list("values").unwrap().adopt(1, o),
        3 => r
            .reborrow()
            .get_list("groups")
            .unwrap()
            .get_struct(1)
            .unwrap()
            .adopt_named("body", o),
        _ => unreachable!(),
    }
}
#[derive(serde::Deserialize)]
struct Trace {
    steps: Vec<Step>,
}
#[derive(serde::Deserialize)]
struct Step {
    action: String,
    state: Vec<u8>,
}

async fn replay(trace: &Trace, mode: u8, small: bool, loader: &SchemaLoader) -> capnp::Result<()> {
    let drops: [_; 3] = std::array::from_fn(|_| Rc::new(Cell::new(0)));
    let clients = drops.each_ref().map(client);
    let identities = clients.each_ref().map(|c| c.client.hook.get_ptr());
    let [first, second, third] = clients;
    let mut caps = Vec::new();
    let mut other_caps = Vec::new();
    let mut wrong_caps = Vec::new();
    let mut m = message::Builder::new(allocator(small));
    let mut foreign = message::Builder::new(allocator(small));
    {
        let mut r = m.init_root::<orphan_case::Builder>();
        r.imbue_mut(&mut caps);
        r.reborrow().init_arm();
        match mode {
            0 => {
                r.reborrow().init_source().set_cap(first);
                r.reborrow().init_target().set_cap(second);
            }
            1 => {
                let mut l = r.reborrow().init_tokens(2);
                l.set(0, first.client.hook);
                l.set(1, second.client.hook);
            }
            2 => {
                let mut l = r.reborrow().init_values(2);
                l.reborrow().get(0).set_cap(first);
                l.get(1).set_cap(second);
            }
            3 => {
                let mut l = r.reborrow().init_groups(2);
                l.reborrow().get(0).set_sibling(11);
                l.reborrow().get(1).set_sibling(22);
                l.reborrow().get(0).init_body().set_cap(first);
                l.get(1).init_body().set_cap(second);
            }
            _ => unreachable!(),
        }
        let mut r = foreign.init_root::<orphan_case::Builder>();
        r.imbue_mut(&mut other_caps);
        r.init_target().set_cap(third);
    }
    let s = schema(loader, "OrphanCase");
    let (mut r, token) = root(&mut m, s.clone(), &mut caps)?.with_orphanage();
    let mut other = root(&mut foreign, s, &mut other_caps)?;
    let mut owner = None;
    let mut held: Option<harness::Client> = None;
    for step in &trace.steps {
        match step.action.as_str() {
            "take" => owner = Some(take_slot(&mut r, mode, 0, &token)?),
            "adopt" => adopt_slot(&mut r, mode, owner.take().unwrap()).unwrap(),
            "drop-orphan" => drop(owner.take()),
            "wrong-arena" => {
                let e = other
                    .adopt_named("target", owner.take().unwrap())
                    .unwrap_err();
                assert_eq!(e.error.kind, ErrorKind::WrongArena);
                owner = Some(e.orphan);
            }
            "wrong-type" => {
                let e = r
                    .adopt_named("sentinel", owner.take().unwrap())
                    .unwrap_err();
                assert_eq!(e.error.kind, ErrorKind::TypeMismatch);
                owner = Some(e.orphan);
            }
            "wrong-context" => {
                let mut wrong = r.reborrow();
                wrong.imbue_mut(&mut wrong_caps);
                let e = adopt_slot(&mut wrong, mode, owner.take().unwrap()).unwrap_err();
                assert_eq!(e.error.kind, ErrorKind::WrongArena);
                owner = Some(e.orphan);
            }
            "read" => {
                let c = slot_cap(r.as_reader(), mode, 0)?;
                let c = if c.as_client().is_ok() {
                    c
                } else {
                    slot_cap(r.as_reader(), mode, 1)?
                };
                held = Some(harness::Client {
                    client: capnp::capability::Client::new(c.as_client()?.hook.add_ref()),
                });
            }
            "drop-held" => drop(held.take()),
            "clear-source" => drop(take_slot(&mut r, mode, 0, &token)?),
            "clear-dest" => drop(take_slot(&mut r, mode, 1, &token)?),
            _ => panic!("unknown action"),
        }
        let id = |c: dynamic::Client<'_>| {
            c.as_client().ok().map_or(0, |c| {
                (identities
                    .iter()
                    .position(|&id| id == c.hook.get_ptr())
                    .unwrap()
                    + 1) as u8
            })
        };
        assert_eq!(
            [
                id(slot_cap(r.as_reader(), mode, 0)?),
                id(slot_cap(r.as_reader(), mode, 1)?),
                id(slot_cap(other.as_reader(), 0, 1)?),
                u8::from(owner.is_some()),
                u8::from(held.is_some())
            ],
            step.state[..5],
            "{} mode={mode} small={small}",
            step.action
        );
        assert_eq!(
            u8::from(r.as_reader().which()?.unwrap().get_proto().get_name()? == "arm"),
            step.state[12]
        );
        for (i, drop_count) in drops.iter().enumerate() {
            assert_eq!(
                u8::from(drop_count.get() == 0),
                step.state[13 + i],
                "{} cap={i}",
                step.action
            );
        }
        if let Some(c) = &held {
            assert_eq!(c.client.hook.get_ptr(), identities[0]);
            assert_eq!(
                c.echo_request().send().promise.await?.get()?.get_value(),
                123
            );
        }
        if mode == 3 {
            for i in 0..2 {
                assert_eq!(
                    uint(
                        structure(list(r.as_reader().get_named("groups")?).get(i)?)
                            .get_named("sibling")?
                    ),
                    11 * (u64::from(i) + 1)
                );
            }
        }
    }
    Ok(())
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_loaded_orphan_traces() -> capnp::Result<()> {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_DYNAMIC_ORPHAN_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!traces.is_empty());
    let loader = loader();
    for trace in &traces {
        for mode in 0..4 {
            for small in [false, true] {
                replay(trace, mode, small, &loader).await?;
            }
        }
    }
    Ok(())
}

type NativeList = capnp::struct_list::Owned<orphan_payload::Owned>;
enum Held<'m, 's> {
    Dynamic(orphan::Orphan<'m, 's>),
    Typed(orphan::TypedOrphan<'m, 's, NativeList>),
}
struct Observation {
    len: u32,
    first: harness::Client,
    second: Option<harness::Client>,
    address: *const u8,
}
fn cap(v: Value<'_, '_>) -> capnp::Result<harness::Client> {
    let Value::Capability(c) = v else { panic!() };
    Ok(harness::Client {
        client: capnp::capability::Client::new(c.as_client()?.hook.add_ref()),
    })
}
fn observe(l: dynamic::ListReader<'_, '_>) -> capnp::Result<Observation> {
    let first = structure(l.get(0)?);
    let Value::Text(t) = first.get_named("text")? else {
        panic!()
    };
    Ok(Observation {
        len: l.len(),
        first: cap(first.get_named("cap")?)?,
        second: if l.len() > 1 {
            cap(structure(l.get(1)?).get_named("cap")?).ok()
        } else {
            None
        },
        address: t.as_bytes().as_ptr(),
    })
}
impl<'m, 's> Held<'m, 's> {
    fn into_dynamic(self) -> orphan::Orphan<'m, 's> {
        match self {
            Self::Dynamic(o) => o,
            Self::Typed(o) => o.into_dynamic(),
        }
    }
    fn observe(&mut self, a: &mut orphan::Access<'_, '_>) -> capnp::Result<Observation> {
        match self {
            Self::Dynamic(o) => a.read(o, |v| observe(list(v))),
            Self::Typed(o) => a.read_typed(o, |l| {
                Ok(Observation {
                    len: l.len(),
                    first: l.get(0).get_cap()?,
                    second: if l.len() > 1 && l.get(1).has_cap() {
                        Some(l.get(1).get_cap()?)
                    } else {
                        None
                    },
                    address: l.get(0).get_text()?.as_bytes().as_ptr(),
                })
            }),
        }
    }
    fn resize(&mut self, a: &mut orphan::Access<'_, '_>, n: u32) -> capnp::Result<()> {
        match self {
            Self::Dynamic(o) => a.resize(o, n),
            Self::Typed(o) => a.resize_typed(o, n),
        }
    }
    fn edit(
        &mut self,
        a: &mut orphan::Access<'_, '_>,
        c: harness::Client,
        s: Schema<'_>,
        panic: bool,
    ) -> capnp::Result<()> {
        fn finish(panic: bool) -> capnp::Result<()> {
            if panic {
                panic!("editor panic")
            };
            Err(capnp::Error::failed("editor failed".into()))
        }
        match self {
            Self::Dynamic(o) => a.edit(o, |v| {
                let Editor::List(l) = v else { panic!() };
                l.get_struct(0)?
                    .set_named("cap", Value::Capability(dynamic::Client::new(c, s)?))?;
                finish(panic)
            }),
            Self::Typed(o) => a.edit_typed(o, |l| {
                l.get(0).set_cap(c);
                finish(panic)
            }),
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_loaded_orphan_access_traces() -> capnp::Result<()> {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_ORPHAN_ACCESS_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!cases.is_empty());
    let mut loader = loader();
    loader.load_compiled_type_and_dependencies::<orphan_case::Owned>()?;
    let payload = schema(&loader, "OrphanPayload");
    let capnp::schema_loader::Type::Interface(cap_schema) = payload.field("cap")?.get_type()?
    else {
        panic!()
    };
    for small in [false, true] {
        for panic in [false, true] {
            for case in &cases {
                let drops: [_; 3] = std::array::from_fn(|_| Rc::new(Cell::new(0)));
                let ambient = Rc::new(Cell::new(0));
                let mut m = message::Builder::new(allocator(small));
                let mut caps = Vec::new();
                let (mut r, token) =
                    root(&mut m, schema(&loader, "OrphanCase"), &mut caps)?.with_orphanage();
                let ambient_client = client(&ambient);
                let ambient_id = ambient_client.client.hook.get_ptr();
                r.set_named(
                    "token",
                    Value::Capability(dynamic::Client::new(ambient_client, cap_schema.clone())?),
                )?;
                let mut other_m = message::Builder::new_default();
                let mut other_caps = Vec::new();
                let (mut other, other_token) =
                    root(&mut other_m, schema(&loader, "OrphanCase"), &mut other_caps)?
                        .with_orphanage();
                let mut owner: Option<Held<'_, '_>> = None;
                let mut held: Option<harness::Client> = None;
                let mut ids = [0; 3];
                let mut address = std::ptr::null();
                for step in &case.steps {
                    let s = &step.state;
                    match step.action.as_str() {
                        "allocate" => {
                            let mut o = token
                                .in_struct(&mut r)?
                                .new_list(capnp::schema_loader::Type::Struct(payload.clone()), 2)?;
                            token.in_struct(&mut r)?.edit(&mut o, |v| {
                                let Editor::List(mut l) = v else { panic!() };
                                for i in 0..2 {
                                    let c = client(&drops[i]);
                                    ids[i] = c.client.hook.get_ptr();
                                    let mut b = l.reborrow().get_struct(i as u32)?;
                                    b.set_named(
                                        "cap",
                                        Value::Capability(dynamic::Client::new(
                                            c,
                                            cap_schema.clone(),
                                        )?),
                                    )?;
                                    b.set_named("text", Value::Text("child".into()))?;
                                }
                                address = observe(l.into_reader())?.address;
                                Ok(())
                            })?;
                            owner = Some(Held::Dynamic(o));
                        }
                        "edit" => {
                            let c = client(&drops[2]);
                            ids[2] = c.client.hook.get_ptr();
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    owner.as_mut().unwrap().edit(
                                        &mut token.in_struct(&mut r)?,
                                        c,
                                        cap_schema.clone(),
                                        panic,
                                    )
                                }));
                            assert_eq!(result.is_err(), panic);
                            if !panic {
                                assert!(result.unwrap().is_err());
                            }
                        }
                        "read" => {
                            held = Some(if let Some(o) = &mut owner {
                                o.observe(&mut token.in_struct(&mut r)?)?.first
                            } else {
                                observe(list(r.as_reader().get_named("values")?))?.first
                            })
                        }
                        "shrink" => owner
                            .as_mut()
                            .unwrap()
                            .resize(&mut token.in_struct(&mut r)?, 1)?,
                        "grow" => owner
                            .as_mut()
                            .unwrap()
                            .resize(&mut token.in_struct(&mut r)?, 3)?,
                        "typed" => {
                            owner = Some(Held::Typed(
                                owner
                                    .take()
                                    .unwrap()
                                    .into_dynamic()
                                    .release_as::<NativeList>()
                                    .unwrap(),
                            ))
                        }
                        "bad-type" => {
                            let e = owner
                                .take()
                                .unwrap()
                                .into_dynamic()
                                .release_as::<capnp::text_list::Owned>()
                                .err()
                                .unwrap();
                            assert_eq!(e.error.kind, ErrorKind::TypeMismatch);
                            owner = Some(if s[9] != 0 {
                                Held::Typed(e.orphan.release_as::<NativeList>().unwrap())
                            } else {
                                Held::Dynamic(e.orphan)
                            });
                        }
                        "bad-arena" => {
                            let e = owner
                                .as_mut()
                                .unwrap()
                                .observe(&mut other_token.in_struct(&mut other)?)
                                .err()
                                .unwrap();
                            assert_eq!(e.kind, ErrorKind::WrongArena);
                        }
                        "adopt" => r
                            .adopt_named("values", owner.take().unwrap().into_dynamic())
                            .unwrap(),
                        "drop" => {
                            if let Some(o) = owner.take() {
                                drop(o)
                            } else {
                                r.clear_named("values")?
                            }
                        }
                        "release" => drop(held.take()),
                        _ => panic!("unknown action"),
                    }
                    assert_eq!(owner.is_some(), s[0] == 1);
                    assert_eq!(r.as_reader().has(r.schema().field("values")?)?, s[0] == 2);
                    let observation = if let Some(o) = &mut owner {
                        Some(o.observe(&mut token.in_struct(&mut r)?)?)
                    } else if s[0] == 2 {
                        Some(observe(list(r.as_reader().get_named("values")?))?)
                    } else {
                        None
                    };
                    if let Some(o) = observation {
                        assert_eq!(o.len, u32::from(s[1]));
                        assert_eq!(o.first.client.hook.get_ptr(), ids[usize::from(s[2] - 1)]);
                        assert_eq!(o.address, address);
                        assert_eq!(o.second.is_some(), s[3] == 2);
                        if let Some(c) = o.second {
                            assert_eq!(c.client.hook.get_ptr(), ids[1]);
                        }
                    }
                    assert_eq!(held.is_some(), s[4] != 0);
                    if let Some(c) = &held {
                        assert_eq!(c.client.hook.get_ptr(), ids[usize::from(s[4] - 1)]);
                        assert_eq!(
                            c.echo_request().send().promise.await?.get()?.get_value(),
                            123
                        );
                    }
                    for i in 0..3 {
                        let created = if i == 2 { s[5] } else { u8::from(s[0] != 0) };
                        assert_eq!(
                            drops[i].get(),
                            usize::from(created - s[14 + i]),
                            "{} cap {i}",
                            step.action
                        );
                    }
                    assert_eq!(ambient.get(), 0);
                    assert_eq!(
                        cap(r.as_reader().get_named("token")?)?
                            .client
                            .hook
                            .get_ptr(),
                        ambient_id
                    );
                }
            }
        }
    }
    Ok(())
}

fn text_address(v: Value<'_, '_>) -> capnp::Result<*const u8> {
    let Value::Text(t) = v else { panic!() };
    Ok(t.as_bytes().as_ptr())
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_loaded_orphan_group_traces() -> capnp::Result<()> {
    use capntproto_test_support::dynamic_test_capnp::orphan_group;
    let path = capntproto_test_support::verification::input("CAPNTPROTO_ORPHAN_GROUPS_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!cases.is_empty());
    let mut loader = loader();
    loader.load_compiled_type_and_dependencies::<orphan_group::Owned>()?;
    let group_schema = schema(&loader, "OrphanGroup");
    let capnp::schema_loader::Type::Struct(body_schema) = group_schema.field("body")?.get_type()?
    else {
        panic!()
    };
    let capnp::schema_loader::Type::Interface(cap_schema) = body_schema.field("cap")?.get_type()?
    else {
        panic!()
    };
    for case in &cases {
        for small in [false, true] {
            for panic in [false, true] {
                let drops: [_; 3] = std::array::from_fn(|_| Rc::new(Cell::new(0)));
                let a = client(&drops[0]);
                let b = client(&drops[1]);
                let mut ids = [a.client.hook.get_ptr(), b.client.hook.get_ptr(), 0];
                let mut m = message::Builder::new(allocator(small));
                let mut caps = Vec::new();
                {
                    let mut r = m.init_root::<orphan_group::Builder>();
                    r.imbue_mut(&mut caps);
                    r.set_sibling(91);
                    let mut g = r.init_body();
                    g.set_cap(a);
                    g.set_label("stable allocation");
                    g.init_nested().set_cap(b);
                }
                let (mut r, token) =
                    root(&mut m, group_schema.clone(), &mut caps)?.with_orphanage();
                let address =
                    text_address(structure(r.as_reader().get_named("body")?).get_named("label")?)?;
                let mut other_m = message::Builder::new_default();
                let mut other_caps = Vec::new();
                let (mut other, other_token) =
                    root(&mut other_m, group_schema.clone(), &mut other_caps)?.with_orphanage();
                let mut group = None;
                let mut moved = None;
                let mut held: Option<harness::Client> = None;
                for step in &case.steps {
                    let s = &step.state;
                    match step.action.as_str() {
                        "detach" => group = Some(r.disown_named("body", &token)?),
                        "materialize" => token
                            .in_struct(&mut r)?
                            .materialize_group(group.as_mut().unwrap())?,
                        "edit" => {
                            let c = client(&drops[2]);
                            ids[2] = c.client.hook.get_ptr();
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    token.in_struct(&mut r)?.edit_group(
                                        group.as_mut().unwrap(),
                                        |mut g| {
                                            g.set_named(
                                                "cap",
                                                Value::Capability(dynamic::Client::new(
                                                    c,
                                                    cap_schema.clone(),
                                                )?),
                                            )?;
                                            if panic {
                                                panic!("group editor unwind")
                                            };
                                            Err::<(), _>(capnp::Error::failed(
                                                "group edit error".into(),
                                            ))
                                        },
                                    )
                                }));
                            assert_eq!(result.is_err(), panic);
                            if !panic {
                                assert!(result.unwrap().is_err());
                            }
                        }
                        "select" => token
                            .in_struct(&mut r)?
                            .edit_group(group.as_mut().unwrap(), |mut g| {
                                g.set_named("value", Value::UInt32(17))
                            })?,
                        "read" => {
                            held = Some(
                                token
                                    .in_struct(&mut r)?
                                    .read_group(group.as_mut().unwrap(), |mut g| {
                                        g.read_named("cap", cap)
                                    })?,
                            )
                        }
                        "release" => drop(held.take()),
                        "take" => {
                            moved = Some(
                                token
                                    .in_struct(&mut r)?
                                    .edit_group(group.as_mut().unwrap(), |mut g| {
                                        g.disown_named("cap")
                                    })?,
                            )
                        }
                        "put" => token.in_struct(&mut r)?.edit_group(
                            group.as_mut().unwrap(),
                            |mut g| {
                                g.adopt_named("cap", moved.take().unwrap())
                                    .map_err(|e| e.error)
                            },
                        )?,
                        "drop-moved" => drop(moved.take()),
                        "bad-type" => {
                            let bad = token.in_struct(&mut r)?.new_text(3)?;
                            let mut bad = token.in_struct(&mut r)?.edit_group(
                                group.as_mut().unwrap(),
                                |mut g| {
                                    let e = g.adopt_named("value", bad).unwrap_err();
                                    assert_eq!(e.error.kind, ErrorKind::TypeMismatch);
                                    Ok(e.orphan)
                                },
                            )?;
                            assert_eq!(
                                token.in_struct(&mut r)?.read(&mut bad, |v| {
                                    let Value::Text(t) = v else { panic!() };
                                    Ok(t.len())
                                })?,
                                3
                            );
                        }
                        "bad-arena" => {
                            let bad = other_token.in_struct(&mut other)?.copy(Value::UInt32(99))?;
                            let mut bad = token.in_struct(&mut r)?.edit_group(
                                group.as_mut().unwrap(),
                                |mut g| {
                                    let e = g.adopt_named("value", bad).unwrap_err();
                                    assert_eq!(e.error.kind, ErrorKind::WrongArena);
                                    Ok(e.orphan)
                                },
                            )?;
                            assert_eq!(
                                other_token
                                    .in_struct(&mut other)?
                                    .read(&mut bad, |v| Ok(uint(v)))?,
                                99
                            );
                        }
                        "adopt" => r.adopt_named("body", group.take().unwrap()).unwrap(),
                        "drop" => {
                            if let Some(g) = group.take() {
                                drop(g)
                            } else {
                                drop(r.disown_named("body", &token)?)
                            }
                        }
                        _ => panic!("unknown action"),
                    }
                    assert_eq!(group.is_some(), s[0] == 1);
                    assert_eq!(held.is_some(), s[6] != 0);
                    assert_eq!(moved.is_some(), s[8] != 0);
                    assert_eq!(uint(r.as_reader().get_named("sibling")?), u64::from(s[12]));
                    if s[0] == 1 && s[17] != 0 {
                        let mut g = group.take().unwrap();
                        token.in_struct(&mut r)?.materialize_group(&mut g)?;
                        let mut t = g.release_as::<orphan_group::body::Owned>().unwrap();
                        token.in_struct(&mut r)?.read_typed(&mut t, |g| {
                            assert_eq!(g.get_label()?.as_bytes().as_ptr(), address);
                            assert_eq!(g.has_cap(), s[1] != 0);
                            if s[1] != 0 {
                                assert_eq!(
                                    g.get_cap()?.client.hook.get_ptr(),
                                    ids[if s[1] == 1 { 0 } else { 2 }]
                                );
                            }
                            if s[4] == 0 {
                                let orphan_group::body::Nested(n) = g.which()? else {
                                    panic!()
                                };
                                assert_eq!(n.get_cap()?.client.hook.get_ptr(), ids[1]);
                            } else {
                                let orphan_group::body::Value(v) = g.which()? else {
                                    panic!()
                                };
                                assert_eq!(v, 17);
                            }
                            Ok(())
                        })?;
                        group = Some(t.into_dynamic());
                    } else if let Some(g) = &mut group {
                        token.in_struct(&mut r)?.read_group(g, |mut g| {
                            assert_eq!(
                                g.which()?.unwrap().get_proto().get_name()?,
                                if s[4] == 0 { "nested" } else { "value" }
                            );
                            assert_eq!(g.read_named("label", text_address)?, address);
                            assert_eq!(g.has_named("cap")?, s[1] != 0);
                            if s[1] != 0 {
                                assert_eq!(
                                    g.read_named("cap", cap)?.client.hook.get_ptr(),
                                    ids[if s[1] == 1 { 0 } else { 2 }]
                                );
                            }
                            if s[4] == 0 {
                                g.read_group_named("nested", |mut n| {
                                    assert_eq!(
                                        n.read_named("cap", cap)?.client.hook.get_ptr(),
                                        ids[1]
                                    );
                                    Ok(())
                                })?;
                            } else {
                                assert_eq!(g.read_named("value", |v| Ok(uint(v)))?, 17);
                            }
                            Ok(())
                        })?;
                    }
                    let body = structure(r.as_reader().get_named("body")?);
                    if s[0] == 2 {
                        assert_eq!(
                            body.which()?.unwrap().get_proto().get_name()?,
                            if s[4] == 0 { "nested" } else { "value" }
                        );
                        assert_eq!(text_address(body.get_named("label")?)?, address);
                        assert_eq!(body.has(body.schema().field("cap")?)?, s[1] != 0);
                        if s[1] != 0 {
                            assert_eq!(
                                cap(body.get_named("cap")?)?.client.hook.get_ptr(),
                                ids[if s[1] == 1 { 0 } else { 2 }]
                            );
                        }
                        if s[4] == 0 {
                            assert_eq!(
                                cap(structure(body.get_named("nested")?).get_named("cap")?)?
                                    .client
                                    .hook
                                    .get_ptr(),
                                ids[1]
                            );
                        } else {
                            assert_eq!(uint(body.get_named("value")?), 17);
                        }
                    } else {
                        assert!(!body.has(body.schema().field("cap")?)?);
                        assert!(!body.has(body.schema().field("label")?)?);
                        assert_eq!(body.which()?.unwrap().get_proto().get_name()?, "empty");
                    }
                    if let Some(o) = &mut moved {
                        let c = token.in_struct(&mut r)?.read(o, cap)?;
                        assert_eq!(c.client.hook.get_ptr(), ids[if s[8] == 1 { 0 } else { 2 }]);
                        assert_eq!(
                            c.echo_request().send().promise.await?.get()?.get_value(),
                            123
                        );
                    }
                    if let Some(c) = &held {
                        assert_eq!(c.client.hook.get_ptr(), ids[0]);
                        assert_eq!(
                            c.echo_request().send().promise.await?.get()?.get_value(),
                            123
                        );
                    }
                    for i in 0..3 {
                        assert_eq!(
                            drops[i].get(),
                            usize::from(s[14 + i] == 0 && (i != 2 || ids[2] != 0)),
                            "{} cap {i}",
                            step.action
                        );
                    }
                }
                drop(group);
                drop(moved);
                drop(held);
                drop(r.disown_named("body", &token)?);
                assert_eq!(drops[0].get(), 1);
                assert_eq!(drops[1].get(), 1);
                assert_eq!(drops[2].get(), usize::from(ids[2] != 0));
            }
        }
    }
    Ok(())
}
