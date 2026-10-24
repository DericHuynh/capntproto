use super::*;
use capntproto_rpc_engine::ids::QuestionId;
use capntproto_rpc_wire::{attachments::ImportReceiver, calls};

#[test]
fn large_server_reply_retains_the_application_payload_until_flush() {
    #[derive(Clone)]
    struct LargeReply(Rc<Cell<*const u8>>);
    impl capability::Server for LargeReply {
        fn as_ptr(&self) -> usize {
            Rc::as_ptr(&self.0) as usize
        }
        fn dispatch_call(
            self,
            _: u64,
            _: u16,
            context: capability::CallContext<'_, any_pointer::Owned, any_pointer::Owned>,
        ) -> capability::DispatchCallResult {
            let (_, mut results) = context.into_owned();
            results
                .get()
                .set_as::<capnp::data::Owned>(&[0xa5; 8192][..])
                .unwrap();
            self.0.set(
                results
                    .get()
                    .get_as::<capnp::data::Builder>()
                    .unwrap()
                    .as_ptr(),
            );
            capability::DispatchCallResult {
                promise: Promise::ok(()),
                is_streaming: false,
                allow_cancellation: true,
            }
        }
    }
    struct Output {
        original: Rc<Cell<*const u8>>,
        bytes: Vec<u8>,
        retained: Option<messages::SharedParts>,
        accepted: usize,
        write_waited: bool,
        flush_waited: bool,
    }
    impl io::Flush for Output {
        type Error = ();
        fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), io::IoError<()>>> {
            if !self.flush_waited {
                self.flush_waited = true;
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            self.retained.take();
            Poll::Ready(Ok(()))
        }
    }
    impl io::WriteResources<()> for Output {
        fn accepts_parts(&self) -> bool {
            true
        }
        fn poll_write_parts(
            &mut self,
            cx: &mut Context<'_>,
            parts: messages::SharedParts,
            resources: Option<&()>,
        ) -> Poll<Result<usize, io::IoError<()>>> {
            assert!(resources.is_some());
            assert!((0..parts.count()).any(|i| parts
                .part(i)
                .windows(8192)
                .any(|bytes| bytes.as_ptr() == self.original.get())));
            if !self.write_waited {
                self.write_waited = true;
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            let length = parts.len();
            for i in 0..parts.count() {
                self.bytes.extend_from_slice(parts.part(i));
            }
            self.accepted += 1;
            self.retained = Some(parts);
            Poll::Ready(Ok(length))
        }
        fn poll_write_resources(
            &mut self,
            _: &mut Context<'_>,
            bytes: &[u8],
            _: Option<&()>,
        ) -> Poll<Result<usize, io::IoError<()>>> {
            self.bytes.extend_from_slice(bytes);
            Poll::Ready(Ok(bytes.len()))
        }
    }
    for owned in [false, true] {
        for budget in [1, 64] {
            with_peer(|mut driver, messages| {
                driver.use_owned_output_queues(owned);
                let original = Rc::new(Cell::new(core::ptr::null()));
                let target = expose(
                    &mut driver,
                    Rc::new(crate::local::Client::new(LargeReply(original.clone()))),
                );
                let mut opts = options();
                opts.budget = NonZeroUsize::new(budget).unwrap();
                let mut coordinator = Coordinator::new(
                    driver,
                    ClientHooks::new(8, 8),
                    NonZeroUsize::new(8).unwrap(),
                    None,
                    opts,
                )
                .unwrap();
                let mut queue = messages::Queue::new_growing(0, 1).unwrap();
                let mut output = Output {
                    original,
                    bytes: Vec::new(),
                    retained: None,
                    accepted: 0,
                    write_waited: false,
                    flush_waited: false,
                };
                let mut control = Policy {
                    preserve_plain: true,
                    ..Default::default()
                };
                let mut pending = Some(
                    Frame::build(messages, 512, opts.reader, |builder| {
                        calls::dynamic_call(
                            builder,
                            QuestionId::from_wire(17),
                            calls::Target::Imported(target),
                            calls::DynamicCall {
                                interface: 7,
                                no_promise_pipelining: true,
                                ..Default::default()
                            },
                            |_| Ok(()),
                        )
                    })
                    .unwrap(),
                );
                let mut pump = Pump::default();
                let mut reader = BufferedInput;
                let mut stop = std::pin::pin!(std::future::pending());
                let mut cx = Context::from_waker(Waker::noop());
                let wake = coordinator.notifications.waker();
                for _ in 0..4096 {
                    if let Poll::Ready(result) = coordinator.poll_prepared(
                        messages,
                        &mut queue,
                        &mut reader,
                        &mut output,
                        &mut [0; 32],
                        &NoResources,
                        &mut control,
                        stop.as_mut(),
                        &mut pending,
                        &mut pump,
                        &mut cx,
                        &wake,
                    ) {
                        result.unwrap();
                    }
                    if pending.is_none()
                        && coordinator.schedule.active == WorkSet::NONE
                        && coordinator.worker.is_empty()
                        && queue.is_empty()
                        && coordinator.output.is_clean()
                    {
                        break;
                    }
                }
                // A one-step quantum separates collection from publication;
                // its existing staged copy path keeps that fairness boundary.
                assert_eq!(
                    output.accepted,
                    usize::from(owned && budget > 1),
                    "owned {owned}, budget {budget}"
                );
                assert!(output.retained.is_none() && output.flush_waited);
                assert!(
                    pending.is_none()
                        && coordinator.worker.is_empty()
                        && coordinator.output.is_clean()
                );
                let mut bytes = output.bytes.as_slice();
                let message = capnp::serialize::read_message(&mut bytes, opts.reader).unwrap();
                let crate::rpc_capnp::message::Return(Ok(returned)) = message
                    .get_root::<crate::rpc_capnp::message::Reader>()
                    .unwrap()
                    .which()
                    .unwrap()
                else {
                    panic!("Return")
                };
                assert_eq!(returned.get_answer_id(), 17);
                assert!(returned.get_no_finish_needed());
                assert!(!returned.get_release_param_caps());
                let crate::rpc_capnp::return_::Results(Ok(payload)) = returned.which().unwrap()
                else {
                    panic!("results")
                };
                assert_eq!(
                    payload
                        .get_content()
                        .get_as::<capnp::data::Reader>()
                        .unwrap(),
                    &[0xa5; 8192]
                );
                assert!(bytes.is_empty());
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
fn large_generated_request_retains_its_payload_through_output_pressure() {
    struct Output {
        original: *const u8,
        bytes: Vec<u8>,
        retained: Option<messages::SharedParts>,
        accepted_parts: usize,
        write_waited: bool,
        flush_waited: bool,
    }
    impl io::Flush for Output {
        type Error = ();
        fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), io::IoError<()>>> {
            if !self.flush_waited {
                self.flush_waited = true;
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            self.retained.take();
            Poll::Ready(Ok(()))
        }
    }
    impl io::WriteResources<()> for Output {
        fn accepts_parts(&self) -> bool {
            true
        }
        fn poll_write_parts(
            &mut self,
            cx: &mut Context<'_>,
            parts: messages::SharedParts,
            resources: Option<&()>,
        ) -> Poll<Result<usize, io::IoError<()>>> {
            assert!(resources.is_some());
            // This checks the public request construction and coordinator path,
            // not just the wire encoder: output must retain the original data.
            assert!((0..parts.count()).any(|i| parts
                .part(i)
                .windows(8192)
                .any(|window| window.as_ptr() == self.original)));
            if !self.write_waited {
                self.write_waited = true;
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            let length = parts.len();
            for i in 0..parts.count() {
                self.bytes.extend_from_slice(parts.part(i));
            }
            self.accepted_parts += 1;
            self.retained = Some(parts);
            Poll::Ready(Ok(length))
        }
        fn poll_write_resources(
            &mut self,
            _: &mut Context<'_>,
            bytes: &[u8],
            _: Option<&()>,
        ) -> Poll<Result<usize, io::IoError<()>>> {
            self.bytes.extend_from_slice(bytes);
            Poll::Ready(Ok(bytes.len()))
        }
    }
    for owned in [false, true] {
        for budget in [1, 64] {
            with_peer(|mut driver, messages| {
                driver.use_owned_output_queues(owned);
                let mut hooks = ClientHooks::new(8, 8);
                let client = remote(&mut driver, &mut hooks, ImportId::from_wire(7));
                let mut opts = options();
                opts.budget = NonZeroUsize::new(budget).unwrap();
                let mut coordinator =
                    Coordinator::new(driver, hooks, NonZeroUsize::new(8).unwrap(), None, opts)
                        .unwrap();
                let mut request = client
                    .new_call_with_hints::<any_pointer::Owned, any_pointer::Owned>(
                        7,
                        0,
                        None,
                        capability::CallHints {
                            no_promise_pipelining: true,
                            ..Default::default()
                        },
                    );
                request
                    .get()
                    .set_as::<capnp::data::Owned>(&[0xa5; 8192][..])
                    .unwrap();
                let original = request
                    .get()
                    .get_as::<capnp::data::Builder>()
                    .unwrap()
                    .as_ptr();
                let response = request.send();
                let mut output = Output {
                    original,
                    bytes: Vec::new(),
                    retained: None,
                    accepted_parts: 0,
                    write_waited: false,
                    flush_waited: false,
                };
                let mut queue = messages::Queue::new_growing(0, 1).unwrap();
                let mut control = Policy {
                    preserve_plain: true,
                    ..Default::default()
                };
                let mut cx = Context::from_waker(Waker::noop());
                for _ in 0..512 {
                    coordinator
                        .poll_io_work::<(), _, _>(
                            messages,
                            &mut queue,
                            &mut output,
                            &mut [0; 32],
                            &NoResources,
                            &mut control,
                            &mut cx,
                        )
                        .unwrap();
                    if coordinator.schedule.active == WorkSet::NONE
                        && queue.is_empty()
                        && coordinator.output.is_clean()
                    {
                        break;
                    }
                }
                assert!(queue.is_empty() && coordinator.output.is_clean());
                assert_eq!(output.accepted_parts, usize::from(owned));
                assert!(output.retained.is_none());
                let mut bytes = output.bytes.as_slice();
                let message =
                    capnp::serialize::read_message(&mut bytes, Default::default()).unwrap();
                let crate::rpc_capnp::message::Call(call) = message
                    .get_root::<crate::rpc_capnp::message::Reader>()
                    .unwrap()
                    .which()
                    .unwrap()
                else {
                    panic!("Call")
                };
                let call = call.unwrap();
                assert_eq!(call.get_question_id(), 0);
                assert_eq!(
                    call.get_params()
                        .unwrap()
                        .get_content()
                        .get_as::<capnp::data::Reader>()
                        .unwrap(),
                    &[0xa5; 8192]
                );
                assert!(bytes.is_empty());
                drop((response, client));
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
fn plain_reply_to_an_opaque_sink_preserves_errors_owners_and_reentrant_work() {
    struct Params(capnp::message::Builder<capnp::message::HeapAllocator>);
    impl ParamsHook for Params {
        fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
            self.0.get_root_as_reader()
        }
    }
    struct Sink {
        retained: Rc<RefCell<Option<Rc<dyn capnp::private::capability::ResponseHook>>>>,
        client: capability::Client,
        next: Rc<RefCell<Option<capability::RemotePromise<any_pointer::Owned>>>>,
        reject: bool,
        calls: Rc<Cell<usize>>,
    }
    impl ResultsHook for Sink {
        fn set_pipeline(&mut self) -> capnp::Result<()> {
            unreachable!("plain reply has no pipeline")
        }
        fn set_pipeline_from(
            &mut self,
            _: Box<dyn capnp::private::capability::PipelineHook>,
        ) -> capnp::Result<()> {
            unreachable!("plain reply has no pipeline")
        }
        fn allow_cancellation(&self) {}
        fn tail_call(
            self: Box<Self>,
            _: Box<dyn capnp::private::capability::RequestHook>,
        ) -> Promise<(), Error> {
            unreachable!("plain reply has no tail call")
        }
        fn direct_tail_call(
            self: Box<Self>,
            _: Box<dyn capnp::private::capability::RequestHook>,
        ) -> (
            Promise<(), Error>,
            Box<dyn capnp::private::capability::PipelineHook>,
        ) {
            unreachable!("plain reply has no tail call")
        }
        fn get_with_size_hint(
            &mut self,
            _: Option<capnp::MessageSize>,
        ) -> capnp::Result<any_pointer::Builder<'_>> {
            panic!("an owning response must reach the opaque result sink")
        }
        fn set_response(
            &mut self,
            response: Rc<dyn capnp::private::capability::ResponseHook>,
        ) -> capnp::Result<()> {
            assert_eq!(
                response.get()?.get_as::<capnp::text::Reader>()?,
                "retained reply"
            );
            self.calls.set(self.calls.get() + 1);
            *self.retained.borrow_mut() = Some(response);
            *self.next.borrow_mut() = Some(
                self.client
                    .new_call_with_hints::<any_pointer::Owned, any_pointer::Owned>(
                        7,
                        1,
                        None,
                        capability::CallHints {
                            no_promise_pipelining: true,
                            ..Default::default()
                        },
                    )
                    .send(),
            );
            if self.reject {
                Err(Error::failed("opaque result rejection".into()))
            } else {
                Ok(())
            }
        }
    }
    for reject in [false, true] {
        with_peer(|mut driver, messages| {
            let mut hooks = ClientHooks::new(8, 8);
            let client = remote(&mut driver, &mut hooks, ImportId::from_wire(7));
            let retained = Rc::new(RefCell::new(None));
            let next = Rc::new(RefCell::new(None));
            let calls = Rc::new(Cell::new(0));
            let completion = client.hook.call_with_hints(
                7,
                0,
                Box::new(Params(capnp::message::Builder::new_default())),
                Box::new(Sink {
                    retained: retained.clone(),
                    client: client.clone(),
                    next: next.clone(),
                    reject,
                    calls: calls.clone(),
                }),
                capability::CallHints {
                    no_promise_pipelining: true,
                    ..Default::default()
                },
            );
            let mut coordinator = Coordinator::new(
                driver,
                hooks,
                NonZeroUsize::new(8).unwrap(),
                None,
                options(),
            )
            .unwrap();
            let mut queue = messages::Queue::new_growing(0, 4).unwrap();
            let mut cx = Context::from_waker(Waker::noop());
            for _ in 0..256 {
                if let Poll::Ready(result) =
                    coordinator.poll_work(messages, &mut queue, &mut [0; 32], &NoResources, &mut cx)
                {
                    result.unwrap();
                }
                if coordinator.schedule.active == WorkSet::NONE && !queue.is_empty() {
                    break;
                }
            }
            assert!(!queue.is_empty());
            let bytes = queue.window(messages).unwrap().unwrap().bytes().to_vec();
            let mut source = bytes.as_slice();
            let sent = capnp::serialize::read_message(&mut source, Default::default()).unwrap();
            let crate::rpc_capnp::message::Call(call) = sent
                .get_root::<crate::rpc_capnp::message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            else {
                panic!("Call")
            };
            let id = call.unwrap().get_question_id();
            let mut reply = capnp::message::Builder::new_default();
            {
                let mut result = reply
                    .init_root::<crate::rpc_capnp::message::Builder>()
                    .init_return();
                result.set_answer_id(id);
                result.set_no_finish_needed(true);
                result.set_release_param_caps(false);
                result
                    .init_results()
                    .init_content()
                    .set_as::<capnp::text::Owned>("retained reply")
                    .unwrap();
            }
            let frame = Frame::copy_from(
                messages,
                &capnp::serialize::write_message_to_words(&reply),
                options().reader,
            )
            .unwrap();
            let received = coordinator
                .driver
                .try_receive_plain_owned(frame, messages, |key, owner| {
                    coordinator.calls.plain_receiver(key, owner)
                })
                .unwrap_or_else(|(error, _)| panic!("{error:?}"));
            let connection::PlainReceived::Delivered(index, message) = received else {
                panic!("the opaque sink must use plain admission")
            };
            let mut control = Policy {
                direct_returns: true,
                ..Default::default()
            };
            coordinator.delivered_plain::<NoResources>(index, message, &control);
            assert_eq!(calls.get(), 1);
            assert_eq!(coordinator.hooks.inbox.queue.borrow().len(), 1);
            assert_ne!(coordinator.notifications.pending(), WorkSet::NONE);
            let result = futures::executor::block_on(completion);
            if reject {
                assert_eq!(result.unwrap_err().extra, "opaque result rejection");
            } else {
                result.unwrap();
            }
            drop(next.borrow_mut().take());
            drop(client);
            futures::executor::block_on(coordinator.terminate::<NoResources, (), ()>(
                messages,
                &mut queue,
                &mut control,
                Ok(()),
            ))
            .unwrap();
            assert_eq!(
                retained
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_as::<capnp::text::Reader>()
                    .unwrap(),
                "retained reply"
            );
            drop(retained.borrow_mut().take());
            reclaimed(messages);
        });
    }
}

#[test]
fn owned_transport_can_admit_pipeline_work_while_previous_output_is_flushing() {
    #[derive(Default)]
    struct OwnedOutput(Vec<Vec<u8>>);
    impl io::Flush for OwnedOutput {
        type Error = ();
        fn poll_flush(&mut self, _: &mut Context<'_>) -> Poll<Result<(), io::IoError<()>>> {
            Poll::Pending
        }
    }
    impl io::WriteResources<()> for OwnedOutput {
        fn can_write_while_flushing(&self) -> bool {
            true
        }
        fn poll_write_resources(
            &mut self,
            _: &mut Context<'_>,
            bytes: &[u8],
            _: Option<&()>,
        ) -> Poll<Result<usize, io::IoError<()>>> {
            if self.0.len() == 2 {
                return Poll::Pending;
            }
            self.0.push(bytes.to_vec());
            Poll::Ready(Ok(bytes.len()))
        }
    }
    with_peer(|mut driver, messages| {
        driver.use_owned_output_queues(true);
        let mut hooks = ClientHooks::new(32, 32);
        let client = remote(&mut driver, &mut hooks, ImportId::from_wire(7));
        let mut coordinator = Coordinator::new(
            driver,
            hooks,
            NonZeroUsize::new(32).unwrap(),
            None,
            options(),
        )
        .unwrap();
        let mut queue = messages::Queue::new_growing(0, 4).unwrap();
        let mut bytes = OwnedOutput::default();
        let mut control = Policy::default();
        let mut cx = Context::from_waker(Waker::noop());
        let mut responses = Vec::new();
        for pipeline_only in [false, true] {
            let mut request = client.new_call::<any_pointer::Owned, any_pointer::Owned>(7, 0, None);
            request.hook.set_hints(capability::CallHints {
                only_promise_pipeline: pipeline_only,
                ..Default::default()
            });
            responses.push(request.send());
            // An independent protocol notification must not make publication
            // wait for the earlier frame's physical completion.
            coordinator.changed();
            for _ in 0..128 {
                coordinator
                    .poll_io_work::<(), _, _>(
                        messages,
                        &mut queue,
                        &mut bytes,
                        &mut [0; 32],
                        &NoResources,
                        &mut control,
                        &mut cx,
                    )
                    .unwrap();
                if bytes.0.len() == responses.len() {
                    break;
                }
            }
            assert_eq!(bytes.0.len(), responses.len());
            assert!(!coordinator.output.is_clean());
            assert!(!coordinator.idle);
        }
        for (id, frame) in bytes.0.iter().enumerate() {
            let mut source = frame.as_slice();
            let message = capnp::serialize::read_message(&mut source, options().reader).unwrap();
            let crate::rpc_capnp::message::Call(call) = message
                .get_root::<crate::rpc_capnp::message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            else {
                panic!("Call")
            };
            assert_eq!(call.unwrap().get_question_id(), id as u32);
            assert!(source.is_empty());
        }
        drop((responses, client, bytes));
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
                        .map(|index| {
                            let hints = capability::CallHints {
                                no_promise_pipelining: index % 2 == 0,
                                ..Default::default()
                            };
                            let mut request = if index % 3 == 1 {
                                capability::Request::new(Box::new(crate::local::Request::new(
                                    7,
                                    0,
                                    None,
                                    client.hook.add_ref(),
                                )))
                            } else {
                                client
                                    .new_call::<any_pointer::Owned, any_pointer::Owned>(7, 0, None)
                            };
                            request.hook.set_hints(hints);
                            request.send()
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
                        if coordinator.schedule.active == WorkSet::NONE
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
    progress: Option<Rc<Cell<usize>>>,
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
    fn has_buffered_next_frame(&mut self) -> bool {
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
        if let Some(progress) = &self.progress {
            progress.set(self.consumed);
        }
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
        input: Rc<Cell<usize>>,
        first_flush_input: Option<usize>,
    }
    impl io::Flush for OutputBytes {
        type Error = ();
        fn poll_flush(&mut self, _: &mut Context<'_>) -> Poll<Result<(), io::IoError<()>>> {
            self.flushes += 1;
            self.first_flush_input.get_or_insert(self.input.get());
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
        for capacity in [1, 8, 32] {
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
                let mut reader = Replies {
                    progress: Some(bytes.input.clone()),
                    ..Default::default()
                };
                let mut control = Policy {
                    preserve_plain: true,
                    ..Default::default()
                };
                let mut pending = None;
                let mut pump = Pump::default();
                let mut cx = Context::from_waker(Waker::noop());
                let application_waker = coordinator.notifications.waker();
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
                    if coordinator.schedule.active == WorkSet::NONE {
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
                    call.set_no_promise_pipelining(id % 2 == 0);
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
                // Valid control messages produce no output. They must not
                // extend the final reply batch until all input is consumed.
                for _ in 0..128 {
                    let mut message = capnp::message::Builder::new_default();
                    message
                        .init_root::<crate::rpc_capnp::message::Builder>()
                        .init_unimplemented()
                        .init_resolve()
                        .init_cap()
                        .set_none(());
                    let encoded = capnp::serialize::write_message_to_words(&message);
                    let mut words = capnp::Word::allocate_zeroed_vec(encoded.len() / 8);
                    capnp::Word::words_to_bytes_mut(&mut words).copy_from_slice(&encoded);
                    reader.frames.push_back(words);
                }
                let mut initial = capnp::message::Builder::new_default();
                let mut reply = initial
                    .init_root::<crate::rpc_capnp::message::Builder>()
                    .init_return();
                reply.set_answer_id(999);
                reply.set_no_finish_needed(true);
                reply.init_results().init_content();
                let initial = capnp::serialize::write_message_to_words(&initial);
                let writer = messages.reserve_exact(initial.len()).unwrap();
                messages.try_write(&writer).unwrap()[..initial.len()].copy_from_slice(&initial);
                let lease = messages.seal(writer, initial.len()).unwrap();
                let offer = messages.offer(&lease).unwrap();
                queue.reserve().unwrap().accept(messages, offer);
                messages.release(lease);
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
                        && coordinator.schedule.active == WorkSet::NONE
                        && coordinator.output.is_clean()
                    {
                        break;
                    }
                }
                assert_eq!(seen.get(), 12, "budget {budget}, capacity {capacity}");
                assert!(queue.is_empty() && coordinator.output.is_clean());
                assert_eq!(reader.consumed, 140);
                assert!(bytes.first_flush_input.unwrap() <= 32,
                    "budget {budget}, capacity {capacity}: control input postponed an already queued reply until {}", bytes.first_flush_input.unwrap());
                if budget == 64 && capacity == 8 {
                    assert!(
                        bytes.flushes < 12,
                        "buffered burst was not coalesced: {}",
                        bytes.flushes
                    );
                }
                let mut source = bytes.bytes.as_slice();
                let mut answers = Vec::new();
                for index in 0..13 {
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
                    let id = ret.unwrap().get_answer_id();
                    if index == 0 {
                        assert_eq!(id, 999);
                    } else {
                        answers.push(id);
                    }
                }
                // Independent calls may finish in different worker rotations.
                // Each must return exactly once after the prequeued frame.
                answers.sort_unstable();
                assert_eq!(answers, (0..12).collect::<Vec<_>>());
                assert!(source.is_empty());
                // Completed answers and their exported service can retire
                // while every application lane is idle. Exercise both quiet
                // controls and a notification delivered during receipt; the
                // last service owner must disappear without another Call.
                control.notify_receive = budget == 1;
                let control_polls = control.polls;
                for id in 0..13 {
                    let mut message = capnp::message::Builder::new_default();
                    let root = message.init_root::<crate::rpc_capnp::message::Builder>();
                    if id < 12 {
                        let mut finish = root.init_finish();
                        finish.set_question_id(id);
                        finish.set_release_result_caps(false);
                    } else {
                        let mut release = root.init_release();
                        release.set_id(target.to_wire());
                        release.set_reference_count(1);
                    }
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
                        && coordinator.schedule.active == WorkSet::NONE
                        && queue.is_empty()
                        && coordinator.output.is_clean()
                    {
                        break;
                    }
                }
                assert_eq!(reader.consumed, 153);
                assert_eq!(seen.get(), 12);
                assert_eq!(Rc::strong_count(&seen), 1, "Release retained the service");
                if control.notify_receive {
                    assert!(control.polls > control_polls, "control wake was lost");
                }
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
fn buffered_reply_batches_respect_frame_budget_and_stop_between_frames() {
    for budget in [1, 4, 8, 64] {
        for pipeline in [false, true] {
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
                                        no_promise_pipelining: !pipeline,
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
                    let application_waker = coordinator.notifications.waker();
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
                        if coordinator.schedule.active == WorkSet::NONE
                            && queue.is_empty()
                            && coordinator.output.is_clean()
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
                        let message =
                            capnp::serialize::read_message(&mut sent, opts.reader).unwrap();
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
                        if !pipeline && budget == 1 && before == 1 && !stop_after_first {
                            // A reply that only retires its own question preserves
                            // idle scans, but an independent control wake still
                            // invalidates that certificate before the next frame.
                            assert_eq!(coordinator.schedule.active, WorkSet::NONE);
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
                        if !pipeline && budget == 1 && before == 1 && !stop_after_first {
                            assert!(control.polls > control_polls);
                        }
                        assert!(reader.consumed - before <= budget.min(8));
                        if pipeline && budget >= 4 && before == 0 && !stop_after_first {
                            assert!(
                                reader.consumed > 1,
                                "buffered pipeline replies yielded after one frame"
                            );
                        }
                        if let Poll::Ready(result) = result {
                            assert_eq!(result.unwrap(), None);
                            stopped = true;
                            break;
                        }
                        pipe.borrow_mut().bytes.clear();
                        if let Some(wake) = pipe.borrow_mut().write_wake.take() {
                            wake.wake();
                        }
                        if reader.frames.is_empty() && coordinator.calls.is_empty() {
                            break;
                        }
                    }
                    if stop_after_first {
                        assert!(stopped);
                        assert_eq!(reader.consumed, 1);
                    } else {
                        assert_eq!(reader.consumed, 12);
                        assert!(
                            coordinator.calls.is_empty(),
                            "budget {budget}, pipeline {pipeline}"
                        );
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
}

#[test]
fn capability_replies_rescan_controls_and_flush_under_small_budgets() {
    #[derive(Clone)]
    struct ReturnCap(Rc<dyn ClientHook>, Rc<Cell<usize>>);
    impl capability::Server for ReturnCap {
        fn as_ptr(&self) -> usize {
            Rc::as_ptr(&self.1) as usize
        }
        fn dispatch_call(
            self,
            _: u64,
            _: u16,
            context: capability::CallContext<'_, any_pointer::Owned, any_pointer::Owned>,
        ) -> capability::DispatchCallResult {
            let (_, mut results) = context.into_owned();
            self.1.set(self.1.get() + 1);
            results.get().set_as_capability(self.0.add_ref());
            capability::DispatchCallResult {
                promise: Promise::ok(()),
                is_streaming: false,
                allow_cancellation: true,
            }
        }
    }
    for budget in [1, 8, 64] {
        with_peer(|mut driver, messages| {
            let calls = Rc::new(Cell::new(0));
            let cap: Rc<dyn ClientHook> =
                Rc::new(crate::local::Client::new(Echo(Rc::new(Cell::new(0)))));
            let target = expose(
                &mut driver,
                Rc::new(crate::local::Client::new(ReturnCap(cap, calls.clone()))),
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
            let pipe = Rc::new(RefCell::new(Pipe::default()));
            let mut bytes = Writer(pipe.clone());
            let mut writer = transport::Writer(&mut bytes);
            let mut reader = BufferedInput;
            let mut control = Policy {
                preserve_plain: true,
                ..Default::default()
            };
            let mut pending = None;
            let mut pump = Pump::default();
            let mut scratch = [0; 32];
            let mut stop = std::pin::pin!(std::future::pending());
            let mut cx = Context::from_waker(Waker::noop());
            let wake = coordinator.notifications.waker();
            macro_rules! poll {
                () => {
                    if let Poll::Ready(result) = coordinator.poll_prepared(
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
                        &wake,
                    ) {
                        result.unwrap();
                    }
                };
            }
            for _ in 0..4096 {
                poll!();
                if coordinator.schedule.active == WorkSet::NONE && coordinator.control_idle {
                    break;
                }
            }
            assert!(coordinator.schedule.active == WorkSet::NONE && coordinator.control_idle);
            let before = control.polls;
            pending = Some(
                Frame::build(messages, 512, opts.reader, |builder| {
                    calls::dynamic_call(
                        builder,
                        QuestionId::from_wire(0),
                        calls::Target::Imported(target),
                        calls::DynamicCall {
                            interface: 7,
                            no_promise_pipelining: true,
                            ..Default::default()
                        },
                        |_| Ok(()),
                    )
                })
                .unwrap(),
            );
            pipe.borrow_mut().block_flush = true;
            let mut received = Vec::new();
            for step in 0..4096 {
                if step == 4 {
                    pipe.borrow_mut().block_flush = false;
                    if let Some(wake) = pipe.borrow_mut().write_wake.take() {
                        wake.wake();
                    }
                }
                poll!();
                received.extend(pipe.borrow_mut().bytes.drain(..));
                if step >= 4
                    && pending.is_none()
                    && coordinator.schedule.active == WorkSet::NONE
                    && coordinator.output.is_clean()
                    && coordinator.control_idle
                {
                    break;
                }
            }
            assert_eq!(calls.get(), 1);
            assert!(pending.is_none() && coordinator.worker.is_empty());
            assert!(coordinator.output.is_clean());
            assert!(
                control.polls > before,
                "capability output must remain visible to controls"
            );
            let mut bytes = received.as_slice();
            let message = capnp::serialize::read_message(&mut bytes, opts.reader).unwrap();
            let crate::rpc_capnp::message::Return(result) = message
                .get_root::<crate::rpc_capnp::message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            else {
                panic!("Return")
            };
            let result = result.unwrap();
            assert_eq!(result.get_answer_id(), 0);
            let crate::rpc_capnp::return_::Results(payload) = result.which().unwrap() else {
                panic!("results")
            };
            assert_eq!(payload.unwrap().get_cap_table().unwrap().len(), 1);
            assert!(bytes.is_empty());
            futures::executor::block_on(coordinator.terminate::<NoResources, (), &'static str>(
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
                let remote = remote(
                    &mut coordinator.driver,
                    &mut coordinator.hooks,
                    ImportId::from_wire(88),
                );
                let mut outstanding = None;
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
                let application_waker = coordinator.notifications.waker();
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
                    if coordinator.schedule.active == WorkSet::NONE && coordinator.control_idle {
                        break;
                    }
                }
                assert_eq!(coordinator.schedule.active, WorkSet::NONE);
                assert!(coordinator.control_idle);
                let mut payload = capnp::message::Builder::new_default();
                payload.set_root("plain result").unwrap();
                for id in 0..3 {
                    if id == 2 {
                        outstanding = Some(
                            remote
                                .new_call_with_hints::<any_pointer::Owned, any_pointer::Owned>(
                                    7,
                                    0,
                                    None,
                                    capability::CallHints {
                                        no_promise_pipelining: true,
                                        ..Default::default()
                                    },
                                )
                                .send(),
                        );
                        for _ in 0..4096 {
                            poll!();
                            if coordinator.schedule.active == WorkSet::NONE
                                && coordinator.output.is_clean()
                                && queue.is_empty()
                            {
                                break;
                            }
                        }
                        assert_eq!(coordinator.schedule.active, WorkSet::NONE);
                        assert!(!coordinator.calls.is_empty());
                        let bytes: Vec<_> = output.borrow_mut().bytes.drain(..).collect();
                        let mut source = bytes.as_slice();
                        let message =
                            capnp::serialize::read_message(&mut source, opts.reader).unwrap();
                        assert!(matches!(
                            message
                                .get_root::<crate::rpc_capnp::message::Reader>()
                                .unwrap()
                                .which()
                                .unwrap(),
                            crate::rpc_capnp::message::Call(_)
                        ));
                        assert!(source.is_empty());
                    }
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
                            .receive(frame, messages, &mut scratch, |_| (), server::Producer::new)
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
                            && queue.is_empty()
                            && coordinator.schedule.active == WorkSet::NONE
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
                    if id == 2 {
                        assert!(
                            !coordinator.calls.is_empty(),
                            "a callback reply cannot consume the outstanding request"
                        );
                        assert!(Pin::new(&mut outstanding.as_mut().unwrap().promise)
                            .poll(&mut cx)
                            .is_pending());
                    }
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
                drop((outstanding, remote));
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
        source: Rc<MetadataSource>,
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
        fn metadata_source(&self, _: &()) -> Option<Rc<MetadataSource>> {
            self.queries.set(self.queries.get() + 1);
            Some(self.source.clone())
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
                source: Rc::new(MetadataSource::default()),
                queries: Cell::new(0),
            };
            let mut cx = Context::from_waker(Waker::noop());
            for _ in 0..256 {
                if let Poll::Ready(result) =
                    coordinator.poll_work(messages, &mut queue, &mut [0; 32], &output, &mut cx)
                {
                    result.unwrap();
                }
                if coordinator.schedule.active == WorkSet::NONE {
                    break;
                }
            }
            assert_eq!(coordinator.schedule.active, WorkSet::NONE);
            assert!(client.hook.get_fd().is_none());
            let previous = output.queries.get();
            let fd = Rc::new(OwnedFd::from(std::fs::File::open("/dev/null").unwrap()));
            output
                .source
                .update(|metadata| metadata.fd = Some(fd.clone()));
            let wake = coordinator.notifications.waker();
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
            assert_eq!(
                output.queries.get(),
                previous,
                "revision avoids another projection"
            );
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

#[test]
fn early_call_publication_preserves_budget_for_the_next_request() {
    with_peer(|mut driver, messages| {
        let mut hooks = ClientHooks::new(8, 8);
        let client = remote(&mut driver, &mut hooks, ImportId::from_wire(7));
        let mut coordinator = Coordinator::new(
            driver,
            hooks,
            NonZeroUsize::new(4).unwrap(),
            None,
            options(),
        )
        .unwrap();
        let mut queue = messages::Queue::new_growing(0, 4).unwrap();
        let mut cx = Context::from_waker(Waker::noop());
        for _ in 0..256 {
            if let Poll::Ready(result) =
                coordinator.poll_work(messages, &mut queue, &mut [0; 32], &NoResources, &mut cx)
            {
                result.unwrap();
            }
            if coordinator.schedule.active == WorkSet::NONE {
                break;
            }
        }
        assert_eq!(coordinator.schedule.active, WorkSet::NONE);
        let requests: Vec<_> = (0..2)
            .map(|method| {
                client
                    .new_call_with_hints::<any_pointer::Owned, any_pointer::Owned>(
                        7,
                        method,
                        None,
                        capability::CallHints {
                            no_promise_pipelining: true,
                            ..Default::default()
                        },
                    )
                    .send()
            })
            .collect();
        let wake = coordinator.notifications.waker();
        let mut remaining = 4;
        while remaining != 0 && queue.len() != 2 {
            let before = remaining;
            if let Poll::Ready(result) = coordinator.poll_work_budget(
                messages,
                &mut queue,
                &mut [0; 32],
                &NoResources,
                &mut cx,
                &mut remaining,
                &wake,
            ) {
                result.unwrap();
            }
            assert!(remaining < before, "every active visit consumes work");
        }
        // Admission/publication uses one step. The unused allowance remains
        // available for the rotating idle checks and the second publication.
        assert_eq!(queue.len(), 2);
        assert!(coordinator.hooks.inbox.queue.borrow().is_empty());
        drop(requests);
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

#[test]
fn plain_admission_does_not_repoll_a_parked_sibling() {
    #[derive(Clone)]
    struct Parked(Rc<Cell<usize>>);
    impl capability::Server for Parked {
        fn as_ptr(&self) -> usize {
            Rc::as_ptr(&self.0) as usize
        }
        fn dispatch_call(
            self,
            _: u64,
            method: u16,
            context: capability::CallContext<'_, any_pointer::Owned, any_pointer::Owned>,
        ) -> capability::DispatchCallResult {
            let (_, mut results) = context.into_owned();
            let promise = if method == 0 {
                Promise::from_future(std::future::poll_fn(move |_| {
                    self.0.set(self.0.get() + 1);
                    let _ = &mut results;
                    Poll::Pending
                }))
            } else {
                Promise::from(results.get().set_as::<capnp::text::Owned>("independent"))
            };
            capability::DispatchCallResult {
                promise,
                is_streaming: false,
                allow_cancellation: true,
            }
        }
    }
    for owned in [false, true] {
        for budget in [1, 8, 64] {
            for busy in [false, true] {
                with_peer(|mut driver, messages| {
                    driver.use_owned_output_queues(owned);
                    let polls = Rc::new(Cell::new(0));
                    let target = expose(
                        &mut driver,
                        Rc::new(crate::local::Client::new(Parked(polls.clone()))),
                    );
                    let mut opts = options();
                    opts.budget = NonZeroUsize::new(budget).unwrap();
                    let mut coordinator = Coordinator::new(
                        driver,
                        ClientHooks::new(8, 8),
                        NonZeroUsize::new(8).unwrap(),
                        None,
                        opts,
                    )
                    .unwrap();
                    let mut queue = messages::Queue::new_growing(0, 1).unwrap();
                    let pipe = Rc::new(RefCell::new(Pipe::default()));
                    let mut writer = Writer(pipe.clone());
                    let mut control = Policy {
                        preserve_plain: true,
                        ..Default::default()
                    };
                    let mut reader = BufferedInput;
                    let mut pump = Pump::default();
                    let mut stop = std::pin::pin!(std::future::pending());
                    let mut cx = Context::from_waker(Waker::noop());
                    let wake = coordinator.notifications.waker();
                    for method in [0, 1] {
                        let before = polls.get();
                        if busy && method == 1 {
                            coordinator.schedule.active |= WorkSet::MAINTENANCE;
                        }
                        let mut pending = Some(
                            Frame::build(messages, 512, opts.reader, |builder| {
                                calls::dynamic_call(
                                    builder,
                                    QuestionId::from_wire(u32::from(method)),
                                    calls::Target::Imported(target),
                                    calls::DynamicCall {
                                        interface: 7,
                                        method,
                                        no_promise_pipelining: true,
                                        ..Default::default()
                                    },
                                    |_| Ok(()),
                                )
                            })
                            .unwrap(),
                        );
                        for _ in 0..4096 {
                            if let Poll::Ready(result) = coordinator.poll_prepared(
                                messages,
                                &mut queue,
                                &mut reader,
                                &mut transport::Writer(&mut writer),
                                &mut [0; 32],
                                &NoResources,
                                &mut control,
                                stop.as_mut(),
                                &mut pending,
                                &mut pump,
                                &mut cx,
                                &wake,
                            ) {
                                result.unwrap();
                            }
                            if pending.is_none()
                                && coordinator.schedule.active == WorkSet::NONE
                                && coordinator.notifications.pending() == WorkSet::NONE
                                && queue.is_empty()
                                && coordinator.output.is_clean()
                            {
                                break;
                            }
                        }
                        assert!(pending.is_none() && coordinator.schedule.active == WorkSet::NONE);
                        assert!(!coordinator.worker.is_empty());
                        if method == 0 {
                            assert!(polls.get() > 0);
                        } else {
                            assert_eq!(
                                polls.get(),
                                before,
                                "owned {owned}, budget {budget}, busy {busy}"
                            );
                        }
                    }
                    let bytes: Vec<_> = pipe.borrow_mut().bytes.drain(..).collect();
                    let mut bytes = bytes.as_slice();
                    let message = capnp::serialize::read_message(&mut bytes, opts.reader).unwrap();
                    let crate::rpc_capnp::message::Return(Ok(returned)) = message
                        .get_root::<crate::rpc_capnp::message::Reader>()
                        .unwrap()
                        .which()
                        .unwrap()
                    else {
                        panic!("Return")
                    };
                    assert_eq!(returned.get_answer_id(), 1);
                    let crate::rpc_capnp::return_::Results(Ok(payload)) = returned.which().unwrap()
                    else {
                        panic!("results")
                    };
                    assert_eq!(
                        payload
                            .get_content()
                            .get_as::<capnp::text::Reader>()
                            .unwrap(),
                        "independent"
                    );
                    assert!(bytes.is_empty());
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
}
