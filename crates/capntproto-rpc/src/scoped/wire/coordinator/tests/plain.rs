use super::*;
use capntproto_rpc_engine::ids::QuestionId;
use capntproto_rpc_wire::{attachments::ImportReceiver, calls};

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
