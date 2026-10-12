//! Owning bootstrap/call/reply and cleanup, over fault-injected caller IO.
use super::*;
use capntproto_rpc_engine::{
    answers,
    clients::{self, dispatch},
    connection, exports,
    ids::WireId,
    imports, messages, questions,
};
use capntproto_rpc_wire::{
    calls, inbound,
    owned::{self, answer, controls, pipelines, tasks, Frame},
    reception, rpc_capnp,
};
use core::{
    future::Future,
    num::NonZeroUsize,
    task::{Context, Poll, Waker},
};
use portable_capnp::{
    api::echo::{EchoParams, EchoParamsRef, EchoResults},
    echo,
};

struct Echo;
impl echo::Server for Echo {
    async fn echo<'call>(
        &'call mut self,
        params: EchoParamsRef<'call>,
        mut results: capnp::field_api::FixedStruct<'call, EchoResults>,
    ) -> Result<()> {
        let mut once = false;
        core::future::poll_fn(|cx| {
            if once {
                Poll::Ready(())
            } else {
                once = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        })
        .await;
        results.scalar(EchoResults::VALUE).set(params.value() + 1);
        Ok(())
    }
}

pub(super) fn exchange<'pool>(
    messages: &mut messages::Messages<'pool, '_, '_>,
    queue: &mut messages::Queue<'pool, '_>,
    chunk: usize,
) -> Frame<'pool> {
    let mut bytes = [0; 1024];
    let mut io = super::rpc::Pipe::new(&mut bytes, chunk);
    let mut cx = Context::from_waker(Waker::noop());
    let budget = NonZeroUsize::new(2).unwrap();
    for attempt in 0..4096 {
        if capntproto_rpc_engine::io::poll_queue(&mut io, queue, messages, &mut cx, budget)
            .map(|r| r.unwrap())
            .is_ready()
        {
            break;
        }
        assert!(attempt < 4095, "output poll bound");
    }
    let writer = messages.reserve(97 * 8).unwrap();
    let writing = messages.write_owned(writer).unwrap();
    let mut future = core::pin::pin!(tasks::receive(writing, &mut io, Default::default(), budget));
    for _ in 0..4096 {
        if let Poll::Ready(outcome) = future.as_mut().poll(&mut cx) {
            return outcome.finish(messages).unwrap().unwrap();
        }
        // A pending read owns only its buffer. Other caller tasks keep working.
        let side = messages.reserve(4).unwrap();
        messages.write(&side)[..4].copy_from_slice(b"side");
        messages.discard(side);
    }
    panic!("input poll bound");
}

fn finish<'pool, 'a, A, E>(
    questions: &mut questions::Questions<'_, '_>,
    answers: &mut answers::Answers<'a, '_, answer::State<'pool, A>>,
    exports: &mut exports::Exports<'_, '_, E>,
    messages: &mut messages::Messages<'pool, '_, '_>,
    queue: &mut messages::Queue<'pool, '_>,
    chunk: usize,
) {
    assert!(controls::finish_one(questions, messages, queue, 97 * 8, Default::default()).unwrap());
    let frame = exchange(messages, queue, chunk);
    let finished = answer::finished(&frame, messages, answers, exports, &mut [0])
        .unwrap()
        .unwrap();
    if let Some(retired) = finished.retired {
        retired.release(messages);
    }
    // An identical second Finish cannot release the result grants again.
    assert!(
        answer::finished(&frame, messages, answers, exports, &mut [0])
            .unwrap()
            .is_none()
    );
    frame.release(messages);
}

/// No manual frame/receipt pairing or temporary decode copy. A queued call can
/// be canceled before dispatch; late Return and cleanup still drain completely.
pub fn owned_rpc_conversation(chunk: usize, cancel: bool) -> Result<()> {
    use rpc_capnp::api::{CapDescriptor, Message, Payload, Return};
    let mut a = [ZERO; 97];
    let mut b = [ZERO; 97];
    let mut c = [ZERO; 97];
    let mut d = [ZERO; 97];
    let mut e = [ZERO; 97];
    let mut f = [ZERO; 97];
    let mut buffers = [
        messages::Buffer::new(Word::words_to_bytes_mut(&mut a)),
        messages::Buffer::new(Word::words_to_bytes_mut(&mut b)),
        messages::Buffer::new(Word::words_to_bytes_mut(&mut c)),
        messages::Buffer::new(Word::words_to_bytes_mut(&mut d)),
        messages::Buffer::new(Word::words_to_bytes_mut(&mut e)),
        messages::Buffer::new(Word::words_to_bytes_mut(&mut f)),
    ];
    let mut sends = [messages::SendSlot::EMPTY];
    messages::with_messages(&mut buffers, &mut sends, |messages| {
        let mut queue_slots = [None];
        let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
        let mut imports = [imports::Slot::EMPTY];
        let mut questions = [questions::Slot::EMPTY];
        connection::with_connection(
            connection::Storage {
                imports: &mut imports,
                questions: &mut questions,
                exports: &mut [],
                answers: &mut [],
            },
            |client: &mut connection::Connection<'_, '_, (), (), ()>| {
                let mut graph_slots = [clients::Slot::EMPTY];
                clients::with_graph(
                    &mut graph_slots,
                    |graph: &mut clients::Graph<'_, '_, u32, u8>| {
                        let service_client = graph.endpoint(41, clients::Location::Local).unwrap();
                        let mut exports = [exports::Slot::EMPTY];
                        let mut answers = [answers::Slot::EMPTY];
                        connection::with_connection(
                            connection::Storage {
                                imports: &mut [],
                                questions: &mut [],
                                exports: &mut exports,
                                answers: &mut answers,
                            },
                            |server: &mut connection::Connection<
                                '_,
                                '_,
                                (),
                                clients::Client<'_>,
                                answer::State<'_, ()>,
                            >| {
                                let mut service = echo::ServerDispatch::new(Echo);
                                let reservation = client.questions.reserve().unwrap();
                                let frame =
                                    Frame::build(messages, 97 * 8, Default::default(), |b| {
                                        calls::bootstrap(b, reservation.id())
                                    })
                                    .unwrap();
                                let bootstrap = owned::Question::send(
                                    reservation,
                                    messages,
                                    frame,
                                    queue.reserve().unwrap(),
                                )
                                .unwrap();
                                let frame = exchange(messages, &mut queue, chunk);
                                let key = controls::bootstrap(
                                    frame,
                                    messages,
                                    &mut server.answers,
                                    answer::State::new(()),
                                )
                                .unwrap_or_else(|r| panic!("{}", r.error));
                                let mut batch = server.exports.batch();
                                let grant = batch
                                    .export(graph.retain(&service_client).unwrap())
                                    .unwrap();
                                let frame =
                                    Frame::build(messages, 97 * 8, Default::default(), |b| {
                                        let mut root = b.try_init_root::<Message>()?;
                                        let mut result = root.field(Message::RETURN).init()?;
                                        result.scalar(Return::ANSWER_ID).set(key.id().to_wire());
                                        result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
                                        let mut payload = result.field(Return::RESULTS).init()?;
                                        payload.field(Payload::CONTENT).set_capability_index(0)?;
                                        payload
                                            .field(Payload::CAP_TABLE)
                                            .init(1)?
                                            .get_mut(0)
                                            .unwrap()
                                            .scalar(CapDescriptor::SENDER_HOSTED)
                                            .set(grant.id().to_wire());
                                        Ok(())
                                    })
                                    .unwrap();
                                assert!(answer::send_granting(
                                    key,
                                    &mut server.answers,
                                    batch,
                                    messages,
                                    frame,
                                    &mut [0],
                                    queue.reserve().unwrap()
                                )
                                .unwrap()
                                .is_none());
                                let frame = exchange(messages, &mut queue, chunk);
                                let response = bootstrap
                                    .returned::<_, _, _, 1>(
                                        frame,
                                        messages,
                                        &mut client.questions,
                                        &mut client.imports,
                                        &mut client.exports,
                                        &mut client.answers,
                                        &mut [],
                                        |_| (),
                                    )
                                    .unwrap_or_else(|r| panic!("{}", r.error));
                                let capability = response
                                    .with(messages, |reply| {
                                        let owned::Reply::Results(payload) = reply else {
                                            panic!("bootstrap results")
                                        };
                                        let pointer = payload
                                    .content()
                                    .get_as::<capnp::wire_capability::Reader<'_, echo::Owned>>()
                                    .unwrap();
                                        let reception::full::Capability::Remote(local) =
                                            payload.capability(pointer).unwrap()
                                        else {
                                            panic!("remote")
                                        };
                                        client.imports.retain(local).unwrap()
                                    })
                                    .unwrap();
                                response.release(
                                    messages,
                                    &mut client.questions,
                                    &mut client.imports,
                                    &mut client.exports,
                                    &mut client.answers,
                                );
                                finish(
                                    &mut client.questions,
                                    &mut server.answers,
                                    &mut server.exports,
                                    messages,
                                    &mut queue,
                                    chunk,
                                );

                                let reservation = client.questions.reserve().unwrap();
                                let frame =
                                    Frame::build(messages, 97 * 8, Default::default(), |b| {
                                        calls::call(
                                            b,
                                            reservation.id(),
                                            client.imports.wire_id(&capability),
                                            echo::Owned::ECHO,
                                            |mut p| {
                                                p.scalar(EchoParams::VALUE).set(41);
                                                Ok(())
                                            },
                                        )
                                    })
                                    .unwrap();
                                let mut question = owned::Question::send(
                                    reservation,
                                    messages,
                                    frame,
                                    queue.reserve().unwrap(),
                                )
                                .unwrap();
                                let frame = exchange(messages, &mut queue, chunk);
                                let request = owned::Call::<_, 1, 0>::admit(
                                    frame,
                                    messages,
                                    &mut server.imports,
                                    &mut server.exports,
                                    &mut server.answers,
                                    answer::State::new(()),
                                    |_| (),
                                )
                                .unwrap_or_else(|r| panic!("{}", r.error));
                                let mut pipeline_slots = [pipelines::Slot::<0>::EMPTY];
                                let mut bindings =
                                    pipelines::Bindings::new(&mut pipeline_slots).unwrap();
                                let request = {
                                    let mut no_tasks = [];
                                    let mut runtime =
                                        dispatch::borrow_runtime(graph, &mut no_tasks).unwrap();
                                    let rejected = request
                                        .enqueue(
                                            &mut runtime,
                                            &server.exports,
                                            &mut server.answers,
                                            &mut bindings,
                                        )
                                        .err()
                                        .unwrap();
                                    assert!(matches!(
                                        rejected.error,
                                        owned::EnqueueError::Dispatch(_)
                                    ));
                                    rejected.call
                                };
                                let mut task_slots = [dispatch::TaskSlot::EMPTY];
                                let mut runtime =
                                    dispatch::borrow_runtime(graph, &mut task_slots).unwrap();
                                request
                                    .enqueue(
                                        &mut runtime,
                                        &server.exports,
                                        &mut server.answers,
                                        &mut bindings,
                                    )
                                    .unwrap_or_else(|_| panic!("owning queue admission"));
                                // Rejecting execution leaves the owning request queued.
                                drop(runtime.next_ready().unwrap().unwrap());
                                let ready = runtime.next_ready().unwrap().unwrap();
                                assert!(matches!(
                                    ready.destination(),
                                    clients::Destination::Endpoint {
                                        owner: &41,
                                        location: clients::Location::Local,
                                        ..
                                    }
                                ));
                                let running = ready.accept();
                                let request = running.owner();
                                let key = request.answer();
                                if cancel {
                                    question.cancel(&mut client.questions);
                                    finish(
                                        &mut client.questions,
                                        &mut server.answers,
                                        &mut server.exports,
                                        messages,
                                        &mut queue,
                                        chunk,
                                    );
                                }
                                let info = request
                                    .with(messages, |view| view.incoming().service_info(&service))
                                    .unwrap()?;
                                let active = server
                                    .answers
                                    .dispatch(key, info.allow_cancellation)
                                    .unwrap();
                                assert_eq!(active, !cancel);
                                let frame = if active {
                                    let writer = messages.reserve(97 * 8).unwrap();
                                    let writing = messages.write_owned(writer).unwrap();
                                    let service = &mut service;
                                    let future = request.with_async(messages, async move |view| {
                                        tasks::build(writing, Default::default(), async |builder| {
                                            view.incoming().serve(service, builder).await
                                        })
                                        .await
                                    });
                                    let mut future = core::pin::pin!(future);
                                    let mut cx = Context::from_waker(Waker::noop());
                                    assert!(future.as_mut().poll(&mut cx).is_pending());
                                    let side = messages.reserve(4).unwrap();
                                    messages.write(&side)[0] = 1;
                                    messages.discard(side);
                                    assert_eq!(runtime.queued(), 0);
                                    assert!(runtime.take_retired().is_none());
                                    let Poll::Ready(built) = future.as_mut().poll(&mut cx) else {
                                        panic!("server poll bound")
                                    };
                                    let (destination, frame) =
                                        built.unwrap().finish(messages).unwrap();
                                    assert_eq!(destination, calls::ResultsTo::Caller);
                                    frame
                                } else {
                                    Frame::build(messages, 97 * 8, Default::default(), |b| {
                                        calls::cancelled(b, key.id())
                                    })
                                    .unwrap()
                                };
                                runtime.complete(running).release(
                                    messages,
                                    &mut server.imports,
                                    &mut server.exports,
                                    &mut server.answers,
                                );
                                drop(runtime);
                                if let Some(retired) = answer::send(
                                    key,
                                    &mut server.answers,
                                    messages,
                                    frame,
                                    queue.reserve().unwrap(),
                                )
                                .unwrap()
                                {
                                    retired.release(messages);
                                }
                                let frame = exchange(messages, &mut queue, chunk);
                                let response = question
                                    .returned::<_, _, _, 0>(
                                        frame,
                                        messages,
                                        &mut client.questions,
                                        &mut client.imports,
                                        &mut client.exports,
                                        &mut client.answers,
                                        &mut [],
                                        |_| (),
                                    )
                                    .unwrap_or_else(|r| panic!("{}", r.error));
                                response
                                    .with(messages, |reply| {
                                        if cancel {
                                            assert!(matches!(reply, owned::Reply::Discarded));
                                        } else {
                                            let owned::Reply::Results(payload) = reply else {
                                                panic!("call results")
                                            };
                                            assert_eq!(
                                                echo::Owned::ECHO
                                                    .read_results(payload.content())
                                                    .unwrap()
                                                    .value(),
                                                42
                                            );
                                        }
                                    })
                                    .unwrap();
                                response.release(
                                    messages,
                                    &mut client.questions,
                                    &mut client.imports,
                                    &mut client.exports,
                                    &mut client.answers,
                                );
                                if !cancel {
                                    finish(
                                        &mut client.questions,
                                        &mut server.answers,
                                        &mut server.exports,
                                        messages,
                                        &mut queue,
                                        chunk,
                                    );
                                }
                                client.imports.release(capability);
                                assert!(controls::release_one(
                                    0,
                                    &mut client.imports,
                                    messages,
                                    &mut queue,
                                    97 * 8,
                                    Default::default()
                                )
                                .unwrap()
                                .is_some());
                                let frame = exchange(messages, &mut queue, chunk);
                                frame
                                    .with(messages, |root| {
                                        let rpc_capnp::message::Release(release) =
                                            root.which().unwrap()
                                        else {
                                            panic!("Release")
                                        };
                                        inbound::released(
                                            release.unwrap(),
                                            &mut server.exports,
                                            &mut [0],
                                        )
                                        .unwrap()
                                        .accept();
                                    })
                                    .unwrap();
                                frame.release(messages);
                                graph.release(server.exports.take_retired(0).unwrap());
                                graph.release(service_client);
                                assert_eq!(graph.take_retired(), Some(41));
                                assert!(graph.is_empty());
                                assert!(client.questions.is_empty() && client.imports.is_empty());
                                assert!(server.answers.is_empty() && server.exports.is_empty());
                                assert!(queue.is_empty());
                                let reservations = core::array::from_fn::<_, 6, _>(|_| {
                                    messages.reserve(97 * 8).unwrap()
                                });
                                for writer in reservations {
                                    messages.discard(writer);
                                }
                                Ok::<_, capnp::Error>(())
                            },
                        )
                        .unwrap()
                    },
                )
                .unwrap()
            },
        )
        .unwrap()
    })
    .unwrap()
}
