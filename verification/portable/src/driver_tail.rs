//! Owning same-connection tail calls over framed, fault-injected IO.
use super::{
    driver::with_owned_peer,
    portable_capnp::{api::echo::CapabilityResults, echo},
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
        connection::{Driver, Propagation, Received, Termination},
        pipelines, Frame,
    },
    pipeline,
    rpc_capnp::api::{CapDescriptor, Message, Payload, Return},
};
type Peer<'p, 'id, 's> = Driver<'p, 'id, 's, (), u32, u8, u32, 4, 4>;
fn receive<'p, 'id>(
    peer: &mut Peer<'p, 'id, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_>,
    chunk: usize,
    producer: u32,
) -> Received<'id, u32> {
    let frame = super::owning::exchange(messages, queue, chunk);
    peer.receive(frame, messages, &mut [0; 4], |_| (), || producer)
        .unwrap_or_else(|rejected| panic!("receive: {}", rejected.error))
}
fn close<'p>(
    peer: Peer<'p, '_, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_>,
) -> u32 {
    queue.fail();
    while queue.discard_one(messages).unwrap() {}
    let mut closing = peer.terminate();
    let mut servers = 0;
    for _ in 0..200 {
        match closing
            .poll(messages, core::num::NonZeroUsize::new(1).unwrap())
            .unwrap()
        {
            Termination::Done => return servers,
            Termination::Pending | Termination::Import(()) => (),
            Termination::Server(server) => servers += server,
            Termination::Producer(_) => {
                panic!("Finish must retire producers before terminal cleanup")
            }
        }
    }
    panic!("bounded terminal cleanup")
}
pub fn owning_tail_driver(chunk: usize) {
    let mut words = [[ZERO; 97]; 12];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [const {messages::SendSlot::EMPTY};2], |messages| {
        let mut queue_slots = [None];
        let mut queue = super::driver::message_queue(&mut queue_slots).unwrap();
        with_owned_peer(|mut alice| with_owned_peer(|mut bob| {
            // Seed the two peers' already advertised service grants.
            let local_a = alice.capabilities().endpoint(binding::Route::local(41), clients::Location::Local).unwrap();
            let (mut graph, mut batch) = alice.export_batch();
            let a = binding::stage(&mut graph, &mut batch, &local_a).unwrap().id();
            batch.commit();
            let local_b = bob.capabilities().endpoint(binding::Route::local(43), clients::Location::Local).unwrap();
            let (mut graph, mut batch) = bob.export_batch();
            let b = binding::stage(&mut graph, &mut batch, &local_b).unwrap().id();
            batch.commit();
            let original = alice.send(messages, &mut queue, 776, Default::default(), |builder,id| calls::call(builder,id,ImportId::from_wire(b.to_wire()),echo::Owned::CAPABILITY, |_| Ok(()))).unwrap();
            let pipeline = alice.pipeline(original, pipeline::Path::new(&[0])).unwrap();
            let Received::Queued(parent) = receive(&mut bob,messages,&mut queue,chunk,47) else { panic!("original Call") };
            let mut dispatch = bob.capabilities();
                let running_parent = dispatch.next_ready().unwrap().unwrap().accept();
            assert!(bob.answers().dispatch(parent,true).unwrap());
            let child = bob.send_redirected(messages,&mut queue,776,Default::default(),|builder,id| calls::tail_call_to(builder,id,calls::Target::Imported(ImportId::from_wire(a.to_wire())),echo::Owned::CAPABILITY, |_| Ok(()))).unwrap();
            assert!(matches!(bob.tail_answer(parent,child,messages,&mut queue,776,Default::default()),Err(owned::connection::Error::Input(owned::Error::Storage(messages::Error::NoSendSlot)))));
            let Received::Queued(producer) = receive(&mut alice,messages,&mut queue,chunk,49) else { panic!("redirected Call") };
            let mut dispatch = alice.capabilities();
                let running_child = dispatch.next_ready().unwrap().unwrap().accept();
            assert!(alice.answers().dispatch(producer,true).unwrap());
            assert!(bob.tail_answer(parent,child,messages,&mut queue,776,Default::default()).unwrap().is_none());
            assert!(matches!(bob.tail_answer(parent,child,messages,&mut queue,776,Default::default()),Err(owned::connection::Error::TailAlreadyUsed)));
            let request = bob.capabilities().complete(running_parent).into_call().unwrap_or_else(|_| panic!("Call"));
            bob.release_call(request,messages);
            assert!(matches!(receive(&mut alice,messages,&mut queue,chunk,0),Received::Returned(key, _) if key == original));
            assert_eq!(alice.poll_outgoing(0,messages).unwrap(),Propagation::Pending);
            assert!(matches!(alice.take_response(original),Err(owned::connection::Error::PendingResponse)));
            // The redirected service keeps its actual result locally. No
            // capability grants are sent in ResultsSentElsewhere.
            let result_service = alice.capabilities().endpoint(binding::Route::local(67),clients::Location::Local).unwrap();
            let (mut graph, mut batch) = alice.export_batch();
            let grant = binding::stage(&mut graph,&mut batch,&result_service).unwrap();
            let result = Frame::build(messages,776,Default::default(),|builder| {
                let mut root=builder.try_init_root::<Message>()?;
                let mut result=root.field(Message::RETURN).init()?;
                result.scalar(Return::ANSWER_ID).set(producer.id().to_wire());
                result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
                let mut payload=result.field(Return::RESULTS).init()?;
                payload.field(Payload::CONTENT).cast::<capnp::field_api::Struct<CapabilityResults>>().init()?.field(CapabilityResults::VALUE).set_index(0)?;
                payload.field(Payload::CAP_TABLE).init(1)?.get_mut(0).unwrap().scalar(CapDescriptor::SENDER_HOSTED).set(grant.id().to_wire());
                Ok(())
            }).unwrap();
            let published=pipelines::Published::from_exports(&result,messages,&batch,&mut graph,&mut [0;4]).unwrap();
            drop(batch);
            result.release(messages);
            let elsewhere=Frame::build(messages,776,Default::default(),|builder| calls::results_sent_elsewhere(builder,producer.id())).unwrap();
            assert!(alice.publish_answer_frame(producer,messages,elsewhere,queue.reserve().unwrap()).unwrap().is_none());
            alice.publish_answer_pipeline(producer,Ok(published)).unwrap_or_else(|_| panic!("publish"));
            let request=alice.capabilities().complete(running_child).into_call().unwrap_or_else(|_| panic!("Call"));
            alice.release_call(request,messages);
            assert!(matches!(receive(&mut bob,messages,&mut queue,chunk,0),Received::Returned(key, _) if key==child));
            let child_response=bob.take_response(child).unwrap();
            child_response.with(messages,|reply| assert!(matches!(reply,owned::Reply::ResultsSentElsewhere))).unwrap();
            bob.release_response(child_response,messages);
            assert!(!bob.ledgers().questions.has_pending_finish(),"tail parent retains child through its Finish");
            assert_eq!(alice.poll_outgoing(0,messages).unwrap(),Propagation::Resolved);
            assert!(matches!(alice.runtime().inspect(&pipeline).unwrap(),clients::Destination::Endpoint{owner:binding::Route::Capability(binding::Endpoint::Local(67)),..}));
            let response=alice.take_response(original).unwrap();
            response.with(messages,|reply| assert!(matches!(reply,owned::Reply::Adopted(_)))).unwrap();
            alice.release_response(response,messages);
            alice.capabilities().release(pipeline);
            // Retiring the original outgoing route releases its question ref.
            while let Some(server)=alice.drain_endpoint() { assert!(server.is_none()); }
            assert!(alice.send_pending_finish(messages,&mut queue,776,Default::default()).unwrap());
            assert!(matches!(receive(&mut bob,messages,&mut queue,chunk,0),Received::Finished(Some(47))));
            for _ in 0..200 {
                if !queue.is_empty(){break}
                match bob.poll_maintenance(messages,&mut queue,776,Default::default(),&mut [0;4],core::num::NonZeroUsize::new(1).unwrap()).unwrap() {
                    owned::connection::Progress::Idle | owned::connection::Progress::Yield | owned::connection::Progress::Advanced => (),
                    _=>panic!("tail cleanup"),
                }
            }
            assert!(!queue.is_empty(),"Finish releases retained tail child");
            assert!(matches!(receive(&mut alice,messages,&mut queue,chunk,0),Received::Finished(Some(49))));
            alice.capabilities().release(local_a);
            alice.capabilities().release(result_service);
            bob.capabilities().release(local_b);
            assert_eq!(close(alice,messages,&mut queue),41+67);
            assert_eq!(close(bob,messages,&mut queue),43);
            let writers=core::array::from_fn::<_,12,_>(|_| messages.reserve(776).unwrap());
            for writer in writers {messages.discard(writer)}
        }));
    }).unwrap();
}

/// Tail pipelines keep the original answer generation across early Finish and
/// wire-ID reuse. Full graph storage must not consume the redirect obligation.
pub fn owning_tail_pipeline() {
    use capntproto_rpc_engine::ids::QuestionId;
    let mut words = [[ZERO; 97]; 10];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut queue_slots = [None];
        let mut queue = super::driver::message_queue(&mut queue_slots).unwrap();
        with_owned_peer(|mut peer| {
            let original = Frame::build(messages, 776, Default::default(), |builder| {
                calls::bootstrap(builder, QuestionId::from_wire(71))
            })
            .unwrap();
            let Received::Bootstrap(parent) = peer
                .receive(original, messages, &mut [0; 4], |_| (), || 77)
                .unwrap_or_else(|_| panic!("parent"))
            else {
                panic!("parent")
            };
            let parent = parent.answer();
            let child = peer
                .send_redirected(
                    messages,
                    &mut queue,
                    776,
                    Default::default(),
                    |builder, id| {
                        calls::tail_call_to(
                            builder,
                            id,
                            calls::Target::Imported(ImportId::from_wire(19)),
                            echo::Owned::CAPABILITY,
                            |_| Ok(()),
                        )
                    },
                )
                .unwrap();
            super::owning::exchange(messages, &mut queue, 3).release(messages);
            let frame = Frame::build(messages, 776, Default::default(), |builder| {
                calls::call_to(
                    builder,
                    QuestionId::from_wire(72),
                    calls::Target::Promised {
                        question: QuestionId::from_wire(71),
                        path: pipeline::Path::new(&[0, 1]),
                    },
                    echo::Owned::ECHO,
                    |_| Ok(()),
                )
            })
            .unwrap();
            let Received::Queued(call_key) = peer
                .receive(frame, messages, &mut [0; 4], |_| (), || 88)
                .unwrap_or_else(|_| panic!("pipeline Call"))
            else {
                panic!("pipeline Call")
            };
            assert!(peer.capabilities().next_ready().unwrap().is_none());
            peer.tail_answer(parent, child, messages, &mut queue, 776, Default::default())
                .unwrap();
            super::owning::exchange(messages, &mut queue, 3).release(messages);
            let frame = Frame::build(messages, 776, Default::default(), |builder| {
                calls::finish(builder, QuestionId::from_wire(71), false)
            })
            .unwrap();
            assert!(matches!(
                peer.receive(frame, messages, &mut [0; 4], |_| (), || panic!("control"))
                    .unwrap_or_else(|_| panic!("Finish")),
                Received::Finished(None)
            ));
            let frame = Frame::build(messages, 776, Default::default(), |builder| {
                calls::bootstrap(builder, QuestionId::from_wire(71))
            })
            .unwrap();
            let Received::Bootstrap(reused) = peer
                .receive(frame, messages, &mut [0; 4], |_| (), || 56)
                .unwrap_or_else(|_| panic!("reuse"))
            else {
                panic!("reuse")
            };
            let reused = reused.answer();
            assert_ne!(parent, reused);
            let lease = peer.answer_pipeline(reused.id()).unwrap();
            let unrelated = peer
                .project_answer(&lease, pipeline::Path::new(&[0, 1]))
                .unwrap();
            peer.release_answer_pipeline_deferred(lease);
            let mut held = [const { None }; 12];
            for slot in &mut held {
                match peer
                    .capabilities()
                    .endpoint(binding::Route::local(999), clients::Location::Local)
                {
                    Ok(client) => *slot = Some(client),
                    Err(_) => break,
                }
            }
            assert!(matches!(
                peer.poll_pipeline(0, messages),
                Err(owned::connection::Error::Pipeline(pipelines::Error::Graph(
                    clients::Error::Full
                )))
            ));
            assert!(peer.capabilities().next_ready().unwrap().is_none());
            for client in held.into_iter().flatten() {
                peer.capabilities().release(client)
            }
            while let Some(server) = peer.drain_endpoint() {
                assert_eq!(server, Some(999))
            }
            assert_eq!(
                peer.poll_pipeline(0, messages).unwrap(),
                Propagation::Resolved
            );
            assert_eq!(
                peer.poll_pipeline(1, messages).unwrap(),
                Propagation::Pending,
                "reused wire ID is a different producer"
            );
            let (mut dispatch, ledgers) = peer.split_graph();
            let ready = dispatch.next_ready().unwrap().unwrap();
            let clients::Destination::Endpoint {
                owner: binding::Route::Question(route),
                ..
            } = ready.destination()
            else {
                panic!("tail question route")
            };
            let (id, path) = route.target(&ledgers.questions).unwrap();
            assert_eq!(id, child.id());
            assert_eq!(path.fields(), &[0, 1]);
            let running = ready.accept();
            let call = peer
                .capabilities()
                .complete(running)
                .into_call()
                .unwrap_or_else(|_| panic!("Call"));
            assert_eq!(call.answer(), call_key);
            peer.release_call(call, messages);
            assert_eq!(peer.drain_answer(0, messages), Some(77));
            // Publish the forwarded call's ordinary result before its Finish.
            let result = Frame::build(messages, 776, Default::default(), |builder| {
                calls::results(builder, call_key.id(), echo::Owned::ECHO, |_| Ok(()))
            })
            .unwrap();
            peer.publish_answer_frame(call_key, messages, result, queue.reserve().unwrap())
                .unwrap();
            super::owning::exchange(messages, &mut queue, 3).release(messages);
            let frame = Frame::build(messages, 776, Default::default(), |builder| {
                calls::finish(builder, QuestionId::from_wire(72), false)
            })
            .unwrap();
            assert!(matches!(
                peer.receive(frame, messages, &mut [0; 4], |_| (), || panic!("control"))
                    .unwrap_or_else(|_| panic!("Finish")),
                Received::Finished(Some(88))
            ));
            peer.capabilities().release(unrelated);
            queue.fail();
            while queue.discard_one(messages).unwrap() {}
            let mut closing = peer.terminate();
            let mut producer = false;
            let mut done = false;
            for _ in 0..200 {
                match closing
                    .poll(messages, core::num::NonZeroUsize::new(1).unwrap())
                    .unwrap()
                {
                    Termination::Pending => (),
                    Termination::Producer(56) => assert!(!core::mem::replace(&mut producer, true)),
                    Termination::Done => {
                        done = true;
                        break;
                    }
                    _ => panic!("unexpected tail owner"),
                }
            }
            assert!(done && producer);
            let writers = core::array::from_fn::<_, 10, _>(|_| messages.reserve(776).unwrap());
            for writer in writers {
                messages.discard(writer)
            }
        });
    })
    .unwrap();
}
