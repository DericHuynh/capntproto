use super::*;
use std::ops::Range;

type Input = BufferedRead<futures::io::Cursor<Vec<u8>>>;
type SharedFrame = (Arc<Vec<u8>>, Range<usize>);

fn frames(count: usize) -> Vec<Vec<u8>> {
    (0..count)
        .map(|value| {
            let mut message = capnp::message::Builder::new_default();
            message
                .initn_root::<capnp::data::Builder>(32)
                .fill(value as u8);
            capnp::serialize::write_message_to_words(&message)
        })
        .collect()
}

fn input(frames: &[Vec<u8>]) -> Input {
    BufferedRead::with_buffer_size(
        futures::io::Cursor::new(frames.concat()),
        Default::default(),
        MIN_BUFFER_WORDS,
    )
    .unwrap()
}

fn read(input: &mut Input, cx: &mut Context<'_>) -> SharedFrame {
    assert!(matches!(
        input.poll_borrowed_frame(cx),
        Poll::Ready(Ok(Some(_)))
    ));
    let frame = input.share_borrowed_frame().unwrap();
    input.finish_borrowed_frame();
    frame
}

#[test]
fn shared_frames_reuse_released_allocations_without_overwriting_retained_bytes_or_successors() {
    let frames = frames(16);
    let mut input = input(&frames);
    let mut cx = Context::from_waker(std::task::Waker::noop());
    let first = read(&mut input, &mut cx);
    let first_address = first.0.as_ptr();
    let second = read(&mut input, &mut cx);
    let second_address = second.0.as_ptr();
    assert_ne!(first_address, second_address);
    assert_eq!(&first.0[first.1], frames[0]);
    assert_eq!(&second.0[second.1.clone()], frames[1]);
    drop(first.0);
    let mut retained = second;

    let allocations = allocation_counter::measure(|| {
        for index in 2..frames.len() {
            // The preceding result remains live while probing its successor,
            // just as the RPC receive loop runs before waking the client.
            let next = read(&mut input, &mut cx);
            assert_eq!(&retained.0[retained.1.clone()], frames[index - 1]);
            assert_eq!(&next.0[next.1.clone()], frames[index]);
            assert_eq!(next.0.as_ptr(), [first_address, second_address][index % 2]);
            retained = next;
        }
    });
    assert_eq!(allocations.count_total, 0);
    assert_eq!(input.consumed_bytes(), frames.concat().len() as u64);
    drop(retained);
    assert!(matches!(
        input.poll_borrowed_frame(&mut cx),
        Poll::Ready(Ok(None))
    ));
}

#[test]
fn retained_spare_owners_prevent_reuse() {
    let frames = frames(3);
    let mut input = input(&frames);
    let mut cx = Context::from_waker(std::task::Waker::noop());
    let first = read(&mut input, &mut cx);
    let second = read(&mut input, &mut cx);
    let third = read(&mut input, &mut cx);
    assert_ne!(third.0.as_ptr(), first.0.as_ptr());
    assert_ne!(third.0.as_ptr(), second.0.as_ptr());
    assert_eq!(&first.0[first.1], frames[0]);
    assert_eq!(&second.0[second.1], frames[1]);
    assert_eq!(&third.0[third.1], frames[2]);
}

#[test]
fn weak_observers_of_a_spare_allocation_prevent_reuse_and_expire_on_the_next_detach() {
    let frames = frames(3);
    let mut input = input(&frames);
    let mut cx = Context::from_waker(std::task::Waker::noop());
    let first = read(&mut input, &mut cx);
    let second = read(&mut input, &mut cx);
    // This weak observer is created after the first allocation entered the
    // spare slot. It must still prevent mutable reuse on the next receive.
    let weak = Arc::downgrade(&first.0);
    drop(first);
    let third = read(&mut input, &mut cx);
    assert!(weak.upgrade().is_none());
    assert_eq!(&second.0[second.1], frames[1]);
    assert_eq!(&third.0[third.1], frames[2]);
}

#[test]
fn shared_recycling_preserves_capacity_and_rejects_oversized_retention() {
    let mut input = input(&[]);
    input.buffer = ReceiveBuffer::Bytes(AlignedBuffer::new(MIN_BUFFER_WORDS * 2));
    input.shared_spare = Some(AlignedBuffer::new(MIN_BUFFER_WORDS));
    input.shared_borrowed = true;
    let owner = input.buffer.shared_bytes().allocation.clone();
    input.prepare_shared_buffer();
    assert_eq!(input.buffer.len(), MIN_BUFFER_WORDS * 2);
    assert!(!Arc::ptr_eq(
        &owner,
        &input.buffer.shared_bytes().allocation
    ));
    assert_eq!(
        input.shared_spare.as_ref().unwrap().len(),
        MIN_BUFFER_WORDS * 2
    );

    // Capacity is the retention bound, including a currently unused suffix.
    // No large initialized payload is necessary to exercise this boundary.
    let oversized = Arc::new(Vec::with_capacity(MAX_CACHED_SPILL_WORDS * 8 + 1));
    input.buffer = ReceiveBuffer::Bytes(AlignedBuffer {
        allocation: oversized.clone(),
        offset: 0,
        length: 0,
    });
    input.shared_borrowed = true;
    input.prepare_shared_buffer();
    assert!(input.shared_spare.is_none());
    assert_eq!(Arc::strong_count(&oversized), 1);
}
