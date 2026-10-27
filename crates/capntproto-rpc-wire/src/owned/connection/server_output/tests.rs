use super::*;
use capnp::{word, Word};
use capntproto_rpc_engine::{connection as storage, ids::QuestionId};
use core::num::NonZeroUsize;
const ZERO: Word = word(0, 0, 0, 0, 0, 0, 0, 0);
type Peer<'p, 'id, 's> = Driver<'p, 'id, 's, (), u32, u8, u32, 4, 4>;

fn deferred_promised_request<'p, 'id>(
    driver: &mut Peer<'p, 'id, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    parent: answers::Key<'id>,
    question: u32,
) -> answers::Key<'id> {
    driver.defer_call_dispatch(true);
    let frame = Frame::build(messages, 512, Default::default(), |builder| {
        calls::dynamic_call(
            builder,
            QuestionId::from_wire(question),
            calls::Target::Promised {
                question: QuestionId::from_wire(parent.id().to_wire()),
                path: pipeline::Path::new(&[]),
            },
            calls::DynamicCall {
                no_promise_pipelining: true,
                ..Default::default()
            },
            |_| Ok(()),
        )
    })
    .unwrap();
    let Received::DeferredCall(key) = driver
        .receive(frame, messages, &mut [0; 8], |_| (), || 84)
        .unwrap_or_else(|rejected| panic!("{}", rejected.error))
    else {
        panic!("deferred Call")
    };
    key
}

#[test]
fn settled_promised_dispatch_needs_no_observer_and_preserves_early_pipeline_priority() {
    for early in [false, true] {
        let measured = allocation_counter::measure(|| {
            with_driver(|mut driver, messages| {
                let (parent, target) = request(&mut driver, messages, false);
                let alternative = driver
                    .runtime
                    .endpoint(binding::Route::local(92), clients::Location::Local)
                    .unwrap();
                let result = driver
                    .local_result_dynamic(
                        7,
                        9,
                        &[Some(&target)],
                        messages,
                        512,
                        Default::default(),
                        |content| content.set_capability_index(0),
                    )
                    .unwrap();
                driver
                    .complete_answer(&parent, Ok(result), messages)
                    .unwrap_or_else(|_| panic!("completion"));
                driver.pipelines = pipelines::Bindings::new(&mut []).unwrap();
                let child = deferred_promised_request(&mut driver, messages, parent.answer(), 4);
                assert!(
                    !driver
                        .ledgers
                        .answers
                        .finish(parent.answer().id(), false)
                        .cancel
                );
                let (running, execute) = driver
                    .dispatch_immediate_call_with(true, messages, |_, _, graph| {
                        if early {
                            graph
                                .retain_key(alternative.key())
                                .map(|client| Some(Ok(client)))
                                .map_err(pipelines::Error::Graph)
                        } else {
                            Ok(None)
                        }
                    })
                    .unwrap()
                    .expect("settled target dispatches without an observer");
                assert!(execute);
                assert_eq!(
                    running.target().key(),
                    if early {
                        alternative.key()
                    } else {
                        target.key()
                    }
                );
                assert_eq!(running.owner().answer(), child);
                assert_eq!(driver.runtime.queued(), 0);
                assert!(driver.call_target.is_none());
                let call = driver.runtime.complete(running);
                driver.release_call(call, messages);
                driver.release_call(parent, messages);
                driver.runtime.release(target);
                driver.runtime.release(alternative);
                close(
                    driver,
                    messages,
                    &mut messages::Queue::<()>::new(&mut []).unwrap(),
                );
            })
        });
        assert_eq!(measured.count_total, 0);
    }
}

#[test]
fn resolved_promised_dispatch_reuses_target_and_preserves_order_and_cleanup() {
    for mode in 0..3 {
        let measured = allocation_counter::measure(|| {
            with_driver(|mut driver, messages| {
                let (parent, target) = request(&mut driver, messages, false);
                let waiting = driver.runtime.promise(&mut None).unwrap();
                driver.pipelines = pipelines::Bindings::new(&mut []).unwrap();
                let child = deferred_promised_request(&mut driver, messages, parent.answer(), 4);
                assert!(driver
                    .dispatch_immediate_call_with(true, messages, |_, _, graph| {
                        graph
                            .retain_key(waiting.key())
                            .map(|client| Some(Ok(client)))
                            .map_err(pipelines::Error::Graph)
                    })
                    .unwrap()
                    .is_none());
                assert!(driver.call_target.is_some());
                assert!(driver
                    .dispatch_immediate_call_with(true, messages, |_, _, _| {
                        panic!("retry must retain its original resolved target")
                    })
                    .unwrap()
                    .is_none());
                if mode == 1 {
                    assert_eq!(driver.retry_call().unwrap(), Some(child));
                    assert!(driver.call_target.is_none());
                    let later =
                        deferred_promised_request(&mut driver, messages, parent.answer(), 5);
                    assert!(driver
                        .dispatch_immediate_call_with(true, messages, |_, _, _| {
                            panic!("an older queued call must defer target resolution")
                        })
                        .unwrap()
                        .is_none());
                    let replacement = driver.runtime.retain_key(target.key()).unwrap();
                    let barrier = driver
                        .runtime
                        .prepare_resolution(
                            waiting.key(),
                            &mut Some(replacement),
                            clients::Source::Direct,
                        )
                        .unwrap()
                        .accept();
                    assert!(barrier.is_none());
                    let ready = driver
                        .runtime
                        .next_ready()
                        .unwrap()
                        .expect("resolved child");
                    let running = ready.accept();
                    assert_eq!(running.target().key(), target.key());
                    let call = driver
                        .runtime
                        .complete(running)
                        .into_call()
                        .unwrap_or_else(|_| panic!("child Call"));
                    assert_eq!(call.answer(), child);
                    driver.release_call(call, messages);
                    let (running, _) = driver
                        .dispatch_immediate_call_with(true, messages, |_, _, graph| {
                            graph
                                .retain_key(target.key())
                                .map(|client| Some(Ok(client)))
                                .map_err(pipelines::Error::Graph)
                        })
                        .unwrap()
                        .expect("later child follows the queued child");
                    assert_eq!(running.owner().answer(), later);
                    let call = driver.runtime.complete(running);
                    driver.release_call(call, messages);
                } else if mode == 2 {
                    assert!(driver.ledgers.answers.finish(child.id(), false).cancel);
                    let mut slots = [None];
                    let mut queue = messages::Queue::<()>::new(&mut slots).unwrap();
                    driver.work_cursor = 11;
                    assert!(matches!(
                        driver
                            .poll_maintenance(
                                messages,
                                &mut queue,
                                128,
                                Default::default(),
                                &mut [0; 8],
                                NonZeroUsize::new(1).unwrap()
                            )
                            .unwrap(),
                        Progress::Producer(84)
                    ));
                    assert!(driver.call.is_none());
                    assert!(
                        driver.call_target.is_none(),
                        "cancellation releases the cached target"
                    );
                    drain(&mut queue, messages, |root| {
                        let rpc_capnp::message::Return(Ok(returned)) = root.which().unwrap() else {
                            panic!("Return")
                        };
                        assert_eq!(returned.get_answer_id(), child.id().to_wire());
                        assert!(matches!(
                            returned.which().unwrap(),
                            rpc_capnp::return_::Canceled(())
                        ));
                    });
                }
                driver.runtime.release(waiting);
                driver.release_call(parent, messages);
                driver.runtime.release(target);
                close(
                    driver,
                    messages,
                    &mut messages::Queue::<()>::new(&mut []).unwrap(),
                );
            })
        });
        assert_eq!(measured.count_total, 0);
    }
}

#[cfg(feature = "alloc")]
#[test]
fn direct_prepared_return_preserves_dependencies_and_rolls_back_every_capacity_failure() {
    let mut prepared = PreparedReturn::from_template(
        &PreparedReturnTemplate::default(),
        capnp::message::HeapAllocator::new(),
    );
    prepared
        .results()
        .set_as::<capnp::text::Owned>("direct")
        .unwrap();
    let prepared = prepared.freeze();
    let measured = allocation_counter::measure(|| {
        with_driver(|mut driver, messages| {
            let (call, target) = request(&mut driver, messages, false);
            let key = call.answer();
            let pipeline = driver.ledgers.answers.pipeline(key.id()).unwrap();
            let mut slots = [None];
            let mut queue = messages::Queue::new(&mut slots).unwrap();
            assert!(driver
                .try_send_answer_prepared(
                    &call,
                    &prepared,
                    messages,
                    &mut queue,
                    Default::default(),
                    ()
                )
                .unwrap()
                .is_none());
            assert!(queue.is_empty());
            assert!(driver.ledgers.answers.release_pipeline(pipeline).is_none());

            let mut empty = messages::Queue::new(&mut []).unwrap();
            assert!(driver
                .try_send_answer_prepared(
                    &call,
                    &prepared,
                    messages,
                    &mut empty,
                    Default::default(),
                    ()
                )
                .is_err());
            let mut held = [const { None }; 6];
            for slot in &mut held {
                *slot = messages.reserve(1).ok();
            }
            assert!(driver
                .try_send_answer_prepared(
                    &call,
                    &prepared,
                    messages,
                    &mut queue,
                    Default::default(),
                    ()
                )
                .is_err());
            assert!(queue.is_empty());
            assert!(driver.ledgers.answers.get(key).unwrap().owner.is_pending());
            for writer in held.into_iter().flatten() {
                messages.discard(writer);
            }
            let strict = capnp::message::ReaderOptions {
                nesting_limit: 0,
                ..Default::default()
            };
            assert!(driver
                .try_send_answer_prepared(&call, &prepared, messages, &mut queue, strict, ())
                .is_err());
            assert!(queue.is_empty());
            assert!(driver.ledgers.answers.get(key).unwrap().owner.is_pending());

            assert_eq!(
                driver
                    .try_send_answer_prepared(
                        &call,
                        &prepared,
                        messages,
                        &mut queue,
                        Default::default(),
                        ()
                    )
                    .unwrap(),
                Some(73)
            );
            assert!(driver.ledgers.answers.get(key).is_err());
            drain(&mut queue, messages, |root| {
                let rpc_capnp::message::Return(Ok(returned)) = root.which().unwrap() else {
                    panic!("return")
                };
                assert!(returned.get_no_finish_needed());
                let rpc_capnp::return_::Results(Ok(payload)) = returned.which().unwrap() else {
                    panic!("results")
                };
                assert_eq!(
                    payload
                        .get_content()
                        .get_as::<capnp::text::Reader>()
                        .unwrap(),
                    "direct"
                );
            });
            driver.release_call(call, messages);
            driver.runtime.release(target);
            close(driver, messages, &mut queue);
        })
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn prepared_data_reply_reuses_storage_and_late_cancel_rebuilds_without_allocating() {
    check_prepared_data_reply(false);
}

#[cfg(feature = "alloc")]
#[test]
fn hosted_return_template_preserves_retry_cancellation_and_allocation_free_publication() {
    check_prepared_data_reply(true);
}

fn check_prepared_data_reply(_hosted: bool) {
    // Only the hosted application builder allocates. Its envelope relocation,
    // validation and publication must still use the caller's fixed storage.
    #[cfg(feature = "alloc")]
    let prepared = _hosted.then(|| {
        let mut source = PreparedReturn::from_template(
            &PreparedReturnTemplate::default(),
            capnp::message::HeapAllocator::new(),
        );
        source
            .results()
            .set_as::<capnp::text::Owned>("retained result")
            .unwrap();
        source.freeze()
    });
    for canceled in [false, true] {
        let measured = allocation_counter::measure(|| {
            with_driver(|mut driver, messages| {
                let (call, target) = request(&mut driver, messages, false);
                let key = call.answer();
                #[cfg(feature = "alloc")]
                let result = prepared.as_ref().map(|source| {
                    driver
                        .local_answer_prepared(key, 7, 9, source, messages, Default::default())
                        .unwrap()
                        .unwrap()
                });
                let mut dynamic = || {
                    driver
                        .local_answer_dynamic(
                            key,
                            7,
                            9,
                            &[],
                            messages,
                            512,
                            Default::default(),
                            |field| {
                                field
                                    .cast::<capnp::field_api::Text>()
                                    .copy_from("retained result")
                            },
                        )
                        .unwrap()
                };
                #[cfg(feature = "alloc")]
                let result = result.unwrap_or_else(&mut dynamic);
                #[cfg(not(feature = "alloc"))]
                let result = dynamic();
                let original = messages.read(&result.frame.lease).as_ptr();
                driver
                    .complete_answer(&call, Ok(result), messages)
                    .unwrap_or_else(|_| panic!("completion"));
                if canceled {
                    driver.ledgers.answers.finish(key.id(), false);
                }
                // Every byte buffer is occupied. A data-only prepared reply needs
                // no new buffer; cancellation still needs its distinct envelope.
                let mut held = [const { None }; 6];
                for slot in &mut held {
                    *slot = messages.reserve(1).ok();
                }
                let mut slots = [None];
                let mut queue = messages::Queue::new(&mut slots).unwrap();
                if !canceled {
                    let strict = capnp::message::ReaderOptions {
                        nesting_limit: 0,
                        ..Default::default()
                    };
                    assert!(
                        driver
                            .send_answer(
                                key,
                                messages,
                                &mut queue,
                                512,
                                strict,
                                &mut [0; 8],
                                |_, _| panic!("exception"),
                            )
                            .is_err(),
                        "a certificate does not bypass changed reader limits"
                    );
                    assert!(queue.is_empty());
                    assert!(driver.ledgers.answers.prepare_return(key).is_ok());
                }
                let outcome = driver.send_answer(
                    key,
                    messages,
                    &mut queue,
                    512,
                    Default::default(),
                    &mut [0; 8],
                    |_, _| panic!("exception"),
                );
                if canceled {
                    assert!(outcome.is_err());
                    assert!(queue.is_empty());
                    messages.discard(held.iter_mut().find_map(Option::take).unwrap());
                    driver
                        .send_answer(
                            key,
                            messages,
                            &mut queue,
                            512,
                            Default::default(),
                            &mut [0; 8],
                            |_, _| panic!("exception"),
                        )
                        .unwrap();
                } else {
                    assert!(outcome.is_ok());
                    let window = queue.window(messages).unwrap().unwrap();
                    assert_eq!(window.bytes().as_ptr(), original);
                    drop(window);
                }
                drain(&mut queue, messages, |root| {
                    let rpc_capnp::message::Return(returned) = root.which().unwrap() else {
                        panic!("Return");
                    };
                    let returned = returned.unwrap();
                    assert_eq!(returned.get_answer_id(), 3);
                    if canceled {
                        assert!(matches!(
                            returned.which().unwrap(),
                            rpc_capnp::return_::Canceled(())
                        ));
                    } else {
                        assert!(returned.get_no_finish_needed());
                        let rpc_capnp::return_::Results(payload) = returned.which().unwrap() else {
                            panic!("results");
                        };
                        assert_eq!(
                            payload
                                .unwrap()
                                .get_content()
                                .get_as::<capnp::text::Reader>()
                                .unwrap(),
                            "retained result"
                        );
                    }
                });
                for writer in held.into_iter().flatten() {
                    messages.discard(writer);
                }
                driver.release_call(call, messages);
                driver.runtime.release(target);
                close(driver, messages, &mut queue);
            })
        });
        assert_eq!(measured.count_total, 0);
    }
}

#[test]
fn introduction_publication_validates_grants_and_retries_without_allocation() {
    use crate::publication::descriptors::Descriptors;
    use crate::rpc_capnp::api::ThirdPartyCapDescriptor;
    let measured = allocation_counter::measure(|| {
        with_driver(|mut driver, messages| {
            let (call, target) = request(&mut driver, messages, false);
            let vine = driver
                .runtime
                .endpoint(binding::Route::Vine(99), clients::Location::Local)
                .unwrap();
            let result = driver
                .local_result_dynamic(
                    7,
                    9,
                    &[Some(&vine)],
                    messages,
                    256,
                    Default::default(),
                    |field| field.set_capability_index(0),
                )
                .unwrap();
            driver
                .complete_answer(&call, Ok(result), messages)
                .unwrap_or_else(|_| panic!("completion"));
            let mut slots = [None];
            let mut queue = messages::Queue::new(&mut slots).unwrap();
            driver.set_descriptors(Descriptors {
                extra_bytes: |_| Ok(64),
                write: |_, _, mut field| {
                    let mut third = field.field(CapDescriptor::THIRD_PARTY_HOSTED).init()?;
                    third.scalar(ThirdPartyCapDescriptor::VINE_ID).set(u32::MAX);
                    Ok(())
                },
            });
            assert_eq!(
                driver
                    .send_answer(
                        call.answer(),
                        messages,
                        &mut queue,
                        256,
                        Default::default(),
                        &mut [0; 8],
                        |_, _| Ok(())
                    )
                    .unwrap_err(),
                Error::Input(crate::owned::Error::Output(
                    crate::admission::Error::Publication(crate::publication::Error::Descriptors(
                        exports::DescriptorError::Unreserved
                    ))
                ))
            );
            assert!(queue.is_empty());
            driver.set_descriptors(Descriptors {
                extra_bytes: |_| Ok(64),
                write: |_, id, mut field| {
                    let mut third = field.field(CapDescriptor::THIRD_PARTY_HOSTED).init()?;
                    third
                        .scalar(ThirdPartyCapDescriptor::VINE_ID)
                        .set(id.to_wire());
                    third
                        .field(ThirdPartyCapDescriptor::ID)
                        .cast::<capnp::field_api::Data>()
                        .copy_from(&b"authenticated"[..])
                },
            });
            driver
                .send_answer(
                    call.answer(),
                    messages,
                    &mut queue,
                    256,
                    Default::default(),
                    &mut [0; 8],
                    |_, _| Ok(()),
                )
                .unwrap();
            drain(&mut queue, messages, |root| {
                let rpc_capnp::message::Return(ret) = root.which().unwrap() else {
                    panic!("Return")
                };
                let rpc_capnp::return_::Results(payload) = ret.unwrap().which().unwrap() else {
                    panic!("Results")
                };
                let rpc_capnp::cap_descriptor::ThirdPartyHosted(third) = payload
                    .unwrap()
                    .get_cap_table()
                    .unwrap()
                    .get(0)
                    .which()
                    .unwrap()
                else {
                    panic!("introduction")
                };
                let third = third.unwrap();
                assert_eq!(
                    third.get_id().get_as::<capnp::data::Reader>().unwrap(),
                    b"authenticated"
                );
                assert!(driver
                    .ledgers
                    .exports
                    .lookup(ExportId::from_wire(third.get_vine_id()))
                    .is_some());
            });
            driver.release_call(call, messages);
            driver.runtime.release(vine);
            driver.runtime.release(target);
            close(driver, messages, &mut queue);
        });
    });
    assert_eq!(measured.count_total, 0);
}

fn with_driver(
    f: impl for<'p, 'id, 's, 'm, 'b> FnOnce(Peer<'p, 'id, 's>, &mut messages::Messages<'p, 'm, 'b>),
) {
    let mut bytes = [[ZERO; 128]; 6];
    let mut buffers = bytes
        .each_mut()
        .map(|b| messages::Buffer::new(Word::words_to_bytes_mut(b)));
    messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        storage::with_domain(|domain| {
            let mut slots = FixedStorage::<(), u32, u8, u32, 4, 4, 8, 24>::new();
            f(slots.bind(domain, 1).unwrap(), messages);
        });
    })
    .unwrap();
}
fn request<'p, 'id>(
    driver: &mut Peer<'p, 'id, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    redirected: bool,
) -> (Request<'p, 'id, (), 4, 4>, clients::Client<'id>) {
    let target = driver
        .runtime
        .endpoint(binding::Route::local(91), clients::Location::Local)
        .unwrap();
    let id = {
        let mut batch = driver.ledgers.exports.batch();
        let id = binding::stage(&mut driver.runtime, &mut batch, &target)
            .unwrap()
            .id();
        batch.commit();
        id
    };
    let frame = Frame::build(messages, 512, Default::default(), |builder| {
        calls::dynamic_call(
            builder,
            QuestionId::from_wire(3),
            calls::Target::Imported(ImportId::from_wire(id.to_wire())),
            calls::DynamicCall {
                interface: 7,
                method: 9,
                ..Default::default()
            },
            |_| Ok(()),
        )?;
        if redirected {
            use crate::rpc_capnp::api::{call::SendResultsTo, Call};
            builder
                .try_get_root::<Message>()?
                .into_field(Message::CALL)
                .edit()?
                .group(Call::SEND_RESULTS_TO)
                .select()
                .void(SendResultsTo::YOURSELF)
                .set();
        }
        Ok(())
    })
    .unwrap();
    let call = Request::admit(
        frame,
        messages,
        &mut driver.ledgers.imports,
        &mut driver.ledgers.exports,
        &mut driver.ledgers.answers,
        Answer::new(pipelines::Results::new(73)),
        |_| (),
    )
    .unwrap_or_else(|_| panic!("admission"));
    assert!(driver
        .ledgers
        .answers
        .dispatch(call.answer(), true)
        .unwrap());
    (call, target)
}
fn drain<'p, R>(
    queue: &mut messages::Queue<'p, '_, R>,
    messages: &mut messages::Messages<'p, '_, '_>,
    check: impl FnOnce(rpc_capnp::message::Reader<'_>),
) {
    let window = queue.window(messages).unwrap().unwrap();
    let mut bytes = window.bytes();
    let parsed =
        capnp::serialize::read_message_from_flat_slice_no_alloc(&mut bytes, Default::default())
            .unwrap();
    check(parsed.get_root().unwrap());
    let n = window.bytes().len();
    window.advance(n).unwrap();
}
fn close<'p, R>(
    driver: Peer<'p, '_, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_, R>,
) {
    let mut terminal = driver.terminate(queue);
    for _ in 0..256 {
        if matches!(
            terminal
                .poll(messages, NonZeroUsize::new(32).unwrap())
                .unwrap(),
            Termination::Done
        ) {
            let mut slots = [None, None, None, None, None, None];
            for s in &mut slots {
                *s = Some(messages.reserve(1024).unwrap());
            }
            for s in slots {
                messages.discard(s.unwrap());
            }
            return;
        }
    }
    panic!("cleanup");
}
#[test]
fn adopted_output_waits_for_local_completion_and_retains_result_after_finish_without_allocating() {
    for failed in [false, true] {
        let measured = allocation_counter::measure(|| {
            with_driver(|mut driver, messages| {
                let (call, target) = request(&mut driver, messages, true);
                let mut slots = [None];
                let mut queue = messages::Queue::new(&mut slots).unwrap();
                let question = driver
                    .send(
                        messages,
                        &mut queue,
                        512,
                        Default::default(),
                        |builder, id| {
                            calls::dynamic_call(
                                builder,
                                id,
                                calls::Target::Imported(ImportId::from_wire(0)),
                                calls::DynamicCall {
                                    interface: 99,
                                    method: 12,
                                    ..Default::default()
                                },
                                |_| Ok(()),
                            )
                        },
                    )
                    .unwrap();
                drain(&mut queue, messages, |_| ());
                let returned = Frame::build(messages, 512, Default::default(), |builder| {
                    let mut root = builder.try_init_root::<Message>()?;
                    let mut returned = root.field(Message::RETURN).init()?;
                    returned
                        .scalar(Return::ANSWER_ID)
                        .set(question.id().to_wire());
                    returned
                        .scalar(Return::TAKE_FROM_OTHER_QUESTION)
                        .set(call.answer().id().to_wire());
                    Ok(())
                })
                .unwrap();
                driver
                    .receive(returned, messages, &mut [0; 8], |_| (), || 0)
                    .unwrap_or_else(|_| panic!("adopted return"));
                assert!(
                    matches!(
                        driver.take_output(question, messages),
                        Err(Error::PendingResponse)
                    ),
                    "adoption is not an application response before local completion"
                );
                let result = if failed {
                    Err(capnp::ErrorKind::Overloaded)
                } else {
                    Ok(driver
                        .local_result_dynamic(
                            7,
                            9,
                            &[None, Some(&target)],
                            messages,
                            512,
                            Default::default(),
                            |field| field.set_capability_index(1),
                        )
                        .unwrap())
                };
                driver
                    .complete_answer(&call, result, messages)
                    .unwrap_or_else(|_| panic!("completed"));
                // Finish and release the original invocation before consuming
                // the result. The adoption lease alone must preserve authority.
                assert!(driver
                    .ledgers
                    .answers
                    .finish(call.answer().id(), false)
                    .retired
                    .is_none());
                driver.release_call(call, messages);
                let output = driver.take_output(question, messages).unwrap();
                if failed {
                    assert!(matches!(output, Err(capnp::ErrorKind::Overloaded)));
                } else {
                    let Ok(CallOutput::Local(result)) = output else {
                        panic!("local adopted payload")
                    };
                    result
                        .with(messages, |payload| {
                            assert_eq!((payload.interface, payload.method), (99, 12));
                            assert_eq!(payload.capabilities().len(), 2);
                            assert!(payload.capabilities()[0].is_none());
                            assert_eq!(payload.content().capability_index().unwrap(), 1);
                            assert_eq!(
                                payload.capabilities()[1].as_ref().unwrap().key(),
                                target.key()
                            );
                        })
                        .unwrap();
                    result.release(messages, &mut driver.runtime);
                }
                assert!(
                    driver.take_output(question, messages).is_err(),
                    "result is consumed once"
                );
                driver.runtime.release(target);
                close(driver, messages, &mut queue);
            });
        });
        assert_eq!(measured.count_total, 0);
    }
}

#[test]
fn completed_answer_retries_without_allocating_and_keeps_pipeline_after_finish() {
    for cancel in [false, true] {
        for redirected in [false, true] {
            let measured = allocation_counter::measure(|| {
                with_driver(|mut driver, messages| {
                    let (call, target) = request(&mut driver, messages, redirected);
                    let key = call.answer();
                    let parent = driver.ledgers.answers.pipeline(key.id()).unwrap();
                    let observer = driver
                        .pipelines
                        .bind(
                            &mut driver.runtime,
                            &mut driver.ledgers.answers,
                            &parent,
                            pipeline::Path::new(&[]),
                        )
                        .unwrap();
                    driver.ledgers.answers.release_pipeline_deferred(parent);
                    let result = driver
                        .local_result_dynamic(
                            7,
                            9,
                            &[None, Some(&target)],
                            messages,
                            512,
                            Default::default(),
                            |p| p.set_capability_index(1),
                        )
                        .unwrap();
                    driver
                        .complete_answer(&call, Ok(result), messages)
                        .unwrap_or_else(|_| panic!("completion"));
                    let rejected = driver
                        .complete_answer(&call, Err(capnp::ErrorKind::Failed), messages)
                        .err()
                        .unwrap();
                    assert_eq!(
                        rejected.error,
                        Error::Pipeline(pipelines::Error::AlreadyCompleted)
                    );
                    let mut slots = [None];
                    let mut queue =
                        messages::Queue::<crate::attachments::Sender<u32, 2>>::with_resources(
                            &mut slots,
                        )
                        .unwrap();
                    let blocker = Frame::build(messages, 512, Default::default(), |b| {
                        calls::acknowledge(b, capntproto_rpc_engine::ids::AnswerId::from_wire(99))
                    })
                    .unwrap();
                    let offer = messages.offer(&blocker.lease).unwrap();
                    queue.reserve().unwrap().accept(messages, offer);
                    blocker.release(messages);
                    assert!(driver
                        .send_answer(
                            key,
                            messages,
                            &mut queue,
                            512,
                            Default::default(),
                            &mut [0; 8],
                            |_, _| Ok(())
                        )
                        .is_err());
                    assert!(driver
                        .ledgers
                        .answers
                        .get(key)
                        .unwrap()
                        .with_result(messages, |_| ())
                        .unwrap()
                        .is_none());
                    drain(&mut queue, messages, |_| ());
                    // Failure after grants were staged must preserve the result and
                    // leave Return owed, just like failure before reservation.
                    if !redirected {
                        assert!(driver
                            .send_answer(
                                key,
                                messages,
                                &mut queue,
                                512,
                                Default::default(),
                                &mut [],
                                |_, _| Ok(())
                            )
                            .is_err());
                    }
                    if cancel {
                        assert!(driver
                            .ledgers
                            .answers
                            .finish(key.id(), false)
                            .retired
                            .is_none());
                    }
                    assert!(driver
                        .send_answer_with_attachments(
                            key,
                            messages,
                            &mut queue,
                            512,
                            Default::default(),
                            &mut [0; 8],
                            Capacity::Inline,
                            |_| None,
                            |owner| Some(*owner),
                            |_, _| Ok(())
                        )
                        .unwrap()
                        .is_none());
                    {
                        let window = queue.window(messages).unwrap().unwrap();
                        let resources = window.resources().unwrap();
                        assert_eq!(resources.len(), usize::from(!cancel && !redirected));
                        if !cancel && !redirected {
                            assert_eq!(resources.iter().copied().next(), Some(91));
                        }
                    }
                    drain(&mut queue, messages, |root| {
                        let rpc_capnp::message::Return(returned) = root.which().unwrap() else {
                            panic!("return")
                        };
                        let returned = returned.unwrap();
                        assert_eq!(returned.get_answer_id(), 3);
                        match returned.which().unwrap() {
                            rpc_capnp::return_::Canceled(()) => assert!(cancel),
                            rpc_capnp::return_::ResultsSentElsewhere(()) => {
                                assert!(redirected && !cancel)
                            }
                            rpc_capnp::return_::Results(payload) => {
                                assert!(!cancel && !redirected);
                                let payload = payload.unwrap();
                                assert_eq!(payload.get_content().capability_index().unwrap(), 1);
                                let caps = payload.get_cap_table().unwrap();
                                assert_eq!(caps.len(), 2);
                                assert!(matches!(
                                    caps.get(0).which().unwrap(),
                                    rpc_capnp::cap_descriptor::None(())
                                ));
                                assert_eq!(caps.get(1).get_attached_fd(), 0);
                            }
                            _ => panic!("wrong return"),
                        }
                    });
                    assert_eq!(
                        driver.poll_pipeline(0, messages).unwrap(),
                        Propagation::Resolved
                    );
                    assert!(matches!(
                        driver.runtime.inspect(&observer).unwrap(),
                        clients::Destination::Endpoint {
                            owner: binding::Route::Capability(binding::Endpoint::Local(91)),
                            ..
                        }
                    ));
                    driver.runtime.release(observer);
                    driver.release_call(call, messages);
                    driver.runtime.release(target);
                    close(driver, messages, &mut queue);
                })
            });
            assert_eq!(measured.count_total, 0);
        }
    }
}

#[test]
fn wrong_method_and_exception_encoder_failure_preserve_completion() {
    with_driver(|mut driver, messages| {
        let (call, target) = request(&mut driver, messages, false);
        let key = call.answer();
        let result = driver
            .local_result_dynamic(7, 10, &[], messages, 512, Default::default(), |_| Ok(()))
            .unwrap();
        let rejected = driver
            .complete_answer(&call, Ok(result), messages)
            .err()
            .unwrap();
        assert_eq!(
            rejected.error,
            Error::Input(crate::owned::Error::WrongMessage)
        );
        rejected
            .result
            .unwrap_or_else(|_| panic!("preserved result"))
            .release(messages, &mut driver.runtime);
        driver
            .complete_answer(&call, Err(capnp::ErrorKind::Overloaded), messages)
            .unwrap_or_else(|_| panic!("completion"));
        let mut slots = [None];
        let mut queue = messages::Queue::new(&mut slots).unwrap();
        assert!(driver
            .send_answer(
                key,
                messages,
                &mut queue,
                512,
                Default::default(),
                &mut [],
                |_, _| Err(capnp::Error::from_kind(capnp::ErrorKind::Failed))
            )
            .is_err());
        assert!(queue.is_empty());
        driver
            .send_answer(
                key,
                messages,
                &mut queue,
                512,
                Default::default(),
                &mut [],
                |producer, mut exception| {
                    assert_eq!(*producer, 73);
                    exception.field(Exception::REASON).copy_from("retry me")
                },
            )
            .unwrap();
        drain(&mut queue, messages, |root| {
            let rpc_capnp::message::Return(returned) = root.which().unwrap() else {
                panic!()
            };
            let rpc_capnp::return_::Exception(exception) = returned.unwrap().which().unwrap()
            else {
                panic!()
            };
            let exception = exception.unwrap();
            assert_eq!(
                exception.get_type().unwrap(),
                rpc_capnp::exception::Type::Overloaded
            );
            assert_eq!(
                exception.get_reason().unwrap().to_str().unwrap(),
                "retry me"
            );
        });
        driver.release_call(call, messages);
        driver.runtime.release(target);
        close(driver, messages, &mut queue);
    });
}

#[test]
fn early_pipeline_retry_is_transactional_and_survives_final_failure() {
    with_driver(|mut driver, messages| {
        let (call, target) = request(&mut driver, messages, false);
        let parent = driver.ledgers.answers.pipeline(call.answer().id()).unwrap();
        let observer = driver
            .pipelines
            .bind(
                &mut driver.runtime,
                &mut driver.ledgers.answers,
                &parent,
                pipeline::Path::new(&[2, 3]),
            )
            .unwrap();
        driver.ledgers.answers.release_pipeline_deferred(parent);
        assert!(driver
            .poll_pipeline_with(0, messages, |_, _, _| Err(pipelines::Error::Capacity))
            .is_err());
        assert_eq!(
            driver
                .poll_pipeline_with(0, messages, |_, _, _| Ok(None))
                .unwrap(),
            Propagation::Pending
        );
        driver
            .complete_answer(&call, Err(capnp::ErrorKind::Overloaded), messages)
            .unwrap_or_else(|_| panic!("completion"));
        assert_eq!(
            driver
                .poll_pipeline_with(0, messages, |producer, path, graph| {
                    assert_eq!(*producer, 73);
                    assert_eq!(path.fields(), [2, 3]);
                    graph
                        .retain(&target)
                        .map(|c| Some(Ok(c)))
                        .map_err(pipelines::Error::Graph)
                })
                .unwrap(),
            Propagation::Resolved
        );
        assert!(matches!(
            driver.runtime.inspect(&observer).unwrap(),
            clients::Destination::Endpoint {
                location: clients::Location::Local,
                ..
            }
        ));
        driver.runtime.release(observer);
        driver.release_call(call, messages);
        driver.runtime.release(target);
        let mut slots = [None];
        close(
            driver,
            messages,
            &mut messages::Queue::new(&mut slots).unwrap(),
        );
    });
}

#[test]
fn endpoint_retirement_charges_each_owner_and_preserves_fair_selection() {
    with_driver(|mut driver, messages| {
        let mut slots = [None];
        let mut queue = messages::Queue::new(&mut slots).unwrap();
        for _ in 0..10 {
            let client = driver
                .runtime
                .endpoint(
                    binding::Route::Capability(crate::clients::Endpoint::Broken(
                        capnp::ErrorKind::Failed,
                    )),
                    clients::Location::Broken,
                )
                .unwrap();
            driver.runtime.release(client);
        }
        driver.work_cursor = 5;
        let mut remaining = 8;
        macro_rules! poll {
            () => {
                driver
                    .poll_maintenance_budget_with(
                        messages,
                        &mut queue,
                        512,
                        Default::default(),
                        &mut [0; 8],
                        &mut remaining,
                        &mut || (),
                        &mut |_, _, _| 255,
                        &mut |_, _, _| Ok(None),
                    )
                    .unwrap()
            };
        }
        assert!(matches!(poll!(), Progress::Advanced));
        assert_eq!(remaining, 0);
        assert_eq!(driver.work_cursor, 6);
        assert!(driver.runtime.has_retired());
        remaining = 1;
        assert!(matches!(poll!(), Progress::Advanced));
        assert_eq!(remaining, 0);
        assert_eq!(
            driver.work_cursor, 6,
            "the next rotation skips empty phases and charges one retired owner"
        );
        // The first burst freed exactly eight owners. A single subsequent
        // step frees one more; it cannot drain both remaining owners.
        assert!(driver.drain_endpoint().is_some());
        assert!(driver.drain_endpoint().is_none());
        assert!(queue.is_empty());
        close(driver, messages, &mut queue);
    });
}

#[test]
fn productive_maintenance_preserves_unused_budget_for_following_cleanup() {
    with_driver(|mut driver, messages| {
        let mut slots = [None];
        let mut queue = messages::Queue::new(&mut slots).unwrap();
        let mut remaining = 0;
        macro_rules! poll {
            () => {
                driver
                    .poll_maintenance_budget_with(
                        messages,
                        &mut queue,
                        512,
                        Default::default(),
                        &mut [0; 8],
                        &mut remaining,
                        &mut || (),
                        &mut |_, _, _| 255,
                        &mut |_, _, _| Ok(None),
                    )
                    .unwrap()
            };
        }
        assert!(matches!(poll!(), Progress::Yield));
        assert_eq!(remaining, 0);
        remaining = 1;
        assert!(matches!(poll!(), Progress::Idle));
        assert_eq!(remaining, 0);

        for owner in [91, 92] {
            let client = driver
                .runtime
                .endpoint(binding::Route::local(owner), clients::Location::Local)
                .unwrap();
            driver.runtime.release(client);
        }
        driver.work_cursor = 5;
        remaining = 128;
        let Progress::Server(first) = poll!() else {
            panic!("first retired endpoint")
        };
        assert_eq!(
            remaining, 127,
            "a productive step must not consume the whole allowance"
        );
        let Progress::Server(second) = poll!() else {
            panic!("second retired endpoint")
        };
        assert!(remaining > 0 && remaining < 127);
        let mut owners = [first, second];
        owners.sort_unstable();
        assert_eq!(owners, [91, 92]);
        let before_idle = remaining;
        assert!(matches!(poll!(), Progress::Idle));
        assert_eq!(remaining, before_idle - 1);
        assert!(!driver.runtime.has_retired());
        assert!(queue.is_empty());
        close(driver, messages, &mut queue);
    });
}

#[test]
fn pending_pipeline_callback_exposes_new_cleanup_in_the_same_maintenance_poll() {
    with_driver(|mut driver, messages| {
        let (call, target) = request(&mut driver, messages, false);
        let parent = driver.ledgers.answers.pipeline(call.answer().id()).unwrap();
        let observer = driver
            .pipelines
            .bind(
                &mut driver.runtime,
                &mut driver.ledgers.answers,
                &parent,
                pipeline::Path::new(&[]),
            )
            .unwrap();
        driver.ledgers.answers.release_pipeline_deferred(parent);
        let mut retiring = Some(
            driver
                .runtime
                .endpoint(binding::Route::local(99), clients::Location::Local)
                .unwrap(),
        );
        assert!(!driver.runtime.has_retired());
        driver.work_cursor = 2;
        let mut slots = [None];
        let mut queue = messages::Queue::new(&mut slots).unwrap();
        let outcome = driver
            .poll_maintenance_with(
                messages,
                &mut queue,
                512,
                Default::default(),
                &mut [0; 8],
                NonZeroUsize::new(32).unwrap(),
                &mut || (),
                &mut |_, _, _| 255,
                &mut |_, _, graph| {
                    graph.release(retiring.take().unwrap());
                    Ok(None)
                },
            )
            .unwrap();
        assert!(retiring.is_none());
        assert!(matches!(outcome, Progress::Server(99)));
        driver.runtime.release(observer);
        driver.release_call(call, messages);
        driver.runtime.release(target);
        close(driver, messages, &mut queue);
    });
}

#[test]
fn maintenance_uses_early_pipeline_before_failed_result() {
    with_driver(|mut driver, messages| {
        let (call, target) = request(&mut driver, messages, false);
        let parent = driver.ledgers.answers.pipeline(call.answer().id()).unwrap();
        let observer = driver
            .pipelines
            .bind(
                &mut driver.runtime,
                &mut driver.ledgers.answers,
                &parent,
                pipeline::Path::new(&[]),
            )
            .unwrap();
        driver.ledgers.answers.release_pipeline_deferred(parent);
        driver
            .complete_answer(&call, Err(capnp::ErrorKind::Failed), messages)
            .unwrap_or_else(|_| panic!());
        let mut slots = [None];
        let mut queue = messages::Queue::new(&mut slots).unwrap();
        let mut invoked = false;
        for _ in 0..128 {
            driver
                .poll_maintenance_with(
                    messages,
                    &mut queue,
                    512,
                    Default::default(),
                    &mut [0; 8],
                    NonZeroUsize::new(1).unwrap(),
                    &mut || (),
                    &mut |_, _, _| 255,
                    &mut |_, _, graph| {
                        invoked = true;
                        graph
                            .retain(&target)
                            .map(|c| Some(Ok(c)))
                            .map_err(pipelines::Error::Graph)
                    },
                )
                .unwrap();
            if invoked {
                break;
            }
        }
        assert!(invoked);
        assert!(matches!(
            driver.runtime.inspect(&observer).unwrap(),
            clients::Destination::Endpoint {
                location: clients::Location::Local,
                ..
            }
        ));
        driver.runtime.release(observer);
        driver.release_call(call, messages);
        driver.runtime.release(target);
        close(driver, messages, &mut queue);
    });
}

#[test]
fn bootstrap_completion_retries_and_preserves_pipelines_after_finish_without_allocation() {
    for failed in [false, true] {
        for finished in [false, true] {
            let measured = allocation_counter::measure(|| {
                with_driver(|mut driver, messages| {
                    let target = driver
                        .runtime
                        .endpoint(binding::Route::local(91), clients::Location::Local)
                        .unwrap();
                    let frame = Frame::build(messages, 512, Default::default(), |b| {
                        calls::bootstrap(b, QuestionId::from_wire(3))
                    })
                    .unwrap();
                    let Received::Bootstrap(bootstrap) = driver
                        .receive(frame, messages, &mut [0; 8], |_| (), || 73)
                        .unwrap_or_else(|e| panic!("{}", e.error))
                    else {
                        panic!("bootstrap")
                    };
                    let key = bootstrap.answer();
                    let parent = driver.ledgers.answers.pipeline(key.id()).unwrap();
                    let observer = driver
                        .pipelines
                        .bind(
                            &mut driver.runtime,
                            &mut driver.ledgers.answers,
                            &parent,
                            pipeline::Path::new(&[]),
                        )
                        .unwrap();
                    driver.ledgers.answers.release_pipeline_deferred(parent);
                    // Exhausted storage rolls back completion, including its clients.
                    if !failed {
                        let held =
                            core::array::from_fn::<_, 6, _>(|_| messages.reserve(1024).unwrap());
                        assert!(driver
                            .complete_bootstrap(
                                &bootstrap,
                                Ok(&target),
                                messages,
                                512,
                                Default::default()
                            )
                            .is_err());
                        for writer in held {
                            messages.discard(writer);
                        }
                    }
                    driver
                        .complete_bootstrap(
                            &bootstrap,
                            if failed {
                                Err(capnp::ErrorKind::Overloaded)
                            } else {
                                Ok(&target)
                            },
                            messages,
                            512,
                            Default::default(),
                        )
                        .unwrap();
                    assert_eq!(
                        driver.complete_bootstrap(
                            &bootstrap,
                            Err(capnp::ErrorKind::Failed),
                            messages,
                            512,
                            Default::default()
                        ),
                        Err(Error::Pipeline(pipelines::Error::AlreadyCompleted))
                    );
                    let mut slots = [None];
                    let mut queue = messages::Queue::new(&mut slots).unwrap();
                    let blocker = Frame::build(messages, 512, Default::default(), |b| {
                        calls::acknowledge(b, key.id())
                    })
                    .unwrap();
                    let offer = messages.offer(&blocker.lease).unwrap();
                    queue.reserve().unwrap().accept(messages, offer);
                    blocker.release(messages);
                    assert!(driver
                        .send_answer(
                            key,
                            messages,
                            &mut queue,
                            512,
                            Default::default(),
                            &mut [0; 8],
                            |_, _| Ok(())
                        )
                        .is_err());
                    drain(&mut queue, messages, |_| ());
                    if finished {
                        assert!(driver
                            .ledgers
                            .answers
                            .finish(key.id(), false)
                            .retired
                            .is_none());
                    }
                    driver
                        .send_answer(
                            key,
                            messages,
                            &mut queue,
                            512,
                            Default::default(),
                            &mut [0; 8],
                            |producer, mut exception| {
                                assert_eq!(*producer, 73);
                                exception
                                    .field(Exception::REASON)
                                    .copy_from("bootstrap refused")
                            },
                        )
                        .unwrap();
                    drain(&mut queue, messages, |root| {
                        let rpc_capnp::message::Return(returned) = root.which().unwrap() else {
                            panic!("return")
                        };
                        let returned = returned.unwrap();
                        assert_eq!(returned.get_answer_id(), 3);
                        match returned.which().unwrap() {
                            rpc_capnp::return_::Canceled(()) => assert!(finished),
                            rpc_capnp::return_::Exception(exception) => {
                                assert!(failed && !finished);
                                assert_eq!(
                                    exception.unwrap().get_reason().unwrap().to_str().unwrap(),
                                    "bootstrap refused"
                                );
                            }
                            rpc_capnp::return_::Results(payload) => {
                                assert!(!failed && !finished);
                                let payload = payload.unwrap();
                                assert_eq!(payload.get_content().capability_index().unwrap(), 0);
                                assert_eq!(payload.get_cap_table().unwrap().len(), 1);
                            }
                            _ => panic!("wrong return"),
                        }
                    });
                    assert_eq!(
                        driver.poll_pipeline(0, messages).unwrap(),
                        Propagation::Resolved
                    );
                    match driver.runtime.inspect(&observer).unwrap() {
                        clients::Destination::Endpoint {
                            owner: binding::Route::Capability(binding::Endpoint::Local(91)),
                            ..
                        } => assert!(!failed),
                        clients::Destination::Endpoint {
                            owner:
                                binding::Route::Capability(binding::Endpoint::Broken(
                                    capnp::ErrorKind::Overloaded,
                                )),
                            ..
                        } => assert!(failed),
                        _ => panic!("pipeline"),
                    }
                    // Finish permits wire-ID reuse but cannot redirect the old token.
                    if finished {
                        let frame = Frame::build(messages, 512, Default::default(), |b| {
                            calls::bootstrap(b, QuestionId::from_wire(3))
                        })
                        .unwrap();
                        let Received::Bootstrap(new) = driver
                            .receive(frame, messages, &mut [0; 8], |_| (), || 74)
                            .unwrap_or_else(|e| panic!("{}", e.error))
                        else {
                            panic!("bootstrap")
                        };
                        assert_ne!(key, new.answer());
                        assert!(driver
                            .complete_bootstrap(
                                &bootstrap,
                                Ok(&target),
                                messages,
                                512,
                                Default::default()
                            )
                            .is_err());
                        driver
                            .complete_bootstrap(
                                &new,
                                Ok(&target),
                                messages,
                                512,
                                Default::default(),
                            )
                            .unwrap();
                    }
                    driver.runtime.release(observer);
                    driver.runtime.release(target);
                    close(driver, messages, &mut queue);
                })
            });
            assert_eq!(measured.count_total, 0);
        }
    }
}

#[test]
fn empty_work_classes_do_not_delay_the_next_owned_operation() {
    for phase in 0..16 {
        with_driver(|mut driver, messages| {
            let client = driver
                .runtime
                .endpoint(binding::Route::local(99), clients::Location::Local)
                .unwrap();
            driver.runtime.release(client);
            driver.work_cursor = phase;
            let mut slots = [None];
            let mut queue = messages::Queue::new(&mut slots).unwrap();
            let mut remaining = 1;
            let outcome = driver
                .poll_maintenance_budget_with(
                    messages,
                    &mut queue,
                    512,
                    Default::default(),
                    &mut [0; 8],
                    &mut remaining,
                    &mut || (),
                    &mut |_, _, _| 255,
                    &mut |_, _, _| Ok(None),
                )
                .unwrap();
            assert!(
                matches!(outcome, Progress::Server(99)),
                "starting phase {phase}"
            );
            assert_eq!(remaining, 0);
            assert_eq!(driver.work_cursor, 6);
            assert!(!driver.runtime.has_retired());
            close(driver, messages, &mut queue);
        });
    }
}
