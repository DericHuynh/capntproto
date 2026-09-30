use capnp::{any_pointer, message, raw, schema_capnp::node, Equality, Word};

mod structural_equality {
    pub mod verification;
}

// Wire fixtures deliberately vary layout without relying on a schema's
// allocation sizes, canonicalization, capability extraction or deep copying.
fn structure(data: &[u64], pointers: &[Vec<u64>]) -> Vec<u64> {
    if data.is_empty() && pointers.is_empty() {
        return vec![0xffff_fffc]; // Non-null empty struct.
    }
    let mut words = vec![(data.len() as u64) << 32 | (pointers.len() as u64) << 48];
    words.extend(data);
    words.resize(1 + data.len() + pointers.len(), 0);
    for (i, pointer) in pointers.iter().enumerate() {
        append(&mut words, 1 + data.len() + i, pointer);
    }
    words
}

fn append(words: &mut Vec<u64>, slot: usize, value: &[u64]) {
    let root = value[0];
    words[slot] = if root == 0 || root & 3 == 3 || root == 0xffff_fffc {
        root
    } else {
        let offset = (root as u32 as i32 >> 2) as i64 + words.len() as i64 - 1 - slot as i64;
        (root & 0xffff_ffff_0000_0003) | u64::from((offset as u32) << 2)
    };
    words.extend_from_slice(&value[1..]);
}

fn primitive(encoding: u64, count: u64, data: &[u64]) -> Vec<u64> {
    let mut words = vec![1 | (encoding << 32) | (count << 35)];
    words.extend(data);
    words
}

fn pointers(values: &[Vec<u64>]) -> Vec<u64> {
    let mut words = primitive(6, values.len() as u64, &[]);
    words.resize(1 + values.len(), 0);
    for (i, value) in values.iter().enumerate() {
        append(&mut words, 1 + i, value);
    }
    words
}

type StructElement = (Vec<u64>, Vec<Vec<u64>>);

fn composite(data_words: usize, pointer_count: usize, values: &[StructElement]) -> Vec<u64> {
    let step = data_words + pointer_count;
    let mut words = primitive(7, (values.len() * step) as u64, &[]);
    words.push(
        (values.len() as u64) << 2 | (data_words as u64) << 32 | (pointer_count as u64) << 48,
    );
    words.resize(2 + values.len() * step, 0);
    for (i, (data, ptrs)) in values.iter().enumerate() {
        assert!(data.len() <= data_words && ptrs.len() <= pointer_count);
        let start = 2 + i * step;
        words[start..start + data.len()].copy_from_slice(data);
        for (j, ptr) in ptrs.iter().enumerate() {
            append(&mut words, start + data_words + j, ptr);
        }
    }
    words
}

fn cap(index: u32) -> Vec<u64> {
    vec![3 | u64::from(index) << 32]
}

// The model has independently specified semantic values for these encodings.
fn shape(index: u64) -> Vec<u64> {
    let blob = |byte| primitive(2, 1, &[byte]);
    match index {
        0 => vec![0],
        1 => structure(&[], &[]),
        2 => structure(&[0, 0], &[vec![0], vec![0]]),
        3 => structure(&[42], &[]),
        4 => structure(&[42, 0], &[vec![0], vec![0]]),
        5 => structure(&[43], &[]),
        6 => structure(&[], &[cap(0)]),
        7 => structure(&[], &[cap(99)]),
        8 => structure(&[1], &[cap(0)]),
        9 => structure(&[], &[cap(0), vec![0]]),
        10 => structure(&[], &[cap(0), blob(1)]),
        11 => structure(&[], &[cap(99), blob(2)]),
        12 => structure(&[], &[blob(1), cap(0)]),
        13..=15 => primitive(0, index - 13, &[]),
        16 => primitive(2, 1, &[0xffff_ffff_ffff_ff01]),
        17 => blob(1),
        18 => blob(2),
        19 => primitive(3, 1, &[1]),
        20 => primitive(1, 3, &[0xfd]),
        21 => primitive(1, 3, &[5]),
        22 => primitive(1, 3, &[4]),
        23 => primitive(1, 4, &[5]),
        24 => cap(0),
        25 => cap(99),
        26 => pointers(&[cap(0), blob(1)]),
        27 => pointers(&[cap(99), blob(1)]),
        28 => pointers(&[cap(99), blob(2)]),
        29 => composite(1, 1, &[(vec![42], vec![cap(0)]), (vec![42], vec![blob(1)])]),
        30 => composite(
            2,
            2,
            &[(vec![42], vec![cap(99)]), (vec![42], vec![blob(1)])],
        ),
        31 => composite(
            1,
            1,
            &[(vec![42], vec![cap(99)]), (vec![43], vec![blob(1)])],
        ),
        32 => composite(0, 0, &[]),
        33 => pointers(&[]),
        34 => composite(0, 1, &[(vec![], vec![cap(1)])]),
        35 => pointers(&[cap(1)]),
        _ => panic!("unknown shape {index}"),
    }
}

struct Wire(Vec<Vec<Word>>);
impl Wire {
    fn new(segments: &[Vec<u64>]) -> Self {
        Self(
            segments
                .iter()
                .map(|segment| {
                    let mut words = Word::allocate_zeroed_vec(segment.len());
                    for (bytes, value) in Word::words_to_bytes_mut(&mut words)
                        .chunks_exact_mut(8)
                        .zip(segment)
                    {
                        bytes.copy_from_slice(&value.to_le_bytes());
                    }
                    words
                })
                .collect(),
        )
    }
    fn reader(&self, options: message::ReaderOptions) -> message::Reader<Vec<&[u8]>> {
        message::Reader::new(
            self.0.iter().map(|s| Word::words_to_bytes(s)).collect(),
            options,
        )
    }
}

fn compare(left: &[u64], right: &[u64]) -> capnp::Result<Equality> {
    compare_segments(
        &[left.to_vec()],
        &[right.to_vec()],
        message::ReaderOptions::new(),
        0,
    )
}

// Modes 1/2 also exercise the public raw struct/list entry points. AnyPointer
// list readers retain the physical layout when reading inline composites.
fn compare_segments(
    left: &[Vec<u64>],
    right: &[Vec<u64>],
    options: message::ReaderOptions,
    mode: u64,
) -> capnp::Result<Equality> {
    let left = Wire::new(left);
    let right = Wire::new(right);
    let left = left.reader(options);
    let right = right.reader(options);
    match mode {
        0 => left
            .get_root::<any_pointer::Reader>()?
            .equals(right.get_root()?),
        1 => raw::struct_equals(
            left.get_root::<node::Reader>()?,
            right.get_root::<node::Reader>()?,
        ),
        2 => raw::list_equals(
            left.get_root::<capnp::any_pointer_list::Reader>()?,
            right.get_root::<capnp::any_pointer_list::Reader>()?,
        ),
        _ => panic!(),
    }
}

#[test]
fn structural_comparison_preserves_kinds_padding_and_capability_uncertainty() {
    use Equality::*;
    for (a, b, result) in [
        (0, 0, Equal),
        (0, 1, NotEqual),
        (1, 2, Equal),
        (3, 4, Equal),
        (3, 5, NotEqual),
        (6, 7, UnknownContainsCapabilities),
        (6, 9, UnknownContainsCapabilities),
        (6, 8, NotEqual),
        (10, 11, NotEqual),
        (11, 12, NotEqual),
        (13, 14, NotEqual),
        (16, 17, Equal),
        (17, 19, NotEqual),
        (20, 21, Equal),
        (20, 22, NotEqual),
        (21, 23, NotEqual),
        (24, 25, UnknownContainsCapabilities),
        (26, 27, UnknownContainsCapabilities),
        (26, 28, NotEqual),
        (29, 30, UnknownContainsCapabilities),
        (29, 31, NotEqual),
        (32, 33, NotEqual),
        (34, 35, NotEqual),
    ] {
        assert_eq!(compare(&shape(a), &shape(b)).unwrap(), result, "{a} vs {b}");
        assert_eq!(compare(&shape(b), &shape(a)).unwrap(), result, "{b} vs {a}");
    }
}

#[test]
fn all_primitive_widths_float_bits_and_bit_boundaries_are_compared() {
    for (encoding, width) in [(2, 8u64), (3, 16), (4, 32), (5, 64)] {
        for count in [0, 1, 2, 7, 8, 9, 16, 17] {
            let words = (count * width).div_ceil(64);
            let zeros = primitive(encoding, count, &vec![0; words as usize]);
            assert_eq!(compare(&zeros, &zeros).unwrap(), Equality::Equal);
            if count > 0 {
                let mut changed = zeros.clone();
                let bit = (count - 1) * width;
                changed[1 + (bit / 64) as usize] |= 1 << (bit % 64);
                assert_eq!(compare(&zeros, &changed).unwrap(), Equality::NotEqual);
            }
        }
    }
    for count in 1u64..=129 {
        let zeros = primitive(1, count, &vec![0; count.div_ceil(64) as usize]);
        let mut changed = zeros.clone();
        let bit = count - 1;
        changed[1 + (bit / 64) as usize] |= 1 << (bit % 64);
        assert_eq!(compare(&zeros, &changed).unwrap(), Equality::NotEqual);
        if count % 64 != 0 {
            let mut padding = zeros.clone();
            *padding.last_mut().unwrap() |= u64::MAX << (count % 64);
            assert_eq!(compare(&zeros, &padding).unwrap(), Equality::Equal);
        }
    }
    let list = |bits| primitive(5, 1, &[bits]);
    assert_eq!(
        compare(&list(0.0f64.to_bits()), &list((-0.0f64).to_bits())).unwrap(),
        Equality::NotEqual
    );
    assert_eq!(
        compare(&list(f64::NAN.to_bits()), &list(f64::NAN.to_bits())).unwrap(),
        Equality::Equal
    );
    assert_eq!(
        compare(&list(f64::NAN.to_bits()), &list(f64::NAN.to_bits() ^ 1)).unwrap(),
        Equality::NotEqual
    );
}

#[test]
fn reader_limits_errors_and_short_circuiting_are_preserved() {
    let invalid = vec![7]; // Reserved pointer kind.
    assert!(compare(&invalid, &invalid).is_err());
    assert!(compare(&[1u64 << 32], &[1u64 << 32]).is_err()); // Missing struct data.
    let nested = structure(&[], std::slice::from_ref(&invalid));
    assert!(compare(&nested, &nested).is_err());
    let late_invalid = structure(&[], &[cap(0), invalid.clone()]);
    assert!(
        compare(&late_invalid, &late_invalid).is_err(),
        "unknown must not hide a later error"
    );
    assert_eq!(
        compare(
            &structure(&[1], std::slice::from_ref(&invalid)),
            &structure(&[2], &[invalid])
        )
        .unwrap(),
        Equality::NotEqual
    );
    // A self-referential struct cannot bypass limits through pointer identity.
    let cyclic = vec![1 << 48, (1 << 48) | 0xffff_fffc];
    assert!(compare(&cyclic, &cyclic).is_err());
    let one = shape(3);
    let mut options = message::ReaderOptions::new();
    options.traversal_limit_in_words = Some(1); // The root pointer consumes this word.
    assert!(compare_segments(
        std::slice::from_ref(&one),
        std::slice::from_ref(&one),
        options,
        0
    )
    .is_err());
    options = message::ReaderOptions::new();
    options.nesting_limit = 1;
    for words in [pointers(&[vec![0]]), composite(0, 0, &[(vec![], vec![])])] {
        assert!(compare_segments(
            std::slice::from_ref(&words),
            std::slice::from_ref(&words),
            options,
            0
        )
        .is_err());
    }
}

#[test]
fn aggregate_helpers_keep_unknown_fields_and_pointer_section_metadata() {
    let a = Wire::new(&[shape(10)]);
    let b = Wire::new(&[shape(11)]);
    let a = a.reader(message::ReaderOptions::new());
    let b = b.reader(message::ReaderOptions::new());
    let a = a.get_root::<node::Reader>().unwrap();
    let b = b.get_root::<node::Reader>().unwrap();
    assert_eq!(raw::struct_equals(a, b).unwrap(), Equality::NotEqual);
    assert_eq!(
        raw::list_equals(
            raw::get_struct_pointer_section(a),
            raw::get_struct_pointer_section(b)
        )
        .unwrap(),
        Equality::NotEqual
    );
    // Data past the chosen generated schema's fields still participates.
    let a = Wire::new(&[structure(&[0, 0, 0, 42], &[])]);
    let b = Wire::new(&[structure(&[0, 0, 0, 43], &[])]);
    let a = a.reader(message::ReaderOptions::new());
    let b = b.reader(message::ReaderOptions::new());
    assert_eq!(
        raw::struct_equals(
            a.get_root::<capnp::schema_capnp::type_::Reader>().unwrap(),
            b.get_root::<capnp::schema_capnp::type_::Reader>().unwrap()
        )
        .unwrap(),
        Equality::NotEqual
    );
    // Typed promotion does not turn physically different list encodings equal.
    let a = Wire::new(&[primitive(2, 1, &[1])]);
    let b = Wire::new(&[primitive(3, 1, &[1])]);
    let a = a.reader(message::ReaderOptions::new());
    let b = b.reader(message::ReaderOptions::new());
    assert_eq!(
        raw::list_equals(
            a.get_root::<capnp::primitive_list::Reader<u8>>().unwrap(),
            b.get_root::<capnp::primitive_list::Reader<u8>>().unwrap()
        )
        .unwrap(),
        Equality::NotEqual
    );
}

#[test]
fn generated_and_both_dynamic_apis_compare_the_same_storage() {
    use capnp::{
        dynamic_value::Reader as Compiled,
        introspect::Introspect,
        schema_loader::{dynamic, SchemaLoader},
    };
    use reproto_test_support::native_list_capnp::lists;
    let mut loader = SchemaLoader::default();
    loader
        .load_compiled_type_and_dependencies::<lists::Owned>()
        .unwrap();
    let id = lists::Owned::introspect()
        .as_struct_schema()
        .unwrap()
        .get_proto()
        .get_id();
    let mut a = message::Builder::new_default();
    let mut b = message::Builder::new_default();
    for message in [&mut a, &mut b] {
        let mut numbers = message.init_root::<lists::Builder>().init_numbers(2);
        numbers.set(1, 7);
    }
    let native = a.get_root_as_reader::<lists::Reader>().unwrap();
    let Compiled::Struct(compiled) = Compiled::from(native) else {
        panic!()
    };
    let loaded =
        dynamic::Reader::new(b.get_root_as_reader().unwrap(), loader.get(id).unwrap()).unwrap();
    assert_eq!(
        raw::struct_equals(native, compiled).unwrap(),
        Equality::Equal
    );
    assert_eq!(
        raw::struct_equals(loaded.clone(), compiled).unwrap(),
        Equality::Equal
    );
    // Different nominal schema IDs do not reject physical comparison.
    let Compiled::Struct(other) = Compiled::from(a.get_root_as_reader::<node::Reader>().unwrap())
    else {
        panic!()
    };
    assert_eq!(
        raw::struct_equals(compiled, other).unwrap(),
        Equality::Equal
    );
    let Compiled::List(compiled_list) = compiled.get_named("numbers").unwrap() else {
        panic!()
    };
    let dynamic::Value::List(loaded_list) = loaded.get_named("numbers").unwrap() else {
        panic!()
    };
    assert_eq!(
        raw::list_equals(compiled_list, loaded_list).unwrap(),
        Equality::Equal
    );
    assert_eq!(
        raw::list_equals(compiled_list, native.get_numbers().unwrap()).unwrap(),
        Equality::Equal
    );
    b.get_root::<lists::Builder>()
        .unwrap()
        .get_numbers()
        .unwrap()
        .set(1, 8);
    let loaded =
        dynamic::Reader::new(b.get_root_as_reader().unwrap(), loader.get(id).unwrap()).unwrap();
    assert_eq!(
        raw::struct_equals(compiled, loaded).unwrap(),
        Equality::NotEqual
    );
}

#[test]
fn builder_comparison_never_consults_capability_hooks_or_retains_authority() {
    use capnp::{
        capability::{Promise, Request},
        private::capability::{ClientHook, ParamsHook, ResultsHook},
        traits::ImbueMut,
        Error,
    };
    use std::rc::Rc;
    struct Opaque(Rc<()>);
    impl ClientHook for Opaque {
        fn add_ref(&self) -> Box<dyn ClientHook> {
            panic!("comparison cloned a capability")
        }
        fn new_call(
            &self,
            _: u64,
            _: u16,
            _: Option<capnp::MessageSize>,
        ) -> Request<any_pointer::Owned, any_pointer::Owned> {
            panic!("comparison called a capability")
        }
        fn call(
            &self,
            _: u64,
            _: u16,
            _: Box<dyn ParamsHook>,
            _: Box<dyn ResultsHook>,
        ) -> Promise<(), Error> {
            panic!("comparison called a capability")
        }
        fn get_brand(&self) -> usize {
            panic!("comparison queried a brand")
        }
        fn get_ptr(&self) -> usize {
            panic!("comparison queried identity")
        }
        fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
            panic!("comparison queried resolution")
        }
        fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, Error>> {
            panic!("comparison polled resolution")
        }
        fn when_resolved(&self) -> Promise<(), Error> {
            panic!("comparison polled resolution")
        }
    }
    impl Drop for Opaque {
        fn drop(&mut self) {
            assert_eq!(Rc::strong_count(&self.0), 1);
        }
    }
    let alive = Rc::new(());
    let weak = Rc::downgrade(&alive);
    let mut message = message::Builder::new_default();
    let mut caps = capnp::private::layout::CapTable::default();
    let mut root = message.init_root::<any_pointer::Builder>();
    root.imbue_mut(&mut caps);
    root.set_as_capability(Box::new(Opaque(alive)));
    for _ in 0..3 {
        assert_eq!(
            root.equals(root.as_reader()).unwrap(),
            Equality::UnknownContainsCapabilities
        );
    }
    drop(message);
    drop(caps);
    assert!(weak.upgrade().is_none());
}
