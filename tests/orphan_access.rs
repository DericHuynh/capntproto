use capnp::{
    dynamic_struct, dynamic_value as value, introspect::Introspect, traits::ImbueMut, ErrorKind,
};
use reproto_test_support::{
    dynamic_test_capnp::{orphan_case, orphan_payload, parcel},
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
fn cap(drops: &Rc<Cell<usize>>) -> harness::Client {
    capnp_rpc::new_client(Service(drops.clone()))
}
fn allocator(small: bool) -> capnp::message::HeapAllocator {
    let allocator = capnp::message::HeapAllocator::new();
    if small {
        allocator
            .first_segment_words(1)
            .allocation_strategy(capnp::message::AllocationStrategy::FixedSize)
    } else {
        allocator
    }
}
#[tokio::test(flavor = "current_thread")]
async fn independent_allocation_edit_read_release_and_adoption_preserve_capabilities(
) -> capnp::Result<()> {
    for small in [false, true] {
        let drops = Rc::new(Cell::new(0));
        let replaced = Rc::new(Cell::new(0));
        let mut caps = vec![];
        let mut message = capnp::message::Builder::new(allocator(small));
        let mut root = message.init_root::<orphan_case::Builder>();
        root.imbue_mut(&mut caps);
        root.reborrow().init_target().set_cap(cap(&replaced));
        let (mut root, token) = value::Builder::from(root)
            .downcast::<dynamic_struct::Builder>()
            .with_orphanage();
        let mut orphan = token
            .in_struct(&mut root)?
            .new_struct(orphan_payload::Owned::introspect().as_struct_schema()?)?;
        let mut address = std::ptr::null();
        token.in_struct(&mut root)?.edit(&mut orphan, |v| {
            let mut v = v.downcast_struct::<orphan_payload::Owned>();
            assert_eq!(v.reborrow().get_number(), 42);
            v.set_number(99);
            v.set_text("detached");
            v.set_cap(cap(&drops));
            address = v.reborrow().get_text()?.into_reader().0.as_ptr();
            Ok(())
        })?;
        let held = token.in_struct(&mut root)?.read(&mut orphan, |v| {
            let v = v.downcast_struct::<orphan_payload::Owned>();
            assert_eq!(v.get_number(), 99);
            assert_eq!(v.get_text()?.0.as_ptr(), address);
            v.get_cap()
        })?;
        let error = orphan
            .release_as::<parcel::Owned<capnp::text::Owned>>()
            .err()
            .unwrap();
        assert_eq!(error.error.kind, ErrorKind::TypeMismatch);
        let mut typed = error.orphan.release_as::<orphan_payload::Owned>().unwrap();
        token
            .in_struct(&mut root)?
            .edit_typed(&mut typed, |mut v| {
                v.set_number(100);
                Ok(())
            })?;
        assert_eq!(
            token
                .in_struct(&mut root)?
                .read_typed(&mut typed, |v| Ok(v.get_number()))?,
            100
        );
        root.adopt_named("target", typed.into_dynamic()).unwrap();
        assert_eq!(replaced.get(), 1);
        assert_eq!(
            root.reborrow_as_reader()
                .get_named("target")?
                .downcast_struct::<orphan_payload::Owned>()
                .get_text()?
                .0
                .as_ptr(),
            address
        );
        let orphan = root.disown_named("target", &token)?;
        drop(orphan);
        assert_eq!(drops.get(), 0);
        assert_eq!(
            held.echo_request().send().promise.await?.get()?.get_value(),
            73
        );
        drop(held);
        assert_eq!(drops.get(), 1);
    }
    Ok(())
}
#[test]
fn returned_errors_and_unwinding_keep_new_and_replaced_capability_ownership() -> capnp::Result<()> {
    for panic in [false, true] {
        let old = Rc::new(Cell::new(0));
        let new = Rc::new(Cell::new(0));
        let ambient = Rc::new(Cell::new(0));
        let mut caps = vec![];
        let mut message = capnp::message::Builder::new_default();
        let mut root = message.init_root::<orphan_case::Builder>();
        root.imbue_mut(&mut caps);
        root.set_token(cap(&ambient));
        root.reborrow().init_source().set_cap(cap(&old));
        let (mut root, token) = value::Builder::from(root)
            .downcast::<dynamic_struct::Builder>()
            .with_orphanage();
        let mut orphan = root.disown_named("source", &token)?;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            token
                .in_struct(&mut root)?
                .edit(&mut orphan, |v| -> capnp::Result<()> {
                    let mut v = v.downcast_struct::<orphan_payload::Owned>();
                    v.set_cap(cap(&new));
                    if panic {
                        panic!("application editor panic");
                    }
                    Err(capnp::Error::failed("application editor error".into()))
                })
        }));
        assert_eq!(result.is_err(), panic);
        if !panic {
            assert!(result.unwrap().is_err());
        }
        assert_eq!(old.get(), 1);
        assert_eq!(new.get(), 0);
        assert_eq!(ambient.get(), 0);
        token.in_struct(&mut root)?.read(&mut orphan, |v| {
            v.downcast_struct::<orphan_payload::Owned>().get_cap()?;
            Ok(())
        })?;
        drop(orphan);
        assert_eq!(new.get(), 1);
        assert_eq!(ambient.get(), 0);
        root.clear(root.get_schema().get_field_by_name("token")?)?;
        assert_eq!(ambient.get(), 1);
    }
    Ok(())
}
#[test]
fn null_materialization_scalars_blobs_lists_and_copy_are_independent() -> capnp::Result<()> {
    for small in [false, true] {
        let mut message = capnp::message::Builder::new(allocator(small));
        let root = message.init_root::<orphan_case::Builder>();
        let (mut root, token) = value::Builder::from(root)
            .downcast::<dynamic_struct::Builder>()
            .with_orphanage();
        let mut access = token.in_struct(&mut root)?;
        let mut nil = access.null(orphan_payload::Owned::introspect())?;
        assert!(access.is_null(&mut nil)?);
        assert_eq!(
            access.read(&mut nil, |v| Ok(v
                .downcast_struct::<orphan_payload::Owned>()
                .get_number()))?,
            42
        );
        assert!(access.is_null(&mut nil)?);
        access.edit(&mut nil, |v| {
            v.downcast_struct::<orphan_payload::Owned>().set_number(123);
            Ok(())
        })?;
        assert!(!access.is_null(&mut nil)?);
        let mut data = access.new_data(3)?;
        access.edit(&mut data, |v| {
            v.downcast::<capnp::data::Builder>().copy_from_slice(b"abc");
            Ok(())
        })?;
        assert_eq!(
            access.read(&mut data, |v| Ok(v
                .downcast::<capnp::data::Reader>()
                .to_vec()))?,
            b"abc"
        );
        let mut text = access.new_text(3)?;
        access.edit(&mut text, |v| {
            v.downcast::<capnp::text::Builder>()
                .as_bytes_mut()
                .copy_from_slice(b"xyz");
            Ok(())
        })?;
        assert_eq!(
            access.read(&mut text, |v| Ok(v
                .downcast::<capnp::text::Reader>()
                .to_str()?
                .to_owned()))?,
            "xyz"
        );
        let mut numbers = access.new_list(u32::introspect(), 2)?;
        access.edit(&mut numbers, |v| {
            v.downcast::<capnp::dynamic_list::Builder>()
                .set(1, 99u32.into())
        })?;
        assert_eq!(
            access.read(&mut numbers, |v| Ok(v
                .downcast::<capnp::dynamic_list::Reader>()
                .get(1)?
                .downcast::<u32>()))?,
            99
        );
        assert!(access.new_list(u32::introspect(), 1 << 29).is_err());
        assert!(access.new_text(u32::MAX).is_err());
        let mut scalar = access.copy(13u32.into())?;
        access.edit(&mut scalar, |mut v| {
            assert_eq!(v.reborrow().downcast::<u32>(), 13);
            v = value::Builder::UInt32(99);
            assert_eq!(v.downcast::<u32>(), 99);
            Ok(())
        })?;
        assert_eq!(access.read(&mut scalar, |v| Ok(v.downcast::<u32>()))?, 13);
        let mut source = capnp::message::Builder::new_default();
        let mut original = source.init_root::<orphan_payload::Builder>();
        original.set_text("original");
        let mut copied = access.copy(value::Reader::from(original.reborrow_as_reader()))?;
        original.set_text("changed");
        assert_eq!(
            access.read(&mut copied, |v| Ok(v
                .downcast_struct::<orphan_payload::Owned>()
                .get_text()?
                .to_str()?
                .to_owned()))?,
            "original"
        );
        root.adopt_named("source", nil).unwrap();
        root.adopt_named("target", copied).unwrap();
        root.adopt_named("description", text).unwrap();
        root.adopt_named("numbers", numbers).unwrap();
    }
    Ok(())
}
#[test]
fn private_view_context_rejects_outer_orphan_adoption_without_consuming_it() -> capnp::Result<()> {
    let drops = Rc::new(Cell::new(0));
    let mut caps = vec![];
    let mut message = capnp::message::Builder::new_default();
    let mut root = message.init_root::<orphan_case::Builder>();
    root.imbue_mut(&mut caps);
    root.set_token(cap(&drops));
    let (mut root, token) = value::Builder::from(root)
        .downcast::<dynamic_struct::Builder>()
        .with_orphanage();
    let outer = root.disown_named("token", &token)?;
    let mut detached = token
        .in_struct(&mut root)?
        .new_struct(orphan_payload::Owned::introspect().as_struct_schema()?)?;
    let outer = token.in_struct(&mut root)?.edit(&mut detached, |v| {
        let failed = v
            .downcast::<dynamic_struct::Builder>()
            .adopt_named("cap", outer)
            .unwrap_err();
        assert_eq!(failed.error.kind, ErrorKind::WrongArena);
        Ok(failed.orphan)
    })?;
    assert_eq!(drops.get(), 0);
    root.adopt_named("token", outer).unwrap();
    drop(detached);
    assert_eq!(drops.get(), 0);
    root.clear(root.get_schema().get_field_by_name("token")?)?;
    assert_eq!(drops.get(), 1);
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn foreign_copies_and_interleaved_edits_keep_capability_indices_independent(
) -> capnp::Result<()> {
    for small in [false, true] {
        let drops = Rc::new(Cell::new(0));
        let other_drops = Rc::new(Cell::new(0));
        let mut caps = vec![];
        let mut message = capnp::message::Builder::new(allocator(small));
        let mut root = message.init_root::<orphan_case::Builder>();
        root.imbue_mut(&mut caps);
        let (mut root, token) = value::Builder::from(root)
            .downcast::<dynamic_struct::Builder>()
            .with_orphanage();
        let (mut copied, original_address) = {
            let mut source_caps = vec![];
            let mut source = capnp::message::Builder::new(allocator(small));
            let mut original = source.init_root::<orphan_payload::Builder>();
            original.imbue_mut(&mut source_caps);
            original.set_text("independent");
            original.set_cap(cap(&drops));
            let address = original.reborrow_as_reader().get_text()?.0.as_ptr();
            let copied = token
                .in_struct(&mut root)?
                .copy(original.into_reader().into())?;
            (copied, address)
        };
        assert_eq!(drops.get(), 0);
        let mut second = token
            .in_struct(&mut root)?
            .new_struct(orphan_payload::Owned::introspect().as_struct_schema()?)?;
        token.in_struct(&mut root)?.edit(&mut second, |v| {
            v.downcast_struct::<orphan_payload::Owned>()
                .set_cap(cap(&other_drops));
            Ok(())
        })?;
        let held = token.in_struct(&mut root)?.read(&mut copied, |v| {
            let v = v.downcast_struct::<orphan_payload::Owned>();
            assert_eq!(v.get_text()?, "independent");
            assert_ne!(v.get_text()?.0.as_ptr(), original_address);
            v.get_cap()
        })?;
        token.in_struct(&mut root)?.edit(&mut copied, |v| {
            v.downcast_struct::<orphan_payload::Owned>()
                .set_cap(held.clone());
            Ok(())
        })?;
        root.adopt_named("source", copied).unwrap();
        root.adopt_named("target", second).unwrap();
        let first_id = held.client.hook.get_ptr();
        let first = root
            .reborrow_as_reader()
            .get_named("source")?
            .downcast_struct::<orphan_payload::Owned>()
            .get_cap()?;
        let second = root
            .reborrow_as_reader()
            .get_named("target")?
            .downcast_struct::<orphan_payload::Owned>()
            .get_cap()?;
        assert_eq!(first.client.hook.get_ptr(), first_id);
        assert_ne!(second.client.hook.get_ptr(), first_id);
        assert_eq!(
            second
                .echo_request()
                .send()
                .promise
                .await?
                .get()?
                .get_value(),
            73
        );
        drop(first);
        drop(second);
        drop(root.disown_named("source", &token)?);
        drop(root.disown_named("target", &token)?);
        assert_eq!(other_drops.get(), 1);
        assert_eq!(drops.get(), 0);
        assert_eq!(
            held.echo_request().send().promise.await?.get()?.get_value(),
            73
        );
        drop(held);
        assert_eq!(drops.get(), 1);
    }
    Ok(())
}

#[test]
fn empty_list_anchor_and_typed_release_check_generic_brands() -> capnp::Result<()> {
    let mut message = capnp::message::Builder::new_default();
    let root = message.initn_root::<capnp::primitive_list::Builder<()>>(0);
    let (mut root, token) = value::Builder::from(root)
        .downcast::<capnp::dynamic_list::Builder>()
        .with_orphanage();
    let mut access = token.in_list(&mut root)?;
    let orphan =
        access.new_struct(parcel::Owned::<capnp::text::Owned>::introspect().as_struct_schema()?)?;
    let error = orphan
        .release_as::<parcel::Owned<capnp::data::Owned>>()
        .err()
        .unwrap();
    assert_eq!(error.error.kind, ErrorKind::TypeMismatch);
    let mut typed = error
        .orphan
        .release_as::<parcel::Owned<capnp::text::Owned>>()
        .unwrap();
    access.edit_typed(&mut typed, |mut v| v.set_value("brand checked"))?;
    assert_eq!(
        access.read_typed(&mut typed, |v| Ok(v.get_value()?.to_str()?.to_owned()))?,
        "brand checked"
    );
    Ok(())
}

#[test]
fn resize_preserves_unknown_fields_child_addresses_and_drops_removed_capabilities(
) -> capnp::Result<()> {
    use reproto_test_support::dynamic_test_capnp::{large_orphan_case, small_orphan_case};
    for small in [false, true] {
        let kept = Rc::new(Cell::new(0));
        let removed = Rc::new(Cell::new(0));
        let mut caps = vec![];
        let mut message = capnp::message::Builder::new(allocator(small));
        let mut root = message.init_root::<large_orphan_case::Builder>();
        root.imbue_mut(&mut caps);
        let mut list = root.reborrow().init_targets(2);
        list.reborrow().get(0).set_cap(cap(&kept));
        list.reborrow().get(0).set_text("unknown child");
        let address = list.reborrow().get(0).get_text()?.into_reader().0.as_ptr();
        list.get(1).set_cap(cap(&removed));
        let mut root = message.get_root::<small_orphan_case::Builder>()?;
        root.imbue_mut(&mut caps);
        let (mut root, token) = value::Builder::from(root)
            .downcast::<dynamic_struct::Builder>()
            .with_orphanage();
        let mut orphan = root.disown_named("targets", &token)?;
        token.in_struct(&mut root)?.resize(&mut orphan, 1)?;
        assert_eq!(removed.get(), 1);
        assert_eq!(kept.get(), 0);
        token.in_struct(&mut root)?.resize(&mut orphan, 3)?;
        root.adopt_named("targets", orphan).unwrap();
        let mut root = message.get_root::<large_orphan_case::Builder>()?;
        root.imbue_mut(&mut caps);
        let mut list = root.get_targets()?;
        assert_eq!(list.len(), 3);
        assert_eq!(
            list.reborrow().into_reader().get(0).get_text()?.0.as_ptr(),
            address
        );
        assert_eq!(list.reborrow().into_reader().get(1).get_number(), 42);
        assert!(!list.reborrow().into_reader().get(1).has_cap());
        let list = value::Builder::from(list).downcast::<capnp::dynamic_list::Builder>();
        let (mut list, token) = list.with_orphanage();
        let mut orphan = list.disown(0, &token)?;
        assert_eq!(
            token
                .in_list(&mut list)?
                .resize(&mut orphan, 0)
                .err()
                .unwrap()
                .kind,
            ErrorKind::TypeMismatch
        );
        drop(orphan);
        assert_eq!(kept.get(), 1);
    }
    Ok(())
}
#[test]
fn resize_scalars_bits_pointer_lists_nulls_and_text() -> capnp::Result<()> {
    for small in [false, true] {
        let mut message = capnp::message::Builder::new(allocator(small));
        let root = message.init_root::<orphan_case::Builder>();
        let (mut root, token) = value::Builder::from(root)
            .downcast::<dynamic_struct::Builder>()
            .with_orphanage();
        let mut access = token.in_struct(&mut root)?;
        for (ty, initial) in [
            (bool::introspect(), value::Reader::Bool(true)),
            (u8::introspect(), value::Reader::UInt8(123)),
            (u16::introspect(), value::Reader::UInt16(123)),
            (u32::introspect(), value::Reader::UInt32(123)),
            (u64::introspect(), value::Reader::UInt64(123)),
            (
                capnp::text::Owned::introspect(),
                value::Reader::Text("child".into()),
            ),
        ] {
            let mut list = access.new_list(ty, 9)?;
            access.edit(&mut list, |v| {
                v.downcast::<capnp::dynamic_list::Builder>()
                    .set(1, initial.clone())
            })?;
            for count in [17, 2, 9] {
                access.resize(&mut list, count)?;
                access.read(&mut list, |v| {
                    let l = v.downcast::<capnp::dynamic_list::Reader>();
                    assert_eq!(l.len(), count);
                    assert_eq!(format!("{:?}", l.get(1)?), format!("{initial:?}"));
                    Ok(())
                })?;
            }
            access.resize(&mut list, 0)?;
            assert_eq!(
                access.read(&mut list, |v| Ok(v
                    .downcast::<capnp::dynamic_list::Reader>()
                    .len()))?,
                0
            );
        }
        for (ty, text) in [
            (capnp::text::Owned::introspect(), true),
            (capnp::data::Owned::introspect(), false),
        ] {
            let mut blob = access.null(ty)?;
            access.resize(&mut blob, 3)?;
            access.edit(&mut blob, |v| {
                if text {
                    v.downcast::<capnp::text::Builder>()
                        .as_bytes_mut()
                        .copy_from_slice(b"abc");
                } else {
                    v.downcast::<capnp::data::Builder>().copy_from_slice(b"abc");
                }
                Ok(())
            })?;
            access.resize(&mut blob, 2)?;
            access.resize(&mut blob, 4)?;
            access.read(&mut blob, |v| {
                let bytes = if text {
                    v.downcast::<capnp::text::Reader>().0
                } else {
                    v.downcast::<capnp::data::Reader>()
                };
                assert_eq!(bytes, b"ab\0\0");
                Ok(())
            })?;
            assert!(access.resize(&mut blob, u32::MAX).is_err());
        }
        let mut empty = access.null(capnp::primitive_list::Owned::<u32>::introspect())?;
        access.resize(&mut empty, 2)?;
        assert_eq!(
            access.read(&mut empty, |v| Ok(v
                .downcast::<capnp::dynamic_list::Reader>()
                .get(1)?
                .downcast::<u32>()))?,
            0
        );
    }
    Ok(())
}

type ListOwned = capnp::struct_list::Owned<orphan_payload::Owned>;
enum HeldOrphan<'a> {
    Dynamic(capnp::dynamic_orphan::Orphan<'a>),
    Typed(capnp::dynamic_orphan::TypedOrphan<'a, ListOwned>),
}
impl<'a> HeldOrphan<'a> {
    fn into_dynamic(self) -> capnp::dynamic_orphan::Orphan<'a> {
        match self {
            Self::Dynamic(o) => o,
            Self::Typed(o) => o.into_dynamic(),
        }
    }
    fn read<R>(
        &mut self,
        a: &mut capnp::dynamic_orphan::Access<'_, '_>,
        f: impl for<'b> FnOnce(capnp::dynamic_list::Reader<'b>) -> capnp::Result<R>,
    ) -> capnp::Result<R> {
        match self {
            Self::Dynamic(o) => a.read(o, |v| f(v.downcast())),
            Self::Typed(o) => a.read_typed(o, |v| f(value::Reader::from(v).downcast())),
        }
    }
    fn edit<R>(
        &mut self,
        a: &mut capnp::dynamic_orphan::Access<'_, '_>,
        f: impl for<'b> FnOnce(capnp::dynamic_list::Builder<'b>) -> capnp::Result<R>,
    ) -> capnp::Result<R> {
        match self {
            Self::Dynamic(o) => a.edit(o, |v| f(v.downcast())),
            Self::Typed(o) => a.edit_typed(o, |v| f(value::Builder::from(v).downcast())),
        }
    }
    fn resize(
        &mut self,
        a: &mut capnp::dynamic_orphan::Access<'_, '_>,
        size: u32,
    ) -> capnp::Result<()> {
        match self {
            Self::Dynamic(o) => a.resize(o, size),
            Self::Typed(o) => a.resize_typed(o, size),
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_orphan_access_traces() -> capnp::Result<()> {
    let path = reproto_test_support::verification::input("REPROTO_ORPHAN_ACCESS_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    // Expected editor panics are caught below; avoid thousands of diagnostic
    // lines, restoring the process hook before this conditional test finishes.
    type PanicHook = Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Send + Sync>;
    struct Hook(Option<PanicHook>);
    impl Drop for Hook {
        fn drop(&mut self) {
            std::panic::set_hook(self.0.take().unwrap());
        }
    }
    let _hook = Hook(Some(std::panic::take_hook()));
    std::panic::set_hook(Box::new(|_| {}));
    for small in [false, true] {
        for panic in [false, true] {
            for (case_index, case) in cases.iter().enumerate() {
                let drops = [
                    Rc::new(Cell::new(0)),
                    Rc::new(Cell::new(0)),
                    Rc::new(Cell::new(0)),
                ];
                let ambient = Rc::new(Cell::new(0));
                let mut caps = vec![];
                let mut message = capnp::message::Builder::new(allocator(small));
                let mut root = message.init_root::<orphan_case::Builder>();
                root.imbue_mut(&mut caps);
                let ambient_cap = cap(&ambient);
                let ambient_id = ambient_cap.client.hook.get_ptr();
                root.set_token(ambient_cap);
                let (mut root, token) = value::Builder::from(root)
                    .downcast::<dynamic_struct::Builder>()
                    .with_orphanage();
                let mut other_message = capnp::message::Builder::new_default();
                let other = other_message.init_root::<orphan_case::Builder>();
                let (mut other, other_token) = value::Builder::from(other)
                    .downcast::<dynamic_struct::Builder>()
                    .with_orphanage();
                let mut orphan: Option<HeldOrphan<'_>> = None;
                let mut held: Option<harness::Client> = None;
                let mut ids = [0; 3];
                let mut address = std::ptr::null();
                for step in case["steps"].as_array().unwrap() {
                    let s: Vec<usize> = step["state"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|n| n.as_u64().unwrap() as usize)
                        .collect();
                    match step["action"].as_str().unwrap() {
                        "allocate" => {
                            let mut o = token
                                .in_struct(&mut root)?
                                .new_list(orphan_payload::Owned::introspect(), 2)?;
                            token.in_struct(&mut root)?.edit(&mut o, |v| {
                                let mut list = v.downcast::<capnp::dynamic_list::Builder>();
                                for i in 0..2 {
                                    let c = cap(&drops[i]);
                                    ids[i] = c.client.hook.get_ptr();
                                    let mut v = list
                                        .reborrow()
                                        .get(i as u32)?
                                        .downcast_struct::<orphan_payload::Owned>();
                                    v.set_cap(c);
                                    v.set_text("child");
                                }
                                address = list
                                    .into_reader()
                                    .get(0)?
                                    .downcast_struct::<orphan_payload::Owned>()
                                    .get_text()?
                                    .0
                                    .as_ptr();
                                Ok(())
                            })?;
                            orphan = Some(HeldOrphan::Dynamic(o));
                        }
                        "edit" => {
                            let c = cap(&drops[2]);
                            ids[2] = c.client.hook.get_ptr();
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    orphan.as_mut().unwrap().edit(
                                        &mut token.in_struct(&mut root)?,
                                        |list| -> capnp::Result<()> {
                                            list.get(0)?
                                                .downcast_struct::<orphan_payload::Owned>()
                                                .set_cap(c);
                                            if panic {
                                                panic!("editor panic");
                                            }
                                            Err(capnp::Error::failed("editor error".into()))
                                        },
                                    )
                                }));
                            assert_eq!(result.is_err(), panic);
                            if !panic {
                                assert!(result.unwrap().is_err());
                            }
                        }
                        "read" => {
                            held = Some(if let Some(o) = orphan.as_mut() {
                                o.read(&mut token.in_struct(&mut root)?, |l| {
                                    l.get(0)?
                                        .downcast_struct::<orphan_payload::Owned>()
                                        .get_cap()
                                })?
                            } else {
                                root.reborrow_as_reader()
                                    .get_named("values")?
                                    .downcast::<capnp::dynamic_list::Reader>()
                                    .get(0)?
                                    .downcast_struct::<orphan_payload::Owned>()
                                    .get_cap()?
                            });
                        }
                        "shrink" => orphan
                            .as_mut()
                            .unwrap()
                            .resize(&mut token.in_struct(&mut root)?, 1)?,
                        "grow" => orphan
                            .as_mut()
                            .unwrap()
                            .resize(&mut token.in_struct(&mut root)?, 3)?,
                        "typed" => {
                            orphan = Some(HeldOrphan::Typed(
                                orphan
                                    .take()
                                    .unwrap()
                                    .into_dynamic()
                                    .release_as::<ListOwned>()
                                    .unwrap(),
                            ));
                        }
                        "bad-type" => {
                            let failed = orphan
                                .take()
                                .unwrap()
                                .into_dynamic()
                                .release_as::<capnp::text_list::Owned>()
                                .err()
                                .unwrap();
                            assert_eq!(failed.error.kind, ErrorKind::TypeMismatch);
                            orphan = Some(if s[9] == 1 {
                                HeldOrphan::Typed(failed.orphan.release_as::<ListOwned>().unwrap())
                            } else {
                                HeldOrphan::Dynamic(failed.orphan)
                            });
                        }
                        "bad-arena" => {
                            let err = orphan
                                .as_mut()
                                .unwrap()
                                .read(&mut other_token.in_struct(&mut other)?, |_| Ok(()))
                                .err()
                                .unwrap();
                            assert_eq!(err.kind, ErrorKind::WrongArena);
                        }
                        "adopt" => root
                            .adopt_named("values", orphan.take().unwrap().into_dynamic())
                            .unwrap(),
                        "drop" => {
                            if let Some(o) = orphan.take() {
                                drop(o);
                            } else {
                                root.clear(root.get_schema().get_field_by_name("values")?)?;
                            }
                        }
                        "release" => {
                            held.take();
                        }
                        other => panic!("unknown step {other}"),
                    }
                    assert_eq!(orphan.is_some(), s[0] == 1, "trace {case_index}: {step}");
                    assert_eq!(root.has_named("values")?, s[0] == 2);
                    let observe = |list: capnp::dynamic_list::Reader<'_>| -> capnp::Result<()> {
                        assert_eq!(list.len() as usize, s[1]);
                        let first = list.get(0)?.downcast_struct::<orphan_payload::Owned>();
                        assert_eq!(first.get_cap()?.client.hook.get_ptr(), ids[s[2] - 1]);
                        assert_eq!(first.get_text()?.0.as_ptr(), address);
                        if s[1] >= 2 {
                            let second = list.get(1)?.downcast_struct::<orphan_payload::Owned>();
                            assert_eq!(second.has_cap(), s[3] == 2);
                            if s[3] == 2 {
                                assert_eq!(second.get_cap()?.client.hook.get_ptr(), ids[1]);
                            }
                        }
                        Ok(())
                    };
                    if let Some(o) = orphan.as_mut() {
                        o.read(&mut token.in_struct(&mut root)?, observe)?;
                    } else if s[0] == 2 {
                        observe(root.reborrow_as_reader().get_named("values")?.downcast())?;
                    }
                    assert_eq!(held.is_some(), s[4] != 0);
                    if let Some(cap) = &held {
                        assert_eq!(cap.client.hook.get_ptr(), ids[s[4] - 1]);
                        assert_eq!(
                            cap.echo_request().send().promise.await?.get()?.get_value(),
                            73
                        );
                    }
                    for i in 0..3 {
                        let created = if i == 2 { s[5] } else { usize::from(s[0] != 0) };
                        assert_eq!(
                            drops[i].get(),
                            created - s[14 + i],
                            "cap {i}, trace {case_index}: {step}"
                        );
                    }
                    assert_eq!(ambient.get(), 0);
                    assert_eq!(
                        root.reborrow_as_reader()
                            .get_named("token")?
                            .downcast::<value::Capability>()
                            .cast::<harness::Client>()?
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
