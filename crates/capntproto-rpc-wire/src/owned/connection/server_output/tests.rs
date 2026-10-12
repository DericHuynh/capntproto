use super::*;
use capnp::{word, Word};
use capntproto_rpc_engine::{connection as storage, ids::QuestionId};
use core::num::NonZeroUsize;
const ZERO: Word = word(0, 0, 0, 0, 0, 0, 0, 0);
type Peer<'p, 'id, 's> = Driver<'p, 'id, 's, (), u32, u8, u32, 4, 4>;

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
        source
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
                #[cfg(not(feature = "alloc"))]
                let result = None;
                let result = result.unwrap_or_else(|| {
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
                });
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
