//! Calls made against retained graph capabilities, including parameter grants,
//! output rejection, local dispatch, promise targets and task-slot reuse.
use super::{
    driver::{with_owned_peer, with_owned_peer_tasks},
    portable_capnp::{
        api::echo::{CapabilityParams, CapabilityResults, EchoParams},
        echo,
    },
    ZERO,
};
use capntproto_rpc_engine::{
    clients::{self, Capabilities},
    ids::{ImportId, WireId},
    messages,
};
use capntproto_rpc_wire::{
    calls, clients as binding,
    owned::{
        self,
        connection::{Driver, Received, Task, Termination},
        Frame,
    },
    rpc_capnp,
};
type Peer<'p, 'id, 's> = Driver<'p, 'id, 's, (), u32, u8, u32, 4, 4>;

pub fn encoded_immediate_return(chunk: usize) {
    for canceled in [false, true] {
        for (no_finish, missing_descriptor) in
            [(false, false), (false, true), (true, false), (true, true)]
        {
            let mut words = [[ZERO; 97]; 12];
            let mut buffers = words
                .each_mut()
                .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
            super::driver::message_scope(
                &mut buffers,
                &mut [messages::SendSlot::EMPTY],
                |messages| {
                    let mut slots = [None];
                    let mut queue = super::driver::message_queue(&mut slots).unwrap();
                    with_owned_peer(|mut peer| {
                        let target = remote(&mut peer, ImportId::from_wire(91));
                        let method = calls::DynamicCall {
                            interface: 47,
                            method: 3,
                            no_promise_pipelining: true,
                            only_promise_pipeline: false,
                        };
                        let mut source = [ZERO; 97];
                        let mut builder = capnp::message::FixedBuilder::new(&mut source).unwrap();
                        calls::dynamic_call(
                            &mut builder,
                            capntproto_rpc_engine::ids::QuestionId::from_wire(0),
                            calls::Target::Imported(ImportId::from_wire(0)),
                            method,
                            |_| Ok(()),
                        )
                        .unwrap();
                        let encoded = owned::connection::EncodedCall::from_fixed(
                            &builder,
                            method,
                            Default::default(),
                        )
                        .unwrap();
                        let key = peer
                            .try_send_immediate_from(
                                &target,
                                method,
                                owned::connection::ImmediateParams::EncodedCall(encoded),
                                messages,
                                &mut queue,
                                776,
                                Default::default(),
                                (),
                            )
                            .unwrap()
                            .unwrap();
                        super::owning::exchange(messages, &mut queue, chunk).release(messages);
                        if canceled {
                            peer.cancel(key).unwrap();
                        }
                        let answer =
                            capntproto_rpc_engine::ids::AnswerId::from_wire(key.id().to_wire());
                        // Live replies reject unknown descriptor owners. Canceled
                        // replies discard descriptors, so exercise rejection with
                        // an unknown question ID in that case.
                        let malformed =
                            Frame::build(messages, 776, Default::default(), |builder| {
                                calls::results(
                                    builder,
                                    if canceled {
                                        capntproto_rpc_engine::ids::AnswerId::from_wire(u32::MAX)
                                    } else {
                                        answer
                                    },
                                    echo::Owned::CAPABILITY,
                                    |mut result| {
                                        result.field(CapabilityResults::VALUE).set_index(0)
                                    },
                                )?;
                                builder
                                    .try_get_root::<rpc_capnp::api::Message>()?
                                    .field(rpc_capnp::api::Message::RETURN)
                                    .edit()?
                                    .field(rpc_capnp::api::Return::RESULTS)
                                    .edit()?
                                    .field(rpc_capnp::api::Payload::CAP_TABLE)
                                    .init(1)?
                                    .get_mut(0)
                                    .unwrap()
                                    .scalar(rpc_capnp::api::CapDescriptor::RECEIVER_HOSTED)
                                    .set(u32::MAX);
                                Ok(())
                            })
                            .unwrap();
                        let rejected = peer
                            .receive(malformed, messages, &mut [0; 4], |_| (), || panic!())
                            .err()
                            .expect("unknown question or descriptor owner must fail");
                        rejected.frame.release(messages);
                        assert_eq!(
                            peer.ledgers().questions.len(),
                            1,
                            "rejection retains outstanding question"
                        );
                        let returned = Frame::build(messages, 776, Default::default(), |builder| {
                            if missing_descriptor {
                                calls::results(
                                    builder,
                                    answer,
                                    echo::Owned::CAPABILITY,
                                    |mut result| {
                                        result.field(CapabilityResults::VALUE).set_index(0)
                                    },
                                )?;
                            } else {
                                calls::acknowledge(builder, answer)?;
                            }
                            let mut root = builder.try_get_root::<rpc_capnp::api::Message>()?;
                            let mut returned =
                                root.field(rpc_capnp::api::Message::RETURN).edit()?;
                            returned
                                .scalar(rpc_capnp::api::Return::NO_FINISH_NEEDED)
                                .set(no_finish);
                            Ok(())
                        })
                        .unwrap();
                        peer.receive(returned, messages, &mut [0; 4], |_| (), || panic!())
                            .unwrap_or_else(|_| panic!("valid retry"));
                        let duplicate =
                            Frame::build(messages, 776, Default::default(), |builder| {
                                calls::acknowledge(builder, answer)
                            })
                            .unwrap();
                        let rejected = peer
                            .receive(duplicate, messages, &mut [0; 4], |_| (), || panic!())
                            .err()
                            .expect("duplicate Return");
                        rejected.frame.release(messages);
                        if canceled {
                            assert!(peer.drain_canceled_response(messages));
                        } else {
                            let output = peer.take_output(key, messages).unwrap().unwrap();
                            let owned::connection::CallOutput::Remote(response) = &output else {
                                panic!("remote result");
                            };
                            response
                                .with(messages, |reply| {
                                    let owned::Reply::Results(payload) = reply else {
                                        panic!("result");
                                    };
                                    if missing_descriptor {
                                        // Content is checked on access, as in C++. A
                                        // wire index never grants capability authority
                                        // without its exact admitted descriptor receipt.
                                        let pointer = echo::Owned::CAPABILITY
                                            .read_results(payload.content())
                                            .unwrap()
                                            .value()
                                            .unwrap();
                                        assert!(payload.capability(pointer).is_err());
                                    } else {
                                        assert!(payload.content().is_null());
                                    }
                                })
                                .unwrap();
                            peer.release_output(output, messages);
                        }
                        let finish = peer
                            .send_pending_finish(messages, &mut queue, 776, Default::default())
                            .unwrap();
                        // A canceled call already owed Finish before Return arrived.
                        assert_eq!(finish, canceled || !no_finish);
                        if finish {
                            super::owning::exchange(messages, &mut queue, chunk).release(messages);
                        }
                        assert!(peer.ledgers().questions.is_empty());
                        peer.capabilities().release(target);
                        assert_eq!(close(peer, messages, &mut queue), 0);
                        let writers =
                            core::array::from_fn::<_, 12, _>(|_| messages.reserve(776).unwrap());
                        for writer in writers {
                            messages.discard(writer);
                        }
                    });
                },
            )
            .unwrap();
        }
    }
}

pub fn immediate_publication(chunk: usize) {
    immediate_publication_case(chunk, false);
    immediate_publication_case(chunk, true);
}
fn immediate_publication_case(chunk: usize, no_promise_pipelining: bool) {
    let mut words = [[ZERO; 97]; 12];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut slots = [None];
        let mut queue = super::driver::message_queue(&mut slots).unwrap();
        with_owned_peer(|mut peer| {
            let target = remote(&mut peer, ImportId::from_wire(91));
            let body = [ZERO];
            let segments = [capnp::Word::words_to_bytes(&body)];
            let reader = capnp::message::Reader::new(
                capnp::message::SegmentArray::new(&segments),
                Default::default(),
            );
            let params = reader.get_root::<capnp::any_pointer::Reader>().unwrap();
            let method = calls::DynamicCall {
                interface: 47,
                method: 3,
                no_promise_pipelining,
                only_promise_pipeline: false,
            };
            let pending = peer.capabilities().promise(&mut None).unwrap();
            let older = peer
                .enqueue_dynamic(
                    &pending,
                    method,
                    &[],
                    messages,
                    776,
                    Default::default(),
                    |_| Ok(()),
                )
                .unwrap();
            assert!(peer
                .try_send_immediate(
                    &target,
                    method,
                    params,
                    messages,
                    &mut queue,
                    776,
                    Default::default(),
                    ()
                )
                .unwrap()
                .is_none());
            assert!(peer.cancel_invocation(older, messages));
            peer.capabilities().release(pending);
            let writers = core::array::from_fn::<_, 12, _>(|_| messages.reserve(776).unwrap());
            assert!(peer
                .try_send_immediate(
                    &target,
                    method,
                    params,
                    messages,
                    &mut queue,
                    776,
                    Default::default(),
                    ()
                )
                .is_err());
            assert_eq!(peer.ledgers().questions.len(), 0);
            assert!(queue.is_empty());
            for writer in writers {
                messages.discard(writer);
            }
            assert!(peer
                .try_send_immediate(
                    &target,
                    method,
                    params,
                    messages,
                    &mut queue,
                    776,
                    capnp::message::ReaderOptions {
                        traversal_limit_in_words: Some(0),
                        ..Default::default()
                    },
                    ()
                )
                .is_err());
            assert_eq!(peer.ledgers().questions.len(), 0);
            assert!(queue.is_empty());
            let key = peer
                .try_send_immediate(
                    &target,
                    method,
                    params,
                    messages,
                    &mut queue,
                    776,
                    Default::default(),
                    (),
                )
                .unwrap()
                .unwrap();
            assert_eq!(peer.runtime().queued(), 0);
            assert!(peer
                .try_send_immediate(
                    &target,
                    method,
                    params,
                    messages,
                    &mut queue,
                    776,
                    Default::default(),
                    ()
                )
                .is_err());
            assert_eq!(peer.ledgers().questions.len(), 1);
            let frame = super::owning::exchange(messages, &mut queue, chunk);
            frame
                .with(messages, |root| {
                    let rpc_capnp::message::Call(call) = root.which().unwrap() else {
                        panic!()
                    };
                    let call = call.unwrap();
                    assert_eq!(call.get_question_id(), key.id().to_wire());
                    assert_eq!(call.get_interface_id(), 47);
                    assert_eq!(call.get_method_id(), 3);
                    assert_eq!(call.get_no_promise_pipelining(), no_promise_pipelining);
                    assert!(call.get_params().unwrap().get_content().is_null());
                })
                .unwrap();
            frame.release(messages);
            let returned = Frame::build(messages, 776, Default::default(), |builder| {
                calls::acknowledge(
                    builder,
                    capntproto_rpc_engine::ids::AnswerId::from_wire(key.id().to_wire()),
                )
            })
            .unwrap();
            peer.receive(returned, messages, &mut [0; 4], |_| (), || panic!())
                .unwrap_or_else(|_| panic!("Return"));
            let output = peer.take_output(key, messages).unwrap().unwrap();
            peer.release_output(output, messages);
            assert!(peer
                .send_pending_finish(messages, &mut queue, 776, Default::default())
                .unwrap());
            super::owning::exchange(messages, &mut queue, chunk).release(messages);
            assert!(peer.ledgers().questions.is_empty());
            peer.capabilities().release(target);
            assert_eq!(close(peer, messages, &mut queue), 0);
            let writers = core::array::from_fn::<_, 12, _>(|_| messages.reserve(776).unwrap());
            for writer in writers {
                messages.discard(writer);
            }
        });
    })
    .unwrap();
}
pub(super) fn remote<'id>(peer: &mut Peer<'_, 'id, '_>, id: ImportId) -> clients::Client<'id> {
    let imported = peer
        .admit_import(id, ())
        .unwrap_or_else(|_| panic!("seed import"));
    let client = peer.bind_import(&imported).unwrap();
    peer.release_import(imported);
    client
}
pub(super) fn close<'p>(
    peer: Peer<'p, '_, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_>,
) -> u32 {
    let mut closing = peer.terminate(queue);
    let mut servers = 0;
    for _ in 0..300 {
        match closing
            .poll(messages, core::num::NonZeroUsize::new(1).unwrap())
            .unwrap()
        {
            Termination::Pending | Termination::Import(()) => (),
            Termination::Server(value) => servers += value,
            Termination::Producer(_) => panic!("producer must Finish"),
            Termination::Done => return servers,
        }
    }
    panic!("terminal cleanup must progress")
}
pub fn owning_invocation_driver(chunk: usize) {
    let mut words = [[ZERO; 97]; 12];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers,&mut [messages::SendSlot::EMPTY],|messages|{
        let mut slots=[None];
        let mut queue=super::driver::message_queue(&mut slots).unwrap();
        with_owned_peer(|mut client|with_owned_peer(|mut server|{
            // Already-advertised service; subsequent parameter grants use the
            // real generated Call/Return/Release path in both driver domains.
            let service=server.capabilities().endpoint(binding::Route::local(41),clients::Location::Local).unwrap();
            let (mut graph, mut batch) = server.export_batch();
            let grant=binding::stage(&mut graph,&mut batch,&service).unwrap();
            let target=remote(&mut client,ImportId::from_wire(grant.id().to_wire()));
            batch.commit();
            let callback=client.capabilities().endpoint(binding::Route::local(43),clients::Location::Local).unwrap();
            let task=client.enqueue_call(&target,echo::Owned::CAPABILITY,&[&callback],messages,776,Default::default(),|mut params|params.field(CapabilityParams::VALUE).set_index(0)).unwrap();
            client.capabilities().release(callback);
            assert_eq!(client.ledgers().exports.len(),0,"no grants before output");
            client.send(messages,&mut queue,776,Default::default(),calls::bootstrap).unwrap();
            assert!(matches!(client.send_invocation(messages,&mut queue,776,Default::default(),&mut [0;4]),Err(owned::connection::Error::Input(owned::Error::Storage(messages::Error::NoSendSlot)))));
            assert_eq!(client.capabilities().next_ready().unwrap().unwrap().owner().invocation_key().unwrap(),task);
            super::owning::exchange(messages,&mut queue,chunk).release(messages);
            // A failing post-build descriptor check must retain the invocation
            // and recover the candidate output, question and grant reservations.
            assert!(client.send_invocation(messages,&mut queue,776,Default::default(),&mut []).is_err());
            assert_eq!(client.ledgers().questions.len(),1);
            assert!(queue.is_empty());
            for i in 0..4 {client.drain_export(i);}
            assert!(client.drain_endpoint().is_none(),"invocation retains its callback");
            let sent=client.send_invocation(messages,&mut queue,776,Default::default(),&mut [0;4]).unwrap().unwrap();
            assert_eq!(sent.task,task);
            assert!(!client.cancel_invocation(task,messages));
            let frame=super::owning::exchange(messages,&mut queue,chunk);
            let Received::Queued(key)=server.receive(frame,messages,&mut [0;4],|_|(),||47).unwrap_or_else(|_|panic!("Call")) else {panic!("Call")};
            let running=server.capabilities().next_ready().unwrap().unwrap().accept();
            assert!(server.answers().dispatch(key,true).unwrap());
            let call=running.owner().as_call().unwrap();
            call.with(messages,|request|{
                let dispatch=request.incoming().dispatch(echo::Owned::CAPABILITY).unwrap();
                let capability=request.capability(dispatch.params.value().unwrap()).unwrap();
                let bound=server.bind_capability(capability).unwrap();
                let client=bound.into_client();
                assert!(matches!(server.runtime().inspect(&client).unwrap(),clients::Destination::Endpoint{owner:binding::Route::Capability(binding::Endpoint::Imported(_)),..}));
                server.capabilities().release(client);
            }).unwrap();
            let result=Frame::build(messages,776,Default::default(),|builder|calls::results(builder,key.id(),echo::Owned::CAPABILITY,|_|Ok(()))).unwrap();
            assert!(server.publish_answer_frame(key,messages,result,queue.reserve().unwrap()).unwrap().is_none());
            let call=server.capabilities().complete(running).into_call().unwrap_or_else(|_|panic!("Call"));
            server.release_call(call,messages);
            let frame=super::owning::exchange(messages,&mut queue,chunk);
            assert!(matches!(client.receive(frame,messages,&mut [0;4],|_|(),||panic!("Return")).unwrap_or_else(|_|panic!("Return")),Received::Returned(key, _)if key==sent.question));
            let response=client.take_response(sent.question).unwrap();
            client.release_response(response,messages);
            assert!(client.send_pending_finish(messages,&mut queue,776,Default::default()).unwrap());
            let frame=super::owning::exchange(messages,&mut queue,chunk);
            assert!(matches!(server.receive(frame,messages,&mut [0;4],|_|(),||panic!("Finish")).unwrap_or_else(|_|panic!("Finish")),Received::Finished(Some(47))));
            // Finish on the result must not release the separately advertised
            // parameter grant; its import emits exactly one Release.
            while let Some(owner)=server.drain_endpoint(){assert!(owner.is_none());}
            assert!(server.send_pending_release(messages,&mut queue,776,Default::default()).unwrap().is_some());
            let frame=super::owning::exchange(messages,&mut queue,chunk);
            assert!(matches!(client.receive(frame,messages,&mut [0;4],|_|(),||panic!("Release")).unwrap_or_else(|_|panic!("Release")),Received::Released));
            client.capabilities().release(target);
            server.capabilities().release(service);
            assert_eq!(close(client,messages,&mut queue),43);
            assert_eq!(close(server,messages,&mut queue),41);
            let writers=core::array::from_fn::<_,12,_>(|_|messages.reserve(776).unwrap());
            for writer in writers {messages.discard(writer)}
        }));
    }).unwrap();
}
pub fn owning_invocation_ordering() {
    let mut words = [[ZERO; 97]; 12];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut slots = [None];
        let mut queue = super::driver::message_queue(&mut slots).unwrap();
        with_owned_peer_tasks::<4, _>(|mut peer| {
            let target = remote(&mut peer, ImportId::from_wire(91));
            let pending = peer.capabilities().promise(&mut None).unwrap();
            assert!(matches!(
                peer.enqueue_call(
                    &target,
                    echo::Owned::CAPABILITY,
                    &[],
                    messages,
                    776,
                    Default::default(),
                    |mut params| params.field(CapabilityParams::VALUE).set_index(0)
                ),
                Err(owned::connection::Error::Input(owned::Error::Wire(
                    capnp::ErrorKind::MessageContainsInvalidCapabilityPointer
                )))
            ));
            assert_eq!(peer.runtime().queued(), 0);
            assert_eq!(peer.ledgers().questions.len(), 0);
            let mut enqueue = |value| {
                peer.enqueue_call(
                    &pending,
                    echo::Owned::ECHO,
                    &[],
                    messages,
                    776,
                    Default::default(),
                    |mut params| {
                        params.scalar(EchoParams::VALUE).set(value);
                        Ok(())
                    },
                )
                .unwrap()
            };
            let first = enqueue(11);
            let second = enqueue(13);
            let third = enqueue(17);
            assert!(peer
                .send_invocation(messages, &mut queue, 776, Default::default(), &mut [0; 4])
                .unwrap()
                .is_none());
            assert!(peer.cancel_invocation(first, messages));
            let fourth = peer
                .enqueue_call(
                    &pending,
                    echo::Owned::ECHO,
                    &[],
                    messages,
                    776,
                    Default::default(),
                    |mut params| {
                        params.scalar(EchoParams::VALUE).set(19);
                        Ok(())
                    },
                )
                .unwrap();
            assert!(
                !peer.cancel_invocation(first, messages),
                "stale cancellation must not hit reused storage"
            );
            let mut replacement = Some(peer.capabilities().retain(&target).unwrap());
            assert!(peer
                .capabilities()
                .resolve(pending.key(), &mut replacement, clients::Source::Direct)
                .unwrap()
                .is_none());
            while peer.capabilities().advance_dispatch_one().unwrap() {}
            for (task, value) in [(second, 13), (third, 17), (fourth, 19)] {
                let sent = peer
                    .send_invocation(messages, &mut queue, 776, Default::default(), &mut [0; 4])
                    .unwrap()
                    .unwrap();
                assert_eq!(sent.task, task, "oldest call precedes reused task slot");
                let frame = super::owning::exchange(messages, &mut queue, 1);
                frame
                    .with(messages, |root| {
                        let rpc_capnp::message::Call(call) = root.which().unwrap() else {
                            panic!("Call")
                        };
                        let call = call.unwrap();
                        assert_eq!(call.get_question_id(), sent.question.id().to_wire());
                        assert!(matches!(
                            call.get_target().unwrap().which().unwrap(),
                            rpc_capnp::message_target::ImportedCap(91)
                        ));
                        assert_eq!(
                            calls::Incoming::new(call)
                                .dispatch(echo::Owned::ECHO)
                                .unwrap()
                                .params
                                .value(),
                            value
                        );
                    })
                    .unwrap();
                frame.release(messages);
            }
            let local = peer
                .capabilities()
                .endpoint(binding::Route::local(53), clients::Location::Local)
                .unwrap();
            let task = peer
                .enqueue_call(
                    &local,
                    echo::Owned::CAPABILITY,
                    &[&target],
                    messages,
                    776,
                    Default::default(),
                    |mut params| params.field(CapabilityParams::VALUE).set_index(0),
                )
                .unwrap();
            assert!(peer
                .send_invocation(messages, &mut queue, 776, Default::default(), &mut [0; 4])
                .unwrap()
                .is_none());
            let mut dispatch = peer.capabilities();
            let ready = dispatch.next_ready().unwrap().unwrap();
            assert_eq!(ready.owner().invocation_key().unwrap(), task);
            let running = ready.accept();
            let Task::Invoke(invocation) = running.owner() else {
                panic!("local call")
            };
            invocation
                .with(messages, |request| {
                    let params = request.params(echo::Owned::CAPABILITY).unwrap();
                    assert_eq!(
                        request.capability(params.value().unwrap()).unwrap().key(),
                        target.key()
                    );
                })
                .unwrap();
            let Task::Invoke(invocation) = peer.capabilities().complete(running) else {
                panic!("local call")
            };
            invocation.release(messages, &mut peer.capabilities());
            // Leave an unresolved invocation for terminal cleanup.
            let never = peer.capabilities().promise(&mut None).unwrap();
            peer.enqueue_call(
                &never,
                echo::Owned::ECHO,
                &[&local],
                messages,
                776,
                Default::default(),
                |_| Ok(()),
            )
            .unwrap();
            peer.capabilities().release(never);
            peer.capabilities().release(local);
            peer.capabilities().release(pending);
            peer.capabilities().release(target);
            assert_eq!(close(peer, messages, &mut queue), 53);
            let writers = core::array::from_fn::<_, 12, _>(|_| messages.reserve(776).unwrap());
            for writer in writers {
                messages.discard(writer)
            }
        });
    })
    .unwrap();
}
