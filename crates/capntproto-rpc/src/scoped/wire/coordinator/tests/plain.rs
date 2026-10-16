use super::*;
use capntproto_rpc_engine::ids::QuestionId;
use capntproto_rpc_wire::{attachments::ImportReceiver, calls};

#[test]
fn outgoing_bursts_flush_under_pool_pressure_and_pending_receipts() {
    #[derive(Default)]
    struct OutputBytes {
        bytes: Vec<u8>,
        flushes: usize,
        waiting: bool,
        acknowledged: bool,
        last_flush: usize,
        largest_batch: usize,
    }
    impl io::Flush for OutputBytes {
        type Error = ();
        fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), io::IoError<()>>> {
            if !self.waiting {
                self.waiting = true;
                self.acknowledged = false;
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            if !self.acknowledged {
                return Poll::Pending;
            }
            self.waiting = false;
            self.flushes += 1;
            let mut batch = &self.bytes[self.last_flush..];
            let mut frames = 0;
            while !batch.is_empty() {
                capnp::serialize::read_message(&mut batch, Default::default()).unwrap();
                frames += 1;
            }
            self.largest_batch = self.largest_batch.max(frames);
            self.last_flush = self.bytes.len();
            Poll::Ready(Ok(()))
        }
    }
    impl io::Write for OutputBytes {
        fn poll_write(
            &mut self,
            _: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<Result<usize, io::IoError<()>>> {
            assert!(!self.waiting, "a physical flush still owns earlier output");
            self.bytes.extend_from_slice(bytes);
            Poll::Ready(Ok(bytes.len()))
        }
    }
    for budget in [1, 8, 64] {
        for capacity in [1, 4, 32] {
            for (owned, occupied) in [(false, 0), (true, 0), (true, 30)] {
                with_peer(|mut driver, messages| {
                    driver.use_owned_output_queues(owned);
                    let mut hooks = ClientHooks::new(32, 32);
                    let client = remote(&mut driver, &mut hooks, ImportId::from_wire(7));
                    let mut opts = options();
                    opts.budget = NonZeroUsize::new(budget).unwrap();
                    let mut coordinator =
                        Coordinator::new(driver, hooks, NonZeroUsize::new(32).unwrap(), None, opts)
                            .unwrap_or_else(|error| {
                                panic!("budget {budget}, capacity {capacity}, owned {owned}, occupied {occupied}: {error:?}")
                            });
                    let holders: Vec<_> = (0..occupied)
                        .map(|_| messages.reserve(1024).unwrap())
                        .collect();
                    let responses: Vec<_> = (0..24)
                        .map(|_| {
                            client
                                .new_call_with_hints::<any_pointer::Owned, any_pointer::Owned>(
                                    7,
                                    0,
                                    None,
                                    capability::CallHints {
                                        no_promise_pipelining: true,
                                        ..Default::default()
                                    },
                                )
                                .send()
                        })
                        .collect();
                    let mut queue = messages::Queue::new_growing(0, capacity).unwrap();
                    let mut bytes = OutputBytes::default();
                    let mut control = Policy {
                        preserve_plain: true,
                        ..Default::default()
                    };
                    let mut cx = Context::from_waker(Waker::noop());
                    for _ in 0..4096 {
                        bytes.acknowledged = true;
                        coordinator
                            .poll_io_work::<(), _, _>(
                                messages,
                                &mut queue,
                                &mut transport::Writer(&mut bytes),
                                &mut [0; 32],
                                &NoResources,
                                &mut control,
                                &mut cx,
                            )
                            .unwrap();
                        if coordinator.active == 0
                            && queue.is_empty()
                            && coordinator.output.is_clean()
                        {
                            break;
                        }
                    }
                    assert!(queue.is_empty() && coordinator.output.is_clean());
                    let mut source = bytes.bytes.as_slice();
                    for id in 0..24 {
                        let message =
                            capnp::serialize::read_message(&mut source, opts.reader).unwrap();
                        let crate::rpc_capnp::message::Call(call) = message
                            .get_root::<crate::rpc_capnp::message::Reader>()
                            .unwrap()
                            .which()
                            .unwrap()
                        else {
                            panic!("Call")
                        };
                        assert_eq!(call.unwrap().get_question_id(), id);
                    }
                    assert!(source.is_empty());
                    if owned && occupied == 0 && budget == 64 && capacity == 32 {
                        assert!(
                            bytes.largest_batch > 8,
                            "burst stopped at a cooperative yield"
                        );
                    }
                    assert!(bytes.largest_batch <= capacity);
                    drop((responses, client));
                    futures::executor::block_on(coordinator.terminate::<NoResources, (), ()>(
                        messages,
                        &mut queue,
                        &mut control,
                        Ok(()),
                    ))
                    .unwrap();
                    for holder in holders {
                        messages.discard(holder);
                    }
                    reclaimed(messages);
                });
            }
        }
    }
}

struct BufferedInput;
impl io::Read for BufferedInput {
    type Error = ();
    fn poll_read(
        &mut self,
        _: &mut Context<'_>,
        _: &mut [u8],
    ) -> Poll<Result<usize, io::IoError<()>>> {
        panic!("test supplies complete frames")
    }
}
impl ImportReceiver<()> for BufferedInput {
    fn make_import(&mut self, _: ImportId) {}
}
impl transport::Input<()> for BufferedInput {
    fn poll_frame_size(&mut self, _: &mut Context<'_>) -> Poll<Result<Option<usize>, ()>> {
        Poll::Pending
    }
    fn finish_frame(&mut self) {}
}

#[derive(Default)]
struct Replies {
    frames: std::collections::VecDeque<Vec<capnp::Word>>,
    consumed: usize,
    stop: Option<Rc<Signal>>,
}
impl io::Read for Replies {
    type Error = ();
    fn poll_read(
        &mut self,
        _: &mut Context<'_>,
        _: &mut [u8],
    ) -> Poll<Result<usize, io::IoError<()>>> {
        panic!("complete buffered replies")
    }
}
impl ImportReceiver<()> for Replies {
    fn make_import(&mut self, _: ImportId) {}
}
impl transport::Input<()> for Replies {
    fn has_buffered_next_frame(&self) -> bool {
        !self.frames.is_empty()
    }
    fn take_buffered_frame<'p>(
        &mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        options: capnp::message::ReaderOptions,
    ) -> Result<Option<Frame<'p>>, capntproto_rpc_wire::owned::Error> {
        Frame::copy_from(messages, self.buffered_frame().unwrap(), options).map(Some)
    }
    fn poll_frame_size(&mut self, _: &mut Context<'_>) -> Poll<Result<Option<usize>, ()>> {
        match self.frames.front() {
            Some(frame) => Poll::Ready(Ok(Some(frame.len() * 8))),
            None => Poll::Pending,
        }
    }
    fn buffered_frame(&self) -> Option<&[u8]> {
        self.frames
            .front()
            .map(|words| capnp::Word::words_to_bytes(words))
    }
    fn finish_frame(&mut self) {
        self.frames.pop_front().unwrap();
        self.consumed += 1;
        if let Some(stop) = &self.stop {
            stop.stop();
        }
    }
}

#[test]
fn buffered_server_bursts_flush_at_budget_end_and_when_input_runs_out() {
    #[derive(Default)]
    struct OutputBytes {
        bytes: Vec<u8>,
        flushes: usize,
    }
    impl io::Flush for OutputBytes {
        type Error = ();
        fn poll_flush(&mut self, _: &mut Context<'_>) -> Poll<Result<(), io::IoError<()>>> {
            self.flushes += 1;
            Poll::Ready(Ok(()))
        }
    }
    impl io::Write for OutputBytes {
        fn poll_write(
            &mut self,
            _: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<Result<usize, io::IoError<()>>> {
            self.bytes.extend_from_slice(bytes);
            Poll::Ready(Ok(bytes.len()))
        }
    }
    for budget in [1, 8, 64] {
        for capacity in [1, 8] {
            with_peer(|mut driver, messages| {
                let seen = Rc::new(Cell::new(0));
                let target = expose(
                    &mut driver,
                    Rc::new(crate::local::Client::new(Echo(seen.clone()))),
                );
                let mut opts = options();
                opts.budget = NonZeroUsize::new(budget).unwrap();
                let mut coordinator = Coordinator::new(
                    driver,
                    ClientHooks::new(16, 16),
                    NonZeroUsize::new(16).unwrap(),
                    None,
                    opts,
                )
                .unwrap();
                let mut queue = messages::Queue::new_growing(0, capacity).unwrap();
                let mut bytes = OutputBytes::default();
                let mut reader = Replies::default();
                let mut control = Policy {
                    preserve_plain: true,
                    ..Default::default()
                };
                let mut pending = None;
                let mut pump = Pump::default();
                let mut cx = Context::from_waker(Waker::noop());
                let application_waker = Waker::from(coordinator.wake.clone());
                let mut stop = std::pin::pin!(std::future::pending());
                // Certify idle before the first arrival, just as a live server
                // waiting for its next peer burst does.
                for _ in 0..256 {
                    coordinator
                        .poll_io_work::<(), _, _>(
                            messages,
                            &mut queue,
                            &mut transport::Writer(&mut bytes),
                            &mut [0; 32],
                            &NoResources,
                            &mut control,
                            &mut cx,
                        )
                        .unwrap();
                    if coordinator.active == 0 {
                        break;
                    }
                }
                for id in 0..12 {
                    let mut message = capnp::message::Builder::new_default();
                    let mut call = message
                        .init_root::<crate::rpc_capnp::message::Builder>()
                        .init_call();
                    call.set_question_id(id);
                    call.set_interface_id(7);
                    call.set_no_promise_pipelining(true);
                    call.reborrow()
                        .init_target()
                        .set_imported_cap(target.to_wire());
                    call.init_params()
                        .init_content()
                        .set_as::<capnp::text::Owned>("batched")
                        .unwrap();
                    let encoded = capnp::serialize::write_message_to_words(&message);
                    let mut words = capnp::Word::allocate_zeroed_vec(encoded.len() / 8);
                    capnp::Word::words_to_bytes_mut(&mut words).copy_from_slice(&encoded);
                    reader.frames.push_back(words);
                }
                for _ in 0..4096 {
                    if let Poll::Ready(result) = coordinator.poll_prepared(
                        messages,
                        &mut queue,
                        &mut reader,
                        &mut transport::Writer(&mut bytes),
                        &mut [0; 32],
                        &NoResources,
                        &mut control,
                        stop.as_mut(),
                        &mut pending,
                        &mut pump,
                        &mut cx,
                        &application_waker,
                    ) {
                        result.unwrap();
                    }
                    if reader.frames.is_empty()
                        && pending.is_none()
                        && queue.is_empty()
                        && coordinator.active == 0
                        && coordinator.output.is_clean()
                    {
                        break;
                    }
                }
                assert_eq!(seen.get(), 12, "budget {budget}, capacity {capacity}");
                assert!(queue.is_empty() && coordinator.output.is_clean());
                if budget == 64 && capacity == 8 {
                    assert!(
                        bytes.flushes < 12,
                        "buffered burst was not coalesced: {}",
                        bytes.flushes
                    );
                }
                let mut source = bytes.bytes.as_slice();
                for id in 0..12 {
                    let reply =
                        capnp::serialize::read_message(&mut source, Default::default()).unwrap();
                    let crate::rpc_capnp::message::Return(ret) = reply
                        .get_root::<crate::rpc_capnp::message::Reader>()
                        .unwrap()
                        .which()
                        .unwrap()
                    else {
                        panic!("Return")
                    };
                    assert_eq!(ret.unwrap().get_answer_id(), id);
                }
                assert!(source.is_empty());
                futures::executor::block_on(coordinator.terminate::<NoResources, (), ()>(
                    messages,
                    &mut queue,
                    &mut control,
                    Ok(()),
                ))
                .unwrap();
                reclaimed(messages);
            });
        }
    }
}

#[test]
fn indexed_reply_batches_respect_frame_budget_and_stop_between_frames() {
    for budget in [1, 4, 8, 64] {
        for stop_after_first in [false, true] {
            with_peer(|mut driver, messages| {
                let mut hooks = ClientHooks::new(16, 16);
                let client = remote(&mut driver, &mut hooks, ImportId::from_wire(7));
                let mut opts = options();
                opts.budget = NonZeroUsize::new(budget).unwrap();
                let mut coordinator =
                    Coordinator::new(driver, hooks, NonZeroUsize::new(16).unwrap(), None, opts)
                        .unwrap();
                let responses: Vec<_> = (0..12)
                    .map(|_| {
                        client
                            .new_call_with_hints::<any_pointer::Owned, any_pointer::Owned>(
                                7,
                                0,
                                None,
                                capability::CallHints {
                                    no_promise_pipelining: true,
                                    ..Default::default()
                                },
                            )
                            .send()
                    })
                    .collect();
                let mut queue = messages::Queue::new_growing(0, 4).unwrap();
                let pipe = Rc::new(RefCell::new(Pipe::default()));
                let mut bytes = Writer(pipe.clone());
                let mut writer = transport::Writer(&mut bytes);
                let mut control = Policy {
                    direct_returns: true,
                    ..Default::default()
                };
                let mut cx = Context::from_waker(Waker::noop());
                let application_waker = Waker::from(coordinator.wake.clone());
                let mut scratch = [0; 32];
                let mut sent = Vec::new();
                for _ in 0..4096 {
                    coordinator
                        .poll_io_work::<(), _, _>(
                            messages,
                            &mut queue,
                            &mut writer,
                            &mut scratch,
                            &NoResources,
                            &mut control,
                            &mut cx,
                        )
                        .unwrap();
                    sent.extend(pipe.borrow_mut().bytes.drain(..));
                    if let Some(wake) = pipe.borrow_mut().write_wake.take() {
                        wake.wake();
                    }
                    if coordinator.active == 0 && queue.is_empty() && coordinator.output.is_clean()
                    {
                        break;
                    }
                }
                let signal = Rc::new(Signal::default());
                let mut reader = Replies {
                    stop: stop_after_first.then(|| signal.clone()),
                    ..Default::default()
                };
                let mut sent = sent.as_slice();
                while !sent.is_empty() {
                    let message = capnp::serialize::read_message(&mut sent, opts.reader).unwrap();
                    let crate::rpc_capnp::message::Call(call) = message
                        .get_root::<crate::rpc_capnp::message::Reader>()
                        .unwrap()
                        .which()
                        .unwrap()
                    else {
                        panic!("Call")
                    };
                    let id = call.unwrap().get_question_id();
                    let mut reply = capnp::message::Builder::new_default();
                    let mut result = reply
                        .init_root::<crate::rpc_capnp::message::Builder>()
                        .init_return();
                    result.set_answer_id(id);
                    result.set_release_param_caps(false);
                    result.set_no_finish_needed(id != 6 && id != 10);
                    result.init_results();
                    let encoded = capnp::serialize::write_message_to_words(&reply);
                    let mut words = capnp::Word::allocate_zeroed_vec(encoded.len() / 8);
                    capnp::Word::words_to_bytes_mut(&mut words).copy_from_slice(&encoded);
                    reader.frames.push_back(words);
                }
                assert_eq!(reader.frames.len(), 12);
                let mut pending = None;
                let mut pump = Pump::default();
                let mut stop = std::pin::pin!(signal.wait());
                let mut stopped = false;
                for _ in 0..4096 {
                    let before = reader.consumed;
                    let control_polls = control.polls;
                    if budget == 1 && before == 1 && !stop_after_first {
                        // A reply that only retires its own question preserves
                        // idle scans, but an independent control wake still
                        // invalidates that certificate before the next frame.
                        assert_eq!(coordinator.active, 0);
                        control.wake.as_ref().unwrap().wake_by_ref();
                    }
                    let result = coordinator.poll_prepared(
                        messages,
                        &mut queue,
                        &mut reader,
                        &mut writer,
                        &mut scratch,
                        &NoResources,
                        &mut control,
                        stop.as_mut(),
                        &mut pending,
                        &mut pump,
                        &mut cx,
                        &application_waker,
                    );
                    if budget == 1 && before == 1 && !stop_after_first {
                        assert!(control.polls > control_polls);
                    }
                    assert!(reader.consumed - before <= budget.min(8));
                    if let Poll::Ready(result) = result {
                        assert_eq!(result.unwrap(), None);
                        stopped = true;
                        break;
                    }
                    if reader.frames.is_empty() {
                        break;
                    }
                }
                if stop_after_first {
                    assert!(stopped);
                    assert_eq!(reader.consumed, 1);
                } else {
                    assert_eq!(reader.consumed, 12);
                    assert!(coordinator.calls.is_empty());
                }
                drop((responses, client));
                futures::executor::block_on(
                    coordinator.terminate::<NoResources, (), &'static str>(
                        messages,
                        &mut queue,
                        &mut control,
                        Ok(()),
                    ),
                )
                .unwrap();
                reclaimed(messages);
            });
        }
    }
}

#[test]
fn plain_calls_preserve_control_wakes_fairness_and_output_backpressure() {
    for budget in [1, 4, 8, 64] {
        for (preserve_plain, notify_receive) in [(false, false), (true, false), (true, true)] {
            with_peer(|mut driver, messages| {
                let seen = Rc::new(Cell::new(0));
                let target = expose(
                    &mut driver,
                    Rc::new(crate::local::Client::new(Echo(seen.clone()))),
                );
                let mut opts = options();
                opts.budget = NonZeroUsize::new(budget).unwrap();
                let mut coordinator = Coordinator::new(
                    driver,
                    ClientHooks::new(16, 16),
                    NonZeroUsize::new(2).unwrap(),
                    None,
                    opts,
                )
                .unwrap();
                let mut queue = messages::Queue::new_growing(0, 1).unwrap();
                let output = Rc::new(RefCell::new(Pipe::default()));
                let mut bytes = Writer(output.clone());
                let mut writer = transport::Writer(&mut bytes);
                let mut reader = BufferedInput;
                let mut control = Policy {
                    preserve_plain,
                    notify_receive,
                    ..Default::default()
                };
                let mut pending = None;
                let mut pump = Pump::default();
                let mut scratch = [0; 32];
                let wake = Arc::new(WakeCount::default());
                let waker = Waker::from(wake.clone());
                let mut cx = Context::from_waker(&waker);
                let mut stop = std::pin::pin!(std::future::pending());
                let application_waker = Waker::from(coordinator.wake.clone());
                macro_rules! poll {
                    () => {
                        if let Poll::Ready(outcome) = coordinator.poll_prepared(
                            messages,
                            &mut queue,
                            &mut reader,
                            &mut writer,
                            &mut scratch,
                            &NoResources,
                            &mut control,
                            stop.as_mut(),
                            &mut pending,
                            &mut pump,
                            &mut cx,
                            &application_waker,
                        ) {
                            outcome.unwrap();
                        }
                    };
                }
                for _ in 0..4096 {
                    poll!();
                    if coordinator.active == 0 && coordinator.control_idle {
                        break;
                    }
                }
                assert_eq!(coordinator.active, 0);
                assert!(coordinator.control_idle);
                let mut payload = capnp::message::Builder::new_default();
                payload.set_root("plain result").unwrap();
                for id in 0..3 {
                    let before = control.polls;
                    let calls_before = seen.get();
                    // Insert an older ordered dispatcher task without an inbox
                    // notification. Idle certificates must inspect live work.
                    if id == 1 {
                        let frame = Frame::build(messages, 512, opts.reader, |builder| {
                            calls::dynamic_call(
                                builder,
                                QuestionId::from_wire(3),
                                calls::Target::Imported(target),
                                calls::DynamicCall {
                                    interface: 7,
                                    no_promise_pipelining: true,
                                    ..Default::default()
                                },
                                |field| {
                                    field.copy_with_capabilities(
                                        payload.get_root_as_reader()?,
                                        capnp::wire_capability::IndexMap::identity(0),
                                    )
                                },
                            )
                        })
                        .unwrap();
                        coordinator.driver.defer_call_dispatch(false);
                        let event = coordinator
                            .driver
                            .receive(frame, messages, &mut scratch, |_| (), server::Results::new)
                            .map_err(|e| e.error)
                            .unwrap();
                        assert!(matches!(event, connection::Received::Queued(_)));
                        coordinator.driver.defer_call_dispatch(true);
                    }
                    pending = Some(
                        Frame::build(messages, 512, opts.reader, |builder| {
                            calls::dynamic_call(
                                builder,
                                QuestionId::from_wire(id),
                                calls::Target::Imported(target),
                                calls::DynamicCall {
                                    interface: 7,
                                    no_promise_pipelining: true,
                                    ..Default::default()
                                },
                                |field| {
                                    field.copy_with_capabilities(
                                        payload.get_root_as_reader()?,
                                        capnp::wire_capability::IndexMap::identity(0),
                                    )
                                },
                            )
                        })
                        .unwrap(),
                    );
                    // Publication may succeed while its physical flush remains blocked.
                    output.borrow_mut().block_flush = true;
                    poll!();
                    assert!(!coordinator.idle || seen.get() == calls_before);
                    output.borrow_mut().block_flush = false;
                    if let Some(wake) = output.borrow_mut().write_wake.take() {
                        wake.wake();
                    }
                    let mut received = Vec::new();
                    for _ in 0..4096 {
                        poll!();
                        received.extend(output.borrow_mut().bytes.drain(..));
                        if pending.is_none()
                            && coordinator.active == 0
                            && coordinator.output.is_clean()
                        {
                            break;
                        }
                    }
                    assert_eq!(
                        seen.get(),
                        calls_before + if id == 1 { 2 } else { 1 },
                        "budget {budget}"
                    );
                    assert!(pending.is_none() && coordinator.worker.is_empty());
                    let mut frame = received.as_slice();
                    for expected in if id == 1 { vec![3, 1] } else { vec![id] } {
                        // The fragmented writer accumulates bytes, not aligned
                        // words. The stream reader supplies aligned ownership.
                        let message =
                            capnp::serialize::read_message(&mut frame, opts.reader).unwrap();
                        let crate::rpc_capnp::message::Return(result) = message
                            .get_root::<crate::rpc_capnp::message::Reader>()
                            .unwrap()
                            .which()
                            .unwrap()
                        else {
                            panic!("Return")
                        };
                        let result = result.unwrap();
                        assert_eq!(result.get_answer_id(), expected);
                        assert!(result.get_no_finish_needed());
                        let crate::rpc_capnp::return_::Results(result) = result.which().unwrap()
                        else {
                            panic!("results")
                        };
                        assert_eq!(
                            result
                                .unwrap()
                                .get_content()
                                .get_as::<capnp::text::Reader>()
                                .unwrap(),
                            "plain result"
                        );
                    }
                    assert!(frame.is_empty(), "exactly one reply per request");
                    if !preserve_plain || notify_receive {
                        assert!(
                            control.polls > before,
                            "control invalidation cannot be hidden"
                        );
                    }
                }
                futures::executor::block_on(
                    coordinator.terminate::<NoResources, (), &'static str>(
                        messages,
                        &mut queue,
                        &mut control,
                        Ok(()),
                    ),
                )
                .unwrap();
                reclaimed(messages);
            });
        }
    }
}

#[test]
#[cfg(all(unix, not(miri)))]
fn general_notification_refreshes_late_metadata_before_enqueue_only_admission() {
    use capntproto_rpc_engine::imports::Imports;
    use capntproto_rpc_wire::clients::Route;
    use std::os::fd::OwnedFd;
    struct Metadata {
        fd: RefCell<Option<Rc<OwnedFd>>>,
        queries: Cell<usize>,
    }
    impl Output<()> for Metadata {
        type Bundle = ();
        fn bundle(&self) {}
        fn attach<'id, P: Copy, const H: usize>(
            &self,
            _: &Route<'id, P, Rc<dyn ClientHook>, H>,
            _: &Imports<'id, '_, ()>,
            _: &mut (),
        ) -> u8 {
            u8::MAX
        }
        fn metadata(&self, _: &()) -> EndpointMetadata {
            self.queries.set(self.queries.get() + 1);
            EndpointMetadata {
                fd: self.fd.borrow().clone(),
                ..Default::default()
            }
        }
    }
    for notify_first in [false, true] {
        with_peer(|mut driver, messages| {
            let mut hooks = ClientHooks::new(8, 8);
            let client = remote(&mut driver, &mut hooks, ImportId::from_wire(7));
            let mut opts = options();
            opts.budget = NonZeroUsize::new(64).unwrap();
            let mut coordinator =
                Coordinator::new(driver, hooks, NonZeroUsize::new(4).unwrap(), None, opts).unwrap();
            let mut queue = messages::Queue::new_growing(0, 4).unwrap();
            let output = Metadata {
                fd: RefCell::new(None),
                queries: Cell::new(0),
            };
            let mut cx = Context::from_waker(Waker::noop());
            for _ in 0..256 {
                if let Poll::Ready(result) =
                    coordinator.poll_work(messages, &mut queue, &mut [0; 32], &output, &mut cx)
                {
                    result.unwrap();
                }
                if coordinator.active == 0 {
                    break;
                }
            }
            assert_eq!(coordinator.active, 0);
            assert!(client.hook.get_fd().is_none());
            let previous = output.queries.get();
            let fd = Rc::new(OwnedFd::from(std::fs::File::open("/dev/null").unwrap()));
            *output.fd.borrow_mut() = Some(fd.clone());
            let wake = Waker::from(coordinator.wake.clone());
            if notify_first {
                wake.wake_by_ref();
            }
            let request = client
                .new_call_with_hints::<any_pointer::Owned, any_pointer::Owned>(
                    7,
                    0,
                    None,
                    capability::CallHints {
                        no_promise_pipelining: true,
                        ..Default::default()
                    },
                )
                .send();
            if !notify_first {
                wake.wake_by_ref();
            }
            for _ in 0..256 {
                if let Poll::Ready(result) =
                    coordinator.poll_work(messages, &mut queue, &mut [0; 32], &output, &mut cx)
                {
                    result.unwrap();
                }
                if !queue.is_empty() {
                    break;
                }
            }
            assert_eq!(queue.len(), 1);
            assert!(output.queries.get() > previous);
            assert!(Rc::ptr_eq(
                &client.hook.get_fd().expect("late metadata"),
                &fd
            ));
            drop(request);
            drop(client);
            futures::executor::block_on(coordinator.terminate::<NoResources, (), ()>(
                messages,
                &mut queue,
                &mut Policy::default(),
                Ok(()),
            ))
            .unwrap();
            reclaimed(messages);
        });
    }
}
