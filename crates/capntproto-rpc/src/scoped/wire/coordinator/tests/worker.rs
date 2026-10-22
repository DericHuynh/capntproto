use super::*;
use capntproto_rpc_engine::ids::QuestionId;
use capntproto_rpc_wire::{calls, rpc_capnp};

type Application = dyn Fn(
    capability::Params<any_pointer::Owned>,
    capability::Results<any_pointer::Owned>,
) -> capability::DispatchCallResult;
#[derive(Clone)]
struct Service(Rc<Application>);
impl capability::Server for Service {
    fn as_ptr(&self) -> usize {
        Rc::as_ptr(&self.0) as *const () as usize
    }
    fn dispatch_call(
        self,
        _: u64,
        _: u16,
        context: capability::CallContext<'_, any_pointer::Owned, any_pointer::Owned>,
    ) -> capability::DispatchCallResult {
        let (params, results) = context.into_owned();
        (self.0)(params, results)
    }
}
fn service(
    f: impl Fn(capability::Results<any_pointer::Owned>) -> capability::DispatchCallResult + 'static,
) -> Rc<dyn ClientHook> {
    Rc::new(crate::local::Client::new(Service(Rc::new(
        move |_, results| f(results),
    ))))
}

#[test]
fn resumed_plain_results_release_parameter_grants_without_another_input() {
    use capntproto_rpc_wire::rpc_capnp::api::{Call as WireCall, CapDescriptor, Message, Payload};
    for budget in [1, 7, 64] {
        with_peer(|mut driver, messages| {
            let (finish, receiver) = shared::channel::<()>();
            let receiver = RefCell::new(Some(receiver));
            let target = expose(
                &mut driver,
                Rc::new(crate::local::Client::new(Service(Rc::new(
                    move |params, mut results| {
                        let receiver = receiver.borrow_mut().take().unwrap();
                        capability::DispatchCallResult::new(
                            capability::Promise::from_future(async move {
                                receiver.await.map_err(crate::canceled_to_error)?;
                                // Keep the received capability table alive through suspension.
                                drop(params);
                                results.get().set_as::<capnp::text::Owned>("resumed")?;
                                Ok(())
                            }),
                            false,
                        )
                    },
                )))),
            );
            let frame = Frame::build(messages, 512, Default::default(), |builder| {
                calls::dynamic_call(
                    builder,
                    QuestionId::from_wire(4),
                    calls::Target::Imported(target),
                    DynamicCall {
                        interface: 7,
                        no_promise_pipelining: true,
                        ..Default::default()
                    },
                    |content| content.set_capability_index(0),
                )?;
                let mut root = builder.try_get_root::<Message>()?;
                let mut call = root.field(Message::CALL).edit()?;
                let mut params = call.field(WireCall::PARAMS).edit()?;
                let mut table = params.field(Payload::CAP_TABLE).init(1)?;
                table
                    .get_mut(0)
                    .unwrap()
                    .scalar(CapDescriptor::SENDER_HOSTED)
                    .set(17);
                Ok(())
            })
            .unwrap();
            driver
                .receive(frame, messages, &mut [0; 16], |_| (), server::Producer::new)
                .unwrap_or_else(|rejected| panic!("{}", rejected.error));
            let mut opts = options();
            opts.budget = NonZeroUsize::new(budget).unwrap();
            let mut coordinator = Coordinator::new(
                driver,
                ClientHooks::new(64, 16),
                NonZeroUsize::new(1).unwrap(),
                None,
                opts,
            )
            .unwrap();
            let mut queue = messages::Queue::new_growing(0, 1).unwrap();
            let mut cx = Context::from_waker(Waker::noop());
            let mut scratch = [0; 32];
            for _ in 0..512 {
                if let Poll::Ready(result) =
                    coordinator.poll_work(messages, &mut queue, &mut scratch, &NoResources, &mut cx)
                {
                    result.unwrap();
                }
                if coordinator.schedule.active == WorkSet::NONE
                    && coordinator.notifications.pending() == WorkSet::NONE
                {
                    break;
                }
            }
            assert!(queue.is_empty());
            assert!(!coordinator.worker.is_empty());
            finish.send(()).unwrap();
            let mut replies = 0;
            let mut releases = 0;
            for _ in 0..2048 {
                if let Poll::Ready(result) =
                    coordinator.poll_work(messages, &mut queue, &mut scratch, &NoResources, &mut cx)
                {
                    result.unwrap();
                }
                if let Some(window) = queue.window(messages).unwrap() {
                    let mut bytes = window.bytes();
                    let reader = capnp::serialize::read_message_from_flat_slice_no_alloc(
                        &mut bytes,
                        Default::default(),
                    )
                    .unwrap();
                    let message = reader.get_root::<rpc_capnp::message::Reader>().unwrap();
                    match message.which().unwrap() {
                        rpc_capnp::message::Return(result) => {
                            let result = result.unwrap();
                            assert_eq!(result.get_answer_id(), 4);
                            assert!(result.get_no_finish_needed());
                            let rpc_capnp::return_::Results(payload) = result.which().unwrap()
                            else {
                                panic!("results");
                            };
                            assert_eq!(
                                payload
                                    .unwrap()
                                    .get_content()
                                    .get_as::<capnp::text::Reader>()
                                    .unwrap(),
                                "resumed"
                            );
                            replies += 1;
                        }
                        rpc_capnp::message::Release(release) => {
                            let release = release.unwrap();
                            assert_eq!(release.get_id(), 17);
                            assert_eq!(release.get_reference_count(), 1);
                            releases += 1;
                        }
                        _ => panic!("unexpected control"),
                    }
                    let count = window.bytes().len();
                    window.advance(count).unwrap();
                    // Completion of blocked FIFO output wakes every blocked lane.
                    coordinator.changed();
                }
                if replies == 1 && releases == 1 && coordinator.schedule.active == WorkSet::NONE {
                    break;
                }
            }
            assert_eq!((replies, releases), (1, 1));
            assert!(coordinator.worker.is_empty());
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
fn pending_worker_wake_dispatches_early_pipeline_before_parent_completes() {
    for budget in [1, 7, 64] {
        with_peer(|mut driver, messages| {
            let child = service(|mut results| {
                results.get().set_as::<capnp::text::Owned>("child").unwrap();
                capability::DispatchCallResult::new(capability::Promise::ok(()), false)
            });
            let (early, early_receiver) = shared::channel::<()>();
            let (finish, finish_receiver) = shared::channel::<()>();
            let receivers = RefCell::new(Some((early_receiver, finish_receiver)));
            let target = expose(
                &mut driver,
                service(move |mut results| {
                    let child = child.add_ref();
                    let (early, finish) = receivers.borrow_mut().take().unwrap();
                    capability::DispatchCallResult::new(
                        capability::Promise::from_future(async move {
                            early.await.map_err(crate::canceled_to_error)?;
                            results.get().set_as_capability(child);
                            results.set_pipeline()?;
                            finish.await.map_err(crate::canceled_to_error)?;
                            Err(Error::overloaded("parent completed after child".into()))
                        }),
                        false,
                    )
                }),
            );
            for (id, target) in [
                (0, calls::Target::Imported(target)),
                (
                    1,
                    calls::Target::Promised {
                        question: QuestionId::from_wire(0),
                        path: pipeline::Path::new(&[]),
                    },
                ),
            ] {
                let frame = Frame::build(messages, 512, Default::default(), |root| {
                    calls::dynamic_call(
                        root,
                        QuestionId::from_wire(id),
                        target,
                        DynamicCall {
                            interface: 7,
                            ..Default::default()
                        },
                        |_| Ok(()),
                    )
                })
                .unwrap();
                driver
                    .receive(frame, messages, &mut [0; 16], |_| (), server::Producer::new)
                    .unwrap_or_else(|rejected| panic!("{}", rejected.error));
            }
            let mut opts = options();
            opts.budget = NonZeroUsize::new(budget).unwrap();
            let mut coordinator = Coordinator::new(
                driver,
                ClientHooks::new(64, 16),
                NonZeroUsize::new(2).unwrap(),
                None,
                opts,
            )
            .unwrap();
            let mut queue = messages::Queue::new_growing(0, 4).unwrap();
            let parent = Arc::new(WakeCount::default());
            let parent_waker = Waker::from(parent.clone());
            let mut cx = Context::from_waker(&parent_waker);
            let mut scratch = [0; 32];
            for _ in 0..512 {
                if let Poll::Ready(result) =
                    coordinator.poll_work(messages, &mut queue, &mut scratch, &NoResources, &mut cx)
                {
                    result.unwrap();
                }
                if coordinator.schedule.active == WorkSet::NONE
                    && coordinator.notifications.pending() == WorkSet::NONE
                {
                    break;
                }
            }
            assert_eq!(coordinator.schedule.active, WorkSet::NONE);
            assert!(queue.is_empty());
            let wakes = parent.0.load(Ordering::SeqCst);
            early.send(()).unwrap();
            assert!(parent.0.load(Ordering::SeqCst) > wakes);
            assert_ne!(coordinator.notifications.pending(), WorkSet::NONE);
            for _ in 0..1024 {
                if let Poll::Ready(result) =
                    coordinator.poll_work(messages, &mut queue, &mut scratch, &NoResources, &mut cx)
                {
                    result.unwrap();
                }
                if !queue.is_empty() {
                    break;
                }
            }
            {
                let window = queue
                    .window(messages)
                    .unwrap()
                    .expect("early child reply while parent waits");
                let mut bytes = window.bytes();
                let reader = capnp::serialize::read_message_from_flat_slice_no_alloc(
                    &mut bytes,
                    Default::default(),
                )
                .unwrap();
                let root = reader.get_root::<rpc_capnp::message::Reader>().unwrap();
                let rpc_capnp::message::Return(reply) = root.which().unwrap() else {
                    panic!("Return")
                };
                let reply = reply.unwrap();
                assert_eq!(reply.get_answer_id(), 1);
                let rpc_capnp::return_::Results(result) = reply.which().unwrap() else {
                    panic!("child results")
                };
                assert_eq!(
                    result
                        .unwrap()
                        .get_content()
                        .get_as::<capnp::text::Reader>()
                        .unwrap(),
                    "child"
                );
                let count = window.bytes().len();
                window.advance(count).unwrap();
            }
            finish.send(()).unwrap();
            for _ in 0..1024 {
                if let Poll::Ready(result) =
                    coordinator.poll_work(messages, &mut queue, &mut scratch, &NoResources, &mut cx)
                {
                    result.unwrap();
                }
                if !queue.is_empty() {
                    break;
                }
            }
            assert!(!queue.is_empty(), "parent completion wakes independently");
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
fn single_service_wake_never_polls_parked_siblings() {
    // Repetition measures native scheduling variation. Under the interpreter,
    // keep both output modes, dispatch paths, budget extremes and table sizes
    // on either side of the small-table threshold without repeating the probe.
    let repeats = if cfg!(miri) { 1 } else { 5 };
    let budgets: &[usize] = if cfg!(miri) { &[1, 64] } else { &[1, 8, 64] };
    let counts: &[usize] = if cfg!(miri) {
        &[1, 2, 16]
    } else {
        &[1, 2, 8, 16]
    };
    for generic in [false, true] {
        for repeat in 0..repeats {
            for owned in [false, true] {
                for &budget in budgets {
                    for &count in counts {
                        for selected in [0, count / 2, count - 1] {
                            with_peer(|mut driver, messages| {
                                driver.use_owned_output_queues(owned);
                                let polls = Rc::new(RefCell::new(vec![0usize; count]));
                                let waiters = Rc::new(RefCell::new(vec![None::<Waker>; count]));
                                let ready = Rc::new(RefCell::new(vec![false; count]));
                                let service_polls = polls.clone();
                                let service_waiters = waiters.clone();
                                let service_ready = ready.clone();
                                let next = std::cell::Cell::new(0);
                                let target = expose(
                                    &mut driver,
                                    service(move |mut results| {
                                        let index = next.get();
                                        next.set(index + 1);
                                        let polls = service_polls.clone();
                                        let waiters = service_waiters.clone();
                                        let ready = service_ready.clone();
                                        capability::DispatchCallResult::with_cancellation_policy(
                                            capability::Promise::from_future(
                                                futures::future::poll_fn(move |cx| {
                                                    polls.borrow_mut()[index] += 1;
                                                    waiters.borrow_mut()[index] =
                                                        Some(cx.waker().clone());
                                                    if ready.borrow()[index] {
                                                        results
                                                            .get()
                                                            .set_as::<capnp::text::Owned>(
                                                                "ready",
                                                            )?;
                                                        Poll::Ready(Ok(()))
                                                    } else {
                                                        Poll::Pending
                                                    }
                                                }),
                                            ),
                                            false,
                                            true,
                                        )
                                    }),
                                );
                                for index in 0..count {
                                    let frame = Frame::build(
                                        messages,
                                        512,
                                        Default::default(),
                                        |builder| {
                                            calls::dynamic_call(
                                                builder,
                                                QuestionId::from_wire(index as u32),
                                                calls::Target::Imported(target),
                                                DynamicCall {
                                                    interface: 7,
                                                    no_promise_pipelining: true,
                                                    ..Default::default()
                                                },
                                                |_| Ok(()),
                                            )
                                        },
                                    )
                                    .unwrap();
                                    driver
                                        .receive(
                                            frame,
                                            messages,
                                            &mut [0; 16],
                                            |_| (),
                                            server::Producer::new,
                                        )
                                        .unwrap_or_else(|rejected| panic!("{}", rejected.error));
                                }
                                let mut opts = options();
                                opts.budget = NonZeroUsize::new(budget).unwrap();
                                let mut coordinator = Coordinator::new(
                                    driver,
                                    ClientHooks::new(128, 32),
                                    NonZeroUsize::new(count).unwrap(),
                                    None,
                                    opts,
                                )
                                .unwrap();
                                let mut queue = messages::Queue::new_growing(0, count).unwrap();
                                let mut scratch = [0; 32];
                                let mut cx = Context::from_waker(Waker::noop());
                                let mut parked = false;
                                for _ in 0..4096 {
                                    if let Poll::Ready(result) = coordinator.poll_work(
                                        messages,
                                        &mut queue,
                                        &mut scratch,
                                        &NoResources,
                                        &mut cx,
                                    ) {
                                        result.unwrap();
                                    }
                                    if coordinator.schedule.active == WorkSet::NONE
                                        && coordinator.notifications.pending() == WorkSet::NONE
                                        && waiters.borrow().iter().all(Option::is_some)
                                    {
                                        parked = true;
                                        break;
                                    }
                                }
                                assert!(parked);
                                assert!(queue.is_empty());
                                let before = polls.borrow().clone();
                                ready.borrow_mut()[selected] = true;
                                waiters.borrow()[selected].as_ref().unwrap().wake_by_ref();
                                if generic {
                                    coordinator.notifications.waker().wake_by_ref();
                                }
                                let mut published = false;
                                let mut turns = 0;
                                for _ in 0..4096 {
                                    turns += 1;
                                    if let Poll::Ready(result) = coordinator.poll_work(
                                        messages,
                                        &mut queue,
                                        &mut scratch,
                                        &NoResources,
                                        &mut cx,
                                    ) {
                                        result.unwrap();
                                    }
                                    if !queue.is_empty() {
                                        published = true;
                                        break;
                                    }
                                }
                                assert!(published);
                                let delta: Vec<_> = polls
                                    .borrow()
                                    .iter()
                                    .zip(before)
                                    .map(|(after, before)| after - before)
                                    .collect();
                                let sibling_polls = delta
                                    .iter()
                                    .enumerate()
                                    .filter(|(i, _)| *i != selected)
                                    .map(|(_, n)| *n)
                                    .sum::<usize>();
                                println!("COORDINATOR_PROBE generic={} repeat={repeat} owned={} budget={budget} parked={count} selected={selected} ready_polls={} sibling_polls={sibling_polls} coordinator_polls={turns}", usize::from(generic), usize::from(owned), delta[selected]);
                                assert_eq!(delta[selected], 1);
                                assert_eq!(
                                sibling_polls, 0,
                                "owned={owned} budget={budget} count={count} selected={selected}"
                            );
                                assert_eq!(queue.len(), 1);
                                let mut bytes = Vec::new();
                                while let Some(window) = queue.window(messages).unwrap() {
                                    let length = window.bytes().len();
                                    bytes.extend_from_slice(window.bytes());
                                    window.advance(length).unwrap();
                                }
                                let mut input = bytes.as_slice();
                                // Transport bytes have byte alignment, including under Miri.
                                let message =
                                    capnp::serialize::read_message(&mut input, Default::default())
                                        .unwrap();
                                assert!(input.is_empty());
                                let rpc_capnp::message::Return(reply) = message
                                    .get_root::<rpc_capnp::message::Reader>()
                                    .unwrap()
                                    .which()
                                    .unwrap()
                                else {
                                    panic!("expected one Return")
                                };
                                let reply = reply.unwrap();
                                assert_eq!(reply.get_answer_id(), selected as u32);
                                let rpc_capnp::return_::Results(payload) = reply.which().unwrap()
                                else {
                                    panic!("expected results")
                                };
                                assert_eq!(
                                    payload
                                        .unwrap()
                                        .get_content()
                                        .get_as::<capnp::text::Reader>()
                                        .unwrap(),
                                    "ready"
                                );
                                futures::executor::block_on(
                                    coordinator.terminate::<NoResources, (), ()>(
                                        messages,
                                        &mut queue,
                                        &mut Policy::default(),
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
        }
    }
}
