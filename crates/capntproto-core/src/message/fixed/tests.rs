use super::*;
const ZERO: Word = crate::word(0, 0, 0, 0, 0, 0, 0, 0);
const DIRTY: Word = crate::word(0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5);

#[test]
fn empty_copy_stage_uses_exact_capacity_and_reclaims_partial_failures() {
    let mut source_words = [ZERO; 2];
    let mut source = FixedBuilder::new(&mut source_words).unwrap();
    source.try_set_root_data(b"12345678").unwrap();
    let content = source
        .get_root_as_reader::<crate::any_pointer::Reader<'_>>()
        .unwrap();
    for mapped in [false, true] {
        for capacity in [1, 2] {
            let mut words = [DIRTY; 2];
            let mut builder = FixedBuilder::new(&mut words[..capacity]).unwrap();
            let result = if mapped {
                builder.try_copy_root_with_capabilities(
                    content,
                    crate::wire_capability::IndexMap::identity(0),
                )
            } else {
                builder.try_copy_root(content)
            };
            if capacity == 1 {
                assert!(matches!(
                    result.unwrap_err().kind,
                    ErrorKind::InsufficientCapacity { .. }
                ));
                assert_eq!(builder.size_in_words(), 1);
                assert_eq!(builder.get_segments_for_output()[0], &[0; 8]);
            } else {
                result.unwrap();
                assert_eq!(builder.size_in_words(), 2);
                assert_eq!(
                    builder
                        .get_root_as_reader::<crate::data::Reader<'_>>()
                        .unwrap(),
                    b"12345678"
                );
            }
        }
    }
    let mut words = [DIRTY; 4];
    let mut builder = FixedBuilder::new(&mut words).unwrap();
    for _ in 0..3 {
        let error = builder
            .root()
            .stage(|p| {
                p.try_init_text(16)?;
                Err(crate::Error::from_kind(ErrorKind::TypeMismatch))
            })
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::TypeMismatch);
        assert_eq!(builder.size_in_words(), 1);
        assert_eq!(builder.get_segments_for_output()[0], &[0; 8]);
    }
    builder.try_copy_root(content).unwrap();
    assert_eq!(builder.size_in_words(), 2);
}

#[test]
fn list_sizes_reserve_exactly_and_reject_without_publication() {
    use crate::field_api::FixedElement;
    fn check<E: FixedElement>(bits: usize) {
        for capacity in 1..=12 {
            for len in 0..=65 {
                let mut words = [DIRTY; 12];
                let mut builder = FixedBuilder::new(&mut words[..capacity]).unwrap();
                let result = builder.try_init_root_list::<E>(len);
                let needed = 1 + (len * bits).div_ceil(64);
                if needed > capacity {
                    assert!(matches!(
                        result.err().unwrap().kind,
                        ErrorKind::InsufficientCapacity { .. }
                    ));
                    assert_eq!(builder.size_in_words(), 1);
                    assert_eq!(builder.get_segments_for_output()[0], &[0; 8]);
                } else {
                    assert_eq!(result.unwrap().len(), len);
                    assert_eq!(builder.size_in_words(), needed);
                    assert_eq!(
                        builder
                            .try_init_root_list::<E>(usize::MAX)
                            .err()
                            .unwrap()
                            .kind,
                        ErrorKind::LengthOverflow
                    );
                    assert_eq!(builder.try_get_root_list::<E>().unwrap().len(), len);
                    assert_eq!(builder.size_in_words(), needed);
                }
            }
        }
    }
    check::<()>(0);
    check::<bool>(1);
    check::<u8>(8);
    check::<u16>(16);
    check::<u32>(32);
    check::<u64>(64);
    check::<crate::field_api::Text>(64);
}

#[test]
fn inline_tag_and_word_counts_are_checked_before_reserving() {
    let mut words = [ZERO; 5];
    let mut builder = FixedBuilder::new(&mut words).unwrap();
    builder.try_set_root_text("old").unwrap();
    for (count, size) in [
        (
            1 << 30,
            layout::StructSize {
                data: 0,
                pointers: 0,
            },
        ),
        (
            1 << 29,
            layout::StructSize {
                data: 1,
                pointers: 0,
            },
        ),
        (
            u32::MAX,
            layout::StructSize {
                data: u16::MAX,
                pointers: u16::MAX,
            },
        ),
    ] {
        assert_eq!(
            builder
                .root()
                .try_init_struct_list(count, size)
                .err()
                .unwrap()
                .kind,
            ErrorKind::LengthOverflow
        );
        assert_eq!(builder.size_in_words(), 2);
        assert_eq!(
            builder
                .get_root_as_reader::<crate::text::Reader<'_>>()
                .unwrap(),
            "old"
        );
    }
}

#[cfg(feature = "std")]
#[test]
fn unpublished_copy_stage_is_reclaimed_when_callback_unwinds() {
    for populated in [false, true] {
        let mut words = [DIRTY; 16];
        let mut builder = FixedBuilder::new(&mut words).unwrap();
        if populated {
            builder.try_set_root_text("old").unwrap();
        }
        let before = builder.size_in_words();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = builder.root().stage(|p| {
                p.try_init_text(16)?;
                panic!("injected unwind before publication");
            });
        }));
        assert!(panic.is_err());
        assert_eq!(builder.size_in_words(), before);
        assert_eq!(
            builder
                .get_root_as_reader::<crate::text::Reader<'_>>()
                .unwrap(),
            if populated { "old" } else { "" }
        );
        builder
            .root()
            .stage(|p| {
                p.try_init_data(8)?;
                Ok(())
            })
            .unwrap();
    }
}

#[test]
fn every_capacity_and_blob_length_reserves_exactly_or_preserves_the_message() {
    let input = [b'x'; 128];
    for capacity in 1..=18 {
        for size in 0..=input.len() {
            for text in [false, true] {
                let mut words = [DIRTY; 18];
                {
                    let mut builder = FixedBuilder::new(&mut words[..capacity]).unwrap();
                    assert_eq!(builder.remaining_words(), capacity as u32 - 1);
                    let result = if text {
                        builder.try_set_root_text(core::str::from_utf8(&input[..size]).unwrap())
                    } else {
                        builder.try_set_root_data(&input[..size])
                    };
                    let needed = 1 + (size + usize::from(text)).div_ceil(8);
                    if needed > capacity {
                        assert!(matches!(
                            result.unwrap_err().kind,
                            ErrorKind::InsufficientCapacity { .. }
                        ));
                        assert_eq!(builder.size_in_words(), 1);
                        assert_eq!(builder.get_segments_for_output()[0], &[0; 8]);
                    } else {
                        result.unwrap();
                        assert_eq!(builder.size_in_words(), needed);
                        let read = if text {
                            builder
                                .get_root_as_reader::<crate::text::Reader<'_>>()
                                .unwrap()
                                .as_bytes()
                        } else {
                            builder
                                .get_root_as_reader::<crate::data::Reader<'_>>()
                                .unwrap()
                        };
                        assert_eq!(read, &input[..size]);
                        // Full/insufficient reserve attempts must not erase the old root.
                        let used = builder.size_in_words();
                        let mut before = [0; 18 * 8];
                        before[..used * 8].copy_from_slice(builder.get_segments_for_output()[0]);
                        assert!(builder.try_set_root_data(&[1; 144]).is_err());
                        assert_eq!(builder.size_in_words(), used);
                        assert_eq!(builder.get_segments_for_output()[0], &before[..used * 8]);
                    }
                    builder.reset();
                    assert_eq!(builder.size_in_words(), 1);
                    builder.try_set_root_data(&[]).unwrap();
                    assert!(builder
                        .get_root_as_reader::<crate::data::Reader<'_>>()
                        .unwrap()
                        .is_empty());
                }
                // Drop/reset clear reservations, not untouched caller capacity.
                assert_eq!(words[0], ZERO);
                assert!(words[capacity..].iter().all(|word| *word == DIRTY));
            }
        }
    }
}

#[test]
fn construction_and_wire_length_errors_do_not_modify_storage() {
    let mut empty = [];
    assert_eq!(
        FixedBuilder::new(&mut empty).err().unwrap().kind,
        ErrorKind::InsufficientCapacity {
            requested_words: 1,
            available_words: 0
        }
    );
    let mut words = [DIRTY; 4];
    if !cfg!(feature = "unaligned") {
        assert_eq!(
            FixedBuilder::from_bytes(&mut Word::words_to_bytes_mut(&mut words)[1..])
                .err()
                .unwrap()
                .kind,
            ErrorKind::UnalignedSegment
        );
        assert!(words.iter().all(|word| *word == DIRTY));
    }
    for size in [(1 << 29) - 1, 1 << 29, u32::MAX] {
        let mut builder = FixedBuilder::new(&mut words).unwrap();
        builder.try_set_root_text("old").unwrap();
        let used = builder.size_in_words();
        assert_eq!(
            builder.root().try_init_text(size).err().unwrap().kind,
            ErrorKind::LengthOverflow
        );
        if size >= 1 << 29 {
            assert_eq!(
                builder.root().try_init_data(size).err().unwrap().kind,
                ErrorKind::LengthOverflow
            );
        }
        assert_eq!(builder.size_in_words(), used);
        assert_eq!(
            builder
                .get_root_as_reader::<crate::text::Reader<'_>>()
                .unwrap(),
            "old"
        );
    }
}

#[test]
fn output_uses_standard_framing_and_drop_zeroes_used_words() {
    let mut words = [DIRTY; 5];
    {
        let mut builder =
            FixedBuilder::from_bytes(&mut Word::words_to_bytes_mut(&mut words)[..35]).unwrap();
        assert_eq!(builder.capacity_words(), 4);
        builder.try_set_root_text("payload").unwrap();
        let mut wire = [ZERO; 4];
        let mut output = Word::words_to_bytes_mut(&mut wire);
        crate::serialize::write_message_segments(&mut output, &builder).unwrap();
        let remaining = output.len();
        let mut input = &Word::words_to_bytes(&wire)[..32 - remaining];
        let reader =
            crate::serialize::read_message_from_flat_slice_no_alloc(&mut input, Default::default())
                .unwrap();
        assert_eq!(
            reader.get_root::<crate::text::Reader<'_>>().unwrap(),
            "payload"
        );
        assert!(input.is_empty());
    }
    assert_eq!(words[..2], [ZERO; 2]);
    assert_eq!(words[2..], [DIRTY; 3]);
}

#[test]
fn consumed_builder_publishes_segment_without_clearing_its_buffer() {
    let mut words = [crate::word(0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5); 8];
    let length;
    {
        let mut builder = super::FixedBuilder::new(&mut words).unwrap();
        builder.try_set_root_text("published").unwrap();
        length = builder.size_in_words() * 8;
        let segment = builder.into_segment();
        assert_eq!(segment.len(), length);
        let segments = [segment];
        let reader = crate::message::Reader::new(&segments[..], Default::default());
        assert_eq!(
            reader.get_root::<crate::text::Reader<'_>>().unwrap(),
            "published"
        );
    }
    assert!(crate::Word::words_to_bytes(&words)[length..]
        .iter()
        .all(|&byte| byte == 0xa5));
    let builder = super::FixedBuilder::new(&mut words).unwrap();
    assert_eq!(builder.into_segment(), &[0; 8]);
}
