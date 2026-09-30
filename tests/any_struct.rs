use capnp::{
    any_pointer::{self, PointerType},
    any_struct,
    capability::FromClientHook,
    introspect::Introspect,
    message,
    private::layout::CapTable,
    traits::{HasStructSize, Imbue, ImbueMut},
    Equality, Word,
};
use reproto_test_support::membrane_copy_capnp::{empty, payload, service};
use std::rc::Rc;

#[path = "any_struct/verification.rs"]
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
    fn value(&self) -> any_struct::Reader<'_> {
        self.reader().get_as().unwrap()
    }
}

#[test]
fn explicit_layout_sections_and_safe_schema_casts() {
    let size = payload::Builder::STRUCT_SIZE;
    let mut message = Message::default();
    let mut value = message
        .builder()
        .init_as_any_struct(size.data + 1, size.pointers + 1);
    assert_eq!(
        value.as_reader().get_data_section(),
        vec![0; usize::from(size.data + 1) * 8]
    );
    assert_eq!(
        value.as_reader().get_pointer_section().len(),
        u32::from(size.pointers + 1)
    );
    value
        .reborrow()
        .get_as::<payload::Owned>()
        .unwrap()
        .set_number(0x0102_0304);
    assert_eq!(&value.get_data_section()[..4], &[4, 3, 2, 1]);
    *value.get_data_section().last_mut().unwrap() = 91;
    value
        .get_pointer_section()
        .get(size.pointers.into())
        .set_as::<capnp::text::Owned>("unknown")
        .unwrap();
    let schema = payload::Owned::introspect().as_struct_schema().unwrap();
    let mut dynamic = value.reborrow().get_as_dynamic(schema).unwrap();
    dynamic.set_named("number", 42u32.into()).unwrap();
    assert_eq!(
        value.as_reader().get_as::<payload::Owned>().get_number(),
        42
    );
    assert_eq!(
        value
            .as_reader()
            .get_as_dynamic(schema)
            .get_named("number")
            .unwrap()
            .downcast::<u32>(),
        42
    );
    let erased = any_struct::Reader::from_reader(value.as_reader().get_as::<empty::Owned>());
    let mut copy = Message::default();
    copy.builder().set_as::<any_pointer::Owned>(erased).unwrap();
    assert_eq!(copy.value().equals(erased).unwrap(), Equality::Equal);
    value.get_data_section()[0] = 90;
    assert_eq!(copy.value().get_data_section()[0], 42);
    assert_eq!(*copy.value().get_data_section().last().unwrap(), 91);
    assert_eq!(
        copy.value()
            .get_pointer_section()
            .get(size.pointers.into())
            .get_as::<capnp::text::Reader>()
            .unwrap(),
        "unknown"
    );
    for (data, pointers) in [(0, size.pointers), (size.data, 0)] {
        let mut small = message.builder().init_as_any_struct(data, pointers);
        assert!(small.reborrow().get_as::<payload::Owned>().is_err());
        assert!(small.reborrow().get_as_dynamic(schema).is_err());
        assert_eq!(
            small.as_reader().get_data_section().len(),
            usize::from(data) * 8
        );
        assert_eq!(
            small.as_reader().get_pointer_section().len(),
            u32::from(pointers)
        );
        // Read-only views can safely supply absent fields as defaults.
        assert_eq!(small.as_reader().get_as::<payload::Owned>().get_number(), 0);
    }
}

#[test]
fn erasure_reborrows_and_null_empty_structs() {
    let mut message = message::Builder::new_default();
    let mut generated = message.init_root::<payload::Builder>();
    generated.set_number(9);
    let mut erased = any_struct::Builder::from_builder(generated.reborrow()).unwrap();
    erased.get_data_section()[0] = 7;
    assert_eq!(
        erased.into_reader().get_as::<payload::Owned>().get_number(),
        7
    );
    assert_eq!(generated.reborrow().get_number(), 7);
    let dynamic: capnp::dynamic_value::Builder = generated.into();
    assert_eq!(
        any_struct::Builder::from_builder(dynamic)
            .unwrap()
            .as_reader()
            .get_data_section()[0],
        7
    );
    assert!(any_struct::Builder::from_builder(capnp::dynamic_value::Builder::UInt32(0)).is_err());
    let mut message = Message::default();
    assert_eq!(
        message.reader().get_pointer_type().unwrap(),
        PointerType::Null
    );
    assert!(message.value().get_data_section().is_empty());
    assert!(message.value().get_pointer_section().is_empty());
    let mut empty = message.builder().init_as::<any_struct::Builder>();
    assert!(empty.get_data_section().is_empty());
    assert!(empty.get_pointer_section().is_empty());
    assert_eq!(
        message.reader().get_pointer_type().unwrap(),
        PointerType::Struct
    );
    assert_eq!(
        message
            .value()
            .equals(any_struct::Reader::default())
            .unwrap(),
        Equality::Equal
    );
}

#[test]
fn loaded_schemas_and_virtual_list_elements_keep_physical_storage() {
    use capnp::schema_loader::{dynamic, SchemaLoader};
    let mut loader = SchemaLoader::default();
    loader
        .load_compiled_type_and_dependencies::<payload::Owned>()
        .unwrap();
    let schema = payload::Owned::introspect().as_struct_schema().unwrap();
    let mut message = message::Builder::new_default();
    message.init_root::<payload::Builder>().set_number(73);
    let loaded = dynamic::Reader::new(
        message.get_root_as_reader().unwrap(),
        loader.get(schema.get_proto().get_id()).unwrap(),
    )
    .unwrap();
    let erased = any_struct::Reader::from_reader(loaded);
    assert_eq!(erased.get_as::<payload::Owned>().get_number(), 73);
    assert_eq!(
        erased
            .equals(any_struct::Reader::from_reader(
                erased.get_as_dynamic(schema)
            ))
            .unwrap(),
        Equality::Equal
    );

    // List(UInt8) can be read as structs with a one-byte virtual data section.
    // Copying/canonicalizing promotes that element to a word without reading
    // data belonging to its neighbor.
    let mut message = message::Builder::new_default();
    let mut bytes = message.initn_root::<capnp::primitive_list::Builder<u8>>(2);
    bytes.set(0, 7);
    bytes.set(1, 99);
    let structs = message
        .get_root_as_reader::<capnp::struct_list::Reader<empty::Owned>>()
        .unwrap();
    let first = any_struct::Reader::from_reader(structs.get(0));
    assert_eq!(first.get_data_section(), &[7]);
    assert_eq!(first.total_size().unwrap().word_count, 1);
    let canonical = first.canonicalize().unwrap();
    let segments = [Word::words_to_bytes(&canonical)];
    let reader = message::Reader::new(&segments[..], message::ReaderOptions::new());
    assert_eq!(
        reader
            .get_root::<any_struct::Reader>()
            .unwrap()
            .get_data_section(),
        &[7, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        any_struct::Reader::from_reader(structs.get(1)).get_data_section(),
        &[99]
    );
}

#[test]
fn capabilities_survive_erasure_copy_and_section_mutation() {
    let mut source = Message::default();
    let mut value = source.builder().init_as_any_struct(2, 3);
    let cap = server();
    let identity = cap.as_client_hook().get_ptr();
    value
        .get_pointer_section()
        .get(1)
        .set_as_capability(cap.into_client_hook());
    value
        .get_pointer_section()
        .get(2)
        .set_as::<capnp::data::Owned>(&[73u8][..])
        .unwrap();
    let size = value.as_reader().total_size().unwrap();
    assert_eq!((size.word_count, size.cap_count), (6, 1));
    assert_eq!(
        value.equals(value.as_reader()).unwrap(),
        Equality::UnknownContainsCapabilities
    );
    assert!(value.as_reader().canonicalize().is_err());
    let mut copy = Message::default();
    copy.builder()
        .set_as::<any_pointer::Owned>(value.as_reader())
        .unwrap();
    value.get_pointer_section().get(1).clear();
    value.get_data_section().fill(255);
    drop(source);
    let reader = copy.value();
    let cap: service::Client = reader
        .get_pointer_section()
        .get(1)
        .get_as_capability()
        .unwrap();
    assert_eq!(cap.as_client_hook().get_ptr(), identity);
    assert_eq!(
        futures::executor::block_on(cap.ping_request().send().promise)
            .unwrap()
            .get()
            .unwrap()
            .get_value(),
        17
    );
    assert_eq!(reader.get_data_section(), &[0u8; 16]);
    assert_eq!(
        reader
            .get_pointer_section()
            .get(2)
            .get_as::<capnp::data::Reader>()
            .unwrap(),
        &[73]
    );
    copy.builder()
        .get_as::<any_struct::Builder>()
        .unwrap()
        .get_pointer_section()
        .get(1)
        .clear();
    assert_eq!(copy.value().total_size().unwrap().cap_count, 0);
    assert!(copy.value().canonicalize().is_ok());
}

#[test]
fn canonicalization_trims_padding_and_rejects_invalid_children() {
    let mut message = Message::default();
    let mut value = message.builder().init_as_any_struct(3, 3);
    value.get_data_section()[0] = 7;
    value
        .get_pointer_section()
        .get(0)
        .set_as::<capnp::text::Owned>("x")
        .unwrap();
    let words = value.as_reader().canonicalize().unwrap();
    let segments = [Word::words_to_bytes(&words)];
    let canonical = message::Reader::new(&segments[..], message::ReaderOptions::new());
    assert!(canonical.is_canonical().unwrap());
    let reader = canonical.get_root::<any_struct::Reader>().unwrap();
    assert_eq!(reader.get_data_section().len(), 8);
    assert_eq!(reader.get_pointer_section().len(), 1);
    assert_eq!(reader.equals(value.as_reader()).unwrap(), Equality::Equal);
    assert_eq!(
        Word::words_to_bytes(&reader.canonicalize().unwrap()),
        Word::words_to_bytes(&words)
    );
    assert_eq!(value.as_reader().get_data_section().len(), 24);
    assert_eq!(value.as_reader().get_pointer_section().len(), 3);
    let empty = any_struct::Reader::default().canonicalize().unwrap();
    assert_eq!(Word::words_to_bytes(&empty), &0xffff_fffcu64.to_le_bytes());
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
fn far_pointers_malformed_inputs_and_reader_limits() {
    let direct = words(&[1 << 32, 42]);
    let single = words(&[2 | (1 << 32)]);
    let double = words(&[6 | (1 << 32)]);
    let landing = words(&[2 | (2 << 32), 1 << 32]);
    let data = words(&[42]);
    for segments in [
        vec![Word::words_to_bytes(&direct)],
        vec![Word::words_to_bytes(&single), Word::words_to_bytes(&direct)],
        vec![
            Word::words_to_bytes(&double),
            Word::words_to_bytes(&landing),
            Word::words_to_bytes(&data),
        ],
    ] {
        let message = message::Reader::new(segments, message::ReaderOptions::new());
        let root = message.get_root::<any_pointer::Reader>().unwrap();
        assert_eq!(root.get_pointer_type().unwrap(), PointerType::Struct);
        let reader = root.get_as::<any_struct::Reader>().unwrap();
        assert_eq!(reader.get_data_section(), &42u64.to_le_bytes());
        assert_eq!(reader.total_size().unwrap().word_count, 1);
    }
    for invalid in [
        vec![1 << 32],
        vec![7],
        vec![2 | (9 << 32)],
        vec![1 | (2 << 32)],
        vec![3],
    ] {
        let words = words(&invalid);
        let segments = [Word::words_to_bytes(&words)];
        let message = message::Reader::new(&segments[..], message::ReaderOptions::new());
        assert!(message.get_root::<any_struct::Reader>().is_err());
    }
    for values in [vec![1 << 48, 7], vec![1 << 48, 0xffff_fffc | (1 << 48)]] {
        let words = words(&values);
        let segments = [Word::words_to_bytes(&words)];
        let message = message::Reader::new(&segments[..], message::ReaderOptions::new());
        assert!(message
            .get_root::<any_struct::Reader>()
            .unwrap()
            .canonicalize()
            .is_err());
    }
    for options in [
        message::ReaderOptions {
            nesting_limit: 0,
            ..message::ReaderOptions::new()
        },
        message::ReaderOptions {
            traversal_limit_in_words: Some(0),
            ..message::ReaderOptions::new()
        },
    ] {
        let segments = [Word::words_to_bytes(&direct)];
        let message = message::Reader::new(&segments[..], options);
        assert!(message.get_root::<any_struct::Reader>().is_err());
    }
}
