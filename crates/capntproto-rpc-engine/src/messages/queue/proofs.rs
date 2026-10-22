use super::*;
#[kani::proof]
#[kani::unwind(4)]
fn fifo_never_overtakes_partial_frame() {
    let count: usize = kani::any();
    kani::assume(count <= 2);
    let owned: bool = kani::any();
    let mut a = [0; 2];
    let mut b = [0; 2];
    let mut buffers = [Buffer::new(&mut a), Buffer::new(&mut b)];
    let mut sends = [SendSlot::EMPTY, SendSlot::EMPTY];
    with_messages(&mut buffers, &mut sends, |messages| {
        let mut slots = [const { None }; 2];
        let mut queue = Queue::new(&mut slots).unwrap();
        for bytes in [[3u8, 5], [7, 9]] {
            let writer = messages.reserve(2).unwrap();
            messages.write(&writer).copy_from_slice(&bytes);
            let lease = messages.seal(writer, 2).unwrap();
            if owned {
                queue.reserve().unwrap().accept_owned(messages, lease);
            } else {
                let offer = messages.offer(&lease).unwrap();
                queue.reserve().unwrap().accept(messages, offer);
                messages.release(lease);
            }
        }
        let result = queue.window(messages).unwrap().unwrap().advance(count);
        let expected: &[u8] = match count {
            0 => {
                assert_eq!(result, Err(Error::WriteZero));
                &[3, 5]
            }
            1 => &[5],
            _ => &[7, 9],
        };
        kani::assert(
            queue.window(messages).unwrap().unwrap().bytes() == expected,
            "FIFO preserves oldest unsent suffix",
        );
        assert_eq!(queue.len(), if count == 2 { 1 } else { 2 });
    })
    .unwrap();
}

#[kani::proof]
#[kani::unwind(4)]
fn terminal_queue_discards_without_releasing_reader() {
    let mut bytes = [0; 2];
    let mut buffers = [Buffer::new(&mut bytes)];
    with_messages(&mut buffers, &mut [SendSlot::EMPTY], |messages| {
        let mut slots = [None];
        let mut queue = Queue::new(&mut slots).unwrap();
        let writer = messages.reserve(2).unwrap();
        messages.write(&writer).copy_from_slice(&[17, 23]);
        let lease = messages.seal(writer, 2).unwrap();
        let offer = messages.offer(&lease).unwrap();
        queue.reserve().unwrap().accept(messages, offer);
        assert!(!queue.window(messages).unwrap().unwrap().advance(1).unwrap());
        let live = queue.discard_one(messages);
        kani::assert(
            live == Err(Error::QueueStillLive),
            "live output cannot discard an accepted send",
        );
        queue.fail();
        assert!(queue.discard_one(messages).unwrap());
        assert!(!queue.discard_one(messages).unwrap());
        assert_eq!(&*messages.read(&lease), &[17, 23]);
        assert!(matches!(messages.reserve(2), Err(Error::NoBuffer)));
        messages.release(lease);
        let writer = messages.reserve(2).unwrap();
        messages.discard(writer);
        assert!(matches!(queue.reserve(), Err(Error::QueueFailed)));
    })
    .unwrap();
}

#[kani::proof]
#[kani::unwind(4)]
fn resources_are_offered_only_before_first_accepted_byte() {
    let count: usize = kani::any();
    kani::assume(count <= 4);
    let owned: bool = kani::any();
    let mut bytes = [0; 3];
    let mut buffers = [Buffer::new(&mut bytes)];
    with_messages(&mut buffers, &mut [SendSlot::EMPTY], |messages| {
        let mut entries = [None];
        let mut queue = Queue::with_resources(&mut entries).unwrap();
        let writer = messages.reserve(3).unwrap();
        let lease = messages.seal(writer, 3).unwrap();
        if owned {
            queue
                .reserve_with(71u8)
                .unwrap()
                .accept_owned(messages, lease);
        } else {
            let offer = messages.offer(&lease).unwrap();
            queue.reserve_with(71u8).unwrap().accept(messages, offer);
            messages.release(lease);
        }
        assert_eq!(
            queue.window(messages).unwrap().unwrap().resources(),
            Some(&71)
        );
        let result = queue.window(messages).unwrap().unwrap().advance(count);
        if count == 3 {
            assert_eq!(result, Ok(true));
            assert!(queue.is_empty());
        } else {
            let window = queue.window(messages).unwrap().unwrap();
            let valid = count > 0 && count < 3;
            kani::assert(
                window.resources().is_none() == valid,
                "attachments stop exactly after first accepted byte",
            );
            assert_eq!(window.bytes().len(), if valid { 3 - count } else { 3 });
        }
        queue.fail();
        if count != 3 {
            assert!(queue.discard_one(messages).unwrap());
        }
        assert!(queue.is_empty());
    })
    .unwrap();
}
