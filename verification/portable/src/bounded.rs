//! The same bounded paths run on the host, under Miri, and in the ARM link fixture.
use super::*;
use capnp::field_api::{Data, List, Schema, Struct, Text};
use portable_capnp::api::{Box, ChoiceBox, Entry, EntryV2, EntryWide};

pub fn bounded_list_upgrades() -> Result<()> {
    // The staging slot, inline tag and all elements must fit before any child
    // ownership moves. Exhaust every capacity boundary, including the exact fit.
    let before = 2 + 2 * Entry::SIZE.total() as usize + 2;
    let after = before + 2 + 2 * EntryV2::SIZE.total() as usize;
    for capacity in before..=after {
        let mut words = [ZERO; 32];
        let mut message = message::FixedBuilder::new(&mut words[..capacity])?;
        let mut list = message.try_init_root_list::<Entry>(2)?;
        for i in 0..2 {
            let mut item = list.get_mut(i).unwrap();
            item.scalar(Entry::ID).set(31 + i as u64);
            item.field(Entry::LABEL)
                .copy_from(if i == 0 { "first" } else { "last" })?;
        }
        let pointers = [
            list.read().get(0).unwrap().label()?.as_ptr(),
            list.read().get(1).unwrap().label()?.as_ptr(),
        ];
        assert_eq!(message.size_in_words(), before);
        assert_eq!(
            message.try_get_root_list::<EntryV2>().err().unwrap().kind,
            ErrorKind::NeedsUpgrade
        );
        let result = message.try_upgrade_root_list::<EntryV2>();
        if capacity < after {
            assert!(matches!(
                result.err().unwrap().kind,
                ErrorKind::InsufficientCapacity { .. }
            ));
            assert_eq!(message.size_in_words(), before);
        } else {
            let mut newer = result?;
            newer.get_mut(1).unwrap().scalar(EntryV2::EXTRA).set(123);
            assert_eq!(newer.read().get(0).unwrap().extra(), 17);
            assert_eq!(message.size_in_words(), after);
            message
                .try_upgrade_root_list::<Entry>()?
                .get_mut(0)
                .unwrap()
                .scalar(Entry::ID)
                .set(9);
            assert_eq!(message.size_in_words(), after); // No-op/downgrade preserves layout.
            assert_eq!(
                message
                    .try_get_root_list::<EntryV2>()?
                    .read()
                    .get(1)
                    .unwrap()
                    .extra(),
                123
            );
        }
        let old = message.try_get_root_list::<Entry>()?;
        for (i, pointer) in pointers.into_iter().enumerate() {
            assert_eq!(old.read().get(i).unwrap().label()?.as_ptr(), pointer);
        }
        assert_eq!(old.read().get(1).unwrap().id(), 32);
    }

    // Grow the data section through a schema that knows fewer pointer fields.
    // The unknown pointer remains owned and its payload is moved, never copied.
    let mut words = [ZERO; 64];
    let mut message = message::FixedBuilder::new(&mut words)?;
    let mut list = message.try_init_root_list::<EntryV2>(1)?;
    list.get_mut(0)
        .unwrap()
        .field(EntryV2::PAYLOAD)
        .copy_from(b"unknown")?;
    let payload = list.read().get(0).unwrap().payload()?.as_ptr();
    message
        .try_upgrade_root_list::<EntryWide>()?
        .get_mut(0)
        .unwrap()
        .scalar(EntryWide::COUNTER)
        .set(99);
    let list = message.try_get_root_list::<EntryV2>()?;
    assert_eq!(list.read().get(0).unwrap().payload()?, b"unknown");
    assert_eq!(list.read().get(0).unwrap().payload()?.as_ptr(), payload);
    Ok(())
}

pub fn bounded_list_promotions() -> Result<()> {
    // Pointer-list promotion has a separate physical layout. Force failure
    // before/after the staging word and tag reservation, then check exact fit.
    for capacity in 5..=15 {
        let mut words = [ZERO; 15];
        let mut message = message::FixedBuilder::new(&mut words[..capacity])?;
        let mut list = message.try_init_root_list::<Text>(2)?;
        list.get_mut(0).unwrap().reborrow().copy_from("first")?;
        list.get_mut(1).unwrap().copy_from("second")?;
        let result = message.try_upgrade_root_list::<EntryV2>();
        if capacity < 15 {
            assert!(matches!(
                result.err().unwrap().kind,
                ErrorKind::InsufficientCapacity { .. }
            ));
            assert_eq!(message.size_in_words(), 5);
        } else {
            assert_eq!(result?.read().get(1).unwrap().label()?, "second");
        }
        assert_eq!(
            message
                .try_get_root_list::<Text>()?
                .read()
                .get(0)
                .unwrap()?,
            "first"
        );
    }
    let mut words = [ZERO; 64];
    let mut message = message::FixedBuilder::new(&mut words)?;
    macro_rules! primitive {
        ($ty:ty) => {{
            message.reset();
            let mut old = message.try_init_root_list::<$ty>(3)?;
            for i in 0..3 {
                old.get_mut(i).unwrap().set((17 + i) as $ty);
            }
            let newer = message.try_upgrade_root_list::<EntryV2>()?;
            for i in 0..3 {
                let item = newer.read().get(i).unwrap();
                assert_eq!(item.id(), 17 + i as u64);
                assert_eq!(item.extra(), 17);
                assert_eq!(item.label()?, "");
            }
        }};
    }
    primitive!(u8);
    primitive!(u16);
    primitive!(u32);
    primitive!(u64);
    message.reset();
    let mut old = message.try_init_root_list::<Text>(2)?;
    old.get_mut(0).unwrap().copy_from("pointer")?;
    old.get_mut(1).unwrap().copy_from("second")?;
    let pointer = old.read().get(0).unwrap()?.as_ptr();
    let new = message.try_upgrade_root_list::<EntryV2>()?;
    assert_eq!(new.read().get(0).unwrap().label()?.as_ptr(), pointer);
    assert_eq!(new.read().get(1).unwrap().label()?, "second");
    assert_eq!(new.read().get(1).unwrap().id(), 0);

    message.reset();
    message.try_init_root_list::<()>(3)?;
    assert_eq!(message.try_upgrade_root_list::<EntryV2>()?.read().len(), 3);
    message.reset();
    message.try_init_root_list::<u8>(0)?;
    assert!(message
        .try_upgrade_root_list::<EntryV2>()?
        .read()
        .is_empty());
    message.reset();
    message.try_init_root_list::<()>(0x1fff_ffff)?;
    let before = message.size_in_words();
    assert_eq!(
        message
            .try_upgrade_root_list::<EntryV2>()
            .err()
            .unwrap()
            .kind,
        ErrorKind::LengthOverflow
    );
    assert_eq!(message.size_in_words(), before);
    assert_eq!(message.try_get_root_list::<()>()?.len(), 0x1fff_ffff);
    assert_eq!(
        message
            .try_upgrade_root_list::<portable_capnp::api::Empty>()?
            .len(),
        0x1fff_ffff
    );
    assert_eq!(message.size_in_words(), before + 2);

    message.reset();
    message
        .try_init_root_list::<bool>(1)?
        .get_mut(0)
        .unwrap()
        .set(true);
    let before = message.size_in_words();
    assert_eq!(
        message
            .try_upgrade_root_list::<EntryV2>()
            .err()
            .unwrap()
            .kind,
        ErrorKind::FoundBitListWhereStructListWasExpected
    );
    assert_eq!(message.size_in_words(), before);
    assert_eq!(
        message.try_get_root_list::<bool>()?.read().get(0),
        Some(true)
    );
    message.reset();
    assert_eq!(
        message
            .try_upgrade_root_list::<EntryV2>()
            .err()
            .unwrap()
            .kind,
        ErrorKind::NotPresent
    );
    Ok(())
}

pub fn bounded_generic_fields() -> Result<()> {
    // A generic AnyPointer copy must retain the previous non-pointer union
    // selection as well as reclaiming unpublished storage on failure.
    let mut source_words = [ZERO; 8];
    let mut source = message::FixedBuilder::new(&mut source_words)?;
    source.try_set_root_text("copied")?;
    type Untyped = ChoiceBox<capnp::any_pointer::Owned>;
    let base = 1 + Untyped::SIZE.total() as usize;
    for capacity in base..=base + 2 {
        let mut words = [ZERO; 8];
        let mut dest = message::FixedBuilder::new(&mut words[..capacity])?;
        let mut root = dest.try_init_root::<Untyped>()?;
        root.scalar(Untyped::COUNT).set(37);
        let result = root
            .field(Untyped::VALUE)
            .reborrow()
            .typed()
            .copy_from(source.get_root_as_reader()?);
        // The null union pointer itself is the unpublished staging slot. Its
        // one-word text value needs no additional orphan pointer word.
        if capacity < base + 1 {
            assert!(matches!(
                result.err().unwrap().kind,
                ErrorKind::InsufficientCapacity { .. }
            ));
            assert_eq!(root.read().count()?, 37);
            assert_eq!(dest.size_in_words(), base);
        } else {
            result?;
            assert_eq!(
                root.field(Untyped::VALUE)
                    .typed()
                    .read()?
                    .get_as::<capnp::text::Reader<'_>>()?,
                "copied"
            );
            assert_eq!(dest.size_in_words(), base + 1);
        }
    }
    let mut words = [ZERO; 96];
    let mut message = message::FixedBuilder::new(&mut words)?;
    type TextBox = Box<capnp::text::Owned>;
    let mut root = message.try_init_root::<TextBox>()?;
    root.field(TextBox::VALUE)
        .typed()
        .copy_from("bounded generic")?;
    assert_eq!(root.read().value()?.to_str()?, "bounded generic");

    message.reset();
    type StructBox = Box<portable_capnp::entry::Owned>;
    message
        .try_init_root::<StructBox>()?
        .field(StructBox::VALUE)
        .typed()
        .init()?
        .scalar(Entry::ID)
        .set(71);
    assert_eq!(
        message
            .try_get_root::<StructBox>()?
            .read()
            .value()?
            .get_id(),
        71
    );
    // Both the legacy owner and the generated field-API marker map to a bounded editor.
    type NewBox = Box<Entry>;
    assert_eq!(
        message
            .try_get_root::<NewBox>()?
            .field(NewBox::VALUE)
            .typed()
            .edit()?
            .read()
            .id(),
        71
    );

    message.reset();
    type ListBox = Box<capnp::struct_list::Owned<portable_capnp::entry::Owned>>;
    message
        .try_init_root::<ListBox>()?
        .field(ListBox::VALUE)
        .typed()
        .init(1)?
        .get_mut(0)
        .unwrap()
        .scalar(Entry::ID)
        .set(82);
    assert_eq!(
        message
            .try_get_root::<ListBox>()?
            .read()
            .value()?
            .get(0)
            .get_id(),
        82
    );

    message.reset();
    type EnumBox = Box<capnp::enum_list::Owned<portable_capnp::State>>;
    message
        .try_init_root::<EnumBox>()?
        .field(EnumBox::VALUE)
        .typed()
        .init(1)?
        .get_mut(0)
        .unwrap()
        .set(portable_capnp::api::State::Unknown(417));
    assert_eq!(
        message
            .try_get_root::<EnumBox>()?
            .field(EnumBox::VALUE)
            .typed()
            .edit()?
            .read()
            .get(0),
        Some(portable_capnp::api::State::Unknown(417))
    );

    message.reset();
    type Nested = Box<capnp::list_list::Owned<capnp::primitive_list::Owned<u16>>>;
    message
        .try_init_root::<Nested>()?
        .field(Nested::VALUE)
        .typed()
        .init(1)?
        .get_mut(0)
        .unwrap()
        .init(2)?
        .get_mut(1)
        .unwrap()
        .set(65530);
    assert_eq!(
        message
            .try_get_root::<Nested>()?
            .read()
            .value()?
            .get(0)?
            .get(1),
        65530
    );

    message.reset();
    type Texts = Box<capnp::text_list::Owned>;
    message
        .try_init_root::<Texts>()?
        .field(Texts::VALUE)
        .typed()
        .init(1)?
        .get_mut(0)
        .unwrap()
        .copy_from("text")?;
    assert_eq!(
        message.try_get_root::<Texts>()?.read().value()?.get(0)?,
        "text"
    );
    message.reset();
    type Chunks = Box<capnp::data_list::Owned>;
    message
        .try_init_root::<Chunks>()?
        .field(Chunks::VALUE)
        .typed()
        .init(1)?
        .get_mut(0)
        .unwrap()
        .copy_from(b"data")?;
    assert_eq!(
        message.try_get_root::<Chunks>()?.read().value()?.get(0)?,
        b"data"
    );
    message.reset();
    type Bytes = Box<capnp::data::Owned>;
    message
        .try_init_root::<Bytes>()?
        .field(Bytes::VALUE)
        .typed()
        .copy_from(b"bytes")?;
    assert_eq!(message.try_get_root::<Bytes>()?.read().value()?, b"bytes");
    message.reset();
    type StructPointers = Box<capnp::list_list::Owned<Entry>>;
    message
        .try_init_root::<StructPointers>()?
        .field(StructPointers::VALUE)
        .typed()
        .init(1)?
        .get_mut(0)
        .unwrap()
        .init()?
        .scalar(Entry::ID)
        .set(29);
    assert_eq!(
        message
            .try_get_root::<StructPointers>()?
            .read()
            .value()?
            .get(0)?
            .id(),
        29
    );
    message.reset();
    type AnyPointers = Box<capnp::list_list::Owned<capnp::any_pointer::Owned>>;
    message
        .try_init_root::<AnyPointers>()?
        .field(AnyPointers::VALUE)
        .typed()
        .init(1)?
        .get_mut(0)
        .unwrap()
        .typed()
        .cast::<Text>()
        .copy_from("any element")?;
    assert_eq!(
        message
            .try_get_root::<AnyPointers>()?
            .read()
            .value()?
            .get(0)?
            .get_as::<capnp::text::Reader<'_>>()?,
        "any element"
    );

    message.reset();
    let mut root = message.try_init_root::<Untyped>()?;
    assert_eq!(
        root.field(Untyped::VALUE)
            .typed()
            .cast::<List<Entry>>()
            .upgrade()
            .err()
            .unwrap()
            .kind,
        ErrorKind::NotPresent
    );
    root.field(Untyped::VALUE)
        .typed()
        .cast::<List<Entry>>()
        .init(1)?
        .get_mut(0)
        .unwrap()
        .field(Entry::LABEL)
        .copy_from("live")?;
    let original = root
        .field(Untyped::VALUE)
        .typed()
        .read()?
        .get_as::<capnp::struct_list::Reader<'_, portable_capnp::entry::Owned>>()?
        .get(0)
        .get_label()?
        .as_bytes()
        .as_ptr();
    root.field(Untyped::VALUE)
        .typed()
        .cast::<List<EntryV2>>()
        .upgrade()?
        .get_mut(0)
        .unwrap()
        .scalar(EntryV2::EXTRA)
        .set(21);
    let current = root
        .field(Untyped::VALUE)
        .typed()
        .read()?
        .get_as::<capnp::struct_list::Reader<'_, portable_capnp::entry_v2::Owned>>()?;
    assert_eq!(current.get(0).get_label()?.as_bytes().as_ptr(), original);
    assert_eq!(current.get(0).get_extra(), 21);
    // AnyPointer has to be explicitly cast; attempting to edit an incompatible
    // wire kind rejects it and preserves the selected union arm.
    assert!(root
        .field(Untyped::VALUE)
        .typed()
        .cast::<Struct<Entry>>()
        .edit()
        .is_err());

    root.field(Untyped::VALUE)
        .typed()
        .copy_from(source.get_root_as_reader()?)?;
    assert_eq!(
        root.field(Untyped::VALUE)
            .typed()
            .read()?
            .get_as::<capnp::text::Reader<'_>>()?,
        "copied"
    );
    root.field(Untyped::VALUE)
        .typed()
        .cast::<Data>()
        .copy_from(b"bytes")?;
    assert_eq!(
        root.field(Untyped::VALUE).typed().cast::<Data>().edit()?,
        b"bytes"
    );
    Ok(())
}
