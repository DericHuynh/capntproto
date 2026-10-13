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
fn detached_drive_writes_immediately_but_keeps_ownership_until_flush() {
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
