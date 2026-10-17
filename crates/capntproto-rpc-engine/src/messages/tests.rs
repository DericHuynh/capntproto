extern crate std;

use super::*;

#[test]
fn capacity_lookahead_preserves_fixed_generations_and_live_owners() {
    let mut bytes = [0; 16];
    let mut buffers = [Buffer::new(&mut bytes)];
    let usage = allocation_counter::measure(|| {
        with_messages(&mut buffers, &mut [SendSlot::EMPTY], |messages| {
            assert!(messages.can_reserve(16));
            assert!(!messages.can_reserve(17));
            assert!(!messages.can_reserve(usize::MAX));
            assert_eq!(messages.buffers[0].generation.get(), 0);
            let writer = messages.reserve(16).unwrap();
            assert!(!messages.can_reserve(0));
            let lease = messages.seal(writer, 8).unwrap();
            let offer = messages.offer(&lease).unwrap();
            let send = messages.accept(offer);
            messages.release(lease);
            assert!(!messages.can_reserve(0));
            messages.fail_send(send).unwrap();
            assert!(messages.can_reserve(16));
            let lease = messages.copy_from(b"borrowed").unwrap();
            core::mem::forget(messages.read(&lease));
            messages.release(lease);
            assert!(!messages.can_reserve(0));
        })
        .unwrap();
    });
    assert_eq!(usage.count_total, 0);
    with_messages(&mut buffers, &mut [], |messages| {
        messages.buffers[0].generation.set(u64::MAX);
        assert!(!messages.can_reserve(0));
    })
    .unwrap();
}

#[test]
fn owned_send_transfer_preserves_aliases_and_partial_write_ownership_without_allocating() {
    for retain in [false, true] {
        let mut bytes = [0; 16];
        let mut buffers = [Buffer::new(&mut bytes)];
        let usage = allocation_counter::measure(|| {
            with_messages(&mut buffers, &mut [SendSlot::EMPTY], |messages| {
                let lease = messages.copy_from(b"payload").unwrap();
                let alias = retain.then(|| messages.retain(&lease).unwrap());
                let offer = messages.offer_owned(lease).unwrap();
                assert_eq!(messages.reserve(1).unwrap_err(), Error::NoBuffer);
                let key = messages.accept(offer);
                let window = messages.send_window(key).unwrap();
                assert_eq!(window.bytes(), b"payload");
                assert!(!window.advance(3).unwrap());
                let window = messages.send_window(key).unwrap();
                assert_eq!(window.bytes(), b"load");
                assert_eq!(window.advance(5), Err(Error::InvalidWriteCount));
                assert_eq!(messages.send_window(key).unwrap().bytes(), b"load");
                messages.fail_send(key).unwrap();
                assert_eq!(messages.fail_send(key), Err(Error::StaleSend));
                if let Some(alias) = alias {
                    assert_eq!(&*messages.read(&alias), b"payload");
                    assert_eq!(messages.reserve(1).unwrap_err(), Error::NoBuffer);
                    messages.release(alias);
                }
                let writer = messages.reserve(16).unwrap();
                messages.discard(writer);
            })
            .unwrap();
        });
        assert_eq!(usage.count_total, 0);
    }
}

#[test]
fn rejected_owned_send_returns_the_exact_lease_without_consuming_a_generation() {
    let mut bytes = [0; 16];
    let mut buffers = [Buffer::new(&mut bytes)];
    with_messages(&mut buffers, &mut [SendSlot::EMPTY], |messages| {
        let lease = messages.copy_from(b"original").unwrap();
        let first = messages.offer(&lease).unwrap();
        let original = lease.key;
        let state = messages.buffers[original.index].state.get();
        let generation = messages.sends[0].generation;
        let rejected = messages.offer_owned(lease).unwrap_err();
        assert_eq!(rejected.error, Error::NoSendSlot);
        assert_eq!(rejected.lease.key, original);
        assert_eq!(messages.buffers[original.index].state.get(), state);
        assert_eq!(messages.sends[0].generation, generation);
        messages.reject(first);
        messages.sends[0].generation = u64::MAX;
        let rejected = messages.offer_owned(rejected.lease).unwrap_err();
        assert_eq!(rejected.error, Error::GenerationExhausted);
        assert_eq!(rejected.lease.key, original);
        assert_eq!(&*messages.read(&rejected.lease), b"original");
        messages.release(rejected.lease);
        let writer = messages.reserve(16).unwrap();
        messages.discard(writer);
    })
    .unwrap();
}

#[test]
fn atomic_copy_parts_edits_only_initialized_bytes_and_recovers_after_unwind() {
    let mut bytes = [0xa5; 16];
    let mut buffers = [Buffer::new(&mut bytes)];
    with_messages(&mut buffers, &mut [], |messages| {
        let generation = messages.buffers[0].generation.get();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = messages.copy_from_parts(&[b"head", b"", b"body"], |copied| {
                assert_eq!(copied, b"headbody");
                copied[0] = b'H';
                panic!("failed application initialization");
            });
        }));
        assert!(outcome.is_err());
        assert_eq!(messages.buffers[0].generation.get(), generation);
        let initialized = Cell::new(false);
        let usage = allocation_counter::measure(|| {
            let lease = messages
                .copy_from_parts(&[b"ab", b"cde"], |copied| {
                    initialized.set(true);
                    assert_eq!(copied, b"abcde");
                    copied[1..4].copy_from_slice(b"XYZ");
                })
                .unwrap();
            assert!(initialized.get());
            assert_eq!(&*messages.read(&lease), b"aXYZe");
            assert_eq!(
                messages
                    .copy_from_parts(&[b"x"], |_| panic!("live owner must reject first"))
                    .err(),
                Some(Error::NoBuffer)
            );
            messages.release(lease);
            let writer = messages.reserve(1).unwrap();
            assert_eq!(&*messages.write(&writer), &[0; 16]);
            messages.discard(writer);
        });
        assert_eq!(usage.count_total, 0);
    })
    .unwrap();
}

#[test]
fn atomic_copy_preserves_views_sends_and_unexposed_capacity_without_allocating() {
    let measured = allocation_counter::measure(|| {
        let mut bytes = [0xa5; 16];
        let mut buffers = [Buffer::new(&mut bytes)];
        let mut sends = [SendSlot::EMPTY];
        with_messages(&mut buffers, &mut sends, |messages| {
            let first = messages.copy_from(b"original").unwrap();
            assert_eq!(&*messages.read(&first), b"original");
            assert_eq!(messages.copy_from(b"bad").err(), Some(Error::NoBuffer));
            let offer = messages.offer(&first).unwrap();
            let send = messages.accept(offer);
            messages.release(first);
            assert_eq!(messages.copy_from(b"bad").err(), Some(Error::NoBuffer));
            assert_eq!(messages.send_window(send).unwrap().bytes(), b"original");
            assert_eq!(messages.send_window(send).unwrap().advance(8), Ok(true));
            let second = messages.copy_from(b"new").unwrap();
            assert_eq!(&*messages.read(&second), b"new");
            core::mem::forget(messages.read(&second));
            messages.release(second);
            assert_eq!(messages.copy_from(b"bad").err(), Some(Error::NoBuffer));
        })
        .unwrap();
        assert_eq!(&bytes[..8], b"newginal");
        assert_eq!(&bytes[8..], &[0xa5; 8]);
        let mut buffers = [Buffer::new(&mut bytes)];
        with_messages(&mut buffers, &mut sends, |messages| {
            let empty = messages.copy_from(b"").unwrap();
            assert!(messages.read(&empty).is_empty());
            messages.release(empty);
            let writer = messages.reserve(1).unwrap();
            assert_eq!(&*messages.write(&writer), &[0; 16]);
            messages.discard(writer);
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn atomic_copy_never_reuses_an_exhausted_generation() {
    let mut bytes = [0; 8];
    let mut buffers = [Buffer::new(&mut bytes)];
    with_messages(&mut buffers, &mut [], |messages| {
        messages.buffers[0].generation.set(u64::MAX - 1);
        let last = messages.copy_from(b"last").unwrap();
        assert_eq!(last.key.generation, u64::MAX);
        messages.release(last);
        assert_eq!(
            messages.copy_from(b"next").err(),
            Some(Error::GenerationExhausted)
        );
    })
    .unwrap();
}

#[test]
fn exact_reservation_never_exposes_or_seals_uncleared_capacity() {
    let measured = allocation_counter::measure(|| {
        let mut bytes = [0xa5; 64];
        let mut buffers = [Buffer::new(&mut bytes)];
        with_messages(&mut buffers, &mut [], |messages| {
            let mut writer = messages.reserve_exact(3).unwrap();
            assert_eq!(&*messages.write(&writer), &[0; 3]);
            messages
                .write_borrowed(&mut writer)
                .unwrap()
                .copy_from_slice(b"abc");
            let rejected = messages.seal(writer, 4).unwrap_err();
            assert_eq!(rejected.error, Error::InvalidLength);
            let mut writing = messages.write_owned(rejected.writer).unwrap();
            assert_eq!(writing.bytes(), b"abc");
            let lease = messages.seal(writing.into_writer(), 3).unwrap();
            assert_eq!(&*messages.read(&lease), b"abc");
            messages.release(lease);
        })
        .unwrap();
        assert_eq!(&bytes[3..], &[0xa5; 61], "unused capacity was not cleared");
        let mut buffers = [Buffer::new(&mut bytes)];
        with_messages(&mut buffers, &mut [], |messages| {
            let writer = messages.reserve(1).unwrap();
            assert_eq!(
                &*messages.write(&writer),
                &[0; 64],
                "ordinary reservation clears all exposed bytes"
            );
            messages.discard(writer);
            let empty = messages.reserve_exact(0).unwrap();
            assert!(messages.write(&empty).is_empty());
            let lease = messages.seal(empty, 0).unwrap();
            messages.release(lease);
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn owned_writer_survives_movement_and_forgetting_requires_terminal_teardown() {
    let measured = allocation_counter::measure(|| {
        let mut a = [0; 4];
        let mut b = [0; 4];
        let mut buffers = [Buffer::new(&mut a), Buffer::new(&mut b)];
        with_messages(&mut buffers, &mut [], |messages| {
            let writer = messages.reserve(4).unwrap();
            let mut writing = messages.write_owned(writer).unwrap();
            writing.bytes().copy_from_slice(b"kept");
            let other = messages.reserve(4).unwrap();
            messages.write(&other).copy_from_slice(b"side");
            messages.discard(other);
            let moved = writing;
            let lease = messages.seal(moved.into_writer(), 4).unwrap();
            assert_eq!(&*messages.read(&lease), b"kept");
            messages.release(lease);
            let writer = messages.reserve(4).unwrap();
            core::mem::forget(messages.write_owned(writer).unwrap());
            let other = messages.reserve(4).unwrap();
            assert_eq!(messages.reserve(1).err(), Some(Error::NoBuffer));
            messages.discard(other);
        })
        .unwrap();
        with_messages(&mut buffers, &mut [], |messages| {
            let first = messages.reserve(4).unwrap();
            let second = messages.reserve(4).unwrap();
            assert_eq!(&*messages.write(&first), &[0; 4]);
            messages.discard(first);
            messages.discard(second);
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn a_request_view_does_not_block_nested_calls_or_queued_output() {
    let measured = allocation_counter::measure(|| {
        let mut request = [0; 4];
        let mut outgoing = [0; 4];
        let mut buffers = [Buffer::new(&mut request), Buffer::new(&mut outgoing)];
        let mut sends = [const { SendSlot::EMPTY }; 2];
        with_messages(&mut buffers, &mut sends, |messages| {
            let writer = messages.reserve(4).unwrap();
            messages.write(&writer).copy_from_slice(b"args");
            let lease = messages.seal(writer, 4).unwrap();
            let view = messages.read(&lease);
            let clone = Ref::clone(&view);
            let retained = messages.retain(&lease).unwrap();
            let offer = messages.offer(&lease).unwrap();
            let first = messages.accept(offer);
            messages.release(retained);
            let writer = messages.reserve(4).unwrap();
            messages.write(&writer).copy_from_slice(b"call");
            let call = messages.seal(writer, 4).unwrap();
            let offer = messages.offer(&call).unwrap();
            let second = messages.accept(offer);
            messages.release(call);
            assert_eq!(messages.send_window(first).unwrap().advance(2), Ok(false));
            assert_eq!(messages.send_window(second).unwrap().advance(4), Ok(true));
            let writer = messages.reserve(4).unwrap();
            messages.write(&writer).fill(0xff);
            messages.discard(writer);
            assert_eq!(&*view, b"args");
            drop(view);
            assert_eq!(&*clone, b"args");
            drop(clone);
            messages.release(lease);
            assert_eq!(messages.send_window(first).unwrap().bytes(), b"gs");
            assert_eq!(messages.send_window(first).unwrap().advance(2), Ok(true));
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn forgotten_views_reserve_bytes_until_terminal_scope_recovery() {
    let mut bytes = [0; 4];
    let mut buffers = [Buffer::new(&mut bytes)];
    let mut sends = [SendSlot::EMPTY];
    with_messages(&mut buffers, &mut sends, |messages| {
        let writer = messages.reserve(4).unwrap();
        core::mem::forget(messages.write(&writer));
        assert_eq!(
            messages.try_write(&writer).unwrap_err(),
            Error::BorrowedBuffer
        );
        let rejected = messages.seal(writer, 4).unwrap_err();
        assert_eq!(rejected.error, Error::BorrowedBuffer);
        messages.discard(rejected.writer);
        assert_eq!(messages.reserve(4).unwrap_err(), Error::NoBuffer);
    })
    .unwrap();
    with_messages(&mut buffers, &mut sends, |messages| {
        let writer = messages.reserve(4).unwrap();
        let lease = messages.seal(writer, 4).unwrap();
        core::mem::forget(messages.read(&lease));
        messages.release(lease);
        assert_eq!(messages.reserve(4).unwrap_err(), Error::NoBuffer);
    })
    .unwrap();
    with_messages(&mut buffers, &mut sends, |messages| {
        let writer = messages.reserve(4).unwrap();
        assert_eq!(&*messages.write(&writer), &[0; 4]);
        messages.discard(writer);
    })
    .unwrap();
}

#[test]
fn readers_and_queued_sends_independently_retain_immutable_bytes() {
    let mut bytes = [99; 8];
    let mut buffers = [Buffer::new(&mut bytes)];
    let mut sends = [SendSlot::EMPTY];
    with_messages(&mut buffers, &mut sends, |messages| {
        let writer = messages.reserve(8).unwrap();
        assert_eq!(&*messages.write(&writer), &[0; 8]);
        messages.write(&writer)[..3].copy_from_slice(b"rpc");
        let reader = messages.seal(writer, 3).unwrap();
        let retained = messages.retain(&reader).unwrap();
        let offer = messages.offer(&reader).unwrap();
        messages.release(reader);
        let key = messages.accept(offer);
        assert_eq!(messages.reserve(1).unwrap_err(), Error::NoBuffer);
        assert_eq!(messages.send_window(key).unwrap().advance(1), Ok(false));
        assert_eq!(messages.send_window(key).unwrap().bytes(), b"pc");
        assert_eq!(messages.send_window(key).unwrap().advance(2), Ok(true));
        assert_eq!(messages.fail_send(key), Err(Error::StaleSend));
        assert_eq!(&*messages.read(&retained), b"rpc");
        assert_eq!(messages.reserve(1).unwrap_err(), Error::NoBuffer);
        messages.release(retained);
        let writer = messages.reserve(8).unwrap();
        assert_eq!(&*messages.write(&writer), &[0; 8]);
        messages.discard(writer);
    })
    .unwrap();
}

#[test]
fn rejection_and_capacity_failure_preserve_existing_owners() {
    let mut bytes = [1; 4];
    let mut buffers = [Buffer::new(&mut bytes)];
    let mut sends = [SendSlot::EMPTY];
    with_messages(&mut buffers, &mut sends, |messages| {
        assert_eq!(messages.reserve(5).unwrap_err(), Error::NoBuffer);
        let writer = messages.reserve(4).unwrap();
        messages.write(&writer).fill(7);
        let rejected = messages.seal(writer, usize::MAX).unwrap_err();
        assert_eq!(rejected.error, Error::InvalidLength);
        assert_eq!(&*messages.write(&rejected.writer), &[7; 4]);
        let lease = messages.seal(rejected.writer, 4).unwrap();
        let offer = messages.offer(&lease).unwrap();
        assert_eq!(messages.offer(&lease).unwrap_err(), Error::NoSendSlot);
        messages.reject(offer);
        assert_eq!(&*messages.read(&lease), &[7; 4]);
        let offer = messages.offer(&lease).unwrap();
        messages.release(lease);
        assert_eq!(messages.reserve(0).unwrap_err(), Error::NoBuffer);
        messages.reject(offer);
        let writer = messages.reserve(4).unwrap();
        messages.discard(writer);
    })
    .unwrap();
}

#[test]
fn duplicate_and_late_completions_cannot_release_a_reused_send_slot() {
    let mut bytes = [0; 2];
    let mut buffers = [Buffer::new(&mut bytes)];
    let mut sends = [const { SendSlot::EMPTY }; 2];
    with_messages(&mut buffers, &mut sends, |messages| {
        let writer = messages.reserve(2).unwrap();
        messages.write(&writer).copy_from_slice(b"ab");
        let lease = messages.seal(writer, 2).unwrap();
        let first = messages.offer(&lease).unwrap();
        let second = messages.offer(&lease).unwrap();
        let first = messages.accept(first);
        let second = messages.accept(second);
        assert_eq!(messages.send_window(second).unwrap().advance(2), Ok(true));
        assert_eq!(messages.send_window(first).unwrap().advance(1), Ok(false));
        let third = messages.offer(&lease).unwrap();
        let third = messages.accept(third);
        assert_eq!(third.index, second.index);
        assert_ne!(third.generation, second.generation);
        messages.release(lease);
        assert_eq!(messages.fail_send(second), Err(Error::StaleSend));
        assert!(matches!(
            messages.send_window(second),
            Err(Error::StaleSend)
        ));
        messages.fail_send(first).unwrap();
        assert_eq!(messages.reserve(0).unwrap_err(), Error::NoBuffer);
        assert_eq!(messages.send_window(third).unwrap().bytes(), b"ab");
        assert_eq!(messages.send_window(third).unwrap().advance(2), Ok(true));
        let writer = messages.reserve(2).unwrap();
        messages.write(&writer).copy_from_slice(b"cd");
        let lease = messages.seal(writer, 2).unwrap();
        assert_eq!(messages.fail_send(third), Err(Error::StaleSend));
        assert_eq!(&*messages.read(&lease), b"cd");
        messages.release(lease);
    })
    .unwrap();
}

#[test]
fn partial_write_faults_at_every_boundary_preserve_progress_until_completion() {
    for fail_at in 0..=16 {
        let mut bytes = [0; 16];
        let mut buffers = [Buffer::new(&mut bytes)];
        let mut sends = [SendSlot::EMPTY];
        with_messages(&mut buffers, &mut sends, |messages| {
            let writer = messages.reserve(16).unwrap();
            for (i, byte) in messages.write(&writer).iter_mut().enumerate() {
                *byte = i as u8;
            }
            let lease = messages.seal(writer, 16).unwrap();
            let offer = messages.offer(&lease).unwrap();
            let key = messages.accept(offer);
            messages.release(lease);
            for position in 0..fail_at {
                {
                    let pending = messages.send_window(key).unwrap();
                    assert_eq!(pending.bytes()[0], position as u8);
                }
                assert_eq!(
                    messages.send_window(key).unwrap().advance(usize::MAX),
                    Err(Error::InvalidWriteCount)
                );
                assert_eq!(
                    messages.send_window(key).unwrap().advance(0),
                    Err(Error::WriteZero)
                );
                assert_eq!(
                    messages.send_window(key).unwrap().bytes()[0],
                    position as u8
                );
                assert_eq!(
                    messages.send_window(key).unwrap().advance(1),
                    Ok(position == 15)
                );
            }
            if fail_at < 16 {
                assert_eq!(messages.reserve(16).unwrap_err(), Error::NoBuffer);
                messages.fail_send(key).unwrap();
            }
            assert_eq!(messages.fail_send(key), Err(Error::StaleSend));
            let writer = messages.reserve(16).unwrap();
            assert_eq!(&*messages.write(&writer), &[0; 16]);
            messages.discard(writer);
        })
        .unwrap();
    }
}

#[test]
fn empty_messages_and_zero_capacities_are_explicit() {
    with_messages(&mut [], &mut [], |messages| {
        assert_eq!(messages.reserve(0).unwrap_err(), Error::NoBuffer);
    })
    .unwrap();
    let mut bytes = [];
    let mut buffers = [Buffer::new(&mut bytes)];
    with_messages(&mut buffers, &mut [], |messages| {
        let writer = messages.reserve(0).unwrap();
        let lease = messages.seal(writer, 0).unwrap();
        assert_eq!(messages.offer(&lease).unwrap_err(), Error::NoSendSlot);
        messages.release(lease);
    })
    .unwrap();
    with_messages(&mut buffers, &mut [SendSlot::EMPTY], |messages| {
        let writer = messages.reserve(0).unwrap();
        let lease = messages.seal(writer, 0).unwrap();
        let offer = messages.offer(&lease).unwrap();
        messages.release(lease);
        let key = messages.accept(offer);
        assert!(messages.send_window(key).unwrap().bytes().is_empty());
        assert_eq!(messages.send_window(key).unwrap().advance(0), Ok(true));
    })
    .unwrap();
}

#[test]
fn generation_and_reference_exhaustion_are_atomic() {
    let mut bytes = [0; 1];
    let mut buffers = [Buffer::new(&mut bytes)];
    buffers[0].generation.set(u64::MAX - 1);
    let mut sends = [SendSlot {
        generation: u64::MAX - 1,
        send: None,
    }];
    with_messages(&mut buffers, &mut sends, |messages| {
        let writer = messages.reserve(1).unwrap();
        let lease = messages.seal(writer, 1).unwrap();
        messages.buffers[0].state.set(State::Sealed {
            length: 1,
            readers: u32::MAX,
            sends: 0,
        });
        assert_eq!(
            messages.retain(&lease).unwrap_err(),
            Error::ReferencesExhausted
        );
        assert!(matches!(
            messages.buffers[0].state.get(),
            State::Sealed {
                readers: u32::MAX,
                ..
            }
        ));
        messages.buffers[0].state.set(State::Sealed {
            length: 1,
            readers: 1,
            sends: u32::MAX,
        });
        assert_eq!(
            messages.offer(&lease).unwrap_err(),
            Error::ReferencesExhausted
        );
        assert_eq!(messages.sends[0].generation, u64::MAX - 1);
        messages.buffers[0].state.set(State::Sealed {
            length: 1,
            readers: 1,
            sends: 0,
        });
        let offer = messages.offer(&lease).unwrap();
        messages.reject(offer);
        assert_eq!(
            messages.offer(&lease).unwrap_err(),
            Error::GenerationExhausted
        );
        messages.release(lease);
        assert_eq!(messages.reserve(1).unwrap_err(), Error::GenerationExhausted);
    })
    .unwrap();
    assert_eq!(buffers[0].generation.get(), u64::MAX);
    assert_eq!(sends[0].generation, u64::MAX);
}

#[test]
fn failed_construction_and_terminal_unwind_preserve_storage_rules() {
    extern crate std;
    let mut bytes = [0; 1];
    let mut buffers = [Buffer::new(&mut bytes)];
    let mut sends = [SendSlot::EMPTY];
    buffers[0].state.set(State::Writing);
    assert_eq!(
        with_messages(&mut buffers, &mut sends, |_| ()),
        Err(Error::NonemptyStorage)
    );
    assert_eq!(buffers[0].state.get(), State::Writing);
    buffers[0].state.set(State::Free);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = with_messages(&mut buffers, &mut sends, |messages| {
            let writer = messages.reserve(1).unwrap();
            let lease = messages.seal(writer, 1).unwrap();
            let _forgotten = messages.offer(&lease).unwrap();
            messages.release(lease);
            assert_eq!(messages.reserve(1).unwrap_err(), Error::NoBuffer);
            panic!("driver stopped IO before teardown");
        });
    }));
    assert!(result.is_err());
    assert_eq!(buffers[0].state.get(), State::Free);
    assert!(sends[0].send.is_none());
    with_messages(&mut buffers, &mut sends, |messages| {
        let writer = messages.reserve(1).unwrap();
        assert_eq!(writer.key.generation, 2);
        messages.discard(writer);
    })
    .unwrap();
}

#[test]
fn buffer_selection_and_send_lifecycle_do_not_allocate() {
    let counts = allocation_counter::measure(|| {
        let mut small = [0; 2];
        let mut large = [0; 8];
        let mut buffers = [Buffer::new(&mut small), Buffer::new(&mut large)];
        let mut sends = [const { SendSlot::EMPTY }; 2];
        with_messages(&mut buffers, &mut sends, |messages| {
            for value in 0..=255u8 {
                let writer = messages.reserve(8).unwrap();
                assert_eq!(writer.key.index, 1);
                messages.write(&writer).fill(value);
                let lease = messages.seal(writer, 8).unwrap();
                let offer = messages.offer(&lease).unwrap();
                let key = messages.accept(offer);
                messages.release(lease);
                let other = messages.reserve(2).unwrap();
                assert_eq!(other.key.index, 0);
                messages.discard(other);
                assert_eq!(messages.send_window(key).unwrap().bytes(), &[value; 8]);
                assert_eq!(messages.send_window(key).unwrap().advance(8), Ok(true));
            }
        })
        .unwrap();
    });
    assert_eq!(counts.count_total, 0);
}

#[test]
fn mutable_view_pins_one_writer_without_blocking_other_buffers() {
    let measured = allocation_counter::measure(|| {
        let mut a = [0u8; 4];
        let mut b = [0u8; 4];
        let mut buffers = [Buffer::new(&mut a), Buffer::new(&mut b)];
        let mut sends = [SendSlot::EMPTY];
        with_messages(&mut buffers, &mut sends, |messages| {
            let mut writer = messages.reserve(4).unwrap();
            let mut view = messages.write_borrowed(&mut writer).unwrap();
            view.copy_from_slice(b"held");
            for _ in 0..8 {
                let other = messages.reserve(4).unwrap();
                messages.write(&other).copy_from_slice(b"next");
                let lease = messages.seal(other, 4).unwrap();
                let offer = messages.offer(&lease).unwrap();
                let send = messages.accept(offer);
                messages.release(lease);
                assert!(messages.send_window(send).unwrap().advance(4).unwrap());
                assert_eq!(&*view, b"held");
            }
            drop(view);
            let lease = messages.seal(writer, 4).unwrap();
            assert_eq!(&*messages.read(&lease), b"held");
            messages.release(lease);
            let mut writer = messages.reserve(4).unwrap();
            core::mem::forget(messages.write_borrowed(&mut writer).unwrap());
            assert!(matches!(
                messages.write_borrowed(&mut writer),
                Err(Error::BorrowedBuffer)
            ));
            let failed = messages.seal(writer, 4).unwrap_err();
            assert_eq!(failed.error, Error::BorrowedBuffer);
            messages.discard(failed.writer);
            let other = messages.reserve(4).unwrap();
            assert!(matches!(messages.reserve(4), Err(Error::NoBuffer)));
            messages.discard(other);
        })
        .unwrap();
        with_messages(&mut buffers, &mut sends, |messages| {
            let a = messages.reserve(4).unwrap();
            let b = messages.reserve(4).unwrap();
            assert_eq!(&*messages.write(&a), &[0; 4]);
            messages.discard(a);
            messages.discard(b);
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}
