//! Allocation contracts measured only on this test binary's current thread.
use capnp::{field_api::Message, message::ReaderOptions};
use capntproto_test_support::field_api_capnp::api::Person;
use std::hint::black_box;

#[test]
fn counter_detects_an_allocation() {
    let counts = allocation_counter::measure(|| {
        drop(black_box(Box::new([0u8; 128])));
    });
    assert_eq!(counts.count_total, 1);
    assert_eq!(counts.bytes_total, 128);
    assert_eq!(counts.count_current, 0);
}

#[test]
fn basic_errors_and_empty_diagnostics_remain_allocation_free() {
    use capnp::{Error, ErrorKind};
    let counts = allocation_counter::measure(|| {
        for kind in [
            ErrorKind::ReadLimitExceeded,
            ErrorKind::InvalidReadCount(usize::MAX, usize::MAX - 1),
            ErrorKind::InsufficientCapacity {
                requested_words: u32::MAX,
                available_words: 1,
            },
        ] {
            let mut error = Error::from_kind(black_box(kind));
            error.write_fmt(format_args!(""));
            assert!(error.extra.is_empty());
            assert!(error.remote_trace().is_none());
            assert!(error.detail(0).is_none());
            assert_eq!(error.details().count(), 0);
            let cloned = black_box(error.clone());
            assert_eq!(cloned.kind, kind);
            assert!(cloned.into_extra().is_empty());
        }
        assert!(Error::failed(String::new()).into_extra().is_empty());
    });
    assert_eq!(counts.count_total, 0, "{counts:?}");
    assert_eq!(counts.bytes_total, 0);
}

#[test]
fn caller_indexed_readers_and_capacity_errors_do_not_allocate() {
    use capnp::{
        message::{Builder, HeapAllocator},
        serialize::{IndexedSliceSegments, SegmentOffset},
    };
    let mut message = Builder::new(HeapAllocator::new().first_segment_words(1));
    message
        .set_root("far pointer into a second segment")
        .unwrap();
    assert!(message.get_segments_for_output().len() > 1);
    let bytes = capnp::serialize::write_message_to_words(&message);
    let counts = allocation_counter::measure(|| {
        for _ in 0..128 {
            let mut input = bytes.as_slice();
            let mut index = [SegmentOffset::EMPTY; 8];
            let segments =
                IndexedSliceSegments::from_slice(&mut input, &mut index, ReaderOptions::new())
                    .unwrap();
            let reader = capnp::message::Reader::new(segments, ReaderOptions::new());
            assert_eq!(
                reader
                    .get_root::<capnp::text::Reader<'_>>()
                    .unwrap()
                    .as_bytes(),
                b"far pointer into a second segment"
            );
            let mut input = bytes.as_slice();
            assert!(
                IndexedSliceSegments::from_slice(&mut input, &mut [], ReaderOptions::new())
                    .is_err()
            );
            assert_eq!(input, bytes.as_slice());
        }
    });
    assert_eq!(counts.count_total, 0, "{counts:?}");
}

#[test]
fn single_segment_builders_allocate_only_the_word_buffer() {
    use capnp::message::{Builder, HeapAllocator, ScratchSpaceHeapAllocator};
    let counts = allocation_counter::measure(|| {
        for _ in 0..128 {
            let mut message = Builder::new(HeapAllocator::new().first_segment_words(64));
            message
                .set_root::<capnp::text::Owned>("inline segment")
                .unwrap();
            assert_eq!(message.get_segments_for_output().len(), 1);
            black_box(message);
        }
    });
    assert_eq!(counts.count_total, 128, "{counts:?}");
    assert_eq!(counts.bytes_total, 128 * 64 * 8, "{counts:?}");
    assert_eq!(counts.count_current, 0);

    let mut words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 64];
    let counts = allocation_counter::measure(|| {
        let mut message = Builder::new(ScratchSpaceHeapAllocator::new(
            capnp::Word::words_to_bytes_mut(&mut words),
        ));
        message.set_root::<capnp::text::Owned>("scratch").unwrap();
        assert_eq!(message.get_segments_for_output().len(), 1);
        black_box(message);
    });
    assert_eq!(counts.count_total, 0, "{counts:?}");
}

#[test]
fn pooled_builders_reuse_storage_without_allocations_or_stale_data() {
    use capnp::message::{Builder, HeapAllocator, SegmentPool};
    let pool = SegmentPool::new(256, 4);
    let allocator = || {
        HeapAllocator::new()
            .first_segment_words(64)
            .segment_pool(pool.clone())
    };
    let fill = |value| {
        let mut message = Builder::new(allocator());
        let data = message.initn_root::<capnp::data::Builder<'_>>(128);
        assert!(data.iter().all(|&byte| byte == 0));
        data.fill(value);
        message
    };
    drop(fill(0xff));
    let counts = allocation_counter::measure(|| {
        for i in 0..128 {
            black_box(fill(i));
        }
    });
    assert_eq!(counts.count_total, 0, "{counts:?}");
    // Holding a live message cannot expose its storage to a later builder.
    let first = fill(0x17);
    let second = fill(0x23);
    assert!(first
        .get_root_as_reader::<capnp::data::Reader<'_>>()
        .unwrap()
        .iter()
        .all(|&b| b == 0x17));
    assert!(second
        .get_root_as_reader::<capnp::data::Reader<'_>>()
        .unwrap()
        .iter()
        .all(|&b| b == 0x23));
    drop(first);
    drop(fill(0x31));
    assert!(second
        .get_root_as_reader::<capnp::data::Reader<'_>>()
        .unwrap()
        .iter()
        .all(|&b| b == 0x23));
}

#[test]
fn moved_builder_preserves_first_segment_and_spilled_segment_ids() {
    use capnp::message::{AllocationStrategy, Builder, HeapAllocator};
    let mut message = Builder::new(
        HeapAllocator::new()
            .first_segment_words(4)
            .allocation_strategy(AllocationStrategy::FixedSize),
    );
    let mut list = message.initn_root::<capnp::text_list::Builder<'_>>(24);
    for i in 0..24 {
        list.set(i, format!("spilled segment {i}"));
    }
    // Force the arena metadata to move; all segment storage must remain valid.
    let messages = vec![message];
    let message = black_box(messages).pop().unwrap();
    let segments = message.get_segments_for_output();
    assert!(segments.len() > 20);
    let reader = capnp::message::Reader::new(segments, ReaderOptions::new());
    let list = reader.get_root::<capnp::text_list::Reader<'_>>().unwrap();
    for i in 0..24 {
        assert_eq!(
            list.get(i).unwrap().to_str().unwrap(),
            format!("spilled segment {i}")
        );
    }
}

#[test]
fn steady_write_batches_allocate_only_completion_receipts() {
    use futures::{Future, FutureExt};
    use std::{task::Context, time::Duration};

    let (sender, driver) =
        capnp_futures::write_queue_with_clock(futures::io::sink(), || Duration::ZERO);
    let mut driver = Box::pin(driver);
    let mut cx = Context::from_waker(futures::task::noop_waker_ref());
    let mut message = capnp::message::Builder::new_default();
    message.set_root::<capnp::text::Owned>("reuse").unwrap();
    // Warm both buffers: producers can enqueue while another batch is active.
    for _ in 0..2 {
        let receipt = sender.send(message);
        assert!(driver.as_mut().poll(&mut cx).is_pending());
        message = receipt.now_or_never().unwrap().unwrap();
    }
    let counts = allocation_counter::measure(|| {
        for _ in 0..128 {
            let receipt = sender.send(message);
            assert!(driver.as_mut().poll(&mut cx).is_pending());
            message = receipt.now_or_never().unwrap().unwrap();
        }
        drop(message);
    });
    // Message arenas and queue storage are reused; each completion channel
    // owns one allocation. Framing must not add per-message allocations.
    assert_eq!(counts.count_total, 128, "{counts:?}");
}

#[test]
fn detached_write_batches_do_not_allocate_after_warmup() {
    use futures::Future;
    use std::{rc::Rc, task::Context, time::Duration};

    let (sender, driver) =
        capnp_futures::write_queue_with_clock(futures::io::sink(), || Duration::ZERO);
    let mut driver = Box::pin(driver);
    let mut cx = Context::from_waker(futures::task::noop_waker_ref());
    let mut message = capnp::message::Builder::new_default();
    message.set_root::<capnp::text::Owned>("detached").unwrap();
    let message = Rc::new(message);
    for _ in 0..2 {
        sender.send_detached(message.clone());
        assert!(driver.as_mut().poll(&mut cx).is_pending());
    }
    let counts = allocation_counter::measure(|| {
        for _ in 0..128 {
            sender.send_detached(message.clone());
            assert!(driver.as_mut().poll(&mut cx).is_pending());
            assert_eq!(Rc::strong_count(&message), 1);
        }
    });
    assert_eq!(counts.count_total, 0, "{counts:?}");
}

#[test]
fn borrowed_generated_fields_do_not_allocate() {
    let mut message = Message::<Person>::new().unwrap();
    message.edit().id().set(73);
    message.edit().name().copy_from("borrowed").unwrap();
    message
        .edit()
        .address()
        .ensure()
        .unwrap()
        .city()
        .copy_from("Edmonton")
        .unwrap();
    let counts = allocation_counter::measure(|| {
        for _ in 0..128 {
            let view = black_box(&message).read();
            assert_eq!(view.id(), 73);
            assert_eq!(view.name().unwrap(), "borrowed");
            assert_eq!(view.address().unwrap().city().unwrap(), "Edmonton");
        }
    });
    assert_eq!(counts.count_total, 0, "{counts:?}");
    assert_eq!(counts.bytes_total, 0);
}

#[test]
fn framing_and_decoding_reuse_caller_storage() {
    let mut message = capnp::message::Builder::new_default();
    message
        .set_root::<capnp::text::Owned>("allocation budget")
        .unwrap();
    let bytes = capnp::serialize::write_message_to_words(&message);
    let mut buffer = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 64];
    let counts = allocation_counter::measure(|| {
        for _ in 0..128 {
            let reader = capnp::serialize::read_message_no_alloc(
                black_box(bytes.as_slice()),
                capnp::Word::words_to_bytes_mut(&mut buffer),
                ReaderOptions::new(),
            )
            .unwrap();
            assert_eq!(
                reader.get_root::<capnp::text::Reader<'_>>().unwrap(),
                "allocation budget"
            );
        }
    });
    assert_eq!(counts.count_total, 0, "{counts:?}");
    assert_eq!(counts.bytes_total, 0);
}

#[test]
fn bounded_builders_do_not_allocate_on_success_exhaustion_or_schema_mismatch() {
    use capnp::{field_api::Schema, message::FixedBuilder, ErrorKind};
    use capntproto_test_support::field_api_capnp::api::{Address, Choice, NewRecord, OldRecord};
    let mut words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 64];
    let oversized = [b'x'; 1024];
    let oversized = core::str::from_utf8(&oversized).unwrap();
    let counts = allocation_counter::measure(|| {
        for _ in 0..64 {
            let mut message = FixedBuilder::new(&mut words).unwrap();
            let mut person = message.try_init_root::<Person>().unwrap();
            person.scalar(Person::ID).set(73);
            person.scalar(Person::ENABLED).set(false);
            person.scalar(Person::RATIO).set(3.5);
            person.field(Person::NAME).copy_from("bounded").unwrap();
            person
                .field(Person::ADDRESS)
                .init()
                .unwrap()
                .field(Address::CITY)
                .copy_from("Edmonton")
                .unwrap();
            assert_eq!(person.read().id(), 73);
            assert!(!person.read().enabled());
            assert_eq!(person.read().ratio(), 3.5);
            assert_eq!(person.read().address().unwrap().city().unwrap(), "Edmonton");
            let before = message.size_in_words();
            assert!(matches!(
                message
                    .try_get_root::<Person>()
                    .unwrap()
                    .field(Person::NAME)
                    .copy_from(oversized)
                    .err()
                    .unwrap()
                    .kind,
                ErrorKind::InsufficientCapacity { .. }
            ));
            assert_eq!(message.size_in_words(), before);
            message.reset();
            message
                .try_init_root::<OldRecord>()
                .unwrap()
                .scalar(OldRecord::VALUE)
                .set(19);
            let before = message.size_in_words();
            assert_eq!(
                message.try_get_root::<NewRecord>().err().unwrap().kind,
                ErrorKind::NeedsUpgrade
            );
            assert_eq!(message.size_in_words(), before);
            assert_eq!(
                message.try_get_root::<OldRecord>().unwrap().read().value(),
                19
            );
        }
        // A union tag and its prior payload survive failed publication at capacity.
        let cap = 1 + Choice::SIZE.total() as usize + 1;
        let mut message = FixedBuilder::new(&mut words[..cap]).unwrap();
        message
            .try_init_root::<Choice>()
            .unwrap()
            .field(Choice::TEXT)
            .copy_from("old")
            .unwrap();
        assert_eq!(message.remaining_words(), 0);
        assert!(message
            .try_get_root::<Choice>()
            .unwrap()
            .field(Choice::TEXT)
            .copy_from("too long for available storage")
            .is_err());
        assert_eq!(
            message
                .try_get_root::<Choice>()
                .unwrap()
                .read()
                .text()
                .unwrap(),
            "old"
        );
    });
    assert_eq!(counts.count_total, 0, "{counts:?}");
    assert_eq!(counts.bytes_total, 0);
}

#[test]
fn bounded_recursive_copy_lists_groups_and_upgrade_never_allocate() {
    use capnp::{
        field_api::{List, Text},
        message::{FixedBuilder, HeapAllocator},
    };
    use capntproto_test_support::field_api_capnp::{
        self as schema,
        api::{choice, Choice, NewRecord, OldRecord, PhoneNumber},
    };
    // Deliberately force far pointers in the source. Reader construction and
    // hosted source allocation are excluded from the bounded destination count.
    let mut source = capnp::message::Builder::new(HeapAllocator::new().first_segment_words(1));
    let mut person = source.init_root::<schema::person::Builder<'_>>();
    person.set_id(19);
    person.set_name("source");
    person.reborrow().init_address().set_city("city");
    let mut phones = person.reborrow().init_phones(2);
    phones.reborrow().get(0).set_number("first");
    phones.get(1).set_number("second");
    person.init_names(2).set(0, "nested");
    assert!(source.get_segments_for_output().len() > 1);
    let root = source
        .get_root_as_reader::<capnp::any_pointer::Reader<'_>>()
        .unwrap();
    let view = source
        .get_root_as_reader::<schema::person::Reader<'_>>()
        .unwrap()
        .into_api();
    let mut words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 128];
    let mut successes = 0;
    let mut failures = 0;
    let counts = allocation_counter::measure(|| {
        for capacity in 2..=64 {
            let mut dest = FixedBuilder::new(&mut words[..capacity]).unwrap();
            dest.try_set_root_text("old").unwrap();
            match dest.try_copy_root(root) {
                Ok(()) => {
                    successes += 1;
                    assert_eq!(
                        dest.try_get_root::<Person>()
                            .unwrap()
                            .read()
                            .phones()
                            .unwrap()
                            .get(1)
                            .unwrap()
                            .number()
                            .unwrap(),
                        "second"
                    );
                }
                Err(e) => {
                    failures += 1;
                    assert!(matches!(
                        e.kind,
                        capnp::ErrorKind::InsufficientCapacity { .. }
                    ));
                    assert_eq!(dest.size_in_words(), 2);
                    assert_eq!(
                        dest.get_root_as_reader::<capnp::text::Reader<'_>>()
                            .unwrap(),
                        "old"
                    );
                }
            }
        }
        let mut dest = FixedBuilder::new(&mut words).unwrap();
        let mut person = dest.try_init_root::<Person>().unwrap();
        person
            .field(Person::PHONES)
            .copy_from(view.phones().unwrap())
            .unwrap();
        person
            .field(Person::ADDRESS)
            .copy_from(view.address().unwrap())
            .unwrap();
        person
            .field(Person::PHONES)
            .edit()
            .unwrap()
            .get_mut(0)
            .unwrap()
            .field(PhoneNumber::NUMBER)
            .copy_from("changed")
            .unwrap();
        assert_eq!(
            person
                .read()
                .phones()
                .unwrap()
                .get(0)
                .unwrap()
                .number()
                .unwrap(),
            "changed"
        );
        dest.reset();
        dest.try_init_root_list::<List<Text>>(1)
            .unwrap()
            .get_mut(0)
            .unwrap()
            .init(1)
            .unwrap()
            .get_mut(0)
            .unwrap()
            .copy_from("bounded")
            .unwrap();
        dest.reset();
        let mut choice = dest.try_init_root::<Choice>().unwrap();
        choice.scalar(Choice::UNTOUCHED).set(77);
        let mut details = choice.group(Choice::DETAILS).select();
        details.scalar(choice::Details::COUNT).set(3);
        details
            .field(choice::Details::LABEL)
            .copy_from("group")
            .unwrap();
        choice.group(Choice::DETAILS).reset();
        assert_eq!(choice.read().untouched(), 77);
        choice.void(Choice::EMPTY).set();
        dest.reset();
        dest.try_init_root::<OldRecord>()
            .unwrap()
            .scalar(OldRecord::VALUE)
            .set(91);
        let mut newer = dest.try_upgrade_root::<NewRecord>().unwrap();
        newer.field(NewRecord::EXTRA).copy_from("upgraded").unwrap();
        assert_eq!(newer.read().value(), 91);
        dest.reset();
        type Generic = schema::api::Box<capnp::struct_list::Owned<schema::old_record::Owned>>;
        let mut root = dest.try_init_root::<Generic>().unwrap();
        root.field(Generic::VALUE)
            .typed()
            .init(2)
            .unwrap()
            .get_mut(0)
            .unwrap()
            .scalar(OldRecord::VALUE)
            .set(21);
        type Upgraded = schema::api::Box<capnp::struct_list::Owned<schema::new_record::Owned>>;
        let mut root = dest.try_get_root::<Upgraded>().unwrap();
        let mut list = root.field(Upgraded::VALUE).typed().upgrade().unwrap();
        assert_eq!(list.read().get(0).unwrap().value(), 21);
        list.get_mut(0)
            .unwrap()
            .field(NewRecord::EXTRA)
            .copy_from("payload")
            .unwrap();
        // Fail promotion after filling a pointer-list arena. The old payload,
        // union-independent pointer and cursor remain valid with alloc enabled.
        for capacity in 3..=6 {
            let mut small = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 6];
            let mut bounded = FixedBuilder::new(&mut small[..capacity]).unwrap();
            bounded
                .try_init_root_list::<Text>(1)
                .unwrap()
                .get_mut(0)
                .unwrap()
                .copy_from("live")
                .unwrap();
            assert!(bounded.try_upgrade_root_list::<NewRecord>().is_err());
            assert_eq!(bounded.size_in_words(), 3);
            assert_eq!(
                bounded
                    .try_get_root_list::<Text>()
                    .unwrap()
                    .read()
                    .get(0)
                    .unwrap()
                    .unwrap(),
                "live"
            );
        }
        // Exhaustion during a nested list copy leaves its old value and cursor.
        dest.reset();
        dest.try_init_root::<Person>()
            .unwrap()
            .field(Person::NAMES)
            .init(1)
            .unwrap()
            .get_mut(0)
            .unwrap()
            .copy_from("old")
            .unwrap();
        let remaining = dest.remaining_words();
        dest.try_get_root::<Person>()
            .unwrap()
            .field(Person::PAYLOAD)
            .init((remaining as usize - 2) * 8)
            .unwrap();
        let used = dest.size_in_words();
        assert!(dest
            .try_get_root::<Person>()
            .unwrap()
            .field(Person::NAMES)
            .copy_from(view.names().unwrap())
            .is_err());
        assert_eq!(dest.size_in_words(), used);
        assert_eq!(
            dest.try_get_root::<Person>()
                .unwrap()
                .read()
                .names()
                .unwrap()
                .get(0)
                .unwrap()
                .unwrap(),
            "old"
        );
    });
    assert!(successes > 0 && failures > 0);
    assert_eq!(counts.count_total, 0, "{counts:?}");
    assert_eq!(counts.bytes_total, 0);
}

#[test]
fn resumable_async_framing_and_errors_do_not_allocate() {
    use capnp::{serialize::FrameReceiver, ErrorKind, Word};
    use futures::FutureExt;

    let mut builder =
        capnp::message::Builder::new(capnp::message::HeapAllocator::new().first_segment_words(1));
    builder
        .set_root::<capnp::text::Owned>("fixed async receiver")
        .unwrap();
    assert_eq!(builder.get_segments_for_output().len(), 2);
    let bytes = capnp::serialize::write_message_to_words(&builder);
    let mut words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 64];
    struct Broken;
    impl futures::io::AsyncRead for Broken {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
            _: &mut [u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            std::task::Poll::Ready(Err(std::io::ErrorKind::ConnectionReset.into()))
        }
    }
    let counts = allocation_counter::measure(|| {
        let mut receiver =
            FrameReceiver::new(Word::words_to_bytes_mut(&mut words), ReaderOptions::new()).unwrap();
        for _ in 0..128 {
            let mut input = futures::io::Cursor::new(bytes.as_slice());
            let message =
                capnp_futures::serialize::try_read_message_into(&mut input, &mut receiver)
                    .now_or_never()
                    .unwrap()
                    .unwrap()
                    .unwrap();
            assert_eq!(
                message.get_root::<capnp::text::Reader<'_>>().unwrap(),
                "fixed async receiver"
            );
            receiver.next_message().unwrap();
        }
        let error = capnp_futures::serialize::try_read_message_into(&mut Broken, &mut receiver)
            .now_or_never()
            .unwrap()
            .err()
            .unwrap();
        assert_eq!(error.kind, ErrorKind::Disconnected);
        assert_eq!(
            receiver.input().err().unwrap().kind,
            ErrorKind::Disconnected
        );
        let error = capnp_futures::serialize::try_read_message_no_alloc(
            futures::io::Cursor::new(bytes.as_slice()),
            Word::words_to_bytes_mut(&mut words[..2]),
            ReaderOptions::new(),
        )
        .now_or_never()
        .unwrap()
        .err()
        .unwrap();
        assert_eq!(error.kind, ErrorKind::BufferNotLargeEnough);
        // A partial frame fails without formatting an allocated diagnostic.
        let error = capnp_futures::serialize::try_read_message_no_alloc(
            futures::io::Cursor::new(&bytes[..bytes.len() - 1]),
            Word::words_to_bytes_mut(&mut words),
            ReaderOptions::new(),
        )
        .now_or_never()
        .unwrap()
        .err()
        .unwrap();
        assert_eq!(error.kind, ErrorKind::PrematureEndOfFile);
    });
    assert_eq!(counts.count_total, 0, "{counts:?}");
    assert_eq!(counts.bytes_total, 0);
}

#[test]
fn async_scratch_allocates_only_segment_metadata_when_the_payload_fits() {
    use futures::FutureExt;

    let payload = vec![0x5a; 8192];
    let mut message = capnp::message::Builder::new(
        capnp::message::HeapAllocator::new().first_segment_words(2048),
    );
    message
        .set_root::<capnp::data::Owned>(payload.as_slice())
        .unwrap();
    let bytes = capnp::serialize::write_message_to_words(&message);
    assert_eq!(&bytes[..4], &[0; 4], "control must use a single segment");
    let mut scratch = capnp::Word::allocate_zeroed_vec(2048);
    let counts = allocation_counter::measure(|| {
        let reader = capnp_futures::serialize::read_message_with_scratch(
            futures::io::Cursor::new(&bytes),
            &mut scratch,
            ReaderOptions::new(),
        )
        .now_or_never()
        .unwrap()
        .unwrap();
        assert_eq!(reader.get_root::<capnp::data::Reader>().unwrap(), payload);
        assert!(reader.into_segments().uses_scratch());
    });
    // One (start, end) pair; the 8 KiB payload must remain in caller memory.
    assert_eq!(counts.count_total, 1, "{counts:?}");
    assert_eq!(
        counts.bytes_total,
        std::mem::size_of::<(usize, usize)>() as u64
    );
    assert_eq!(counts.count_current, 0);

    let fallback = allocation_counter::measure(|| {
        let reader = capnp_futures::serialize::read_message_with_scratch(
            futures::io::Cursor::new(&bytes),
            &mut [],
            ReaderOptions::new(),
        )
        .now_or_never()
        .unwrap()
        .unwrap();
        assert_eq!(reader.get_root::<capnp::data::Reader>().unwrap(), payload);
        assert!(!reader.into_segments().uses_scratch());
    });
    assert!(fallback.bytes_total >= counts.bytes_total + payload.len() as u64);
    assert_eq!(fallback.count_current, 0);
}

#[test]
fn buffered_two_segment_framing_keeps_metadata_inline() {
    use capnp::message::ReaderSegments;
    use futures::FutureExt;

    let payload = [0x5a; 1024];
    let mut message =
        capnp::message::Builder::new(capnp::message::HeapAllocator::new().first_segment_words(1));
    message
        .set_root::<capnp::data::Owned>(payload.as_slice())
        .unwrap();
    let bytes = capnp::serialize::write_message_to_words(&message);
    assert_eq!(u32::from_le_bytes(bytes[..4].try_into().unwrap()), 1);
    let frames = [bytes.as_slice(), bytes.as_slice()].concat();
    for short in [true, false] {
        let mut input = capnp_futures::BufferedRead::new(
            futures::io::Cursor::new(&frames),
            ReaderOptions::new(),
        );
        let counts = allocation_counter::measure(|| {
            let reader = input
                .try_read_message(|_| Ok(short))
                .now_or_never()
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(reader.get_segments().len(), 2);
            assert_eq!(reader.get_root::<capnp::data::Reader>().unwrap(), payload);
        });
        // The first retained frame creates its payload, cache owner and cache
        // slots. Segment-table metadata stays inline; short-lived frames need
        // none of those allocations. Released retained storage stays cached.
        let cold_allocations = 3 * u64::from(!short);
        assert_eq!(counts.count_total, cold_allocations, "{counts:?}");
        assert_eq!(counts.count_current, 3 * i64::from(!short));
        let warm = allocation_counter::measure(|| {
            let reader = input
                .try_read_message(|_| Ok(short))
                .now_or_never()
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(reader.get_segments().len(), 2);
            assert_eq!(reader.get_root::<capnp::data::Reader>().unwrap(), payload);
        });
        assert_eq!(warm.count_total, 0, "{warm:?}");
        assert_eq!(warm.bytes_total, 0);
        assert_eq!(warm.count_current, 0);
    }
}

#[test]
fn buffered_scratch_avoids_payload_allocations_and_reuses_owned_fallback() {
    use futures::FutureExt;
    let payload = vec![0x5a; 8192];
    let mut message = capnp::message::Builder::new(
        capnp::message::HeapAllocator::new().first_segment_words(2048),
    );
    message
        .set_root::<capnp::data::Owned>(payload.as_slice())
        .unwrap();
    let bytes = capnp::serialize::write_message_to_words(&message);
    let mut words = capnp::Word::allocate_zeroed_vec(bytes.len() / 8);
    let frames = [bytes.as_slice(), bytes.as_slice()].concat();
    let mut input =
        capnp_futures::BufferedRead::new(futures::io::Cursor::new(&frames), ReaderOptions::new());
    let fitting = allocation_counter::measure(|| {
        let message = input
            .try_read_message_with_scratch(&mut words, |_| Ok(false))
            .now_or_never()
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(message.get_segments().uses_scratch());
        assert_eq!(message.get_root::<capnp::data::Reader>().unwrap(), payload);
    });
    assert_eq!(fitting.count_total, 0, "{fitting:?}");
    assert_eq!(fitting.bytes_total, 0);
    assert_eq!(fitting.count_current, 0);
    let mut input =
        capnp_futures::BufferedRead::new(futures::io::Cursor::new(&frames), ReaderOptions::new());
    let fallback = allocation_counter::measure(|| {
        let message = input
            .try_read_message_with_scratch(&mut [], |_| Ok(false))
            .now_or_never()
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!message.get_segments().uses_scratch());
        assert_eq!(message.get_root::<capnp::data::Reader>().unwrap(), payload);
    });
    assert_eq!(
        fallback.count_total,
        fitting.count_total + 3,
        "{fallback:?}"
    );
    assert!(fallback.bytes_total >= fitting.bytes_total + bytes.len() as u64);
    assert_eq!(fallback.count_current, 3);
    let warm = allocation_counter::measure(|| {
        let message = input
            .try_read_message_with_scratch(&mut [], |_| Ok(false))
            .now_or_never()
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!message.get_segments().uses_scratch());
        assert_eq!(message.get_root::<capnp::data::Reader>().unwrap(), payload);
    });
    assert_eq!(warm.count_total, 0, "{warm:?}");
    assert_eq!(warm.bytes_total, 0);
    assert_eq!(warm.count_current, 0);
}
