use super::*;
use capnp::message::ReaderSegments;
use io::{Close, Flush, Read, WriteResources};

#[derive(Default)]
struct Gate {
    result: RefCell<Option<capnp::Result<()>>>,
    wake: RefCell<Option<Waker>>,
}
impl Gate {
    fn complete(&self, result: capnp::Result<()>) {
        *self.result.borrow_mut() = Some(result);
        let wake = self.wake.borrow_mut().take();
        if let Some(wake) = wake {
            wake.wake();
        }
    }
    fn promise(self: &Rc<Self>) -> Promise<(), Error> {
        let gate = self.clone();
        Promise::from_future(std::future::poll_fn(move |cx| {
            *gate.wake.borrow_mut() = Some(cx.waker().clone());
            gate.result
                .borrow()
                .clone()
                .map_or(Poll::Pending, Poll::Ready)
        }))
    }
}
struct Message {
    segments: Vec<Vec<u8>>,
    size: Option<usize>,
}
impl crate::IncomingMessage for Message {
    fn get_segments(&self) -> Option<&dyn ReaderSegments> {
        Some(&self.segments)
    }
    fn size_in_words(&self) -> usize {
        self.size
            .unwrap_or_else(|| self.segments.iter().map(|s| s.len() / 8).sum())
    }
    fn get_body(&self) -> capnp::Result<capnp::any_pointer::Reader<'_>> {
        panic!("the adapter must never traverse or rebuild capability pointers")
    }
}
#[derive(Default)]
struct State {
    incoming: RefCell<VecDeque<Message>>,
    reads: Cell<usize>,
    canceled_reads: Cell<usize>,
    sends: RefCell<Vec<Vec<u8>>>,
    retain_frames: Cell<bool>,
    frames: RefCell<Vec<crate::OutgoingFrame>>,
    send: Rc<Gate>,
    close: Rc<Gate>,
    finished: Rc<Gate>,
    shutdowns: RefCell<Vec<capnp::Result<()>>>,
}
struct PendingRead(Rc<State>);
impl Drop for PendingRead {
    fn drop(&mut self) {
        self.0.canceled_reads.set(self.0.canceled_reads.get() + 1);
    }
}
struct Transport(Rc<State>);
impl crate::Connection<u8> for Transport {
    fn get_peer_vat_id(&self) -> u8 {
        7
    }
    fn connection_id(&self) -> usize {
        23
    }
    fn new_outgoing_message(&mut self, _: u32) -> Box<dyn crate::OutgoingMessage> {
        panic!("raw frames must not be rebuilt")
    }
    fn receive_incoming_message(
        &mut self,
    ) -> Promise<Option<Box<dyn crate::IncomingMessage>>, Error> {
        self.0.reads.set(self.0.reads.get() + 1);
        if let Some(message) = self.0.incoming.borrow_mut().pop_front() {
            return Promise::ok(Some(Box::new(message)));
        }
        let guard = PendingRead(self.0.clone());
        Promise::from_future(async move {
            let _guard = guard;
            std::future::pending().await
        })
    }
    fn send_frame(&mut self, frame: crate::OutgoingFrame) -> Promise<(), Error> {
        self.0
            .sends
            .borrow_mut()
            .push(capnp::serialize::write_message_segments_to_words(
                &frame.segments(),
            ));
        if self.0.retain_frames.get() {
            self.0.frames.borrow_mut().push(frame);
        }
        self.0.send.promise()
    }
    fn when_write_finished(&self) -> Option<Promise<(), Error>> {
        Some(self.0.finished.promise())
    }
    fn shutdown(&mut self, result: capnp::Result<()>) -> Promise<(), Error> {
        self.0.shutdowns.borrow_mut().push(result);
        self.0.close.promise()
    }
}
fn fixture() -> (Connection<u8>, Rc<State>) {
    let state = Rc::new(State::default());
    (Connection::new(Box::new(Transport(state.clone()))), state)
}
fn cx() -> Context<'static> {
    Context::from_waker(Waker::noop())
}

#[test]
fn shared_input_adoption_validates_ranges_and_falls_back_without_consuming() {
    struct Frames {
        bytes: std::sync::Arc<Vec<u8>>,
        range: core::ops::Range<usize>,
        wrong_range: bool,
        finished: Rc<Cell<usize>>,
    }
    impl crate::IncomingFrameStream for Frames {
        fn poll_frame(&mut self, _: &mut Context<'_>) -> Poll<capnp::Result<Option<&[u8]>>> {
            Poll::Ready(Ok(self.frame()))
        }
        fn frame(&self) -> Option<&[u8]> {
            (self.finished.get() == 0).then(|| &self.bytes[self.range.clone()])
        }
        fn share_frame(&mut self) -> Option<(std::sync::Arc<Vec<u8>>, core::ops::Range<usize>)> {
            let mut range = self.range.clone();
            if self.wrong_range {
                range.end -= 1;
            }
            Some((self.bytes.clone(), range))
        }
        fn finish_frame(&mut self) {
            self.finished.set(self.finished.get() + 1);
        }
    }
    for fault in 0..5 {
        let (connection, state) = fixture();
        let (mut reader, _writer, _failure) = connection.into_io(NonZeroUsize::new(32).unwrap());
        let expected = frame();
        let mut bytes = vec![0; if fault == 1 { 256 } else { expected.len() + 7 }];
        let start = bytes.as_ptr().align_offset(8);
        bytes[start..start + expected.len()].copy_from_slice(&expected);
        let bytes = std::sync::Arc::new(bytes);
        let length = if fault == 2 { 16 } else { expected.len() };
        let finished = Rc::new(Cell::new(0));
        reader.input_frames = Some(Box::new(Frames {
            bytes: bytes.clone(),
            range: start..start + length,
            wrong_range: fault == 4,
            finished: finished.clone(),
        }));
        assert!(
            matches!(reader.poll_frame_size(&mut cx()), Poll::Ready(Ok(Some(n))) if n == length)
        );
        let mut pool = messages::GrowingBuffers::new(1, 32, 39);
        messages::with_growing_messages(
            &mut pool,
            capntproto_rpc_engine::connection::Growth {
                initial: 0,
                maximum: 0,
            },
            |messages| {
                let options = capnp::message::ReaderOptions {
                    traversal_limit_in_words: Some(if fault == 3 { 1 } else { 2 }),
                    ..Default::default()
                };
                let result = reader.take_shared_frame(messages, options);
                assert_eq!(finished.get(), 0);
                match fault {
                    0 => {
                        let frame = result.unwrap().unwrap();
                        let shared = frame.share(messages).unwrap();
                        assert_eq!(&*shared, expected);
                        assert_eq!(shared.as_ptr(), bytes[start..].as_ptr());
                        frame.release(messages);
                        reader.finish_frame();
                        assert_eq!(&*shared, expected);
                        assert_eq!(messages.reserve(1).unwrap_err(), messages::Error::NoBuffer);
                        drop(shared);
                    }
                    1 => {
                        assert!(result.unwrap().is_none());
                        assert_eq!(reader.buffered_frame().unwrap(), expected);
                        let frame = reader
                            .take_buffered_frame(messages, options)
                            .unwrap()
                            .unwrap();
                        assert_eq!(&*frame.share(messages).unwrap(), expected);
                        frame.release(messages);
                        reader.finish_frame();
                    }
                    _ => {
                        assert!(result.is_err());
                        assert_eq!(reader.position, 0);
                        assert_eq!(reader.remaining, length);
                        reader.finish_frame();
                    }
                }
                assert_eq!(finished.get(), 1);
                let writer = messages.reserve(24).unwrap();
                messages.discard(writer);
            },
        )
        .unwrap();
        assert_eq!(state.reads.get(), 0);
    }
}
fn frame() -> Vec<u8> {
    capnp::serialize::write_message_segments_to_words(&vec![vec![0u8; 16]])
}

#[cfg(unix)]
#[test]
fn attachment_storage_is_lazy_and_preserves_limits_and_ownership() {
    use std::os::{fd::OwnedFd, unix::net::UnixStream};
    let (socket, _peer) = UnixStream::pair().unwrap();
    let fd = Rc::new(OwnedFd::from(socket));
    let disabled = Resources::new(0);
    let mut bundle = disabled.bundle();
    assert_eq!(disabled.attach_fd(&mut bundle, Some(fd.clone())), u8::MAX);
    assert!(bundle.fds.is_none());
    assert_eq!(Rc::strong_count(&fd), 1);

    let resources = Resources::new(5);
    assert_eq!(resources.attach_fd(&mut bundle, None), u8::MAX);
    assert!(bundle.fds.is_none());
    for index in 0..5 {
        assert_eq!(resources.attach_fd(&mut bundle, Some(fd.clone())), index);
    }
    assert_eq!(resources.attach_fd(&mut bundle, Some(fd.clone())), u8::MAX);
    assert_eq!(bundle.fds.as_ref().unwrap().len(), 5);
    assert_eq!(Rc::strong_count(&fd), 6);
    assert!(bundle
        .fds
        .as_ref()
        .unwrap()
        .iter()
        .all(|retained| Rc::ptr_eq(retained, &fd)));
    drop(bundle);
    assert_eq!(Rc::strong_count(&fd), 1);
}

#[test]
fn buffered_frame_transfer_validates_bytes_and_holds_the_message_through_admission() {
    struct Raw {
        bytes: Vec<u8>,
        size: usize,
    }
    impl crate::IncomingMessage for Raw {
        fn framed_bytes(&self) -> Option<&[u8]> {
            Some(&self.bytes)
        }
        fn size_in_words(&self) -> usize {
            self.size
        }
        fn get_body(&self) -> capnp::Result<any_pointer::Reader<'_>> {
            panic!("must not traverse raw input")
        }
    }
    for (length, size, limit, valid) in [
        (24, 2, 2, true),
        (16, 2, 2, false),
        (32, 2, 2, false),
        (24, 1, 2, false),
        (24, 2, 1, false),
    ] {
        let (connection, state) = fixture();
        let (mut reader, _writer, _failure) = connection.into_io(NonZeroUsize::new(32).unwrap());
        let mut bytes = frame();
        bytes.resize(length, 0);
        reader.start(Box::new(Raw { bytes, size })).unwrap();
        let mut words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 4];
        let mut buffers = [messages::Buffer::new(capnp::Word::words_to_bytes_mut(
            &mut words,
        ))];
        messages::with_messages(&mut buffers, &mut [], |messages| {
            let result = reader.take_buffered_frame(
                messages,
                capnp::message::ReaderOptions {
                    traversal_limit_in_words: Some(limit),
                    ..Default::default()
                },
            );
            assert_eq!(result.is_ok(), valid);
            if let Ok(Some(frame)) = result {
                assert!(
                    reader.message.is_some(),
                    "admission still owns the message and attachments"
                );
                assert!(reader
                    .take_buffered_frame(messages, Default::default())
                    .unwrap()
                    .is_none());
                frame.release(messages);
            }
            assert_eq!(state.reads.get(), 0);
            reader.finish_frame();
            assert!(reader.message.is_none());
            let writer = messages.reserve(32).unwrap();
            messages.discard(writer);
        })
        .unwrap();
    }
}
fn ready_ok<T>(value: Poll<Result<T, IoError<Error>>>) -> T {
    match value {
        Poll::Ready(Ok(value)) => value,
        Poll::Ready(Err(error)) => panic!("IO failed: {error:?}"),
        Poll::Pending => panic!("expected successful IO, got Pending"),
    }
}

#[test]
fn raw_frame_fallback_preserves_partial_reads_and_admission_boundary() {
    struct Raw(Vec<u8>);
    impl crate::IncomingMessage for Raw {
        fn framed_bytes(&self) -> Option<&[u8]> {
            Some(&self.0)
        }
        fn size_in_words(&self) -> usize {
            2
        }
        fn get_body(&self) -> capnp::Result<any_pointer::Reader<'_>> {
            panic!("raw fallback must not traverse the payload")
        }
    }
    for consume in [false, true] {
        let (connection, state) = fixture();
        let (mut reader, _writer, _) = connection.into_io(NonZeroUsize::new(32).unwrap());
        let expected = frame();
        reader.start(Box::new(Raw(expected.clone()))).unwrap();
        let mut received = Vec::new();
        while received.len() < expected.len() {
            let mut bytes = [0xa5; 5];
            let count = ready_ok(reader.poll_read(&mut cx(), &mut bytes));
            assert!(count > 0);
            received.extend_from_slice(&bytes[..count]);
            assert_eq!(&bytes[count..], &vec![0xa5; 5 - count]);
            if consume {
                break;
            }
        }
        if consume {
            reader.consume_buffered_frame();
        } else {
            assert_eq!(received, expected);
            assert!(matches!(
                reader.poll_read(&mut cx(), &mut [0; 1]),
                Poll::Ready(Err(_))
            ));
            reader.finish_frame();
        }
        assert!(reader.message.is_none());
        if consume {
            assert!(reader.poll_read(&mut cx(), &mut [0; 1]).is_pending());
            assert_eq!(state.reads.get(), 1);
        } else {
            // Reading across an unfinished admission is terminal, so no new
            // receive may hide the error or reuse its attachment boundary.
            assert_eq!(ready_ok(reader.poll_read(&mut cx(), &mut [0; 1])), 0);
            assert_eq!(state.reads.get(), 0);
        }
    }
}

#[test]
fn incoming_segment_bytes_and_capability_indices_are_preserved_without_traversal() {
    let (connection, state) = fixture();
    assert_eq!(connection.id(), 23);
    assert_eq!(*connection.peer(), 7);
    // A raw capability pointer, empty middle segment, and unreachable tail.
    // get_body panics, so decoding or rebuilding the root cannot pass this test.
    let segments = vec![vec![3, 0, 0, 0, 19, 0, 0, 0], vec![], vec![0xa5; 24]];
    let expected = capnp::serialize::write_message_segments_to_words(&segments);
    state.incoming.borrow_mut().push_back(Message {
        segments,
        size: None,
    });
    let (mut reader, _writer, _failure) = connection.into_io(NonZeroUsize::new(256).unwrap());
    for _ in 0..2 {
        assert!(
            matches!(reader.poll_frame_size(&mut cx()), Poll::Ready(Ok(Some(size))) if size == expected.len())
        );
    }
    assert_eq!(
        state.reads.get(),
        1,
        "sizing retains the same received frame"
    );
    let mut output = Vec::new();
    while output.len() < expected.len() {
        let mut bytes = [0; 3];
        let count = ready_ok(reader.poll_read(&mut cx(), &mut bytes));
        output.extend_from_slice(&bytes[..count]);
    }
    assert_eq!(output, expected);
    assert_eq!(state.reads.get(), 1);
    reader.finish_frame();
    assert!(reader.poll_read(&mut cx(), &mut [0; 8]).is_pending());
    assert_eq!(state.reads.get(), 2);
}

#[test]
fn next_input_cannot_prefetch_before_admission_finishes() {
    let (connection, state) = fixture();
    for _ in 0..2 {
        state.incoming.borrow_mut().push_back(Message {
            segments: vec![vec![0; 8]],
            size: None,
        });
    }
    let (mut reader, _writer, _) = connection.into_io(NonZeroUsize::new(64).unwrap());
    assert_eq!(ready_ok(reader.poll_read(&mut cx(), &mut [0; 64])), 8);
    assert_eq!(ready_ok(reader.poll_read(&mut cx(), &mut [0; 64])), 8);
    assert!(matches!(
        reader.poll_read(&mut cx(), &mut [0; 64]),
        Poll::Ready(Err(_))
    ));
    assert_eq!(state.reads.get(), 1);
}

#[test]
fn invalid_segment_shapes_sizes_and_capacity_are_rejected_before_exposing_bytes() {
    for (segments, size, maximum) in [
        (vec![], None, 64),
        (vec![vec![0; 7]], None, 64),
        (vec![vec![0; 8]], Some(0), 64),
        (vec![vec![0; 64]], None, 64),
        (vec![vec![], vec![]], None, 8),
        (vec![vec![]; 512], None, 4096),
    ] {
        let (connection, state) = fixture();
        state
            .incoming
            .borrow_mut()
            .push_back(Message { segments, size });
        let (mut reader, _writer, _) = connection.into_io(NonZeroUsize::new(maximum).unwrap());
        let mut bytes = [0xa5; 8];
        assert!(matches!(
            reader.poll_read(&mut cx(), &mut bytes),
            Poll::Ready(Err(_))
        ));
        assert_eq!(bytes, [0xa5; 8]);
        assert_eq!(ready_ok(reader.poll_read(&mut cx(), &mut bytes)), 0);
        assert_eq!(state.reads.get(), 1);
    }
}

#[test]
fn frame_sizing_rejects_oversize_before_reservation_and_preserves_pending_receive() {
    let (connection, state) = fixture();
    state.incoming.borrow_mut().push_back(Message {
        segments: vec![vec![0; 64]],
        size: None,
    });
    let (mut reader, _writer, _) = connection.into_io(NonZeroUsize::new(64).unwrap());
    assert!(matches!(
        reader.poll_frame_size(&mut cx()),
        Poll::Ready(Err(_))
    ));
    assert_eq!(ready_ok(reader.poll_read(&mut cx(), &mut [0; 64])), 0);
    assert_eq!(state.reads.get(), 1);

    let (connection, state) = fixture();
    let (mut reader, _writer, _) = connection.into_io(NonZeroUsize::new(512 * 1024).unwrap());
    assert!(reader.poll_frame_size(&mut cx()).is_pending());
    assert!(reader.poll_frame_size(&mut cx()).is_pending());
    assert_eq!(state.reads.get(), 1);
    reader.finish_frame();
    assert_eq!(
        state.canceled_reads.get(),
        0,
        "shutdown must drain output first"
    );
    drop(reader);
    assert_eq!(state.canceled_reads.get(), 1);
}

#[test]
fn interrupted_message_receive_stays_alive_until_output_can_drain() {
    let (connection, state) = fixture();
    let (mut reader, mut writer, _) = connection.into_io(NonZeroUsize::new(64).unwrap());
    assert!(reader.poll_read(&mut cx(), &mut [0; 8]).is_pending());
    reader.finish_frame();
    assert_eq!(
        state.canceled_reads.get(),
        0,
        "canceling now can close both transport halves"
    );
    let bytes = frame();
    assert_eq!(
        ready_ok(writer.poll_write_resources(&mut cx(), &bytes, None)),
        bytes.len()
    );
    assert!(writer.poll_close(&mut cx()).is_pending());
    assert!(state.shutdowns.borrow().is_empty());
    state.send.complete(Ok(()));
    assert!(writer.poll_close(&mut cx()).is_pending());
    state.close.complete(Ok(()));
    ready_ok(writer.poll_close(&mut cx()));
    drop(reader);
    assert_eq!(state.canceled_reads.get(), 1);
    drop(writer);
    assert_eq!(state.shutdowns.borrow().len(), 1);
    assert!(state.shutdowns.borrow()[0].is_ok());
}

#[test]
fn output_receipts_apply_backpressure_and_close_is_a_separate_idempotent_fence() {
    let (connection, state) = fixture();
    let (_reader, mut writer, mut failure) = connection.into_io(NonZeroUsize::new(64).unwrap());
    let bytes = frame();
    for _ in 0..MAXIMUM_RECEIPTS {
        ready_ok(writer.poll_write_resources(&mut cx(), &bytes, None));
    }
    assert!(writer
        .poll_write_resources(&mut cx(), &bytes, None)
        .is_pending());
    assert!(writer.poll_flush(&mut cx()).is_pending());
    assert_eq!(state.sends.borrow().len(), MAXIMUM_RECEIPTS);
    state.send.complete(Ok(()));
    ready_ok(writer.poll_write_resources(&mut cx(), &bytes, None));
    assert_eq!(
        state.sends.borrow().as_slice(),
        vec![bytes; MAXIMUM_RECEIPTS + 1]
    );
    assert!(writer.poll_close(&mut cx()).is_pending());
    state.finished.complete(Ok(()));
    assert!(
        Pin::new(&mut failure).poll(&mut cx()).is_pending(),
        "normal drain is not an abort"
    );
    state.close.complete(Ok(()));
    ready_ok(writer.poll_close(&mut cx()));
    ready_ok(writer.poll_close(&mut cx()));
    assert_eq!(state.shutdowns.borrow().len(), 1);
    assert!(matches!(
        writer.poll_write_resources(&mut cx(), &frame(), None),
        Poll::Ready(Err(_))
    ));
}

#[test]
fn allocating_output_shares_bytes_and_pins_pool_until_transport_releases_frame() {
    let (connection, state) = fixture();
    state.retain_frames.set(true);
    let (_reader, mut writer, _) = connection.into_io(NonZeroUsize::new(64).unwrap());
    let bytes = frame();
    let mut pool = messages::GrowingBuffers::new(1, bytes.len(), bytes.len() + 7);
    messages::with_growing_messages(
        &mut pool,
        capntproto_rpc_engine::connection::Growth {
            initial: 0,
            maximum: 1,
        },
        |messages| {
            let reserved = messages.reserve(bytes.len()).unwrap();
            let pointer = {
                let mut buffer = messages.write(&reserved);
                buffer.copy_from_slice(&bytes);
                buffer.as_ptr()
            };
            let lease = messages.seal(reserved, bytes.len()).unwrap();
            let offer = messages.offer(&lease).unwrap();
            let mut queue = messages::Queue::<Bundle>::growing_with_resources(0, 1).unwrap();
            queue.reserve().unwrap().accept(messages, offer);
            messages.release(lease);
            let mut output = io::Output::new();
            assert!(output
                .poll(
                    &mut writer,
                    &mut queue,
                    messages,
                    &mut cx(),
                    NonZeroUsize::new(8).unwrap()
                )
                .is_pending());
            assert!(queue.is_empty());
            let segment = state.frames.borrow()[0].segments()[0].as_ptr();
            assert_eq!(
                segment,
                pointer.wrapping_add(8),
                "transport must borrow the original segment"
            );
            assert_eq!(messages.reserve(1).unwrap_err(), messages::Error::NoBuffer);
            state.send.complete(Ok(()));
            assert!(matches!(
                output.poll(
                    &mut writer,
                    &mut queue,
                    messages,
                    &mut cx(),
                    NonZeroUsize::new(8).unwrap(),
                ),
                Poll::Ready(Ok(()))
            ));
            assert_eq!(messages.reserve(1).unwrap_err(), messages::Error::NoBuffer);
            assert_eq!(state.sends.borrow()[0], bytes);
            state.frames.borrow_mut().clear();
            let reused = messages.reserve(bytes.len()).unwrap();
            assert!(messages.write(&reused).iter().all(|byte| *byte == 0));
            messages.discard(reused);
        },
    )
    .unwrap();
}

#[test]
fn receipt_failure_is_sticky_and_reaches_independent_observer_while_input_is_parked() {
    let (connection, state) = fixture();
    let (mut reader, mut writer, mut failure) = connection.into_io(NonZeroUsize::new(64).unwrap());
    assert!(reader.poll_read(&mut cx(), &mut [0; 8]).is_pending());
    assert!(Pin::new(&mut failure).poll(&mut cx()).is_pending());
    ready_ok(writer.poll_write_resources(&mut cx(), &frame(), None));
    state
        .send
        .complete(Err(Error::overloaded("injected queue error".into())));
    state
        .finished
        .complete(Err(Error::overloaded("injected writer error".into())));
    let Poll::Ready(error) = Pin::new(&mut failure).poll(&mut cx()) else {
        panic!("failure")
    };
    assert_eq!(error.extra, "injected writer error");
    for _ in 0..2 {
        let Poll::Ready(Err(IoError::Failed(error))) = writer.poll_flush(&mut cx()) else {
            panic!("receipt")
        };
        assert_eq!(error.extra, "injected queue error");
    }
    drop(writer);
    assert_eq!(state.shutdowns.borrow().len(), 1);
    assert_eq!(
        state.shutdowns.borrow()[0].as_ref().unwrap_err().kind,
        capnp::ErrorKind::Overloaded
    );
}

#[test]
fn unexpectedly_finished_writer_disconnects_but_empty_input_reads_do_not_start_receiving() {
    let (connection, state) = fixture();
    let (mut reader, _writer, mut failure) = connection.into_io(NonZeroUsize::new(64).unwrap());
    assert_eq!(ready_ok(reader.poll_read(&mut cx(), &mut [])), 0);
    assert_eq!(state.reads.get(), 0);
    state.finished.complete(Ok(()));
    let Poll::Ready(error) = Pin::new(&mut failure).poll(&mut cx()) else {
        panic!("closed")
    };
    assert_eq!(error.kind, capnp::ErrorKind::Disconnected);
}

#[test]
fn outgoing_frames_preserve_raw_segments_and_reject_truncation_or_concatenation() {
    let segments = vec![
        vec![3, 0, 0, 0, 0xff, 0xff, 0xff, 0x7f],
        vec![],
        vec![0xa5; 32],
    ];
    let bytes = capnp::serialize::write_message_segments_to_words(&segments);
    let frame = crate::OutgoingFrame::copy_from(&bytes).unwrap();
    assert_eq!(frame.size_in_words(), 5);
    assert_eq!(
        capnp::serialize::write_message_segments_to_words(&frame.segments()),
        bytes
    );
    for offset in 0..8 {
        let mut unaligned = vec![0; bytes.len() + offset];
        unaligned[offset..].copy_from_slice(&bytes);
        let copied = crate::OutgoingFrame::copy_from(&unaligned[offset..]).unwrap();
        assert_eq!(
            capnp::serialize::write_message_segments_to_words(&copied.segments()),
            bytes
        );
    }
    for length in 0..bytes.len() {
        assert!(crate::OutgoingFrame::copy_from(&bytes[..length]).is_err());
    }
    let duplicate = [bytes.as_slice(), bytes.as_slice()].concat();
    assert!(crate::OutgoingFrame::copy_from(&duplicate).is_err());
    assert!(crate::OutgoingFrame::copy_from(&[0, 0, 0, 0, 0xff, 0xff, 0xff, 0x7f]).is_err());
}

#[test]
fn call_credit_is_retained_until_physical_receipt_or_abandoned_frame_release() {
    for retain_frame in [false, true] {
        let (connection, state) = fixture();
        state.retain_frames.set(retain_frame);
        let (_reader, mut writer, _) = connection.into_io(NonZeroUsize::new(64).unwrap());
        let admission = crate::admission::Admission::new(1);
        let policy = Resources::default();
        let mut bundle = policy.bundle();
        policy
            .retain_call(&mut bundle, admission.reserve().unwrap())
            .unwrap();
        ready_ok(writer.poll_write_resources(&mut cx(), &frame(), Some(&bundle)));
        drop(bundle);
        assert!(writer.poll_flush(&mut cx()).is_pending());
        assert!(admission.reserve().is_err());
        if retain_frame {
            drop(writer); // Receipt observer gone; transport still owns actual bytes.
            assert_eq!(admission.used(), 1);
            state.frames.borrow_mut().clear();
        } else {
            state.send.complete(Ok(()));
            ready_ok(writer.poll_flush(&mut cx()));
        }
        assert_eq!(admission.used(), 0);
        assert!(admission.reserve().is_ok());
    }
}
