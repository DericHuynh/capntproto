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
fn large_pending_exchange_tables_only_scan_after_actual_cancellation() {
    with_driver(|mut driver, messages| {
        let mut queue = messages::Queue::new_growing(0, 4).unwrap();
        let mut keys = alloc::vec::Vec::new();
        for _ in 0..16 {
            let key = driver
                .send(
                    messages,
                    &mut queue,
                    128,
                    Default::default(),
                    |builder, id| {
                        calls::dynamic_call(
                            builder,
                            id,
                            calls::Target::Imported(ImportId::from_wire(41)),
                            DynamicCall {
                                no_promise_pipelining: true,
                                ..Default::default()
                            },
                            |_| Ok(()),
                        )
                    },
                )
                .unwrap();
            drain(&mut queue, messages, |_| {});
            keys.push(key);
        }
        assert_eq!(driver.exchanges.len(), 16);
        assert!(driver.maintenance_is_idle());
        for index in [0, 15] {
            driver.cancel(keys[index]).unwrap();
            driver.cancel(keys[index]).unwrap();
        }
        assert!(!driver.maintenance_is_idle());
        assert!(!driver.drain_canceled_response(messages));
        assert!(
            driver.canceled_exchanges,
            "pending cancellation must survive a full scan"
        );
        for index in [15, 0] {
            let frame = Frame::build(messages, 128, Default::default(), |builder| {
                let mut ret = builder
                    .try_init_root::<Message>()?
                    .into_field(Message::RETURN)
                    .init()?;
                ret.scalar(Return::ANSWER_ID)
                    .set(keys[index].id().to_wire());
                ret.scalar(Return::NO_FINISH_NEEDED).set(true);
                ret.into_field(Return::RESULTS).init()?;
                Ok(())
            })
            .unwrap();
            driver
                .receive(frame, messages, &mut [], |_| (), || ())
                .unwrap_or_else(|rejected| panic!("{}", rejected.error));
            assert!(driver.drain_canceled_response(messages));
        }
        assert!(!driver.drain_canceled_response(messages));
        assert!(!driver.canceled_exchanges);
        for _ in 0..256 {
            let progress = driver
                .poll_maintenance(
                    messages,
                    &mut queue,
                    128,
                    Default::default(),
                    &mut [],
                    NonZeroUsize::new(1).unwrap(),
                )
                .unwrap();
            assert_eq!(driver.maintenance_published_retirement(), !queue.is_empty());
            if !queue.is_empty() {
                drain(&mut queue, messages, |root| {
                    assert!(matches!(
                        root.which().unwrap(),
                        rpc_capnp::message::Finish(_)
                    ));
                });
            }
            if matches!(progress, Progress::Idle) {
                break;
            }
        }
        assert!(driver.maintenance_is_idle());
        driver.cancel(keys[7]).unwrap();
        assert!(driver.canceled_exchanges);
        assert!(!driver.maintenance_is_idle());
        close(driver, messages, &mut queue);
        let writers: alloc::vec::Vec<_> = (0..32).map(|_| messages.reserve(128).unwrap()).collect();
        for writer in writers {
            messages.discard(writer);
        }
    });
}

#[test]
fn retained_vacant_pipeline_capacity_does_not_require_maintenance() {
    with_driver(|mut driver, messages| {
        driver
            .tails
            .vacant_mut(|_, _| true, || TailSlot::EMPTY)
            .unwrap();
        driver
            .invocation_pipelines
            .vacant_mut(|_, _| true, || InvocationPipelineSlot::EMPTY)
            .unwrap();
        assert_eq!(driver.tails.len(), 1);
        assert_eq!(driver.invocation_pipelines.len(), 1);
        assert!(driver.maintenance_is_idle());
        let mut slots = [None];
        let mut queue = messages::Queue::new(&mut slots).unwrap();
        assert!(matches!(
            driver.poll_maintenance(
                messages,
                &mut queue,
                256,
                Default::default(),
                &mut [],
                NonZeroUsize::new(1).unwrap()
            ),
            Ok(Progress::Idle)
        ));
        assert!(queue.is_empty());
        close(driver, messages, &mut queue);
    });
}

#[test]
fn immediate_promised_targets_preserve_retry_paths_and_segmented_parameters() {
    for encoded in [false, true] {
        for first_words in [32, 1024] {
            with_driver(|mut driver, messages| {
                let mut queue = messages::Queue::new_growing(0, 8).unwrap();
                let parent = driver
                    .send(
                        messages,
                        &mut queue,
                        512,
                        Default::default(),
                        |builder, id| {
                            calls::dynamic_call(
                                builder,
                                id,
                                calls::Target::Imported(ImportId::from_wire(41)),
                                DynamicCall::default(),
                                |_| Ok(()),
                            )
                        },
                    )
                    .unwrap();
                drain(&mut queue, messages, |_| {});
                let fields = [0, 2, 1];
                let projected = driver
                    .pipeline(parent, pipeline::Path::new(&fields))
                    .unwrap();
                let method = DynamicCall {
                    interface: 71,
                    method: 5,
                    no_promise_pipelining: true,
                    only_promise_pipeline: false,
                };
                let mut prepared = PreparedCall::new(
                    capnp::message::Builder::new(
                        capnp::message::HeapAllocator::new().first_segment_words(first_words),
                    ),
                    71,
                    5,
                )
                .unwrap();
                let payload = [0xa5; 1500];
                prepared
                    .params()
                    .set_as::<capnp::data::Owned>(&payload[..])
                    .unwrap();
                let rpc_capnp::message::Call(source) = prepared
                    .message()
                    .get_root_as_reader::<rpc_capnp::message::Reader>()
                    .unwrap()
                    .which()
                    .unwrap()
                else {
                    panic!("source Call")
                };
                let params = if encoded {
                    ImmediateParams::EncodedCall(
                        prepared
                            .encoded(method, Default::default())
                            .unwrap()
                            .unwrap(),
                    )
                } else {
                    ImmediateParams::Content(source.unwrap().get_params().unwrap().get_content())
                };
                let count = driver.ledgers.questions.len();
                let mut full = messages::Queue::new_growing(0, 0).unwrap();
                assert!(driver
                    .try_send_immediate_borrowed(
                        &projected,
                        method,
                        &params,
                        messages,
                        &mut full,
                        512,
                        Default::default(),
                        ()
                    )
                    .is_err());
                assert!(driver
                    .try_send_immediate_borrowed(
                        &projected,
                        method,
                        &params,
                        messages,
                        &mut queue,
                        8192,
                        Default::default(),
                        ()
                    )
                    .is_err());
                assert!(driver
                    .try_send_immediate_borrowed(
                        &projected,
                        method,
                        &params,
                        messages,
                        &mut queue,
                        512,
                        capnp::message::ReaderOptions {
                            nesting_limit: 0,
                            ..Default::default()
                        },
                        ()
                    )
                    .is_err());
                assert_eq!(driver.ledgers.questions.len(), count);
                assert!(queue.is_empty());
                let waiting = driver.runtime.promise(&mut None).unwrap();
                let older = driver
                    .enqueue_dynamic(
                        &waiting,
                        DynamicCall::default(),
                        &[],
                        messages,
                        256,
                        Default::default(),
                        |_| Ok(()),
                    )
                    .unwrap();
                assert!(driver
                    .try_send_immediate_borrowed(
                        &projected,
                        method,
                        &params,
                        messages,
                        &mut queue,
                        512,
                        Default::default(),
                        ()
                    )
                    .unwrap()
                    .is_none());
                assert!(driver.cancel_invocation(older, messages));
                driver.runtime.release(waiting);
                let child = driver
                    .try_send_immediate_borrowed(
                        &projected,
                        method,
                        &params,
                        messages,
                        &mut queue,
                        512,
                        Default::default(),
                        (),
                    )
                    .unwrap()
                    .unwrap();
                assert_eq!(driver.runtime.queued(), 0);
                drain(&mut queue, messages, |root| {
                    let rpc_capnp::message::Call(call) = root.which().unwrap() else {
                        panic!("child Call")
                    };
                    let call = call.unwrap();
                    assert_eq!(call.get_question_id(), child.id().to_wire());
                    assert_eq!(call.get_interface_id(), 71);
                    assert_eq!(call.get_method_id(), 5);
                    assert!(call.get_no_promise_pipelining());
                    let rpc_capnp::message_target::PromisedAnswer(target) =
                        call.get_target().unwrap().which().unwrap()
                    else {
                        panic!("promised target")
                    };
                    let target = target.unwrap();
                    assert_eq!(target.get_question_id(), parent.id().to_wire());
                    assert_eq!(
                        pipeline::Path::decode(target, &mut [0; 8])
                            .unwrap()
                            .fields(),
                        &fields
                    );
                    assert_eq!(
                        call.get_params()
                            .unwrap()
                            .get_content()
                            .get_as::<capnp::data::Reader>()
                            .unwrap(),
                        &payload
                    );
                });
                driver.runtime.release(projected);
                close(driver, messages, &mut queue);
            });
        }
    }
}

#[test]
fn immediate_capability_publication_preserves_retries_and_descriptor_authority() {
    for failure in ["queue", "storage", "reader", "index", "scratch", "older"] {
        with_driver(|mut driver, messages| {
            let target = remote(&mut driver);
            let value = driver
                .runtime
                .endpoint(binding::Route::local(73), clients::Location::Local)
                .unwrap();
            let source = [capnp::word(3, 0, 0, 0, 0, 0, 0, 0)];
            let bad_source = [capnp::word(3, 0, 0, 0, 1, 0, 0, 0)];
            let segments = [capnp::Word::words_to_bytes(&source)];
            let bad_segments = [capnp::Word::words_to_bytes(&bad_source)];
            let reader = capnp::message::Reader::new(
                capnp::message::SegmentArray::new(&segments),
                Default::default(),
            );
            let bad_reader = capnp::message::Reader::new(
                capnp::message::SegmentArray::new(&bad_segments),
                Default::default(),
            );
            let params = reader.get_root::<capnp::any_pointer::Reader>().unwrap();
            let invalid = bad_reader.get_root::<capnp::any_pointer::Reader>().unwrap();
            let method = DynamicCall {
                interface: 91,
                method: 17,
                no_promise_pipelining: true,
                only_promise_pipeline: false,
            };
            let mut queue = messages::Queue::new_growing(0, 1).unwrap();
            let mut full = messages::Queue::new_growing(0, 0).unwrap();
            let reservations: alloc::vec::Vec<_> = if failure == "storage" {
                (0..32)
                    .map(|_| messages.reserve_exact(512).unwrap())
                    .collect()
            } else {
                alloc::vec::Vec::new()
            };
            let older = if failure == "older" {
                Some(
                    driver
                        .enqueue_dynamic(
                            &target,
                            DynamicCall::default(),
                            &[],
                            messages,
                            128,
                            Default::default(),
                            |_| Ok(()),
                        )
                        .unwrap(),
                )
            } else {
                None
            };
            let options = if failure == "reader" {
                capnp::message::ReaderOptions {
                    traversal_limit_in_words: Some(0),
                    ..Default::default()
                }
            } else {
                Default::default()
            };
            let mut scratch = [0; 16];
            let rejected = driver.try_send_immediate_with_capability(
                &target,
                method,
                if failure == "index" { invalid } else { params },
                &value,
                messages,
                if failure == "queue" {
                    &mut full
                } else {
                    &mut queue
                },
                128,
                options,
                if failure == "scratch" {
                    &mut []
                } else {
                    &mut scratch
                },
                (),
                |_, _, _| u8::MAX,
            );
            if failure == "older" {
                assert!(matches!(rejected, Ok(None)));
            } else {
                assert!(rejected.is_err(), "{failure}");
            }
            assert!(queue.is_empty());
            assert_eq!(driver.ledgers.questions.len(), 0);
            // An aborted staged owner may await ordinary maintenance, but
            // it must confer no peer authority or published reference.
            for id in 0..driver.ledgers.exports.capacity() {
                assert!(driver
                    .ledgers
                    .exports
                    .lookup(capntproto_rpc_engine::ids::ExportId::from_wire(id as u32))
                    .is_none());
            }
            for writer in reservations {
                messages.discard(writer);
            }
            if let Some(task) = older {
                assert!(driver.cancel_invocation(task, messages));
            }
            let key = driver
                .try_send_immediate_with_capability(
                    &target,
                    method,
                    params,
                    &value,
                    messages,
                    &mut queue,
                    128,
                    Default::default(),
                    &mut scratch,
                    (),
                    |owner, _, _| {
                        assert!(matches!(
                            owner,
                            binding::Route::Capability(binding::Endpoint::Local(73))
                        ));
                        3
                    },
                )
                .unwrap()
                .unwrap();
            assert_eq!(driver.runtime.queued(), 0);
            assert_eq!(driver.ledgers.questions.len(), 1);
            assert_eq!(
                (0..driver.ledgers.exports.capacity())
                    .filter(|id| driver
                        .ledgers
                        .exports
                        .lookup(capntproto_rpc_engine::ids::ExportId::from_wire(*id as u32))
                        .is_some())
                    .count(),
                1
            );
            drain(&mut queue, messages, |root| {
                let rpc_capnp::message::Call(call) = root.which().unwrap() else {
                    panic!("Call")
                };
                let call = call.unwrap();
                assert_eq!(call.get_question_id(), key.id().to_wire());
                assert_eq!(call.get_interface_id(), 91);
                assert_eq!(call.get_method_id(), 17);
                assert!(call.get_no_promise_pipelining());
                assert!(!call.get_only_promise_pipeline());
                let rpc_capnp::message_target::ImportedCap(id) =
                    call.get_target().unwrap().which().unwrap()
                else {
                    panic!("imported target")
                };
                assert_eq!(id, 41);
                let params = call.get_params().unwrap();
                params.get_content().validate_capability_indices(1).unwrap();
                let table = params.get_cap_table().unwrap();
                assert_eq!(table.len(), 1);
                let descriptor = table.get(0);
                assert_eq!(descriptor.get_attached_fd(), 3);
                assert!(matches!(
                    descriptor.which().unwrap(),
                    rpc_capnp::cap_descriptor::SenderHosted(_)
                ));
            });
            driver.cancel(key).unwrap();
            driver.runtime.release(value);
            driver.runtime.release(target);
            close(driver, messages, &mut queue);
            let all: alloc::vec::Vec<_> = (0..32)
                .map(|_| messages.reserve_exact(512).unwrap())
                .collect();
            for writer in all {
                messages.discard(writer);
            }
        });
    }
}

#[test]
fn late_completion_binding_preserves_task_and_sender_on_rejection() {
    use capntproto_rpc_engine::completion::shared;
    with_driver(|mut driver, messages| {
        let target = driver
            .runtime
            .endpoint(binding::Route::local(17), clients::Location::Local)
            .unwrap();
        let task = driver
            .enqueue_dynamic(
                &target,
                DynamicCall::default(),
                &[],
                messages,
                256,
                Default::default(),
                |_| Ok(()),
            )
            .unwrap();
        let mut full = Invocations::new_growing(0, 0).unwrap();
        let (sender, receiver) = shared::channel();
        let rejected = full
            .bind_queued(&driver, messages, task, sender.into())
            .expect_err("zero capacity");
        assert_eq!(rejected.error, Error::Busy);
        assert_eq!(driver.runtime.queued(), 1);
        let mut invocations = Invocations::new_growing(0, 1).unwrap();
        invocations
            .bind_queued(&driver, messages, task, rejected.sender)
            .unwrap_or_else(|_| panic!("retry keeps sender"));
        let (duplicate, duplicate_receiver) = shared::channel();
        let rejected = invocations
            .bind_queued(&driver, messages, task, duplicate.into())
            .expect_err("duplicate binding");
        assert_eq!(rejected.error, Error::UnknownInvocation);
        let local = invocations.accept_local(&mut driver).unwrap().unwrap();
        assert_eq!(local.task(), task);
        assert_eq!(driver.runtime.queued(), 0);
        // A running or retired generation cannot be rebound, even in another
        // registry with free capacity.
        let mut other = Invocations::new_growing(0, 1).unwrap();
        let rejected = other
            .bind_queued(&driver, messages, task, rejected.sender)
            .expect_err("running task is no longer queued");
        assert_eq!(rejected.error, Error::UnknownInvocation);
        assert_eq!(invocations.terminate(&mut driver, messages), 1);
        local.release(&mut driver, messages);
        let rejected = other
            .bind_queued(&driver, messages, task, rejected.sender)
            .expect_err("retired task");
        assert_eq!(rejected.error, Error::UnknownInvocation);
        drop((rejected, receiver, duplicate_receiver));
        driver.runtime.release(target);
        let mut queue = messages::Queue::new_growing(0, 8).unwrap();
        close(driver, messages, &mut queue);
    });
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
                Progress::Import(()) => {
                    assert!(driver.maintenance_published_retirement());
                    released = true;
                }
                Progress::Idle => {
                    assert!(!driver.maintenance_published_retirement());
                    assert!(released);
                    break;
                }
                _ => assert!(!driver.maintenance_published_retirement()),
            }
        }
        assert!(released);
        assert!(driver.ledgers.imports.is_empty());
        assert_eq!(queue.len(), 1);
        close(driver, messages, &mut queue);
    });
}

#[test]
fn server_idle_certificate_observes_deferred_answer_and_export_cleanup() {
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
        let lease = driver.ledgers.exports.lease(export).unwrap();
        let id = AnswerId::from_wire(3);
        let answer = driver
            .ledgers
            .answers
            .admit(id, Answer::new(pipelines::Results::new(())))
            .unwrap_or_else(|_| panic!("answer"));
        let pipeline = driver.ledgers.answers.pipeline(id).unwrap();
        let observer = driver
            .pipelines
            .bind(
                &mut driver.runtime,
                &mut driver.ledgers.answers,
                &pipeline,
                pipeline::Path::new(&[]),
            )
            .unwrap();
        assert!(!driver.pipelines.vacant_within(8));
        assert!(!matches!(poll!(), Progress::Idle));
        driver.runtime.release(observer);
        let mut idle = false;
        for _ in 0..128 {
            if matches!(poll!(), Progress::Idle) {
                idle = true;
                break;
            }
        }
        assert!(idle);
        assert!(!driver.pipelines.vacant_within(0));
        assert!(driver.pipelines.vacant_within(8));
        assert!(driver
            .ledgers
            .answers
            .prepare_return(answer)
            .unwrap()
            .accept()
            .is_none());
        assert!(matches!(poll!(), Progress::Idle));
        assert!(driver.ledgers.answers.finish(id, false).retired.is_none());
        assert!(driver.ledgers.exports.release(export, 1).unwrap().is_none());
        assert!(matches!(poll!(), Progress::Idle));

        // No invalidation call: the bounded check must observe these facts.
        driver.ledgers.answers.release_pipeline_deferred(pipeline);
        driver.ledgers.exports.release_local_deferred(lease);
        let mut producer = false;
        let mut idle = false;
        for _ in 0..128 {
            match poll!() {
                Progress::Producer(()) => producer = true,
                Progress::Idle => {
                    idle = true;
                    break;
                }
                _ => (),
            }
        }
        assert!(producer && idle);
        assert!(driver.ledgers.answers.is_empty());
        assert!(driver.ledgers.exports.is_empty());
        driver.runtime.release(target);
        close(driver, messages, &mut queue);
    });
}

#[test]
fn maintenance_idle_scan_keeps_its_budget_after_empty_tables_grow() {
    for quantum in [1, 2, 7, 32] {
        with_driver(|mut driver, messages| {
            // Exceed the bounded vacancy certificate so this still exercises
            // the general scanner and exact quantum accounting.
            assert!(driver.tails.grow(9, || TailSlot::EMPTY));
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
                            NonZeroUsize::new(quantum).unwrap(),
                        )
                        .unwrap()
                };
            }
            let initial = 17 + driver.tails.len();
            for _ in 1..initial.div_ceil(quantum) {
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
            let total = 16 + driver.tails.len() + driver.ledgers.imports.capacity();
            for _ in 1..total.div_ceil(quantum) {
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
fn outgoing_observers_use_only_their_authorized_question_after_slot_reuse() {
    with_driver(|mut driver, messages| {
        let target = remote(&mut driver);
        let mut queue = messages::Queue::new_growing(0, 8).unwrap();
        let mut questions = alloc::vec::Vec::new();
        let mut projections = alloc::vec::Vec::new();
        for _ in 0..2 {
            driver
                .enqueue_dynamic(
                    &target,
                    DynamicCall::default(),
                    &[],
                    messages,
                    256,
                    Default::default(),
                    |_| Ok(()),
                )
                .unwrap();
            let sent = driver
                .send_invocation(messages, &mut queue, 256, Default::default(), &mut [0; 8])
                .unwrap()
                .unwrap();
            drain(&mut queue, messages, |_| {});
            projections.push(
                driver
                    .pipeline(sent.question, pipeline::Path::new(&[]))
                    .unwrap(),
            );
            questions.push(sent.question);
        }
        // Waiting observers have no admissible response to inspect. They may
        // certify idle without spending a quantum walking unrelated slots.
        assert!(driver.outgoing_maintenance_is_idle());
        assert!(driver.maintenance_is_idle());
        assert!(matches!(
            driver
                .poll_maintenance(
                    messages,
                    &mut queue,
                    256,
                    Default::default(),
                    &mut [0; 8],
                    NonZeroUsize::new(1).unwrap(),
                )
                .unwrap(),
            Progress::Idle
        ));
        assert!(!driver.runtime.is_settled(&projections[0]).unwrap());
        assert!(!driver.runtime.is_settled(&projections[1]).unwrap());
        for index in [1, 0] {
            let frame = Frame::build(messages, 256, Default::default(), |builder| {
                calls::exception(
                    builder,
                    AnswerId::from_wire(questions[index].id().to_wire()),
                    capnp::ErrorKind::Overloaded,
                    "selected question",
                )
            })
            .unwrap();
            driver
                .receive(frame, messages, &mut [0; 8], |_| (), || ())
                .unwrap_or_else(|rejected| panic!("{}", rejected.error));
            let exchange = driver.exchanges.iter().position(|slot| {
                matches!(&slot.exchange, Some(Exchange::Returned(response)) if response.key() == questions[index])
            }).unwrap();
            driver.exchanges[exchange].authorized = false;
            if index == 1 {
                assert!(driver.outgoing_maintenance_is_idle());
            }
            assert_eq!(
                driver.poll_outgoing(index, messages).unwrap(),
                Propagation::Unrelated
            );
            assert!(!driver.runtime.is_settled(&projections[index]).unwrap());
            driver.exchanges[exchange].authorized = true;
            assert!(!driver.outgoing_maintenance_is_idle());
            assert!(!driver.maintenance_is_idle());
            if index == 1 {
                assert_eq!(
                    driver.poll_outgoing(0, messages).unwrap(),
                    Propagation::Unrelated
                );
            }
            assert_eq!(
                driver.poll_outgoing(index, messages).unwrap(),
                Propagation::Resolved
            );
            assert!(driver.runtime.is_settled(&projections[index]).unwrap());
            assert_eq!(
                driver.poll_outgoing(index, messages).unwrap(),
                Propagation::Unrelated
            );
            if index == 1 {
                // Reuse the retired observer while its old Return is still
                // retained. The new observer must wait for question zero.
                let reused = driver
                    .pipeline(questions[0], pipeline::Path::new(&[0]))
                    .unwrap();
                assert_eq!(driver.outgoing.question(1), Some(questions[0]));
                assert_eq!(
                    driver.poll_outgoing(1, messages).unwrap(),
                    Propagation::Unrelated
                );
                projections.push(reused);
            }
        }
        assert_eq!(
            driver.poll_outgoing(1, messages).unwrap(),
            Propagation::Resolved
        );
        for question in questions {
            let response = driver.take_response(question).unwrap();
            driver.release_response(response, messages);
        }
        for projection in projections {
            driver.runtime.release(projection);
        }
        driver.runtime.release(target);
        close(driver, messages, &mut queue);
    });
}

#[test]
fn return_before_pipeline_attachment_preserves_late_observers_and_cleanup() {
    for abandon in [false, true] {
        with_driver(|mut driver, messages| {
            let target = remote(&mut driver);
            let task = driver
                .enqueue_dynamic(
                    &target,
                    DynamicCall::default(),
                    &[],
                    messages,
                    256,
                    Default::default(),
                    |_| Ok(()),
                )
                .unwrap();
            let projection = driver
                .invocation_pipeline(task, pipeline::Path::new(&[]), messages)
                .unwrap();
            let mut queue = messages::Queue::new_growing(0, 8).unwrap();
            let sent = driver
                .send_invocation(messages, &mut queue, 256, Default::default(), &mut [0; 8])
                .unwrap()
                .unwrap();
            drain(&mut queue, messages, |_| {});
            let frame = Frame::build(messages, 256, Default::default(), |builder| {
                calls::dynamic_results(
                    builder,
                    AnswerId::from_wire(sent.question.id().to_wire()),
                    |content| content.set_capability_index(0),
                )?;
                let mut root = builder.try_get_root::<Message>()?;
                let mut result = root.field(Message::RETURN).edit()?;
                let mut payload = result.field(Return::RESULTS).edit()?;
                payload
                    .field(Payload::CAP_TABLE)
                    .init(1)?
                    .get_mut(0)
                    .unwrap()
                    .scalar(CapDescriptor::SENDER_HOSTED)
                    .set(73);
                Ok(())
            })
            .unwrap();
            driver
                .receive(frame, messages, &mut [0; 8], |_| (), || ())
                .unwrap_or_else(|rejected| panic!("{}", rejected.error));
            assert!(matches!(
                driver.take_response(sent.question),
                Err(Error::PendingResponse)
            ));
            // Attach after Return, before its original result owner can retire.
            // This observer never advertised a promised-answer route.
            assert_eq!(
                driver.poll_invocation_pipeline(0).unwrap(),
                Propagation::Resolved
            );
            let late = driver
                .pipeline(sent.question, pipeline::Path::new(&[]))
                .unwrap();
            assert!(driver
                .pipeline(sent.question, pipeline::Path::new(&[0; 9]))
                .is_err());
            if abandon {
                driver.runtime.release(late);
            } else {
                assert_eq!(
                    driver.poll_outgoing(1, messages).unwrap(),
                    Propagation::Resolved
                );
                let clients::Destination::Endpoint { owner, .. } =
                    driver.runtime.inspect(&late).unwrap()
                else {
                    panic!("late observer must settle to the returned capability");
                };
                let Some(calls::Target::Imported(id)) =
                    owner.target(&driver.ledgers.questions).unwrap()
                else {
                    panic!("returned sender-hosted capability must be imported");
                };
                assert_eq!(id, ImportId::from_wire(73));
                driver.runtime.release(late);
            }
            assert_eq!(
                driver.poll_outgoing(0, messages).unwrap(),
                Propagation::Resolved
            );
            if abandon {
                assert_eq!(
                    driver.poll_outgoing(1, messages).unwrap(),
                    Propagation::Abandoned
                );
            }
            assert!(driver.runtime.is_settled(&projection).unwrap());
            let response = driver.take_response(sent.question).unwrap();
            driver.release_response(response, messages);
            driver.runtime.release(projection);
            driver.runtime.release(target);
            close(driver, messages, &mut queue);
            let writers: alloc::vec::Vec<_> =
                (0..32).map(|_| messages.reserve(128).unwrap()).collect();
            for writer in writers {
                messages.discard(writer);
            }
        });
    }
}

#[test]
fn authenticated_handoff_keeps_advertised_routes_and_settles_late_observers() {
    use core::{
        future::Future,
        task::{Context, Poll, Waker},
    };
    for outcome in ["success", "abandoned", "failure", "bad-path"] {
        with_driver(|mut driver, messages| {
            driver.allow_answer_handoff(true);
            let target = remote(&mut driver);
            driver
                .enqueue_dynamic(
                    &target,
                    DynamicCall::default(),
                    &[],
                    messages,
                    256,
                    Default::default(),
                    |_| Ok(()),
                )
                .unwrap();
            let mut queue = messages::Queue::new_growing(0, 8).unwrap();
            let sent = driver
                .send_invocation(messages, &mut queue, 256, Default::default(), &mut [0; 8])
                .unwrap()
                .unwrap();
            drain(&mut queue, messages, |_| {});
            let early = driver
                .pipeline(sent.question, pipeline::Path::new(&[]))
                .unwrap();
            let frame = Frame::build(messages, 256, Default::default(), |builder| {
                crate::network::await_from_third_party(
                    builder,
                    AnswerId::from_wire(sent.question.id().to_wire()),
                    |metadata| {
                        metadata
                            .cast::<capnp::field_api::Data>()
                            .copy_from(b"rendezvous")
                    },
                )
            })
            .unwrap();
            driver
                .receive(frame, messages, &mut [0; 8], |_| (), || ())
                .unwrap_or_else(|rejected| panic!("{}", rejected.error));
            let NetworkRequest::Await(awaiting) = driver.take_network().unwrap() else {
                panic!("authenticated handoff request");
            };
            let path = if outcome == "bad-path" {
                &[0][..]
            } else {
                &[][..]
            };
            let late = driver
                .pipeline(sent.question, pipeline::Path::new(path))
                .unwrap();
            let value = driver
                .runtime
                .endpoint(binding::Route::local(73), clients::Location::Local)
                .unwrap();
            let result = if outcome == "failure" {
                Err(capnp::ErrorKind::Overloaded)
            } else {
                let mut draft = LocalResultDraft::<0>::reserve(
                    messages,
                    256,
                    Default::default(),
                    0,
                    0,
                    Capacity::Growing { maximum: 1 },
                )
                .unwrap();
                {
                    let mut write =
                        core::pin::pin!(draft.write_async(messages, async |field, caps| {
                            caps.set(
                                field.cast::<capnp::field_api::WireCapability<()>>(),
                                driver.runtime.retain(&value).unwrap(),
                            )
                            .unwrap_or_else(|_| panic!("result capability"));
                            Ok(())
                        }));
                    assert!(matches!(
                        write.as_mut().poll(&mut Context::from_waker(Waker::noop())),
                        Poll::Ready(Ok(()))
                    ));
                }
                Ok(draft.finish(messages, &mut driver.runtime).unwrap())
            };
            driver
                .complete_await_retaining_routes(awaiting, result, messages, |metadata| {
                    assert_eq!(
                        metadata.get_as::<capnp::data::Reader>().unwrap(),
                        b"rendezvous"
                    );
                    Ok::<_, ()>(())
                })
                .unwrap_or_else(|_| panic!("matched handoff"));
            assert_eq!(
                driver.poll_outgoing(0, messages).unwrap(),
                Propagation::Resolved
            );
            if outcome == "failure" {
                assert!(matches!(
                    driver.runtime.inspect(&early).unwrap(),
                    clients::Destination::Endpoint {
                        location: clients::Location::Broken,
                        ..
                    }
                ));
            } else {
                assert!(
                    matches!(
                        driver.runtime.inspect(&early).unwrap(),
                        clients::Destination::Endpoint {
                            owner: binding::Route::Question(_),
                            ..
                        }
                    ),
                    "advertised routes must retain relay ordering"
                );
            }
            if outcome == "abandoned" {
                driver.runtime.release(late);
                assert_eq!(
                    driver.poll_outgoing(1, messages).unwrap(),
                    Propagation::Abandoned
                );
            } else {
                assert_eq!(
                    driver.poll_outgoing(1, messages).unwrap(),
                    Propagation::Resolved
                );
                if outcome == "success" {
                    assert!(
                        matches!(
                            driver.runtime.inspect(&late).unwrap(),
                            clients::Destination::Endpoint {
                                owner: binding::Route::Capability(binding::Endpoint::Local(73)),
                                ..
                            }
                        ),
                        "an unadvertised observer adopts the authenticated answer directly"
                    );
                } else {
                    assert!(matches!(
                        driver.runtime.inspect(&late).unwrap(),
                        clients::Destination::Endpoint {
                            location: clients::Location::Broken,
                            ..
                        }
                    ));
                }
                driver.runtime.release(late);
            }
            let output = driver.take_output(sent.question, messages).unwrap();
            if outcome == "failure" {
                assert_eq!(output.err(), Some(capnp::ErrorKind::Overloaded));
            } else {
                driver.release_output(output.unwrap(), messages);
            }
            driver.runtime.release(early);
            driver.runtime.release(value);
            driver.runtime.release(target);
            close(driver, messages, &mut queue);
        });
    }
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

#[test]
fn plain_results_preserve_parameter_grants_until_the_peer_releases_them() {
    use super::super::PlainReceived;
    use capnp::private::capability::ResponseHook;
    for release_params in [false, true] {
        with_driver(|mut driver, messages| {
            let target = remote(&mut driver);
            let value = driver
                .runtime
                .endpoint(binding::Route::local(73), clients::Location::Local)
                .unwrap();
            let source = [capnp::word(3, 0, 0, 0, 0, 0, 0, 0)];
            let segments = [capnp::Word::words_to_bytes(&source)];
            let reader = capnp::message::Reader::new(
                capnp::message::SegmentArray::new(&segments),
                Default::default(),
            );
            let mut queue = messages::Queue::new_growing(0, 8).unwrap();
            let key = driver
                .try_send_immediate_with_capability(
                    &target,
                    DynamicCall {
                        no_promise_pipelining: true,
                        ..Default::default()
                    },
                    reader.get_root().unwrap(),
                    &value,
                    messages,
                    &mut queue,
                    256,
                    Default::default(),
                    &mut [0; 8],
                    (),
                    |_, _, _| u8::MAX,
                )
                .unwrap()
                .unwrap();
            let mut exported = None;
            drain(&mut queue, messages, |root| {
                let rpc_capnp::message::Call(call) = root.which().unwrap() else {
                    panic!("Call");
                };
                let descriptors = call.unwrap().get_params().unwrap().get_cap_table().unwrap();
                let rpc_capnp::cap_descriptor::SenderHosted(id) =
                    descriptors.get(0).which().unwrap()
                else {
                    panic!("sender hosted");
                };
                exported = Some(capntproto_rpc_engine::ids::ExportId::from_wire(id));
            });
            let exported = exported.unwrap();
            assert!(driver.ledgers.exports.lookup(exported).is_some());
            let frame = Frame::build(messages, 256, Default::default(), |builder| {
                calls::dynamic_results(
                    builder,
                    AnswerId::from_wire(key.id().to_wire()),
                    |content| {
                        content
                            .cast::<capnp::field_api::Data>()
                            .copy_from(b"callback completed")
                    },
                )?;
                let mut root = builder.try_get_root::<Message>()?;
                let mut result = root.field(Message::RETURN).edit()?;
                result.scalar(Return::NO_FINISH_NEEDED).set(true);
                result
                    .scalar(Return::RELEASE_PARAM_CAPS)
                    .set(release_params);
                Ok(())
            })
            .unwrap();
            let received = driver
                .try_receive_plain_owned(frame, messages, |_| Some(()))
                .unwrap_or_else(|(error, _)| panic!("{error:?}"));
            if release_params {
                let PlainReceived::Deferred(frame) = received else {
                    panic!("parameter releases require the descriptor transaction");
                };
                driver
                    .receive(frame, messages, &mut [0; 8], |_| (), || ())
                    .unwrap_or_else(|rejected| panic!("{}", rejected.error));
                let response = driver.take_response(key).unwrap();
                driver.release_response(response, messages);
                assert!(driver.ledgers.exports.lookup(exported).is_none());
            } else {
                let PlainReceived::Delivered((), response) = received else {
                    panic!("a grant-preserving plain reply can complete directly");
                };
                assert_eq!(
                    response
                        .get()
                        .unwrap()
                        .get_as::<capnp::data::Reader>()
                        .unwrap(),
                    b"callback completed"
                );
                assert!(
                    driver.ledgers.exports.lookup(exported).is_some(),
                    "Return must not revoke peer authority"
                );
                drop(response);
                let frame = Frame::build(messages, 128, Default::default(), |builder| {
                    calls::release(builder, ImportId::from_wire(exported.to_wire()), 1)
                })
                .unwrap();
                driver
                    .receive(frame, messages, &mut [0; 8], |_| (), || ())
                    .unwrap_or_else(|rejected| panic!("{}", rejected.error));
                assert!(driver.ledgers.exports.lookup(exported).is_none());
            }
            assert!(driver.ledgers.questions.is_empty());
            driver.runtime.release(value);
            driver.runtime.release(target);
            close(driver, messages, &mut queue);
        });
    }
}

#[test]
fn plain_results_cannot_complete_a_redirected_question() {
    use super::super::PlainReceived;
    with_driver(|mut driver, messages| {
        let mut queue = messages::Queue::new_growing(0, 8).unwrap();
        let key = driver
            .send_redirected(
                messages,
                &mut queue,
                256,
                Default::default(),
                |builder, id| {
                    calls::dynamic_call(
                        builder,
                        id,
                        calls::Target::Imported(ImportId::from_wire(41)),
                        DynamicCall::default(),
                        |_| Ok(()),
                    )?;
                    builder
                        .try_get_root::<Message>()?
                        .field(Message::CALL)
                        .edit()?
                        .group(WireCall::SEND_RESULTS_TO)
                        .select()
                        .void(rpc_capnp::api::call::SendResultsTo::YOURSELF)
                        .set();
                    Ok(())
                },
            )
            .unwrap();
        drain(&mut queue, messages, |_| {});
        let frame = Frame::build(messages, 128, Default::default(), |builder| {
            calls::dynamic_results(builder, AnswerId::from_wire(key.id().to_wire()), |_| Ok(()))?;
            let mut root = builder.try_get_root::<Message>()?;
            let mut returned = root.field(Message::RETURN).edit()?;
            returned.scalar(Return::RELEASE_PARAM_CAPS).set(false);
            returned.scalar(Return::NO_FINISH_NEEDED).set(true);
            Ok(())
        })
        .unwrap();
        let PlainReceived::Deferred(frame) = driver
            .try_receive_plain_owned(frame, messages, |_| Some(()))
            .unwrap_or_else(|(error, _)| panic!("{error:?}"))
        else {
            panic!("redirected call cannot receive direct Results");
        };
        let rejected = driver
            .receive(frame, messages, &mut [0; 8], |_| (), || ())
            .err()
            .expect("wrong result destination");
        rejected.frame.release(messages);
        assert!(driver.ledgers.questions.contains(key));
        close(driver, messages, &mut queue);
    });
}

#[test]
fn plain_return_shortcuts_preserve_cancellation_pipeline_and_control_owners() {
    use super::super::PlainReceived;
    for buffered in [false, true] {
        for reason in [
            "no-question",
            "canceled",
            "pipeline",
            "control",
            "disconnected",
        ] {
            with_driver(|mut driver, messages| {
                let target = remote(&mut driver);
                let mut queue = messages::Queue::new_growing(0, 8).unwrap();
                let params = capnp::message::Builder::new_default();
                let key = if reason == "no-question" {
                    None
                } else {
                    let key = driver
                        .try_send_immediate(
                            &target,
                            DynamicCall {
                                no_promise_pipelining: reason != "pipeline",
                                ..Default::default()
                            },
                            params.get_root_as_reader().unwrap(),
                            messages,
                            &mut queue,
                            128,
                            Default::default(),
                            (),
                        )
                        .unwrap()
                        .unwrap();
                    drain(&mut queue, messages, |_| {});
                    Some(key)
                };
                let projection = if reason == "pipeline" {
                    Some(
                        driver
                            .pipeline(key.unwrap(), pipeline::Path::new(&[]))
                            .unwrap(),
                    )
                } else {
                    None
                };
                if reason == "canceled" {
                    driver.cancel(key.unwrap()).unwrap();
                }
                if reason == "disconnected" {
                    driver.disconnect(capnp::ErrorKind::Disconnected);
                }
                let frame = Frame::build(messages, 128, Default::default(), |builder| {
                    if reason == "control" {
                        return crate::calls::bootstrap(builder, QuestionId::from_wire(42));
                    }
                    let root = builder.try_init_root::<Message>()?;
                    let mut ret = root.into_field(Message::RETURN).init()?;
                    ret.scalar(Return::ANSWER_ID)
                        .set(key.map_or(0, |key| key.id().to_wire()));
                    ret.scalar(Return::NO_FINISH_NEEDED).set(true);
                    ret.into_field(Return::RESULTS).init()?;
                    Ok(())
                })
                .unwrap();
                let bytes = messages.read(&frame.lease).to_vec();
                let frame = if buffered {
                    assert!(driver
                        .try_receive_plain_buffered::<()>(
                            &bytes,
                            Default::default(),
                            messages,
                            |_| panic!("{reason} cannot select a direct sink")
                        )
                        .unwrap()
                        .is_none());
                    frame
                } else {
                    let PlainReceived::Deferred(frame) = driver
                        .try_receive_plain_owned::<()>(frame, messages, |_| {
                            panic!("{reason} cannot select a direct sink")
                        })
                        .unwrap_or_else(|(error, _)| panic!("{error:?}"))
                    else {
                        panic!("ineligible reply was consumed")
                    };
                    frame
                };
                assert_eq!(&*messages.read(&frame.lease), bytes.as_slice());
                if let Some(key) = key {
                    assert!(driver.ledgers.questions.contains(key));
                }
                frame.release(messages);
                if let Some(projection) = projection {
                    driver.runtime.release(projection);
                }
                driver.runtime.release(target);
                close(driver, messages, &mut queue);
            });
        }
    }
}

#[test]
fn direct_plain_return_is_transactional_and_pins_the_original_result() {
    use super::super::PlainReceived;
    use capnp::private::capability::ResponseHook;
    with_driver(|mut driver, messages| {
        let target = remote(&mut driver);
        let mut queue = messages::Queue::new_growing(0, 8).unwrap();
        let request = capnp::message::Builder::new_default();
        let key = driver
            .try_send_immediate(
                &target,
                DynamicCall {
                    no_promise_pipelining: true,
                    ..Default::default()
                },
                request.get_root_as_reader().unwrap(),
                messages,
                &mut queue,
                128,
                Default::default(),
                (),
            )
            .unwrap()
            .unwrap();
        drain(&mut queue, messages, |_| {});
        fn reply<'p>(
            messages: &mut messages::Messages<'p, '_, '_>,
            id: u32,
            malformed: bool,
            omit: bool,
        ) -> Frame<'p> {
            Frame::build(messages, 128, Default::default(), |b| {
                let root = b.try_init_root::<Message>()?;
                let mut result = root.into_field(Message::RETURN).init()?;
                result.scalar(Return::ANSWER_ID).set(id);
                result.scalar(Return::NO_FINISH_NEEDED).set(omit);
                let content = result
                    .into_field(Return::RESULTS)
                    .init()?
                    .into_field(Payload::CONTENT);
                if malformed {
                    content.set_capability_index(0)
                } else {
                    content
                        .cast::<capnp::field_api::Data>()
                        .copy_from(b"exact result")
                }
            })
            .unwrap()
        }
        let frame = reply(messages, key.id().to_wire(), false, true);
        let PlainReceived::Deferred(frame) = driver
            .try_receive_plain_owned::<()>(frame, messages, |_| None)
            .unwrap_or_else(|(e, _)| panic!("{e:?}"))
        else {
            panic!("unclaimed result")
        };
        assert!(driver.ledgers.questions.contains(key));
        frame.release(messages);
        let frame = reply(messages, key.id().to_wire(), true, true);
        let (error, frame) = match driver.try_receive_plain_owned(frame, messages, |_| Some(17)) {
            Err(rejected) => rejected,
            Ok(_) => panic!("invalid capability must be rejected"),
        };
        assert!(matches!(error, Error::Input(_)));
        assert!(driver.ledgers.questions.contains(key));
        frame.release(messages);
        let frame = reply(messages, key.id().to_wire(), false, false);
        let PlainReceived::Deferred(frame) = driver
            .try_receive_plain_owned(frame, messages, |_| Some(17))
            .unwrap_or_else(|(e, _)| panic!("{e:?}"))
        else {
            panic!("Finish obligation uses ordinary receive")
        };
        assert!(driver.ledgers.questions.contains(key));
        frame.release(messages);
        let frame = reply(messages, key.id().to_wire(), false, true);
        let PlainReceived::Delivered(sink, result) = driver
            .try_receive_plain_owned(frame, messages, |selected| {
                assert_eq!(selected, key);
                Some(17)
            })
            .unwrap_or_else(|(e, _)| panic!("{e:?}"))
        else {
            panic!("owned plain reply")
        };
        assert_eq!(sink, 17);
        assert!(driver.ledgers.questions.is_empty());
        assert!(!driver.ledgers.questions.has_pending_finish());
        assert_eq!(
            result
                .get()
                .unwrap()
                .get_as::<capnp::data::Reader>()
                .unwrap(),
            b"exact result"
        );
        driver.runtime.release(target);
        close(driver, messages, &mut queue);
        assert_eq!(
            result
                .get()
                .unwrap()
                .get_as::<capnp::data::Reader>()
                .unwrap(),
            b"exact result"
        );
    });
}

#[test]
fn buffered_plain_return_is_transactional_and_pins_the_original_result() {
    use super::super::PlainReceived;
    use capnp::private::capability::ResponseHook;
    fn receive<'p, 'id, T>(
        driver: &mut Peer<'p, 'id, '_>,
        frame: Frame<'p>,
        messages: &mut messages::Messages<'p, '_, '_>,
        select: impl FnOnce(questions::Key<'id>) -> Option<T>,
    ) -> Result<PlainReceived<'p, T>, (Error, Frame<'p>)> {
        let bytes = messages.read(&frame.lease).to_vec();
        match driver.try_receive_plain_buffered(&bytes, frame.options, messages, select) {
            Ok(Some((sink, result))) => {
                frame.release(messages);
                Ok(PlainReceived::Delivered(sink, result))
            }
            Ok(None) => Ok(PlainReceived::Deferred(frame)),
            Err(error) => Err((error, frame)),
        }
    }
    with_driver(|mut driver, messages| {
        let target = remote(&mut driver);
        let mut queue = messages::Queue::new_growing(0, 8).unwrap();
        let request = capnp::message::Builder::new_default();
        let key = driver
            .try_send_immediate(
                &target,
                DynamicCall {
                    no_promise_pipelining: true,
                    ..Default::default()
                },
                request.get_root_as_reader().unwrap(),
                messages,
                &mut queue,
                128,
                Default::default(),
                (),
            )
            .unwrap()
            .unwrap();
        drain(&mut queue, messages, |_| {});
        fn reply<'p>(
            messages: &mut messages::Messages<'p, '_, '_>,
            id: u32,
            malformed: bool,
            omit: bool,
        ) -> Frame<'p> {
            Frame::build(messages, 128, Default::default(), |b| {
                let root = b.try_init_root::<Message>()?;
                let mut result = root.into_field(Message::RETURN).init()?;
                result.scalar(Return::ANSWER_ID).set(id);
                result.scalar(Return::NO_FINISH_NEEDED).set(omit);
                let content = result
                    .into_field(Return::RESULTS)
                    .init()?
                    .into_field(Payload::CONTENT);
                if malformed {
                    content.set_capability_index(0)
                } else {
                    content
                        .cast::<capnp::field_api::Data>()
                        .copy_from(b"exact result")
                }
            })
            .unwrap()
        }
        let frame = reply(messages, key.id().to_wire(), false, true);
        let PlainReceived::Deferred(frame) = receive::<()>(&mut driver, frame, messages, |_| None)
            .unwrap_or_else(|(e, _)| panic!("{e:?}"))
        else {
            panic!("unclaimed result")
        };
        assert!(driver.ledgers.questions.contains(key));
        frame.release(messages);
        let frame = reply(messages, key.id().to_wire(), true, true);
        let (error, frame) = match receive(&mut driver, frame, messages, |_| Some(17)) {
            Err(rejected) => rejected,
            Ok(_) => panic!("invalid capability must be rejected"),
        };
        assert!(matches!(error, Error::Input(_)));
        assert!(driver.ledgers.questions.contains(key));
        frame.release(messages);
        let frame = reply(messages, key.id().to_wire(), false, false);
        let PlainReceived::Deferred(frame) = receive(&mut driver, frame, messages, |_| Some(17))
            .unwrap_or_else(|(e, _)| panic!("{e:?}"))
        else {
            panic!("Finish obligation uses ordinary receive")
        };
        assert!(driver.ledgers.questions.contains(key));
        frame.release(messages);
        let frame = reply(messages, key.id().to_wire(), false, true);
        let PlainReceived::Delivered(sink, result) =
            receive(&mut driver, frame, messages, |selected| {
                assert_eq!(selected, key);
                Some(17)
            })
            .unwrap_or_else(|(e, _)| panic!("{e:?}"))
        else {
            panic!("owned plain reply")
        };
        assert_eq!(sink, 17);
        assert!(driver.ledgers.questions.is_empty());
        assert!(!driver.ledgers.questions.has_pending_finish());
        assert_eq!(
            result
                .get()
                .unwrap()
                .get_as::<capnp::data::Reader>()
                .unwrap(),
            b"exact result"
        );
        driver.runtime.release(target);
        close(driver, messages, &mut queue);
        assert_eq!(
            result
                .get()
                .unwrap()
                .get_as::<capnp::data::Reader>()
                .unwrap(),
            b"exact result"
        );
    });
}

#[test]
fn buffered_plain_return_rejects_invalid_framing_and_defers_unknown_questions() {
    with_driver(|mut driver, messages| {
        let target = remote(&mut driver);
        let mut queue = messages::Queue::new_growing(0, 8).unwrap();
        let request = capnp::message::Builder::new_default();
        let key = driver
            .try_send_immediate(
                &target,
                DynamicCall {
                    no_promise_pipelining: true,
                    ..Default::default()
                },
                request.get_root_as_reader().unwrap(),
                messages,
                &mut queue,
                128,
                Default::default(),
                (),
            )
            .unwrap()
            .unwrap();
        drain(&mut queue, messages, |_| {});
        let frame = Frame::build(messages, 128, Default::default(), |b| {
            let root = b.try_init_root::<Message>()?;
            let mut result = root.into_field(Message::RETURN).init()?;
            result.scalar(Return::ANSWER_ID).set(key.id().to_wire() + 1);
            result.scalar(Return::NO_FINISH_NEEDED).set(true);
            result
                .into_field(Return::RESULTS)
                .init()?
                .into_field(Payload::CONTENT)
                .cast::<capnp::field_api::Data>()
                .copy_from(b"wrong question")
        })
        .unwrap();
        let bytes = messages.read(&frame.lease).to_vec();
        frame.release(messages);
        assert!(driver
            .try_receive_plain_buffered::<()>(&bytes, Default::default(), messages, |_| panic!(
                "unknown question must not select a sink"
            ))
            .unwrap()
            .is_none());
        for length in [0, 4, bytes.len() - 1] {
            let result = driver.try_receive_plain_buffered::<()>(
                &bytes[..length],
                Default::default(),
                messages,
                |_| panic!("invalid frame must not select a sink"),
            );
            assert!(matches!(result, Err(Error::Input(_))));
            assert!(driver.ledgers.questions.contains(key));
            assert!(!driver.ledgers.questions.has_pending_finish());
        }
        driver.runtime.release(target);
        close(driver, messages, &mut queue);
    });
}

#[test]
fn direct_deferred_admission_matches_general_validation_and_ownership() {
    // Compare the specialized Call classifier to the full receive path, with
    // identical tables and wire bytes. In particular, rejected input may neither
    // make imports nor lose its exact frame/producer, and Busy calls no factory.
    for fault in 0..8 {
        let run = |direct: bool| {
            with_driver(|mut driver, messages| {
                driver.defer_call_dispatch(true);
                let target = driver
                    .runtime
                    .endpoint(binding::Route::local(9), clients::Location::Local)
                    .unwrap();
                let export = {
                    let mut batch = driver.ledgers.exports.batch();
                    let grant =
                        crate::clients::stage(&mut driver.runtime, &mut batch, &target).unwrap();
                    batch.commit();
                    grant.id()
                };
                if fault == 3 {
                    driver
                        .ledgers
                        .answers
                        .admit(
                            AnswerId::from_wire(9),
                            Answer::new(pipelines::Results::new(())),
                        )
                        .unwrap_or_else(|_| panic!("preexisting answer"));
                }
                if fault == 7 {
                    let frame = Frame::build(messages, 1024, Default::default(), |builder| {
                        calls::dynamic_call(
                            builder,
                            QuestionId::from_wire(8),
                            calls::Target::Imported(ImportId::from_wire(export.to_wire())),
                            DynamicCall::default(),
                            |_| Ok(()),
                        )
                    })
                    .unwrap();
                    assert!(matches!(
                        driver
                            .receive_general(frame, messages, &mut [0; 8], &mut |_| (), |_| Ok(())),
                        Ok(Received::DeferredCall(_))
                    ));
                }
                let options = capnp::message::ReaderOptions {
                    traversal_limit_in_words: if fault == 6 { Some(1) } else { Some(1024) },
                    ..Default::default()
                };
                let frame = Frame::build(messages, 1024, options, |builder| {
                    calls::dynamic_call(
                        builder,
                        QuestionId::from_wire(9),
                        calls::Target::Imported(ImportId::from_wire(if fault == 1 {
                            u32::MAX
                        } else {
                            export.to_wire()
                        })),
                        DynamicCall::default(),
                        |field| {
                            if fault == 2 || fault == 4 {
                                field.set_capability_index(6)
                            } else {
                                Ok(())
                            }
                        },
                    )?;
                    if fault == 4 {
                        let mut root = builder.try_get_root::<Message>()?;
                        let mut call = root.field(Message::CALL).edit()?;
                        let mut params = call.field(WireCall::PARAMS).edit()?;
                        let mut caps = params.field(Payload::CAP_TABLE).init(7)?;
                        for index in 0..7 {
                            caps.get_mut(index)
                                .unwrap()
                                .scalar(CapDescriptor::SENDER_HOSTED)
                                .set(99);
                        }
                    }
                    Ok(())
                })
                .unwrap();
                let original = messages.read(&frame.lease).to_vec();
                let mut producers = 0;
                let make = |_| {
                    producers += 1;
                    if fault == 5 {
                        Err(capnp::Error::failed("producer failure".into()))
                    } else {
                        Ok(())
                    }
                };
                let mut imports = 0;
                let mut receiver = |_| {
                    imports += 1;
                };
                let result = if direct {
                    driver.receive_with_policy(frame, messages, &mut [0; 8], &mut receiver, make)
                } else {
                    driver.receive_general(frame, messages, &mut [0; 8], &mut receiver, make)
                };
                let result = match result {
                    Ok(Received::DeferredCall(key)) => Ok(key.id().to_wire()),
                    Ok(_) => panic!("unexpected event"),
                    Err(rejected) => {
                        assert_eq!(&*messages.read(&rejected.frame.lease), original);
                        let outcome = Err((rejected.error, rejected.producer.is_some()));
                        rejected.frame.release(messages);
                        outcome
                    }
                };
                assert_eq!(result.is_ok(), fault == 0);
                assert_eq!(imports, 0);
                if fault == 7 {
                    assert_eq!(producers, 0);
                }
                let snapshot = (
                    result,
                    producers,
                    imports,
                    driver.ledgers.answers.len(),
                    driver.ledgers.imports.len(),
                );
                driver.runtime.release(target);
                close(
                    driver,
                    messages,
                    &mut messages::Queue::new_growing(0, 8).unwrap(),
                );
                snapshot
            })
        };
        assert_eq!(run(true), run(false), "fault {fault}");
    }
}
