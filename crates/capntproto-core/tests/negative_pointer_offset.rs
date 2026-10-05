/// Regression test for a bug where a struct pointer with a large negative
/// signed offset would cause `ReaderArenaImpl::check_offset` to panic with
/// a `TryFromIntError` (from `usize::try_from` on a negative `i64`) instead
/// of returning `MessageContainsOutOfBoundsPointer`.
///
/// Originally discovered by `cargo fuzz run test_all_types`.
#[test]
pub fn negative_root_pointer_offset() {
    // Root struct pointer whose signed 30-bit offset field decodes to a
    // large negative value, pointing well before the start of the segment.
    let segment: &[capnp::Word] = &[
        capnp::word(0x00, 0x00, 0x6d, 0x97, 0x01, 0x00, 0x00, 0x00),
        capnp::word(0x00, 0x00, 0x6d, 0x6d, 0x6d, 0x6d, 0xff, 0x00),
    ];

    let segments = &[capnp::Word::words_to_bytes(segment)];
    let segment_array = capnp::message::SegmentArray::new(segments);
    let message = capnp::message::Reader::new(segment_array, Default::default());
    let root: capnp::any_pointer::Reader = message.get_root().unwrap();

    // Before the fix, this panicked in `check_offset` instead of returning
    // an out-of-bounds error.
    let result = root.target_size();

    assert!(result.is_err());

    // Struct decoding combines the relative-offset and complete-target checks.
    // Exercise signed extremes, an empty target exactly at the end, targets
    // before/after the segment, both struct sections, and traversal accounting.
    for offset in [i32::MIN >> 2, -2, -1, 0, 1, 2, 3, i32::MAX >> 2] {
        for (data, pointers) in [(0u16, 0u16), (1, 0), (0, 1), (1, 1), (u16::MAX, u16::MAX)] {
            let mut words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 3];
            let tag = u64::from((offset as u32) << 2)
                | (u64::from(data) << 32)
                | (u64::from(pointers) << 48);
            capnp::Word::words_to_bytes_mut(&mut words)[..8].copy_from_slice(&tag.to_le_bytes());
            let segments = [capnp::Word::words_to_bytes(&words)];
            let target = i64::from(offset) + 1;
            let size = usize::from(data) + usize::from(pointers);
            let valid = (0..=3).contains(&target) && target + size as i64 <= 3;
            for limit in [1, 2, 3] {
                let mut options = capnp::message::ReaderOptions::new();
                options.traversal_limit_in_words(Some(limit));
                let message = capnp::message::Reader::new(&segments[..], options);
                let result = message.get_root::<capnp::schema_capnp::node::Reader<'_>>();
                // An all-zero root is null and consumes only the root word.
                let expected = if tag == 0 {
                    None
                } else if !valid {
                    Some(capnp::ErrorKind::MessageContainsOutOfBoundsPointer)
                } else if 1 + size > limit {
                    Some(capnp::ErrorKind::ReadLimitExceeded)
                } else {
                    None
                };
                assert_eq!(
                    result.err().map(|e| e.kind),
                    expected,
                    "offset={offset} data={data} pointers={pointers} limit={limit}"
                );
            }
        }
    }
}
