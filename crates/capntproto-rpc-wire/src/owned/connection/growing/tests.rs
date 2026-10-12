use super::*;
use crate::{
    calls::DynamicCall,
    rpc_capnp::api::{Call as WireCall, CapDescriptor, Message, Payload, Return},
};
use capntproto_rpc_engine::clients::Capabilities;
use capntproto_rpc_engine::ids::{QuestionId, WireId};
use core::num::NonZeroUsize;

type Peer<'p, 'id, 's> = Driver<'p, 'id, 's, (), (), u8, u32, 0, 0>;
fn with_driver<R>(
    f: impl for<'p, 'id, 's, 'm, 'b> FnOnce(Peer<'p, 'id, 's>, &mut messages::Messages<'p, 'm, 'b>) -> R,
) -> R {
    let mut buffers = messages::GrowingBuffers::new(32, 4096, 32 * 4103);
    messages::with_growing_messages(
        &mut buffers,
        connection::Growth {
            initial: 0,
            maximum: 8,
        },
        |messages| {
            connection::with_domain(|domain| {
                let bound = connection::Growth {
                    initial: 0,
                    maximum: 32,
                };
                let tables = connection::GrowingStorage {
                    imports: bound,
                    exports: bound,
                    questions: bound,
                    answers: bound,
                    clients: bound,
                };
                let graph = domain.bind_growing_adoptions(tables, bound).unwrap();
                let mut storage = GrowingStorage::uniform(0, 32);
                storage.message_capabilities = Capacity::Growing { maximum: 7 };
                storage.message_path = Capacity::Growing { maximum: 8 };
                let driver =
                    Driver::new_growing(graph, storage, 1).unwrap_or_else(|_| panic!("driver"));
                f(driver, messages)
            })
        },
    )
    .unwrap()
}
fn close<'p>(
    driver: Peer<'p, '_, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_>,
) {
    let mut closing = driver.terminate(queue);
    for _ in 0..2048 {
        if matches!(
            closing
                .poll(messages, NonZeroUsize::new(16).unwrap())
                .unwrap(),
            super::super::Termination::Done
        ) {
            return;
        }
    }
    panic!("terminal cleanup did not finish")
}
fn remote<'p, 'id>(driver: &mut Peer<'p, 'id, '_>) -> clients::Client<'id> {
    let mut slots = [imports::ReceiveSlot::new(ImportId::from_wire(41), ())];
    driver
        .ledgers
        .imports
        .prepare_receive(&mut slots)
        .unwrap()
        .commit();
    let receipt = slots[0].take_local().unwrap();
    let client = driver
        .imports
        .bind(&mut driver.runtime, &mut driver.ledgers.imports, &receipt)
        .unwrap();
    driver.ledgers.imports.release(receipt);
    client
}

#[test]
fn idle_certificate_observes_new_retirement_without_invalidation() {
    with_driver(|mut driver, messages| {
        let mut queue = messages::Queue::new_growing(0, 8).unwrap();
        macro_rules! poll {
            () => {
                driver
                    .poll_maintenance(
                        messages,
                        &mut queue,
                        128,
                        Default::default(),
                        &mut [0; 8],
                        NonZeroUsize::new(1).unwrap(),
                    )
                    .unwrap()
            };
        }
        assert!(matches!(poll!(), Progress::Idle));
        let client = remote(&mut driver);
        assert!(matches!(poll!(), Progress::Idle));
        driver.runtime.release(client);
        let mut released = false;
        for _ in 0..128 {
            match poll!() {
                Progress::Import(()) => released = true,
                Progress::Idle => {
                    assert!(released);
                    break;
                }
                _ => (),
            }
        }
        assert!(released);
        assert!(driver.ledgers.imports.is_empty());
        assert_eq!(queue.len(), 1);
        close(driver, messages, &mut queue);
    });
}

#[test]
fn maintenance_idle_scan_keeps_its_budget_after_empty_tables_grow() {
    with_driver(|mut driver, messages| {
        // Exercise the general scanner even when the hosted simple-client
        // certificate could otherwise prove quiescence without a scan.
        assert!(driver.tails.grow(1, || TailSlot::EMPTY));
        let mut queue = messages::Queue::new_growing(0, 8).unwrap();
        macro_rules! poll {
            () => {
                driver
                    .poll_maintenance(
                        messages,
                        &mut queue,
                        128,
                        Default::default(),
                        &mut [0; 8],
                        NonZeroUsize::new(1).unwrap(),
                    )
                    .unwrap()
            };
        }
        for _ in 1..18 {
            assert!(matches!(poll!(), Progress::Yield));
        }
        assert!(matches!(poll!(), Progress::Idle));
        let mut receipts = core::array::from_fn::<_, 7, _>(|index| {
            imports::ReceiveSlot::new(ImportId::from_wire(index as u32), ())
        });
        driver
            .ledgers
            .imports
            .prepare_receive(&mut receipts)
            .unwrap()
            .commit();
        driver.invalidate_maintenance();
        let total = 17 + driver.ledgers.imports.capacity();
        for _ in 1..total {
            assert!(matches!(poll!(), Progress::Yield));
        }
        assert!(matches!(poll!(), Progress::Idle));
        assert_eq!(driver.maintenance_total, total);
        for receipt in &mut receipts {
            driver
                .ledgers
                .imports
                .release(receipt.take_local().unwrap());
        }
        close(driver, messages, &mut queue);
    });
}

fn drain<'p>(
    queue: &mut messages::Queue<'p, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    f: impl FnOnce(rpc_capnp::message::Reader<'_>),
) {
    let mut words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 512];
    let bytes = capnp::Word::words_to_bytes_mut(&mut words);
    let mut used = 0;
    while let Some(window) = queue.window(messages).unwrap() {
        let n = window.bytes().len();
        bytes[used..used + n].copy_from_slice(window.bytes());
        used += n;
        window.advance(n).unwrap();
    }
    let mut source = &bytes[..used];
    let reader =
        capnp::serialize::read_message_from_flat_slice_no_alloc(&mut source, Default::default())
            .unwrap();
    assert!(source.is_empty());
    f(reader.get_root().unwrap());
}

#[test]
fn data_template_revalidates_changed_reader_limits_and_preserves_retry() {
    with_driver(|mut driver, messages| {
        let target = remote(&mut driver);
        let task = driver
            .enqueue_dynamic(
                &target,
                DynamicCall::default(),
                &[],
                messages,
                512,
                Default::default(),
                |p| {
                    p.cast::<capnp::field_api::Text>()
                        .copy_from("validated payload")
                },
            )
            .unwrap();
        let mut queue = messages::Queue::new_growing(0, 8).unwrap();
        let strict = capnp::message::ReaderOptions {
            nesting_limit: 0,
            ..Default::default()
        };
        assert!(driver
            .send_invocation(messages, &mut queue, 1024, strict, &mut [0; 8])
            .is_err());
        assert!(queue.is_empty() && driver.ledgers.questions.is_empty());
        let sent = driver
            .send_invocation(messages, &mut queue, 1024, Default::default(), &mut [0; 8])
            .unwrap()
            .unwrap();
        assert_eq!(sent.task, task);
        drain(&mut queue, messages, |root| {
            let rpc_capnp::message::Call(call) = root.which().unwrap() else {
                panic!("call");
            };
            let call = call.unwrap();
            assert_eq!(call.get_question_id(), sent.question.id().to_wire());
            assert!(!call.get_only_promise_pipeline());
            assert_eq!(
                call.get_params()
                    .unwrap()
                    .get_content()
                    .get_as::<capnp::text::Reader>()
                    .unwrap(),
                "validated payload"
            );
        });
        driver.runtime.release(target);
        close(driver, messages, &mut queue);
    });
}

#[test]
fn zero_inline_driver_sends_large_cap_tables_paths_and_admits_returns() {
    with_driver(|mut driver, messages| {
        let target = remote(&mut driver);
        let value = driver
            .runtime
            .endpoint(binding::Route::local(9), clients::Location::Local)
            .unwrap();
        let caps = [None, Some(&value), None, Some(&value), None, Some(&value)];
        let task = driver
            .enqueue_dynamic(
                &target,
                DynamicCall::default(),
                &caps,
                messages,
                512,
                Default::default(),
                |p| p.set_capability_index(5),
            )
            .unwrap();
        let mut queue = messages::Queue::new_growing(0, 8).unwrap();
        let sent = driver
            .send_invocation(messages, &mut queue, 1024, Default::default(), &mut [0; 8])
            .unwrap()
            .unwrap();
        assert_eq!(sent.task, task);
        drain(&mut queue, messages, |root| {
            let rpc_capnp::message::Call(call) = root.which().unwrap() else {
                panic!("call")
            };
            let p = call.unwrap().get_params().unwrap();
            assert_eq!(p.get_cap_table().unwrap().len(), 6);
            assert_eq!(p.get_content().capability_index().unwrap(), 5);
            assert!(matches!(
                p.get_cap_table().unwrap().get(0).which().unwrap(),
                rpc_capnp::cap_descriptor::None(())
            ));
        });
        let path = [1, 2, 3, 4, 5, 6, 7, 8];
        assert!(driver
            .pipeline(sent.question, pipeline::Path::new(&[0; 9]))
            .is_err());
        let child = driver
            .pipeline(sent.question, pipeline::Path::new(&path))
            .unwrap();
        let alias = driver
            .pipeline(sent.question, pipeline::Path::new(&path))
            .unwrap();
        assert_eq!(child.key(), alias.key());
        driver
            .enqueue_dynamic(
                &child,
                DynamicCall::default(),
                &[],
                messages,
                512,
                Default::default(),
                |_| Ok(()),
            )
            .unwrap();
        let second = driver
            .send_invocation(messages, &mut queue, 1024, Default::default(), &mut [0; 8])
            .unwrap()
            .unwrap();
        drain(&mut queue, messages, |root| {
            let rpc_capnp::message::Call(call) = root.which().unwrap() else {
                panic!("call")
            };
            let rpc_capnp::message_target::PromisedAnswer(answer) =
                call.unwrap().get_target().unwrap().which().unwrap()
            else {
                panic!("pipeline")
            };
            let answer = answer.unwrap();
            let mut decoded = [0; 8];
            assert_eq!(
                pipeline::Path::decode(answer, &mut decoded)
                    .unwrap()
                    .fields(),
                &path
            );
        });
        let frame = Frame::build(messages, 1024, Default::default(), |builder| {
            calls::dynamic_results(
                builder,
                AnswerId::from_wire(second.question.id().to_wire()),
                |p| p.set_capability_index(5),
            )?;
            let mut root = builder.try_get_root::<Message>()?;
            let mut result = root.field(Message::RETURN).edit()?;
            result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
            let mut payload = result.field(Return::RESULTS).edit()?;
            let mut table = payload.field(Payload::CAP_TABLE).init(6)?;
            for index in 0..6 {
                table
                    .get_mut(index)
                    .unwrap()
                    .scalar(CapDescriptor::SENDER_HOSTED)
                    .set(99);
            }
            Ok(())
        })
        .unwrap();
        let mut made = 0;
        driver
            .receive(
                frame,
                messages,
                &mut [0; 8],
                |_| {
                    made += 1;
                },
                || (),
            )
            .unwrap_or_else(|r| panic!("{}", r.error));
        assert_eq!(made, 1);
        let response = driver.take_response(second.question).unwrap();
        response
            .with(messages, |reply| {
                let super::super::super::Reply::Results(payload) = reply else {
                    panic!("results")
                };
                assert_eq!(payload.capability_count(), 6);
                assert!(payload.capability_at(5).unwrap().is_some());
            })
            .unwrap();
        driver.release_response(response, messages);
        driver.runtime.release(alias);
        driver.runtime.release(child);
        driver.runtime.release(value);
        driver.runtime.release(target);
        close(driver, messages, &mut queue);
    });
}

#[test]
fn incoming_growth_rejection_preserves_frame_and_does_not_make_import_owners() {
    with_driver(|mut driver, messages| {
        let target = driver
            .runtime
            .endpoint(binding::Route::local(9), clients::Location::Local)
            .unwrap();
        let export = {
            let mut batch = driver.ledgers.exports.batch();
            let grant = crate::clients::stage(&mut driver.runtime, &mut batch, &target).unwrap();
            batch.commit();
            grant.id()
        };
        let frame = Frame::build(messages, 1024, Default::default(), |builder| {
            calls::dynamic_call(
                builder,
                QuestionId::from_wire(9),
                calls::Target::Imported(ImportId::from_wire(export.to_wire())),
                DynamicCall::default(),
                |p| p.set_capability_index(6),
            )?;
            let mut root = builder.try_get_root::<Message>()?;
            let mut call = root.field(Message::CALL).edit()?;
            let mut payload = call.field(WireCall::PARAMS).edit()?;
            let mut table = payload.field(Payload::CAP_TABLE).init(7)?;
            for index in 0..7 {
                table
                    .get_mut(index)
                    .unwrap()
                    .scalar(CapDescriptor::SENDER_HOSTED)
                    .set(99);
            }
            Ok(())
        })
        .unwrap();
        let mut made = 0;
        let rejected = driver
            .receive(
                frame,
                messages,
                &mut [0; 8],
                |_| {
                    made += 1;
                },
                || (),
            )
            .err()
            .unwrap();
        assert_eq!(made, 0, "the target receipt also needs capacity");
        assert_eq!(
            rejected.error,
            Error::Input(super::super::super::Error::Capacity)
        );
        let call = Request::<_, 0, 0>::admit_with_capacity(
            rejected.frame,
            messages,
            &mut driver.ledgers.imports,
            &mut driver.ledgers.exports,
            &mut driver.ledgers.answers,
            Answer::new(pipelines::Results::new(())),
            |_| {
                made += 1;
            },
            Capacity::Growing { maximum: 8 },
            Capacity::Growing { maximum: 8 },
        )
        .unwrap_or_else(|r| panic!("{}", r.error));
        assert_eq!(made, 1);
        call.with(messages, |view| assert_eq!(view.capability_count(), 7))
            .unwrap();
        driver.release_call(call, messages);
        driver.runtime.release(target);
        close(
            driver,
            messages,
            &mut messages::Queue::new_growing(0, 8).unwrap(),
        );
    });
}

#[test]
fn growing_result_draft_retains_owners_and_freezes_the_complete_table() {
    with_driver(|mut driver, messages| {
        let value = driver
            .runtime
            .endpoint(binding::Route::local(5), clients::Location::Local)
            .unwrap();
        let mut draft = LocalResultDraft::<0>::reserve(
            messages,
            1024,
            Default::default(),
            1,
            2,
            Capacity::Growing { maximum: 6 },
        )
        .unwrap();
        {
            let future = draft.write_async(messages, async |mut field, caps| {
                for _ in 0..6 {
                    let client = driver.runtime.retain(&value).unwrap();
                    caps.set(
                        field
                            .reborrow()
                            .cast::<capnp::field_api::WireCapability<()>>(),
                        client,
                    )
                    .unwrap_or_else(|_| panic!("insert"));
                }
                let client = driver.runtime.retain(&value).unwrap();
                let rejected = caps
                    .set(field.cast::<capnp::field_api::WireCapability<()>>(), client)
                    .err()
                    .unwrap();
                driver.runtime.release(rejected.client);
                assert_eq!(caps.len(), 6);
                Ok(())
            });
            let mut future = core::pin::pin!(future);
            use core::future::Future;
            assert!(matches!(
                future.as_mut().poll(&mut core::task::Context::from_waker(
                    core::task::Waker::noop()
                )),
                core::task::Poll::Ready(Ok(()))
            ));
        }
        let result = draft.finish(messages, &mut driver.runtime).unwrap();
        result
            .with(messages, |payload| {
                assert_eq!(payload.capabilities().len(), 6);
                assert_eq!(payload.content().capability_index().unwrap(), 5);
            })
            .unwrap();
        let frozen = pipelines::Published::from_local(
            &result,
            AnswerId::from_wire(3),
            messages,
            &mut driver.runtime,
        )
        .unwrap();
        result.release(messages, &mut driver.runtime);
        let bound = frozen
            .target(pipeline::Path::new(&[]), messages, &mut driver.runtime)
            .unwrap();
        assert_eq!(bound.key(), value.key());
        driver.runtime.release(bound);
        frozen.release(messages, &mut driver.runtime);
        driver.runtime.release(value);
        close(
            driver,
            messages,
            &mut messages::Queue::new_growing(0, 8).unwrap(),
        );
    });
}

#[test]
fn incoming_control_paths_and_reflected_descriptors_use_the_explicit_budget() {
    with_driver(|mut driver, messages| {
        use crate::rpc_capnp::api::{
            disembargo::Context, Disembargo, Join, MessageTarget, Provide,
        };
        let parent = driver
            .ledgers
            .answers
            .admit(
                AnswerId::from_wire(3),
                Answer::new(pipelines::Results::new(())),
            )
            .unwrap_or_else(|_| panic!("parent"));
        let path = [1, 2, 3, 4, 5, 6, 7, 8];
        for kind in 0..3 {
            let frame = Frame::build(messages, 1024, Default::default(), |builder| {
                let mut root = builder.try_init_root::<Message>()?;
                match kind {
                    0 => {
                        let mut provide = root.field(Message::PROVIDE).init()?;
                        provide.scalar(Provide::QUESTION_ID).set(4);
                        pipeline::Path::new(&path).write(
                            provide
                                .field(Provide::TARGET)
                                .init()?
                                .field(MessageTarget::PROMISED_ANSWER)
                                .init()?,
                            QuestionId::from_wire(3),
                        )?;
                    }
                    1 => {
                        let mut join = root.field(Message::JOIN).init()?;
                        join.scalar(Join::QUESTION_ID).set(5);
                        pipeline::Path::new(&path).write(
                            join.field(Join::TARGET)
                                .init()?
                                .field(MessageTarget::PROMISED_ANSWER)
                                .init()?,
                            QuestionId::from_wire(3),
                        )?;
                    }
                    _ => {
                        let mut echo = root.field(Message::DISEMBARGO).init()?;
                        echo.group(Disembargo::CONTEXT)
                            .select()
                            .scalar(Context::SENDER_LOOPBACK)
                            .set(7);
                        pipeline::Path::new(&path).write(
                            echo.field(Disembargo::TARGET)
                                .init()?
                                .field(MessageTarget::PROMISED_ANSWER)
                                .init()?,
                            QuestionId::from_wire(3),
                        )?;
                    }
                }
                Ok(())
            })
            .unwrap();
            // Each fixed zero-capacity admission rejects the same intact frame.
            let frame = match kind {
                0 => {
                    super::super::super::provision::Provide::<0>::admit(
                        frame,
                        messages,
                        &mut driver.ledgers.exports,
                        &mut driver.ledgers.answers,
                        Answer::new(pipelines::Results::new(())),
                    )
                    .err()
                    .unwrap()
                    .frame
                }
                1 => {
                    super::super::super::join::Join::<0>::admit(
                        frame,
                        messages,
                        &mut driver.ledgers.exports,
                        &mut driver.ledgers.answers,
                        Answer::new(pipelines::Results::new(())),
                    )
                    .err()
                    .unwrap()
                    .frame
                }
                _ => {
                    super::super::super::loopback::Echo::<0>::admit(
                        frame,
                        messages,
                        &mut driver.ledgers.exports,
                        &mut driver.ledgers.answers,
                    )
                    .err()
                    .unwrap()
                    .frame
                }
            };
            match kind {
                0 => {
                    let request =
                        super::super::super::provision::Provide::<0>::admit_with_capacity(
                            frame,
                            messages,
                            &mut driver.ledgers.exports,
                            &mut driver.ledgers.answers,
                            Answer::new(pipelines::Results::new(())),
                            Capacity::Growing { maximum: 8 },
                        )
                        .unwrap_or_else(|r| panic!("{}", r.error));
                    let TargetRef::Promised { path: actual, .. } = request.target() else {
                        panic!("path")
                    };
                    assert_eq!(actual.fields(), &path);
                    request.release(
                        messages,
                        &mut driver.ledgers.exports,
                        &mut driver.ledgers.answers,
                    );
                }
                1 => {
                    let request = super::super::super::join::Join::<0>::admit_with_capacity(
                        frame,
                        messages,
                        &mut driver.ledgers.exports,
                        &mut driver.ledgers.answers,
                        Answer::new(pipelines::Results::new(())),
                        Capacity::Growing { maximum: 8 },
                    )
                    .unwrap_or_else(|r| panic!("{}", r.error));
                    let TargetRef::Promised { path: actual, .. } = request.target() else {
                        panic!("path")
                    };
                    assert_eq!(actual.fields(), &path);
                    request.release(
                        messages,
                        &mut driver.ledgers.exports,
                        &mut driver.ledgers.answers,
                    );
                }
                _ => {
                    let echo = super::super::super::loopback::Echo::<0>::admit_with_capacity(
                        frame,
                        messages,
                        &mut driver.ledgers.exports,
                        &mut driver.ledgers.answers,
                        Capacity::Growing { maximum: 8 },
                    )
                    .unwrap_or_else(|r| panic!("{}", r.error));
                    let TargetRef::Promised { path: actual, .. } = echo.target() else {
                        panic!("path")
                    };
                    assert_eq!(actual.fields(), &path);
                    echo.release(&mut driver.ledgers.exports, &mut driver.ledgers.answers);
                }
            }
        }
        // A descriptor's transform is decoded while binding its exact receipt,
        // independently of the incoming Call's target transform.
        let target = driver
            .runtime
            .endpoint(binding::Route::local(8), clients::Location::Local)
            .unwrap();
        let export = {
            let mut batch = driver.ledgers.exports.batch();
            let grant = crate::clients::stage(&mut driver.runtime, &mut batch, &target).unwrap();
            batch.commit();
            grant.id()
        };
        let frame = Frame::build(messages, 1024, Default::default(), |builder| {
            calls::dynamic_call(
                builder,
                QuestionId::from_wire(10),
                calls::Target::Imported(ImportId::from_wire(export.to_wire())),
                DynamicCall::default(),
                |field| field.set_capability_index(0),
            )?;
            let mut root = builder.try_get_root::<Message>()?;
            let mut call = root.field(Message::CALL).edit()?;
            let mut payload = call.field(WireCall::PARAMS).edit()?;
            let mut table = payload.field(Payload::CAP_TABLE).init(1)?;
            pipeline::Path::new(&path).write(
                table
                    .get_mut(0)
                    .unwrap()
                    .field(CapDescriptor::RECEIVER_ANSWER)
                    .init()?,
                QuestionId::from_wire(3),
            )?;
            Ok(())
        })
        .unwrap();
        let call = Request::<_, 0, 0>::admit_with_capacity(
            frame,
            messages,
            &mut driver.ledgers.imports,
            &mut driver.ledgers.exports,
            &mut driver.ledgers.answers,
            Answer::new(pipelines::Results::new(())),
            |_| panic!("reflected descriptor is not an import"),
            Capacity::Growing { maximum: 2 },
            Capacity::Growing { maximum: 8 },
        )
        .unwrap_or_else(|r| panic!("{}", r.error));
        let reflected = call
            .with(messages, |view| {
                let receipt = view.capability_at(0).unwrap().unwrap();
                driver
                    .imports
                    .bind_capability(
                        receipt,
                        &mut driver.runtime,
                        &mut driver.ledgers.imports,
                        &driver.ledgers.exports,
                        &mut driver.ledgers.answers,
                        &mut driver.pipelines,
                    )
                    .unwrap()
                    .into_client()
            })
            .unwrap();
        assert!(matches!(
            driver.runtime.inspect(&reflected).unwrap(),
            clients::Destination::Waiting(_)
        ));
        driver.runtime.release(reflected);
        driver.release_call(call, messages);
        driver.runtime.release(target);
        assert!(driver.ledgers.answers.get(parent).is_ok());
        close(
            driver,
            messages,
            &mut messages::Queue::new_growing(0, 8).unwrap(),
        );
    });
}

#[test]
fn invocation_and_resolve_publish_resources_with_exact_descriptors_and_retry_backpressure() {
    use crate::attachments::Sender;
    with_driver(|mut driver, messages| {
        let target = remote(&mut driver);
        let value = driver
            .runtime
            .endpoint(binding::Route::local(73), clients::Location::Local)
            .unwrap();
        let promise = driver.runtime.promise(&mut None).unwrap();
        let caps = [Some(&value), None, Some(&promise), Some(&target)];
        let task = driver
            .enqueue_dynamic(
                &target,
                DynamicCall::default(),
                &caps,
                messages,
                512,
                Default::default(),
                |p| p.set_capability_index(0),
            )
            .unwrap();
        let mut queue = messages::Queue::<Sender<u32, 0>>::growing_with_resources(0, 1).unwrap();
        let sent = driver
            .send_invocation_with_attachments(
                messages,
                &mut queue,
                1024,
                Default::default(),
                &mut [0; 16],
                Capacity::Growing { maximum: 2 },
                |_| Some(91),
                |server| Some(*server),
            )
            .unwrap()
            .unwrap();
        assert_eq!(sent.task, task);
        let later = driver
            .enqueue_dynamic(
                &target,
                DynamicCall::default(),
                &[Some(&value)],
                messages,
                512,
                Default::default(),
                |p| p.set_capability_index(0),
            )
            .unwrap();
        assert!(matches!(
            driver.send_invocation_with_attachments(
                messages,
                &mut queue,
                1024,
                Default::default(),
                &mut [0; 16],
                Capacity::Growing { maximum: 2 },
                |_| panic!("queue full"),
                |_| panic!("queue full")
            ),
            Err(Error::Input(crate::owned::Error::Storage(
                messages::Error::NoSendSlot
            )))
        ));
        let window = queue.window(messages).unwrap().unwrap();
        assert!(window.resources().unwrap().iter().copied().eq([73, 91]));
        let length = window.bytes().len();
        let exported_promise = {
            let mut bytes = window.bytes();
            let reader = capnp::serialize::read_message_from_flat_slice_no_alloc(
                &mut bytes,
                Default::default(),
            )
            .unwrap();
            let root = reader.get_root::<rpc_capnp::message::Reader<'_>>().unwrap();
            let rpc_capnp::message::Call(call) = root.which().unwrap() else {
                panic!("Call")
            };
            let descriptors = call.unwrap().get_params().unwrap().get_cap_table().unwrap();
            assert_eq!(descriptors.get(0).get_attached_fd(), 0);
            assert_eq!(descriptors.get(1).get_attached_fd(), 255);
            assert_eq!(descriptors.get(2).get_attached_fd(), 255);
            assert_eq!(descriptors.get(3).get_attached_fd(), 1);
            let rpc_capnp::cap_descriptor::SenderPromise(id) = descriptors.get(2).which().unwrap()
            else {
                panic!("promise")
            };
            id
        };
        window.advance(1).unwrap();
        assert!(queue
            .window(messages)
            .unwrap()
            .unwrap()
            .resources()
            .is_none());
        queue
            .window(messages)
            .unwrap()
            .unwrap()
            .advance(length - 1)
            .unwrap();
        let sent = driver
            .send_invocation_with_attachments(
                messages,
                &mut queue,
                1024,
                Default::default(),
                &mut [0; 16],
                Capacity::Growing { maximum: 2 },
                |_| Some(91),
                |server| Some(*server),
            )
            .unwrap()
            .unwrap();
        assert_eq!(sent.task, later);
        let window = queue.window(messages).unwrap().unwrap();
        let length = window.bytes().len();
        window.advance(length).unwrap();
        let mut replacement = Some(driver.runtime.retain_key(value.key()).unwrap());
        assert!(driver
            .runtime
            .resolve(promise.key(), &mut replacement, clients::Source::Direct)
            .unwrap()
            .is_none());
        for _ in 0..256 {
            driver
                .poll_maintenance_with_attachments(
                    messages,
                    &mut queue,
                    1024,
                    Default::default(),
                    &mut [0; 16],
                    NonZeroUsize::new(1).unwrap(),
                    Capacity::Growing { maximum: 2 },
                    |_| Some(91),
                    |server| Some(*server),
                )
                .unwrap();
            if !queue.is_empty() {
                break;
            }
        }
        let window = queue
            .window(messages)
            .unwrap()
            .expect("Resolve was published");
        assert!(window.resources().unwrap().iter().copied().eq([73]));
        let mut bytes = window.bytes();
        let reader =
            capnp::serialize::read_message_from_flat_slice_no_alloc(&mut bytes, Default::default())
                .unwrap();
        let root = reader.get_root::<rpc_capnp::message::Reader<'_>>().unwrap();
        let rpc_capnp::message::Resolve(resolve) = root.which().unwrap() else {
            panic!("Resolve")
        };
        let resolve = resolve.unwrap();
        assert_eq!(resolve.get_promise_id(), exported_promise);
        let rpc_capnp::resolve::Cap(descriptor) = resolve.which().unwrap() else {
            panic!("cap")
        };
        assert_eq!(descriptor.unwrap().get_attached_fd(), 0);
        let length = window.bytes().len();
        window.advance(length).unwrap();
        driver.runtime.release(promise);
        driver.runtime.release(value);
        driver.runtime.release(target);
        let mut closing = driver.terminate(&mut queue);
        for _ in 0..256 {
            if matches!(
                closing
                    .poll(messages, NonZeroUsize::new(16).unwrap())
                    .unwrap(),
                super::super::Termination::Done
            ) {
                return;
            }
        }
        panic!("cleanup");
    });
}
