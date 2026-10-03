use capnp::field_api::{Message, MessageView};
use capnp::ErrorKind;
use capntproto_test_support::field_api_capnp::api::{
    Address, Historical, NewRecord, OldRecord, Person, Transfer, Types,
};
use capntproto_test_support::field_api_capnp::service;
use std::{cell::Cell, rc::Rc};

struct Server(Rc<Cell<usize>>);
impl service::Server for Server {}
impl Drop for Server {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}
fn client(drops: &Rc<Cell<usize>>) -> service::Client {
    capnp_rpc::new_client(Server(drops.clone()))
}

#[test]
fn orphans_move_without_payload_copy_and_cross_arena_errors_return_ownership() -> capnp::Result<()>
{
    let allocator = capnp::message::HeapAllocator::new()
        .first_segment_words(1)
        .allocation_strategy(capnp::message::AllocationStrategy::FixedSize);
    let mut a = Message::<Person>::with_allocator(allocator)?;
    a.edit()
        .address()
        .ensure()?
        .city()
        .copy_from("moved storage")?;
    let location = a.read().address()?.city()?.as_ptr();
    let mut b = Message::<Person>::new()?;
    b.edit()
        .previous_address()
        .ensure()?
        .city()
        .copy_from("other arena")?;
    let (mut root, orphanage) = a.edit_with_orphans();
    let (mut other, wrong) = b.edit_with_orphans();
    assert_eq!(
        root.address().take(&wrong).err().unwrap().kind,
        ErrorKind::WrongArena
    );
    let orphan = root.address().take(&orphanage)?.unwrap();
    assert!(root.read().field(Person::ADDRESS).is_null());
    assert_eq!(root.read().address()?.city()?, "default city");
    let error = other.previous_address().adopt(orphan).unwrap_err();
    assert_eq!(error.error.kind, ErrorKind::WrongArena);
    assert_eq!(other.read().previous_address()?.city()?, "other arena");
    root.previous_address().adopt(error.orphan).unwrap();
    assert_eq!(root.read().previous_address()?.city()?.as_ptr(), location);
    assert!(root.address().take(&orphanage)?.is_none());
    Ok(())
}

#[test]
fn orphan_drop_and_adoption_preserve_capability_ownership() -> capnp::Result<()> {
    let drops = Rc::new(Cell::new(0));
    let old_drops = Rc::new(Cell::new(0));
    let mut message = Message::<Transfer>::new()?;
    let cap = client(&drops);
    let identity = cap.client.hook.get_ptr();
    message.edit().source().init()?.cap().copy_from(cap)?;
    message
        .edit()
        .destination()
        .init()?
        .cap()
        .copy_from(client(&old_drops))?;
    let (mut root, orphanage) = message.edit_with_orphans();
    let orphan = root.source().take(&orphanage)?.unwrap();
    assert_eq!(drops.get(), 0);
    root.destination().adopt(orphan).unwrap();
    assert_eq!(old_drops.get(), 1);
    assert_eq!(
        root.read().destination()?.cap()?.client.hook.get_ptr(),
        identity
    );
    let held = root.read().destination()?.cap()?;
    drop(root.destination().take(&orphanage)?);
    assert_eq!(drops.get(), 0);
    drop(held);
    assert_eq!(drops.get(), 1);
    Ok(())
}

#[test]
fn staged_data_text_and_struct_publication_is_explicit_and_atomic() -> capnp::Result<()> {
    let mut m = Message::<Person>::new()?;
    m.edit().payload().copy_from(b"old")?;
    {
        let mut p = m.edit();
        let pending = p.payload().stage_replace(4)?;
        drop(pending);
    }
    assert_eq!(m.read().payload()?, b"old");
    {
        let mut p = m.edit();
        let pending = p.payload().stage_replace(4)?;
        assert!(pending.read_exact_from(&mut &b"no"[..]).is_err());
    }
    assert_eq!(m.read().payload()?, b"old");
    m.edit()
        .payload()
        .stage_replace(4)?
        .read_exact_from(&mut &b"done"[..])?
        .commit()?;
    assert_eq!(m.read().payload()?, b"done");
    m.edit().name().copy_from("old text")?;
    assert!(m
        .edit()
        .name()
        .stage_replace(2)?
        .read_exact_from(&mut &[255, 255][..])
        .is_err());
    assert_eq!(m.read().name()?, "old text");
    m.edit()
        .name()
        .stage_replace(3)?
        .read_exact_from(&mut &b"new"[..])?
        .commit()?;
    assert_eq!(m.read().name()?, "new");
    m.edit()
        .address()
        .replace_with(|mut a| a.city().copy_from("built"))?;
    assert_eq!(m.read().address()?.city()?, "built");
    assert!(m
        .edit()
        .address()
        .replace_with(|mut a| {
            a.city().copy_from("partial")?;
            Err(capnp::Error::failed("cancel".into()))
        })
        .is_err());
    assert_eq!(m.read().address()?.city()?, "built");
    // Forgetting Draft or Ready can waste initialized arena storage, never publish.
    std::mem::forget(m.edit().payload().stage_replace(100)?);
    std::mem::forget(
        m.edit()
            .payload()
            .stage_replace(4)?
            .read_exact_from(&mut &b"lost"[..])?,
    );
    let bytes = m.to_vec();
    assert_eq!(
        MessageView::<Person>::from_unpacked(&bytes, Default::default())?
            .read()
            .payload()?,
        b"done"
    );
    Ok(())
}

#[test]
fn inline_copy_rejects_newer_layout_and_keeps_value_on_bad_descendant() -> capnp::Result<()> {
    let mut future = Message::<NewRecord>::new()?;
    future.edit().value().set(11);
    future.edit().extra().copy_from("unknown field")?;
    let bytes = future.to_vec();
    let old_view = MessageView::<OldRecord>::from_unpacked(&bytes, Default::default())?;
    let mut dest = Message::<Historical>::new()?;
    dest.edit().records().init_with(1, |_, mut r| {
        r.value().set(42);
        Ok(())
    })?;
    assert_eq!(
        dest.edit()
            .records()
            .edit()?
            .get_mut(0)
            .unwrap()
            .copy_from(old_view.read())
            .unwrap_err()
            .kind,
        ErrorKind::WouldTruncate
    );
    assert_eq!(dest.read().records()?.get(0).unwrap().value(), 42);
    let mut address = Message::<Address>::new()?;
    address.edit().city().copy_from("prior")?;
    let mut malformed = address.to_vec();
    malformed[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
    let view = MessageView::<Address>::from_unpacked(&malformed, Default::default())?;
    assert!(address.edit().copy_from(view.read()).is_err());
    assert_eq!(address.read().city()?, "prior");
    let mut good = Message::<Address>::new()?;
    good.edit().city().copy_from("after")?;
    address.edit().copy_from(good.read())?;
    assert_eq!(address.read().city()?, "after");
    Ok(())
}

#[test]
fn old_list_edit_requires_upgrade_but_generic_edit_does_not_allocate() -> capnp::Result<()> {
    use capntproto_test_support::field_api_capnp::api::Future;
    let mut old = Message::<Historical>::new()?;
    old.edit().records().init_with(1, |_, mut r| {
        r.value().set(17);
        Ok(())
    })?;
    let bytes = old.to_vec();
    let view = MessageView::<Future>::from_unpacked(&bytes, Default::default())?;
    let mut m = Message::<Future>::new()?;
    m.edit().records().copy_from(view.read().records()?)?;
    let before = m.to_vec();
    assert_eq!(
        m.edit().records().edit().err().unwrap().kind,
        ErrorKind::NeedsUpgrade
    );
    assert_eq!(m.to_vec(), before);
    m.edit()
        .records()
        .ensure()?
        .get_mut(0)
        .unwrap()
        .extra()
        .copy_from("added")?;
    assert_eq!(m.read().records()?.get(0).unwrap().value(), 17);
    let mut types = Message::<Types>::new()?;
    types
        .edit()
        .r#box()
        .ensure()?
        .value()
        .copy_from("text".into())?;
    let size = types.size_in_words();
    types
        .edit()
        .r#box()
        .edit()?
        .value()
        .edit()?
        .as_bytes_mut()
        .copy_from_slice(b"edit");
    assert_eq!(types.size_in_words(), size);
    assert_eq!(types.read().r#box()?.value()?.to_str()?, "edit");
    Ok(())
}

#[derive(serde::Deserialize)]
struct Trace {
    kind: String,
    steps: Vec<Step>,
}
#[derive(serde::Deserialize)]
struct Step {
    action: String,
    state: Vec<u8>,
}
fn allocator(far: bool) -> capnp::message::HeapAllocator {
    if far {
        capnp::message::HeapAllocator::new()
            .first_segment_words(1)
            .allocation_strategy(capnp::message::AllocationStrategy::FixedSize)
    } else {
        capnp::message::HeapAllocator::new()
    }
}
fn replay_orphans(trace: &Trace, far: bool) -> capnp::Result<()> {
    let drops: [Rc<Cell<usize>>; 3] = std::array::from_fn(|_| Rc::new(Cell::new(0)));
    let mut a = Message::<Transfer>::with_allocator(allocator(far))?;
    let mut b = Message::<Transfer>::with_allocator(allocator(far))?;
    {
        let mut root = a.edit();
        let mut src = root.source().init()?;
        src.signed().set(1);
        src.cap().copy_from(client(&drops[0]))?;
        let mut dst = root.destination().init()?;
        dst.signed().set(2);
        dst.cap().copy_from(client(&drops[1]))?;
    }
    {
        let mut root = b.edit();
        let mut other = root.destination().init()?;
        other.signed().set(3);
        other.cap().copy_from(client(&drops[2]))?;
    }
    let identity = a.read().source()?.cap()?.client.hook.get_ptr();
    let (mut root, orphanage) = a.edit_with_orphans();
    let mut other = b.edit();
    let mut orphan = None;
    let mut held: Option<service::Client> = None;
    for step in &trace.steps {
        match step.action.as_str() {
            "take" => orphan = root.source().take(&orphanage)?,
            "wrong" => {
                let e = other
                    .destination()
                    .adopt(orphan.take().unwrap())
                    .unwrap_err();
                assert_eq!(e.error.kind, ErrorKind::WrongArena);
                orphan = Some(e.orphan);
            }
            "adopt" => root.destination().adopt(orphan.take().unwrap()).unwrap(),
            "drop-orphan" => drop(orphan.take()),
            "read" => {
                held = Some(if root.read().field(Transfer::SOURCE).is_null() {
                    root.read().destination()?.cap()?
                } else {
                    root.read().source()?.cap()?
                });
            }
            "drop-held" => drop(held.take()),
            "clear-source" => root.source().clear(),
            "clear-dest" => root.destination().clear(),
            _ => panic!("unknown action"),
        }
        let src = if root.read().field(Transfer::SOURCE).is_null() {
            0
        } else {
            root.read().source()?.signed() as u8
        };
        let dst = if root.read().field(Transfer::DESTINATION).is_null() {
            0
        } else {
            root.read().destination()?.signed() as u8
        };
        assert_eq!(
            [
                src,
                dst,
                other.read().destination()?.signed() as u8,
                u8::from(orphan.is_some()),
                u8::from(held.is_some())
            ],
            step.state[..5],
            "{} far={far}",
            step.action
        );
        if let Some(cap) = &held {
            assert_eq!(cap.client.hook.get_ptr(), identity);
        }
        for (i, drop_count) in drops.iter().enumerate() {
            assert_eq!(
                u8::from(drop_count.get() == 0),
                step.state[10 + i],
                "cap {} after {} far={far}",
                i + 1,
                step.action
            );
        }
    }
    Ok(())
}
fn replay_staging(trace: &Trace, far: bool) -> capnp::Result<()> {
    let mut m = Message::<Person>::with_allocator(allocator(far))?;
    m.edit().payload().copy_from(b"old")?;
    {
        let mut root = m.edit();
        let mut draft = Some(root.payload().stage_replace(2)?);
        let mut ready = None;
        for step in &trace.steps {
            match step.action.as_str() {
                "stage" => (),
                "chunk" => {
                    draft.as_mut().unwrap().write_chunk(b"N")?;
                    assert_eq!(
                        draft.as_ref().unwrap().remaining(),
                        2 - usize::from(step.state[1])
                    );
                }
                "ready" => ready = Some(draft.take().unwrap().finish()?),
                "fail" => {
                    assert!(draft
                        .take()
                        .unwrap()
                        .fill_with(|_| Err(capnp::Error::failed("source failed".into())))
                        .is_err());
                }
                "drop" => {
                    drop(draft.take());
                    drop(ready.take());
                }
                "forget" => {
                    std::mem::forget(draft.take());
                    std::mem::forget(ready.take());
                }
                "commit" => ready.take().unwrap().commit()?,
                _ => panic!("unknown action"),
            }
            if let Some(pending) = &mut draft {
                assert_eq!(pending.previous()?, b"old");
            }
            if let Some(pending) = &mut ready {
                assert_eq!(pending.previous()?, b"old");
            }
        }
    }
    // Non-terminal prefixes dropped their pending value at the block boundary.
    let expected = if trace.steps.last().unwrap().state[2] == 2 {
        &b"NN"[..]
    } else {
        &b"old"[..]
    };
    assert_eq!(m.read().payload()?, expected);
    let bytes = m.to_vec();
    assert_eq!(
        MessageView::<Person>::from_unpacked(&bytes, Default::default())?
            .read()
            .payload()?,
        expected
    );
    Ok(())
}
#[test]
fn replay_tlc_field_ownership_traces() -> capnp::Result<()> {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_FIELD_OWNERSHIP_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!traces.is_empty());
    for trace in &traces {
        for far in [false, true] {
            match trace.kind.as_str() {
                "orphans" => replay_orphans(trace, far)?,
                "staging" => replay_staging(trace, far)?,
                _ => panic!("unknown model"),
            }
        }
    }
    Ok(())
}

#[test]
fn branded_sessions_move_values_and_select_union_arms() -> capnp::Result<()> {
    use capntproto_test_support::field_api_capnp::api::{Choice, ChoiceUnionTag};
    let mut m = Message::<Person>::new()?;
    m.scoped_edit(|session| {
        let (mut root, orphanage) = session.into_parts();
        root.edit().address().ensure()?.city().copy_from("scoped")?;
        let orphan = root.field(Person::ADDRESS).take(&orphanage)?.unwrap();
        root.field(Person::PREVIOUS_ADDRESS).adopt(orphan).unwrap();
        assert_eq!(root.read().previous_address()?.city()?, "scoped");
        Ok::<(), capnp::Error>(())
    })?;
    let mut m = Message::<Choice>::new()?;
    m.scoped_edit(|session| {
        let (mut root, orphanage) = session.into_parts();
        root.edit().text().copy_from("union")?;
        let orphan = root.field(Choice::TEXT).take(&orphanage)?.unwrap();
        root.edit()
            .details()
            .ensure()
            .label()
            .copy_from("other arm")?;
        root.field(Choice::TEXT).adopt(orphan).unwrap();
        assert_eq!(root.read().tag(), ChoiceUnionTag::Text);
        assert_eq!(root.read().text()?, "union");
        Ok::<(), capnp::Error>(())
    })?;
    Ok(())
}

#[test]
fn orphan_capability_transfer_includes_unknown_fields_and_nested_lists() -> capnp::Result<()> {
    use capnp::traits::IntoInternalStructReader;
    use capntproto_test_support::field_api_capnp::api::{CapForest, OpaqueRef, OpaqueTransfer};
    let drops = Rc::new(Cell::new(0));
    let mut forest = Message::<CapForest>::new()?;
    forest.edit().items().init_with(2, |_, row| {
        row.init_with(2, |_, cap| cap.copy_from(client(&drops)))
    })?;
    let mut dest = Message::<OpaqueTransfer>::new()?;
    dest.edit()
        .source()
        .copy_from(OpaqueRef::from(forest.read().into_internal_struct_reader()))?;
    drop(forest);
    assert_eq!(drops.get(), 0);
    let (mut root, o) = dest.edit_with_orphans();
    let orphan = root.source().take(&o)?.unwrap();
    root.destination().adopt(orphan).unwrap();
    assert_eq!(drops.get(), 0);
    drop(root.destination().take(&o)?);
    assert_eq!(drops.get(), 4);
    Ok(())
}

#[test]
fn chunked_fill_refuses_short_overlong_and_invalid_text_without_publication() -> capnp::Result<()> {
    let mut m = Message::<Person>::new()?;
    m.edit().name().copy_from("old")?;
    {
        let mut p = m.edit();
        let mut pending = p.name().stage_replace(3)?;
        pending.write_chunk(b"a")?;
        assert_eq!(
            pending.write_chunk(b"too much").unwrap_err().kind,
            ErrorKind::LengthOverflow
        );
        assert_eq!(pending.remaining(), 2);
        assert_eq!(
            pending.finish().err().unwrap().kind,
            ErrorKind::IncompleteFill
        );
    }
    assert_eq!(m.read().name()?, "old");
    {
        let mut p = m.edit();
        let mut pending = p.name().stage_replace(3)?;
        pending.write_chunk(&[0xff])?;
        pending.write_chunk(b"bc")?;
        assert!(pending.finish().is_err());
    }
    assert_eq!(m.read().name()?, "old");
    {
        let mut p = m.edit();
        let mut pending = p.name().stage_replace(3)?;
        pending.write_chunk(b"n")?;
        pending.read_exact_from(&mut &b"ew"[..])?.commit()?;
    }
    assert_eq!(m.read().name()?, "new");
    Ok(())
}

#[test]
fn failed_recursive_copy_releases_partially_copied_capabilities() -> capnp::Result<()> {
    use capnp::traits::{Imbue, ImbueMut};
    use capntproto_test_support::field_api_capnp::types;
    let incoming = Rc::new(Cell::new(0));
    let previous = Rc::new(Cell::new(0));
    let mut source = capnp::message::Builder::new_default();
    let mut caps = Vec::new();
    {
        let mut root = source.init_root::<types::Builder>();
        root.imbue_mut(&mut caps);
        root.set_cap(client(&incoming));
    }
    let mut bytes = capnp::serialize::write_message_to_words(&source);
    let root = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
    let pointer_start = 16 + ((root >> 32) & 65535) as usize * 8;
    // Capability slot 2 is copied before malformed aggregate slot 3.
    bytes[pointer_start + 24..pointer_start + 32].copy_from_slice(&u64::MAX.to_le_bytes());
    let reader =
        capnp::serialize::read_message_from_flat_slice(&mut &bytes[..], Default::default())?;
    let mut raw = reader.get_root::<types::Reader>()?;
    raw.imbue(&caps);
    let mut destination = Message::<Transfer>::new()?;
    destination
        .edit()
        .destination()
        .init()?
        .cap()
        .copy_from(client(&previous))?;
    assert!(destination
        .edit()
        .destination()
        .copy_from(raw.into_api())
        .is_err());
    assert_eq!(previous.get(), 0);
    drop(caps);
    assert_eq!(
        incoming.get(),
        1,
        "failed candidates must not retain the copied capability"
    );
    destination.edit().destination().clear();
    assert_eq!(previous.get(), 1);
    Ok(())
}

#[test]
fn primitive_to_struct_list_edit_cannot_silently_allocate() -> capnp::Result<()> {
    use capntproto_test_support::field_api_capnp::api::{
        GenericHistory, GenericPrimitiveHistory, PrimitiveHistory,
    };
    let mut primitive = Message::<PrimitiveHistory>::new()?;
    primitive.edit().records().init_with(1, |_, r| {
        r.set(71);
        Ok(())
    })?;
    let bytes = primitive.to_vec();
    let view = MessageView::<Historical>::from_unpacked(&bytes, Default::default())?;
    let mut m = Message::<Historical>::new()?;
    m.edit().records().copy_from(view.read().records()?)?;
    let before = m.to_vec();
    assert_eq!(
        m.edit().records().edit().err().unwrap().kind,
        ErrorKind::NeedsUpgrade
    );
    assert_eq!(m.to_vec(), before);
    assert_eq!(
        m.edit()
            .records()
            .ensure()?
            .get_mut(0)
            .unwrap()
            .read()
            .value(),
        71
    );
    let mut old = capnp::message::Builder::new_default();
    old.initn_root::<capnp::primitive_list::Builder<u64>>(1)
        .set(0, 93);
    let mut primitive = Message::<GenericPrimitiveHistory>::new()?;
    primitive
        .edit()
        .records()
        .ensure()?
        .value()
        .copy_from(old.get_root_as_reader()?)?;
    let bytes = primitive.to_vec();
    let view = MessageView::<GenericHistory>::from_unpacked(&bytes, Default::default())?;
    let mut m = Message::<GenericHistory>::new()?;
    m.edit().records().copy_from(view.read().records()?)?;
    let before = m.to_vec();
    assert_eq!(
        m.edit()
            .records()
            .edit()?
            .value()
            .edit()
            .err()
            .unwrap()
            .kind,
        ErrorKind::NeedsUpgrade
    );
    assert_eq!(m.to_vec(), before);
    assert_eq!(
        m.edit()
            .records()
            .edit()?
            .value()
            .ensure()?
            .get(0)
            .read()
            .value(),
        93
    );
    Ok(())
}
