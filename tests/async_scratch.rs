//! Caller-owned async payload storage, checked against independent stream frames.
use capnp::message::{ReaderOptions, ReaderSegments};
use capnp::Word;
use capnp_futures::serialize::{read_message_with_scratch, try_read_message_with_scratch};
use futures::{executor::block_on, io::Cursor, AsyncRead, Future};
use std::{
    cell::Cell,
    io,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
};

fn frame(lengths: &[usize]) -> (Vec<u8>, Vec<Vec<u8>>) {
    let segments: Vec<Vec<u8>> = lengths
        .iter()
        .enumerate()
        .map(|(i, &words)| (0..words * 8).map(|j| (i * 31 + j) as u8).collect())
        .collect();
    let mut wire = ((lengths.len() - 1) as u32).to_le_bytes().to_vec();
    for &words in lengths {
        wire.extend_from_slice(&(words as u32).to_le_bytes());
    }
    if lengths.len().is_multiple_of(2) {
        wire.extend_from_slice(&[0; 4]);
    }
    for segment in &segments {
        wire.extend_from_slice(segment);
    }
    (wire, segments)
}

fn scratch(words: usize) -> Vec<Word> {
    vec![capnp::word(0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5); words]
}

struct Fragmented {
    bytes: Vec<u8>,
    position: Rc<Cell<usize>>,
    chunk: usize,
    pending: bool,
    fail_at: Option<usize>,
}
impl Fragmented {
    fn new(bytes: Vec<u8>, chunk: usize) -> Self {
        Self {
            bytes,
            position: Rc::new(Cell::new(0)),
            chunk,
            pending: false,
            fail_at: None,
        }
    }
}
impl AsyncRead for Fragmented {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if self.pending {
            self.pending = false;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        let start = self.position.get();
        if self.fail_at == Some(start) {
            return Poll::Ready(Err(io::Error::other("injected read failure")));
        }
        let mut count = buffer.len().min(self.chunk).min(self.bytes.len() - start);
        if let Some(end) = self.fail_at {
            count = count.min(end - start);
        }
        buffer[..count].copy_from_slice(&self.bytes[start..start + count]);
        self.position.set(start + count);
        self.pending = true;
        Poll::Ready(Ok(count))
    }
}

#[test]
fn fragmented_scratch_and_fallback_preserve_segments_and_frame_boundaries() {
    for count in [1, 2, 3, 4, 7, 511] {
        let lengths: Vec<_> = (0..count).map(|i| i % 3).collect();
        let total: usize = lengths.iter().sum();
        let (wire, expected) = frame(&lengths);
        for capacity in [0, total.saturating_sub(1), total, total + 3] {
            for chunk in [1, 3, 8, 31] {
                let mut input = Fragmented::new(wire.repeat(2), chunk);
                let mut words = scratch(capacity);
                let start = Word::words_to_bytes(&words).as_ptr();
                for message in 1..=2 {
                    let reader = block_on(read_message_with_scratch(
                        &mut input,
                        &mut words,
                        ReaderOptions::new(),
                    ))
                    .unwrap();
                    let segments = reader.into_segments();
                    assert_eq!(segments.uses_scratch(), total <= capacity);
                    assert_eq!(segments.len(), count);
                    let mut offset = 0;
                    for (id, expected) in expected.iter().enumerate() {
                        let bytes = segments.get_segment(id as u32).unwrap();
                        assert_eq!(bytes, expected);
                        if segments.uses_scratch() {
                            assert_eq!(bytes.as_ptr(), start.wrapping_add(offset));
                        }
                        offset += bytes.len();
                    }
                    assert!(segments.get_segment(count as u32).is_none());
                    assert!(segments.get_segment(u32::MAX).is_none());
                    drop(segments);
                    let used = if total <= capacity { total * 8 } else { 0 };
                    assert!(Word::words_to_bytes(&words)[used..]
                        .iter()
                        .all(|&b| b == 0xa5));
                    assert_eq!(input.position.get(), wire.len() * message);
                }
                assert!(block_on(try_read_message_with_scratch(
                    &mut input,
                    &mut words,
                    ReaderOptions::new(),
                ))
                .unwrap()
                .is_none());
            }
        }
    }
}

#[test]
fn scratch_reader_decodes_a_typed_message() {
    let mut builder = capnp::message::Builder::new_default();
    builder
        .set_root::<capnp::text::Owned>("caller-owned async words")
        .unwrap();
    let wire = capnp::serialize::write_message_to_words(&builder);
    for capacity in [0, 64] {
        let mut words = scratch(capacity);
        let message = block_on(read_message_with_scratch(
            Cursor::new(&wire),
            &mut words,
            ReaderOptions::new(),
        ))
        .unwrap();
        assert_eq!(
            message
                .get_root::<capnp::text::Reader>()
                .unwrap()
                .to_str()
                .unwrap(),
            "caller-owned async words"
        );
    }
}

#[test]
fn eof_is_clean_only_before_the_first_header_byte() {
    for lengths in [&[1][..], &[0, 2], &[1, 0, 2], &[0, 1, 0, 2]] {
        let (wire, _) = frame(lengths);
        for capacity in [0, 16] {
            let mut words = scratch(capacity);
            for end in 1..wire.len() {
                let input = Fragmented::new(wire[..end].to_vec(), 3);
                assert!(
                    block_on(try_read_message_with_scratch(
                        input,
                        &mut words,
                        ReaderOptions::new(),
                    ))
                    .is_err(),
                    "truncated frame accepted: {lengths:?}, {end}"
                );
            }
            assert!(block_on(try_read_message_with_scratch(
                Cursor::new(&[]),
                &mut words,
                ReaderOptions::new(),
            ))
            .unwrap()
            .is_none());
            let error = block_on(read_message_with_scratch(
                Cursor::new(&[]),
                &mut words,
                ReaderOptions::new(),
            ))
            .err()
            .unwrap();
            assert_eq!(error.kind, capnp::ErrorKind::PrematureEndOfFile);
        }
    }
}

#[test]
fn invalid_counts_and_limits_fail_before_touching_scratch_or_reading_payload() {
    let (wire, _) = frame(&[1, 2, 3]);
    let mut options = ReaderOptions::new();
    options.traversal_limit_in_words = Some(5);
    for capacity in [0, 16] {
        let mut words = scratch(capacity);
        let mut input = Cursor::new(&wire);
        assert!(block_on(try_read_message_with_scratch(
            &mut input, &mut words, options
        ))
        .is_err());
        assert_eq!(input.position(), 16);
        assert!(Word::words_to_bytes(&words).iter().all(|&b| b == 0xa5));
    }
    for count_minus_one in [511u32, u32::MAX] {
        let mut wire = count_minus_one.to_le_bytes().to_vec();
        wire.extend_from_slice(&1u32.to_le_bytes());
        wire.extend_from_slice(&[0; 32]);
        let mut input = Cursor::new(wire);
        let mut words = scratch(8);
        assert!(block_on(try_read_message_with_scratch(
            &mut input,
            &mut words,
            ReaderOptions::new(),
        ))
        .is_err());
        assert_eq!(input.position(), 8);
        assert!(Word::words_to_bytes(&words).iter().all(|&b| b == 0xa5));
    }
}

#[test]
fn cancelled_partial_read_releases_scratch_for_a_fresh_stream() {
    let (wire, expected) = frame(&[4]);
    let mut input = Fragmented::new(wire.clone(), 3);
    let position = input.position.clone();
    let mut words = scratch(8);
    let mut read = Box::pin(read_message_with_scratch(
        &mut input,
        &mut words,
        ReaderOptions::new(),
    ));
    let mut cx = Context::from_waker(futures::task::noop_waker_ref());
    loop {
        assert!(read.as_mut().poll(&mut cx).is_pending());
        if position.get() > 8 {
            break;
        }
    }
    drop(read);
    let consumed = position.get() - 8;
    assert!(consumed < expected[0].len());
    assert_eq!(
        &Word::words_to_bytes(&words)[..consumed],
        &expected[0][..consumed]
    );
    assert!(Word::words_to_bytes(&words)[consumed..]
        .iter()
        .all(|&b| b == 0xa5));
    let reader = block_on(read_message_with_scratch(
        Cursor::new(wire),
        &mut words,
        ReaderOptions::new(),
    ))
    .unwrap();
    assert_eq!(reader.into_segments().get_segment(0).unwrap(), expected[0]);
}

#[test]
fn io_errors_are_not_clean_eof_and_leave_only_the_read_prefix_modified() {
    let (wire, expected) = frame(&[4]);
    for end in [0, 1, 8, 11, wire.len() - 1] {
        for capacity in [0, 8] {
            let mut input = Fragmented::new(wire.clone(), 3);
            input.fail_at = Some(end);
            let mut words = scratch(capacity);
            let error = block_on(try_read_message_with_scratch(
                &mut input,
                &mut words,
                ReaderOptions::new(),
            ))
            .err()
            .expect("I/O errors must propagate");
            assert!(error.to_string().contains("injected read failure"));
            assert_eq!(input.position.get(), end);
            if capacity > 0 {
                let changed = end.saturating_sub(8);
                assert_eq!(
                    &Word::words_to_bytes(&words)[..changed],
                    &expected[0][..changed]
                );
                assert!(Word::words_to_bytes(&words)[changed..]
                    .iter()
                    .all(|&b| b == 0xa5));
            }
        }
    }
}

#[test]
fn scratch_storage_and_rejections_match_pinned_cpp() {
    use reproto_test_support::verification::{command, cpp, root, run};
    use std::{fmt::Write, fs};

    // (scratch capacity, traversal budget, independently encoded wire bytes).
    let mut cases = vec![(8, 1024, vec![])];
    for lengths in [
        vec![0],
        vec![1],
        vec![2, 1],
        vec![0, 2, 1],
        vec![1, 0, 2, 1],
        vec![0; 511],
    ] {
        let (wire, _) = frame(&lengths);
        let total: usize = lengths.iter().sum();
        for capacity in [0, total.saturating_sub(1), total, total + 3] {
            cases.push((capacity, 1024, wire.clone()));
            // The second message proves that neither implementation prefetches.
            cases.push((capacity, 1024, wire.repeat(2)));
        }
        cases.push((total + 1, total.saturating_sub(1), wire));
    }
    let (wire, _) = frame(&[1, 0, 2, 1]);
    for end in 1..wire.len() {
        for capacity in [0, 16] {
            cases.push((capacity, 1024, wire[..end].to_vec()));
        }
    }
    for count in [511u32, u32::MAX] {
        let mut header = count.to_le_bytes().to_vec();
        header.extend_from_slice(&1u32.to_le_bytes());
        cases.push((16, 1024, header));
    }
    let mut oversized = 0u32.to_le_bytes().to_vec();
    oversized.extend_from_slice(&u32::MAX.to_le_bytes());
    cases.push((16, 1024, oversized));

    let mut input_text = String::new();
    let mut expected = String::new();
    for (id, (capacity, limit, wire)) in cases.iter().enumerate() {
        write!(&mut input_text, "{id} {capacity} {limit} ").unwrap();
        if wire.is_empty() {
            input_text.push('-');
        }
        for byte in wire {
            write!(&mut input_text, "{byte:02x}").unwrap();
        }
        input_text.push('\n');
        let mut words = scratch(*capacity);
        let mut input = Cursor::new(wire);
        let mut options = ReaderOptions::new();
        options.traversal_limit_in_words = Some(*limit);
        let result = block_on(try_read_message_with_scratch(
            &mut input, &mut words, options,
        ));
        let mut borrowed = false;
        let mut used = 0;
        let mut contents = String::new();
        let state = match result {
            Ok(Some(message)) => {
                let segments = message.into_segments();
                borrowed = segments.uses_scratch();
                for id in 0..segments.len() {
                    let bytes = segments.get_segment(id as u32).unwrap();
                    if borrowed {
                        used += bytes.len();
                    }
                    write!(&mut contents, "{}:", bytes.len()).unwrap();
                    for byte in bytes {
                        write!(&mut contents, "{byte:02x}").unwrap();
                    }
                    contents.push(';');
                }
                "ok"
            }
            Ok(None) => "eof",
            Err(_) => "error",
        };
        let tail = state == "error"
            || Word::words_to_bytes(&words)[used..]
                .iter()
                .all(|&b| b == 0xa5);
        writeln!(
            &mut expected,
            "{id} {state} {} {} {} {contents}",
            input.position(),
            u8::from(borrowed),
            u8::from(tail)
        )
        .unwrap();
    }

    let build = cpp::build(&["capnp-rpc"]).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("async-scratch");
    let logs = root().join("target/verification/async-scratch-cpp");
    run(
        command("g++")
            .args([
                "-std=c++23",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-Ivendor/capnproto/c++/src",
                "tests/cpp/async-scratch.c++",
            ])
            .arg(build.join("c++/src/capnp/libcapnp-rpc.a"))
            .arg(build.join("c++/src/capnp/libcapnp.a"))
            .arg(build.join("c++/src/kj/libkj-async.a"))
            .arg(build.join("c++/src/kj/libkj.a"))
            .args(["-pthread", "-o"])
            .arg(&executable),
        &logs.join("compile.log"),
        0,
    )
    .unwrap();
    let input_path = directory.path().join("cases.txt");
    fs::write(&input_path, &input_text).unwrap();
    let actual = run(
        command(&executable).arg(&input_path),
        &logs.join("reference.log"),
        0,
    )
    .unwrap();
    assert_eq!(
        actual, expected,
        "async scratch behavior differs from pinned C++"
    );
    eprintln!("{} async scratch cases match pinned C++", cases.len());
}
