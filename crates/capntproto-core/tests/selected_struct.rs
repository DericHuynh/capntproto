#![cfg(feature = "alloc")]
use capnp::{message, schema_capnp::type_, traits::IntoInternalStructReader, ErrorKind};

fn source() -> message::Reader<capnp::serialize::OwnedSegments> {
    let mut builder = message::Builder::new_default();
    builder
        .init_root::<type_::Builder>()
        .init_list()
        .init_element_type()
        .set_uint64(());
    let bytes = capnp::serialize::write_message_to_words(&builder);
    capnp::serialize::read_message(bytes.as_slice(), Default::default()).unwrap()
}

#[test]
fn scoped_root_cache_borrows_inline_storage_and_shares_its_budget() {
    struct Inline([capnp::Word; 2], std::cell::Cell<bool>);
    impl message::ReaderSegments for Inline {
        fn get_segment(&self, id: u32) -> Option<&[u8]> {
            (id == 0).then(|| {
                capnp::Word::words_to_bytes(&self.0[..if self.1.replace(true) { 1 } else { 2 }])
            })
        }
    }
    let reader = message::Reader::new(
        Inline(
            [
                capnp::word(0, 0, 0, 0, 1, 0, 0, 0),
                capnp::word(42, 0, 0, 0, 0, 0, 0, 0),
            ],
            std::cell::Cell::new(false),
        ),
        message::ReaderOptions {
            traversal_limit_in_words: Some(2),
            ..Default::default()
        },
    );
    let address = reader.get_segments().0.as_ptr();
    let reader = Box::new(reader);
    assert_ne!(address, reader.get_segments().0.as_ptr());
    let value = reader
        .with_root(|root| {
            let root = root.get_as::<capnp::any_struct::Reader>()?;
            Ok::<_, capnp::Error>(root.into_internal_struct_reader().get_data_field::<u64>(0))
        })
        .unwrap()
        .unwrap();
    assert_eq!(
        value, 42,
        "one live borrow retains its first segment despite a changing provider"
    );
    assert_eq!(
        reader.with_root(|_| ()).unwrap_err().kind,
        ErrorKind::ReadLimitExceeded,
        "the cache shares, rather than resets, the reader budget"
    );
    let reader = message::Reader::new((*reader).into_segments(), Default::default());
    assert_eq!(
        reader
            .with_root(|root| root.get_as::<capnp::any_struct::Reader>().map(|_| ()))
            .unwrap()
            .unwrap_err()
            .kind,
        ErrorKind::MessageContainsOutOfBoundsPointer,
        "a later borrow rechecks the newly supplied shorter segment"
    );
}

#[test]
fn on_demand_selection_retains_existing_views_and_rejects_foreign_arenas() {
    let cache = message::StructReaderCache::new(source());
    assert!(cache.get().is_none());
    let foreign = source();
    assert!(cache
        .select(
            foreign
                .get_root::<type_::Reader>()
                .unwrap()
                .into_internal_struct_reader()
        )
        .is_err());
    assert!(cache.get().is_none());
    let original = cache.message().get_root::<type_::Reader>().unwrap();
    let type_::List(list) = original.which().unwrap() else {
        panic!()
    };
    let selected = list.get_element_type().unwrap();
    cache
        .select(selected.into_internal_struct_reader())
        .unwrap();
    assert!(matches!(original.which(), Ok(type_::List(_))));
    assert!(matches!(selected.which(), Ok(type_::Uint64(()))));
    let cache = Box::new(cache);
    for _ in 0..3 {
        assert!(matches!(
            type_::Reader::from(cache.get().unwrap()).which(),
            Ok(type_::Uint64(()))
        ));
    }
    assert!(matches!(
        cache.message().get_root::<type_::Reader>().unwrap().which(),
        Ok(type_::List(_))
    ));
    let source = source();
    let view = source
        .get_root::<type_::Reader>()
        .unwrap()
        .into_internal_struct_reader();
    let words = 1
        + (view.get_data_section_size() as usize).div_ceil(64)
        + view.get_pointer_section_size() as usize;
    let cache = message::StructReaderCache::new(message::Reader::new(
        source.into_segments(),
        message::ReaderOptions {
            traversal_limit_in_words: Some(words),
            ..Default::default()
        },
    ));
    let root = cache.message().get_root::<type_::Reader>().unwrap();
    cache.select(root.into_internal_struct_reader()).unwrap();
    for _ in 0..3 {
        assert!(matches!(
            type_::Reader::from(cache.get().unwrap()).which(),
            Ok(type_::List(_))
        ));
    }
    assert!(
        cache.message().get_root::<type_::Reader>().is_err(),
        "selection does not reset the spent traversal budget"
    );
}

#[test]
fn exact_framing_rejects_trailing_truncated_and_over_limit_frames() {
    let words = [
        capnp::word(0, 0, 0, 0, 1, 0, 0, 0),
        capnp::word(0, 0, 0, 0, 0, 0, 0, 0),
        capnp::word(0, 0, 0, 0, 0, 0, 0, 0),
    ];
    let bytes = capnp::Word::words_to_bytes(&words);
    use capnp::serialize::NoAllocBufferSegments;
    assert!(NoAllocBufferSegments::from_buffer_exact(&bytes[..16], Default::default()).is_ok());
    for length in 0..=24 {
        if length != 16 {
            assert!(
                NoAllocBufferSegments::from_buffer_exact(&bytes[..length], Default::default())
                    .is_err()
            );
        }
    }
    assert!(NoAllocBufferSegments::from_buffer_exact(
        &bytes[..16],
        message::ReaderOptions {
            traversal_limit_in_words: Some(0),
            ..Default::default()
        }
    )
    .is_err());
}

#[test]
fn selected_struct_keeps_its_path_and_storage_across_moves() {
    let selected = message::CheckedStructReader::select(source(), |root| {
        let type_::List(list) = type_::Reader::from(root).which()? else {
            panic!()
        };
        Ok(list.get_element_type()?.into_internal_struct_reader())
    })
    .unwrap();
    let mut owners = Vec::new();
    owners.push(selected);
    for _ in 0..100 {
        owners.reserve(owners.capacity() + 1);
        if owners.capacity() > 128 {
            break;
        }
    }
    let selected = owners.pop().unwrap();
    for _ in 0..100 {
        assert!(matches!(
            type_::Reader::from(selected.root()).which(),
            Ok(type_::Uint64(()))
        ));
    }
    let original = selected.into_reader();
    assert!(matches!(
        original.get_root::<type_::Reader>().unwrap().which(),
        Ok(type_::List(_))
    ));
}

#[test]
fn selected_struct_rebases_inline_storage_and_retains_the_read_budget() {
    struct Inline([capnp::Word; 4]);
    impl message::ReaderSegments for Inline {
        fn get_segment(&self, id: u32) -> Option<&[u8]> {
            (id == 0).then(|| capnp::Word::words_to_bytes(&self.0))
        }
    }
    let source = message::Reader::new(
        Inline([
            capnp::word(0, 0, 0, 0, 1, 0, 1, 0),
            capnp::word(42, 0, 0, 0, 0, 0, 0, 0),
            capnp::word(1, 0, 0, 0, 34, 0, 0, 0),
            capnp::word(b'a', b'b', b'c', 0, 0, 0, 0, 0),
        ]),
        message::ReaderOptions {
            traversal_limit_in_words: Some(4),
            ..Default::default()
        },
    );
    let selected = message::CheckedStructReader::new(source).unwrap();
    let old_address = selected.root().get_data_section_as_blob().as_ptr() as usize;
    let selected = Box::new(selected);
    assert_ne!(
        selected.root().get_data_section_as_blob().as_ptr() as usize,
        old_address
    );
    assert_eq!(selected.root().get_data_field::<u64>(0), 42);
    assert_eq!(
        selected.root().get_pointer_field(0).get_text(None).unwrap(),
        "abc"
    );
    assert!(
        selected.root().get_pointer_field(0).get_text(None).is_err(),
        "selection and first child already spent the traversal budget"
    );
    let original = (*selected).into_reader();
    assert!(original.get_root::<capnp::any_pointer::Reader>().is_err());
}

#[test]
fn selected_struct_rejects_a_segment_provider_that_shortens_its_storage() {
    struct Changing {
        shorter: std::rc::Rc<std::cell::Cell<bool>>,
        words: [capnp::Word; 2],
    }
    impl message::ReaderSegments for Changing {
        fn get_segment(&self, id: u32) -> Option<&[u8]> {
            (id == 0).then(|| {
                capnp::Word::words_to_bytes(&self.words[..if self.shorter.get() { 1 } else { 2 }])
            })
        }
    }
    let shorter = std::rc::Rc::new(std::cell::Cell::new(false));
    let source = message::Reader::new(
        Changing {
            shorter: shorter.clone(),
            words: [
                capnp::word(0, 0, 0, 0, 1, 0, 0, 0),
                capnp::word(42, 0, 0, 0, 0, 0, 0, 0),
            ],
        },
        Default::default(),
    );
    let selected = message::CheckedStructReader::new(source).unwrap();
    assert_eq!(selected.root().get_data_field::<u64>(0), 42);
    shorter.set(true);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| selected.root())).is_err());
}

#[test]
fn selection_rejects_foreign_storage_and_preserves_nesting_limits() {
    // Static foreign storage can satisfy the selector's higher-ranked lifetime;
    // the run-time arena check must still reject it.
    let foreign = Box::into_raw(Box::new(source()));
    // SAFETY: this owner lives until selection has returned and rejected its
    // foreign arena. No reader escapes that operation.
    let borrowed: &'static _ = unsafe { &*foreign };
    let error = message::CheckedStructReader::select(source(), |_| {
        Ok(borrowed
            .get_root::<type_::Reader>()?
            .into_internal_struct_reader())
    })
    .err()
    .unwrap();
    // SAFETY: the selection failed, leaving no live views of the foreign owner.
    drop(unsafe { Box::from_raw(foreign) });
    assert_eq!(error.kind, ErrorKind::TypeMismatch);
    let segments = source().into_segments();
    let limited = message::Reader::new(
        segments,
        message::ReaderOptions {
            nesting_limit: 1,
            ..Default::default()
        },
    );
    assert!(message::CheckedStructReader::select(limited, |root| {
        let type_::List(list) = type_::Reader::from(root).which()? else {
            panic!()
        };
        Ok(list.get_element_type()?.into_internal_struct_reader())
    })
    .is_err());
}

#[test]
fn retained_selection_keeps_shared_storage_after_the_source_is_dropped() {
    use message::ReaderSegments;
    use std::rc::Rc;
    struct Shared(Rc<capnp::serialize::OwnedSegments>);
    impl ReaderSegments for Shared {
        fn get_segment(&self, index: u32) -> Option<&[u8]> {
            self.0.get_segment(index)
        }
        fn len(&self) -> usize {
            self.0.len()
        }
    }
    let segments = Rc::new(source().into_segments());
    let reader = message::Reader::new(Shared(segments.clone()), Default::default());
    let type_::List(list) = reader.get_root::<type_::Reader>().unwrap().which().unwrap() else {
        panic!()
    };
    let root = list
        .get_element_type()
        .unwrap()
        .into_internal_struct_reader();
    let address = root.get_data_section_as_blob().as_ptr();
    let retained =
        message::CheckedStructReader::retain(root, Shared(segments), Default::default()).unwrap();
    drop(reader);
    let retained = Box::new(retained);
    assert_eq!(retained.root().get_data_section_as_blob().as_ptr(), address);
    assert!(matches!(
        type_::Reader::from(retained.root()).which(),
        Ok(type_::Uint64(()))
    ));
}

#[test]
fn retained_selection_checks_every_segment_and_new_reader_limits() {
    use message::ReaderSegments;
    fn multi() -> message::Reader<capnp::serialize::OwnedSegments> {
        let mut builder =
            message::Builder::new(message::HeapAllocator::new().first_segment_words(1));
        builder
            .init_root::<type_::Builder>()
            .init_list()
            .init_element_type()
            .set_uint64(());
        let bytes = capnp::serialize::write_message_to_words(&builder);
        capnp::serialize::read_message(bytes.as_slice(), Default::default()).unwrap()
    }
    let source = multi();
    let other = multi();
    let root = source
        .get_root::<type_::Reader>()
        .unwrap()
        .into_internal_struct_reader();
    let segments: Vec<_> = (0..source.get_segments().len())
        .map(|i| source.get_segments().get_segment(i as u32).unwrap())
        .collect();
    assert!(segments.len() > 1);
    for changed in 0..segments.len() {
        let mut replaced = segments.clone();
        replaced[changed] = other.get_segments().get_segment(changed as u32).unwrap();
        assert!(message::CheckedStructReader::retain(root, replaced, Default::default()).is_err());
    }
    let mut missing = segments.clone();
    missing.pop();
    assert!(message::CheckedStructReader::retain(root, missing, Default::default()).is_err());
    let mut extra = segments.clone();
    extra.push(&[]);
    assert!(message::CheckedStructReader::retain(root, extra, Default::default()).is_err());
    let retained =
        message::CheckedStructReader::retain(root, segments.clone(), Default::default()).unwrap();
    let type_::List(list) = type_::Reader::from(retained.root()).which().unwrap() else {
        panic!()
    };
    assert!(matches!(
        list.get_element_type().unwrap().which(),
        Ok(type_::Uint64(()))
    ));
    for limit in [0, 1] {
        assert!(message::CheckedStructReader::retain(
            root,
            segments.clone(),
            message::ReaderOptions {
                traversal_limit_in_words: Some(limit),
                ..Default::default()
            }
        )
        .is_err());
    }
    assert!(message::CheckedStructReader::retain(
        root,
        segments.clone(),
        message::ReaderOptions {
            nesting_limit: 0,
            ..Default::default()
        }
    )
    .is_err());
    let shallow = message::CheckedStructReader::retain(
        root,
        segments,
        message::ReaderOptions {
            nesting_limit: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let type_::List(list) = type_::Reader::from(shallow.root()).which().unwrap() else {
        panic!()
    };
    assert!(list.get_element_type().is_err());
}

#[test]
fn retained_pointer_checks_identity_budget_and_shortened_storage() {
    use capnp::{any_pointer, any_struct, message::ReaderSegments};
    use std::{cell::Cell, rc::Rc};
    #[derive(Clone)]
    struct Shared {
        words: Rc<Vec<capnp::Word>>,
        short: Rc<Cell<bool>>,
    }
    impl ReaderSegments for Shared {
        fn get_segment(&self, id: u32) -> Option<&[u8]> {
            (id == 0).then(|| {
                capnp::Word::words_to_bytes(if self.short.get() {
                    &self.words[..2]
                } else {
                    &self.words
                })
            })
        }
    }
    let storage = Shared {
        words: Rc::new(vec![
            capnp::word(0, 0, 0, 0, 1, 0, 1, 0),
            capnp::word(42, 0, 0, 0, 0, 0, 0, 0),
            capnp::word(1, 0, 0, 0, 26, 0, 0, 0),
            capnp::word(b'a', b'b', b'c', 0, 0, 0, 0, 0),
        ]),
        short: Rc::default(),
    };
    let reader = message::Reader::new(storage.clone(), Default::default());
    let root = reader
        .get_root::<any_struct::Reader>()
        .unwrap()
        .into_internal_struct_reader();
    let content = any_pointer::Reader::new(root.get_pointer_field(0));
    let copy = Shared {
        words: Rc::new((*storage.words).clone()),
        short: Rc::default(),
    };
    assert!(
        matches!(message::CheckedPointerReader::retain(content, copy, Default::default()), Err(error) if error.kind == ErrorKind::TypeMismatch)
    );
    let checked = message::CheckedPointerReader::retain(
        content,
        storage.clone(),
        message::ReaderOptions {
            traversal_limit_in_words: Some(2),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        checked.root().get_as::<capnp::data::Reader>().unwrap(),
        b"abc"
    );
    assert!(
        matches!(checked.root().get_as::<capnp::data::Reader>(), Err(error) if error.kind == ErrorKind::ReadLimitExceeded)
    );
    let checked =
        message::CheckedPointerReader::retain(content, storage.clone(), Default::default())
            .unwrap();
    let checked = Box::new(checked);
    drop(reader);
    assert_eq!(
        checked.root().get_as::<capnp::data::Reader>().unwrap(),
        b"abc"
    );
    storage.short.set(true);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| checked.root())).is_err());
    let missing = any_pointer::Reader::new(capnp::private::layout::PointerReader::new_default());
    let empty: [&[u8]; 0] = [];
    let checked = message::CheckedPointerReader::retain(
        missing,
        &empty[..],
        message::ReaderOptions {
            traversal_limit_in_words: Some(0),
            nesting_limit: 0,
        },
    )
    .unwrap();
    assert!(checked.root().is_null());
}

#[test]
fn consuming_pointer_selection_preserves_budgets_and_rebases_inline_storage() {
    struct Inline([capnp::Word; 4]);
    impl message::ReaderSegments for Inline {
        fn get_segment(&self, id: u32) -> Option<&[u8]> {
            (id == 0).then(|| capnp::Word::words_to_bytes(&self.0))
        }
    }
    for limit in [3, 4] {
        let source = message::Reader::new(
            Inline([
                capnp::word(0, 0, 0, 0, 1, 0, 1, 0),
                capnp::word(7, 0, 0, 0, 0, 0, 0, 0),
                capnp::word(1, 0, 0, 0, 34, 0, 0, 0),
                capnp::word(b'a', b'b', b'c', 0, 0, 0, 0, 0),
            ]),
            message::ReaderOptions {
                traversal_limit_in_words: Some(limit),
                ..Default::default()
            },
        );
        let selected = source.try_select_pointer(
            message::ReaderOptions {
                traversal_limit_in_words: Some(2),
                ..Default::default()
            },
            |root| -> capnp::Result<_> {
                let root = root
                    .get_as::<capnp::any_struct::Reader>()?
                    .into_internal_struct_reader();
                let content = capnp::any_pointer::Reader::new(root.get_pointer_field(0));
                content.validate_capability_indices(0)?;
                Ok(Some((content, root.get_data_field::<u64>(0))))
            },
        );
        if limit == 3 {
            assert_eq!(selected.err().unwrap().kind, ErrorKind::ReadLimitExceeded);
            continue;
        }
        let (selected, value) = selected.unwrap().unwrap();
        assert_eq!(value, 7);
        let selected = Box::new(selected);
        assert_eq!(
            selected.root().get_as::<capnp::text::Reader>().unwrap(),
            "abc"
        );
        assert_eq!(
            selected
                .root()
                .get_as::<capnp::text::Reader>()
                .unwrap_err()
                .kind,
            ErrorKind::ReadLimitExceeded
        );
    }
}

#[test]
fn consuming_selection_can_decline_or_retain_a_missing_field_without_a_word_charge() {
    assert!(source()
        .try_select_pointer(Default::default(), |_| Ok::<_, capnp::Error>(
            None::<(capnp::any_pointer::Reader<'_>, ())>
        ))
        .unwrap()
        .is_none());
    let (missing, ()) = source()
        .try_select_pointer(
            message::ReaderOptions {
                traversal_limit_in_words: Some(0),
                ..Default::default()
            },
            |root| -> capnp::Result<_> {
                let root = root
                    .get_as::<capnp::any_struct::Reader>()?
                    .into_internal_struct_reader();
                Ok(Some((
                    capnp::any_pointer::Reader::new(root.get_pointer_field(usize::from(u16::MAX))),
                    (),
                )))
            },
        )
        .unwrap()
        .unwrap();
    assert!(missing.root().is_null());
}

#[test]
fn arena_interval_rejects_overflow_without_consuming_the_read_budget() {
    use capnp::private::arena::{ReaderArena, ReaderArenaImpl};
    let words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 2];
    let bytes = capnp::Word::words_to_bytes(&words);
    let segments = [bytes];
    let arena = ReaderArenaImpl::new(
        message::SegmentArray::new(&segments),
        message::ReaderOptions {
            traversal_limit_in_words: Some(2),
            ..Default::default()
        },
    );
    for start in [
        bytes.as_ptr(),
        bytes.as_ptr().wrapping_add(8),
        bytes.as_ptr().wrapping_add(16),
    ] {
        for length in [usize::MAX, usize::MAX / 8 + 1, 3] {
            assert_eq!(
                arena.contains_interval(0, start, length).unwrap_err().kind,
                ErrorKind::MessageContainsOutOfBoundsPointer
            );
        }
    }
    assert!(arena
        .contains_interval(0, bytes.as_ptr().wrapping_add(16), 0)
        .is_ok());
    assert!(arena.contains_interval(0, bytes.as_ptr(), 2).is_ok());
    assert_eq!(
        arena
            .contains_interval(0, bytes.as_ptr(), 1)
            .unwrap_err()
            .kind,
        ErrorKind::ReadLimitExceeded
    );
}

#[test]
fn cached_arena_validates_far_segment_children_with_the_original_budget() {
    for malformed in [false, true] {
        let first = [capnp::word(2, 0, 0, 0, 1, 0, 0, 0)];
        let second = [
            capnp::word(0, 0, 0, 0, 0, 0, 1, 0),
            if malformed {
                capnp::word(0xfd, 0xff, 0xff, 0x7f, 10, 0, 0, 0)
            } else {
                capnp::word(1, 0, 0, 0, 10, 0, 0, 0)
            },
            capnp::word(42, 0, 0, 0, 0, 0, 0, 0),
        ];
        let segments = [
            capnp::Word::words_to_bytes(&first),
            capnp::Word::words_to_bytes(&second),
        ];
        for limit in [3, 4] {
            let message = message::Reader::new(
                message::SegmentArray::new(&segments),
                message::ReaderOptions {
                    traversal_limit_in_words: Some(limit),
                    ..Default::default()
                },
            );
            let result = message
                .with_root(|root| -> capnp::Result<u8> {
                    let root = root
                        .get_as::<capnp::any_struct::Reader>()?
                        .into_internal_struct_reader();
                    Ok(root.get_pointer_field(0).get_data(None)?[0])
                })
                .unwrap();
            if malformed {
                assert_eq!(
                    result.unwrap_err().kind,
                    ErrorKind::MessageContainsOutOfBoundsPointer
                );
            } else if limit == 3 {
                assert_eq!(result.unwrap_err().kind, ErrorKind::ReadLimitExceeded);
            } else {
                assert_eq!(result.unwrap(), 42);
            }
        }
    }
}

#[test]
fn owning_pointer_cache_pins_inline_storage_and_preserves_spent_budget() {
    use std::{cell::Cell, rc::Rc};
    struct Inline {
        words: [capnp::Word; 4],
        lookups: Rc<Cell<usize>>,
        dropped: Rc<Cell<bool>>,
    }
    impl message::ReaderSegments for Inline {
        fn get_segment(&self, id: u32) -> Option<&[u8]> {
            self.lookups.set(self.lookups.get() + 1);
            (id == 0).then(|| capnp::Word::words_to_bytes(&self.words))
        }
    }
    impl Drop for Inline {
        fn drop(&mut self) {
            self.dropped.set(true);
        }
    }
    let lookups = Rc::new(Cell::new(0));
    let dropped = Rc::new(Cell::new(false));
    let reader = message::Reader::new(
        Inline {
            words: [
                capnp::word(0, 0, 0, 0, 1, 0, 1, 0),
                capnp::word(42, 0, 0, 0, 0, 0, 0, 0),
                capnp::word(1, 0, 0, 0, 26, 0, 0, 0),
                capnp::word(b'a', b'b', b'c', 0, 0, 0, 0, 0),
            ],
            lookups: lookups.clone(),
            dropped: dropped.clone(),
        },
        Default::default(),
    );
    let (selected, ()) = reader
        .try_select_pointer::<_, capnp::Error>(
            message::ReaderOptions {
                traversal_limit_in_words: Some(8),
                ..Default::default()
            },
            |root| Ok(Some((root, ()))),
        )
        .unwrap()
        .unwrap();
    let before = selected
        .root()
        .get_as::<capnp::any_struct::Reader>()
        .unwrap();
    let address = before
        .into_internal_struct_reader()
        .get_data_section_as_blob()
        .as_ptr();
    let cached = selected.into_cached().unwrap();
    let calls = lookups.get();
    let root = cached.root().get_as::<capnp::any_struct::Reader>().unwrap();
    let root = root.into_internal_struct_reader();
    assert_ne!(address, root.get_data_section_as_blob().as_ptr());
    assert_eq!(root.get_data_field::<u64>(0), 42);
    let address = root.get_data_section_as_blob().as_ptr();
    for _ in 0..3 {
        assert_eq!(root.get_pointer_field(0).get_data(None).unwrap(), b"abc");
    }
    assert_eq!(
        root.get_pointer_field(0).get_data(None).unwrap_err().kind,
        ErrorKind::ReadLimitExceeded
    );
    assert_eq!(
        lookups.get(),
        calls,
        "all segment-zero reads use the cached descriptor"
    );
    let moved = Box::new(cached);
    assert!(!dropped.get());
    assert!(
        matches!(moved.root().get_as::<capnp::any_struct::Reader>(), Err(error) if error.kind == ErrorKind::ReadLimitExceeded)
    );
    assert!(!address.is_null());
    drop(moved);
    assert!(
        dropped.get(),
        "cache retains and drops its original owner exactly once"
    );
}

#[test]
fn owning_pointer_cache_keeps_far_pointer_and_nesting_checks() {
    for first_segment_words in [1, 64] {
        let mut builder = message::Builder::new(
            message::HeapAllocator::new().first_segment_words(first_segment_words),
        );
        builder
            .init_root::<type_::Builder>()
            .init_list()
            .init_element_type()
            .set_uint64(());
        let bytes = capnp::serialize::write_message_to_words(&builder);
        for limit in [1, 64] {
            let reader =
                capnp::serialize::read_message(bytes.as_slice(), Default::default()).unwrap();
            let (selected, ()) = reader
                .try_select_pointer::<_, capnp::Error>(
                    message::ReaderOptions {
                        nesting_limit: limit,
                        ..Default::default()
                    },
                    |root| Ok(Some((root, ()))),
                )
                .unwrap()
                .unwrap();
            let cached = Box::new(selected.into_cached().unwrap());
            let type_::List(list) = cached
                .root()
                .get_as::<type_::Reader>()
                .unwrap()
                .which()
                .unwrap()
            else {
                panic!()
            };
            if limit == 1 {
                assert!(
                    matches!(list.get_element_type(), Err(error) if error.kind == ErrorKind::MessageIsTooDeeplyNestedOrContainsCycles)
                );
            } else {
                assert!(matches!(
                    list.get_element_type().unwrap().which(),
                    Ok(type_::Uint64(()))
                ));
            }
        }
    }
}
