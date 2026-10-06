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

    let (mut sender, driver) =
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

    let (mut sender, driver) =
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
    for short in [true, false] {
        let mut input = capnp_futures::BufferedRead::new(
            futures::io::Cursor::new(&bytes),
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
        // Retained messages allocate their payload; framing itself must not
        // allocate metadata for the common root-plus-large-body layout.
        assert_eq!(counts.count_total, u64::from(!short), "{counts:?}");
        assert_eq!(counts.count_current, 0);
    }
}

#[test]
fn buffered_scratch_avoids_payload_allocations_and_owned_fallback_allocates_once() {
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
    let mut input =
        capnp_futures::BufferedRead::new(futures::io::Cursor::new(&bytes), ReaderOptions::new());
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
        capnp_futures::BufferedRead::new(futures::io::Cursor::new(&bytes), ReaderOptions::new());
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
        fitting.count_total + 1,
        "{fallback:?}"
    );
    assert!(fallback.bytes_total >= fitting.bytes_total + bytes.len() as u64);
    assert_eq!(fallback.count_current, 0);
}
