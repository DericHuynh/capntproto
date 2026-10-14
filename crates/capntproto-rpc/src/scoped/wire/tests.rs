use super::*;
use capntproto_rpc_engine::{
    clients, connection as storage,
    ids::{ImportId, WireId},
    imports,
};
use capntproto_rpc_wire::{clients::Route, owned::Frame, rpc_capnp};

type Peer<'p, 'id, 's> = Driver<'p, 'id, 's, (), (), u8, u32, 4, 4>;

#[test]
fn data_payloads_share_validated_frames_and_outlive_the_driver_scope() {
    use rpc_capnp::api::{Call, Message, Payload as WirePayload, Return};
    for kind in [PayloadKind::Params, PayloadKind::Results] {
        let mut pool = messages::GrowingBuffers::new(1, 256, 263);
        let payload = messages::with_growing_messages(
            &mut pool,
            storage::Growth {
                initial: 0,
                maximum: 0,
            },
            |messages| {
                let frame = Frame::build(messages, 256, Default::default(), |builder| {
                    let root = builder.try_init_root::<Message>()?;
                    let content = match kind {
                        PayloadKind::Params => root
                            .into_field(Message::CALL)
                            .init()?
                            .into_field(Call::PARAMS)
                            .init()?
                            .into_field(WirePayload::CONTENT),
                        PayloadKind::Results => root
                            .into_field(Message::RETURN)
                            .init()?
                            .into_field(Return::RESULTS)
                            .init()?
                            .into_field(WirePayload::CONTENT),
                    };
                    content
                        .cast::<capnp::field_api::Data>()
                        .copy_from(b"shared payload")
                })
                .unwrap();
                let payload = frame
                    .with(messages, |root| {
                        let content = match root.which().unwrap() {
                            rpc_capnp::message::Call(call) => {
                                call.unwrap().get_params().unwrap().get_content()
                            }
                            rpc_capnp::message::Return(result) => {
                                match result.unwrap().which().unwrap() {
                                    rpc_capnp::return_::Results(payload) => {
                                        payload.unwrap().get_content()
                                    }
                                    _ => unreachable!(),
                                }
                            }
                            _ => unreachable!(),
                        };
                        let pointer = content.get_as::<capnp::data::Reader>().unwrap().as_ptr();
                        content.validate_capability_indices(0).unwrap();
                        let payload = Payload::retain_admitted_or_copy(
                            content,
                            frame.share(messages),
                            kind,
                            0,
                        )
                        .unwrap();
                        assert!(matches!(payload.body, PayloadBody::Shared { .. }));
                        assert_eq!(
                            payload
                                .reader()
                                .unwrap()
                                .get_as::<capnp::data::Reader>()
                                .unwrap()
                                .as_ptr(),
                            pointer
                        );
                        payload
                    })
                    .unwrap();
                frame.release(messages);
                assert_eq!(
                    messages.reserve(256).unwrap_err(),
                    messages::Error::NoBuffer
                );
                payload
            },
        )
        .unwrap();
        drop(pool);
        assert_eq!(
            payload
                .reader()
                .unwrap()
                .get_as::<capnp::data::Reader>()
                .unwrap(),
            b"shared payload"
        );
        let PayloadBody::Shared { message } = &payload.body else {
            panic!("shared frame")
        };
        let allocation = core::ptr::from_ref(&**message).cast::<()>();
        let content = match kind {
            PayloadKind::Params => {
                let params = payload.into_params();
                assert_eq!(core::ptr::from_ref(&*params).cast::<()>(), allocation);
                params
                    .get()
                    .unwrap()
                    .get_as::<capnp::data::Reader>()
                    .unwrap()
                    .to_vec()
            }
            PayloadKind::Results => {
                let response = payload.into_response();
                assert_eq!(core::ptr::from_ref(&*response).cast::<()>(), allocation);
                response
                    .get()
                    .unwrap()
                    .get_as::<capnp::data::Reader>()
                    .unwrap()
                    .to_vec()
            }
        };
        assert_eq!(content, b"shared payload");
    }
}

#[test]
fn shared_payload_still_enforces_the_original_traversal_budget() {
    use rpc_capnp::api::{Call, Message, Payload as WirePayload};
    let mut pool = messages::GrowingBuffers::new(1, 256, 263);
    messages::with_growing_messages(
        &mut pool,
        storage::Growth {
            initial: 0,
            maximum: 0,
        },
        |messages| {
            let frame = Frame::build(
                messages,
                256,
                capnp::message::ReaderOptions {
                    traversal_limit_in_words: Some(64),
                    ..Default::default()
                },
                |builder| {
                    builder
                        .try_init_root::<Message>()?
                        .into_field(Message::CALL)
                        .init()?
                        .into_field(Call::PARAMS)
                        .init()?
                        .into_field(WirePayload::CONTENT)
                        .cast::<capnp::field_api::List<()>>()
                        .init(1000)
                        .map(|_| ())
                },
            )
            .unwrap();
            // Sharing is available only after admission. Exercise the actual
            // admission boundary: this small frame's void list requires 1000
            // virtual words of traversal, exceeding the original limits before
            // a hosted payload can escape.
            with_peer(4, |mut driver| {
                let rejected = match capntproto_rpc_wire::owned::Call::<_, 4, 4>::admit(
                    frame,
                    messages,
                    &mut driver.ledgers.imports,
                    &mut driver.ledgers.exports,
                    &mut driver.ledgers.answers,
                    owned::answer::State::new(owned::pipelines::Results::new(())),
                    |_| (),
                ) {
                    Ok(_) => panic!("over-budget input was admitted"),
                    Err(rejected) => rejected,
                };
                assert!(
                    format!("{:?}", rejected.error).contains("ReadLimitExceeded"),
                    "{:?}",
                    rejected.error
                );
                rejected.frame.release(messages);
            });
            let writer = messages.reserve(256).unwrap();
            messages.discard(writer);
        },
    )
    .unwrap();
}

#[test]
fn exact_payload_size_hint_includes_root_without_spilling_large_data() {
    for size in [0, 1, 8, 1024, 8192, 65536] {
        let data = vec![0x5a; size];
        let mut original = capnp::message::Builder::new_default();
        original
            .set_root::<capnp::data::Owned>(data.as_slice())
            .unwrap();
        let payload = Payload::copy(original.get_root_as_reader().unwrap()).unwrap();
        let PayloadBody::Owned(message) = &payload.body else {
            panic!("copied message")
        };
        assert_eq!(message.get_segments_for_output().len(), 1, "{size} bytes");
        assert_eq!(
            payload
                .reader()
                .unwrap()
                .get_as::<capnp::data::Reader>()
                .unwrap(),
            &data
        );
    }
}
fn with_peer<'p, R>(maximum: usize, f: impl for<'id, 's> FnOnce(Peer<'p, 'id, 's>) -> R) -> R {
    with_peer_storage(
        maximum,
        capntproto_rpc_wire::Capacity::Inline,
        capntproto_rpc_wire::Capacity::Inline,
        f,
    )
}
fn with_peer_storage<'p, const CAPS: usize, const PATH: usize, R>(
    maximum: usize,
    capabilities: capntproto_rpc_wire::Capacity,
    path: capntproto_rpc_wire::Capacity,
    f: impl for<'id, 's> FnOnce(Driver<'p, 'id, 's, (), (), u8, u32, CAPS, PATH>) -> R,
) -> R {
    storage::with_domain(|domain| {
        let bound = storage::Growth {
            initial: 0,
            maximum: 16,
        };
        let connection = domain
            .bind_growing_adoptions(
                storage::GrowingStorage {
                    imports: bound,
                    exports: bound,
                    questions: bound,
                    answers: bound,
                    clients: bound,
                },
                bound,
            )
            .unwrap();
        let mut config = connection::GrowingStorage::uniform(0, 16);
        config.tasks.maximum = maximum;
        config.message_capabilities = capabilities;
        config.message_path = path;
        let driver =
            Driver::new_growing(connection, config, 1).unwrap_or_else(|_| panic!("driver"));
        f(driver)
    })
}
fn with_pool<R>(f: impl for<'p, 's, 'b> FnOnce(&mut messages::Messages<'p, 's, 'b>) -> R) -> R {
    let mut buffers = messages::GrowingBuffers::new(24, 4096, 24 * 4103);
    messages::with_growing_messages(
        &mut buffers,
        storage::Growth {
            initial: 0,
            maximum: 8,
        },
        f,
    )
    .unwrap()
}
fn next(hooks: &mut ClientHooks<'_>) -> Call {
    match hooks.poll_next(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(Some(call)) => call,
        _ => panic!("queued owned call"),
    }
}
fn ready<F: Future + Unpin>(f: &mut F) -> F::Output {
    match Pin::new(f).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("expected ready"),
    }
}
fn no_import<'id, G>(_: &mut G, _: &dyn ClientHook) -> capnp::Result<Client<'id>> {
    panic!("unexpected foreign capability")
}
fn settle<'p, 'id, const CAPS: usize, const PATH: usize>(
    calls: &mut connection::Invocations<'_, '_, 'p, 'id, (), CAPS>,
    driver: &mut Driver<'p, 'id, '_, (), (), u8, u32, CAPS, PATH>,
    messages: &mut messages::Messages<'p, '_, '_>,
) {
    for _ in 0..32 {
        for index in 0..calls.capacity() {
            calls
                .poll_one(
                    index,
                    driver,
                    messages,
                    &mut Context::from_waker(Waker::noop()),
                )
                .unwrap();
        }
    }
}
fn remote<'p, 'id>(driver: &mut Peer<'p, 'id, '_>) -> Client<'id> {
    let mut slots = [imports::ReceiveSlot::new(ImportId::from_wire(7), ())];
    driver
        .ledgers
        .imports
        .prepare_receive(&mut slots)
        .unwrap()
        .commit();
    let reference = slots[0].take_local().unwrap();
    let result = driver
        .imports
        .bind(&mut driver.runtime, &mut driver.ledgers.imports, &reference)
        .unwrap();
    driver.ledgers.imports.release(reference);
    result
}

#[test]
fn owned_text_call_and_result_use_portable_driver_and_tracked_completion() {
    for no_promise_pipelining in [false, true] {
        with_pool(|messages| {
            with_peer(4, |mut driver| {
                let target = driver
                    .runtime
                    .endpoint(Route::local(42), clients::Location::Local)
                    .unwrap();
                let mut hooks = ClientHooks::new(4, 4);
                let client = hooks.export_client(&mut driver.runtime, &target).unwrap();
                let mut request = client
                    .new_call_with_hints::<any_pointer::Owned, any_pointer::Owned>(
                        47,
                        1,
                        None,
                        CallHints {
                            no_promise_pipelining,
                            ..Default::default()
                        },
                    );
                request
                    .get()
                    .set_as::<capnp::text::Owned>("request text")
                    .unwrap();
                let mut remote = request.send();
                let call = next(&mut hooks);
                let mut invocations = connection::Invocations::new_growing(0, 4).unwrap();
                let mut call = hooks
                    .enqueue_wire(
                        call,
                        &mut driver,
                        &mut invocations,
                        messages,
                        Default::default(),
                        no_import,
                    )
                    .unwrap_or_else(|r| panic!("enqueue {}", r.error));
                assert_eq!(call.pipeline.is_none(), no_promise_pipelining);
                let running = invocations.accept_local(&mut driver).unwrap().unwrap();
                running
                    .params(messages, |params| {
                        assert_eq!(params.interface_id(), 47);
                        assert_eq!(params.method_id(), 1);
                        assert_eq!(
                            params
                                .content()
                                .unwrap()
                                .get_as::<capnp::text::Reader>()
                                .unwrap(),
                            "request text"
                        );
                    })
                    .unwrap();
                let output = driver
                    .local_result_dynamic(47, 1, &[], messages, 256, Default::default(), |field| {
                        field
                            .cast::<capnp::field_api::Text>()
                            .copy_from("response text")
                    })
                    .unwrap();
                invocations
                    .complete_local(&mut driver, messages, running, Ok(output))
                    .unwrap_or_else(|_| panic!("complete local"));
                settle(&mut invocations, &mut driver, messages);
                assert!(call
                    .poll(
                        &mut hooks,
                        &mut driver,
                        messages,
                        &mut Context::from_waker(Waker::noop())
                    )
                    .is_ready());
                assert_eq!(
                    ready(&mut remote.promise)
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_as::<capnp::text::Reader>()
                        .unwrap(),
                    "response text"
                );
                settle(&mut invocations, &mut driver, messages);
                assert!(invocations.is_empty());
                hooks.close(&mut driver.runtime, Error::disconnected("done".into()));
                driver.runtime.release(target);
            })
        });
    }
}

#[test]
fn owned_pipeline_queues_child_before_parent_local_completion() {
    with_pool(|messages| {
        with_peer(4, |mut driver| {
            let target = driver
                .runtime
                .endpoint(Route::local(42), clients::Location::Local)
                .unwrap();
            let child_target = driver
                .runtime
                .endpoint(Route::local(43), clients::Location::Local)
                .unwrap();
            let mut hooks = ClientHooks::new(8, 8);
            let client = hooks.export_client(&mut driver.runtime, &target).unwrap();
            let mut remote = client
                .new_call::<any_pointer::Owned, any_pointer::Owned>(47, 1, None)
                .send();
            let mut invocations = connection::Invocations::new_growing(0, 4).unwrap();
            let call = next(&mut hooks);
            let mut call = hooks
                .enqueue_wire(
                    call,
                    &mut driver,
                    &mut invocations,
                    messages,
                    Default::default(),
                    no_import,
                )
                .unwrap_or_else(|r| panic!("enqueue {}", r.error));
            let child = capability::Client::new(remote.pipeline.hook.get_pipelined_cap(&[]));
            let mut child_reply = child
                .new_call::<any_pointer::Owned, any_pointer::Owned>(47, 2, None)
                .send();
            let parent_running = invocations.accept_local(&mut driver).unwrap().unwrap();
            // A projection created after local dispatch must still wait on the
            // parent's exact result; it cannot become an unknown-invocation error.
            call.poll_pipelines(&mut hooks, &mut driver, &mut invocations, messages)
                .unwrap();
            let child_call = next(&mut hooks);
            let mut child_call = hooks
                .enqueue_wire(
                    child_call,
                    &mut driver,
                    &mut invocations,
                    messages,
                    Default::default(),
                    no_import,
                )
                .unwrap_or_else(|r| panic!("child enqueue {}", r.error));
            assert!(invocations.accept_local(&mut driver).unwrap().is_none());
            let output = driver
                .local_result_dynamic(
                    47,
                    1,
                    &[Some(&child_target)],
                    messages,
                    256,
                    Default::default(),
                    |field| field.set_capability_index(0),
                )
                .unwrap();
            invocations
                .complete_local(&mut driver, messages, parent_running, Ok(output))
                .unwrap_or_else(|_| panic!("parent completion"));
            settle(&mut invocations, &mut driver, messages);
            hooks.collect(&mut driver.runtime);
            let child_running = invocations.accept_local(&mut driver).unwrap().unwrap();
            assert_eq!(child_running.task(), child_call.task());
            assert!(call
                .poll(
                    &mut hooks,
                    &mut driver,
                    messages,
                    &mut Context::from_waker(Waker::noop())
                )
                .is_ready());
            let output = driver
                .local_result_dynamic(47, 2, &[], messages, 256, Default::default(), |field| {
                    field
                        .cast::<capnp::field_api::Text>()
                        .copy_from("child ran")
                })
                .unwrap();
            invocations
                .complete_local(&mut driver, messages, child_running, Ok(output))
                .unwrap_or_else(|_| panic!("child completion"));
            settle(&mut invocations, &mut driver, messages);
            assert!(child_call
                .poll(
                    &mut hooks,
                    &mut driver,
                    messages,
                    &mut Context::from_waker(Waker::noop())
                )
                .is_ready());
            assert_eq!(
                ready(&mut child_reply.promise)
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_as::<capnp::text::Reader>()
                    .unwrap(),
                "child ran"
            );
            let response = ready(&mut remote.promise).unwrap();
            assert!(response
                .get()
                .unwrap()
                .get_as_capability::<capability::Client>()
                .is_ok());
            let late = capability::Client::new(remote.pipeline.hook.get_pipelined_cap(&[]));
            let _late_reply = late
                .new_call::<any_pointer::Owned, any_pointer::Owned>(47, 3, None)
                .send();
            assert_eq!(next(&mut hooks).method_id(), 3);
            hooks.close(&mut driver.runtime, Error::disconnected("done".into()));
            driver.runtime.release(target);
            driver.runtime.release(child_target);
        })
    });
}

#[test]
fn wire_reply_exception_preserves_remote_diagnostics() {
    with_pool(|messages| {
        with_peer(4, |mut driver| {
            let target = remote(&mut driver);
            let mut hooks = ClientHooks::new(4, 4);
            let client = hooks.export_client(&mut driver.runtime, &target).unwrap();
            let mut remote = client
                .new_call::<any_pointer::Owned, any_pointer::Owned>(47, 1, None)
                .send();
            let call = next(&mut hooks);
            let mut invocations = connection::Invocations::new_growing(0, 4).unwrap();
            let mut call = hooks
                .enqueue_wire(
                    call,
                    &mut driver,
                    &mut invocations,
                    messages,
                    Default::default(),
                    no_import,
                )
                .unwrap_or_else(|r| panic!("enqueue {}", r.error));
            let mut queue = messages::Queue::new_growing(0, 4).unwrap();
            let sent = invocations
                .send_one(
                    &mut driver,
                    messages,
                    &mut queue,
                    512,
                    Default::default(),
                    &mut [0; 4],
                )
                .unwrap()
                .unwrap();
            let frame = Frame::build(messages, 512, Default::default(), |builder| {
                capntproto_rpc_wire::calls::exception(
                    builder,
                    capntproto_rpc_engine::ids::AnswerId::from_wire(sent.question.id().to_wire()),
                    capnp::ErrorKind::Overloaded,
                    "remote diagnostic",
                )
            })
            .unwrap();
            driver
                .receive(frame, messages, &mut [0; 4], |_| (), || ())
                .unwrap_or_else(|_| panic!("receive"));
            settle(&mut invocations, &mut driver, messages);
            assert!(call
                .poll(
                    &mut hooks,
                    &mut driver,
                    messages,
                    &mut Context::from_waker(Waker::noop())
                )
                .is_ready());
            let error = match ready(&mut remote.promise) {
                Err(error) => error,
                Ok(_) => panic!("exception"),
            };
            assert_eq!(error.kind, capnp::ErrorKind::Overloaded);
            assert!(error.extra.contains("remote diagnostic"));
            hooks.close(&mut driver.runtime, Error::disconnected("done".into()));
            driver.runtime.release(target);
            queue.fail();
            while queue.discard_one(messages).unwrap() {}
        })
    });
}

#[test]
fn rejected_wire_admission_retains_call_and_cancellation_releases_binding() {
    with_pool(|messages| {
        with_peer(2, |mut driver| {
            let target = driver
                .runtime
                .endpoint(Route::local(42), clients::Location::Local)
                .unwrap();
            let mut hooks = ClientHooks::new(4, 4);
            let client = hooks.export_client(&mut driver.runtime, &target).unwrap();
            let mut first = client
                .new_call::<any_pointer::Owned, any_pointer::Owned>(47, 1, None)
                .send();
            let mut second = client
                .new_call::<any_pointer::Owned, any_pointer::Owned>(47, 2, None)
                .send();
            let mut invocations = connection::Invocations::new_growing(0, 4).unwrap();
            let first_call = next(&mut hooks);
            let first_call = hooks
                .enqueue_wire(
                    first_call,
                    &mut driver,
                    &mut invocations,
                    messages,
                    Default::default(),
                    no_import,
                )
                .unwrap_or_else(|r| panic!("enqueue {}", r.error));
            let second_call = next(&mut hooks);
            let rejected = match hooks.enqueue_wire(
                second_call,
                &mut driver,
                &mut invocations,
                messages,
                Default::default(),
                no_import,
            ) {
                Err(rejected) => rejected,
                Ok(_) => panic!("dispatch reserve should reject second call"),
            };
            assert_eq!(rejected.call.method_id(), 2);
            first_call.cancel();
            settle(&mut invocations, &mut driver, messages);
            assert!(invocations.is_empty());
            assert!(ready(&mut first.promise).is_err());
            let second_call = hooks
                .enqueue_wire(
                    rejected.call,
                    &mut driver,
                    &mut invocations,
                    messages,
                    Default::default(),
                    no_import,
                )
                .unwrap_or_else(|r| panic!("retry {}", r.error));
            drop(second_call);
            settle(&mut invocations, &mut driver, messages);
            assert!(invocations.is_empty());
            assert!(ready(&mut second.promise).is_err());
            assert!(driver.runtime.next_ready().unwrap().is_none());
            hooks.close(&mut driver.runtime, Error::disconnected("done".into()));
            driver.runtime.release(target);
        })
    });
}

#[test]
fn dynamic_null_capability_slots_survive_local_and_wire_encoding() {
    with_pool(|messages| {
        with_peer(4, |mut driver| {
            let target = remote(&mut driver);
            let cap = driver
                .runtime
                .endpoint(Route::local(55), clients::Location::Local)
                .unwrap();
            let method = DynamicCall {
                interface: 47,
                method: 1,
                no_promise_pipelining: false,
                only_promise_pipeline: true,
            };
            let task = driver
                .enqueue_dynamic(
                    &target,
                    method,
                    &[None, Some(&cap)],
                    messages,
                    256,
                    Default::default(),
                    |params| params.set_capability_index(1),
                )
                .unwrap();
            let mut queue = messages::Queue::new_growing(0, 4).unwrap();
            let sent = driver
                .send_invocation(messages, &mut queue, 512, Default::default(), &mut [0; 4])
                .unwrap()
                .unwrap();
            assert_eq!(sent.task, task);
            let mut words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 128];
            let bytes = capnp::Word::words_to_bytes_mut(&mut words);
            let mut used = 0;
            while let Some(window) = queue.window(messages).unwrap() {
                let length = window.bytes().len();
                bytes[used..used + length].copy_from_slice(window.bytes());
                used += length;
                window.advance(length).unwrap();
            }
            let reader = capnp::serialize::read_message_from_flat_slice_no_alloc(
                &mut &bytes[..used],
                Default::default(),
            )
            .unwrap();
            let rpc_capnp::message::Call(call) = reader
                .get_root::<rpc_capnp::message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            else {
                panic!("call")
            };
            let call = call.unwrap();
            assert!(
                !call.get_only_promise_pipeline(),
                "ordinary question still requires a Return"
            );
            let caps = call.get_params().unwrap().get_cap_table().unwrap();
            assert_eq!(caps.len(), 2);
            assert!(matches!(
                caps.get(0).which().unwrap(),
                rpc_capnp::cap_descriptor::None(())
            ));
            assert!(matches!(
                caps.get(1).which().unwrap(),
                rpc_capnp::cap_descriptor::SenderHosted(_)
            ));
            driver.cancel(sent.question).unwrap();
            driver.runtime.release(cap);
            driver.runtime.release(target);
        })
    });
}

#[test]
fn malformed_dynamic_payload_rolls_back_all_admission() {
    with_pool(|messages| {
        with_peer(4, |mut driver| {
            let target = driver
                .runtime
                .endpoint(Route::local(42), clients::Location::Local)
                .unwrap();
            let method = DynamicCall {
                interface: 47,
                method: 1,
                ..Default::default()
            };
            assert!(driver
                .enqueue_dynamic(
                    &target,
                    method,
                    &[],
                    messages,
                    256,
                    Default::default(),
                    |params| params.set_capability_index(1)
                )
                .is_err());
            assert!(driver.runtime.next_ready().unwrap().is_none());
            // A subsequent valid call can use the same capacity and message pool.
            let task = driver
                .enqueue_dynamic(
                    &target,
                    method,
                    &[],
                    messages,
                    256,
                    Default::default(),
                    |_params| Ok(()),
                )
                .unwrap();
            assert!(driver.cancel_invocation(task, messages));
            driver.runtime.release(target);
        })
    });
}

#[test]
fn pipeline_only_interest_retains_parent_until_final_pipeline_drop() {
    with_pool(|messages| {
        with_peer(4, |mut driver| {
            let target = driver
                .runtime
                .endpoint(Route::local(42), clients::Location::Local)
                .unwrap();
            let mut hooks = ClientHooks::new(4, 4);
            let client = hooks.export_client(&mut driver.runtime, &target).unwrap();
            let pipeline = client
                .new_call::<any_pointer::Owned, any_pointer::Owned>(47, 1, None)
                .send_for_pipeline();
            let call = next(&mut hooks);
            let mut invocations = connection::Invocations::new_growing(0, 4).unwrap();
            let mut call = hooks
                .enqueue_wire(
                    call,
                    &mut driver,
                    &mut invocations,
                    messages,
                    Default::default(),
                    no_import,
                )
                .unwrap_or_else(|r| panic!("enqueue {}", r.error));
            assert!(call
                .poll_canceled(&mut Context::from_waker(Waker::noop()))
                .is_pending());
            let child = pipeline.hook.get_pipelined_cap(&[]);
            drop(pipeline);
            assert!(call
                .poll_canceled(&mut Context::from_waker(Waker::noop()))
                .is_pending());
            drop(child);
            assert!(call
                .poll(
                    &mut hooks,
                    &mut driver,
                    messages,
                    &mut Context::from_waker(Waker::noop())
                )
                .is_ready());
            settle(&mut invocations, &mut driver, messages);
            assert!(invocations.is_empty());
            assert!(driver.runtime.next_ready().unwrap().is_none());
            hooks.close(&mut driver.runtime, Error::disconnected("done".into()));
            driver.runtime.release(target);
        })
    });
}

#[test]
fn pipeline_drop_releases_interest_before_synchronous_wake() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    thread_local! {
        static OBSERVED: RefCell<Option<Weak<PipelineState>>> = const { RefCell::new(None) };
    }
    struct Observe(AtomicUsize);
    impl std::task::Wake for Observe {
        fn wake(self: Arc<Self>) {
            OBSERVED.with(|slot| {
                self.0.store(
                    slot.borrow().as_ref().unwrap().strong_count(),
                    Ordering::SeqCst,
                )
            });
        }
    }
    let mut hooks = ClientHooks::new(1, 1);
    let pipeline = Rc::new(PipelineState {
        used: Cell::new(false),
        adopted: RefCell::default(),
        cached: RefCell::default(),
        _credit: None,
        waiting: RefCell::new(VecDeque::new()),
        finished: RefCell::new(None),
        inbox: Rc::downgrade(&hooks.inbox),
        maximum: 1,
    });
    OBSERVED.with(|slot| *slot.borrow_mut() = Some(Rc::downgrade(&pipeline)));
    let observe = Arc::new(Observe(AtomicUsize::new(usize::MAX)));
    let waker = Waker::from(observe.clone());
    assert!(hooks
        .poll_next(&mut Context::from_waker(&waker))
        .is_pending());
    drop(WirePipeline(PipelineRef(Some(pipeline.clone()))));
    assert_eq!(observe.0.load(Ordering::SeqCst), 1);
    OBSERVED.with(|slot| slot.borrow_mut().take());
}

#[test]
fn prepared_local_server_keeps_parameters_after_original_frame_release() {
    with_pool(|messages| {
        with_peer(4, |mut driver| {
            let target = driver
                .runtime
                .endpoint(Route::local(42), clients::Location::Local)
                .unwrap();
            let value = driver
                .runtime
                .endpoint(Route::local(43), clients::Location::Local)
                .unwrap();
            let mut hooks = ClientHooks::new(8, 4);
            let mut invocations = connection::Invocations::new_growing(0, 4).unwrap();
            let (sender, receiver) = shared::channel();
            invocations
                .enqueue_dynamic(
                    &mut driver,
                    sender.into(),
                    &target,
                    DynamicCall {
                        interface: 47,
                        method: 6,
                        no_promise_pipelining: true,
                        only_promise_pipeline: false,
                    },
                    &[None, Some(&value)],
                    messages,
                    512,
                    Default::default(),
                    |field| field.set_capability_index(1),
                )
                .unwrap_or_else(|_| panic!("enqueue"));
            let running = invocations.accept_local(&mut driver).unwrap().unwrap();
            let prepared = hooks
                .prepare_local_server(&running, &mut driver, messages)
                .unwrap();
            invocations
                .complete_local(
                    &mut driver,
                    messages,
                    running,
                    Err(capnp::ErrorKind::Failed),
                )
                .unwrap_or_else(|_| panic!("complete"));
            assert_eq!(prepared.method().interface, 47);
            assert_eq!(prepared.method().method, 6);
            assert!(prepared.method().no_promise_pipelining);
            let pointer =
                capability::Client::new(prepared.params().unwrap().get_pipelined_cap(&[]).unwrap());
            let retained = hooks
                .try_retain(&mut driver.runtime, &*pointer.hook)
                .unwrap()
                .unwrap();
            assert_eq!(retained.key(), value.key());
            driver.runtime.release(retained);

            // Use an ordinary hosted result context through the server boundary.
            // Dispatch itself remains synchronous; its completion future and
            // result/pipeline ownership are the original hook machinery.
            let server = hooks.export_client(&mut driver.runtime, &target).unwrap();
            let mut reply = server
                .new_call::<any_pointer::Owned, any_pointer::Owned>(1, 1, None)
                .send();
            let Call {
                results,
                mut completion,
                ..
            } = next(&mut hooks);
            let mut execution = prepared.dispatch(&*server.hook, results.into_hook());
            let mut delivered = next(&mut hooks);
            assert_eq!(delivered.interface_id(), 47);
            assert_eq!(delivered.method_id(), 6);
            assert!(delivered.hints().no_promise_pipelining);
            let cap = delivered.params().unwrap().get_pipelined_cap(&[]).unwrap();
            let retained = hooks
                .try_retain(&mut driver.runtime, &*cap)
                .unwrap()
                .unwrap();
            assert_eq!(retained.key(), value.key());
            driver.runtime.release(retained);
            delivered
                .results()
                .get()
                .unwrap()
                .set_as::<capnp::text::Owned>("server result")
                .unwrap();
            delivered.complete(Ok(()));
            completion.complete(ready(&mut execution));
            assert_eq!(
                ready(&mut reply.promise)
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_as::<capnp::text::Reader>()
                    .unwrap(),
                "server result"
            );
            drop((pointer, receiver, cap, server, reply));
            settle(&mut invocations, &mut driver, messages);
            assert!(invocations.is_empty());
            hooks.close(&mut driver.runtime, Error::disconnected("done".into()));
            driver.runtime.release(target);
            driver.runtime.release(value);
        });
    });
}

#[test]
fn received_server_uses_admitted_receipts_and_recovers_from_hook_capacity() {
    use capntproto_rpc_engine::ids::{ExportId, QuestionId};
    use capntproto_rpc_wire::clients as binding;
    with_pool(|messages| {
        with_peer(4, |mut driver| {
            let target = driver
                .runtime
                .endpoint(Route::local(42), clients::Location::Local)
                .unwrap();
            let export = {
                let mut batch = driver.ledgers.exports.batch();
                let grant = binding::stage(&mut driver.runtime, &mut batch, &target).unwrap();
                batch.commit();
                grant.id()
            };
            let frame = Frame::build(messages, 512, Default::default(), |builder| {
                use capntproto_rpc_wire::rpc_capnp::api::{
                    Call as CallSchema, CapDescriptor, Message, Payload as PayloadSchema,
                };
                capntproto_rpc_wire::calls::dynamic_call(
                    builder,
                    QuestionId::from_wire(9),
                    capntproto_rpc_wire::calls::Target::Imported(ImportId::from_wire(
                        export.to_wire(),
                    )),
                    DynamicCall {
                        interface: 47,
                        method: 3,
                        no_promise_pipelining: false,
                        only_promise_pipeline: false,
                    },
                    |field| field.set_capability_index(1),
                )?;
                let mut root = builder.try_get_root::<Message>()?;
                let mut call = root.field(Message::CALL).edit()?;
                let mut payload = call.field(CallSchema::PARAMS).edit()?;
                let mut table = payload.field(PayloadSchema::CAP_TABLE).init(2)?;
                table.get_mut(0).unwrap().void(CapDescriptor::NONE).set();
                table
                    .get_mut(1)
                    .unwrap()
                    .scalar(CapDescriptor::SENDER_HOSTED)
                    .set(71);
                Ok(())
            })
            .unwrap();
            let call = connection::Request::admit(
                frame,
                messages,
                &mut driver.ledgers.imports,
                &mut driver.ledgers.exports,
                &mut driver.ledgers.answers,
                connection::Answer::new(owned::pipelines::Results::new(())),
                |_| (),
            )
            .unwrap_or_else(|_| panic!("admit"));
            assert!(call.share_plain_params(messages).unwrap().is_none());
            let mut full = ClientHooks::new(0, 4);
            assert!(full
                .prepare_received_server(&call, &mut driver, messages)
                .is_err());
            full.close(&mut driver.runtime, Error::disconnected("full".into()));
            let mut hooks = ClientHooks::new(4, 4);
            let prepared = hooks
                .prepare_received_server(&call, &mut driver, messages)
                .unwrap();
            driver.release_call(call, messages);
            assert_eq!(prepared.method().method, 3);
            let pointer =
                capability::Client::new(prepared.params().unwrap().get_pipelined_cap(&[]).unwrap());
            let retained = hooks
                .try_retain(&mut driver.runtime, &*pointer.hook)
                .unwrap()
                .unwrap();
            assert!(
                matches!(driver.runtime.inspect(&retained).unwrap(), clients::Destination::Endpoint {
                owner: Route::Capability(binding::Endpoint::Imported(remote)), ..
            } if remote.id() == ImportId::from_wire(71))
            );
            driver.runtime.release(retained);
            drop((prepared, pointer));
            hooks.close(&mut driver.runtime, Error::disconnected("done".into()));
            let exported = driver
                .ledgers
                .exports
                .release(ExportId::from_wire(export.to_wire()), 1)
                .unwrap()
                .unwrap();
            driver.runtime.release(exported);
            driver.runtime.release(target);
        });
    });
}

#[test]
fn late_invalid_local_projection_breaks_instead_of_waiting_for_retry() {
    with_pool(|messages| {
        with_peer(4, |mut driver| {
            let target = driver
                .runtime
                .endpoint(Route::local(42), clients::Location::Local)
                .unwrap();
            let mut calls = connection::Invocations::new_growing(0, 4).unwrap();
            let (sender, receiver) = shared::channel();
            let task = calls
                .enqueue_dynamic(
                    &mut driver,
                    sender.into(),
                    &target,
                    DynamicCall {
                        interface: 47,
                        method: 1,
                        ..Default::default()
                    },
                    &[],
                    messages,
                    256,
                    Default::default(),
                    |_| Ok(()),
                )
                .unwrap_or_else(|_| panic!("enqueue"));
            let running = calls.accept_local(&mut driver).unwrap().unwrap();
            let result = driver
                .local_result_dynamic(47, 1, &[], messages, 256, Default::default(), |field| {
                    field
                        .cast::<capnp::field_api::Text>()
                        .copy_from("not a capability")
                })
                .unwrap();
            calls
                .complete_local(&mut driver, messages, running, Ok(result))
                .unwrap_or_else(|_| panic!("complete"));
            let projected = calls
                .pipeline(&mut driver, task, pipeline::Path::new(&[]), messages)
                .unwrap();
            assert!(matches!(
                driver.runtime.inspect(&projected).unwrap(),
                clients::Destination::Endpoint {
                    location: clients::Location::Broken,
                    ..
                }
            ));
            driver.runtime.release(projected);
            drop(receiver);
            settle(&mut calls, &mut driver, messages);
            assert!(calls.is_empty());
            driver.runtime.release(target);
        });
    });
}

#[test]
fn redirected_server_calls_cannot_suppress_required_result_completion() {
    with_pool(|messages| {
        for destination in 0..3 {
            let frame = Frame::build(messages, 512, Default::default(), |builder| {
                use capntproto_rpc_wire::rpc_capnp::api::{
                    call::SendResultsTo, Call as CallSchema, Message,
                };
                capntproto_rpc_wire::calls::dynamic_call(
                    builder,
                    capntproto_rpc_engine::ids::QuestionId::from_wire(9),
                    capntproto_rpc_wire::calls::Target::Imported(ImportId::from_wire(7)),
                    DynamicCall {
                        only_promise_pipeline: true,
                        ..Default::default()
                    },
                    |_| Ok(()),
                )?;
                let mut root = builder.try_get_root::<Message>()?;
                let mut call = root.field(Message::CALL).edit()?;
                let mut to = call.group(CallSchema::SEND_RESULTS_TO).select();
                match destination {
                    0 => to.void(SendResultsTo::CALLER).set(),
                    1 => to.void(SendResultsTo::YOURSELF).set(),
                    _ => to.field(SendResultsTo::THIRD_PARTY).clear(),
                }
                Ok(())
            })
            .unwrap();
            frame
                .with(messages, |root| {
                    let rpc_capnp::message::Call(call) = root.which().unwrap() else {
                        panic!("call")
                    };
                    let incoming = capntproto_rpc_wire::calls::Incoming::new(call.unwrap());
                    assert_eq!(incoming.hints().only_promise_pipeline, destination == 0);
                })
                .unwrap();
            frame.release(messages);
        }
    });
}

#[test]
fn hosted_hook_bridge_exceeds_inline_capability_capacity_in_both_directions() {
    use capntproto_rpc_wire::Capacity;
    with_pool(|messages| {
        with_peer_storage::<0, 0, _>(
            8,
            Capacity::Growing { maximum: 24 },
            Capacity::Growing { maximum: 8 },
            |mut driver| {
                let target = driver
                    .runtime
                    .endpoint(Route::local(42), clients::Location::Local)
                    .unwrap();
                let value = driver
                    .runtime
                    .endpoint(Route::local(55), clients::Location::Local)
                    .unwrap();
                let mut hooks = ClientHooks::new(8, 8);
                let client = hooks.export_client(&mut driver.runtime, &target).unwrap();
                let cap = hooks.export_client(&mut driver.runtime, &value).unwrap();
                let mut request =
                    client.new_call::<any_pointer::Owned, any_pointer::Owned>(47, 1, None);
                let mut list = request
                    .get()
                    .initn_as::<capnp::capability_list::Builder<capability::Client>>(20);
                for index in 0..20 {
                    list.set(index, cap.hook.add_ref());
                }
                let mut remote = request.send();
                let queued = next(&mut hooks);
                let mut invocations = connection::Invocations::new_growing(0, 8).unwrap();
                let mut call = hooks
                    .enqueue_wire(
                        queued,
                        &mut driver,
                        &mut invocations,
                        messages,
                        Default::default(),
                        no_import,
                    )
                    .unwrap_or_else(|r| panic!("{}", r.error));
                let running = invocations.accept_local(&mut driver).unwrap().unwrap();
                running
                    .params(messages, |params| {
                        assert_eq!(params.capabilities().len(), 20);
                        for cap in params.capabilities() {
                            assert_eq!(cap.as_ref().unwrap().key(), value.key());
                        }
                    })
                    .unwrap();
                let caps = [Some(&value); 20];
                let output = driver
                    .local_result_dynamic(
                        47,
                        1,
                        &caps,
                        messages,
                        512,
                        Default::default(),
                        |field| field.set_capability_index(19),
                    )
                    .unwrap();
                invocations
                    .complete_local(&mut driver, messages, running, Ok(output))
                    .unwrap_or_else(|_| panic!("local complete"));
                settle(&mut invocations, &mut driver, messages);
                assert!(call
                    .poll(
                        &mut hooks,
                        &mut driver,
                        messages,
                        &mut Context::from_waker(Waker::noop())
                    )
                    .is_ready());
                let reply = ready(&mut remote.promise).unwrap();
                let result = reply
                    .get()
                    .unwrap()
                    .get_as_capability::<capability::Client>()
                    .unwrap();
                let retained = hooks
                    .try_retain(&mut driver.runtime, &*result.hook)
                    .unwrap()
                    .unwrap();
                assert_eq!(retained.key(), value.key());
                driver.runtime.release(retained);
                drop((call, reply, result, remote.pipeline, cap, client));
                settle(&mut invocations, &mut driver, messages);
                hooks.close(&mut driver.runtime, Error::disconnected("done".into()));
                driver.runtime.release(value);
                driver.runtime.release(target);
            },
        );
    });
}

#[test]
fn retained_plain_params_keep_segments_and_outlive_the_connection() {
    use capntproto_rpc_engine::ids::ExportId;
    use capntproto_rpc_wire::clients as binding;
    fn prepare<'p>(
        messages: &mut messages::Messages<'p, '_, '_>,
        segmented: bool,
        shared: bool,
    ) -> server::Prepared {
        with_peer(4, |mut driver| {
            let target = driver
                .runtime
                .endpoint(Route::local(42), clients::Location::Local)
                .unwrap();
            let export = {
                let mut batch = driver.ledgers.exports.batch();
                let grant = binding::stage(&mut driver.runtime, &mut batch, &target).unwrap();
                batch.commit();
                grant.id()
            };
            let mut builder = capnp::message::Builder::new(
                capnp::message::HeapAllocator::new().first_segment_words(if segmented {
                    1
                } else {
                    128
                }),
            );
            {
                let root = builder.init_root::<rpc_capnp::message::Builder<'_>>();
                let mut call = root.init_call();
                call.set_question_id(9);
                call.set_interface_id(47);
                call.set_method_id(3);
                call.set_no_promise_pipelining(true);
                call.reborrow()
                    .init_target()
                    .set_imported_cap(export.to_wire());
                call.reborrow().init_send_results_to().set_caller(());
                call.init_params()
                    .get_content()
                    .set_as::<capnp::data::Owned>(b"retained parameters".as_slice())
                    .unwrap();
            }
            assert_eq!(builder.get_segments_for_output().len() > 1, segmented);
            let bytes = capnp::serialize::write_message_to_words(&builder);
            let frame = Frame::copy_from(messages, &bytes, Default::default()).unwrap();
            let call = connection::Request::admit(
                frame,
                messages,
                &mut driver.ledgers.imports,
                &mut driver.ledgers.exports,
                &mut driver.ledgers.answers,
                connection::Answer::new(owned::pipelines::Results::new(())),
                |_| (),
            )
            .unwrap_or_else(|_| panic!("admit"));
            assert_eq!(call.share_plain_params(messages).unwrap().is_some(), shared);
            let original = call
                .with(messages, |p| {
                    p.incoming()
                        .payload()
                        .unwrap()
                        .get_content()
                        .get_as::<capnp::data::Reader>()
                        .unwrap()
                        .as_ptr()
                })
                .unwrap();
            let mut hooks = ClientHooks::new(0, 0);
            let prepared = hooks
                .prepare_received_server(&call, &mut driver, messages)
                .unwrap();
            assert_eq!(prepared.method().interface, 47);
            assert_eq!(prepared.method().method, 3);
            assert!(prepared.method().no_promise_pipelining);
            let payload = prepared;
            assert_eq!(
                payload
                    .params()
                    .unwrap()
                    .get_as::<capnp::data::Reader>()
                    .unwrap()
                    .as_ptr()
                    == original,
                shared
            );
            driver.release_call(call, messages);
            hooks.close(&mut driver.runtime, Error::disconnected("done".into()));
            let exported = driver
                .ledgers
                .exports
                .release(ExportId::from_wire(export.to_wire()), 1)
                .unwrap()
                .unwrap();
            driver.runtime.release(exported);
            driver.runtime.release(target);
            payload
        })
    }
    for segmented in [false, true] {
        for shared in [false, true] {
            let payload = if shared {
                with_pool(|messages| prepare(messages, segmented, true))
            } else {
                // Caller-owned wire storage must guarantee eight-byte alignment,
                // including on 32-bit targets and under Miri.
                let mut words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 512];
                let mut buffers = [messages::Buffer::new(capnp::Word::words_to_bytes_mut(
                    &mut words,
                ))];
                messages::with_messages(&mut buffers, &mut [], |messages| {
                    prepare(messages, segmented, false)
                })
                .unwrap()
            };
            assert_eq!(
                payload
                    .params()
                    .unwrap()
                    .get_as::<capnp::data::Reader>()
                    .unwrap(),
                b"retained parameters"
            );
        }
    }
}
