use super::*;
use capntproto_rpc_wire::rpc_capnp::api::{CapDescriptor, Message, Payload, Return};

#[test]
fn completed_pipeline_cleanup_and_successor_calls_survive_small_budgets_and_backpressure() {
    for budget in [1, 8, 64] {
        for capacity in [1, 4] {
            with_peer(|mut driver, messages| {
                let mut hooks = ClientHooks::new(16, 16);
                let client = remote(&mut driver, &mut hooks, ImportId::from_wire(7));
                let mut opts = options();
                opts.budget = NonZeroUsize::new(budget).unwrap();
                let mut coordinator =
                    Coordinator::new(driver, hooks, NonZeroUsize::new(8).unwrap(), None, opts)
                        .unwrap();
                let mut queue = messages::Queue::new_growing(0, capacity).unwrap();
                let pipe = Rc::new(RefCell::new(Pipe::default()));
                let mut writer = Writer(pipe.clone());
                let mut control = Policy::default();
                let mut scratch = [0; 32];
                let mut cx = Context::from_waker(Waker::noop());
                let mut previous_question = None;
                let mut deferred = Vec::new();
                for generation in 0..3 {
                    let mut parent = client
                        .new_call::<any_pointer::Owned, any_pointer::Owned>(7, 0, None)
                        .send();
                    let projected =
                        capability::Client::new(parent.pipeline.hook.get_pipelined_cap(&[]));
                    let mut child = projected
                        .new_call::<any_pointer::Owned, any_pointer::Owned>(7, 1, None)
                        .send();
                    let mut sent = std::mem::take(&mut deferred);
                    // Hold physical output briefly, including across repeated
                    // unrelated notifications. Neither cleanup nor admission
                    // may monopolize the shared allowance or lose a wake.
                    pipe.borrow_mut().blocked = true;
                    for poll in 0..4096 {
                        if poll == 4 {
                            pipe.borrow_mut().blocked = false;
                        }
                        if poll < 8 {
                            coordinator.notifications.waker().wake_by_ref();
                        }
                        coordinator
                            .poll_io_work::<(), _, _>(
                                messages,
                                &mut queue,
                                &mut transport::Writer(&mut writer),
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
                            && coordinator.hooks.inbox.queue.borrow().is_empty()
                        {
                            break;
                        }
                    }
                    let mut calls = Vec::new();
                    let mut finishes = Vec::new();
                    let mut releases = Vec::new();
                    let mut bytes = sent.as_slice();
                    while !bytes.is_empty() {
                        let message =
                            capnp::serialize::read_message(&mut bytes, opts.reader).unwrap();
                        match message
                            .get_root::<crate::rpc_capnp::message::Reader>()
                            .unwrap()
                            .which()
                            .unwrap()
                        {
                            crate::rpc_capnp::message::Call(call) => {
                                let call = call.unwrap();
                                calls.push((call.get_method_id(), call.get_question_id()));
                            }
                            crate::rpc_capnp::message::Finish(finish) => {
                                finishes.push(finish.unwrap().get_question_id())
                            }
                            crate::rpc_capnp::message::Release(release) => {
                                let release = release.unwrap();
                                releases.push((release.get_id(), release.get_reference_count()));
                            }
                            _ => panic!("unexpected output"),
                        }
                    }
                    assert_eq!(calls.len(), 2, "budget {budget}, capacity {capacity}");
                    assert_eq!(finishes, previous_question.into_iter().collect::<Vec<_>>());
                    assert_eq!(
                        releases,
                        if generation == 0 {
                            vec![]
                        } else {
                            vec![(16 + generation, 1)]
                        }
                    );
                    for &(method, question) in &calls {
                        let frame = Frame::build(messages, 512, opts.reader, |builder| {
                            let mut returned = builder
                                .try_init_root::<Message>()?
                                .into_field(Message::RETURN)
                                .init()?;
                            returned.scalar(Return::ANSWER_ID).set(question);
                            returned.scalar(Return::RELEASE_PARAM_CAPS).set(false);
                            returned.scalar(Return::NO_FINISH_NEEDED).set(method != 0);
                            let mut payload = returned.field(Return::RESULTS).init()?;
                            if method == 0 {
                                payload.field(Payload::CONTENT).set_capability_index(0)?;
                                payload
                                    .field(Payload::CAP_TABLE)
                                    .init(1)?
                                    .get_mut(0)
                                    .unwrap()
                                    .scalar(CapDescriptor::SENDER_HOSTED)
                                    .set(17 + generation);
                                previous_question = Some(question);
                            }
                            Ok(())
                        })
                        .unwrap();
                        coordinator
                            .driver
                            .receive(frame, messages, &mut scratch, |_| (), server::Results::new)
                            .unwrap_or_else(|rejected| panic!("{}", rejected.error));
                        coordinator.changed();
                        // As in the transport pump, settle ordered projections
                        // from this Return before admitting the next Return.
                        for _ in 0..4096 {
                            coordinator
                                .poll_io_work::<(), _, _>(
                                    messages,
                                    &mut queue,
                                    &mut transport::Writer(&mut writer),
                                    &mut scratch,
                                    &NoResources,
                                    &mut control,
                                    &mut cx,
                                )
                                .unwrap();
                            deferred.extend(pipe.borrow_mut().bytes.drain(..));
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
                    }
                    assert!(
                        coordinator.calls.is_empty(),
                        "budget {budget}, capacity {capacity}, generation {generation}"
                    );
                    assert!(matches!(
                        Pin::new(&mut parent.promise).poll(&mut cx),
                        Poll::Ready(Ok(_))
                    ));
                    assert!(matches!(
                        Pin::new(&mut child.promise).poll(&mut cx),
                        Poll::Ready(Ok(_))
                    ));
                    drop((parent, child, projected));
                }
                drop(client);
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
