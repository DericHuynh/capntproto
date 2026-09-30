use super::*;
use capnp::{message::ReaderOptions, Word};

fn words(n: usize) -> Vec<Word> {
    let mut words = Word::allocate_zeroed_vec(n);
    Word::words_to_bytes_mut(&mut words).fill(0xa5);
    words
}

#[test]
fn scratch_capacity_includes_framing_and_preserves_unused_words() {
    for size in [0, 17, 300, 1400, 5000] {
        let frame = data(size, 19);
        let needed = frame.len() / 8;
        for capacity in [0, needed - 1, needed, needed + 7] {
            for short in [false, true] {
                let input = source(frame.clone());
                let mut stream =
                    BufferedRead::with_buffer_size(input, Default::default(), 256).unwrap();
                let mut scratch = words(capacity);
                let start = scratch.as_ptr() as usize;
                let message = stream
                    .try_read_message_with_scratch(&mut scratch, |_| Ok(short))
                    .now_or_never()
                    .unwrap()
                    .unwrap()
                    .unwrap();
                let buffered = frame.len() <= 2048;
                let borrowed = buffered && !short && capacity >= needed;
                assert_eq!(message.get_segments().uses_scratch(), borrowed);
                assert_eq!(message.get_segments().is_shared_buffer(), buffered && short);
                assert_eq!(
                    message.get_root::<capnp::data::Reader>().unwrap(),
                    vec![19; size]
                );
                if borrowed {
                    let count = message.get_segments().len();
                    assert_eq!(
                        message.get_segments().get_segment(0).unwrap().as_ptr() as usize,
                        start + (count / 2 + 1) * 8
                    );
                }
                // The view can outlive its reader, even for shared and scratch storage.
                drop(stream);
                let owned = message.into_segments().into_owned();
                let used = if borrowed { frame.len() } else { 0 };
                assert!(Word::words_to_bytes(&scratch)[used..]
                    .iter()
                    .all(|&v| v == 0xa5));
                if borrowed {
                    assert_eq!(&Word::words_to_bytes(&scratch)[..used], frame);
                }
                scratch.fill(capnp::word(0, 0, 0, 0, 0, 0, 0, 0));
                let owned = Reader::new(owned, Default::default());
                assert_eq!(
                    owned.get_root::<capnp::data::Reader>().unwrap(),
                    vec![19; size]
                );
            }
        }
    }
}

#[test]
fn scratch_readers_survive_prefetch_reuse_and_shared_rejections() {
    let input = source([data(17, 1), data(17, 2), data(17, 3)].concat());
    let mut stream = BufferedRead::new(input.clone(), Default::default());
    let mut a = words(64);
    let mut b = words(64);
    let first = stream
        .try_read_message_with_scratch(&mut a, |_| Ok(false))
        .now_or_never()
        .unwrap()
        .unwrap()
        .unwrap();
    let shared = stream
        .try_read_message_with_scratch(&mut b, |_| Ok(true))
        .now_or_never()
        .unwrap()
        .unwrap()
        .unwrap();
    let calls = input.0.borrow().calls;
    assert!(stream
        .try_read_message_with_scratch(&mut [], |_| Ok(false))
        .now_or_never()
        .unwrap()
        .is_err());
    assert_eq!(input.0.borrow().calls, calls);
    drop(shared);
    let last = stream
        .try_read_message_with_scratch(&mut b, |_| Ok(false))
        .now_or_never()
        .unwrap()
        .unwrap()
        .unwrap();
    drop(stream);
    assert_eq!(first.get_root::<capnp::data::Reader>().unwrap(), &[1; 17]);
    assert_eq!(last.get_root::<capnp::data::Reader>().unwrap(), &[3; 17]);
    assert_eq!(input.0.borrow().calls, 1);
}

#[test]
fn canceled_scratch_reads_resume_with_different_storage_at_every_prefix() {
    for size in [17, 3000] {
        let frame = data(size, 7);
        let input = source([frame.clone(), data(8, 9)].concat());
        input.0.borrow_mut().available = 0;
        input.0.borrow_mut().closed = false;
        let mut stream =
            BufferedRead::with_buffer_size(input.clone(), Default::default(), 256).unwrap();
        for available in 0..frame.len() {
            input.0.borrow_mut().available = available;
            let mut scratch = words(512);
            assert!(stream
                .try_read_message_with_scratch(&mut scratch, |_| Ok(false))
                .now_or_never()
                .is_none());
            assert!(Word::words_to_bytes(&scratch).iter().all(|&v| v == 0xa5));
        }
        input.0.borrow_mut().available = frame.len();
        let mut scratch = words(512);
        let msg = stream
            .try_read_message_with_scratch(&mut scratch, |_| Ok(false))
            .now_or_never()
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            msg.get_root::<capnp::data::Reader>().unwrap(),
            vec![7; size]
        );
        assert_eq!(msg.get_segments().uses_scratch(), size == 17);
        drop(msg);
        let end = input.0.borrow().bytes.len();
        input.0.borrow_mut().available = end;
        let next = stream
            .try_read_message_with_scratch(&mut scratch, |_| Ok(false))
            .now_or_never()
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(next.get_root::<capnp::data::Reader>().unwrap(), &[9; 8]);
    }
}

#[test]
fn errors_and_eof_leave_scratch_unchanged_and_fail_stickily() {
    let frame = data(17, 3);
    let mut cases = vec![(vec![], true), (vec![255; 8], false)];
    cases.extend((1..frame.len()).map(|n| (frame[..n].to_vec(), false)));
    for (bytes, eof) in cases {
        let input = source(bytes);
        let mut stream = BufferedRead::new(input.clone(), Default::default());
        let mut scratch = words(512);
        for _ in 0..2 {
            let result = stream
                .try_read_message_with_scratch(&mut scratch, |_| Ok(false))
                .now_or_never()
                .unwrap();
            if eof {
                assert!(result.unwrap().is_none());
            } else {
                assert!(result.is_err());
            }
        }
        assert!(Word::words_to_bytes(&scratch).iter().all(|&v| v == 0xa5));
    }
    for error in ["limit", "io", "classifier"] {
        let input = source(frame.clone());
        input.0.borrow_mut().failed = error == "io";
        let mut options = ReaderOptions::new();
        if error == "limit" {
            options.traversal_limit_in_words = Some(0);
        }
        let mut stream = BufferedRead::new(input.clone(), options);
        let mut scratch = words(512);
        assert!(stream
            .try_read_message_with_scratch(&mut scratch, |_| Err(capnp::Error::failed(
                "classifier failure".into()
            )))
            .now_or_never()
            .unwrap()
            .is_err());
        let calls = input.0.borrow().calls;
        assert!(stream
            .try_read_message_with_scratch(&mut scratch, |_| Ok(false))
            .now_or_never()
            .unwrap()
            .is_err());
        assert_eq!(input.0.borrow().calls, calls);
        assert!(Word::words_to_bytes(&scratch).iter().all(|&v| v == 0xa5));
    }
}

#[test]
fn multisegment_scratch_uses_frame_offsets_including_maximum_tables() {
    for count in [1usize, 2, 3, 4, 511, 512] {
        let lengths: Vec<_> = (0..count).map(|i| i % 3).collect();
        let table = (count / 2 + 1) * 8;
        let mut bytes = vec![0; table];
        bytes[..4].copy_from_slice(&((count - 1) as u32).to_le_bytes());
        for (i, &n) in lengths.iter().enumerate() {
            bytes[4 + i * 4..8 + i * 4].copy_from_slice(&(n as u32).to_le_bytes());
            bytes.extend(vec![i as u8; n * 8]);
        }
        let mut scratch = words(bytes.len() / 8 + 1);
        let input = source(bytes.clone());
        let mut stream = BufferedRead::new(input, Default::default());
        let result = stream
            .try_read_message_with_scratch(&mut scratch, |_| Ok(false))
            .now_or_never()
            .unwrap();
        if count == 512 {
            assert!(result.is_err());
            assert!(Word::words_to_bytes(&scratch).iter().all(|&v| v == 0xa5));
            continue;
        }
        let message = result.unwrap().unwrap();
        assert!(message.get_segments().uses_scratch());
        for (i, &n) in lengths.iter().enumerate() {
            assert_eq!(
                message.get_segments().get_segment(i as u32).unwrap(),
                vec![i as u8; n * 8]
            );
        }
        assert!(message.get_segments().get_segment(count as u32).is_none());
        drop(message);
        assert_eq!(&Word::words_to_bytes(&scratch)[..bytes.len()], bytes);
        assert_eq!(&Word::words_to_bytes(&scratch)[bytes.len()..], &[0xa5; 8]);
    }
}
