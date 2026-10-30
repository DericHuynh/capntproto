//! Two owning connection domains, fault-injected transport and bounded queues.
use super::portable_capnp::{
    api::echo::{EchoParams, EchoResults},
    echo,
};
#[cfg(not(feature = "growing"))]
use capntproto_rpc_engine::{answers, clients::dispatch, exports, imports, questions};
use capntproto_rpc_engine::{
    clients::{self, Capabilities},
    connection,
    ids::WireId,
    messages,
};
#[cfg(not(feature = "growing"))]
use capntproto_rpc_wire::owned::pipelines;
use capntproto_rpc_wire::{
    calls, clients as binding,
    owned::{
        self,
        connection::{Driver, Received},
        Frame,
    },
    pipeline,
    rpc_capnp::api::{CapDescriptor, Message, Payload, Return},
};

#[cfg(not(feature = "growing"))]
use capntproto_rpc_wire::{
    clients::outgoing,
    owned::connection::{ExchangeSlot, Storage},
};

type Peer<'p, 'id, 's> = Driver<'p, 'id, 's, (), u32, u8, u32, 4, 4>;
/// Replay tracked result ownership with either fixed or shared completion slots.
pub(super) fn reply_pair<T>(
    slot: &capntproto_rpc_engine::completion::Slot<T>,
) -> (
    capntproto_rpc_engine::completion::Sender<'_, T>,
    capntproto_rpc_engine::completion::Receiver<'_, T>,
) {
    #[cfg(not(feature = "growing"))]
    {
        slot.try_pair().unwrap()
    }
    #[cfg(feature = "growing")]
    {
        let _ = slot;
        let (sender, receiver) = capntproto_rpc_engine::completion::shared::channel();
        (sender.into(), receiver.into())
    }
}
/// Replay the same conversations with identical capacity limits. The growing
/// profile starts with no message buffers, send records or queue slots.
pub(super) fn message_scope<R>(
    buffers: &mut [messages::Buffer<'_>],
    sends: &mut [messages::SendSlot],
    f: impl for<'id, 'storage, 'bytes> FnOnce(&mut messages::Messages<'id, 'storage, 'bytes>) -> R,
) -> Result<R, messages::Error> {
    #[cfg(not(feature = "growing"))]
    {
        messages::with_messages(buffers, sends, f)
    }
    #[cfg(feature = "growing")]
    {
        let bytes = buffers.iter().map(|buffer| buffer.capacity() + 7).sum();
        let maximum = buffers
            .iter()
            .map(messages::Buffer::capacity)
            .max()
            .unwrap_or(0);
        let mut growing = messages::GrowingBuffers::new(buffers.len(), maximum, bytes);
        messages::with_growing_messages(
            &mut growing,
            connection::Growth {
                initial: 0,
                maximum: sends.len(),
            },
            f,
        )
    }
}
pub(super) fn message_queue<'pool, 'storage>(
    slots: &'storage mut [Option<messages::QueueEntry<'pool>>],
) -> Result<messages::Queue<'pool, 'storage>, messages::Error> {
    #[cfg(not(feature = "growing"))]
    {
        messages::Queue::new(slots)
    }
    #[cfg(feature = "growing")]
    {
        messages::Queue::new_growing(0, slots.len())
    }
}
pub(super) fn with_peer<'p, R>(f: impl for<'id, 's> FnOnce(&mut Peer<'p, 'id, 's>) -> R) -> R {
    with_owned_peer(|mut peer| f(&mut peer))
}
pub(super) fn with_owned_peer<'p, R>(f: impl for<'id, 's> FnOnce(Peer<'p, 'id, 's>) -> R) -> R {
    with_owned_peer_tasks::<2, R>(f)
}
pub(super) fn with_owned_peer_tasks<'p, const TASKS: usize, R>(
    f: impl for<'id, 's> FnOnce(Peer<'p, 'id, 's>) -> R,
) -> R {
    with_owned_peer_storage::<TASKS, 0, R>(f)
}
pub(super) fn with_owned_peer_storage<'p, const TASKS: usize, const ADOPTED: usize, R>(
    f: impl for<'id, 's> FnOnce(Peer<'p, 'id, 's>) -> R,
) -> R {
    connection::with_domain(|domain| {
        #[cfg(not(feature = "growing"))]
        let mut is = [const { imports::Slot::EMPTY }; 4];
        #[cfg(not(feature = "growing"))]
        let mut es = [const { exports::Slot::EMPTY }; 4];
        #[cfg(not(feature = "growing"))]
        let mut qs = [const { questions::Slot::EMPTY }; 4];
        #[cfg(not(feature = "growing"))]
        let mut adopted = [const { questions::AdoptedSlot::EMPTY }; ADOPTED];
        #[cfg(not(feature = "growing"))]
        let mut ans = [const { answers::Slot::EMPTY }; 4];
        #[cfg(not(feature = "growing"))]
        let mut ns = [const { clients::Slot::EMPTY }; 12];
        #[cfg(not(feature = "growing"))]
        let mut listeners = [const { clients::ListenerSlot::EMPTY }; 8];
        #[cfg(not(feature = "growing"))]
        let mut subscriptions = [const { clients::SubscriptionSlot::EMPTY }; 48];
        #[cfg(not(feature = "growing"))]
        let connection = domain
            .bind_with_adoptions(
                connection::GraphStorage {
                    notifications: clients::NotificationStorage {
                        listeners: &mut listeners,
                        subscriptions: &mut subscriptions,
                    },
                    imports: &mut is,
                    exports: &mut es,
                    questions: &mut qs,
                    pipeline_questions: &mut [],
                    answers: &mut ans,
                    clients: &mut ns,
                },
                &mut adopted,
            )
            .unwrap();
        #[cfg(feature = "growing")]
        let connection = {
            let bound = connection::Growth {
                initial: 0,
                maximum: 4,
            };
            domain
                .bind_growing_adoptions(
                    connection::GrowingStorage {
                        notifications:
                            capntproto_rpc_engine::clients::NotificationGrowth::for_clients(
                                connection::Growth {
                                    initial: 0,
                                    maximum: 12,
                                },
                            ),
                        imports: bound,
                        exports: bound,
                        questions: bound,
                        pipeline_questions: bound,
                        answers: bound,
                        clients: connection::Growth {
                            initial: 0,
                            maximum: 12,
                        },
                    },
                    connection::Growth {
                        initial: 0,
                        maximum: ADOPTED,
                    },
                )
                .unwrap()
        };
        #[cfg(not(feature = "growing"))]
        let mut tasks = [const { dispatch::TaskSlot::EMPTY }; TASKS];
        #[cfg(not(feature = "growing"))]
        let mut pipelines = [const { pipelines::Slot::EMPTY }; 4];
        #[cfg(not(feature = "growing"))]
        let mut outgoing = [const { outgoing::Slot::EMPTY }; 4];
        #[cfg(not(feature = "growing"))]
        let mut exported = [const { binding::exporting::Slot::EMPTY }; 4];
        #[cfg(not(feature = "growing"))]
        let mut loopbacks = [const { owned::loopback::Slot::EMPTY }; 2];
        #[cfg(not(feature = "growing"))]
        let mut embargoes = [const { owned::loopback::ReceiptSlot::EMPTY }; 2];
        #[cfg(not(feature = "growing"))]
        let mut exchanges = [const { ExchangeSlot::EMPTY }; 4];
        #[cfg(not(feature = "growing"))]
        let mut invocation_pipelines =
            [const { owned::connection::InvocationPipelineSlot::EMPTY }; 4];
        #[cfg(not(feature = "growing"))]
        let mut operations = [const { owned::connection::OperationSlot::EMPTY }; 16];
        #[cfg(not(feature = "growing"))]
        let driver = Driver::new(
            connection,
            Storage {
                tasks: &mut tasks,
                pipelines: &mut pipelines,
                outgoing: &mut outgoing,
                exported: &mut exported,
                loopbacks: &mut loopbacks,
                embargoes: &mut embargoes,
                exchanges: &mut exchanges,
                invocation_pipelines: &mut invocation_pipelines,
                operations: &mut operations,
            },
            1,
        )
        .unwrap_or_else(|_| panic!("connection"));
        #[cfg(feature = "growing")]
        let driver = {
            let mut storage = owned::connection::GrowingStorage::uniform(0, 4);
            storage.tasks.maximum = TASKS;
            storage.loopbacks.maximum = 2;
            storage.embargoes.maximum = 2;
            Driver::new_growing(connection, storage, 1)
                .unwrap_or_else(|_| panic!("growing connection"))
        };
        f(driver)
    })
}
fn receive<'p, 'id>(
    peer: &mut Peer<'p, 'id, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_>,
    chunk: usize,
) -> Received<'id, u32> {
    let frame = super::owning::exchange(messages, queue, chunk);
    peer.receive(frame, messages, &mut [0; 4], |_| (), || 43)
        .unwrap_or_else(|rejected| panic!("receive: {}", rejected.error))
}
fn finish<'p>(
    sender: &mut Peer<'p, '_, '_>,
    receiver: &mut Peer<'p, '_, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_>,
    chunk: usize,
) {
    assert!(sender
        .send_pending_finish(messages, queue, 776, Default::default())
        .unwrap());
    assert!(matches!(
        receive(receiver, messages, queue, chunk),
        Received::Finished { .. } | Received::IgnoredFinish
    ));
}

pub fn owning_connection_driver(chunk: usize, cancel: bool) {
    fn idle<'p>(
        peer: &mut Peer<'p, '_, '_>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_>,
    ) {
        for _ in 0..100 {
            match peer
                .poll_maintenance(
                    messages,
                    queue,
                    776,
                    Default::default(),
                    &mut [0; 4],
                    core::num::NonZeroUsize::new(1).unwrap(),
                )
                .unwrap()
            {
                owned::connection::Progress::Idle => return,
                owned::connection::Progress::Yield => (),
                _ => panic!("quiescent connection"),
            }
        }
        panic!("maintenance failed to detect idle");
    }
    let mut words = [[super::ZERO; 97]; 12];
    let mut buffers: [messages::Buffer<'_>; 12] = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [const { messages::SendSlot::EMPTY }; 2], |messages| {
        let mut queue_slots = [None, None];
        let mut queue = super::driver::message_queue(&mut queue_slots).unwrap();
        with_peer(|client| with_peer(|server| {
            let service = server.capabilities().endpoint(binding::Route::local(41), clients::Location::Local).unwrap();
            let bootstrap = client.send(messages, &mut queue, 776, Default::default(), calls::bootstrap).unwrap();
            let remote = client.pipeline(bootstrap, pipeline::Path::new(&[])).unwrap();
            let Received::Bootstrap(incoming) = receive(server, messages, &mut queue, chunk) else { panic!("bootstrap") };
            server.complete_bootstrap(&incoming, Ok(&service), messages, 776, Default::default()).unwrap();
            assert!(server.send_answer(incoming.answer(), messages, &mut queue, 776, Default::default(), &mut [0; 4], |_, _| Ok(())).unwrap().is_none());
            assert!(matches!(receive(client, messages, &mut queue, chunk), Received::Returned(key, _) if key == bootstrap));
            assert!(matches!(client.take_response(bootstrap), Err(owned::connection::Error::PendingResponse)));
            assert!(matches!(client.poll_outgoing(0, messages).unwrap(), owned::connection::Propagation::Resolved));
            let response = client.take_response(bootstrap).unwrap();
            client.release_response(response, messages);
            // Retired original question routes are drained before Finish.
            while let Some(server) = client.drain_endpoint() { assert!(server.is_none()); }
            finish(client, server, messages, &mut queue, chunk);

            let target = match client.runtime().inspect(&remote).unwrap() {
                clients::Destination::Endpoint { owner: binding::Route::Capability(binding::Endpoint::Imported(remote)), .. } => remote.id(),
                _ => panic!("imported result"),
            };
            let mut calls = [None, None];
            let mut answers = [None, None];
            for i in 0..2 {
                calls[i] = Some(client.send(messages, &mut queue, 776, Default::default(), |builder, id| calls::call(builder, id, target, echo::Owned::ECHO, |mut params| { params.scalar(EchoParams::VALUE).set(41 + i as u64); Ok(()) })).unwrap());
                answers[i] = Some(match receive(server, messages, &mut queue, chunk) {
                    Received::Queued(key) if i == 0 => key,
                    Received::DeferredCall(key) if i == 1 => key,
                    _ => panic!("bounded task queue"),
                });
            }
            // A duplicate cannot invoke the application factory while the
            // once-admitted second call awaits queue capacity.
            let frame = Frame::build(messages, 776, Default::default(), |builder| calls::call(builder, calls[1].unwrap().id(), target, echo::Owned::ECHO, |_| Ok(()))).unwrap();
            let rejected = server.receive(frame, messages, &mut [0; 4], |_| panic!("factory"), || panic!("producer")).err().unwrap();
            assert_eq!(rejected.error, owned::connection::Error::Busy);
            assert!(rejected.producer.is_none());
            rejected.frame.release(messages);
            if cancel {
                client.cancel(calls[0].unwrap()).unwrap();
                finish(client, server, messages, &mut queue, chunk);
                // Fair maintenance reaches cancellation even though the first
                // deferred-admission attempt sees a full dispatch queue.
                let mut canceled = false;
                for _ in 0..100 {
                    match server.poll_maintenance(messages, &mut queue, 776, Default::default(), &mut [0; 4], core::num::NonZeroUsize::new(1).unwrap()) {
                        Ok(owned::connection::Progress::Producer(43)) => { canceled = true; break; }
                        Ok(owned::connection::Progress::Advanced | owned::connection::Progress::Yield) => (),
                        Err(owned::connection::Error::Enqueue(_)) => (),
                        _ => panic!("unexpected maintenance result"),
                    }
                }
                assert!(canceled);
                assert_eq!(server.runtime().queued(), 0);
                assert_eq!(server.retry_call().unwrap(), answers[1]);
                assert!(matches!(receive(client, messages, &mut queue, chunk), Received::Returned(key, _) if key == calls[0].unwrap()));
                assert!(client.drain_canceled_response(messages));
            }
            for i in usize::from(cancel)..2 {
                let mut dispatch = server.capabilities();
                let running = dispatch.next_ready().unwrap().unwrap().accept();
                let request = running.owner().as_call().unwrap();
                assert_eq!(request.answer(), answers[i].unwrap());
                let active = server.answers().dispatch(request.answer(), true).unwrap();
                assert_eq!(active, i != 0 || !cancel);
                let value = request.with(messages, |view| view.incoming().dispatch(echo::Owned::ECHO).map(|typed| typed.params.value())).unwrap().unwrap();
                assert_eq!(value, 41 + i as u64);
                let frame = Frame::build(messages, 776, Default::default(), |builder| {
                    if active {
                        calls::results(builder, request.answer().id(), echo::Owned::ECHO, |mut results| { results.scalar(EchoResults::VALUE).set(value + 1); Ok(()) })
                    } else { calls::cancelled(builder, request.answer().id()) }
                }).unwrap();
                if let Some(state) = server.publish_answer_frame(request.answer(), messages, frame, queue.reserve().unwrap()).unwrap() {
                    assert_eq!(server.release_answer(state, messages), 43);
                }
                let request = server.capabilities().complete(running).into_call().unwrap_or_else(|_| panic!("Call"));
                server.release_call(request, messages);
                if i == 0 {
                    assert!(matches!(server.enqueue_call(&service, echo::Owned::ECHO, &[], messages, 776, Default::default(), |_| panic!("new calls cannot bypass deferred input")), Err(owned::connection::Error::Busy)));
                    assert_eq!(server.retry_call().unwrap(), answers[1]);
                }
                assert!(matches!(receive(client, messages, &mut queue, chunk), Received::Returned(key, _) if key == calls[i].unwrap()));
                if i == 0 && cancel {
                    assert!(client.drain_canceled_response(messages));
                    assert!(!client.drain_canceled_response(messages));
                } else {
                    let response = client.take_response(calls[i].unwrap()).unwrap();
                    response.with(messages, |reply| {
                        let owned::Reply::Results(payload) = reply else { panic!("result") };
                        assert_eq!(echo::Owned::ECHO.read_results(payload.content()).unwrap().value(), 42 + i as u64);
                    }).unwrap();
                    client.release_response(response, messages);
                    finish(client, server, messages, &mut queue, chunk);
                }
            }
            client.capabilities().release(remote);
            while let Some(server) = client.drain_endpoint() { assert!(server.is_none()); }
            assert!(client.send_pending_release( messages, &mut queue, 776, Default::default()).unwrap().is_some());
            assert!(matches!(receive(server, messages, &mut queue, chunk), Received::Released));
            assert!(server.drain_export(0));
            server.capabilities().release(service);
            assert_eq!(server.drain_endpoint(), Some(Some(41)));
            assert!(client.runtime().is_empty() && server.runtime().is_empty());
            assert!(client.ledgers().questions.is_empty() && client.ledgers().imports.is_empty());
            assert!(server.ledgers().answers.is_empty() && server.ledgers().exports.is_empty());
            // A one-slot polling budget still eventually detects quiescence.
            idle(client, messages, &mut queue);
            idle(server, messages, &mut queue);
            let writers = core::array::from_fn::<_, 12, _>(|_| messages.reserve(776).unwrap());
            for writer in writers { messages.discard(writer); }
        }));
    }).unwrap();
}

/// A following Resolve cannot overtake propagation of the Return which first
/// introduced its promise. This needs no executor-specific yield convention.
pub fn owning_return_resolve_order() {
    use capntproto_rpc_wire::rpc_capnp::api::{exception::Type, Exception, Resolve};
    let mut words = [[super::ZERO; 97]; 4];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut queue_slots = [None];
        let mut queue = super::driver::message_queue(&mut queue_slots).unwrap();
        with_peer(|peer| {
            let question = peer
                .send(
                    messages,
                    &mut queue,
                    776,
                    Default::default(),
                    calls::bootstrap,
                )
                .unwrap();
            let pipeline = peer.pipeline(question, pipeline::Path::new(&[])).unwrap();
            let request = super::owning::exchange(messages, &mut queue, 3);
            request.release(messages);
            let returned = Frame::build(messages, 776, Default::default(), |builder| {
                let mut root = builder.try_init_root::<Message>()?;
                let mut result = root.field(Message::RETURN).init()?;
                result
                    .scalar(Return::ANSWER_ID)
                    .set(question.id().to_wire());
                result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
                let mut payload = result.field(Return::RESULTS).init()?;
                payload.field(Payload::CONTENT).set_capability_index(0)?;
                payload
                    .field(Payload::CAP_TABLE)
                    .init(1)?
                    .get_mut(0)
                    .unwrap()
                    .scalar(CapDescriptor::SENDER_PROMISE)
                    .set(19);
                Ok(())
            })
            .unwrap();
            assert!(matches!(
                peer.receive(
                    returned,
                    messages,
                    &mut [0; 4],
                    |_| (),
                    || panic!("no producer")
                )
                .unwrap_or_else(|_| panic!("Return")),
                Received::Returned(_, _)
            ));
            let resolved = Frame::build(messages, 776, Default::default(), |builder| {
                let mut root = builder.try_init_root::<Message>()?;
                let mut result = root.field(Message::RESOLVE).init()?;
                result.scalar(Resolve::PROMISE_ID).set(19);
                result
                    .field(Resolve::EXCEPTION)
                    .init()?
                    .scalar(Exception::TYPE)
                    .set(Type::Overloaded);
                Ok(())
            })
            .unwrap();
            let retry = match peer.receive(
                resolved,
                messages,
                &mut [0; 4],
                |_| panic!("no import"),
                || panic!("no producer"),
            ) {
                Err(rejected) => {
                    assert_eq!(rejected.error, owned::connection::Error::Busy);
                    assert!(rejected.producer.is_none());
                    Some(rejected.frame)
                }
                Ok(Received::Resolved) => None,
                _ => panic!("Resolve admission"),
            };
            assert!(matches!(
                peer.poll_outgoing(0, messages).unwrap(),
                owned::connection::Propagation::Resolved
            ));
            if let Some(frame) = retry {
                assert!(matches!(
                    peer.receive(
                        frame,
                        messages,
                        &mut [0; 4],
                        |_| panic!("no import"),
                        || panic!("no producer")
                    )
                    .unwrap_or_else(|_| panic!("Resolve retry")),
                    Received::Resolved
                ));
            }
            assert!(
                matches!(
                    peer.runtime().inspect(&pipeline).unwrap(),
                    clients::Destination::Endpoint {
                        owner: binding::Route::Capability(binding::Endpoint::Broken(
                            capnp::ErrorKind::Overloaded
                        )),
                        ..
                    }
                ),
                "later Resolve propagates through the returned capability"
            );
            let response = peer.take_response(question).unwrap();
            peer.release_response(response, messages);
            peer.capabilities().release(pipeline);
            while let Some(owner) = peer.drain_endpoint() {
                assert!(owner.is_none());
            }
            assert!(peer
                .send_pending_finish(messages, &mut queue, 776, Default::default())
                .unwrap());
            super::owning::exchange(messages, &mut queue, 3).release(messages);
            assert!(peer
                .send_pending_release(messages, &mut queue, 776, Default::default())
                .unwrap()
                .is_some());
            super::owning::exchange(messages, &mut queue, 3).release(messages);
            assert!(
                peer.runtime().is_empty()
                    && peer.ledgers().imports.is_empty()
                    && peer.ledgers().questions.is_empty()
            );
            let writers = core::array::from_fn::<_, 4, _>(|_| messages.reserve(776).unwrap());
            for writer in writers {
                messages.discard(writer);
            }
        });
    })
    .unwrap();
}

/// Replay the same reply-binding scenarios with fixed or growing backing.
pub(super) fn invocations<'s, 'wait, 'pool, 'id, I, const CAPS: usize>(
    slots: &'s mut [owned::connection::InvocationSlot<'wait, 'pool, 'id, I, CAPS>],
) -> Result<owned::connection::Invocations<'s, 'wait, 'pool, 'id, I, CAPS>, owned::connection::Error>
{
    #[cfg(not(feature = "growing"))]
    {
        owned::connection::Invocations::new(slots)
    }
    #[cfg(feature = "growing")]
    {
        owned::connection::Invocations::new_growing(0, slots.len())
    }
}
