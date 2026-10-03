use capnp::{
    dynamic_list, dynamic_struct, dynamic_value as value,
    introspect::Introspect,
    traits::{Imbue, ImbueMut, IntoInternalStructReader},
    ErrorKind,
};
use capntproto_test_support::{
    dynamic_test_capnp::{orphan_case, orphan_payload, parcel, small_orphan},
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
fn allocator(small: bool) -> capnp::message::HeapAllocator {
    let a = capnp::message::HeapAllocator::new();
    if small {
        a.first_segment_words(1)
            .allocation_strategy(capnp::message::AllocationStrategy::FixedSize)
    } else {
        a
    }
}
struct Input {
    message: capnp::message::Builder<capnp::message::HeapAllocator>,
    caps: capnp::private::layout::CapTable,
}
impl Input {
    fn new(number: u32, cap: harness::Client, small: bool) -> Self {
        let mut this = Self {
            message: capnp::message::Builder::new(allocator(small)),
            caps: vec![],
        };
        let mut pointer = this.message.init_root::<capnp::any_pointer::Builder>();
        pointer.imbue_mut(&mut this.caps);
        let list = pointer.initn_as::<capnp::struct_list::Builder<orphan_payload::Owned>>(1);
        let mut value = list.get(0);
        value.set_number(number ^ 42);
        value.set_text("unknown child");
        value.set_cap(cap);
        this
    }
    fn reader(&self) -> capnp::Result<dynamic_list::Reader<'_>> {
        let mut pointer = self
            .message
            .get_root_as_reader::<capnp::any_pointer::Reader>()?;
        pointer.imbue(&self.caps);
        let list = pointer.get_as::<capnp::struct_list::Reader<small_orphan::Owned>>()?;
        Ok(value::Reader::from(list).downcast())
    }
    fn set_number(&mut self, number: u32) -> capnp::Result<()> {
        let mut pointer = self.message.get_root::<capnp::any_pointer::Builder>()?;
        pointer.imbue_mut(&mut self.caps);
        pointer
            .get_as::<capnp::struct_list::Builder<orphan_payload::Owned>>()?
            .get(0)
            .set_number(number ^ 42);
        Ok(())
    }
}
fn wide(value: value::Reader<'_>) -> orphan_payload::Reader<'_> {
    // Observe fields unknown to the declared SmallOrphan schema through the
    // native reader conversion, without upgrading or copying its allocation.
    value
        .downcast_struct::<small_orphan::Owned>()
        .into_internal_struct_reader()
        .into()
}
fn address(list: dynamic_list::Reader<'_>) -> capnp::Result<*const u8> {
    Ok(list
        .get(0)?
        .downcast_struct::<small_orphan::Owned>()
        .into_internal_struct_reader()
        .get_data_section_as_blob()
        .as_ptr())
}

#[test]
fn concatenation_handles_primitives_unaligned_bits_empty_and_null_lists() -> capnp::Result<()> {
    for small in [false, true] {
        let mut message = capnp::message::Builder::new(allocator(small));
        let (mut root, token) = value::Builder::from(message.init_root::<orphan_case::Builder>())
            .downcast::<dynamic_struct::Builder>()
            .with_orphanage();
        let mut access = token.in_struct(&mut root)?;
        macro_rules! check {
            ($ty:ty, $values:expr) => {{
                let values: [$ty; 9] = $values;
                let mut a = capnp::message::Builder::new(allocator(small));
                let mut b = capnp::message::Builder::new(allocator(small));
                let empty = capnp::message::Builder::new_default();
                let mut la = a.initn_root::<capnp::primitive_list::Builder<$ty>>(3);
                for (i, v) in values[..3].iter().enumerate() {
                    la.set(i as u32, *v);
                }
                let mut lb = b.initn_root::<capnp::primitive_list::Builder<$ty>>(6);
                for (i, v) in values[3..].iter().enumerate() {
                    lb.set(i as u32, *v);
                }
                let lists = [
                    value::Reader::from(
                        a.get_root_as_reader::<capnp::primitive_list::Reader<$ty>>()?,
                    )
                    .downcast(),
                    value::Reader::from(
                        empty.get_root_as_reader::<capnp::primitive_list::Reader<$ty>>()?,
                    )
                    .downcast(),
                    value::Reader::from(
                        b.get_root_as_reader::<capnp::primitive_list::Reader<$ty>>()?,
                    )
                    .downcast(),
                ];
                let mut o = access.concat(<$ty>::introspect(), &lists)?;
                access.read(&mut o, |v| {
                    let l = v.downcast::<dynamic_list::Reader>();
                    assert_eq!(l.len(), 9);
                    for (i, v) in values.iter().enumerate() {
                        assert_eq!(l.get(i as u32)?.downcast::<$ty>(), *v);
                    }
                    Ok(())
                })?;
            }};
        }
        check!(
            bool,
            [true, true, false, false, true, false, false, true, false]
        );
        check!(u8, [1, 2, 3, 4, 5, 6, 7, 8, 9]);
        check!(u16, [1, 2, 3, 4, 5, 6, 7, 8, 900]);
        check!(u32, [1, 2, 3, 4, 5, 6, 7, 8, 90000]);
        check!(u64, [1, 2, 3, 4, 5, 6, 7, 8, u64::MAX]);
        check!(i64, [-1, -2, 3, 4, 5, 6, 7, 8, i64::MIN]);
        check!(f64, [1.5, 2., 3., 4., 5., 6., 7., 8., -9.25]);
        check!((), [(); 9]);
        let mut empty = access.concat(bool::introspect(), &[])?;
        access.read(&mut empty, |v| {
            assert!(v.downcast::<dynamic_list::Reader>().is_empty());
            Ok(())
        })?;
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn concatenation_keeps_unknown_capabilities_and_copies_independent_payloads(
) -> capnp::Result<()> {
    for small in [false, true] {
        let drops = Rc::new(Cell::new(0));
        let mut source = Input::new(11, client(&drops), small);
        let original = source.reader()?;
        let original_address = wide(original.get(0)?).get_text()?.0.as_ptr();
        let mut caps = vec![];
        let mut message = capnp::message::Builder::new(allocator(small));
        let mut root = message.init_root::<orphan_case::Builder>();
        root.imbue_mut(&mut caps);
        let (mut root, token) = value::Builder::from(root)
            .downcast::<dynamic_struct::Builder>()
            .with_orphanage();
        let mut orphan = token
            .in_struct(&mut root)?
            .concat(small_orphan::Owned::introspect(), &[original, original])?;
        source.set_number(99)?;
        let held = token.in_struct(&mut root)?.read(&mut orphan, |v| {
            let list = v.downcast::<dynamic_list::Reader>();
            let first = wide(list.get(0)?);
            let second = wide(list.get(1)?);
            assert_eq!(first.get_number() ^ 42, 11);
            assert_ne!(first.get_text()?.0.as_ptr(), original_address);
            assert_ne!(first.get_text()?.0.as_ptr(), second.get_text()?.0.as_ptr());
            first.get_cap()
        })?;
        let start = token
            .in_struct(&mut root)?
            .read(&mut orphan, |v| address(v.downcast()))?;
        token.in_struct(&mut root)?.resize(&mut orphan, 1)?;
        assert_eq!(
            start,
            token
                .in_struct(&mut root)?
                .read(&mut orphan, |v| address(v.downcast()))?
        );
        drop(source);
        assert_eq!(drops.get(), 0);
        root.adopt_named("any", orphan).unwrap();
        root.clear(root.get_schema().get_field_by_name("any")?)?;
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
fn concatenation_rejects_brands_overflow_and_partial_capability_copy_failure() -> capnp::Result<()>
{
    let drops = Rc::new(Cell::new(0));
    let source = Input::new(1, client(&drops), true);
    let mut invalid = Input::new(2, client(&Rc::new(Cell::new(0))), true);
    invalid.caps.clear(); // Keep a structurally valid reader with an invalid capability index.
    let mut caps = vec![];
    let mut message = capnp::message::Builder::new_default();
    let mut root = message.init_root::<orphan_case::Builder>();
    root.imbue_mut(&mut caps);
    root.set_sentinel(123);
    let (mut root, token) = value::Builder::from(root)
        .downcast::<dynamic_struct::Builder>()
        .with_orphanage();
    let mut access = token.in_struct(&mut root)?;
    assert_eq!(
        access
            .concat(
                small_orphan::Owned::introspect(),
                &[source.reader()?, invalid.reader()?]
            )
            .err()
            .unwrap()
            .kind,
        ErrorKind::MessageContainsInvalidCapabilityPointer
    );
    drop(source);
    assert_eq!(
        drops.get(),
        1,
        "failed concatenation leaked a previously copied capability"
    );
    let mut branded = capnp::message::Builder::new_default();
    let reader = branded
        .initn_root::<capnp::struct_list::Builder<parcel::Owned<capnp::text::Owned>>>(0)
        .into_reader();
    assert_eq!(
        access
            .concat(
                parcel::Owned::<capnp::data::Owned>::introspect(),
                &[value::Reader::from(reader).downcast()]
            )
            .err()
            .unwrap()
            .kind,
        ErrorKind::TypeMismatch
    );
    let mut big = capnp::message::Builder::new_default();
    let list = big
        .initn_root::<capnp::primitive_list::Builder<()>>((1 << 29) - 1)
        .into_reader();
    let list = value::Reader::from(list).downcast();
    assert!(access.concat(<()>::introspect(), &[list, list]).is_err());
    let mut virtual_list = capnp::message::Builder::new_default();
    virtual_list.initn_root::<capnp::primitive_list::Builder<()>>(1 << 28);
    let virtual_list = value::Reader::from(
        virtual_list.get_root_as_reader::<capnp::struct_list::Reader<orphan_payload::Owned>>()?,
    )
    .downcast();
    assert!(access
        .concat(orphan_payload::Owned::introspect(), &[virtual_list])
        .is_err());
    assert_eq!(
        root.reborrow_as_reader()
            .get_named("sentinel")?
            .downcast::<u32>(),
        123
    );
    assert!(!root.has_named("any")?);
    Ok(())
}

#[test]
fn concatenation_upgrades_primitive_encodings_and_preserves_maximum_struct_sections(
) -> capnp::Result<()> {
    use capntproto_test_support::dynamic_test_capnp::orphan_group;
    for small in [false, true] {
        let drops = Rc::new(Cell::new(0));
        let source = Input::new(1, client(&drops), small);
        let mut primitive = capnp::message::Builder::new(allocator(small));
        let mut list = primitive.initn_root::<capnp::primitive_list::Builder<u64>>(2);
        list.set(0, 123);
        list.set(1, 456);
        let primitive = value::Reader::from(
            primitive.get_root_as_reader::<capnp::struct_list::Reader<small_orphan::Owned>>()?,
        )
        .downcast();
        let mut larger = capnp::message::Builder::new(allocator(small));
        let mut g = larger
            .initn_root::<capnp::struct_list::Builder<orphan_group::Owned>>(1)
            .get(0);
        g.set_sibling(987);
        g.reborrow().init_body().set_value(55);
        let larger = value::Reader::from(
            larger.get_root_as_reader::<capnp::struct_list::Reader<small_orphan::Owned>>()?,
        )
        .downcast();
        let mut caps = vec![];
        let mut message = capnp::message::Builder::new(allocator(small));
        let mut root = message.init_root::<orphan_case::Builder>();
        root.imbue_mut(&mut caps);
        let (mut root, token) = value::Builder::from(root)
            .downcast::<dynamic_struct::Builder>()
            .with_orphanage();
        let mut orphan = token.in_struct(&mut root)?.concat(
            small_orphan::Owned::introspect(),
            &[source.reader()?, primitive, larger],
        )?;
        token.in_struct(&mut root)?.read(&mut orphan, |v| {
            let list = v.downcast::<dynamic_list::Reader>();
            assert_eq!(list.len(), 4);
            for i in 0..4 {
                let raw = list
                    .get(i)?
                    .downcast_struct::<small_orphan::Owned>()
                    .into_internal_struct_reader();
                assert!(raw.get_data_section_size() >= 128);
                assert!(raw.get_pointer_section_size() >= 2);
            }
            assert_eq!(
                list.get(1)?
                    .downcast_struct::<small_orphan::Owned>()
                    .get_number(),
                123
            );
            assert_eq!(
                list.get(2)?
                    .downcast_struct::<small_orphan::Owned>()
                    .get_number(),
                456
            );
            assert!(!wide(list.get(1)?).has_cap());
            assert!(wide(list.get(0)?).has_cap());
            let raw = list
                .get(3)?
                .downcast_struct::<small_orphan::Owned>()
                .into_internal_struct_reader();
            let group: orphan_group::Reader = raw.into();
            assert_eq!(group.get_sibling(), 987);
            assert!(matches!(
                group.get_body().which()?,
                orphan_group::body::Which::Value(55)
            ));
            Ok(())
        })?;
        // An inline struct list viewed as primitive must retain the extra
        // sections when copied, including capabilities invisible to that view.
        let mut ptr = source
            .message
            .get_root_as_reader::<capnp::any_pointer::Reader>()?;
        ptr.imbue(&source.caps);
        let primitive_view =
            value::Reader::from(ptr.get_as::<capnp::primitive_list::Reader<u32>>()?).downcast();
        let mut o = token
            .in_struct(&mut root)?
            .concat(u32::introspect(), &[primitive_view])?;
        token.in_struct(&mut root)?.read(&mut o, |v| {
            assert_eq!(
                v.downcast::<dynamic_list::Reader>()
                    .get(0)?
                    .downcast::<u32>(),
                1
            );
            Ok(())
        })?;
        drop(source);
        assert_eq!(drops.get(), 0);
        drop(orphan);
        assert_eq!(drops.get(), 0);
        drop(o);
        assert_eq!(drops.get(), 1);
    }
    Ok(())
}

#[test]
fn shrink_keeps_blob_addresses_and_zeroes_removed_data_and_bit_padding() -> capnp::Result<()> {
    for small in [false, true] {
        let mut message = capnp::message::Builder::new(allocator(small));
        let (mut root, token) = value::Builder::from(message.init_root::<orphan_case::Builder>())
            .downcast::<dynamic_struct::Builder>()
            .with_orphanage();
        let mut a = token.in_struct(&mut root)?;
        let mut data = a.new_data(17)?;
        a.edit(&mut data, |v| {
            v.downcast::<capnp::data::Builder>().fill(255);
            Ok(())
        })?;
        let original = a.read(&mut data, |v| {
            Ok(v.downcast::<capnp::data::Reader>().as_ptr())
        })?;
        a.resize(&mut data, 3)?;
        a.read(&mut data, |v| {
            let d = v.downcast::<capnp::data::Reader>();
            assert_eq!(d, [255; 3]);
            assert_eq!(d.as_ptr(), original);
            Ok(())
        })?;
        a.resize(&mut data, 9)?;
        a.read(&mut data, |v| {
            let d = v.downcast::<capnp::data::Reader>();
            assert_eq!(&d[..3], [255; 3]);
            assert_eq!(&d[3..], [0; 6]);
            Ok(())
        })?;
        let mut text = a.new_text(17)?;
        a.edit(&mut text, |v| {
            v.downcast::<capnp::text::Builder>()
                .as_bytes_mut()
                .fill(b'x');
            Ok(())
        })?;
        let original = a.read(&mut text, |v| {
            Ok(v.downcast::<capnp::text::Reader>().0.as_ptr())
        })?;
        for count in [8, 3, 0] {
            a.resize(&mut text, count)?;
            a.read(&mut text, |v| {
                let t = v.downcast::<capnp::text::Reader>();
                assert_eq!(t.0.len(), count as usize);
                assert_eq!(t.0.as_ptr(), original);
                Ok(())
            })?;
        }
        let mut bits = a.new_list(bool::introspect(), 17)?;
        a.edit(&mut bits, |v| {
            let mut l = v.downcast::<dynamic_list::Builder>();
            for i in 0..17 {
                l.set(i, true.into())?;
            }
            Ok(())
        })?;
        a.resize(&mut bits, 3)?;
        root.adopt_named("any", bits).unwrap();
        // A normal reader still sees the shortened list; private wire tests
        // additionally inspect the erased physical tail and padding directly.
        let list = root
            .reborrow_as_reader()
            .get_named("any")?
            .downcast::<capnp::any_pointer::Reader>();
        let list = list.get_as::<capnp::primitive_list::Reader<bool>>()?;
        assert_eq!(list.len(), 3);
        assert!(list.iter().all(|bit| bit));
        drop(data);
        drop(text);
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_orphan_concat_traces() -> capnp::Result<()> {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_ORPHAN_CONCAT_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for small in [false, true] {
        for (case_index, case) in cases.iter().enumerate() {
            let a_drops = Rc::new(Cell::new(0));
            let b_drops = Rc::new(Cell::new(0));
            let a_client = client(&a_drops);
            let a_id = a_client.client.hook.get_ptr();
            let b_client = client(&b_drops);
            let b_id = b_client.client.hook.get_ptr();
            let mut a = Some(Input::new(1, a_client, small));
            let mut b = Some(Input::new(10, b_client, small));
            let mut invalid = Input::new(99, client(&Rc::new(Cell::new(0))), small);
            invalid.caps.clear();
            let ambient_drops = Rc::new(Cell::new(0));
            let ambient = client(&ambient_drops);
            let ambient_id = ambient.client.hook.get_ptr();
            let mut caps = vec![];
            let mut message = capnp::message::Builder::new(allocator(small));
            let mut root = message.init_root::<orphan_case::Builder>();
            root.imbue_mut(&mut caps);
            root.set_sentinel(123);
            root.set_token(ambient);
            let (mut root, token) = value::Builder::from(root)
                .downcast::<dynamic_struct::Builder>()
                .with_orphanage();
            let mut orphan = None;
            let mut held: Option<harness::Client> = None;
            let mut original_address = std::ptr::null();
            for step in case["steps"].as_array().unwrap() {
                let s: Vec<usize> = step["state"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as usize)
                    .collect();
                match step["action"].as_str().unwrap() {
                    "concat" => {
                        let mut o = token.in_struct(&mut root)?.concat(
                            small_orphan::Owned::introspect(),
                            &[a.as_ref().unwrap().reader()?, b.as_ref().unwrap().reader()?],
                        )?;
                        original_address = token
                            .in_struct(&mut root)?
                            .read(&mut o, |v| address(v.downcast()))?;
                        orphan = Some(o);
                    }
                    "fail-copy" => {
                        let err = token
                            .in_struct(&mut root)?
                            .concat(
                                small_orphan::Owned::introspect(),
                                &[a.as_ref().unwrap().reader()?, invalid.reader()?],
                            )
                            .err()
                            .unwrap();
                        assert_eq!(err.kind, ErrorKind::MessageContainsInvalidCapabilityPointer);
                    }
                    "bad-type" => {
                        let err = token
                            .in_struct(&mut root)?
                            .concat(u32::introspect(), &[a.as_ref().unwrap().reader()?])
                            .err()
                            .unwrap();
                        assert_eq!(err.kind, ErrorKind::TypeMismatch);
                    }
                    "edit-source" => a.as_mut().unwrap().set_number(2)?,
                    "edit-copy" => {
                        token
                            .in_struct(&mut root)?
                            .edit(orphan.as_mut().unwrap(), |v| {
                                v.downcast::<dynamic_list::Builder>()
                                    .get(0)?
                                    .downcast_struct::<small_orphan::Owned>()
                                    .set_number(3);
                                Ok(())
                            })?
                    }
                    "shrink" => token
                        .in_struct(&mut root)?
                        .resize(orphan.as_mut().unwrap(), 1)?,
                    "read" => {
                        held = Some(if let Some(o) = orphan.as_mut() {
                            token.in_struct(&mut root)?.read(o, |v| {
                                wide(v.downcast::<dynamic_list::Reader>().get(0)?).get_cap()
                            })?
                        } else {
                            let l = root
                                .reborrow_as_reader()
                                .get_named("any")?
                                .downcast::<capnp::any_pointer::Reader>()
                                .get_as::<capnp::struct_list::Reader<orphan_payload::Owned>>()?;
                            l.get(0).get_cap()?
                        });
                    }
                    "release" => {
                        held.take();
                    }
                    "drop-a" => {
                        a.take();
                    }
                    "drop-b" => {
                        b.take();
                    }
                    "adopt" => root.adopt_named("any", orphan.take().unwrap()).unwrap(),
                    "drop" => {
                        if let Some(o) = orphan.take() {
                            drop(o);
                        } else {
                            root.clear(root.get_schema().get_field_by_name("any")?)?;
                        }
                    }
                    other => panic!("unknown action: {other}"),
                }
                assert_eq!(a.is_some(), s[0] == 1);
                assert_eq!(b.is_some(), s[1] == 1);
                assert_eq!(orphan.is_some(), s[2] == 1);
                assert_eq!(root.has_named("any")?, s[2] == 2);
                let observe = |list: dynamic_list::Reader<'_>| -> capnp::Result<()> {
                    assert_eq!(list.len(), if s[3] == 1 { 1 } else { 2 });
                    assert_eq!(
                        address(list)?,
                        original_address,
                        "trace {case_index}: {step}"
                    );
                    let first = list.get(0)?;
                    assert_eq!(
                        first
                            .clone()
                            .downcast_struct::<small_orphan::Owned>()
                            .get_number(),
                        s[11] as u32
                    );
                    let first = wide(first);
                    assert_eq!(first.get_cap()?.client.hook.get_ptr(), a_id);
                    assert_eq!(first.get_text()?, "unknown child");
                    if list.len() == 2 {
                        assert_eq!(wide(list.get(1)?).get_cap()?.client.hook.get_ptr(), b_id);
                    }
                    Ok(())
                };
                if let Some(o) = orphan.as_mut() {
                    token
                        .in_struct(&mut root)?
                        .read(o, |v| observe(v.downcast()))?;
                } else if s[2] == 2 {
                    let l = root
                        .reborrow_as_reader()
                        .get_named("any")?
                        .downcast::<capnp::any_pointer::Reader>()
                        .get_as::<capnp::struct_list::Reader<small_orphan::Owned>>()?;
                    observe(value::Reader::from(l).downcast())?;
                }
                if let Some(a) = &a {
                    assert_eq!(
                        a.reader()?
                            .get(0)?
                            .downcast_struct::<small_orphan::Owned>()
                            .get_number(),
                        s[10] as u32
                    );
                }
                assert_eq!(held.is_some(), s[7] == 1);
                if let Some(c) = &held {
                    assert_eq!(c.client.hook.get_ptr(), a_id);
                    assert_eq!(
                        c.echo_request().send().promise.await?.get()?.get_value(),
                        73
                    );
                }
                assert_eq!(
                    a_drops.get(),
                    usize::from(s[15] == 0),
                    "trace {case_index}: {step}"
                );
                assert_eq!(
                    b_drops.get(),
                    usize::from(s[16] == 0),
                    "trace {case_index}: {step}"
                );
                assert_eq!(ambient_drops.get(), 0);
                assert_eq!(
                    root.reborrow_as_reader()
                        .get_named("sentinel")?
                        .downcast::<u32>(),
                    123
                );
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
    Ok(())
}

#[test]
fn concatenation_copies_pointer_lists_and_nested_capabilities() -> capnp::Result<()> {
    for small in [false, true] {
        let mut a = capnp::message::Builder::new(allocator(small));
        let mut b = capnp::message::Builder::new(allocator(small));
        let mut x = a.initn_root::<capnp::text_list::Builder>(3);
        for (i, t) in ["a", "bc", "def"].iter().enumerate() {
            x.set(i as u32, t);
        }
        b.initn_root::<capnp::text_list::Builder>(1).set(0, "ghi");
        let a = value::Reader::from(a.get_root_as_reader::<capnp::text_list::Reader>()?).downcast();
        let b = value::Reader::from(b.get_root_as_reader::<capnp::text_list::Reader>()?).downcast();
        let mut caps = vec![];
        let mut dest = capnp::message::Builder::new(allocator(small));
        let mut root = dest.init_root::<orphan_case::Builder>();
        root.imbue_mut(&mut caps);
        let (mut root, token) = value::Builder::from(root)
            .downcast::<dynamic_struct::Builder>()
            .with_orphanage();
        let mut access = token.in_struct(&mut root)?;
        let mut o = access.concat(capnp::text::Owned::introspect(), &[a, b])?;
        access.read(&mut o, |v| {
            let l = v.downcast::<dynamic_list::Reader>();
            for (i, t) in ["a", "bc", "def", "ghi"].iter().enumerate() {
                assert_eq!(l.get(i as u32)?.downcast::<capnp::text::Reader>(), *t);
            }
            Ok(())
        })?;
        let drops = Rc::new(Cell::new(0));
        let mut source_caps = vec![];
        let mut source = capnp::message::Builder::new(allocator(small));
        let mut p = source.init_root::<capnp::any_pointer::Builder>();
        p.imbue_mut(&mut source_caps);
        p.initn_as::<capnp::list_list::Builder<capnp::capability_list::Owned<harness::Client>>>(1)
            .init(0, 1)
            .set(0, client(&drops).client.hook);
        let mut p = source.get_root_as_reader::<capnp::any_pointer::Reader>()?;
        p.imbue(&source_caps);
        let lists = value::Reader::from(
            p.get_as::<capnp::list_list::Reader<capnp::capability_list::Owned<harness::Client>>>()?,
        )
        .downcast();
        let mut nested = access.concat(
            capnp::capability_list::Owned::<harness::Client>::introspect(),
            &[lists, lists],
        )?;
        access.read(&mut nested, |v| {
            let l = v.downcast::<dynamic_list::Reader>();
            assert_eq!(l.len(), 2);
            let a = l
                .get(0)?
                .downcast::<dynamic_list::Reader>()
                .get(0)?
                .downcast::<value::Capability>()
                .cast::<harness::Client>()?;
            let b = l
                .get(1)?
                .downcast::<dynamic_list::Reader>()
                .get(0)?
                .downcast::<value::Capability>()
                .cast::<harness::Client>()?;
            assert_eq!(a.client.hook.get_ptr(), b.client.hook.get_ptr());
            Ok(())
        })?;
        drop(source);
        drop(source_caps);
        assert_eq!(drops.get(), 0);
        access.resize(&mut nested, 1)?;
        assert_eq!(drops.get(), 0);
        drop(nested);
        assert_eq!(drops.get(), 1);
    }
    Ok(())
}
