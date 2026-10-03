use capnp::{
    any_list::{self, ElementSize},
    any_pointer, any_struct_list,
    capability::FromClientHook,
    introspect::Introspect,
    message,
    private::layout::CapTable,
    traits::{Imbue, ImbueMut},
    Equality, Word,
};
use capntproto_test_support::{
    membrane_copy_capnp::{empty, service},
    native_list_capnp::{item, lists, Choice},
};
use std::rc::Rc;

#[path = "any_list/verification.rs"]
mod verification;

struct Server;
impl service::Server for Server {
    async fn ping(
        self: Rc<Self>,
        _: service::PingParams,
        mut result: service::PingResults,
    ) -> capnp::Result<()> {
        result.get().set_value(17);
        Ok(())
    }
}
fn server() -> service::Client {
    capnp_rpc::new_client(Server)
}
fn ping(pointer: any_pointer::Reader<'_>) {
    let cap: service::Client = pointer.get_as_capability().unwrap();
    let result = futures::executor::block_on(cap.ping_request().send().promise).unwrap();
    assert_eq!(result.get().unwrap().get_value(), 17);
}

#[derive(Default)]
struct Message {
    data: message::Builder<message::HeapAllocator>,
    caps: CapTable,
}
impl Message {
    fn builder(&mut self) -> any_pointer::Builder<'_> {
        let mut root = self.data.get_root::<any_pointer::Builder>().unwrap();
        root.imbue_mut(&mut self.caps);
        root
    }
    fn reader(&self) -> any_pointer::Reader<'_> {
        let mut root = self
            .data
            .get_root_as_reader::<any_pointer::Reader>()
            .unwrap();
        root.imbue(&self.caps);
        root
    }
    fn list(&self) -> any_list::Reader<'_> {
        self.reader().get_as().unwrap()
    }
}
fn encoding(kind: u64) -> ElementSize {
    [
        ElementSize::Void,
        ElementSize::Bit,
        ElementSize::Byte,
        ElementSize::TwoBytes,
        ElementSize::FourBytes,
        ElementSize::EightBytes,
        ElementSize::Pointer,
        ElementSize::InlineComposite,
    ][kind.min(7) as usize]
}
fn layout(kind: u64) -> (u16, u16) {
    match kind {
        7 => (0, 0),
        8 => (1, 0),
        9 => (0, 1),
        10 => (1, 1),
        11 => (2, 2),
        _ => panic!(),
    }
}
fn allocate(message: &mut Message, kind: u64, count: u32) {
    if kind < 7 {
        message
            .builder()
            .init_as_any_list(encoding(kind), count)
            .unwrap();
    } else {
        let (d, p) = layout(kind);
        message
            .builder()
            .init_as_list_of_any_struct(d, p, count)
            .unwrap();
    }
}

#[test]
fn every_encoding_has_explicit_layout_and_checked_views() {
    for kind in 0..12 {
        for count in [0, 1, 2, 7, 8, 9, 65] {
            let mut message = Message::default();
            allocate(&mut message, kind, count);
            let list = message.list();
            assert_eq!(list.len(), count);
            assert_eq!(list.is_empty(), count == 0);
            assert_eq!(list.get_element_size(), encoding(kind));
            assert_eq!(list.equals(list).unwrap(), Equality::Equal);
            let pointer_slots = kind == 6 || kind == 9 || kind >= 10;
            assert_eq!(list.get_raw_bytes().is_err(), pointer_slots);
            assert_eq!(list.get_as_struct_list().is_err(), kind == 1);
            let size = list.total_size().unwrap();
            assert_eq!(
                size.word_count,
                message.reader().target_size().unwrap().word_count
            );
            let mut builder = message.builder().get_as::<any_list::Builder>().unwrap();
            assert_eq!(builder.get_element_size(), encoding(kind));
            assert_eq!(builder.len(), count);
            assert_eq!(builder.reborrow().get_as_struct_list().is_err(), kind == 1);
            if kind != 1 {
                let structs = builder.get_as_struct_list().unwrap();
                assert!(structs.as_reader().try_get(count).is_none());
                assert_eq!(structs.as_reader().iter().count(), count as usize);
                assert_eq!(
                    structs.as_reader().total_size().unwrap().word_count,
                    size.word_count
                );
                if count > 0 {
                    let s = structs.as_reader().get(count - 1).unwrap();
                    let expected = match kind {
                        0 => (0, 0),
                        2 => (1, 0),
                        3 => (2, 0),
                        4 => (4, 0),
                        5 => (8, 0),
                        6 => (0, 1),
                        _ => {
                            let (d, p) = layout(kind);
                            (usize::from(d) * 8, u32::from(p))
                        }
                    };
                    assert_eq!(
                        (s.get_data_section().len(), s.get_pointer_section().len()),
                        expected
                    );
                }
            }
        }
    }
}

#[test]
fn native_primitive_enum_blob_nested_and_dynamic_casts() {
    macro_rules! check {
        ($ty:ty,$size:expr,$value:expr) => {{
            let mut message = Message::default();
            let mut list = message.builder().init_as_any_list($size, 3).unwrap();
            list.reborrow()
                .get_as::<capnp::primitive_list::Owned<$ty>>()
                .unwrap()
                .set(2, $value);
            assert_eq!(
                list.as_reader()
                    .get_as::<capnp::primitive_list::Owned<$ty>>()
                    .unwrap()
                    .get(2),
                $value
            );
            assert_eq!(
                list.as_reader()
                    .get_as_dynamic(<$ty>::introspect())
                    .unwrap()
                    .len(),
                3
            );
        }};
    }
    check!(bool, ElementSize::Bit, true);
    check!(u8, ElementSize::Byte, 17);
    check!(i8, ElementSize::Byte, -17);
    check!(u16, ElementSize::TwoBytes, 500);
    check!(i16, ElementSize::TwoBytes, -500);
    check!(u32, ElementSize::FourBytes, 500);
    check!(i32, ElementSize::FourBytes, -500);
    check!(u64, ElementSize::EightBytes, 500);
    check!(i64, ElementSize::EightBytes, -500);
    check!(f32, ElementSize::FourBytes, 1.5);
    check!(f64, ElementSize::EightBytes, 1.5);
    let mut message = Message::default();
    let mut list = message
        .builder()
        .init_as_any_list(ElementSize::TwoBytes, 2)
        .unwrap();
    list.reborrow()
        .get_as::<capnp::enum_list::Owned<Choice>>()
        .unwrap()
        .set(1, Choice::One);
    assert_eq!(
        list.as_reader()
            .get_as::<capnp::enum_list::Owned<Choice>>()
            .unwrap()
            .get(1)
            .unwrap(),
        Choice::One
    );
    let mut list = message
        .builder()
        .init_as_any_list(ElementSize::Pointer, 3)
        .unwrap();
    list.reborrow()
        .get_as::<capnp::text_list::Owned>()
        .unwrap()
        .set(0, "hello");
    list.reborrow()
        .get_as::<capnp::data_list::Owned>()
        .unwrap()
        .set(1, &[1, 2]);
    list.reborrow()
        .get_as::<capnp::list_list::Owned<capnp::primitive_list::Owned<u32>>>()
        .unwrap()
        .init(2, 1)
        .set(0, 77);
    let typed = list
        .reborrow()
        .get_as::<capnp::any_pointer_list::Owned>()
        .unwrap();
    let erased = any_list::Builder::from_builder(typed).unwrap();
    assert_eq!(
        erased
            .as_reader()
            .get_as::<capnp::text_list::Owned>()
            .unwrap()
            .get(0)
            .unwrap(),
        "hello"
    );
    assert_eq!(
        erased
            .as_reader()
            .get_as::<capnp::data_list::Owned>()
            .unwrap()
            .get(1)
            .unwrap(),
        &[1, 2]
    );
    assert_eq!(
        erased
            .as_reader()
            .get_as::<capnp::list_list::Owned<capnp::primitive_list::Owned<u32>>>()
            .unwrap()
            .get(2)
            .unwrap()
            .get(0),
        77
    );
    let mut dynamic = erased
        .get_as_dynamic(capnp::introspect::TypeVariant::AnyPointer.into())
        .unwrap();
    let mut ptr: any_pointer::Builder = dynamic.reborrow().get(0).unwrap().downcast();
    ptr.clear();
    assert!(any_list::Builder::from_builder(dynamic)
        .unwrap()
        .as_reader()
        .get_as::<capnp::any_pointer_list::Owned>()
        .unwrap()
        .get(0)
        .is_null());
}

#[test]
fn inline_pointer_projection_roundtrip_retains_data_and_authority() {
    let mut message = Message::default();
    allocate(&mut message, 11, 2);
    let mut structs = message
        .builder()
        .get_as::<any_struct_list::Builder>()
        .unwrap();
    for i in 0..2 {
        let mut value = structs.reborrow().get(i);
        value.get_data_section()[0] = 11 + i as u8;
        value.get_data_section()[15] = 91;
        value
            .get_pointer_section()
            .get(0)
            .set_as_capability(server().into_client_hook());
        value
            .get_pointer_section()
            .get(1)
            .set_as::<capnp::text::Owned>("unknown")
            .unwrap();
    }
    // This route previously shifted the base twice on into_reader().
    let mut projected = message
        .builder()
        .get_as::<capnp::capability_list::Builder<service::Client>>()
        .unwrap();
    projected.set(1, server().into_client_hook());
    let erased = any_list::Builder::from_builder(projected).unwrap();
    let mut copy = Message::default();
    copy.builder()
        .set_as::<any_pointer::Owned>(erased.as_reader())
        .unwrap();
    let typed = erased
        .get_as::<capnp::any_pointer_list::Owned>()
        .unwrap()
        .into_reader();
    for i in 0..2 {
        ping(typed.get(i));
    }
    message.builder().clear();
    drop(message);
    let copied = copy.list();
    assert_eq!(copied.get_element_size(), ElementSize::InlineComposite);
    assert_eq!(copied.total_size().unwrap().cap_count, 2);
    assert_eq!(
        copied.equals(copied).unwrap(),
        Equality::UnknownContainsCapabilities
    );
    let structs = copied.get_as_struct_list().unwrap();
    for i in 0..2 {
        let value = structs.get(i).unwrap();
        assert_eq!(value.get_data_section()[0], 11 + i as u8);
        assert_eq!(value.get_data_section()[15], 91);
        ping(value.get_pointer_section().get(0));
        assert_eq!(
            value
                .get_pointer_section()
                .get(1)
                .get_as::<capnp::text::Reader>()
                .unwrap(),
            "unknown"
        );
    }
    // A typed view of the first pointer can update it while retaining the
    // surrounding data and extra pointers; erasure sees the whole element.
    let mut projected = copy
        .builder()
        .get_as::<any_list::Builder>()
        .unwrap()
        .get_as::<capnp::any_pointer_list::Owned>()
        .unwrap();
    projected.reborrow().get(0).clear();
    let view = any_list::Builder::from_builder(projected)
        .unwrap()
        .into_reader();
    assert_eq!(view.total_size().unwrap().cap_count, 1);
    assert_eq!(
        view.get_as_struct_list()
            .unwrap()
            .get(0)
            .unwrap()
            .get_data_section()[15],
        91
    );
}

#[test]
fn incompatible_casts_and_allocation_overflow_leave_storage_intact() {
    let mut message = Message::default();
    let mut bytes = message
        .builder()
        .init_as_any_list(ElementSize::Byte, 2)
        .unwrap();
    bytes
        .reborrow()
        .get_as::<capnp::primitive_list::Owned<u8>>()
        .unwrap()
        .set(1, 7);
    assert!(bytes
        .reborrow()
        .get_as::<capnp::primitive_list::Owned<u64>>()
        .is_err());
    assert!(bytes
        .reborrow()
        .get_as::<capnp::primitive_list::Owned<bool>>()
        .is_err());
    assert!(bytes
        .reborrow()
        .get_as::<capnp::any_pointer_list::Owned>()
        .is_err());
    type Records = capnp::struct_list::Owned<item::Owned<capnp::text::Owned>>;
    assert!(bytes.reborrow().get_as::<Records>().is_err());
    assert_eq!(
        bytes
            .as_reader()
            .get_as::<Records>()
            .unwrap()
            .get(1)
            .get_number(),
        0
    );
    assert_eq!(bytes.as_reader().get_raw_bytes().unwrap(), &[0, 7]);
    for (d, p) in [(0, 1), (1, 0)] {
        let mut list = message
            .builder()
            .init_as_list_of_any_struct(d, p, 1)
            .unwrap()
            .into_any_list();
        assert!(list.reborrow().get_as::<Records>().is_err());
        assert!(list
            .reborrow()
            .get_as_dynamic(item::Owned::<capnp::text::Owned>::introspect())
            .is_err());
        assert_eq!(list.get_element_size(), ElementSize::InlineComposite);
    }
    let mut list = message
        .builder()
        .init_as_list_of_any_struct(1, 1, 1)
        .unwrap()
        .into_any_list();
    list.reborrow()
        .get_as::<Records>()
        .unwrap()
        .get(0)
        .set_number(73);
    let original = message
        .list()
        .get_as_struct_list()
        .unwrap()
        .get(0)
        .unwrap()
        .canonicalize()
        .unwrap();
    assert!(message
        .builder()
        .init_as_any_list(ElementSize::InlineComposite, 0)
        .is_err());
    assert!(message
        .builder()
        .init_as_any_list(ElementSize::Void, 1 << 29)
        .is_err());
    assert!(message
        .builder()
        .init_as_list_of_any_struct(0, 0, 1 << 30)
        .is_err());
    assert!(message
        .builder()
        .init_as_list_of_any_struct(u16::MAX, u16::MAX, u32::MAX)
        .is_err());
    assert_eq!(
        message
            .list()
            .get_as::<Records>()
            .unwrap()
            .get(0)
            .get_number(),
        73
    );
    assert_eq!(
        Word::words_to_bytes(&original),
        Word::words_to_bytes(
            &message
                .list()
                .get_as_struct_list()
                .unwrap()
                .get(0)
                .unwrap()
                .canonicalize()
                .unwrap()
        )
    );
    // Zero-size maximum counts allocate only a tag, without iterating elements.
    let list = message
        .builder()
        .init_as_list_of_any_struct(0, 0, (1 << 30) - 1)
        .unwrap();
    assert_eq!(list.len(), (1 << 30) - 1);
    assert_eq!(list.as_reader().total_size().unwrap().word_count, 1);
    assert!(list
        .as_reader()
        .as_any_list()
        .get_raw_bytes()
        .unwrap()
        .is_empty());
}

#[test]
fn virtual_struct_elements_mutate_only_their_own_bytes() {
    let mut message = Message::default();
    let mut list = message
        .builder()
        .init_as_any_list(ElementSize::Byte, 2)
        .unwrap()
        .get_as_struct_list()
        .unwrap();
    list.reborrow().get(0).get_data_section()[0] = 7;
    list.reborrow().get(1).get_data_section()[0] = 99;
    assert_eq!(
        list.as_reader().as_any_list().get_raw_bytes().unwrap(),
        &[7, 99]
    );
    assert_eq!(
        list.as_reader()
            .iter()
            .map(|s| s.unwrap().get_data_section()[0])
            .collect::<Vec<_>>(),
        vec![7, 99]
    );
    assert!(list.reborrow().try_get(2).is_none());
    let mut pointer_list = message
        .builder()
        .init_as_any_list(ElementSize::Pointer, 1)
        .unwrap()
        .get_as_struct_list()
        .unwrap();
    pointer_list
        .reborrow()
        .get(0)
        .get_pointer_section()
        .get(0)
        .set_as_capability(server().into_client_hook());
    let cap = pointer_list.into_reader().get(0).unwrap();
    assert!(cap.get_data_section().is_empty());
    ping(cap.get_pointer_section().get(0));
}

#[test]
fn checked_views_and_writable_pointer_getters_reject_bit_reinterpretation() {
    let mut message = Message::default();
    allocate(&mut message, 1, 9);
    message
        .builder()
        .get_as::<capnp::primitive_list::Builder<bool>>()
        .unwrap()
        .set(8, true);
    assert!(message
        .reader()
        .get_as::<capnp::primitive_list::Reader<u8>>()
        .is_err());
    assert!(message
        .builder()
        .get_as::<capnp::primitive_list::Builder<u8>>()
        .is_err());
    assert!(message
        .builder()
        .get_as::<capnp::primitive_list::Builder<()>>()
        .is_err());
    assert_eq!(message.list().get_raw_bytes().unwrap(), &[0, 1]);
    allocate(&mut message, 2, 9);
    message
        .builder()
        .get_as::<capnp::primitive_list::Builder<u8>>()
        .unwrap()
        .set(8, 73);
    // Ordinary typed readers retain C++'s permissive primitive interpretation.
    // The new checked schema-free cast and writable getters reject it.
    assert!(message
        .reader()
        .get_as::<capnp::primitive_list::Reader<bool>>()
        .is_ok());
    assert!(message
        .list()
        .get_as::<capnp::primitive_list::Owned<bool>>()
        .is_err());
    assert!(message
        .builder()
        .get_as::<capnp::primitive_list::Builder<bool>>()
        .is_err());
    assert_eq!(
        message.list().get_raw_bytes().unwrap(),
        &[0, 0, 0, 0, 0, 0, 0, 0, 73]
    );
}

#[test]
fn loaded_and_compiled_erasure_and_null_views() {
    use capnp::schema_loader::{dynamic, SchemaLoader};
    let mut loader = SchemaLoader::default();
    loader
        .load_compiled_type_and_dependencies::<lists::Owned>()
        .unwrap();
    let id = lists::Owned::introspect()
        .as_struct_schema()
        .unwrap()
        .get_proto()
        .get_id();
    let mut message = message::Builder::new_default();
    message
        .init_root::<lists::Builder>()
        .init_numbers(2)
        .set(1, 73);
    let native = message
        .get_root_as_reader::<lists::Reader>()
        .unwrap()
        .get_numbers()
        .unwrap();
    let compiled: capnp::dynamic_value::Reader = native.into();
    let compiled: capnp::dynamic_list::Reader = compiled.downcast();
    let loaded = dynamic::Reader::new(
        message.get_root_as_reader().unwrap(),
        loader.get(id).unwrap(),
    )
    .unwrap();
    let dynamic::Value::List(loaded) = loaded.get_named("numbers").unwrap() else {
        panic!()
    };
    assert_eq!(
        any_list::Reader::from_reader(loaded)
            .equals(any_list::Reader::from_reader(compiled))
            .unwrap(),
        Equality::Equal
    );
    let mut message = Message::default();
    assert_eq!(message.list().len(), 0);
    assert_eq!(message.list().total_size().unwrap().word_count, 0);
    assert!(message.list().get_raw_bytes().unwrap().is_empty());
    assert!(message
        .list()
        .get_as::<capnp::primitive_list::Owned<bool>>()
        .unwrap()
        .is_empty());
    assert!(message
        .builder()
        .get_as::<any_list::Builder>()
        .unwrap()
        .is_empty());
    assert!(message.reader().is_null());
    let empty = any_struct_list::Reader::default();
    assert!(empty.is_empty());
    assert_eq!(
        empty.as_any_list().get_element_size(),
        ElementSize::InlineComposite
    );
    assert_eq!(empty.total_size().unwrap().word_count, 1);
    message
        .builder()
        .set_as::<any_pointer::Owned>(empty)
        .unwrap();
    assert_eq!(
        message.list().get_element_size(),
        ElementSize::InlineComposite
    );
}

fn words(values: &[u64]) -> Vec<Word> {
    let mut result = Word::allocate_zeroed_vec(values.len());
    for (bytes, value) in Word::words_to_bytes_mut(&mut result)
        .chunks_exact_mut(8)
        .zip(values)
    {
        bytes.copy_from_slice(&value.to_le_bytes());
    }
    result
}

#[test]
fn nesting_malformed_and_far_list_readers_are_checked() {
    let list = words(&[1 | (7 << 32) | (1 << 35), 4 | (1 << 32), 73]);
    let far = words(&[2 | (1 << 32)]);
    let double = words(&[6 | (1 << 32)]);
    let tag = words(&[2 | (2 << 32), 1 | (7 << 32) | (1 << 35)]);
    let contents = words(&[4 | (1 << 32), 73]);
    for segments in [
        vec![Word::words_to_bytes(&list)],
        vec![Word::words_to_bytes(&far), Word::words_to_bytes(&list)],
        vec![
            Word::words_to_bytes(&double),
            Word::words_to_bytes(&tag),
            Word::words_to_bytes(&contents),
        ],
    ] {
        let reader = message::Reader::new(segments, message::ReaderOptions::new());
        let value = reader.get_root::<any_list::Reader>().unwrap();
        assert_eq!(value.total_size().unwrap().word_count, 2);
        assert_eq!(
            value
                .get_as_struct_list()
                .unwrap()
                .get(0)
                .unwrap()
                .get_data_section()[0],
            73
        );
    }
    let segments = [Word::words_to_bytes(&list)];
    let reader = message::Reader::new(
        &segments[..],
        message::ReaderOptions {
            nesting_limit: 1,
            ..message::ReaderOptions::new()
        },
    );
    let list = reader.get_root::<any_list::Reader>().unwrap();
    assert!(list.get_as_struct_list().unwrap().get(0).is_err());
    assert!(list
        .get_as::<capnp::struct_list::Owned<empty::Owned>>()
        .is_err());
    assert!(list.get_as_dynamic(empty::Owned::introspect()).is_err());
    assert!(list.get_as_struct_list().unwrap().try_get(1).is_none());
    for invalid in [
        vec![7],
        vec![1 << 32],
        vec![1 | (5 << 32) | (1 << 35)],
        vec![1 | (7 << 32), 4 | (1 << 32)],
        vec![1 | (7 << 32), 1],
        vec![2 | (9 << 32)],
    ] {
        let data = words(&invalid);
        let segments = [Word::words_to_bytes(&data)];
        let reader = message::Reader::new(&segments[..], message::ReaderOptions::new());
        assert!(reader.get_root::<any_list::Reader>().is_err());
    }
    // A self-referential pointer list must fail recursive size traversal.
    let data = words(&[
        1 | (6 << 32) | (1 << 35),
        0xffff_fffd | (6 << 32) | (1 << 35),
    ]);
    let segments = [Word::words_to_bytes(&data)];
    let reader = message::Reader::new(
        &segments[..],
        message::ReaderOptions {
            nesting_limit: 8,
            ..message::ReaderOptions::new()
        },
    );
    assert!(reader
        .get_root::<any_list::Reader>()
        .unwrap()
        .total_size()
        .is_err());
    let data = words(&[1 | (0x1fff_ffffu64 << 35)]);
    let segments = [Word::words_to_bytes(&data)];
    let reader = message::Reader::new(&segments[..], message::ReaderOptions::new());
    assert!(
        reader.get_root::<any_list::Reader>().is_err(),
        "amplified void-list read"
    );
}
