use capnp::{
    dynamic_struct, dynamic_value as value, introspect::Introspect, traits::ImbueMut, ErrorKind,
};
use capntproto_test_support::{
    dynamic_test_capnp::{group_defaults, orphan_group, orphan_payload},
    runtime_test_capnp::harness,
};
use std::{cell::Cell, rc::Rc};
struct Service(Rc<Cell<usize>>);
impl harness::Server for Service {
    async fn echo(
        self: Rc<Self>,
        _: harness::EchoParams,
        mut results: harness::EchoResults,
    ) -> capnp::Result<()> {
        results.get().set_value(73);
        Ok(())
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}
fn client(drops: &Rc<Cell<usize>>) -> harness::Client {
    capnp_rpc::new_client(Service(drops.clone()))
}
fn cap_value(c: harness::Client) -> value::Reader<'static> {
    capnp::dynamic_capability::Client::from(c).into()
}
fn cap_read(v: value::Reader<'_>) -> capnp::Result<harness::Client> {
    v.downcast::<value::Capability>().cast()
}
fn allocator(small: bool) -> capnp::message::HeapAllocator {
    let a = capnp::message::HeapAllocator::new();
    if small {
        a.first_segment_words(1)
            .allocation_strategy(capnp::message::AllocationStrategy::FixedSize)
    } else {
        a
    }
}
fn body_schema(root: &dynamic_struct::Builder<'_>) -> capnp::Result<capnp::schema::StructSchema> {
    root.get_schema()
        .get_field_by_name("body")?
        .get_type()
        .as_struct_schema()
}
fn text(v: value::Reader<'_>) -> capnp::Result<String> {
    Ok(v.downcast::<capnp::text::Reader>().to_str()?.to_owned())
}

#[test]
fn defaults_nested_views_selection_and_move_only_field_ownership() -> capnp::Result<()> {
    for small in [false, true] {
        let mut message = capnp::message::Builder::new(allocator(small));
        let mut root = message.init_root::<group_defaults::Builder<capnp::text::Owned>>();
        root.set_sibling(91);
        let (mut root, token) = value::Builder::from(root)
            .downcast::<dynamic_struct::Builder>()
            .with_orphanage();
        let schema = body_schema(&root)?;
        let sibling = root.get_schema().get_field_by_name("sibling")?;
        let mut group = token.in_struct(&mut root)?.new_group(schema)?;
        assert!(token
            .in_struct(&mut root)?
            .new_group(orphan_payload::Owned::introspect().as_struct_schema()?)
            .is_err());
        token
            .in_struct(&mut root)?
            .read_group(&mut group, |mut g| {
                assert_eq!(g.which()?.unwrap().get_proto().get_name()?, "nested");
                assert_eq!(g.read_named("label", text)?, "default");
                assert!(!g.has_named("label")?);
                assert_eq!(g.read_named("count", |v| Ok(v.downcast::<u32>()))?, 42);
                assert!(g.has_named("count")?);
                assert!(!g.has_named("note")?);
                assert!(g.read_named("note", text).is_err());
                assert_eq!(
                    g.read(sibling, |_| Ok(())).err().unwrap().kind,
                    ErrorKind::TypeMismatch
                );
                g.read_group_named("nested", |mut n| {
                    assert_eq!(n.read_named("text", text)?, "nested");
                    assert!(!n.has_named("text")?);
                    assert!(n.read_named("flag", |v| Ok(v.downcast::<bool>()))?);
                    Ok(())
                })
            })?;
        let mut moved = token
            .in_struct(&mut root)?
            .edit_group(&mut group, |mut g| {
                g.edit_named("label", |v| {
                    v.downcast::<capnp::text::Builder>()
                        .as_bytes_mut()
                        .copy_from_slice(b"changed");
                    Ok(())
                })?;
                g.set_named("count", 9u32.into())?;
                g.set_named("item", value::Reader::Text("generic".into()))?;
                let moved = g.disown_named("label")?;
                assert!(!g.has_named("label")?);
                assert_eq!(g.read_named("label", text)?, "default");
                g.clear_named("note")?;
                assert_eq!(g.read_named("note", text)?, "note");
                assert!(!g.has_named("note")?);
                let note = g.disown_named("note")?;
                drop(note);
                assert_eq!(g.which()?.unwrap().get_proto().get_name()?, "note");
                assert!(g.read_group_named("nested", |_| Ok(())).is_err());
                g.clear_named("nested")?;
                g.edit_group_named("nested", |mut n| {
                    n.set_named("flag", false.into())?;
                    n.edit_named("text", |v| {
                        v.downcast::<capnp::text::Builder>()
                            .as_bytes_mut()
                            .copy_from_slice(b"edited");
                        Ok(())
                    })
                })?;
                Ok(moved)
            })?;
        assert_eq!(
            token.in_struct(&mut root)?.read(&mut moved, text)?,
            "changed"
        );
        token
            .in_struct(&mut root)?
            .edit_group(&mut group, |mut g| {
                g.adopt_named("label", moved).map_err(|e| e.error)
            })?;
        root.adopt_named("body", group).unwrap();
        let root = root.into_reader();
        assert_eq!(root.get_named("sibling")?.downcast::<u64>(), 91);
        let body = root.get_named("body")?.downcast::<dynamic_struct::Reader>();
        assert_eq!(
            body.get_named("label")?.downcast::<capnp::text::Reader>(),
            "changed"
        );
        assert_eq!(
            body.get_named("item")?.downcast::<capnp::text::Reader>(),
            "generic"
        );
        let nested = body
            .get_named("nested")?
            .downcast::<dynamic_struct::Reader>();
        assert_eq!(
            nested.get_named("text")?.downcast::<capnp::text::Reader>(),
            "edited"
        );
        assert!(!nested.get_named("flag")?.downcast::<bool>());
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn group_edits_keep_capabilities_and_addresses_on_errors_and_unwinding() -> capnp::Result<()>
{
    for small in [false, true] {
        for panic in [false, true] {
            let old = Rc::new(Cell::new(0));
            let nested = Rc::new(Cell::new(0));
            let new = Rc::new(Cell::new(0));
            let mut caps = vec![];
            let mut message = capnp::message::Builder::new(allocator(small));
            let mut root = message.init_root::<orphan_group::Builder>();
            root.imbue_mut(&mut caps);
            root.set_sibling(91);
            let mut body = root.reborrow().init_body();
            body.set_cap(client(&old));
            body.set_label("same allocation");
            let address = body.reborrow_as_reader().get_label()?.0.as_ptr();
            body.init_nested().set_cap(client(&nested));
            let (mut root, token) = value::Builder::from(root)
                .downcast::<dynamic_struct::Builder>()
                .with_orphanage();
            let mut group = root.disown_named("body", &token)?;
            let held = token
                .in_struct(&mut root)?
                .read_group(&mut group, |mut g| g.read_named("cap", cap_read))?;
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                token
                    .in_struct(&mut root)?
                    .edit_group(&mut group, |mut g| -> capnp::Result<()> {
                        g.set_named("cap", cap_value(client(&new)))?;
                        g.set_named("value", 17u32.into())?; // Releases the former nested union arm.
                        if panic {
                            panic!("group application callback");
                        }
                        Err(capnp::Error::failed("group application error".into()))
                    })
            }));
            assert_eq!(outcome.is_err(), panic);
            if !panic {
                assert!(outcome.unwrap().is_err());
            }
            assert_eq!(nested.get(), 1);
            assert_eq!(old.get(), 0);
            assert_eq!(new.get(), 0);
            token
                .in_struct(&mut root)?
                .read_group(&mut group, |mut g| {
                    assert_eq!(g.which()?.unwrap().get_proto().get_name()?, "value");
                    assert_eq!(g.read_named("value", |v| Ok(v.downcast::<u32>()))?, 17);
                    assert_eq!(
                        g.read_named("label", |v| Ok(v
                            .downcast::<capnp::text::Reader>()
                            .0
                            .as_ptr()))?,
                        address
                    );
                    Ok(())
                })?;
            let mut cap = token
                .in_struct(&mut root)?
                .edit_group(&mut group, |mut g| g.disown_named("cap"))?;
            assert_eq!(
                token
                    .in_struct(&mut root)?
                    .read(&mut cap, cap_read)?
                    .echo_request()
                    .send()
                    .promise
                    .await?
                    .get()?
                    .get_value(),
                73
            );
            root.adopt_named("body", group).unwrap();
            assert_eq!(
                root.reborrow_as_reader()
                    .get_named("sibling")?
                    .downcast::<u64>(),
                91
            );
            drop(cap);
            assert_eq!(new.get(), 1);
            assert_eq!(
                held.echo_request().send().promise.await?.get()?.get_value(),
                73
            );
            drop(held);
            assert_eq!(old.get(), 1);
        }
    }
    Ok(())
}

#[test]
fn group_copy_is_independent_and_rejects_wrong_brand_arena_field_and_type() -> capnp::Result<()> {
    for small in [false, true] {
        let drops = Rc::new(Cell::new(0));
        let mut source_caps = vec![];
        let mut source = capnp::message::Builder::new(allocator(small));
        let mut src = source.init_root::<orphan_group::Builder>();
        src.imbue_mut(&mut source_caps);
        src.set_sibling(999);
        let mut body = src.reborrow().init_body();
        body.set_cap(client(&drops));
        body.set_label("copied");
        body.set_value(22);
        let address = src.reborrow_as_reader().get_body().get_label()?.0.as_ptr();
        let mut caps = vec![];
        let mut message = capnp::message::Builder::new(allocator(small));
        let mut root = message.init_root::<orphan_group::Builder>();
        root.imbue_mut(&mut caps);
        root.set_sibling(91);
        let (mut root, token) = value::Builder::from(root)
            .downcast::<dynamic_struct::Builder>()
            .with_orphanage();
        let mut group = token
            .in_struct(&mut root)?
            .copy(src.reborrow_as_reader().get_body().into())?;
        src.reborrow().get_body().set_label("changed");
        token
            .in_struct(&mut root)?
            .read_group(&mut group, |mut g| {
                assert_eq!(g.read_named("label", text)?, "copied");
                assert_ne!(
                    g.read_named("label", |v| Ok(v
                        .downcast::<capnp::text::Reader>()
                        .0
                        .as_ptr()))?,
                    address
                );
                Ok(())
            })?;
        let mut foreign_message = capnp::message::Builder::new_default();
        let (mut foreign, foreign_token) =
            value::Builder::from(foreign_message.init_root::<orphan_group::Builder>())
                .downcast::<dynamic_struct::Builder>()
                .with_orphanage();
        assert_eq!(
            foreign_token
                .in_struct(&mut foreign)?
                .read_group(&mut group, |_| Ok(()))
                .err()
                .unwrap()
                .kind,
            ErrorKind::WrongArena
        );
        let other = foreign_token.in_struct(&mut foreign)?.copy(17u32.into())?;
        let other = token
            .in_struct(&mut root)?
            .edit_group(&mut group, |mut g| {
                let error = g.adopt_named("value", other).unwrap_err();
                assert_eq!(error.error.kind, ErrorKind::WrongArena);
                let foreign_owner = error.orphan;
                let owned = g.disown_named("cap")?;
                let error = g.adopt_named("missing", owned).unwrap_err();
                let error = g.adopt_named("value", error.orphan).unwrap_err();
                assert_eq!(error.error.kind, ErrorKind::TypeMismatch);
                assert_eq!(g.which()?.unwrap().get_proto().get_name()?, "value");
                assert_eq!(g.read_named("value", |v| Ok(v.downcast::<u32>()))?, 22);
                g.adopt_named("cap", error.orphan).map_err(|e| e.error)?;
                Ok(foreign_owner)
            })?;
        drop(other);
        root.adopt_named("body", group).unwrap();
        assert_eq!(
            root.reborrow_as_reader()
                .get_named("sibling")?
                .downcast::<u64>(),
            91
        );
        root.clear(root.get_schema().get_field_by_name("body")?)?;
        assert_eq!(drops.get(), 0); // Original source still owns the same capability.
        src.reborrow()
            .get_body()
            .set_cap(client(&Rc::new(Cell::new(0))));
        assert_eq!(drops.get(), 1);

        let mut branded_message = capnp::message::Builder::new_default();
        let (mut branded, token) = value::Builder::from(
            branded_message.init_root::<group_defaults::Builder<capnp::text::Owned>>(),
        )
        .downcast::<dynamic_struct::Builder>()
        .with_orphanage();
        let schema = body_schema(&branded)?;
        let mut group = token.in_struct(&mut branded)?.new_group(schema)?;
        let mut other_message = capnp::message::Builder::new_default();
        let other = other_message.init_root::<group_defaults::Builder<capnp::data::Owned>>();
        let other = value::Builder::from(other).downcast::<dynamic_struct::Builder>();
        let wrong = body_schema(&other)?.get_field_by_name("item")?;
        assert_eq!(
            token
                .in_struct(&mut branded)?
                .edit_group(&mut group, |mut g| g
                    .set(wrong, value::Reader::Data(b"wrong")))
                .err()
                .unwrap()
                .kind,
            ErrorKind::TypeMismatch
        );
        token
            .in_struct(&mut branded)?
            .edit_group(&mut group, |mut g| {
                g.set_named("item", value::Reader::Text("ok".into()))
            })?;
        branded.adopt_named("body", group).unwrap();
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_orphan_groups_traces() -> capnp::Result<()> {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_ORPHAN_GROUPS_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    type PanicHook = std::sync::Arc<dyn Fn(&std::panic::PanicHookInfo<'_>) + Send + Sync>;
    struct Hook(Option<PanicHook>);
    impl Drop for Hook {
        fn drop(&mut self) {
            if !std::thread::panicking() {
                let old = self.0.take().unwrap();
                std::panic::set_hook(Box::new(move |info| old(info)));
            }
        }
    }
    let original: PanicHook = std::panic::take_hook().into();
    let _hook = Hook(Some(original.clone()));
    std::panic::set_hook(Box::new(move |info| {
        if info.payload().downcast_ref::<&str>() != Some(&"group callback unwinds after edit") {
            original(info);
        }
    }));
    for (case_index, case) in cases.iter().enumerate() {
        for small in [false, true] {
            for panic in [false, true] {
                let drops: [Rc<Cell<usize>>; 3] = std::array::from_fn(|_| Rc::new(Cell::new(0)));
                let a = client(&drops[0]);
                let b = client(&drops[1]);
                let mut ids = [a.client.hook.get_ptr(), b.client.hook.get_ptr(), 0];
                let mut caps = vec![];
                let mut message = capnp::message::Builder::new(allocator(small));
                let mut root = message.init_root::<orphan_group::Builder>();
                root.imbue_mut(&mut caps);
                root.set_sibling(91);
                let mut body = root.reborrow().init_body();
                body.set_cap(a);
                body.set_label("stable allocation");
                let address = body.reborrow_as_reader().get_label()?.0.as_ptr();
                body.init_nested().set_cap(b);
                let (mut root, token) = value::Builder::from(root)
                    .downcast::<dynamic_struct::Builder>()
                    .with_orphanage();
                let mut group = None;
                let mut moved = None;
                let mut held: Option<harness::Client> = None;
                let mut created_c = false;
                let mut other_message = capnp::message::Builder::new_default();
                let (mut other, other_token) =
                    value::Builder::from(other_message.init_root::<orphan_group::Builder>())
                        .downcast::<dynamic_struct::Builder>()
                        .with_orphanage();
                for step in case["steps"].as_array().unwrap() {
                    let s: Vec<u64> = serde_json::from_value(step["state"].clone()).unwrap();
                    match step["action"].as_str().unwrap() {
                        "detach" => group = Some(root.disown_named("body", &token)?),
                        "materialize" => token
                            .in_struct(&mut root)?
                            .materialize_group(group.as_mut().unwrap())?,
                        "edit" => {
                            let c = client(&drops[2]);
                            ids[2] = c.client.hook.get_ptr();
                            created_c = true;
                            let outcome =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    token.in_struct(&mut root)?.edit_group(
                                        group.as_mut().unwrap(),
                                        |mut g| -> capnp::Result<()> {
                                            g.set_named("cap", cap_value(c))?;
                                            if panic {
                                                panic!("group callback unwinds after edit");
                                            }
                                            Err(capnp::Error::failed(
                                                "group callback error after edit".into(),
                                            ))
                                        },
                                    )
                                }));
                            assert_eq!(outcome.is_err(), panic);
                            if !panic {
                                assert!(outcome.unwrap().is_err());
                            }
                        }
                        "select" => token
                            .in_struct(&mut root)?
                            .edit_group(group.as_mut().unwrap(), |mut g| {
                                g.set_named("value", 17u32.into())
                            })?,
                        "read" => {
                            held = Some(
                                token
                                    .in_struct(&mut root)?
                                    .read_group(group.as_mut().unwrap(), |mut g| {
                                        g.read_named("cap", cap_read)
                                    })?,
                            )
                        }
                        "release" => drop(held.take()),
                        "take" => {
                            moved = Some(
                                token
                                    .in_struct(&mut root)?
                                    .edit_group(group.as_mut().unwrap(), |mut g| {
                                        g.disown_named("cap")
                                    })?,
                            )
                        }
                        "put" => token.in_struct(&mut root)?.edit_group(
                            group.as_mut().unwrap(),
                            |mut g| {
                                g.adopt_named("cap", moved.take().unwrap())
                                    .map_err(|e| e.error)
                            },
                        )?,
                        "drop-moved" => drop(moved.take()),
                        "bad-type" => {
                            let bad = token.in_struct(&mut root)?.new_text(3)?;
                            let mut bad = token.in_struct(&mut root)?.edit_group(
                                group.as_mut().unwrap(),
                                |mut g| {
                                    let e = g.adopt_named("value", bad).unwrap_err();
                                    assert_eq!(e.error.kind, ErrorKind::TypeMismatch);
                                    Ok(e.orphan)
                                },
                            )?;
                            assert_eq!(
                                token.in_struct(&mut root)?.read(&mut bad, |v| Ok(v
                                    .downcast::<capnp::text::Reader>()
                                    .len()))?,
                                3
                            );
                        }
                        "bad-arena" => {
                            let bad = other_token.in_struct(&mut other)?.copy(99u32.into())?;
                            let mut bad = token.in_struct(&mut root)?.edit_group(
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
                                    .read(&mut bad, |v| Ok(v.downcast::<u32>()))?,
                                99
                            );
                        }
                        "adopt" => root.adopt_named("body", group.take().unwrap()).unwrap(),
                        "drop" => {
                            if let Some(g) = group.take() {
                                drop(g);
                            } else {
                                drop(root.disown_named("body", &token)?);
                            }
                        }
                        other => panic!("unknown action {other}"),
                    }
                    assert_eq!(group.is_some(), s[0] == 1);
                    assert_eq!(held.is_some(), s[6] == 1);
                    assert_eq!(moved.is_some(), s[8] != 0);
                    assert_eq!(
                        root.reborrow_as_reader()
                            .get_named("sibling")?
                            .downcast::<u64>(),
                        s[12]
                    );
                    assert_eq!(s[13], 1);
                    if s[0] == 1 && s[17] == 1 {
                        let mut g = group.take().unwrap();
                        token.in_struct(&mut root)?.materialize_group(&mut g)?;
                        let mut typed = g.release_as::<orphan_group::body::Owned>().unwrap();
                        token.in_struct(&mut root)?.read_typed(&mut typed, |g| {
                            assert_eq!(g.get_label()?.0.as_ptr(), address);
                            assert_eq!(g.has_cap(), s[1] != 0);
                            if s[1] != 0 {
                                assert_eq!(
                                    g.get_cap()?.client.hook.get_ptr(),
                                    ids[if s[1] == 1 { 0 } else { 2 }]
                                );
                            }
                            if s[4] == 0 {
                                let orphan_group::body::Nested(n) = g.which()? else {
                                    panic!("lost nested arm");
                                };
                                assert_eq!(n.get_cap()?.client.hook.get_ptr(), ids[1]);
                            } else {
                                let orphan_group::body::Value(v) = g.which()? else {
                                    panic!("lost value arm");
                                };
                                assert_eq!(v, 17);
                            }
                            Ok(())
                        })?;
                        group = Some(typed.into_dynamic());
                    } else if let Some(g) = group.as_mut() {
                        token.in_struct(&mut root)?.read_group(g, |mut g| {
                            assert_eq!(
                                g.which()?.unwrap().get_proto().get_name()?,
                                if s[4] == 0 { "nested" } else { "value" }
                            );
                            assert_eq!(
                                g.read_named("label", |v| Ok(v
                                    .downcast::<capnp::text::Reader>()
                                    .0
                                    .as_ptr()))?,
                                address
                            );
                            assert_eq!(g.has_named("cap")?, s[1] != 0);
                            if s[1] != 0 {
                                assert_eq!(
                                    g.read_named("cap", cap_read)?.client.hook.get_ptr(),
                                    ids[if s[1] == 1 { 0 } else { 2 }]
                                );
                            }
                            if s[4] == 0 {
                                g.read_group_named("nested", |mut n| {
                                    assert_eq!(
                                        n.read_named("cap", cap_read)?.client.hook.get_ptr(),
                                        ids[1]
                                    );
                                    Ok(())
                                })?;
                            } else {
                                assert_eq!(g.read_named("value", |v| Ok(v.downcast::<u32>()))?, 17);
                            }
                            Ok(())
                        })?;
                    }
                    let body = root
                        .reborrow_as_reader()
                        .get_named("body")?
                        .downcast::<dynamic_struct::Reader>();
                    if s[0] == 2 {
                        assert_eq!(
                            body.which()?.unwrap().get_proto().get_name()?,
                            if s[4] == 0 { "nested" } else { "value" }
                        );
                        assert_eq!(
                            body.get_named("label")?
                                .downcast::<capnp::text::Reader>()
                                .0
                                .as_ptr(),
                            address
                        );
                        assert_eq!(body.has_named("cap")?, s[1] != 0);
                        if s[1] != 0 {
                            assert_eq!(
                                cap_read(body.get_named("cap")?)?.client.hook.get_ptr(),
                                ids[if s[1] == 1 { 0 } else { 2 }]
                            );
                        }
                        if s[4] == 0 {
                            assert_eq!(
                                cap_read(
                                    body.get_named("nested")?
                                        .downcast::<dynamic_struct::Reader>()
                                        .get_named("cap")?
                                )?
                                .client
                                .hook
                                .get_ptr(),
                                ids[1]
                            );
                        } else {
                            assert_eq!(body.get_named("value")?.downcast::<u32>(), 17);
                        }
                    } else {
                        assert!(!body.has_named("cap")?);
                        assert!(!body.has_named("label")?);
                        assert_eq!(body.which()?.unwrap().get_proto().get_name()?, "empty");
                    }
                    if let Some(m) = moved.as_mut() {
                        let c = token.in_struct(&mut root)?.read(m, cap_read)?;
                        assert_eq!(c.client.hook.get_ptr(), ids[if s[8] == 1 { 0 } else { 2 }]);
                        assert_eq!(
                            c.echo_request().send().promise.await?.get()?.get_value(),
                            73
                        );
                    }
                    if let Some(c) = &held {
                        assert_eq!(c.client.hook.get_ptr(), ids[0]);
                        assert_eq!(
                            c.echo_request().send().promise.await?.get()?.get_value(),
                            73
                        );
                    }
                    for i in 0..3 {
                        assert_eq!(
                            drops[i].get(),
                            usize::from(s[14 + i] == 0 && (i != 2 || created_c)),
                            "trace {case_index}: {step}"
                        );
                    }
                }
                drop(group);
                drop(moved);
                drop(held);
                drop(root.disown_named("body", &token)?);
                assert_eq!(drops[0].get(), 1);
                assert_eq!(drops[1].get(), 1);
                assert_eq!(drops[2].get(), usize::from(created_c));
            }
        }
    }
    Ok(())
}

#[test]
fn failed_group_copy_releases_partial_capability_ownership() -> capnp::Result<()> {
    use capnp::traits::Imbue;
    for small in [false, true] {
        let a = Rc::new(Cell::new(0));
        let b = Rc::new(Cell::new(0));
        let mut source_caps = vec![];
        let mut source = capnp::message::Builder::new(allocator(small));
        let mut root = source.init_root::<orphan_group::Builder>();
        root.imbue_mut(&mut source_caps);
        let mut body = root.init_body();
        body.set_cap(client(&a));
        body.init_nested().set_cap(client(&b));
        // The active nested arm is copied first. Its capability is valid;
        // the following non-union cap slot is deliberately invalid.
        let invalid_caps = vec![None, Some(source_caps[1].as_ref().unwrap().add_ref())];
        let mut reader = source.get_root_as_reader::<orphan_group::Reader>()?;
        reader.imbue(&invalid_caps);
        let mut caps = vec![];
        let mut destination = capnp::message::Builder::new(allocator(small));
        let mut root = destination.init_root::<orphan_group::Builder>();
        root.imbue_mut(&mut caps);
        root.set_sibling(91);
        let (mut root, token) = value::Builder::from(root)
            .downcast::<dynamic_struct::Builder>()
            .with_orphanage();
        let result = token
            .in_struct(&mut root)?
            .copy_group(value::Reader::from(reader.get_body()).downcast());
        assert!(result.is_err());
        drop(result);
        assert_eq!(
            root.reborrow_as_reader()
                .get_named("sibling")?
                .downcast::<u64>(),
            91
        );
        drop(invalid_caps);
        drop(source);
        drop(source_caps);
        assert_eq!(a.get(), 1);
        assert_eq!(b.get(), 1);
        assert!(caps.iter().all(Option::is_none));
    }
    Ok(())
}

#[test]
fn contiguous_group_typed_edits_preserve_caps_defaults_and_unwind_ownership() -> capnp::Result<()> {
    for small in [false, true] {
        for unwind in [false, true] {
            let old = Rc::new(Cell::new(0));
            let new = Rc::new(Cell::new(0));
            let nested = Rc::new(Cell::new(0));
            let mut caps = vec![];
            let mut message = capnp::message::Builder::new(allocator(small));
            let mut root = message.init_root::<orphan_group::Builder>();
            root.imbue_mut(&mut caps);
            root.set_sibling(91);
            let mut body = root.reborrow().init_body();
            body.set_cap(client(&old));
            body.set_label("stable");
            let address = body.reborrow_as_reader().get_label()?.0.as_ptr();
            body.init_nested().set_cap(client(&nested));
            let (mut root, token) = value::Builder::from(root)
                .downcast::<dynamic_struct::Builder>()
                .with_orphanage();
            let mut group = root.disown_named("body", &token)?;
            token.in_struct(&mut root)?.materialize_group(&mut group)?;
            let failed = group.release_as::<orphan_payload::Owned>().err().unwrap();
            assert_eq!(failed.error.kind, ErrorKind::TypeMismatch);
            let mut typed = failed
                .orphan
                .release_as::<orphan_group::body::Owned>()
                .unwrap();
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                token
                    .in_struct(&mut root)?
                    .edit_typed(&mut typed, |mut g| -> capnp::Result<()> {
                        g.set_cap(client(&new));
                        // Generated union setters can leave inactive pointer slots.
                        // Conversion/adoption must release those hidden owners too.
                        g.set_value(17);
                        if unwind {
                            panic!("typed group edit completed");
                        }
                        Err(capnp::Error::failed("typed group edit completed".into()))
                    })
            }));
            assert_eq!(outcome.is_err(), unwind);
            if !unwind {
                assert!(outcome.unwrap().is_err());
            }
            assert_eq!(old.get(), 1);
            assert_eq!(new.get(), 0);
            token.in_struct(&mut root)?.read_typed(&mut typed, |g| {
                assert_eq!(g.get_label()?.0.as_ptr(), address);
                assert!(matches!(g.which()?, orphan_group::body::Value(17)));
                Ok(())
            })?;
            root.adopt_named("body", typed.into_dynamic()).unwrap();
            assert_eq!(nested.get(), 1);
            assert_eq!(
                root.reborrow_as_reader()
                    .get_named("sibling")?
                    .downcast::<u64>(),
                91
            );
            drop(root.disown_named("body", &token)?);
            assert_eq!(new.get(), 1);
        }
        let mut message = capnp::message::Builder::new(allocator(small));
        let (mut root, token) = value::Builder::from(
            message.init_root::<group_defaults::Builder<capnp::text::Owned>>(),
        )
        .downcast::<dynamic_struct::Builder>()
        .with_orphanage();
        let schema = body_schema(&root)?;
        let group = token.in_struct(&mut root)?.new_struct(schema)?;
        let wrong = group
            .release_as::<group_defaults::body::Owned<capnp::data::Owned>>()
            .err()
            .unwrap();
        assert_eq!(wrong.error.kind, ErrorKind::TypeMismatch);
        let mut group = wrong
            .orphan
            .release_as::<group_defaults::body::Owned<capnp::text::Owned>>()
            .unwrap();
        token.in_struct(&mut root)?.read_typed(&mut group, |g| {
            assert_eq!(g.get_count(), 42);
            assert_eq!(g.get_label()?, "default");
            Ok(())
        })?;
        token
            .in_struct(&mut root)?
            .edit_typed(&mut group, |mut g| {
                g.set_count(73);
                Ok(())
            })?;
        let mut group = group.into_dynamic();
        token
            .in_struct(&mut root)?
            .read_group(&mut group, |mut g| {
                assert_eq!(g.read_named("count", |v| Ok(v.downcast::<u32>()))?, 73);
                Ok(())
            })?;
        root.adopt_named("body", group).unwrap();
    }
    Ok(())
}
