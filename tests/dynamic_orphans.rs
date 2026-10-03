use capnp::{dynamic_list, dynamic_struct, dynamic_value as value, traits::ImbueMut, ErrorKind};
use capntproto_test_support::{
    dynamic_test_capnp::{orphan_case, orphan_payload},
    runtime_test_capnp::harness,
};
use std::{cell::Cell, rc::Rc};

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
fn allocator(small: bool) -> capnp::message::HeapAllocator {
    let a = capnp::message::HeapAllocator::new();
    if small {
        a.first_segment_words(1)
            .allocation_strategy(capnp::message::AllocationStrategy::FixedSize)
    } else {
        a
    }
}
fn structure<'a>(
    root: dynamic_struct::Builder<'a>,
    name: &str,
) -> capnp::Result<dynamic_struct::Builder<'a>> {
    Ok(root.get_named(name)?.downcast())
}
fn list<'a>(
    root: dynamic_struct::Builder<'a>,
    name: &str,
) -> capnp::Result<dynamic_list::Builder<'a>> {
    Ok(root.get_named(name)?.downcast())
}

#[test]
fn pointer_moves_defaults_union_errors_and_wrong_arena_preserve_values() -> capnp::Result<()> {
    for small in [false, true] {
        let mut a = capnp::message::Builder::new(allocator(small));
        let mut root = a.init_root::<orphan_case::Builder>();
        root.reborrow().init_source().set_text("same allocation");
        let address = root
            .reborrow()
            .get_source()?
            .get_text()?
            .into_reader()
            .0
            .as_ptr();
        root.set_sentinel(91);
        let root = value::Builder::from(root).downcast::<dynamic_struct::Builder>();
        let (mut root, token) = root.with_orphanage();
        let mut b = capnp::message::Builder::new_default();
        let other = value::Builder::from(b.init_root::<orphan_case::Builder>())
            .downcast::<dynamic_struct::Builder>();
        let (mut other, wrong) = other.with_orphanage();
        assert_eq!(
            root.disown_named("source", &wrong).err().unwrap().kind,
            ErrorKind::WrongArena
        );
        assert!(root.disown_named("arm", &token).is_err());
        let orphan = root.disown_named("source", &token)?;
        assert!(!root.has_named("source")?);
        let error = other.adopt_named("arm", orphan).unwrap_err();
        assert_eq!(error.error.kind, ErrorKind::WrongArena);
        assert_eq!(other.which()?.unwrap().get_proto().get_name()?, "sentinel");
        let error = root.adopt_named("description", error.orphan).unwrap_err();
        assert_eq!(error.error.kind, ErrorKind::TypeMismatch);
        let error = root.adopt_named("missing", error.orphan).unwrap_err();
        root.adopt_named("arm", error.orphan).unwrap();
        assert_eq!(root.which()?.unwrap().get_proto().get_name()?, "arm");
        assert_eq!(
            root.reborrow_as_reader()
                .get_named("arm")?
                .downcast::<dynamic_struct::Reader>()
                .get_named("text")?
                .downcast::<capnp::text::Reader>()
                .0
                .as_ptr(),
            address
        );
        let orphan = root.disown_named("arm", &token)?;
        root.adopt_named("any", orphan).unwrap();
        assert_eq!(
            root.reborrow_as_reader()
                .get_named("any")?
                .downcast::<capnp::any_pointer::Reader>()
                .get_as::<orphan_payload::Reader>()?
                .get_text()?,
            "same allocation"
        );
        // Dynamic disown follows the C++ builder accessor's default semantics.
        let default = root.disown_named("description", &token)?;
        assert!(!root.has_named("description")?);
        root.adopt_named("any", default).unwrap();
        assert_eq!(
            root.reborrow_as_reader()
                .get_named("any")?
                .downcast::<capnp::any_pointer::Reader>()
                .get_as::<capnp::text::Reader>()?,
            "default"
        );
    }
    Ok(())
}

#[test]
fn scalar_defaults_and_list_elements_move_with_checked_types_and_bounds() -> capnp::Result<()> {
    let mut a = capnp::message::Builder::new_default();
    let mut root = a.init_root::<orphan_case::Builder>();
    root.set_flag(false);
    root.set_float(9.25);
    root.set_kind(orphan_case::Kind::Zero);
    root.reborrow().init_numbers(2).set(0, 88);
    root.reborrow()
        .init_kinds(2)
        .set(0, orphan_case::Kind::Other);
    root.reborrow().init_texts(2).set(0, "move me");
    let root = value::Builder::from(root).downcast::<dynamic_struct::Builder>();
    let (mut root, token) = root.with_orphanage();
    for (field, default, expected) in [
        ("flag", "true", "false"),
        ("float", "-1.5", "9.25"),
        ("kind", "other", "zero"),
    ] {
        let orphan = root.disown_named(field, &token)?;
        assert_eq!(
            format!("{:?}", root.reborrow_as_reader().get_named(field)?),
            default
        );
        root.adopt_named(field, orphan).unwrap();
        assert_eq!(
            format!("{:?}", root.reborrow_as_reader().get_named(field)?),
            expected
        );
    }
    for (field, expected) in [
        ("numbers", "88"),
        ("kinds", "other"),
        ("texts", "\"move me\""),
    ] {
        let mut values = list(root.reborrow(), field)?;
        let orphan = values.disown(0, &token)?;
        let failed = values.adopt(3, orphan).unwrap_err();
        values.adopt(1, failed.orphan).unwrap();
        assert_eq!(format!("{:?}", values.into_reader().get(1)?), expected);
    }
    // A list-only caller can mint the same kind of token without a struct root.
    let numbers = list(root.reborrow(), "numbers")?;
    let (mut numbers, token) = numbers.with_orphanage();
    let orphan = numbers.disown(1, &token)?;
    numbers.adopt(0, orphan).unwrap();
    assert_eq!(numbers.into_reader().get(0)?.downcast::<u32>(), 88);
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn capabilities_survive_detachment_and_release_replaced_or_abandoned_owners(
) -> capnp::Result<()> {
    for small in [false, true] {
        let drops = Rc::new(Cell::new(0));
        let replaced = Rc::new(Cell::new(0));
        let mut caps = Vec::new();
        let mut a = capnp::message::Builder::new(allocator(small));
        let mut root = a.init_root::<orphan_case::Builder>();
        root.imbue_mut(&mut caps);
        let cap = client(&drops);
        let identity = cap.client.hook.get_ptr();
        root.reborrow().init_source().set_cap(cap);
        root.reborrow().init_target().set_cap(client(&replaced));
        root.reborrow().init_tokens(1);
        let root = value::Builder::from(root).downcast::<dynamic_struct::Builder>();
        let (mut root, token) = root.with_orphanage();
        let orphan = root.disown_named("source", &token)?;
        assert_eq!(drops.get(), 0);
        root.adopt_named("target", orphan).unwrap();
        assert_eq!(replaced.get(), 1);
        let mut target = structure(root.reborrow(), "target")?;
        let held = target
            .reborrow_as_reader()
            .get_named("cap")?
            .downcast::<value::Capability>()
            .cast::<harness::Client>()?;
        assert_eq!(held.client.hook.get_ptr(), identity);
        let orphan = target.disown_named("cap", &token)?;
        list(root.reborrow(), "tokens")?.adopt(0, orphan).unwrap();
        let orphan = list(root.reborrow(), "tokens")?.disown(0, &token)?;
        let response = held.echo_request().send().promise.await?;
        assert_eq!(response.get()?.get_value(), 123);
        drop(orphan);
        assert_eq!(drops.get(), 0);
        drop(held);
        assert_eq!(drops.get(), 1);
    }
    Ok(())
}

#[test]
fn inline_struct_and_nested_group_moves_preserve_descendants_and_siblings() -> capnp::Result<()> {
    for small in [false, true] {
        let drops = Rc::new(Cell::new(0));
        let old = Rc::new(Cell::new(0));
        let mut caps = Vec::new();
        let mut a = capnp::message::Builder::new(allocator(small));
        let mut root = a.init_root::<orphan_case::Builder>();
        root.imbue_mut(&mut caps);
        let mut values = root.reborrow().init_values(2);
        values.reborrow().get(0).set_text("inline child");
        values.reborrow().get(0).set_number(90);
        values.reborrow().get(0).set_cap(client(&drops));
        let address = values
            .reborrow()
            .get(0)
            .get_text()?
            .into_reader()
            .0
            .as_ptr();
        values.get(1).set_cap(client(&old));
        let mut groups = root.reborrow().init_groups(2);
        groups.reborrow().get(0).set_sibling(111);
        groups.reborrow().get(1).set_sibling(222);
        let mut body = groups.reborrow().get(0).init_body();
        body.set_label("group child");
        let mut nested = body.init_nested();
        nested.set_flag(true);
        nested.set_text("nested child");
        let nested_address = nested.reborrow().get_text()?.into_reader().0.as_ptr();
        nested.set_cap(client(&drops));
        groups.get(1).init_body().set_cap(client(&old));
        let root = value::Builder::from(root).downcast::<dynamic_struct::Builder>();
        let (mut root, token) = root.with_orphanage();
        let mut values = list(root.reborrow(), "values")?;
        let orphan = values.disown(0, &token)?;
        assert_eq!(
            values
                .reborrow()
                .into_reader()
                .get(0)?
                .downcast::<dynamic_struct::Reader>()
                .get_named("number")?
                .downcast::<u32>(),
            42
        );
        values.adopt(1, orphan).unwrap();
        let moved = values
            .into_reader()
            .get(1)?
            .downcast::<dynamic_struct::Reader>();
        assert_eq!(
            moved
                .get_named("text")?
                .downcast::<capnp::text::Reader>()
                .0
                .as_ptr(),
            address
        );
        assert_eq!(old.get(), 1);
        let mut groups = list(root.reborrow(), "groups")?;
        let mut source = groups
            .reborrow()
            .get(0)?
            .downcast::<dynamic_struct::Builder>();
        let orphan = source.disown_named("body", &token)?;
        assert_eq!(
            source
                .reborrow_as_reader()
                .get_named("sibling")?
                .downcast::<u64>(),
            111
        );
        assert_eq!(
            structure(source.reborrow(), "body")?
                .which()?
                .unwrap()
                .get_proto()
                .get_name()?,
            "empty"
        );
        let mut target = groups
            .reborrow()
            .get(1)?
            .downcast::<dynamic_struct::Builder>();
        target.adopt_named("body", orphan).unwrap();
        assert_eq!(old.get(), 2);
        assert_eq!(
            target
                .reborrow_as_reader()
                .get_named("sibling")?
                .downcast::<u64>(),
            222
        );
        let body = target
            .reborrow_as_reader()
            .get_named("body")?
            .downcast::<dynamic_struct::Reader>();
        let nested = body
            .get_named("nested")?
            .downcast::<dynamic_struct::Reader>();
        assert_eq!(
            nested
                .get_named("text")?
                .downcast::<capnp::text::Reader>()
                .0
                .as_ptr(),
            nested_address
        );
        assert!(nested.get_named("flag")?.downcast::<bool>());
        drop(target.disown_named("body", &token)?);
        assert_eq!(drops.get(), 1);
        drop(list(root.reborrow(), "values")?.disown(1, &token)?);
        assert_eq!(drops.get(), 2);
    }
    Ok(())
}

#[test]
fn evolved_unknown_capabilities_are_retained_and_narrow_adoption_is_atomic() -> capnp::Result<()> {
    use capntproto_test_support::dynamic_test_capnp::{
        large_orphan_case, small_orphan, small_orphan_case,
    };
    for small in [false, true] {
        let drops = Rc::new(Cell::new(0));
        let mut caps = Vec::new();
        let mut a = capnp::message::Builder::new(allocator(small));
        let mut future = a.init_root::<large_orphan_case::Builder>();
        future.imbue_mut(&mut caps);
        future.reborrow().init_source().set_cap(client(&drops));
        future.reborrow().get_source()?.set_text("unknown child");
        future.reborrow().get_source()?.set_number(99);
        let address = future.get_source()?.get_text()?.into_reader().0.as_ptr();
        let mut old = a.get_root::<small_orphan_case::Builder>()?;
        old.imbue_mut(&mut caps);
        old.reborrow().init_targets(1).get(0).set_number(73);
        let root = value::Builder::from(old).downcast::<dynamic_struct::Builder>();
        let (mut root, token) = root.with_orphanage();
        let orphan = root.disown_named("source", &token)?;
        let mut targets = list(root.reborrow(), "targets")?;
        let error = targets.adopt(0, orphan).unwrap_err();
        assert_eq!(error.error.kind, ErrorKind::WouldTruncate);
        assert_eq!(
            targets
                .into_reader()
                .get(0)?
                .downcast::<dynamic_struct::Reader>()
                .get_named("number")?
                .downcast::<u32>(),
            73
        );
        assert_eq!(drops.get(), 0);
        root.adopt_named("source", error.orphan).unwrap();
        let source = root
            .reborrow_as_reader()
            .get_named("source")?
            .downcast::<dynamic_struct::Reader>();
        use capnp::traits::IntoInternalStructReader;
        let future = orphan_payload::Reader::from(
            source
                .downcast::<small_orphan::Owned>()
                .into_internal_struct_reader(),
        );
        assert_eq!(future.get_text()?.0.as_ptr(), address);
        drop(root.disown_named("source", &token)?);
        assert_eq!(drops.get(), 1);
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn context_mismatch_brand_mismatch_and_revocation_cannot_be_bypassed() -> capnp::Result<()> {
    use capntproto_test_support::dynamic_test_capnp::{base, derived, orphan_brands};
    let drops = Rc::new(Cell::new(0));
    let mut caps = Vec::new();
    let mut wrong_caps = Vec::new();
    let mut a = capnp::message::Builder::new_default();
    let owner = capnp_rpc::RevocableServer::<harness::Client>::new(Server(drops.clone()));
    let mut root = a.init_root::<orphan_case::Builder>();
    root.imbue_mut(&mut caps);
    root.set_token(owner.get_client());
    let root = value::Builder::from(root).downcast::<dynamic_struct::Builder>();
    let (mut root, token) = root.with_orphanage();
    let orphan = root.disown_named("token", &token)?;
    let mut wrong = root.reborrow().downcast::<orphan_case::Owned>();
    wrong.imbue_mut(&mut wrong_caps);
    let mut wrong = value::Builder::from(wrong).downcast::<dynamic_struct::Builder>();
    let error = wrong.adopt_named("token", orphan).unwrap_err();
    assert_eq!(error.error.kind, ErrorKind::WrongArena);
    drop(owner);
    assert_eq!(drops.get(), 1);
    root.adopt_named("token", error.orphan).unwrap();
    let cap = root
        .reborrow_as_reader()
        .get_named("token")?
        .downcast::<value::Capability>()
        .cast::<harness::Client>()?;
    assert!(cap.echo_request().send().promise.await.is_err());

    // Equal nominal IDs are insufficient: generic arguments must also match.
    let mut m = capnp::message::Builder::new_default();
    let mut caps = Vec::new();
    let mut root = m.init_root::<orphan_brands::Builder>();
    root.imbue_mut(&mut caps);
    root.reborrow().init_text().set_value("text")?;
    root.reborrow().init_data().set_value(&b"stable"[..])?;
    struct Derived;
    impl base::Server<capnp::text::Owned> for Derived {}
    impl derived::Server<capnp::text::Owned> for Derived {}
    root.set_derived(capnp_rpc::new_client(Derived));
    let root = value::Builder::from(root).downcast::<dynamic_struct::Builder>();
    let (mut root, token) = root.with_orphanage();
    let orphan = root.disown_named("text", &token)?;
    let error = root.adopt_named("data", orphan).unwrap_err();
    assert_eq!(error.error.kind, ErrorKind::TypeMismatch);
    assert_eq!(
        structure(root.reborrow(), "data")?
            .into_reader()
            .get_named("value")?
            .downcast::<capnp::data::Reader>(),
        b"stable"
    );
    root.adopt_named("text", error.orphan).unwrap();
    let orphan = root.disown_named("derived", &token)?;
    let error = root.adopt_named("wrong", orphan).unwrap_err();
    assert_eq!(error.error.kind, ErrorKind::TypeMismatch);
    root.adopt_named("base", error.orphan).unwrap();
    Ok(())
}

fn slot_cap(
    root: dynamic_struct::Reader<'_>,
    mode: u8,
    index: u32,
) -> capnp::Result<value::Capability> {
    let v = match mode {
        0 => root
            .get_named(if index == 0 { "source" } else { "target" })?
            .downcast::<dynamic_struct::Reader>()
            .get_named("cap")?,
        1 => root
            .get_named("tokens")?
            .downcast::<dynamic_list::Reader>()
            .get(index)?,
        2 => root
            .get_named("values")?
            .downcast::<dynamic_list::Reader>()
            .get(index)?
            .downcast::<dynamic_struct::Reader>()
            .get_named("cap")?,
        3 => root
            .get_named("groups")?
            .downcast::<dynamic_list::Reader>()
            .get(index)?
            .downcast::<dynamic_struct::Reader>()
            .get_named("body")?
            .downcast::<dynamic_struct::Reader>()
            .get_named("cap")?,
        _ => unreachable!(),
    };
    Ok(v.downcast())
}
fn take_slot<'m>(
    root: &mut dynamic_struct::Builder<'_>,
    mode: u8,
    index: u32,
    token: &capnp::dynamic_orphan::Orphanage<'m>,
) -> capnp::Result<capnp::dynamic_orphan::Orphan<'m>> {
    match mode {
        0 => root.disown_named(if index == 0 { "source" } else { "target" }, token),
        1 => list(root.reborrow(), "tokens")?.disown(index, token),
        2 => list(root.reborrow(), "values")?.disown(index, token),
        3 => list(root.reborrow(), "groups")?
            .get(index)?
            .downcast::<dynamic_struct::Builder>()
            .disown_named("body", token),
        _ => unreachable!(),
    }
}
fn adopt_slot<'m>(
    root: &mut dynamic_struct::Builder<'_>,
    mode: u8,
    orphan: capnp::dynamic_orphan::Orphan<'m>,
) -> Result<(), capnp::dynamic_orphan::AdoptError<capnp::dynamic_orphan::Orphan<'m>>> {
    match mode {
        0 => root.adopt_named("target", orphan),
        1 => list(root.reborrow(), "tokens").unwrap().adopt(1, orphan),
        2 => list(root.reborrow(), "values").unwrap().adopt(1, orphan),
        3 => list(root.reborrow(), "groups")
            .unwrap()
            .get(1)
            .unwrap()
            .downcast::<dynamic_struct::Builder>()
            .adopt_named("body", orphan),
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

async fn replay(trace: &Trace, mode: u8, small: bool) -> capnp::Result<()> {
    let drops: [_; 3] = std::array::from_fn(|_| Rc::new(Cell::new(0)));
    let clients = drops.each_ref().map(client);
    let identities = clients.each_ref().map(|c| c.client.hook.get_ptr());
    let [first, second, third] = clients;
    let mut caps = Vec::new();
    let mut foreign_caps = Vec::new();
    let mut wrong_caps = Vec::new();
    let mut m = capnp::message::Builder::new(allocator(small));
    let mut foreign = capnp::message::Builder::new(allocator(small));
    let mut root = m.init_root::<orphan_case::Builder>();
    root.imbue_mut(&mut caps);
    root.reborrow().init_arm();
    match mode {
        0 => {
            root.reborrow().init_source().set_cap(first);
            root.reborrow().init_target().set_cap(second);
        }
        1 => {
            let mut l = root.reborrow().init_tokens(2);
            l.set(0, first.client.hook);
            l.set(1, second.client.hook);
        }
        2 => {
            let mut l = root.reborrow().init_values(2);
            l.reborrow().get(0).set_cap(first);
            l.get(1).set_cap(second);
        }
        3 => {
            let mut l = root.reborrow().init_groups(2);
            l.reborrow().get(0).set_sibling(11);
            l.reborrow().get(1).set_sibling(22);
            l.reborrow().get(0).init_body().set_cap(first);
            l.get(1).init_body().set_cap(second);
        }
        _ => unreachable!(),
    }
    let mut other = foreign.init_root::<orphan_case::Builder>();
    other.imbue_mut(&mut foreign_caps);
    other.reborrow().init_target().set_cap(third);
    let mut other = value::Builder::from(other).downcast::<dynamic_struct::Builder>();
    let (mut root, token) = value::Builder::from(root)
        .downcast::<dynamic_struct::Builder>()
        .with_orphanage();
    let mut orphan = None;
    let mut held: Option<harness::Client> = None;
    for step in &trace.steps {
        match step.action.as_str() {
            "take" => orphan = Some(take_slot(&mut root, mode, 0, &token)?),
            "adopt" => adopt_slot(&mut root, mode, orphan.take().unwrap()).unwrap(),
            "drop-orphan" => drop(orphan.take()),
            "wrong-arena" => {
                let error = other
                    .adopt_named("target", orphan.take().unwrap())
                    .unwrap_err();
                assert_eq!(error.error.kind, ErrorKind::WrongArena);
                orphan = Some(error.orphan);
            }
            "wrong-type" => {
                let error = root
                    .adopt_named("sentinel", orphan.take().unwrap())
                    .unwrap_err();
                assert_eq!(error.error.kind, ErrorKind::TypeMismatch);
                orphan = Some(error.orphan);
            }
            "wrong-context" => {
                let mut wrong = root.reborrow().downcast::<orphan_case::Owned>();
                wrong.imbue_mut(&mut wrong_caps);
                let mut wrong = value::Builder::from(wrong).downcast::<dynamic_struct::Builder>();
                let error = adopt_slot(&mut wrong, mode, orphan.take().unwrap()).unwrap_err();
                assert_eq!(error.error.kind, ErrorKind::WrongArena);
                orphan = Some(error.orphan);
            }
            "read" => {
                let source = slot_cap(root.reborrow_as_reader(), mode, 0)?;
                held = Some(
                    if source.as_client().is_ok() {
                        source
                    } else {
                        slot_cap(root.reborrow_as_reader(), mode, 1)?
                    }
                    .cast()?,
                );
            }
            "drop-held" => drop(held.take()),
            "clear-source" => drop(take_slot(&mut root, mode, 0, &token)?),
            "clear-dest" => drop(take_slot(&mut root, mode, 1, &token)?),
            _ => panic!("unknown trace action"),
        }
        let id = |cap: value::Capability| -> u8 {
            cap.as_client().ok().map_or(0, |c| {
                (identities
                    .iter()
                    .position(|&id| id == c.hook.get_ptr())
                    .unwrap()
                    + 1) as u8
            })
        };
        assert_eq!(
            [
                id(slot_cap(root.reborrow_as_reader(), mode, 0)?),
                id(slot_cap(root.reborrow_as_reader(), mode, 1)?),
                id(slot_cap(other.reborrow_as_reader(), 0, 1)?),
                u8::from(orphan.is_some()),
                u8::from(held.is_some()),
            ],
            step.state[..5],
            "{} mode={mode} small={small}",
            step.action
        );
        assert_eq!(
            u8::from(root.which()?.unwrap().get_proto().get_name()? == "arm"),
            step.state[12]
        );
        for (i, drop_count) in drops.iter().enumerate() {
            assert_eq!(
                u8::from(drop_count.get() == 0),
                step.state[13 + i],
                "cap {} after {} mode={mode}",
                i + 1,
                step.action
            );
        }
        if let Some(cap) = &held {
            assert_eq!(cap.client.hook.get_ptr(), identities[0]);
            assert_eq!(
                cap.echo_request().send().promise.await?.get()?.get_value(),
                123
            );
        }
        if mode == 3 {
            let l = root
                .reborrow_as_reader()
                .get_named("groups")?
                .downcast::<dynamic_list::Reader>();
            for i in 0..2 {
                assert_eq!(
                    l.get(i)?
                        .downcast::<dynamic_struct::Reader>()
                        .get_named("sibling")?
                        .downcast::<u64>(),
                    11 * (u64::from(i) + 1)
                );
            }
        }
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_dynamic_orphan_traces() -> capnp::Result<()> {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_DYNAMIC_ORPHAN_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!traces.is_empty());
    for trace in &traces {
        for mode in 0..4 {
            for small in [false, true] {
                replay(trace, mode, small).await?;
            }
        }
    }
    Ok(())
}
