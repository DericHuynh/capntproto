use capnp::{
    dynamic_struct, dynamic_value,
    field_api::{Message, MessageReader, MessageView},
    message::{AllocationStrategy, HeapAllocator, Reader, ReaderOptions, ReaderSegments},
    ErrorKind, Result, Word,
};
use capntproto_memory_checks::field_api_capnp::{
    api::{Address, NativeRecord, Person, Transfer},
    person, service,
};
use std::{cell::Cell, rc::Rc, sync::Arc};

fn allocator(far: bool) -> HeapAllocator {
    let allocator = HeapAllocator::new();
    if far {
        allocator
            .first_segment_words(1)
            .allocation_strategy(AllocationStrategy::FixedSize)
    } else {
        allocator
    }
}

struct InlineSegments {
    words: [Word; 3],
    drops: Rc<Cell<usize>>,
}
impl ReaderSegments for InlineSegments {
    fn get_segment(&self, index: u32) -> Option<&[u8]> {
        (index == 0).then(|| Word::words_to_bytes(&self.words))
    }
}
impl Drop for InlineSegments {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

#[test]
fn checked_inline_segments_survive_owner_moves_and_extraction() -> Result<()> {
    let drops = Rc::new(Cell::new(0));
    // Independent wire fixture: Address { city = "abc" }. The payload is inline
    // in S itself, so moving the reader's arena really would relocate it.
    let segments = InlineSegments {
        words: [
            capnp::word(0, 0, 0, 0, 0, 0, 1, 0),
            capnp::word(1, 0, 0, 0, 34, 0, 0, 0),
            capnp::word(b'a', b'b', b'c', 0, 0, 0, 0, 0),
        ],
        drops: drops.clone(),
    };
    let owner = MessageReader::<Address, _>::from_segments(segments, ReaderOptions::new())?;
    let address = owner.read().city()?.as_ptr();
    let mut moved = vec![owner];
    let owner = Box::new(moved.pop().unwrap());
    assert_eq!(owner.read().city()?, "abc");
    assert_eq!(owner.read().city()?.as_ptr(), address);
    let copy = owner.compact_copy()?;
    assert_ne!(copy.read().city()?.as_ptr(), address);
    let (reader, caps) = owner.into_parts();
    let owner = MessageReader::<Address, _, _>::from_reader_with_capabilities(reader, caps)?;
    assert_eq!(owner.read().city()?, "abc");
    drop(owner);
    assert_eq!(drops.get(), 1);
    assert_eq!(copy.read().city()?, "abc");
    Ok(())
}

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
fn checked_far_readers_preserve_extracted_capability_ownership() -> Result<()> {
    for far in [false, true] {
        let drops = Rc::new(Cell::new(0));
        let mut message = Message::<NativeRecord>::with_allocator(allocator(far))?;
        message.edit().cap().copy_from(client(&drops))?;
        message.edit().label().copy_from("owned")?;
        let (storage, caps) = message.into_parts();
        assert_eq!(storage.get_segments_for_output().len() > 1, far);
        let bytes = capnp::serialize::write_message_to_words(&storage);
        let reader = capnp::serialize::read_message(&mut bytes.as_slice(), ReaderOptions::new())?;
        let owner =
            MessageReader::<NativeRecord, _, _>::from_reader_with_capabilities(reader, caps)?;
        drop(storage);
        let held = owner.read().cap()?;
        let identity = held.client.hook.get_ptr();
        let owner = Box::new(owner);
        let copy = owner.compact_copy()?;
        assert_eq!(copy.read().cap()?.client.hook.get_ptr(), identity);
        drop(owner);
        assert_eq!(copy.read().label()?, "owned");
        drop(copy);
        assert_eq!(drops.get(), 0);
        drop(held);
        assert_eq!(drops.get(), 1);
    }
    Ok(())
}

#[test]
fn checked_root_preserves_budget_and_checks_descendants_lazily() -> Result<()> {
    let mut message = Message::<Address>::new()?;
    message.edit().city().copy_from("abc")?;
    let bytes = message.to_vec();
    let mut aligned = Word::allocate_zeroed_vec(bytes.len() / 8);
    Word::words_to_bytes_mut(&mut aligned).copy_from_slice(&bytes);
    let mut options = ReaderOptions::new();
    options.traversal_limit_in_words(Some(2));
    let (storage, _) = message.into_parts();
    let segments = storage.get_segments_for_output();
    let owner = MessageReader::<Address, _>::from_segments(&segments[..], options)?;
    // Root pointer and struct validation used both allowed words. Accessing the cached root
    // must not reset the budget, including after extracting the original reader.
    assert_eq!(
        owner.read().city().unwrap_err().kind,
        ErrorKind::ReadLimitExceeded
    );
    let (reader, _) = owner.into_parts();
    assert!(MessageReader::<Address, _>::from_reader(reader).is_err());
    let bytes = Word::words_to_bytes_mut(&mut aligned);
    bytes[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
    let owner = MessageView::<Address>::from_unpacked(bytes, ReaderOptions::new())?;
    assert!(owner.read().city().is_err());
    assert!(owner.compact_copy().is_err());
    Ok(())
}

#[test]
fn orphan_adoption_returns_ownership_on_wrong_arena_and_releases_replaced_caps() -> Result<()> {
    for far in [false, true] {
        let drops = Rc::new(Cell::new(0));
        let replaced = Rc::new(Cell::new(0));
        let mut message = Message::<Transfer>::with_allocator(allocator(far))?;
        message
            .edit()
            .source()
            .init()?
            .cap()
            .copy_from(client(&drops))?;
        message
            .edit()
            .destination()
            .init()?
            .cap()
            .copy_from(client(&replaced))?;
        let mut other = Message::<Transfer>::new()?;
        let (mut root, token) = message.edit_with_orphans();
        let identity = root.read().source()?.cap()?.client.hook.get_ptr();
        let orphan = root.source().take(&token)?.unwrap();
        let error = other.edit().destination().adopt(orphan).unwrap_err();
        assert_eq!(error.error.kind, ErrorKind::WrongArena);
        root.destination().adopt(error.orphan).unwrap();
        assert_eq!(replaced.get(), 1);
        let held = root.read().destination()?.cap()?;
        assert_eq!(held.client.hook.get_ptr(), identity);
        drop(root.destination().take(&token)?);
        assert_eq!(drops.get(), 0);
        drop(held);
        assert_eq!(drops.get(), 1);
    }
    Ok(())
}

#[test]
fn detached_data_resize_reuses_tail_and_preserves_neighbor_allocations() -> Result<()> {
    for far in [false, true] {
        let mut message = capnp::message::Builder::new(allocator(far));
        let (mut root, token) =
            dynamic_value::Builder::from(message.init_root::<person::Builder>())
                .downcast::<dynamic_struct::Builder>()
                .with_orphanage();
        let mut access = token.in_struct(&mut root)?;
        let mut data = access.new_data(24)?;
        access.edit(&mut data, |v| {
            v.downcast::<capnp::data::Builder>().fill(85);
            Ok(())
        })?;
        access.resize(&mut data, 8)?;
        let mut neighbor = access.new_data(8)?;
        access.edit(&mut neighbor, |v| {
            v.downcast::<capnp::data::Builder>().fill(170);
            Ok(())
        })?;
        for size in [40, 0, 17, 1, 65] {
            access.resize(&mut data, size)?;
            access.read(&mut data, |v| {
                let bytes = v.downcast::<capnp::data::Reader>();
                assert_eq!(bytes.len(), size as usize);
                if size == 40 {
                    assert_eq!(&bytes[..8], &[85; 8]);
                    assert_eq!(&bytes[8..], &[0; 32]);
                } else {
                    assert!(bytes.iter().all(|b| *b == 0));
                }
                Ok(())
            })?;
            access.read(&mut neighbor, |v| {
                assert_eq!(v.downcast::<capnp::data::Reader>(), &[170; 8]);
                Ok(())
            })?;
        }
        root.adopt_named("payload", data).unwrap();
        assert_eq!(
            root.reborrow_as_reader()
                .get_named("payload")?
                .downcast::<capnp::data::Reader>(),
            &[0; 65]
        );
    }
    Ok(())
}

#[test]
fn external_data_stays_immutable_and_owned_until_arena_drop() -> Result<()> {
    for far in [false, true] {
        let mut words = Word::allocate_zeroed_vec(4);
        Word::words_to_bytes_mut(&mut words)[..25].fill(85);
        let source: Arc<[Word]> = words.into();
        let weak = Arc::downgrade(&source);
        let address = source.as_ptr().cast::<u8>();
        let mut message = capnp::message::Builder::new(allocator(far));
        {
            let (mut root, token) =
                dynamic_value::Builder::from(message.init_root::<person::Builder>())
                    .downcast::<dynamic_struct::Builder>()
                    .with_orphanage();
            let mut access = token.in_struct(&mut root)?;
            let mut data = access
                .reference_external_data(capnp::dynamic_orphan::ExternalData::new(source, 25)?)?;
            assert!(access.resize(&mut data, 8).is_err());
            assert!(access
                .edit(&mut data, |_| -> Result<()> {
                    panic!("mutable external data escaped")
                })
                .is_err());
            access.read(&mut data, |v| {
                assert_eq!(v.downcast::<capnp::data::Reader>().as_ptr(), address);
                Ok(())
            })?;
            root.adopt_named("payload", data).unwrap();
            assert!(root.reborrow().get_named("payload").is_err());
        }
        let bytes = capnp::serialize::write_message_to_words(&message);
        let reader = capnp::serialize::read_message(&mut bytes.as_slice(), ReaderOptions::new())?;
        assert_eq!(
            reader.get_root::<person::Reader>()?.get_payload()?,
            &[85; 25]
        );
        assert_eq!(weak.strong_count(), 1);
        drop(message);
        assert_eq!(weak.strong_count(), 0);
    }
    Ok(())
}

#[test]
fn staged_publication_handles_errors_unwinding_and_success() -> Result<()> {
    for far in [false, true] {
        let mut message = Message::<Person>::with_allocator(allocator(far))?;
        message.edit().name().copy_from("old")?;
        assert!(message
            .edit()
            .name()
            .stage_replace(4)?
            .read_exact_from(&mut &b"no"[..])
            .is_err());
        assert_eq!(message.read().name()?, "old");
        assert!(message
            .edit()
            .name()
            .stage_replace(2)?
            .read_exact_from(&mut &[255, 255][..])
            .is_err());
        assert_eq!(message.read().name()?, "old");
        message
            .edit()
            .address()
            .replace_with(|mut a| a.city().copy_from("kept"))?;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = message.edit().address().replace_with(|mut a| {
                a.city().copy_from("abandoned")?;
                panic!("injected builder failure");
            });
        }));
        assert!(result.is_err());
        assert_eq!(message.read().address()?.city()?, "kept");
        message
            .edit()
            .name()
            .stage_replace(3)?
            .read_exact_from(&mut &b"new"[..])?
            .commit()?;
        assert_eq!(message.read().name()?, "new");
    }
    Ok(())
}

#[test]
fn misaligned_frames_obey_the_selected_alignment_contract() -> Result<()> {
    #[repr(align(8))]
    struct Aligned([u8; 40]);
    let mut storage = Aligned([0; 40]);
    // Independent single-segment Address { city = "abc" } at byte offset one.
    let frame = &mut storage.0[1..33];
    frame[4..8].copy_from_slice(&3u32.to_le_bytes());
    frame[8..16].copy_from_slice(&[0, 0, 0, 0, 0, 0, 1, 0]);
    frame[16..24].copy_from_slice(&[1, 0, 0, 0, 34, 0, 0, 0]);
    frame[24..28].copy_from_slice(b"abc\0");
    let result = MessageView::<Address>::from_unpacked(frame, ReaderOptions::new());
    if cfg!(feature = "unaligned") {
        assert_eq!(result?.read().city()?, "abc");
    } else {
        assert_eq!(result.err().unwrap().kind, ErrorKind::UnalignedSegment);
    }
    Ok(())
}

#[test]
fn scratch_arena_reuse_zeroes_old_pointer_and_list_storage() -> Result<()> {
    let mut scratch = Word::allocate_zeroed_vec(32);
    let mut allocator =
        capnp::message::ScratchSpaceHeapAllocator::new(Word::words_to_bytes_mut(&mut scratch));
    for size in [0, 9, 64, 1, 0] {
        let mut message = capnp::message::Builder::new(&mut allocator);
        {
            let mut list = message.initn_root::<capnp::primitive_list::Builder<u64>>(size);
            for index in 0..size {
                assert_eq!(list.get(index), 0);
                list.set(index, u64::MAX);
            }
        }
        let segments = message.get_segments_for_output();
        let reader = Reader::new(&segments[..], ReaderOptions::new());
        let list = reader.get_root::<capnp::primitive_list::Reader<u64>>()?;
        assert_eq!(list.len(), size);
        for value in list {
            assert_eq!(value, u64::MAX);
        }
    }
    Ok(())
}
