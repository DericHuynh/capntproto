use super::*;
use crate::{
    any_pointer,
    message::{FixedBuilder, Reader, ReaderOptions},
    private::layout::ElementSize,
    Word,
};
fn words<const N: usize>(values: [u64; N]) -> [Word; N] {
    values.map(|v| {
        let b = v.to_le_bytes();
        crate::word(b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7])
    })
}

#[test]
fn mapped_copy_of_absent_pointer_clears_old_content_without_backing_word() {
    let source = words([0]);
    let segments = [Word::words_to_bytes(&source)];
    let reader = Reader::new(&segments[..], ReaderOptions::new());
    let root = reader.get_root::<any_pointer::Reader<'_>>().unwrap();
    // The encoded null root has a word; an absent struct field has none.
    // Both must be valid null values, including with an empty capability map.
    for source in [root, root.pointer_field(19).unwrap()] {
        let mut buffer = words([0; 16]);
        let mut output = FixedBuilder::new(&mut buffer).unwrap();
        output.try_set_root_text("previous").unwrap();
        output
            .try_copy_root_with_capabilities(source, IndexMap::identity(0))
            .unwrap();
        assert!(output
            .get_root_as_reader::<any_pointer::Reader<'_>>()
            .unwrap()
            .is_null());
        output.try_set_root_text("reused").unwrap();
        assert_eq!(
            output
                .get_root_as_reader::<crate::text::Reader<'_>>()
                .unwrap(),
            "reused"
        );
    }
}
fn source() -> [Word; 10] {
    words([
        0x0003_0001_0000_0000,
        42,
        3,
        0x0000_0016_0000_0005,
        0x0000_0017_0000_0009,
        0x0000_0001_0000_0003,
        3,
        0x0001_0001_0000_0004,
        99,
        0x0000_0002_0000_0003,
    ])
}

#[test]
fn identity_mapping_checks_bounds_without_materializing_an_index_table() {
    for count in [0, 1, 17, u32::MAX] {
        let mapping = IndexMap::identity(count);
        assert!(mapping.remap(count).is_err());
        if count != 0 {
            assert_eq!(mapping.remap(count - 1).unwrap(), count - 1);
        }
    }
    let source = source();
    let segments = [Word::words_to_bytes(&source)];
    let reader = Reader::new(&segments[..], ReaderOptions::new());
    let content = reader.get_root::<any_pointer::Reader<'_>>().unwrap();
    let mut buffer = words([0; 32]);
    let mut output = FixedBuilder::new(&mut buffer).unwrap();
    output.try_set_root_text("old").unwrap();
    assert!(output
        .try_copy_root_with_capabilities(content, IndexMap::identity(2))
        .is_err());
    assert_eq!(
        output
            .get_root_as_reader::<crate::text::Reader<'_>>()
            .unwrap(),
        "old"
    );
    output
        .try_copy_root_with_capabilities(content, IndexMap::identity(3))
        .unwrap();
    let root = output
        .get_root_as_reader::<any_pointer::Reader<'_>>()
        .unwrap();
    root.validate_capability_indices(3).unwrap();
    assert_eq!(
        root.pointer_field(0).unwrap().capability_index().unwrap(),
        0
    );
}

#[test]
fn empty_mapped_copy_rewinds_after_a_nested_invalid_capability() {
    let source = source();
    let segments = [Word::words_to_bytes(&source)];
    let reader = Reader::new(&segments[..], ReaderOptions::new());
    let content = reader.get_root::<any_pointer::Reader<'_>>().unwrap();
    let mut buffer = words([0; 10]);
    let mut output = FixedBuilder::new(&mut buffer).unwrap();
    // Two valid capability pointers precede the invalid index in the final
    // nested struct. Each failed attempt must reclaim the partially copied tree.
    for _ in 0..3 {
        let error = output
            .try_copy_root_with_capabilities(content, IndexMap::identity(2))
            .unwrap_err();
        assert_eq!(
            error.kind,
            ErrorKind::MessageContainsInvalidCapabilityPointer
        );
        assert_eq!(output.size_in_words(), 1);
        assert!(output
            .get_root_as_reader::<any_pointer::Reader<'_>>()
            .unwrap()
            .is_null());
    }
    output
        .try_copy_root_with_capabilities(content, IndexMap::new(&[2, 0, 1], 3).unwrap())
        .unwrap();
    assert_eq!(output.size_in_words(), 10);
    check(output.get_root_as_reader().unwrap());
}
fn check(content: any_pointer::Reader<'_>) {
    let root = content.reader.get_struct(None).unwrap();
    assert_eq!(root.get_data_field::<u64>(0), 42);
    assert_eq!(root.get_pointer_field(0).get_capability_index().unwrap(), 2);
    let pointers = root
        .get_pointer_field(1)
        .get_list(ElementSize::Pointer, None)
        .unwrap();
    assert_eq!(pointers.len(), 2);
    assert_eq!(
        pointers
            .get_pointer_element(0)
            .get_capability_index()
            .unwrap(),
        0
    );
    assert_eq!(
        pointers
            .get_pointer_element(1)
            .get_capability_index()
            .unwrap(),
        2
    );
    let list = root
        .get_pointer_field(2)
        .get_list(ElementSize::InlineComposite, None)
        .unwrap();
    let value = list.get_struct_element(0);
    assert_eq!(value.get_data_field::<u64>(0), 99);
    assert_eq!(
        value.get_pointer_field(0).get_capability_index().unwrap(),
        1
    );
    content.validate_capability_indices(3).unwrap();
}

#[test]
fn recursive_remapping_preserves_unknown_data_and_every_pointer_shape() {
    let source = source();
    let landing = words([0x0000_0001_0000_0002]); // Single-far to segment one's root.
    for far in [false, true] {
        let segments = if far {
            [
                Word::words_to_bytes(&landing),
                Word::words_to_bytes(&source),
            ]
        } else {
            [Word::words_to_bytes(&source), &[]]
        };
        let reader = Reader::new(&segments[..], ReaderOptions::new());
        let content = reader.get_root::<any_pointer::Reader<'_>>().unwrap();
        let mut passed = 0;
        let mut rejected = 0;
        for capacity in 4..18 {
            let mut buffer = words([0; 18]);
            let mut output = FixedBuilder::new(&mut buffer[..capacity]).unwrap();
            output.try_set_root_text("old").unwrap();
            let size = output.size_in_words();
            let result = output
                .try_copy_root_with_capabilities(content, IndexMap::new(&[2, 0, 1], 3).unwrap());
            if let Err(e) = result {
                rejected += 1;
                assert!(
                    matches!(e.kind, ErrorKind::InsufficientCapacity { .. }),
                    "{e:?}"
                );
                assert_eq!(output.size_in_words(), size);
                assert_eq!(
                    output
                        .get_root_as_reader::<crate::text::Reader<'_>>()
                        .unwrap(),
                    "old"
                );
            } else {
                passed += 1;
                check(output.get_root_as_reader().unwrap());
            }
        }
        assert!(passed != 0 && rejected != 0);
    }
}

#[test]
fn missing_mapping_and_traversal_failure_rewind_the_whole_copy() {
    let source = source();
    let segments = [Word::words_to_bytes(&source)];
    for (mapping, nesting) in [(&[2, 0][..], 64), (&[2, 0, 1][..], 1)] {
        let reader = Reader::new(
            &segments[..],
            ReaderOptions {
                nesting_limit: nesting,
                ..Default::default()
            },
        );
        let mut buffer = words([0; 32]);
        let mut output = FixedBuilder::new(&mut buffer).unwrap();
        output.try_set_root_text("old").unwrap();
        let size = output.size_in_words();
        let e = output
            .try_copy_root_with_capabilities(
                reader.get_root().unwrap(),
                IndexMap::new(mapping, 3).unwrap(),
            )
            .unwrap_err();
        assert_eq!(
            e.kind,
            if nesting == 1 {
                ErrorKind::MessageIsTooDeeplyNestedOrContainsCycles
            } else {
                ErrorKind::MessageContainsInvalidCapabilityPointer
            }
        );
        assert_eq!(output.size_in_words(), size);
        assert_eq!(
            output
                .get_root_as_reader::<crate::text::Reader<'_>>()
                .unwrap(),
            "old"
        );
        let unrestricted = Reader::new(&segments[..], Default::default());
        output
            .try_copy_root_with_capabilities(
                unrestricted.get_root().unwrap(),
                IndexMap::new(&[2, 0, 1], 3).unwrap(),
            )
            .unwrap();
        check(output.get_root_as_reader().unwrap());
    }
    assert!(IndexMap::new(&[0], 0).is_err());
    assert!(IndexMap::new(&[u32::MAX], u32::MAX).is_err());
}

#[test]
fn explicit_mapping_is_required_even_with_alloc_enabled() {
    let source = words([3]);
    let segments = [Word::words_to_bytes(&source)];
    let reader = Reader::new(&segments[..], Default::default());
    let mut buffer = words([0; 8]);
    let mut output = FixedBuilder::new(&mut buffer).unwrap();
    output.try_set_root_text("old").unwrap();
    assert!(output.try_copy_root(reader.get_root().unwrap()).is_err());
    assert_eq!(
        output
            .get_root_as_reader::<crate::text::Reader<'_>>()
            .unwrap(),
        "old"
    );
    output
        .try_copy_root_with_capabilities(
            reader.get_root().unwrap(),
            IndexMap::new(&[2], 3).unwrap(),
        )
        .unwrap();
    assert_eq!(
        output
            .get_root_as_reader::<any_pointer::Reader<'_>>()
            .unwrap()
            .capability_index()
            .unwrap(),
        2
    );
}
