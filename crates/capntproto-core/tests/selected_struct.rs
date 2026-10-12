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
