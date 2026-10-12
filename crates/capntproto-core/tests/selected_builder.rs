use capnp::{any_pointer_list, message};

#[test]
fn selected_struct_builder_keeps_far_storage_and_descendants_across_moves() {
    let source = message::Builder::new(message::HeapAllocator::new().first_segment_words(1));
    let selected = source
        .try_select_struct(|root| {
            let mut list = root.initn_as::<any_pointer_list::Builder>(2);
            list.reborrow()
                .get(0)
                .set_as::<capnp::text::Owned>("parent")?;
            Ok(list.get(1).init_as_any_struct(1, 1))
        })
        .map_err(|(_, error)| error)
        .unwrap();
    let mut selected = Box::new(selected);
    selected
        .get()
        .get_data_section()
        .copy_from_slice(&42u64.to_le_bytes());
    selected
        .get()
        .get_pointer_section()
        .get(0)
        .set_as::<capnp::data::Owned>(&[7; 8192][..])
        .unwrap();
    let mut selected = *selected;
    assert_eq!(selected.get().get_data_section(), 42u64.to_le_bytes());
    assert!(
        selected
            .get_as::<capnp::schema_capnp::node::Owned>()
            .is_err(),
        "a cached view cannot grow its containing allocation"
    );
    selected
        .get()
        .get_pointer_section()
        .get(0)
        .set_as::<capnp::text::Owned>("replacement")
        .unwrap();
    let mut message = selected.into_inner();
    let root = message
        .get_root_as_reader::<any_pointer_list::Reader>()
        .unwrap();
    assert_eq!(
        root.get(0).get_as::<capnp::text::Reader>().unwrap(),
        "parent"
    );
    let value = root.get(1).get_as::<capnp::any_struct::Reader>().unwrap();
    assert_eq!(value.get_data_section(), 42u64.to_le_bytes());
    assert_eq!(
        value
            .get_pointer_section()
            .get(0)
            .get_as::<capnp::text::Reader>()
            .unwrap(),
        "replacement"
    );
    message.set_root("ancestors editable again").unwrap();
}

#[test]
fn selected_struct_builder_rejects_foreign_storage_and_returns_failed_edits() {
    let foreign = Box::into_raw(Box::new(message::Builder::new_default()));
    // SAFETY: the owner stays live until the selection rejects its foreign
    // arena. No view escapes, and no other access overlaps this exclusive borrow.
    let borrowed: &'static mut _ = unsafe { &mut *foreign };
    let error = message::Builder::new_default()
        .try_select_struct(|_| {
            Ok(borrowed
                .init_root::<capnp::any_pointer::Builder>()
                .init_as_any_struct(1, 1))
        })
        .err()
        .unwrap()
        .1;
    // SAFETY: selection rejected the foreign owner, so its borrow has ended.
    drop(unsafe { Box::from_raw(foreign) });
    assert_eq!(error.kind, capnp::ErrorKind::TypeMismatch);
    let (message, error) = message::Builder::new_default()
        .try_select_struct(|mut root| {
            root.set_as::<capnp::text::Owned>("keep edits")?;
            Err(capnp::Error::from_kind(capnp::ErrorKind::TypeMismatch))
        })
        .err()
        .unwrap();
    assert_eq!(error.kind, capnp::ErrorKind::TypeMismatch);
    assert_eq!(
        message.get_root_as_reader::<capnp::text::Reader>().unwrap(),
        "keep edits"
    );
    let source = message::Builder::new_default()
        .try_select_struct(|root| Ok(root.init_as_any_struct(1, 1)))
        .map_err(|(_, error)| error)
        .unwrap();
    let template = source.into_template().unwrap();
    let mut first = template.instantiate(message::HeapAllocator::new().first_segment_words(1));
    let mut second = template.instantiate(message::HeapAllocator::new());
    drop(template);
    first.get().get_data_section()[0] = 17;
    first
        .get()
        .get_pointer_section()
        .get(0)
        .set_as::<capnp::data::Owned>(&[9; 8192][..])
        .unwrap();
    assert_eq!(second.get().get_data_section()[0], 0);
    assert!(second.get().get_pointer_section().get(0).is_null());
    drop(second);
    assert_eq!(first.get().get_data_section()[0], 17);
    assert!(
        first.into_template().is_err(),
        "multiple segments cannot be frozen"
    );
}

#[test]
fn builder_template_copies_independent_storage_and_can_be_dropped_first() {
    let mut source = message::Builder::new_default();
    source
        .initn_root::<any_pointer_list::Builder>(2)
        .get(0)
        .set_as::<capnp::text::Owned>("header")
        .unwrap();
    let template = message::BuilderTemplate::new(source).unwrap();
    let mut first = template.instantiate(message::HeapAllocator::new().first_segment_words(1));
    let second = template.instantiate(message::HeapAllocator::new());
    drop(template);
    first
        .get_root::<any_pointer_list::Builder>()
        .unwrap()
        .get(1)
        .set_as::<capnp::data::Owned>(&[7; 8192][..])
        .unwrap();
    assert_eq!(
        first
            .get_root_as_reader::<any_pointer_list::Reader>()
            .unwrap()
            .get(0)
            .get_as::<capnp::text::Reader>()
            .unwrap(),
        "header"
    );
    assert!(second
        .get_root_as_reader::<any_pointer_list::Reader>()
        .unwrap()
        .get(1)
        .is_null());
    let template = message::BuilderTemplate::new(second).unwrap();
    let mut words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 64];
    let mut allocator =
        message::SingleSegmentAllocator::new(capnp::Word::words_to_bytes_mut(&mut words));
    for _ in 0..2 {
        let copy = template.instantiate(&mut allocator);
        assert_eq!(
            copy.get_root_as_reader::<any_pointer_list::Reader>()
                .unwrap()
                .get(0)
                .get_as::<capnp::text::Reader>()
                .unwrap(),
            "header"
        );
    }
    assert!(message::BuilderTemplate::new(message::Builder::new_default()).is_err());
    assert!(
        message::BuilderTemplate::new(first).is_err(),
        "multisegment template"
    );
}

#[test]
fn selected_template_keeps_its_pointer_after_parent_owner_is_dropped() {
    let source = message::Builder::new_default();
    let template = source
        .select(|root| {
            let mut list = root.initn_as::<any_pointer_list::Builder>(2);
            list.reborrow()
                .get(0)
                .set_as::<capnp::text::Owned>("header")?;
            Ok(list.get(1))
        })
        .unwrap()
        .into_template()
        .unwrap();
    let mut first = template.instantiate(message::HeapAllocator::new());
    let mut second = template.instantiate(message::HeapAllocator::new());
    drop(template);
    first.get().set_as::<capnp::text::Owned>("first").unwrap();
    second.get().set_as::<capnp::text::Owned>("second").unwrap();
    for (copy, expected) in [(first, "first"), (second, "second")] {
        copy.validate_capability_indices(0, message::ReaderOptions::new())
            .unwrap();
        let message = copy.into_inner();
        let list = message
            .get_root_as_reader::<any_pointer_list::Reader>()
            .unwrap();
        assert_eq!(
            list.get(0).get_as::<capnp::text::Reader>().unwrap(),
            "header"
        );
        assert_eq!(
            list.get(1).get_as::<capnp::text::Reader>().unwrap(),
            expected
        );
    }
}

#[test]
fn selected_pointer_survives_moves_replacement_and_additional_segments() {
    let allocator = message::HeapAllocator::new()
        .first_segment_words(1)
        .allocation_strategy(message::AllocationStrategy::FixedSize);
    let message = message::Builder::new(allocator);
    let selected = message
        .select(|root| {
            let mut list = root.initn_as::<any_pointer_list::Builder>(2);
            list.reborrow()
                .get(0)
                .set_as::<capnp::text::Owned>("parent sentinel")?;
            Ok(list.get(1))
        })
        .unwrap();
    let mut selected = Box::new(selected);
    selected
        .get()
        .set_as::<capnp::data::Owned>(&[7; 8192][..])
        .unwrap();
    selected
        .validate_capability_indices(0, message::ReaderOptions::new())
        .unwrap();
    assert_eq!(
        selected
            .validate_capability_indices(
                0,
                message::ReaderOptions {
                    traversal_limit_in_words: Some(2),
                    ..message::ReaderOptions::new()
                }
            )
            .unwrap_err()
            .kind,
        capnp::ErrorKind::ReadLimitExceeded
    );
    assert_eq!(
        selected.get().get_as::<capnp::data::Builder>().unwrap(),
        &[7; 8192]
    );
    selected
        .get()
        .set_as::<capnp::text::Owned>("replacement")
        .unwrap();
    let mut selected = *selected;
    selected
        .validate_capability_indices(0, message::ReaderOptions::new())
        .unwrap();
    assert_eq!(
        selected
            .get()
            .get_as::<capnp::text::Builder>()
            .unwrap()
            .to_str()
            .unwrap(),
        "replacement"
    );
    let message = selected.into_inner();
    let root = message
        .get_root_as_reader::<any_pointer_list::Reader>()
        .unwrap();
    assert_eq!(
        root.get(0).get_as::<capnp::text::Reader>().unwrap(),
        "parent sentinel"
    );
    assert_eq!(
        root.get(1).get_as::<capnp::text::Reader>().unwrap(),
        "replacement"
    );
}

#[test]
fn selected_root_supports_allocator_borrow_and_reuse() {
    let mut words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 64];
    let mut allocator =
        message::SingleSegmentAllocator::new(capnp::Word::words_to_bytes_mut(&mut words));
    for value in ["first", "second"] {
        {
            let message = message::Builder::new(&mut allocator);
            let mut selected = message
                .try_select_struct(|root| {
                    capnp::any_struct::Builder::from_builder(
                        root.init_as::<capnp::schema_capnp::type_::Builder>(),
                    )
                })
                .map_err(|(_, error)| error)
                .unwrap();
            selected
                .get_as::<capnp::schema_capnp::type_::Owned>()
                .unwrap()
                .set_bool(());
            assert!(matches!(
                selected
                    .get_as::<capnp::schema_capnp::type_::Owned>()
                    .unwrap()
                    .into_reader()
                    .which(),
                Ok(capnp::schema_capnp::type_::Bool(()))
            ));
        }
        for empty in [false, true] {
            let message = message::Builder::new(&mut allocator);
            let mut selected = message
                .try_select_struct(|root| Ok(root.init_as_any_struct(if empty { 0 } else { 1 }, 0)))
                .map_err(|(_, error)| error)
                .unwrap();
            let mut view = selected.get();
            assert_eq!(view.get_data_section().len(), if empty { 0 } else { 8 });
            if !empty {
                view.get_data_section()[0] = 17;
            }
        }
        let message = message::Builder::new(&mut allocator);
        let mut selected = message.select(|root| Ok(root)).unwrap();
        selected.get().set_as::<capnp::text::Owned>(value).unwrap();
        selected
            .validate_capability_indices(0, message::ReaderOptions::new())
            .unwrap();
        let message = selected.into_inner();
        assert_eq!(
            message.get_root_as_reader::<capnp::text::Reader>().unwrap(),
            value
        );
    }
}
