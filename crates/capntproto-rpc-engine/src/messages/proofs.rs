use super::*;

#[kani::proof]
#[kani::unwind(4)]
fn owned_writer_preserves_bytes_until_explicit_publication() {
    let first: u8 = kani::any();
    let second: u8 = kani::any();
    let mut a = [0u8];
    let mut b = [0u8];
    let mut buffers = [Buffer::new(&mut a), Buffer::new(&mut b)];
    with_messages(&mut buffers, &mut [], |messages| {
        let writer = messages.reserve(1).unwrap();
        let mut writing = messages.write_owned(writer).unwrap();
        writing.bytes()[0] = first;
        let other = messages.reserve(1).unwrap();
        messages.write(&other)[0] = second;
        assert_eq!(messages.reserve(1).err(), Some(Error::NoBuffer));
        assert_eq!(writing.bytes()[0], first);
        let lease = messages.seal(writing.into_writer(), 1).unwrap();
        kani::assert(
            messages.read(&lease)[0] == first,
            "owned writer recovery preserves initialized bytes",
        );
        messages.release(lease);
        messages.discard(other);
        assert!(messages
            .buffers
            .iter()
            .all(|b| b.state.get() == State::Free));
    })
    .unwrap();
    let length: usize = kani::any();
    kani::assume(length <= 2);
    let mut bytes = [first; 2];
    let mut buffers = [Buffer::new(&mut bytes)];
    with_messages(&mut buffers, &mut [], |messages| {
        let mut writer = messages.reserve_exact(length).unwrap();
        assert_eq!(messages.write(&writer).len(), length);
        assert!(messages
            .write_borrowed(&mut writer)
            .unwrap()
            .iter()
            .all(|byte| *byte == 0));
        let rejected = messages.seal(writer, length + 1).unwrap_err();
        assert_eq!(rejected.error, Error::InvalidLength);
        let mut writing = messages.write_owned(rejected.writer).unwrap();
        assert_eq!(writing.bytes().len(), length);
        let lease = messages.seal(writing.into_writer(), length).unwrap();
        assert_eq!(messages.read(&lease).len(), length);
        messages.release(lease);
    })
    .unwrap();
    assert!(bytes[length..].iter().all(|byte| *byte == first));
}

/// Arbitrary request/call bytes with a borrowed view held through independent
/// output completion and recycling. This executes the production borrow cells.
#[kani::proof]
#[kani::unwind(4)]
fn borrowed_request_survives_independent_output_reuse() {
    let input: u8 = kani::any();
    let output: u8 = kani::any();
    let mut first = [0];
    let mut second = [0];
    let mut buffers = [Buffer::new(&mut first), Buffer::new(&mut second)];
    let mut sends = [SendSlot::EMPTY];
    with_messages(&mut buffers, &mut sends, |messages| {
        let writer = messages.reserve(1).unwrap();
        messages.write(&writer)[0] = input;
        let request = messages.seal(writer, 1).unwrap();
        let view = messages.read(&request);
        let writer = messages.reserve(1).unwrap();
        messages.write(&writer)[0] = output;
        let response = messages.seal(writer, 1).unwrap();
        let offer = messages.offer(&response).unwrap();
        let send = messages.accept(offer);
        messages.release(response);
        assert_eq!(messages.send_window(send).unwrap().advance(1), Ok(true));
        let writer = messages.reserve(1).unwrap();
        messages.write(&writer)[0] = !input;
        messages.discard(writer);
        kani::assert(
            view[0] == input,
            "nested output cannot overwrite borrowed request",
        );
        drop(view);
        messages.release(request);
        assert!(messages
            .buffers
            .iter()
            .all(|buffer| buffer.state.get() == State::Free));
    })
    .unwrap();
}

/// One buffer and one send, arbitrary full-width starting generations. Scope
/// identity/borrow separation is checked independently by compiler contracts.
#[kani::proof]
#[kani::unwind(4)]
fn message_ownership_and_send_lifecycle() {
    let buffer_generation: u64 = kani::any();
    let send_generation: u64 = kani::any();
    let mut bytes = [7; 2];
    let mut buffers = [Buffer::new(&mut bytes)];
    buffers[0].generation.set(buffer_generation);
    let mut sends = [SendSlot {
        generation: send_generation,
        send: None,
    }];
    with_messages(&mut buffers, &mut sends, |messages| {
        let result = messages.reserve(2);
        if buffer_generation == u64::MAX {
            assert_eq!(result.unwrap_err(), Error::GenerationExhausted);
            return;
        }
        let writer = result.unwrap();
        assert_eq!(&*messages.write(&writer), &[0; 2]);
        messages.write(&writer).copy_from_slice(&[3, 5]);
        let lease = messages.seal(writer, 2).unwrap();
        let result = messages.offer(&lease);
        if send_generation == u64::MAX {
            assert_eq!(result.unwrap_err(), Error::GenerationExhausted);
            assert_eq!(&*messages.read(&lease), &[3, 5]);
            messages.release(lease);
            return;
        }
        let offer = result.unwrap();
        messages.release(lease);
        kani::assert(
            matches!(
                messages.buffers[0].state.get(),
                State::Sealed {
                    readers: 0,
                    sends: 1,
                    ..
                }
            ),
            "queued send retains message buffer",
        );
        assert_eq!(messages.reserve(2).unwrap_err(), Error::NoBuffer);
        if kani::any() {
            let key = messages.accept(offer);
            assert_eq!(messages.send_window(key).unwrap().advance(1), Ok(false));
            assert_eq!(messages.send_window(key).unwrap().bytes(), &[5]);
            if kani::any() {
                messages.fail_send(key).unwrap();
            } else {
                assert_eq!(messages.send_window(key).unwrap().advance(1), Ok(true));
            }
            assert_eq!(messages.fail_send(key), Err(Error::StaleSend));
            if buffer_generation < u64::MAX - 1 && send_generation < u64::MAX - 1 {
                let writer = messages.reserve(2).unwrap();
                let lease = messages.seal(writer, 2).unwrap();
                let offer = messages.offer(&lease).unwrap();
                let replacement = messages.accept(offer);
                assert_eq!(messages.fail_send(key), Err(Error::StaleSend));
                assert_eq!(&*messages.read(&lease), &[0; 2]);
                messages.release(lease);
                messages.fail_send(replacement).unwrap();
            }
        } else {
            messages.reject(offer);
        }
        assert_eq!(messages.buffers[0].state.get(), State::Free);
        assert!(messages.sends[0].send.is_none());
    })
    .unwrap();
}

/// Inductive byte completion over a four-byte buffer, arbitrary valid position,
/// length, reader count and full-width completion count. No wrap or early reuse.
#[kani::proof]
#[kani::unwind(6)]
fn partial_send_completion_preserves_ownership() {
    let length: usize = kani::any();
    let position: usize = kani::any();
    let readers: u32 = kani::any();
    let count: usize = kani::any();
    kani::assume(position <= length && length <= 4);
    let mut bytes: [u8; 4] = kani::any();
    let before = bytes;
    let mut buffers = [Buffer {
        bytes: RefCell::new(Bytes::Borrowed(&mut bytes)),
        capacity: Cell::new(4),
        generation: Cell::new(1),
        #[cfg(all(feature = "alloc", target_has_atomic = "ptr"))]
        received: Cell::new(false),
        state: Cell::new(State::Sealed {
            length,
            readers,
            sends: 1,
        }),
    }];
    let buffer = BufferKey {
        index: 0,
        generation: 1,
    };
    let mut sends = [SendSlot {
        generation: 1,
        send: Some(Send {
            buffer,
            position,
            phase: Phase::Accepted,
        }),
    }];
    let mut messages = Messages {
        buffers: Buffers::Borrowed(&mut buffers),
        sends: crate::storage::Storage::borrowed(&mut sends),
        brand: PhantomData,
    };
    let key = SendKey {
        index: 0,
        generation: 1,
        brand: PhantomData,
    };
    let result = messages.send_window(key).unwrap().advance(count);
    if count > length - position || (count == 0 && length != position) {
        assert_eq!(
            result,
            Err(if count > length - position {
                Error::InvalidWriteCount
            } else {
                Error::WriteZero
            })
        );
        assert_eq!(messages.accepted(key).unwrap().position, position);
        assert_eq!(
            messages.buffers[0].state.get(),
            State::Sealed {
                length,
                readers,
                sends: 1
            }
        );
    } else if count == length - position {
        assert_eq!(result, Ok(true));
        assert!(messages.sends[0].send.is_none());
        assert_eq!(
            messages.buffers[0].state.get(),
            if readers == 0 {
                State::Free
            } else {
                State::Sealed {
                    length,
                    readers,
                    sends: 0,
                }
            }
        );
    } else {
        kani::assert(
            messages.sends[0].send.is_some(),
            "partial write keeps send reserved",
        );
        assert_eq!(result, Ok(false));
        assert_eq!(messages.accepted(key).unwrap().position, position + count);
        assert_eq!(
            messages.buffers[0].state.get(),
            State::Sealed {
                length,
                readers,
                sends: 1
            }
        );
    }
    assert_eq!(&**messages.buffers[0].bytes.borrow(), &before);
}

#[kani::proof]
#[kani::unwind(4)]
fn borrowed_result_survives_independent_output_reuse() {
    let first: u8 = kani::any();
    let second: u8 = kani::any();
    let mut a = [0u8];
    let mut b = [0u8];
    let mut buffers = [Buffer::new(&mut a), Buffer::new(&mut b)];
    let mut sends = [SendSlot::EMPTY];
    with_messages(&mut buffers, &mut sends, |messages| {
        let mut writer = messages.reserve(1).unwrap();
        let mut view = messages.write_borrowed(&mut writer).unwrap();
        view[0] = first;
        let other = messages.reserve(1).unwrap();
        messages.write(&other)[0] = second;
        let lease = messages.seal(other, 1).unwrap();
        let offer = messages.offer(&lease).unwrap();
        let send = messages.accept(offer);
        messages.release(lease);
        assert_eq!(messages.send_window(send).unwrap().advance(1), Ok(true));
        let other = messages.reserve(1).unwrap();
        messages.write(&other)[0] = second.wrapping_add(1);
        kani::assert(
            view[0] == first,
            "borrowed result survives independent message reuse",
        );
        drop(view);
        let lease = messages.seal(writer, 1).unwrap();
        assert_eq!(messages.read(&lease)[0], first);
        messages.release(lease);
        messages.discard(other);
    })
    .unwrap();
}
