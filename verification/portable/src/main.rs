//! Bare-metal link fixture, not board startup code. No global allocator exists.
//! Extend this fixture as RPC transitions move into the portable engine.
#![no_std]
#![no_main]

use capntproto_rpc_engine::flow_control::{Error, Window};
use core::{hint::black_box, time::Duration};

// SAFETY: This is the only exported entry symbol in this link-only executable.
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    capntproto_portable_check::exercise_generated_api().unwrap();
    let mut words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 32];
    let storage =
        capnp::message::SingleSegmentAllocator::new(capnp::Word::words_to_bytes_mut(&mut words));
    let mut message = capnp::message::Builder::new(storage);
    message.set_root(black_box("portable RPC credit")).unwrap();
    let text: capnp::text::Reader<'_> = message.get_root_as_reader().unwrap();
    black_box(text.as_bytes());

    let frame = [
        capnp::word(1, 0, 0, 0, 1, 0, 0, 0),
        capnp::word(0, 0, 0, 0, 0, 0, 0, 0),
        capnp::word(0, 0, 0, 0, 0, 0, 0, 0),
    ];
    let mut input = black_box(capnp::Word::words_to_bytes(&frame));
    let mut index = [capnp::serialize::SegmentOffset::EMPTY; 2];
    let segments = capnp::serialize::IndexedSliceSegments::from_slice(
        &mut input,
        &mut index,
        capnp::message::ReaderOptions::new(),
    )
    .unwrap();
    let reader = capnp::message::Reader::new(segments, capnp::message::ReaderOptions::new());
    assert!(reader
        .get_root::<capnp::any_pointer::Reader<'_>>()
        .unwrap()
        .is_null());

    let mut window = Window::fixed(black_box(1024));
    let receipt = window.sent(black_box(2048), Duration::ZERO).unwrap();
    assert!(window.is_ready());
    window.ack(receipt, Duration::ZERO).unwrap();
    assert_eq!(window.in_flight(), 0);
    let mut adaptive = Window::adaptive(65536);
    let receipt = adaptive.sent(1, Duration::from_secs(2)).unwrap();
    assert_eq!(
        adaptive.ack(receipt, Duration::from_secs(1)),
        Err(Error::ClockWentBackwards)
    );
    black_box(adaptive.in_flight());
    let mut question = capntproto_rpc_engine::question::Question::new();
    let finish = question.release();
    assert!(!finish.reclaim);
    assert_eq!(
        question.returned(false),
        Ok(capntproto_rpc_engine::question::Delivery::Discard)
    );
    assert!(question.release().reclaim);
    // Exercise the high-ID/no-Return ledger in the binary that links without
    // an allocator, including its independent ordinary fallback capacity.
    use capntproto_rpc_engine::{ids::WireId, questions};
    let mut ordinary = [questions::Slot::EMPTY];
    let mut pipeline = [questions::PipelineSlot::EMPTY];
    questions::with_pipeline_questions(&mut ordinary, &mut pipeline, |questions| {
        let local = questions.reserve_for_pipeline().unwrap().accept();
        assert_eq!(local.key().id().to_wire(), black_box(1 << 31));
        let ordinary = questions.reserve_for_pipeline().unwrap();
        assert_eq!(ordinary.id().to_wire(), 0);
        drop(ordinary);
        questions.release(local);
        let finish = questions.pending_finish().unwrap();
        assert!(finish.release_result_caps());
        finish.accept();
        assert!(questions.is_empty());
    })
    .unwrap();
    let mut vacancies = capntproto_rpc_engine::free_slots::FreeSlots::new();
    let mut slots = [const { capntproto_rpc_engine::free_slots::Link::EMPTY }; 3];
    let first = black_box(vacancies.take(&mut slots, |slot| slot));
    let second = black_box(vacancies.take(&mut slots, |slot| slot));
    vacancies.release(second, &mut slots, |slot| slot);
    vacancies.release(first, &mut slots, |slot| slot);
    assert_eq!(vacancies.take(&mut slots, |slot| slot), first);
    assert_eq!(vacancies.take(&mut slots, |slot| slot), second);

    // Link the shared legacy/portable scheduler storage without a global
    // allocator, executor, OS primitives or a hosted waker implementation.
    #[cfg(all(target_has_atomic = "ptr", target_has_atomic = "8"))]
    {
        let ready = capntproto_rpc_engine::ready_queue::ReadyQueue::new();
        let index = black_box(3);
        assert!(ready.enqueue(index));
        assert!(!ready.enqueue(index));
        assert_eq!(ready.front(0), Some(index));
        ready.consume(0, index);
        assert!(!ready.pending(1));
    }
    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
