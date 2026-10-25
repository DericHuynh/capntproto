extern crate std;

use super::*;
use core::cell::Cell;

#[test]
fn inline_and_borrowed_storage_preserve_fifo_and_reuse_without_allocating() {
    fn exercise<const N: usize>() {
        let storage = Storage::<N>::new();
        let wakes = Cell::new(0);
        let mut position = 0usize;
        for round in 0..64 {
            // A new borrowed view must not reset the queue or consumer cursor.
            let queue = storage.queue();
            for index in (0..N).rev() {
                queue.enqueue(index, || wakes.set(wakes.get() + 1));
                queue.enqueue(index, || panic!("duplicate must coalesce"));
            }
            assert_eq!(wakes.get(), (round + 1) * N);
            for expected in (0..N).rev() {
                assert_eq!(queue.front(position), Some(expected));
                queue.consume(position, expected);
                position = position.wrapping_add(1);
            }
            assert_eq!(queue.front(position), None);
        }
    }
    let measured = allocation_counter::measure(|| {
        exercise::<1>();
        exercise::<8>();
        exercise::<128>();
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn callback_can_consume_and_republish_without_losing_the_new_notification() {
    let storage = Storage::<4>::new();
    let queue = storage.queue();
    let notified = Cell::new(false);
    queue.enqueue(2, || {
        assert_eq!(queue.front(0), Some(2));
        queue.consume(0, 2);
        queue.enqueue(2, || notified.set(true));
    });
    assert!(notified.get());
    assert_eq!(queue.front(1), Some(2));
    queue.consume(1, 2);
    assert_eq!(queue.front(2), None);
}

#[test]
fn invalid_capacity_and_slot_are_rejected_before_publication() {
    use std::panic::catch_unwind;
    assert!(catch_unwind(Storage::<0>::new).is_err());
    assert!(catch_unwind(Storage::<3>::new).is_err());
    assert!(catch_unwind(Storage::<256>::new).is_err());
    let storage = Storage::<8>::new();
    let queue = storage.queue();
    assert!(catch_unwind(|| queue.enqueue(8, || panic!("not published"))).is_err());
    assert_eq!(storage.next.load(Ordering::Relaxed), 0);
    assert_eq!(queue.front(0), None);
    assert!(storage.queued.iter().all(|q| !q.load(Ordering::Relaxed)));
}

#[test]
fn full_ring_waits_for_an_unpublished_front_across_cursor_wrap() {
    let storage = Storage::<8>::new();
    let queue = storage.queue();
    let start = usize::MAX - 1;
    storage.next.store(start, Ordering::Relaxed);
    // Pause slot 0 after reservation. Fill all remaining positions, including
    // positions after the integer wrap, without passing or overwriting it.
    assert!(!storage.queued[0].swap(true, Ordering::AcqRel));
    assert_eq!(storage.next.fetch_add(1, Ordering::Relaxed), start);
    for index in 1..8 {
        queue.enqueue(index, || {});
    }
    for index in 0..8 {
        queue.enqueue(index, || panic!("duplicate must not reserve again"));
    }
    assert_eq!(queue.front(start), None);
    storage.entries[start % 8].store(1, Ordering::Release);
    for index in 0..8 {
        let position = start.wrapping_add(index);
        assert_eq!(queue.front(position), Some(index));
        queue.consume(position, index);
    }
    let position = start.wrapping_add(8);
    assert_eq!(queue.front(position), None);
    queue.enqueue(7, || {});
    assert_eq!(queue.front(position), Some(7));
    queue.consume(position, 7);
}

#[test]
fn concurrent_publication_and_consumption_reuse_every_slot() {
    const N: usize = 8;
    let storage = Storage::<N>::new();
    let queue = storage.queue();
    let values = [const { AtomicUsize::new(0) }; N];
    let finished = AtomicUsize::new(0);
    let mut observed = [0; N];
    let rounds = if cfg!(miri) { 2 } else { 256 };
    std::thread::scope(|scope| {
        for (index, value_slot) in values.iter().enumerate() {
            let finished = &finished;
            scope.spawn(move || {
                for value in 1..=rounds {
                    value_slot.store(value, Ordering::Release);
                    queue.enqueue(index, || {});
                }
                finished.fetch_add(1, Ordering::Release);
            });
        }
        let mut position = 0usize;
        loop {
            if let Some(index) = queue.front(position) {
                queue.consume(position, index);
                position = position.wrapping_add(1);
                observed[index] = values[index].load(Ordering::Acquire);
            } else if finished.load(Ordering::Acquire) == N {
                if queue.front(position).is_none() {
                    break;
                }
            } else {
                std::thread::yield_now();
            }
        }
    });
    assert_eq!(observed, [rounds; N]);
    assert!(storage.queued.iter().all(|q| !q.load(Ordering::Acquire)));
}

#[test]
fn a_duplicate_wake_publishes_data_or_leaves_another_notification() {
    use std::sync::Barrier;
    let storage = Storage::<1>::new();
    let queue = storage.queue();
    let data = AtomicBool::new(false);
    let start = Barrier::new(2);
    let done = Barrier::new(2);
    let rounds = if cfg!(miri) { 8 } else { 10_000 };
    let mut lost = None;
    std::thread::scope(|scope| {
        scope.spawn(|| {
            for _ in 0..rounds {
                start.wait();
                data.store(true, Ordering::Release);
                queue.enqueue(0, || {});
                done.wait();
            }
        });
        let mut position = 0usize;
        for round in 0..rounds {
            if let Some(index) = queue.front(position) {
                queue.consume(position, index);
                position = position.wrapping_add(1);
            }
            data.store(false, Ordering::Relaxed);
            queue.enqueue(0, || {});
            start.wait();
            assert_eq!(queue.front(position), Some(0));
            queue.consume(position, 0);
            position = position.wrapping_add(1);
            let observed = data.load(Ordering::Acquire);
            done.wait();
            if !observed && queue.front(position).is_none() {
                lost.get_or_insert(round);
            }
        }
    });
    assert_eq!(lost, None, "duplicate wake lost its data publication");
}
