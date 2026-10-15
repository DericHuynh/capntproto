//! Terminal cleanup recovers pooled frames and transfers owners exactly once.
use super::{
    driver::with_owned_peer,
    portable_capnp::{api::echo::EchoResults, echo},
    ZERO,
};
use capntproto_rpc_engine::{
    clients,
    ids::{ImportId, QuestionId, WireId},
    messages,
};
use capntproto_rpc_wire::{
    calls, clients as binding,
    owned::{
        answer,
        connection::{Propagation, Received, Termination},
        pipelines, Frame,
    },
    pipeline,
    rpc_capnp::api::{CapDescriptor, Message, Payload, Resolve, Return},
};

pub fn owning_terminal_cleanup(budget: usize) {
    let mut words = [[ZERO; 97]; 12];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(
        &mut buffers,
        &mut [const { messages::SendSlot::EMPTY }; 4],
        |messages| {
            let mut slots = [const { None }; 4];
            let mut queue = super::driver::message_queue(&mut slots).unwrap();
            with_owned_peer(|mut peer| {
                let service = peer
                    .runtime
                    .endpoint(binding::Route::local(41), clients::Location::Local)
                    .unwrap();
                let mut batch = peer.ledgers.exports.batch();
                let target = binding::stage(&mut peer.runtime, &mut batch, &service)
                    .unwrap()
                    .id();
                batch.commit();
                let pending = peer
                    .send(
                        messages,
                        &mut queue,
                        776,
                        Default::default(),
                        calls::bootstrap,
                    )
                    .unwrap();
                let pending_path = peer.pipeline(pending, pipeline::Path::new(&[])).unwrap();
                let returned = peer
                    .send(
                        messages,
                        &mut queue,
                        776,
                        Default::default(),
                        calls::bootstrap,
                    )
                    .unwrap();
                let returned_path = peer.pipeline(returned, pipeline::Path::new(&[])).unwrap();
                let frame = Frame::build(messages, 776, Default::default(), |builder| {
                    let mut root = builder.try_init_root::<Message>()?;
                    let mut result = root.field(Message::RETURN).init()?;
                    result
                        .scalar(Return::ANSWER_ID)
                        .set(returned.id().to_wire());
                    result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
                    let mut payload = result.field(Return::RESULTS).init()?;
                    payload.field(Payload::CONTENT).set_capability_index(0)?;
                    payload
                        .field(Payload::CAP_TABLE)
                        .init(1)?
                        .get_mut(0)
                        .unwrap()
                        .scalar(CapDescriptor::SENDER_PROMISE)
                        .set(17);
                    Ok(())
                })
                .unwrap();
                assert!(matches!(
                    peer.receive(frame, messages, &mut [0; 4], |_| (), || panic!("Return"))
                        .unwrap_or_else(|_| panic!("Return")),
                    Received::Returned(_)
                ));
                assert_eq!(
                    peer.poll_outgoing(1, messages).unwrap(),
                    Propagation::Resolved
                );
                let mut occupied = [const { None }; 12];
                let mut servers = 1;
                for slot in &mut occupied {
                    match peer
                        .runtime
                        .endpoint(binding::Route::local(41), clients::Location::Local)
                    {
                        Ok(client) => {
                            *slot = Some(client);
                            servers += 1;
                        }
                        Err(_) => break,
                    }
                }
                let frame = Frame::build(messages, 776, Default::default(), |builder| {
                    let mut root = builder.try_init_root::<Message>()?;
                    let mut resolve = root.field(Message::RESOLVE).init()?;
                    resolve.scalar(Resolve::PROMISE_ID).set(17);
                    resolve
                        .field(Resolve::CAP)
                        .init()?
                        .scalar(CapDescriptor::SENDER_HOSTED)
                        .set(18);
                    Ok(())
                })
                .unwrap();
                assert!(matches!(
                    peer.receive(frame, messages, &mut [0; 4], |_| (), || panic!("Resolve"))
                        .unwrap_or_else(|_| panic!("Resolve")),
                    Received::DeferredResolution
                ));
                for id in [71, 72] {
                    let frame = Frame::build(messages, 776, Default::default(), |builder| {
                        calls::call(
                            builder,
                            QuestionId::from_wire(id),
                            ImportId::from_wire(target.to_wire()),
                            echo::Owned::ECHO,
                            |_| Ok(()),
                        )
                    })
                    .unwrap();
                    assert!(matches!(
                        peer.receive(frame, messages, &mut [0; 4], |_| panic!("params"), || id)
                            .unwrap_or_else(|_| panic!("Call")),
                        Received::Queued(_) | Received::DeferredCall(_)
                    ));
                }
                let frame = Frame::build(messages, 776, Default::default(), |builder| {
                    calls::bootstrap(builder, QuestionId::from_wire(73))
                })
                .unwrap();
                let Received::Bootstrap(answer) = peer
                    .receive(frame, messages, &mut [0; 4], |_| (), || 73)
                    .unwrap_or_else(|_| panic!("Bootstrap"))
                else {
                    panic!("Bootstrap")
                };
                let answer = answer.answer();
                let result = Frame::build(messages, 776, Default::default(), |builder| {
                    calls::results(builder, answer.id(), echo::Owned::ECHO, |mut results| {
                        results.scalar(EchoResults::VALUE).set(41);
                        Ok(())
                    })
                })
                .unwrap();
                let batch = peer.ledgers.exports.batch();
                let published = pipelines::Published::from_exports(
                    &result,
                    messages,
                    &batch,
                    &mut peer.runtime,
                    &mut [0; 4],
                )
                .unwrap();
                drop(batch);
                assert!(answer::send(
                    answer,
                    &mut peer.ledgers.answers,
                    messages,
                    result,
                    queue.reserve().unwrap()
                )
                .unwrap()
                .is_none());
                pipelines::complete(&mut peer.ledgers.answers, answer, Ok(published))
                    .unwrap_or_else(|_| panic!("published"));
                // Stop after a partial frame; no cleanup operation may claim the
                // remainder was sent or permit reuse of the failed output stream.
                assert!(!queue.window(messages).unwrap().unwrap().advance(3).unwrap());
                assert_eq!(
                    queue.discard_one(messages),
                    Err(messages::Error::QueueStillLive)
                );
                // Releasing application clients is optional for terminal graph
                // extraction, but cannot substitute for dropping their table owners.
                peer.runtime.release(service);
                peer.runtime.release(pending_path);
                peer.runtime.release(returned_path);
                for client in occupied.into_iter().flatten() {
                    peer.runtime.release(client);
                }
                let mut closing = peer.terminate(&mut queue);
                let mut seen_producers = [false; 3];
                let mut seen_imports = 0;
                let mut seen_servers = 0;
                let budget = core::num::NonZeroUsize::new(budget).unwrap();
                let mut done = false;
                for _ in 0..200 {
                    match closing.poll(messages, budget).unwrap() {
                        Termination::Pending => (),
                        Termination::Done => {
                            done = true;
                            break;
                        }
                        Termination::Import(()) => seen_imports += 1,
                        Termination::Server(41) => seen_servers += 1,
                        Termination::Producer(id @ 71..=73) => {
                            assert!(!core::mem::replace(
                                &mut seen_producers[(id - 71) as usize],
                                true
                            ));
                        }
                        _ => panic!("unexpected terminal owner"),
                    }
                }
                assert!(done);
                assert!(matches!(
                    closing.poll(messages, budget).unwrap(),
                    Termination::Done
                ));
                assert_eq!(seen_imports, 2);
                assert_eq!(seen_servers, servers);
                assert!(seen_producers.into_iter().all(|seen| seen));
                drop(closing);
                assert!(queue.is_failed() && queue.is_empty());
                assert!(matches!(queue.reserve(), Err(messages::Error::QueueFailed)));
                // The pool remains live and all of this connection's buffers and
                // send slots can immediately serve a different connection.
                let writers = core::array::from_fn::<_, 12, _>(|_| messages.reserve(776).unwrap());
                for writer in writers {
                    messages.discard(writer);
                }
                let writer = messages.reserve(8).unwrap();
                let lease = messages.seal(writer, 8).unwrap();
                let offers = core::array::from_fn::<_, 4, _>(|_| messages.offer(&lease).unwrap());
                for offer in offers {
                    messages.reject(offer);
                }
                messages.release(lease);
            });
        },
    )
    .unwrap();
}
