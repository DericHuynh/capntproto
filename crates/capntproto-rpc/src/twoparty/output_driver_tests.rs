use super::*;
use crate::Connection as _;
use std::{
    cell::Cell,
    pin::Pin,
    task::{Context, Poll, Waker},
};

#[derive(Default)]
struct Output {
    bytes: RefCell<Vec<u8>>,
    allowed: Cell<usize>,
    flushed: Cell<bool>,
    fail: Cell<bool>,
    wake: RefCell<Option<Waker>>,
}
struct Writer(Rc<Output>);
impl AsyncWrite for Writer {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if self.0.fail.get() {
            return Poll::Ready(Err(std::io::ErrorKind::BrokenPipe.into()));
        }
        let count = self.0.allowed.get().min(bytes.len());
        if count == 0 {
            *self.0.wake.borrow_mut() = Some(cx.waker().clone());
            return Poll::Pending;
        }
        self.0.bytes.borrow_mut().extend_from_slice(&bytes[..count]);
        self.0.allowed.set(self.0.allowed.get() - count);
        Poll::Ready(Ok(count))
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        if self.0.flushed.get() {
            Poll::Ready(Ok(()))
        } else {
            *self.0.wake.borrow_mut() = Some(cx.waker().clone());
            Poll::Pending
        }
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
fn frame(text: &str) -> (crate::OutgoingFrame, Vec<u8>) {
    let mut message = capnp::message::Builder::new_default();
    message.set_root(text).unwrap();
    let bytes = capnp::serialize::write_message_to_words(&message);
    (crate::OutgoingFrame::copy_from(&bytes).unwrap(), bytes)
}

#[test]
fn optional_stream_methods_preserve_legacy_transport_behavior() {
    use crate::{IncomingFrameStream as _, IncomingMessage as _, OutgoingMessageStream as _};
    struct Input(Vec<u8>);
    impl crate::IncomingFrameStream for Input {
        fn poll_frame(&mut self, _: &mut Context<'_>) -> Poll<capnp::Result<Option<&[u8]>>> {
            Poll::Ready(Ok(self.frame()))
        }
        fn frame(&self) -> Option<&[u8]> {
            (!self.0.is_empty()).then_some(self.0.as_slice())
        }
        fn finish_frame(&mut self) {
            self.0.clear();
        }
    }
    struct Message(capnp::message::Reader<capnp::serialize::OwnedSegments>);
    impl crate::IncomingMessage for Message {
        fn size_in_words(&self) -> usize {
            self.0.get_segments().as_ref().len() / 8
        }
        fn get_body(&self) -> capnp::Result<capnp::any_pointer::Reader<'_>> {
            self.0.get_root()
        }
    }
    #[derive(Default)]
    struct Sink(Vec<crate::OutgoingFrame>);
    impl crate::OutgoingMessageStream for Sink {
        fn send(&mut self, frame: crate::OutgoingFrame) -> capnp::Result<()> {
            self.0.push(frame);
            Ok(())
        }
        fn poll_flush(&mut self, _: &mut Context<'_>) -> Poll<capnp::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }
    let (outgoing, bytes) = frame("legacy transport");
    let mut input = Input(bytes.clone());
    let mut cx = Context::from_waker(Waker::noop());
    assert!(!input.has_buffered_next_frame());
    assert!(input.share_frame().is_none());
    assert_eq!(
        input.poll_frame_size(&mut cx).map(|r| r.unwrap()),
        Poll::Ready(Some(bytes.len()))
    );
    assert!(
        matches!(input.poll_read_frame(&mut cx, &mut [0; 8]), Poll::Ready(Err(error)) if error.kind == capnp::ErrorKind::Unimplemented)
    );
    assert_eq!(input.frame(), Some(bytes.as_slice()));
    input.finish_frame();
    assert_eq!(
        input.poll_frame_size(&mut cx).map(|r| r.unwrap()),
        Poll::Ready(None)
    );
    let message =
        Message(capnp::serialize::read_message(bytes.as_slice(), Default::default()).unwrap());
    assert!(message.get_segments().is_none());
    assert!(message.size_in_words() > 0);
    assert_eq!(
        message
            .get_body()
            .unwrap()
            .get_as::<capnp::text::Reader>()
            .unwrap(),
        "legacy transport"
    );
    let mut sink = Sink::default();
    sink.send_buffered(outgoing).unwrap();
    assert_eq!(sink.0.len(), 1);
    assert!(sink.poll_flush(&mut cx).is_ready());
}

#[test]
fn tail_transfer_requires_explicit_peer_support_on_standard_streams() {
    let mut network = VatNetwork::new(
        futures::io::Cursor::new(Vec::<u8>::new()),
        futures::io::Cursor::new(Vec::<u8>::new()),
        VatId::Client,
        Default::default(),
    );
    let connection = network.connection.as_ref().unwrap();
    assert!(!connection.supports_tail_call_transfer());
    network.set_tail_call_optimization(true);
    assert!(network
        .connection
        .as_ref()
        .unwrap()
        .supports_tail_call_transfer());
    network.set_tail_call_optimization(false);
    assert!(!network
        .connection
        .as_ref()
        .unwrap()
        .supports_tail_call_transfer());
}

#[test]
fn cached_incoming_payload_preserves_kind_storage_and_rejects_foreign_views() {
    use crate::IncomingMessage as _;
    futures::executor::block_on(async {
        for returned in [false, true] {
            for length in [0, 8, 8192] {
                let data = vec![37; length];
                let mut builder = capnp::message::Builder::new(
                    capnp::message::HeapAllocator::new().first_segment_words(16),
                );
                let root = builder.init_root::<crate::rpc_capnp::message::Builder>();
                let payload = if returned {
                    root.init_return().init_results()
                } else {
                    root.init_call().init_params()
                };
                payload
                    .get_content()
                    .set_as::<capnp::data::Owned>(data.as_slice())
                    .unwrap();
                let frame = capnp::serialize::write_message_to_words(&builder);
                let mut input = capnp_futures::BufferedRead::new(
                    futures::io::Cursor::new(frame),
                    Default::default(),
                );
                let incoming = IncomingMessage::new(
                    input
                        .try_read_message(|_| Ok(false))
                        .await
                        .unwrap()
                        .unwrap(),
                );
                let root = incoming
                    .get_body()
                    .unwrap()
                    .get_as::<crate::rpc_capnp::message::Reader>()
                    .unwrap();
                let payload = if returned {
                    let crate::rpc_capnp::message::Return(ret) = root.which().unwrap() else {
                        panic!()
                    };
                    let crate::rpc_capnp::return_::Results(payload) = ret.unwrap().which().unwrap()
                    else {
                        panic!()
                    };
                    payload.unwrap()
                } else {
                    let crate::rpc_capnp::message::Call(call) = root.which().unwrap() else {
                        panic!()
                    };
                    call.unwrap().get_params().unwrap()
                };
                if returned {
                    incoming.cache_results(payload).unwrap();
                } else {
                    incoming.cache_params(payload).unwrap();
                }
                let mut foreign = capnp::message::Builder::new_default();
                let foreign = foreign
                    .init_root::<crate::rpc_capnp::payload::Builder>()
                    .into_reader();
                assert!(incoming.cache_params(foreign).is_err());
                assert!(incoming.cache_results(foreign).is_err());
                drop(input);
                drop(builder);
                for _ in 0..3 {
                    let content = if returned {
                        assert!(incoming.get_params().is_err());
                        incoming.get_results().unwrap()
                    } else {
                        assert!(incoming.get_results().is_err());
                        incoming.get_params().unwrap()
                    };
                    assert_eq!(content.get_as::<capnp::data::Reader>().unwrap(), data);
                }
            }
        }
    });
}

#[test]
fn finish_templates_keep_identity_and_release_flags_independent() {
    let mut network = VatNetwork::new(
        futures::io::Cursor::new(Vec::<u8>::new()),
        futures::io::sink(),
        VatId::Client,
        Default::default(),
    );
    let mut connection = network.connection.take().unwrap();
    let first = connection.new_outgoing_finish(7, false).unwrap();
    let second = connection.new_outgoing_finish(u32::MAX, true).unwrap();
    for (message, id, release) in [(first, 7, false), (second, u32::MAX, true)] {
        let actual = message.take();
        let mut expected = capnp::message::Builder::new_default();
        let mut finish = expected
            .init_root::<crate::rpc_capnp::message::Builder>()
            .init_finish();
        finish.set_question_id(id);
        finish.set_release_result_caps(release);
        finish.set_require_early_cancellation_workaround(false);
        assert_eq!(
            capnp::serialize::write_message_to_words(&actual),
            capnp::serialize::write_message_to_words(&expected)
        );
    }
}

#[test]
fn cached_call_is_invalidated_by_root_mutation_and_failed_selection_preserves_message() {
    let mut network = VatNetwork::new(
        futures::io::Cursor::new(Vec::<u8>::new()),
        futures::io::sink(),
        VatId::Client,
        Default::default(),
    );
    let mut connection = network.connection.take().unwrap();
    let mut message = connection.new_outgoing_call(1).unwrap();
    message.get_call().unwrap().set_question_id(17);
    let mut second = connection.new_outgoing_call(1).unwrap();
    assert_eq!(second.get_call().unwrap().get_question_id(), 0);
    second.get_call().unwrap().set_question_id(99);
    assert_eq!(message.get_call().unwrap().get_question_id(), 17);
    message.set_call_header(0x1234, 7, true, false).unwrap();
    assert_eq!(message.get_call().unwrap().get_question_id(), 17);
    assert_eq!(message.get_call().unwrap().get_interface_id(), 0x1234);
    assert_eq!(message.get_call().unwrap().get_method_id(), 7);
    message
        .get_call()
        .unwrap()
        .get_params()
        .unwrap()
        .get_content()
        .set_as::<capnp::data::Owned>(&[23; 8192][..])
        .unwrap();
    assert_eq!(message.get_call().unwrap().get_question_id(), 17);
    message
        .get_body()
        .unwrap()
        .get_as::<crate::rpc_capnp::message::Builder>()
        .unwrap()
        .init_finish()
        .set_question_id(29);
    assert!(message.get_call().is_err());
    let crate::rpc_capnp::message::Finish(finish) = message
        .get_body_as_reader()
        .unwrap()
        .get_as::<crate::rpc_capnp::message::Reader>()
        .unwrap()
        .which()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(finish.unwrap().get_question_id(), 29);
    message.init_call().unwrap().set_question_id(41);
    assert_eq!(message.get_call().unwrap().get_question_id(), 41);
    let message = message.take();
    let crate::rpc_capnp::message::Call(call) = message
        .get_root_as_reader::<crate::rpc_capnp::message::Reader>()
        .unwrap()
        .which()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(call.unwrap().get_question_id(), 41);
}

#[test]
fn return_templates_preserve_payloads_and_invalidate_selections_after_root_edits() {
    let mut network = VatNetwork::new(
        futures::io::Cursor::new(Vec::<u8>::new()),
        futures::io::sink(),
        VatId::Client,
        Default::default(),
    );
    let mut connection = network.connection.take().unwrap();
    for hint in [1, 256] {
        let mut message = connection.new_outgoing_return(hint).unwrap();
        message.get_return().unwrap().set_answer_id(17);
        message.get_return().unwrap().set_release_param_caps(false);
        let mut second = connection.new_outgoing_return(hint).unwrap();
        assert_eq!(second.get_return().unwrap().get_answer_id(), 0);
        assert!(second.get_return().unwrap().get_release_param_caps());
        second.get_return().unwrap().set_answer_id(u32::MAX);
        let crate::rpc_capnp::return_::Results(payload) =
            message.get_return().unwrap().which().unwrap()
        else {
            panic!()
        };
        payload
            .unwrap()
            .get_content()
            .set_as::<capnp::data::Owned>(&[23; 8192][..])
            .unwrap();
        assert!(message.get_call().is_err());
        assert_eq!(message.get_return().unwrap().get_answer_id(), 17);
        assert!(!message.get_return().unwrap().get_release_param_caps());
        let crate::rpc_capnp::return_::Results(payload) =
            message.get_return().unwrap().which().unwrap()
        else {
            panic!()
        };
        assert_eq!(
            payload
                .unwrap()
                .get_content()
                .get_as::<capnp::data::Builder>()
                .unwrap(),
            &[23; 8192]
        );
        message
            .get_body()
            .unwrap()
            .init_as::<crate::rpc_capnp::message::Builder>()
            .init_call()
            .set_question_id(29);
        assert!(message.get_return().is_err());
        assert_eq!(message.get_call().unwrap().get_question_id(), 29);
        message
            .get_body()
            .unwrap()
            .get_as::<crate::rpc_capnp::message::Builder>()
            .unwrap()
            .init_return()
            .set_answer_id(41);
        assert_eq!(message.get_return().unwrap().get_answer_id(), 41);
        assert_eq!(second.get_return().unwrap().get_answer_id(), u32::MAX);
    }
}

#[test]
fn pipeline_and_finish_batch_with_terminal_calls_on_the_network_poll() {
    let output = Rc::new(Output::default());
    output.allowed.set(usize::MAX);
    output.flushed.set(true);
    let mut network = VatNetwork::new(
        futures::io::Cursor::new(Vec::<u8>::new()),
        Writer(output.clone()),
        VatId::Client,
        Default::default(),
    );
    let mut connection = network.connection.take().unwrap();
    let mut cx = Context::from_waker(Waker::noop());
    assert!(network._output_driver.poll_network(&mut cx).is_pending());
    connection
        .new_outgoing_finish(7, true)
        .unwrap()
        .send_detached_and_drive();
    let mut parent = connection.new_outgoing_call(64).unwrap();
    parent.get_call().unwrap().set_question_id(8);
    parent.send_detached_and_drive();
    assert!(output.bytes.borrow().is_empty());
    assert_eq!(network.outgoing_queue().snapshot().message_count, 2);
    let mut child = connection.new_outgoing_call(64).unwrap();
    child.get_call().unwrap().set_question_id(9);
    child.get_call().unwrap().set_no_promise_pipelining(true);
    child.send_detached_and_drive();
    assert!(output.bytes.borrow().is_empty());
    assert_eq!(network.outgoing_queue().snapshot().message_count, 3);
    assert!(network._output_driver.poll_network(&mut cx).is_pending());
    let bytes = output.bytes.borrow();
    let mut cursor = bytes.as_slice();
    let finish = capnp::serialize::read_message(&mut cursor, Default::default()).unwrap();
    let crate::rpc_capnp::message::Finish(finish) = finish
        .get_root::<crate::rpc_capnp::message::Reader>()
        .unwrap()
        .which()
        .unwrap()
    else {
        panic!("Finish must precede reused questions")
    };
    assert_eq!(finish.unwrap().get_question_id(), 7);
    for id in [8, 9] {
        let message = capnp::serialize::read_message(&mut cursor, Default::default()).unwrap();
        let crate::rpc_capnp::message::Call(call) = message
            .get_root::<crate::rpc_capnp::message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        else {
            panic!("ordered Call")
        };
        assert_eq!(call.unwrap().get_question_id(), id);
    }
    assert!(cursor.is_empty());
    drop(bytes);
    let before = output.bytes.borrow().len();
    connection
        .new_outgoing_call(64)
        .unwrap()
        .send_detached_and_drive();
    assert_eq!(output.bytes.borrow().len(), before);
    assert!(network._output_driver.poll_network(&mut cx).is_pending());
    assert!(
        output.bytes.borrow().len() > before,
        "a parent without a child still progresses"
    );
}

#[test]
fn calls_and_returns_batch_in_fifo_order_and_retain_guards_until_flush() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    #[derive(Default)]
    struct WakeCount(AtomicUsize);
    impl std::task::Wake for WakeCount {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    struct Guard(Rc<Cell<usize>>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let output = Rc::new(Output::default());
    output.allowed.set(usize::MAX);
    output.flushed.set(true);
    let mut network = VatNetwork::new(
        futures::io::Cursor::new(Vec::<u8>::new()),
        Writer(output.clone()),
        VatId::Client,
        Default::default(),
    );
    let mut connection = network.connection.take().unwrap();
    let wakes = Arc::new(WakeCount::default());
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    let drops = Rc::new(Cell::new(0));
    assert!(network._output_driver.poll_network(&mut cx).is_pending());
    for id in 0..32 {
        let mut message = if id % 2 == 0 {
            let mut message = connection.new_outgoing_call(64).unwrap();
            let mut call = message.get_call().unwrap();
            call.set_question_id(id);
            call.set_no_promise_pipelining(true);
            message
        } else {
            let mut message = connection.new_outgoing_return(64).unwrap();
            message.get_return().unwrap().set_answer_id(id);
            message
        };
        assert!(message
            .retain_until_sent(Rc::new(Guard(drops.clone())))
            .is_ok());
        message.send_detached_and_drive();
        assert!(output.bytes.borrow().is_empty());
        assert_eq!(drops.get(), 0, "queued messages retain their guards");
    }
    assert!(wakes.0.load(Ordering::Relaxed) > 0);
    assert_eq!(network.outgoing_queue().snapshot().message_count, 32);
    output.flushed.set(false);
    assert!(network._output_driver.poll_network(&mut cx).is_pending());
    assert_eq!(
        drops.get(),
        0,
        "written bytes do not complete a blocked flush"
    );
    let bytes = output.bytes.borrow();
    let mut cursor = bytes.as_slice();
    for id in 0..32 {
        let message = capnp::serialize::read_message(&mut cursor, Default::default()).unwrap();
        let root = message
            .get_root::<crate::rpc_capnp::message::Reader>()
            .unwrap();
        match root.which().unwrap() {
            crate::rpc_capnp::message::Call(call) if id % 2 == 0 => {
                assert_eq!(call.unwrap().get_question_id(), id);
            }
            crate::rpc_capnp::message::Return(ret) if id % 2 == 1 => {
                assert_eq!(ret.unwrap().get_answer_id(), id);
            }
            _ => panic!("FIFO call/return order"),
        }
    }
    assert!(cursor.is_empty());
    drop(bytes);
    output.flushed.set(true);
    assert!(network._output_driver.poll_network(&mut cx).is_pending());
    assert_eq!(drops.get(), 32);
    let before = output.bytes.borrow().len();
    let mut message = connection.new_outgoing_call(64).unwrap();
    message.get_call().unwrap().set_no_promise_pipelining(true);
    message.send_detached_and_drive();
    assert_eq!(output.bytes.borrow().len(), before);
    assert!(network._output_driver.poll_network(&mut cx).is_pending());
    assert!(output.bytes.borrow().len() > before);
}

#[test]
fn reading_buffered_messages_or_waiting_does_not_flush_queued_output() {
    struct PendingTail(futures::io::Cursor<Vec<u8>>);
    impl AsyncRead for PendingTail {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            bytes: &mut [u8],
        ) -> Poll<std::io::Result<usize>> {
            if self.0.position() as usize == self.0.get_ref().len() {
                Poll::Pending
            } else {
                Pin::new(&mut self.0).poll_read(cx, bytes)
            }
        }
    }
    for owned in [true, false] {
        let mut incoming = Vec::new();
        for id in 0..2 {
            let mut message = capnp::message::Builder::new_default();
            let mut call = message
                .init_root::<crate::rpc_capnp::message::Builder>()
                .init_call();
            call.set_question_id(id);
            call.set_no_promise_pipelining(true);
            incoming.extend(capnp::serialize::write_message_to_words(&message));
        }
        let output = Rc::new(Output::default());
        output.allowed.set(usize::MAX);
        output.flushed.set(true);
        let mut network = VatNetwork::new(
            PendingTail(futures::io::Cursor::new(incoming)),
            Writer(output.clone()),
            VatId::Client,
            Default::default(),
        );
        let mut connection = network.connection.take().unwrap();
        let mut input = if owned {
            connection.take_owned_incoming_stream().unwrap()
        } else {
            connection.take_incoming_stream().unwrap()
        };
        let mut cx = Context::from_waker(Waker::noop());
        assert!(network._output_driver.poll_network(&mut cx).is_pending());
        let send = |connection: &mut dyn crate::Connection<VatId>| {
            let mut call = connection.new_outgoing_call(64).unwrap();
            call.get_call().unwrap().set_no_promise_pipelining(true);
            call.send_detached_and_drive();
        };
        send(&mut connection);
        let first = output.bytes.borrow().len();
        assert_eq!(first, 0);
        for _ in 0..2 {
            let Poll::Ready(Ok(Some(message))) = input.poll_receive(&mut cx) else {
                panic!("both incoming messages must be buffered")
            };
            drop(message);
            send(&mut connection);
            assert_eq!(output.bytes.borrow().len(), first);
        }
        assert_eq!(network.outgoing_queue().snapshot().message_count, 3);
        assert!(input.poll_receive(&mut cx).is_pending());
        send(&mut connection);
        assert_eq!(output.bytes.borrow().len(), first);
        assert_eq!(network.outgoing_queue().snapshot().message_count, 4);
        assert!(network._output_driver.poll_network(&mut cx).is_pending());
        assert!(output.bytes.borrow().len() > first);
        assert_eq!(network.outgoing_queue().snapshot().message_count, 0);
    }
}

#[test]
fn detached_send_queues_until_driven_and_keeps_ownership_until_flush() {
    struct Guard(Rc<Cell<usize>>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let output = Rc::new(Output::default());
    output.allowed.set(usize::MAX);
    let mut network = VatNetwork::new(
        futures::io::Cursor::new(Vec::<u8>::new()),
        Writer(output.clone()),
        VatId::Client,
        Default::default(),
    );
    let mut connection = network.connection.take().unwrap();
    let drops = Rc::new(Cell::new(0));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(network._output_driver.poll_network(&mut cx).is_pending());
    let mut message = connection.new_outgoing_message(8);
    message
        .get_body()
        .unwrap()
        .set_as::<capnp::text::Owned>("driven message")
        .unwrap();
    assert!(message
        .retain_until_sent(Rc::new(Guard(drops.clone())))
        .is_ok());
    let mut expected = capnp::message::Builder::new_default();
    expected.set_root("driven message").unwrap();
    let expected = capnp::serialize::write_message_to_words(&expected);
    message.send_detached_and_drive();
    assert!(output.bytes.borrow().is_empty());
    assert_eq!(drops.get(), 0);
    assert!(network._output_driver.poll_network(&mut cx).is_pending());
    assert_eq!(*output.bytes.borrow(), expected);
    assert_eq!(
        drops.get(),
        0,
        "writing bytes is not physical flush completion"
    );
    assert_eq!(
        network
            .outgoing_queue()
            .output_snapshot()
            .active
            .message_count,
        1
    );
    output.flushed.set(true);
    if let Some(wake) = output.wake.borrow_mut().take() {
        wake.wake();
    }
    assert!(network._output_driver.poll_network(&mut cx).is_pending());
    assert_eq!(drops.get(), 1);
    assert_eq!(
        network
            .outgoing_queue()
            .output_snapshot()
            .active
            .message_count,
        0
    );
}

#[test]
fn borrowed_frames_hold_exact_input_until_finished_and_end_on_truncation() {
    let (_, first) = frame("first borrowed frame");
    let (_, second) = frame("second borrowed frame");
    let mut bytes = first.clone();
    bytes.extend_from_slice(&second);
    bytes.extend_from_slice(&first[..first.len() - 1]);
    let mut network = VatNetwork::new(
        futures::io::Cursor::new(bytes),
        futures::io::sink(),
        VatId::Client,
        Default::default(),
    );
    let mut connection = network.connection.take().unwrap();
    let mut input = connection.take_incoming_frames().unwrap();
    assert!(connection.take_incoming_frames().is_none());
    assert!(connection.take_incoming_stream().is_none());
    let mut cx = Context::from_waker(Waker::noop());
    for expected in [&first, &second] {
        let Poll::Ready(Ok(Some(received))) = input.poll_frame(&mut cx) else {
            panic!()
        };
        let address = received.as_ptr();
        assert_eq!(received, expected);
        assert_eq!(input.frame().unwrap().as_ptr(), address);
        let Poll::Ready(Ok(Some(repeated))) = input.poll_frame(&mut cx) else {
            panic!()
        };
        assert_eq!(repeated.as_ptr(), address);
        assert_eq!(repeated, expected);
        input.finish_frame();
        assert!(input.frame().is_none());
    }
    assert!(matches!(input.poll_frame(&mut cx), Poll::Ready(Err(_))));
    input.finish_frame();
    assert!(matches!(input.poll_frame(&mut cx), Poll::Ready(Err(_))));
}

#[test]
fn buffered_stream_publication_preserves_pending_flush_resources_and_fifo() {
    let output = Rc::new(Output::default());
    output.allowed.set(usize::MAX);
    let mut network = VatNetwork::new(
        futures::io::Cursor::new(Vec::<u8>::new()),
        Writer(output.clone()),
        VatId::Client,
        Default::default(),
    );
    let mut connection = network.connection.take().unwrap();
    let mut stream = connection.take_outgoing_stream().unwrap();
    let mut cx = Context::from_waker(Waker::noop());
    assert!(network._output_driver.poll_network(&mut cx).is_pending());
    let admission = crate::admission::Admission::new(1);
    let (mut first, mut expected) = frame("buffered first");
    first.credit = Some(admission.reserve().unwrap());
    stream.send_buffered(first).unwrap();
    let (second, bytes) = frame("buffered second");
    expected.extend(bytes);
    stream.send_buffered(second).unwrap();
    assert!(output.bytes.borrow().is_empty());
    assert!(stream.poll_flush(&mut cx).is_pending());
    assert_eq!(*output.bytes.borrow(), expected);
    assert_eq!(admission.used(), 1);
    let (third, bytes) = frame("behind pending physical flush");
    expected.extend(bytes);
    stream.send_buffered(third).unwrap();
    assert!(stream.poll_flush(&mut cx).is_pending());
    assert!(output.bytes.borrow().len() < expected.len());
    drop(stream);
    output.flushed.set(true);
    let mut pump = connection.output_driver().unwrap();
    assert!(Pin::new(&mut pump).poll(&mut cx).is_pending());
    assert_eq!(*output.bytes.borrow(), expected);
    assert_eq!(admission.used(), 0);
    let mut successor = connection.take_outgoing_stream().unwrap();
    assert!(matches!(successor.poll_flush(&mut cx), Poll::Ready(Ok(()))));
}

#[test]
fn reusable_output_sends_first_frame_eagerly_and_flushes_a_following_burst() {
    let output = Rc::new(Output::default());
    output.allowed.set(usize::MAX);
    output.flushed.set(true);
    let mut network = VatNetwork::new(
        futures::io::Cursor::new(Vec::<u8>::new()),
        Writer(output.clone()),
        VatId::Client,
        Default::default(),
    );
    let mut connection = network.connection.take().unwrap();
    let mut stream = connection.take_outgoing_stream().unwrap();
    let mut cx = Context::from_waker(Waker::noop());
    assert!(network._output_driver.poll_network(&mut cx).is_pending());
    let (first, mut expected) = frame("first");
    stream.send(first).unwrap();
    assert_eq!(*output.bytes.borrow(), expected);
    let (second, bytes) = frame("second");
    stream.send(second).unwrap();
    assert_eq!(*output.bytes.borrow(), expected);
    expected.extend(bytes);
    assert!(matches!(stream.poll_flush(&mut cx), Poll::Ready(Ok(()))));
    assert_eq!(*output.bytes.borrow(), expected);
    let (third, bytes) = frame("first after flush");
    stream.send(third).unwrap();
    expected.extend(bytes);
    assert_eq!(*output.bytes.borrow(), expected);
    assert!(matches!(stream.poll_flush(&mut cx), Poll::Ready(Ok(()))));
}

#[test]
fn reusable_output_observes_partial_writes_physical_flush_and_shared_fifo_order() {
    let output = Rc::new(Output::default());
    let mut network = VatNetwork::new(
        futures::io::Cursor::new(Vec::<u8>::new()),
        Writer(output.clone()),
        VatId::Client,
        Default::default(),
    );
    let mut connection = network.connection.take().unwrap();
    let mut stream = connection.take_outgoing_stream().unwrap();
    assert!(connection.take_outgoing_stream().is_none());
    let mut pump = connection.output_driver().unwrap();
    let mut cx = Context::from_waker(Waker::noop());
    let admission = crate::admission::Admission::new(1);
    let (mut first, mut expected) = frame("observed first");
    first.credit = Some(admission.reserve().unwrap());
    let (second, bytes) = frame("legacy receipt");
    expected.extend(bytes);
    let (third, bytes) = frame("observed last");
    expected.extend(bytes);
    stream.send(first).unwrap();
    let mut receipt = connection.send_frame(second);
    stream.send(third).unwrap();
    for _ in 0..expected.len() {
        output.allowed.set(1);
        if let Some(wake) = output.wake.borrow_mut().take() {
            wake.wake();
        }
        assert!(Pin::new(&mut pump).poll(&mut cx).is_pending());
        assert!(stream.poll_flush(&mut cx).is_pending());
        assert!(Pin::new(&mut receipt).poll(&mut cx).is_pending());
        assert_eq!(
            admission.used(),
            1,
            "partial writes cannot return admission credit"
        );
    }
    assert_eq!(*output.bytes.borrow(), expected);
    assert_eq!(admission.used(), 1, "completed writes still await flush");
    output.flushed.set(true);
    if let Some(wake) = output.wake.borrow_mut().take() {
        wake.wake();
    }
    assert!(Pin::new(&mut pump).poll(&mut cx).is_pending());
    assert!(matches!(stream.poll_flush(&mut cx), Poll::Ready(Ok(()))));
    assert_eq!(admission.used(), 0);
    assert!(matches!(
        Pin::new(&mut receipt).poll(&mut cx),
        Poll::Ready(Ok(()))
    ));
    // A new barrier must not reuse the previous completion. Dropping an
    // observer does not cancel its accepted frame or reorder the next owner.
    output.flushed.set(false);
    let (mut fourth, bytes) = frame("dropped observer");
    fourth.credit = Some(admission.reserve().unwrap());
    expected.extend(bytes);
    stream.send(fourth).unwrap();
    assert!(stream.poll_flush(&mut cx).is_pending());
    drop(stream);
    assert_eq!(
        admission.used(),
        1,
        "dropping a flush observer cannot cancel accepted output"
    );
    let mut stream = connection.take_outgoing_stream().unwrap();
    let (fifth, bytes) = frame("replacement observer");
    expected.extend(bytes);
    stream.send(fifth).unwrap();
    output.allowed.set(usize::MAX);
    let _ = Pin::new(&mut pump).poll(&mut cx);
    assert!(stream.poll_flush(&mut cx).is_pending());
    assert_eq!(*output.bytes.borrow(), expected);
    assert_eq!(admission.used(), 1);
    output.fail.set(true);
    output.flushed.set(true);
    if let Some(wake) = output.wake.borrow_mut().take() {
        wake.wake();
    }
    let _ = Pin::new(&mut pump).poll(&mut cx);
    assert!(matches!(stream.poll_flush(&mut cx), Poll::Ready(Ok(()))));
    stream.send(frame("failure").0).unwrap();
    let _ = Pin::new(&mut pump).poll(&mut cx);
    assert!(matches!(stream.poll_flush(&mut cx), Poll::Ready(Err(_))));
}

#[test]
fn cooperative_output_preserves_fifo_partial_writes_and_flush_receipts() {
    let output = Rc::new(Output::default());
    let mut network = VatNetwork::new(
        futures::io::Cursor::new(Vec::<u8>::new()),
        Writer(output.clone()),
        VatId::Client,
        Default::default(),
    );
    let mut connection = network.connection.take().unwrap();
    let mut pump = connection.output_driver().unwrap();
    let mut cx = Context::from_waker(Waker::noop());
    let (first, mut expected) = frame("first");
    let (second, bytes) = frame("second");
    expected.extend_from_slice(&bytes);
    let mut first = connection.send_frame(first);
    let mut second = connection.send_frame(second);
    for _ in 0..expected.len() {
        output.allowed.set(1);
        if let Some(wake) = output.wake.borrow_mut().take() {
            wake.wake();
        }
        assert!(Pin::new(&mut pump).poll(&mut cx).is_pending());
        assert!(Pin::new(&mut first).poll(&mut cx).is_pending());
        assert!(Pin::new(&mut second).poll(&mut cx).is_pending());
    }
    assert_eq!(*output.bytes.borrow(), expected);
    output.flushed.set(true);
    if let Some(wake) = output.wake.borrow_mut().take() {
        wake.wake();
    }
    assert!(Pin::new(&mut pump).poll(&mut cx).is_pending());
    assert!(matches!(
        Pin::new(&mut first).poll(&mut cx),
        Poll::Ready(Ok(()))
    ));
    assert!(matches!(
        Pin::new(&mut second).poll(&mut cx),
        Poll::Ready(Ok(()))
    ));
    // Neither Connection nor its opportunistic observer owns the network task.
    drop(network);
    assert!(matches!(
        Pin::new(&mut pump).poll(&mut cx),
        Poll::Ready(Err(_))
    ));
}

#[test]
fn retained_segments_survive_bytewise_output_and_release_the_pool_charge() {
    use capntproto_rpc_engine::{connection::Growth, messages};
    struct Parts {
        header: Vec<u8>,
        segments: Vec<Vec<capnp::Word>>,
    }
    impl messages::Parts for Parts {
        fn count(&self) -> usize {
            1 + self.segments.len()
        }
        fn part(&self, index: usize) -> &[u8] {
            if index == 0 {
                &self.header
            } else {
                capnp::Word::words_to_bytes(&self.segments[index - 1])
            }
        }
        fn retained_capacity(&self) -> usize {
            self.header.capacity()
                + self
                    .segments
                    .iter()
                    .map(|p| p.capacity() * 8)
                    .sum::<usize>()
        }
    }
    let mut builder =
        capnp::message::Builder::new(capnp::message::HeapAllocator::new().first_segment_words(1));
    builder
        .set_root("far payload retained through every byte and flush")
        .unwrap();
    let expected_first = capnp::serialize::write_message_to_words(&builder);
    let segments = builder.get_segments_for_output();
    assert!(segments.len() > 1);
    let table_len = (segments.len() / 2 + 1) * 8;
    let mut source = Parts {
        header: expected_first[..table_len].to_vec(),
        segments: segments
            .iter()
            .map(|s| {
                let mut words = capnp::Word::allocate_zeroed_vec(s.len() / 8);
                capnp::Word::words_to_bytes_mut(&mut words).copy_from_slice(s);
                words
            })
            .collect(),
    };
    let mut pool = messages::GrowingBuffers::new(1, 1024, 2048);
    // A framed owner must reject a mismatched declared segment extent.
    source.header[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    messages::with_growing_messages(
        &mut pool,
        Growth {
            initial: 0,
            maximum: 0,
        },
        |messages| {
            let bad = messages.retain_parts(Rc::new(source)).unwrap().unwrap();
            assert!(crate::OutgoingFrame::from_parts(bad).is_err());
        },
    )
    .unwrap();
    assert_eq!(pool.retained_bytes(), 0);
    let source = Parts {
        header: expected_first[..table_len].to_vec(),
        segments: segments
            .iter()
            .map(|s| {
                let mut words = capnp::Word::allocate_zeroed_vec(s.len() / 8);
                capnp::Word::words_to_bytes_mut(&mut words).copy_from_slice(s);
                words
            })
            .collect(),
    };
    let first = messages::with_growing_messages(
        &mut pool,
        Growth {
            initial: 0,
            maximum: 0,
        },
        |messages| {
            crate::OutgoingFrame::from_parts(
                messages.retain_parts(Rc::new(source)).unwrap().unwrap(),
            )
            .unwrap()
        },
    )
    .unwrap();
    assert!(pool.retained_bytes() >= expected_first.len());
    let output = Rc::new(Output::default());
    let mut network = VatNetwork::new(
        futures::io::Cursor::new(Vec::<u8>::new()),
        Writer(output.clone()),
        VatId::Client,
        Default::default(),
    );
    let mut connection = network.connection.take().unwrap();
    let mut pump = connection.output_driver().unwrap();
    let (second, second_bytes) = frame("successor");
    let mut expected = expected_first;
    expected.extend_from_slice(&second_bytes);
    let mut first_receipt = connection.send_frame(first);
    let mut second_receipt = connection.send_frame(second);
    let mut cx = Context::from_waker(Waker::noop());
    for index in 0..expected.len() {
        output.allowed.set(1);
        if let Some(wake) = output.wake.borrow_mut().take() {
            wake.wake();
        }
        assert!(Pin::new(&mut pump).poll(&mut cx).is_pending());
        assert!(Pin::new(&mut first_receipt).poll(&mut cx).is_pending());
        assert!(Pin::new(&mut second_receipt).poll(&mut cx).is_pending());
        assert_eq!(&*output.bytes.borrow(), &expected[..index + 1]);
    }
    output.flushed.set(true);
    if let Some(wake) = output.wake.borrow_mut().take() {
        wake.wake();
    }
    assert!(Pin::new(&mut pump).poll(&mut cx).is_pending());
    assert!(matches!(
        Pin::new(&mut first_receipt).poll(&mut cx),
        Poll::Ready(Ok(()))
    ));
    assert!(matches!(
        Pin::new(&mut second_receipt).poll(&mut cx),
        Poll::Ready(Ok(()))
    ));
    assert_eq!(pool.retained_bytes(), 0);
    drop(network);
    assert!(matches!(
        Pin::new(&mut pump).poll(&mut cx),
        Poll::Ready(Err(_))
    ));
}

#[test]
fn cooperative_write_failure_reaches_the_existing_failure_observer() {
    let output = Rc::new(Output::default());
    let mut network = VatNetwork::new(
        futures::io::Cursor::new(Vec::<u8>::new()),
        Writer(output.clone()),
        VatId::Client,
        Default::default(),
    );
    let mut connection = network.connection.take().unwrap();
    let mut pump = connection.output_driver().unwrap();
    let mut failed = connection.when_write_finished().unwrap();
    let mut receipt = connection.send_frame(frame("failed").0);
    let mut cx = Context::from_waker(Waker::noop());
    assert!(Pin::new(&mut failed).poll(&mut cx).is_pending());
    output.fail.set(true);
    let _ = Pin::new(&mut pump).poll(&mut cx);
    assert!(matches!(
        Pin::new(&mut failed).poll(&mut cx),
        Poll::Ready(Err(_))
    ));
    assert!(matches!(
        Pin::new(&mut receipt).poll(&mut cx),
        Poll::Ready(Err(_))
    ));
}

#[test]
fn extracted_network_task_keeps_cooperative_output_alive_without_the_factory() {
    use crate::VatNetwork as _;
    let output = Rc::new(Output::default());
    output.allowed.set(usize::MAX);
    output.flushed.set(true);
    let mut network = VatNetwork::new(
        futures::io::Cursor::new(Vec::<u8>::new()),
        Writer(output.clone()),
        VatId::Client,
        Default::default(),
    );
    let mut connection = network.connection.take().unwrap();
    let task = network.drive_until_shutdown();
    let mut pump = connection.output_driver().unwrap();
    drop(network);
    let (frame, expected) = frame("task owns progress");
    let mut receipt = connection.send_frame(frame);
    let mut cx = Context::from_waker(Waker::noop());
    assert!(Pin::new(&mut pump).poll(&mut cx).is_pending());
    assert!(matches!(
        Pin::new(&mut receipt).poll(&mut cx),
        Poll::Ready(Ok(()))
    ));
    assert_eq!(*output.bytes.borrow(), expected);
    drop(task);
    assert!(matches!(
        Pin::new(&mut pump).poll(&mut cx),
        Poll::Ready(Err(_))
    ));
}
