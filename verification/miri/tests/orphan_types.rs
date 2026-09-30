use capnp::{
    dynamic_list, dynamic_struct, dynamic_value as value,
    introspect::Introspect,
    message::{AllocationStrategy, HeapAllocator},
    traits::{Imbue, ImbueMut, IntoInternalStructReader},
    Result,
};
use reproto_memory_checks::field_api_capnp::{native_record, opaque, person, service, PhoneKind};
use std::{cell::Cell, rc::Rc};

fn allocator(far: bool) -> HeapAllocator {
    if far {
        HeapAllocator::new()
            .first_segment_words(1)
            .allocation_strategy(AllocationStrategy::FixedSize)
    } else {
        HeapAllocator::new()
    }
}

#[test]
fn scalar_list_growth_and_shrink_clear_removed_elements_at_every_wire_width() -> Result<()> {
    for far in [false, true] {
        let mut message = capnp::message::Builder::new(allocator(far));
        let (mut root, token) = value::Builder::from(message.init_root::<person::Builder>())
            .downcast::<dynamic_struct::Builder>()
            .with_orphanage();
        let mut access = token.in_struct(&mut root)?;
        macro_rules! check {
            ($ty:ty, $initial:expr, $zero:expr) => {{
                let initial: $ty = $initial;
                let zero: $ty = $zero;
                let mut expected = vec![zero; 9];
                expected[1] = initial;
                expected[8] = initial;
                let mut orphan = access.new_list(<$ty>::introspect(), 9)?;
                access.edit(&mut orphan, |v| {
                    let mut list = v.downcast::<dynamic_list::Builder>();
                    list.set(1, initial.into())?;
                    list.set(8, initial.into())
                })?;
                for size in [17, 3, 9, 0, 1, 9] {
                    access.resize(&mut orphan, size)?;
                    expected.resize(size as usize, zero);
                    access.read(&mut orphan, |v| {
                        let list = v.downcast::<dynamic_list::Reader>();
                        assert_eq!(list.len(), size);
                        for (i, expected) in expected.iter().enumerate() {
                            assert_eq!(list.get(i as u32)?.downcast::<$ty>(), *expected);
                        }
                        Ok(())
                    })?;
                }
                assert!(access.resize(&mut orphan, u32::MAX).is_err());
            }};
        }
        check!((), (), ());
        check!(bool, true, false);
        check!(u8, u8::MAX, 0);
        check!(i8, i8::MIN, 0);
        check!(u16, u16::MAX, 0);
        check!(i16, i16::MIN, 0);
        check!(u32, u32::MAX, 0);
        check!(i32, i32::MIN, 0);
        check!(u64, u64::MAX, 0);
        check!(i64, i64::MIN, 0);
        check!(f32, -1.25, 0.0);
        check!(f64, -1.25, 0.0);
        let mut enums = access.new_list(PhoneKind::introspect(), 2)?;
        access.edit(&mut enums, |v| {
            v.downcast::<dynamic_list::Builder>()
                .set(0, PhoneKind::Work.into())
        })?;
        access.resize(&mut enums, 1)?;
        access.resize(&mut enums, 3)?;
        access.read(&mut enums, |v| {
            let list = v.downcast::<dynamic_list::Reader>();
            for (index, expected) in [2, 0, 0].into_iter().enumerate() {
                let value::Reader::Enum(actual) = list.get(index as u32)? else {
                    panic!("expected enum")
                };
                assert_eq!(actual.get_value(), expected);
            }
            Ok(())
        })?;
    }
    Ok(())
}

#[test]
fn pointer_list_children_and_text_terminators_survive_resize() -> Result<()> {
    for far in [false, true] {
        let mut message = capnp::message::Builder::new(allocator(far));
        let (mut root, token) = value::Builder::from(message.init_root::<person::Builder>())
            .downcast::<dynamic_struct::Builder>()
            .with_orphanage();
        let mut access = token.in_struct(&mut root)?;
        let mut text = access.null(capnp::text::Owned::introspect())?;
        access.resize(&mut text, 9)?;
        access.edit(&mut text, |v| {
            v.downcast::<capnp::text::Builder>()
                .as_bytes_mut()
                .copy_from_slice(b"abcdefghi");
            Ok(())
        })?;
        let mut typed = text.release_as::<capnp::text::Owned>().unwrap();
        for (length, expected) in [(3, &b"abc"[..]), (8, &b"abc\0\0\0\0\0"[..]), (0, &b""[..])] {
            access.resize_typed(&mut typed, length)?;
            access.read_typed(&mut typed, |text| {
                assert_eq!(text.as_bytes(), expected);
                Ok(())
            })?;
        }
        let mut source = capnp::message::Builder::new(allocator(far));
        {
            let mut outer =
                source.initn_root::<capnp::list_list::Builder<capnp::text_list::Owned>>(2);
            outer.reborrow().init(0, 2).set(1, "kept");
            outer.init(1, 1).set(0, "removed");
        }
        let mut nested = access.copy(value::Reader::from(
            source.get_root_as_reader::<capnp::list_list::Reader<capnp::text_list::Owned>>()?,
        ))?;
        drop(source);
        let address = access.read(&mut nested, |v| {
            Ok(v.downcast::<dynamic_list::Reader>()
                .get(0)?
                .downcast::<dynamic_list::Reader>()
                .get(1)?
                .downcast::<capnp::text::Reader>()
                .as_bytes()
                .as_ptr())
        })?;
        access.resize(&mut nested, 1)?;
        access.resize(&mut nested, 3)?;
        access.read(&mut nested, |v| {
            let outer = v.downcast::<dynamic_list::Reader>();
            let kept = outer
                .get(0)?
                .downcast::<dynamic_list::Reader>()
                .get(1)?
                .downcast::<capnp::text::Reader>();
            assert_eq!(kept, "kept");
            assert_eq!(kept.as_bytes().as_ptr(), address);
            assert!(outer.get(1)?.downcast::<dynamic_list::Reader>().is_empty());
            assert!(outer.get(2)?.downcast::<dynamic_list::Reader>().is_empty());
            Ok(())
        })?;
        access.resize(&mut nested, 0)?;
    }
    Ok(())
}

struct Server(Rc<Cell<usize>>);
impl service::Server for Server {}
impl Drop for Server {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn capability_list_resize_releases_slots_but_preserves_extracted_clients() -> Result<()> {
    use capnp::capability::FromClientHook;
    type CapList = capnp::capability_list::Owned<service::Client>;
    for far in [false, true] {
        let kept = Rc::new(Cell::new(0));
        let removed = Rc::new(Cell::new(0));
        let mut source = capnp::message::Builder::new(allocator(far));
        let mut source_caps = Vec::new();
        {
            let mut pointer = source.init_root::<capnp::any_pointer::Builder>();
            pointer.imbue_mut(&mut source_caps);
            let mut list = pointer.initn_as::<capnp::capability_list::Builder<service::Client>>(2);
            for (index, drops) in [&kept, &removed].into_iter().enumerate() {
                let client: service::Client = capnp_rpc::new_client(Server(drops.clone()));
                list.set(index as u32, client.into_client_hook());
            }
        }
        let mut message = capnp::message::Builder::new(allocator(far));
        let mut caps = Vec::new();
        let mut raw = message.init_root::<person::Builder>();
        raw.imbue_mut(&mut caps);
        let (mut root, token) = value::Builder::from(raw)
            .downcast::<dynamic_struct::Builder>()
            .with_orphanage();
        let mut access = token.in_struct(&mut root)?;
        let orphan = {
            let mut pointer = source.get_root_as_reader::<capnp::any_pointer::Reader>()?;
            pointer.imbue(&source_caps);
            access.copy(value::Reader::from(
                pointer.get_as::<capnp::capability_list::Reader<service::Client>>()?,
            ))?
        };
        drop(source);
        drop(source_caps);
        let mut orphan = orphan.release_as::<CapList>().unwrap();
        let held = access.read_typed(&mut orphan, |list| list.get(0))?;
        access.resize_typed(&mut orphan, 1)?;
        assert_eq!(removed.get(), 1);
        access.resize_typed(&mut orphan, 4)?;
        access.read_typed(&mut orphan, |list| {
            assert_eq!(list.len(), 4);
            assert_eq!(
                list.get(0)?.client.hook.get_ptr(),
                held.client.hook.get_ptr()
            );
            for index in 1..4 {
                assert!(list.get(index).is_err());
            }
            Ok(())
        })?;
        access.resize_typed(&mut orphan, 0)?;
        drop(orphan);
        assert_eq!(kept.get(), 0);
        drop(held);
        assert_eq!(kept.get(), 1);
    }
    Ok(())
}

#[test]
fn unknown_struct_fields_retain_children_and_release_truncated_capabilities() -> Result<()> {
    for far in [false, true] {
        let kept = Rc::new(Cell::new(0));
        let removed = Rc::new(Cell::new(0));
        let mut source = capnp::message::Builder::new(allocator(far));
        let mut caps = Vec::new();
        {
            let mut pointer = source.init_root::<capnp::any_pointer::Builder>();
            pointer.imbue_mut(&mut caps);
            let mut list = pointer.initn_as::<capnp::struct_list::Builder<native_record::Owned>>(2);
            list.reborrow()
                .get(0)
                .set_cap(capnp_rpc::new_client(Server(kept.clone())));
            list.reborrow().get(0).set_label("unknown child");
            list.get(1)
                .set_cap(capnp_rpc::new_client(Server(removed.clone())));
        }
        let mut message = capnp::message::Builder::new(allocator(far));
        let mut destination_caps = Vec::new();
        let mut raw = message.init_root::<person::Builder>();
        raw.imbue_mut(&mut destination_caps);
        let (mut root, token) = value::Builder::from(raw)
            .downcast::<dynamic_struct::Builder>()
            .with_orphanage();
        let mut access = token.in_struct(&mut root)?;
        let mut orphan = {
            let mut pointer = source.get_root_as_reader::<capnp::any_pointer::Reader>()?;
            pointer.imbue(&caps);
            access.copy(value::Reader::from(
                pointer.get_as::<capnp::struct_list::Reader<opaque::Owned>>()?,
            ))?
        };
        drop(source);
        drop(caps);
        let address = access.read(&mut orphan, |v| {
            let raw = v
                .downcast::<dynamic_list::Reader>()
                .get(0)?
                .downcast_struct::<opaque::Owned>()
                .into_internal_struct_reader();
            let record: native_record::Reader = raw.into();
            Ok(record.get_label()?.as_bytes().as_ptr())
        })?;
        access.resize(&mut orphan, 1)?;
        assert_eq!(removed.get(), 1);
        assert_eq!(kept.get(), 0);
        access.resize(&mut orphan, 3)?;
        access.read(&mut orphan, |v| {
            let list = v.downcast::<dynamic_list::Reader>();
            for index in 0..3 {
                let raw = list
                    .get(index)?
                    .downcast_struct::<opaque::Owned>()
                    .into_internal_struct_reader();
                let record: native_record::Reader = raw.into();
                if index == 0 {
                    assert_eq!(record.get_label()?, "unknown child");
                    assert_eq!(record.get_label()?.as_bytes().as_ptr(), address);
                    let _ = record.get_cap()?;
                } else {
                    assert!(!record.has_label());
                    assert!(!record.has_cap());
                }
            }
            Ok(())
        })?;
        drop(orphan);
        assert_eq!(kept.get(), 1);
    }
    Ok(())
}
