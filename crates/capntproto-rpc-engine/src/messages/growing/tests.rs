use super::*;
use crate::connection::Growth;
use core::{
    future::Future,
    task::{Context, Poll, Waker},
};

fn sends(maximum: usize) -> Growth {
    Growth {
        initial: 0,
        maximum,
    }
}

#[test]
#[cfg(target_has_atomic = "ptr")]
fn received_allocations_keep_exact_ranges_and_charge_escaping_owners() {
    use alloc::sync::Arc;
    let mut source = Arc::new(b"prefix-payload-suffix".to_vec());
    let charged = source.capacity();
    let mut pool = GrowingBuffers::new(1, 7, charged);
    let shared = with_growing_messages(&mut pool, sends(1), |messages| {
        let lease = messages
            .adopt_received(source.clone(), 7..14)
            .unwrap()
            .unwrap();
        assert_eq!(&*messages.read(&lease), b"payload");
        assert_eq!(messages.read(&lease).as_ptr(), source[7..].as_ptr());
        let offer = messages.offer(&lease).unwrap();
        let key = messages.accept(offer);
        messages.release(lease);
        assert!(!messages.send_window(key).unwrap().advance(2).unwrap());
        let window = messages.send_window(key).unwrap();
        let shared = window.share().unwrap();
        assert_eq!(&*shared, b"yload");
        assert!(window.advance(5).unwrap());
        assert_eq!(messages.reserve(1).unwrap_err(), Error::NoBuffer);
        assert_eq!(
            messages.adopt_received(source.clone(), 0..1).unwrap_err(),
            Error::NoBuffer
        );
        shared
    })
    .unwrap();
    assert_eq!(pool.retained_bytes(), charged);
    assert_eq!(pool.len(), 1);
    assert!(Arc::get_mut(&mut source).is_none());
    let retained = shared.clone();
    drop(shared);
    with_growing_messages(&mut pool, sends(0), |messages| {
        assert_eq!(&*retained, b"yload");
        assert_eq!(messages.reserve(1).unwrap_err(), Error::NoBuffer);
        drop(retained);
        assert!(Arc::get_mut(&mut source).is_some());
        let lease = messages
            .adopt_received(source.clone(), 7..14)
            .unwrap()
            .unwrap();
        messages.release(lease);
        assert!(Arc::get_mut(&mut source).is_some());
        let writer = messages.reserve_exact(7).unwrap();
        assert_eq!(&*messages.write(&writer), &[0; 7]);
        messages.discard(writer);
    })
    .unwrap();
    assert_eq!(pool.retained_bytes(), 14);
}

#[test]
#[cfg(target_has_atomic = "ptr")]
fn received_admission_rejects_capacity_and_ranges_without_consuming_slots() {
    use alloc::sync::Arc;
    let source = Arc::new(alloc::vec![0xa5; 32]);
    for (buffers, maximum, bytes, range, error) in [
        (0, 32, 32, 0..8, Error::NoBuffer),
        (1, 7, 32, 0..8, Error::NoBuffer),
        (1, 32, 31, 0..8, Error::NoBuffer),
        (1, 32, 32, 32..33, Error::InvalidLength),
        (
            1,
            32,
            32,
            core::ops::Range { start: 9, end: 8 },
            Error::InvalidLength,
        ),
    ] {
        let mut pool = GrowingBuffers::new(buffers, maximum, bytes);
        with_growing_messages(&mut pool, sends(0), |messages| {
            assert_eq!(
                messages.adopt_received(source.clone(), range).unwrap_err(),
                error
            );
        })
        .unwrap();
        assert_eq!(pool.len(), 0);
        assert_eq!(pool.retained_bytes(), 0);
        assert_eq!(Arc::strong_count(&source), 1);
    }
    let mut pool = GrowingBuffers::new(5, 32, 160);
    with_growing_messages(&mut pool, sends(0), |messages| {
        let leases: Vec<_> = (0..5)
            .map(|_| {
                messages
                    .adopt_received(source.clone(), 1..9)
                    .unwrap()
                    .unwrap()
            })
            .collect();
        assert_eq!(
            messages.adopt_received(source.clone(), 1..9).unwrap_err(),
            Error::NoBuffer
        );
        for lease in leases {
            messages.release(lease);
        }
        for index in 0..5 {
            messages.buffers[index].generation.set(u64::MAX);
        }
        assert_eq!(
            messages.adopt_received(source.clone(), 1..9).unwrap_err(),
            Error::GenerationExhausted
        );
    })
    .unwrap();
    assert_eq!(pool.retained_bytes(), 0);
    assert_eq!(Arc::strong_count(&source), 1);
    let mut bytes = [0x5a; 16];
    with_messages(&mut [Buffer::new(&mut bytes)], &mut [], |messages| {
        assert!(messages
            .adopt_received(source.clone(), 1..9)
            .unwrap()
            .is_none());
    })
    .unwrap();
    assert_eq!(bytes, [0x5a; 16]);
}

#[test]
fn direct_shared_copy_preserves_bounds_ownership_and_generation_exhaustion() {
    let mut pool = GrowingBuffers::new(1, 16, 23);
    let shared = with_growing_messages(&mut pool, sends(0), |messages| {
        let shared = messages.copy_shared(b"immutable").unwrap().unwrap();
        assert_eq!(&*shared, b"immutable");
        assert_eq!(
            messages.copy_shared(b"overwrite").err(),
            Some(Error::NoBuffer)
        );
        assert_eq!(messages.reserve(1).unwrap_err(), Error::NoBuffer);
        shared
    })
    .unwrap();
    with_growing_messages(&mut pool, sends(0), |messages| {
        assert_eq!(messages.copy_shared(b"x").err(), Some(Error::NoBuffer));
        let address = shared.as_ptr();
        drop(shared);
        let mut replacement = None;
        let usage = allocation_counter::measure(|| {
            replacement = messages.copy_shared(b"new").unwrap();
        });
        assert_eq!(usage.count_total, 0);
        let replacement = replacement.unwrap();
        assert_eq!(replacement.as_ptr(), address);
        assert_eq!(&*replacement, b"new");
        drop(replacement);
        let writer = messages.reserve(9).unwrap();
        assert!(messages.write(&writer).iter().all(|byte| *byte == 0));
        messages.discard(writer);
        messages.buffers[0].generation.set(u64::MAX);
        assert_eq!(
            messages.copy_shared(b"x").err(),
            Some(Error::GenerationExhausted)
        );
    })
    .unwrap();
    let mut bytes = [0xa5; 16];
    let mut buffers = [Buffer::new(&mut bytes)];
    with_messages(&mut buffers, &mut [], |messages| {
        let usage = allocation_counter::measure(|| {
            assert!(messages.copy_shared(b"x").unwrap().is_none());
        });
        assert_eq!(usage.count_total, 0);
        assert_eq!(messages.buffers[0].generation.get(), 0);
        assert_eq!(&**messages.buffers[0].bytes.borrow(), &[0xa5; 16]);
    })
    .unwrap();
}

#[test]
fn shared_bytes_pin_capacity_across_send_completion_and_scope_teardown() {
    let mut pool = GrowingBuffers::new(1, 16, 23);
    let shared = with_growing_messages(&mut pool, sends(1), |messages| {
        let writer = messages.reserve(8).unwrap();
        messages.write(&writer).copy_from_slice(b"retained");
        let lease = messages.seal(writer, 8).unwrap();
        let offer = messages.offer(&lease).unwrap();
        let key = messages.accept(offer);
        messages.release(lease);
        assert!(!messages.send_window(key).unwrap().advance(3).unwrap());
        let window = messages.send_window(key).unwrap();
        let pointer = window.bytes().as_ptr();
        let mut shared = None;
        let measured = allocation_counter::measure(|| {
            shared = window.share();
        });
        assert_eq!(measured.count_total, 0);
        let shared = shared.unwrap();
        assert_eq!(shared.as_ptr(), pointer);
        assert_eq!(&*shared, b"ained");
        assert!(window.advance(5).unwrap());
        assert_eq!(messages.reserve(8).unwrap_err(), Error::NoBuffer);
        assert_eq!(messages.reserve(16).unwrap_err(), Error::NoBuffer);
        shared
    })
    .unwrap();
    let retained = shared.clone();
    drop(shared);
    with_growing_messages(&mut pool, sends(0), |messages| {
        assert_eq!(&*retained, b"ained");
        assert_eq!(messages.reserve(8).unwrap_err(), Error::NoBuffer);
        drop(retained);
        let writer = messages.reserve(16).unwrap();
        assert_eq!(&*messages.write(&writer), &[0; 16]);
        messages.discard(writer);
    })
    .unwrap();
    assert_eq!(pool.len(), 1);
    assert_eq!(pool.retained_bytes(), 23);
}

#[test]
fn sharing_a_sealed_prefix_never_exposes_unused_capacity() {
    let mut pool = GrowingBuffers::new(1, 32, 39);
    let shared = with_growing_messages(&mut pool, sends(0), |messages| {
        let writer = messages.reserve(32).unwrap();
        messages.write(&writer).fill(0x5a);
        let lease = messages.seal(writer, 3).unwrap();
        let shared = messages.share(&lease).unwrap();
        messages.release(lease);
        shared
    })
    .unwrap();
    drop(pool);
    assert_eq!(&*shared, &[0x5a; 3]);
}

#[test]
fn borrowed_views_survive_segment_growth_and_independent_sends() {
    let measured = allocation_counter::measure(|| {
        let mut pool = GrowingBuffers::new(65, 8, 65 * 15);
        with_growing_messages(&mut pool, sends(65), |messages| {
            let writer = messages.reserve(8).unwrap();
            let mut writing = messages.write_owned(writer).unwrap();
            writing.bytes().copy_from_slice(b"original");
            let original = writing.bytes().as_ptr();
            let writer = messages.reserve(8).unwrap();
            messages.write(&writer).copy_from_slice(b"retained");
            let lease = messages.seal(writer, 8).unwrap();
            let view = messages.read(&lease);
            let mut accepted = Vec::new();
            for number in 2..65 {
                let writer = messages.reserve(8).unwrap();
                let mut bytes = messages.write(&writer);
                assert_eq!(bytes.as_ptr().align_offset(8), 0);
                bytes.fill(number);
                drop(bytes);
                let lease = messages.seal(writer, 8).unwrap();
                let offer = messages.offer(&lease).unwrap();
                accepted.push(messages.accept(offer));
                messages.release(lease);
            }
            assert_eq!(messages.reserve(1).unwrap_err(), Error::NoBuffer);
            assert_eq!(writing.bytes().as_ptr(), original);
            assert_eq!(writing.bytes(), b"original");
            assert_eq!(&*view, b"retained");
            for (number, key) in accepted.into_iter().enumerate().rev() {
                assert_eq!(
                    messages.send_window(key).unwrap().bytes(),
                    &[number as u8 + 2; 8]
                );
                assert_eq!(messages.send_window(key).unwrap().advance(3), Ok(false));
                assert_eq!(messages.send_window(key).unwrap().advance(5), Ok(true));
                assert_eq!(messages.fail_send(key), Err(Error::StaleSend));
            }
            drop(view);
            messages.release(lease);
            let lease = messages.seal(writing.into_writer(), 8).unwrap();
            assert_eq!(&*messages.read(&lease), b"original");
            messages.release(lease);
        })
        .unwrap();
        assert_eq!(pool.len(), 65);
        assert_eq!(pool.retained_bytes(), 65 * 15);
    });
    assert_eq!(
        measured.count_current, 0,
        "provider destruction frees all storage"
    );
}

#[test]
fn byte_and_slot_limits_are_independent_and_growth_reuses_free_records() {
    let mut pool = GrowingBuffers::new(2, 16, 30);
    with_growing_messages(&mut pool, sends(1), |messages| {
        let small = messages.reserve(1).unwrap();
        let index = small.key.index;
        let generation = small.key.generation;
        messages.discard(small);
        let large = messages.reserve(16).unwrap();
        assert_eq!(large.key.index, index);
        assert_eq!(large.key.generation, generation + 1);
        assert_eq!(&*messages.write(&large), &[0; 16]);
        assert_eq!(messages.reserve(1).unwrap_err(), Error::NoBuffer);
        // Empty messages still have aligned storage and consume padding budget.
        let empty = messages.reserve(0).unwrap();
        assert_eq!(messages.reserve(0).unwrap_err(), Error::NoBuffer);
        let lease = messages.seal(large, 16).unwrap();
        let offer = messages.offer(&lease).unwrap();
        assert_eq!(messages.offer(&lease).unwrap_err(), Error::NoSendSlot);
        messages.reject(offer);
        messages.release(lease);
        messages.discard(empty);
        assert_eq!(messages.reserve(usize::MAX).unwrap_err(), Error::NoBuffer);
        let recovered = messages.reserve(16).unwrap();
        assert_eq!(recovered.key.index, index);
        messages.discard(recovered);
    })
    .unwrap();
    assert_eq!(pool.len(), 2);
    assert_eq!(pool.retained_bytes(), 30);
    let measured = allocation_counter::measure(|| {
        with_growing_messages(&mut pool, sends(0), |messages| {
            let writer = messages.reserve(16).unwrap();
            messages.discard(writer);
        })
        .unwrap();
    });
    assert_eq!(
        measured.count_total, 0,
        "warm buffer reuse allocates nothing"
    );
    let mut empty = GrowingBuffers::new(0, usize::MAX, usize::MAX);
    with_growing_messages(&mut empty, sends(0), |messages| {
        assert_eq!(messages.reserve(0).unwrap_err(), Error::NoBuffer);
    })
    .unwrap();
    assert_eq!(
        with_growing_messages(
            &mut empty,
            Growth {
                initial: 2,
                maximum: 1
            },
            |_| ()
        ),
        Err(Error::NoSendSlot)
    );
}

#[test]
fn forgotten_views_require_terminal_scope_before_buffer_recovery() {
    let mut pool = GrowingBuffers::new(1, 25, 32);
    with_growing_messages(&mut pool, sends(0), |messages| {
        let writer = messages.reserve(8).unwrap();
        core::mem::forget(messages.write(&writer));
        assert_eq!(
            messages.try_write(&writer).unwrap_err(),
            Error::BorrowedBuffer
        );
        messages.discard(writer);
        assert_eq!(messages.reserve(16).unwrap_err(), Error::NoBuffer);
    })
    .unwrap();
    with_growing_messages(&mut pool, sends(0), |messages| {
        let writer = messages.reserve(16).unwrap();
        assert_eq!(&*messages.write(&writer), &[0; 16]);
        let lease = messages.seal(writer, 16).unwrap();
        core::mem::forget(messages.read(&lease));
        messages.release(lease);
        assert_eq!(messages.reserve(1).unwrap_err(), Error::NoBuffer);
    })
    .unwrap();
    with_growing_messages(&mut pool, sends(0), |messages| {
        let writer = messages.reserve(16).unwrap();
        messages.discard(writer);
    })
    .unwrap();
}

#[test]
fn cancellation_reclaims_suspended_writer_and_send_without_leaking() {
    let measured = allocation_counter::measure(|| {
        let mut pool = GrowingBuffers::new(2, 8, 30);
        {
            let future = with_growing_messages_async(&mut pool, sends(1), async |messages| {
                let writer = messages.reserve(8).unwrap();
                let mut writing = messages.write_owned(writer).unwrap();
                writing.bytes().fill(42);
                let writer = messages.reserve(8).unwrap();
                let lease = messages.seal(writer, 8).unwrap();
                let offer = messages.offer(&lease).unwrap();
                let key = messages.accept(offer);
                messages.release(lease);
                assert_eq!(messages.send_window(key).unwrap().advance(1), Ok(false));
                core::future::pending::<()>().await;
                messages.discard(writing.into_writer());
            });
            let mut future = core::pin::pin!(future);
            assert_eq!(
                future
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop())),
                Poll::Pending
            );
        }
        with_growing_messages(&mut pool, sends(0), |messages| {
            let first = messages.reserve(8).unwrap();
            let second = messages.reserve(8).unwrap();
            assert_eq!(&*messages.write(&first), &[0; 8]);
            assert_eq!(&*messages.write(&second), &[0; 8]);
            messages.discard(first);
            messages.discard(second);
        })
        .unwrap();
    });
    assert_eq!(measured.count_current, 0);
}

#[test]
fn growing_fifo_preserves_wrapped_head_and_partial_frame() {
    let mut pool = GrowingBuffers::new(4, 8, 60);
    with_growing_messages(&mut pool, sends(4), |messages| {
        let mut queue = Queue::new_growing(0, 4).unwrap();
        // Grow to two slots, consume one, then wrap the tail into its old slot.
        for byte in 1..=2 {
            let writer = messages.reserve(8).unwrap();
            messages.write(&writer).fill(byte);
            let lease = messages.seal(writer, 8).unwrap();
            let offer = messages.offer(&lease).unwrap();
            queue.reserve().unwrap().accept(messages, offer);
            messages.release(lease);
        }
        assert_eq!(
            queue.window(messages).unwrap().unwrap().advance(8),
            Ok(true)
        );
        assert_eq!(
            queue.window(messages).unwrap().unwrap().advance(3),
            Ok(false)
        );
        for byte in 3..=5 {
            let slot = queue.reserve().unwrap();
            let writer = messages.reserve(8).unwrap();
            messages.write(&writer).fill(byte);
            let lease = messages.seal(writer, 8).unwrap();
            let offer = messages.offer(&lease).unwrap();
            slot.accept(messages, offer);
            messages.release(lease);
        }
        assert_eq!(queue.reserve().err(), Some(Error::NoSendSlot));
        for byte in 2..=5 {
            let window = queue.window(messages).unwrap().unwrap();
            let length = if byte == 2 { 5 } else { 8 };
            assert_eq!(
                window.bytes(),
                &[byte; 8][..length],
                "growth preserves frame order and suffix"
            );
            assert_eq!(window.advance(length), Ok(true));
        }
        assert!(queue.is_empty());
    })
    .unwrap();
}
