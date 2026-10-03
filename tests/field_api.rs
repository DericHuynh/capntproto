use capnp::field_api::{Entry, Message, MessageView};
use capnp::{Error, ErrorKind};
use capntproto_test_support::field_api_capnp::{
    api::{Address, Person, PhoneKind, Types},
    person,
};

#[test]
fn documented_workflow_defaults_entries_lists_and_freeze() -> capnp::Result<()> {
    let mut message = Message::<Person>::new()?;
    assert_eq!(message.read().id(), 42);
    assert_eq!(Person::MATRIX.ordinal, 14);
    assert_eq!(message.read().name()?, "anonymous");
    assert_eq!(message.read().address()?.city()?, "default city");
    assert!(message.read().enabled());
    assert_eq!(message.read().ratio(), -1.5);
    assert_eq!(message.read().optional_name()?, None);
    assert!(message.read().field(Person::NAME).is_null());
    assert_eq!(message.read().field(Person::NAME).present()?, None);
    {
        let mut p = message.edit();
        p.id().set(7);
        p.name().copy_from("Deric")?;
        let mut a = p.address().ensure()?;
        a.city().copy_from("Edmonton")?;
        p.phones().init_with(2, |i, mut phone| {
            phone.number().copy_from(["555-0100", "555-0101"][i])?;
            phone.kind().set(PhoneKind::Mobile);
            Ok(())
        })?;
        p.employment().school().copy_from("NAIT")?;
        p.names()
            .init_with(2, |i, name| name.copy_from(["one", "two"][i]))?;
        p.numbers().init_with(3, |i, n| {
            n.set(i as u32);
            Ok(())
        })?;
        p.kinds().init_with(1, |_, k| {
            k.set(PhoneKind::Unknown(60000));
            Ok(())
        })?;
        p.matrix().init_with(1, |_, row| {
            row.init_with(2, |i, n| {
                n.set(i as u32 + 5);
                Ok(())
            })
        })?;
        p.payload().init(3)?.copy_from_slice(b"abc");
        match p.previous_address().entry()? {
            Entry::Vacant(slot) => {
                slot.init()?.city().copy_from("new")?;
            }
            Entry::Occupied(_) => panic!(),
        }
        match p.previous_address().entry()? {
            Entry::Occupied(slot) => {
                assert_eq!(slot.edit()?.read().city()?, "new");
            }
            Entry::Vacant(_) => panic!(),
        }
        assert_eq!(
            p.address().init().err().unwrap().kind,
            ErrorKind::AlreadyPresent
        );
    }
    assert_eq!(message.read().id(), 7);
    assert_eq!(
        message.read().phones()?.get(1).unwrap().number()?,
        "555-0101"
    );
    assert!(message.read().phones()?.get(usize::MAX).is_none());
    assert_eq!(message.read().names()?.get(1).transpose()?, Some("two"));
    assert_eq!(message.read().numbers()?.as_slice(), Some(&[0, 1, 2][..]));
    assert_eq!(
        message.read().kinds()?.get(0),
        Some(PhoneKind::Unknown(60000))
    );
    assert_eq!(message.read().matrix()?.get(0).unwrap()?.get(1), Some(6));
    use capntproto_test_support::field_api_capnp::api::person::{
        EmploymentUnionRef, EmploymentUnionTag,
    };
    assert!(matches!(
        message.read().employment()?,
        EmploymentUnionRef::School("NAIT")
    ));
    assert_eq!(message.read().employment_tag(), EmploymentUnionTag::School);
    let frozen = message.freeze();
    assert_eq!(frozen.read().payload()?, b"abc");
    let bytes = frozen.to_vec();
    let view = MessageView::<Person>::from_unpacked(&bytes, Default::default())?;
    let text = view.read().name()?;
    assert_eq!(text, "Deric");
    assert!(
        text.as_ptr() >= bytes.as_ptr() && text.as_ptr() < bytes.as_ptr().wrapping_add(bytes.len())
    );
    assert_eq!(frozen.compact_copy()?.read().name()?, "Deric");
    Ok(())
}
#[test]
fn handles_are_lazy_and_native_copies_reuse_storage() -> capnp::Result<()> {
    let mut m = Message::<Person>::new()?;
    {
        let mut p = m.edit();
        let _ = p.name();
        let _ = p.employment().school();
        assert!(p.read().field(Person::NAME).is_null());
        assert!(matches!(
            p.read().employment()?,
            capntproto_test_support::field_api_capnp::api::person::EmploymentUnionRef::Unemployed
        ));
        assert_eq!(
            p.address().edit().err().unwrap().kind,
            ErrorKind::NotPresent
        );
        p.name().copy_from("first")?;
        p.payload().copy_from(b"first")?;
    }
    let words = m.size_in_words();
    for _ in 0..100 {
        m.edit().name().copy_from("other")?;
        m.edit().payload().copy_from(b"other")?;
    }
    assert_eq!(m.size_in_words(), words);
    m.edit().address().init()?;
    assert_eq!(m.read().address()?.city()?, ""); // init ignores field default.
    m.edit().address().clear();
    assert_eq!(m.read().address()?.city()?, "default city");
    Ok(())
}
#[test]
fn failed_staged_list_and_length_checks_preserve_destination() -> capnp::Result<()> {
    let mut m = Message::<Person>::new()?;
    m.edit()
        .phones()
        .init_with(1, |_, mut p| p.number().copy_from("original"))?;
    let e = m
        .edit()
        .phones()
        .replace_with(3, |i, mut p| {
            p.number().copy_from("candidate")?;
            if i == 2 {
                Err(Error::failed("callback".into()))
            } else {
                Ok(())
            }
        })
        .err()
        .unwrap();
    assert_eq!(e.kind, ErrorKind::Failed);
    assert_eq!(m.read().phones()?.len(), 1);
    assert_eq!(m.read().phones()?.get(0).unwrap().number()?, "original");
    assert_eq!(
        m.edit().payload().replace(usize::MAX).err().unwrap().kind,
        ErrorKind::LengthOverflow
    );
    assert_eq!(
        m.edit().phones().replace(1usize << 29).err().unwrap().kind,
        ErrorKind::LengthOverflow
    );
    assert_eq!(m.read().phones()?.len(), 1);
    // Sequential edits explicitly do not roll back earlier elements.
    m.edit()
        .phones()
        .edit()?
        .try_for_each_mut(|_, mut p| {
            p.number().copy_from("changed")?;
            Err(Error::failed("later".into()))
        })
        .unwrap_err();
    assert_eq!(m.read().phones()?.get(0).unwrap().number()?, "changed");
    Ok(())
}
#[test]
fn malformed_utf8_is_lazy_but_never_defaulted() -> capnp::Result<()> {
    let mut m = Message::<Person>::new()?;
    m.edit().name().copy_from("unique utf8 sentinel")?;
    let mut bytes = m.to_vec();
    let offset = bytes
        .windows(20)
        .position(|v| v == b"unique utf8 sentinel")
        .unwrap();
    bytes[offset] = 0xff;
    let v = MessageView::<Person>::from_unpacked(&bytes, Default::default())?;
    assert!(v.read().name().is_err());
    assert!(v.read().field(Person::NAME).present().is_err());
    let wire = v.read().field(Person::NAME).wire_text()?;
    assert_eq!(wire.as_bytes()[0], 255);
    assert!(wire.to_str().is_err());
    let mut bytes = m.to_vec();
    bytes[offset + 20] = 1;
    let v = MessageView::<Person>::from_unpacked(&bytes, Default::default())?;
    assert!(v.read().field(Person::NAME).wire_text().is_err());
    Ok(())
}
#[test]
fn framing_is_exact_and_nested_copy_is_atomic_on_bad_pointer() -> capnp::Result<()> {
    let mut source = Message::<Person>::new()?;
    source
        .edit()
        .address()
        .ensure()?
        .city()
        .copy_from("source")?;
    let mut bytes = source.to_vec();
    bytes.extend_from_slice(&[0; 8]);
    assert_eq!(
        MessageView::<Person>::from_unpacked(&bytes, Default::default())
            .err()
            .unwrap()
            .kind,
        ErrorKind::TrailingData
    );
    let (_, tail) = MessageView::<Person>::from_unpacked_prefix(&bytes, Default::default())?;
    assert_eq!(tail.len(), 8);
    let mut address = Message::<Address>::new()?;
    address.edit().city().copy_from("source")?;
    let mut bad = address.to_vec();
    // Address is a zero-data, one-pointer struct; overwrite its city pointer.
    bad[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
    let malformed = MessageView::<Address>::from_unpacked(&bad, Default::default())?;
    let mut dest = Message::<Person>::new()?;
    dest.edit()
        .address()
        .ensure()?
        .city()
        .copy_from("preserved")?;
    assert!(dest.edit().address().copy_from(malformed.read()).is_err());
    assert_eq!(dest.read().address()?.city()?, "preserved");
    Ok(())
}
#[test]
fn generics_and_legacy_rpc_builder_bridge() -> capnp::Result<()> {
    let mut m = Message::<Types>::new()?;
    m.edit()
        .r#box()
        .ensure()?
        .value()
        .copy_from("generic".into())?;
    assert_eq!(m.read().r#box()?.value()?.to_str()?, "generic");
    assert_eq!(m.read().signed(), -25);
    let mut source = Message::<Person>::new()?;
    source.edit().name().copy_from("generic struct")?;
    m.edit()
        .nested()
        .ensure()?
        .outer()
        .copy_from(source.read())?;
    m.edit()
        .nested()
        .ensure()?
        .inner()
        .copy_from("inner".into())?;
    assert_eq!(m.read().nested()?.outer()?.name()?, "generic struct");
    assert_eq!(m.read().nested()?.inner()?.to_str()?, "inner");

    let mut old = capnp::message::Builder::new_default();
    {
        let mut modern = old.init_root::<person::Builder<'_>>().into_api();
        modern.id().set(123);
    }
    assert_eq!(
        old.get_root_as_reader::<person::Reader<'_>>()?
            .into_api()
            .id(),
        123
    );
    Ok(())
}

#[test]
fn group_union_selection_is_lazy_and_clears_overlapping_storage() -> capnp::Result<()> {
    use capntproto_test_support::field_api_capnp::api::{Choice, ChoiceUnionRef, ChoiceUnionTag};
    let mut m = Message::<Choice>::new()?;
    m.edit().untouched().set(123);
    m.edit().text().copy_from("old arm")?;
    {
        let mut c = m.edit();
        let _ = c.details();
    }
    assert_eq!(m.read().tag(), ChoiceUnionTag::Text);
    assert_eq!(
        m.edit().details().edit().err().unwrap().kind,
        ErrorKind::NotPresent
    );
    {
        let mut c = m.edit();
        let mut g = c.details().ensure();
        assert_eq!(g.read().count(), 0);
        assert_eq!(g.read().label()?, "");
        g.count().set(91);
        g.label().copy_from("label")?;
        g.nested().value().copy_from("nested")?;
    }
    assert!(matches!(m.read().which()?,ChoiceUnionRef::Details(g) if g.count()==91));
    m.edit().text().copy_from("new text")?;
    assert_eq!(m.read().text()?, "new text");
    m.edit().details().replace();
    assert_eq!(m.read().details()?.count(), 0);
    assert_eq!(m.read().details()?.label()?, "");
    assert_eq!(m.read().untouched(), 123);
    Ok(())
}

#[test]
fn old_struct_layout_requires_explicit_upgrade() -> capnp::Result<()> {
    use capntproto_test_support::field_api_capnp::api::{Evolving, NewRecord, OldRecord};
    let mut old = Message::<OldRecord>::new()?;
    old.edit().value().set(99);
    let bytes = old.to_vec();
    let view = MessageView::<NewRecord>::from_unpacked(&bytes, Default::default())?;
    assert_eq!(view.read().extra()?, "");
    let mut dest = Message::<Evolving>::new()?;
    dest.edit().record().copy_from(view.read())?;
    let before = dest.to_vec();
    assert_eq!(
        dest.edit().record().edit().err().unwrap().kind,
        ErrorKind::NeedsUpgrade
    );
    assert_eq!(dest.to_vec(), before);
    dest.edit().record().ensure()?.extra().copy_from("added")?;
    assert_eq!(dest.read().record()?.value(), 99);
    assert_eq!(dest.read().record()?.extra()?, "added");
    Ok(())
}

#[test]
fn unions_decode_only_selected_payload_and_preserve_unknown_tags() -> capnp::Result<()> {
    use capntproto_test_support::field_api_capnp::{
        api::{Choice, ChoiceUnionRef, ChoiceUnionTag},
        choice,
    };
    let mut old = capnp::message::Builder::new_default();
    old.init_root::<choice::Builder<'_>>()
        .set_text("unique union text");
    let mut bytes = capnp::serialize::write_message_to_words(&old);
    let at = bytes
        .windows(17)
        .position(|v| v == b"unique union text")
        .unwrap();
    bytes[at] = 255;
    let view = MessageView::<Choice>::from_unpacked(&bytes, Default::default())?;
    assert_eq!(view.read().tag(), ChoiceUnionTag::Text);
    assert!(view.read().which().is_err());
    // Locate the discriminant using the difference between two valid encodings.
    old.get_root::<choice::Builder<'_>>()?.set_empty(());
    let empty = capnp::serialize::write_message_to_words(&old);
    let text = {
        let mut b = capnp::message::Builder::new_default();
        b.init_root::<choice::Builder<'_>>().set_text("");
        capnp::serialize::write_message_to_words(&b)
    };
    let offset = (16..32).find(|&i| empty[i] != text[i]).unwrap();
    bytes[offset..offset + 2].copy_from_slice(&60000u16.to_le_bytes());
    let unknown = MessageView::<Choice>::from_unpacked(&bytes, Default::default())?;
    assert_eq!(unknown.read().tag(), ChoiceUnionTag::Unknown(60000));
    assert!(matches!(
        unknown.read().which()?,
        ChoiceUnionRef::Unknown(60000)
    ));
    Ok(())
}

#[test]
fn capabilities_keep_identity_and_release_replaced_references() -> capnp::Result<()> {
    use capntproto_test_support::field_api_capnp::service;
    use std::{cell::Cell, rc::Rc};
    struct Server(Rc<Cell<usize>>);
    impl service::Server for Server {}
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let drops = Rc::new(Cell::new(0));
    let client: service::Client = capnp_rpc::new_client(Server(drops.clone()));
    let identity = client.client.hook.get_ptr();
    let mut m = Message::<Types>::new()?;
    m.edit().cap().copy_from(client)?;
    assert_eq!(m.read().cap()?.client.hook.get_ptr(), identity);
    let compact = m.compact_copy()?;
    assert_eq!(compact.read().cap()?.client.hook.get_ptr(), identity);
    m.edit().cap().clear();
    assert_eq!(drops.get(), 0);
    drop(compact);
    assert_eq!(drops.get(), 1);
    Ok(())
}

#[test]
fn pointer_list_iteration_propagates_utf8_errors_and_shares_limits() -> capnp::Result<()> {
    let mut m = Message::<Person>::new()?;
    m.edit()
        .names()
        .init_with(2, |i, name| name.copy_from(["valid", "invalid marker"][i]))?;
    let mut bytes = m.to_vec();
    let offset = bytes
        .windows(14)
        .position(|v| v == b"invalid marker")
        .unwrap();
    bytes[offset] = 255;
    let view = MessageView::<Person>::from_unpacked(&bytes, Default::default())?;
    let mut iter = view.read().names()?.into_iter();
    assert_eq!(iter.next().unwrap()?, "valid");
    assert!(iter.next().unwrap().is_err());
    assert!(iter.next().is_none());
    let mut limits = capnp::message::ReaderOptions::new();
    limits.traversal_limit_in_words(Some(40));
    let view = MessageView::<Person>::from_unpacked(&bytes, limits)?;
    let mut exhausted = false;
    for _ in 0..100 {
        if view.read().names().is_err() {
            exhausted = true;
            break;
        }
    }
    assert!(
        exhausted,
        "nested reads must debit the same reader's traversal budget"
    );
    Ok(())
}

#[test]
fn staging_works_across_segments_and_unwinding_keeps_old_value() -> capnp::Result<()> {
    let allocator = capnp::message::HeapAllocator::new()
        .first_segment_words(1)
        .allocation_strategy(capnp::message::AllocationStrategy::FixedSize);
    let mut m = Message::<Person>::with_allocator(allocator)?;
    m.edit()
        .names()
        .init_with(2, |_, name| name.copy_from("old value"))?;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = m.edit().names().replace_with(3, |i, name| {
            name.copy_from("candidate in another segment")?;
            if i == 2 {
                panic!("abort staged fill");
            }
            Ok(())
        });
    }));
    assert!(result.is_err());
    assert_eq!(m.read().names()?.len(), 2);
    assert_eq!(m.read().names()?.get(0).unwrap()?, "old value");
    m.edit()
        .names()
        .replace_with(1, |_, name| name.copy_from("committed"))?;
    let bytes = m.to_vec();
    assert_eq!(
        MessageView::<Person>::from_unpacked(&bytes, Default::default())?
            .read()
            .names()?
            .get(0)
            .unwrap()?,
        "committed"
    );
    Ok(())
}

#[test]
fn annotations_and_keywords_keep_schema_identity() -> capnp::Result<()> {
    use capntproto_test_support::field_api_capnp::api::{RenamedEnum, RenamedFields};
    let mut m = Message::<RenamedFields>::new()?;
    m.edit().label().copy_from("renamed accessor")?;
    m.edit().r#type().copy_from("keyword")?;
    m.edit().choice().set(RenamedEnum::Self_);
    assert_eq!(m.read().choice(), RenamedEnum::Self_);
    assert_eq!(RenamedEnum::from(0), RenamedEnum::Renamed);
    assert_eq!(RenamedEnum::from(2), RenamedEnum::Unknown_);
    assert_eq!(RenamedFields::LABEL.name, "read");
    assert_eq!(m.read().label()?, "renamed accessor");
    assert_eq!(m.read().r#type()?, "keyword");
    Ok(())
}
