//! Invocation pipelines exist before a wire question does; delayed binding,
//! cancellation and Return/Resolve ordering cannot discard their result rights.
use super::{
    driver::with_owned_peer_tasks,
    driver_invocation::{close, remote},
    portable_capnp::{
        api::echo::{CapabilityResults, EchoParams},
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
    clients as binding,
    owned::{
        self,
        connection::{Propagation, Received, Task},
        Frame,
    },
    pipeline,
    rpc_capnp::{
        self,
        api::{CapDescriptor, Message, Payload, Resolve, Return},
    },
};

pub fn queued_invocation_pipeline(early: bool, cancel: bool) {
    let mut words = [[ZERO; 97]; 12];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers,&mut [messages::SendSlot::EMPTY],|messages|{
        let mut slots=[None];let mut queue=super::driver::message_queue(&mut slots).unwrap();
        with_owned_peer_tasks::<4,_>(|mut peer|{
            let target=remote(&mut peer,ImportId::from_wire(91));
            let first=peer.enqueue_call(&target,echo::Owned::CAPABILITY,&[],messages,776,Default::default(),|_|Ok(())).unwrap();
            let pending=peer.invocation_pipeline(first,pipeline::Path::new(&[0]),messages).unwrap();
            let second=peer.enqueue_call(&pending,echo::Owned::ECHO,&[],messages,776,Default::default(),|mut params|{params.scalar(EchoParams::VALUE).set(53);Ok(())}).unwrap();
            let sent=peer.send_invocation(messages,&mut queue,776,Default::default(),&mut [0;4]).unwrap().unwrap();assert_eq!(sent.task,first);
            super::owning::exchange(messages,&mut queue,1).release(messages);
            if cancel{
                peer.cancel(sent.question).unwrap();
                assert!(peer.ledgers.questions.pending_finish().is_none(),"queued pipeline retains result rights through cancellation");
            }
            if early{
                assert_eq!(peer.poll_invocation_pipeline(0).unwrap(),Propagation::Resolved);
                let child=peer.send_invocation(messages,&mut queue,776,Default::default(),&mut [0;4]).unwrap().unwrap();assert_eq!(child.task,second);
                let frame=super::owning::exchange(messages,&mut queue,1);
                frame.with(messages,|root|{
                    let rpc_capnp::message::Call(call)=root.which().unwrap() else{panic!("Call")};
                    let call=call.unwrap();let rpc_capnp::message_target::PromisedAnswer(target)=call.get_target().unwrap().which().unwrap() else{panic!("early pipeline must use parent question")};
                    assert_eq!(target.unwrap().get_question_id(),sent.question.id().to_wire());
                }).unwrap();frame.release(messages);
            }
            let frame=Frame::build(messages,776,Default::default(),|builder|{
                let root=builder.try_init_root::<Message>()?;let mut result=root.into_field(Message::RETURN).init()?;
                result.scalar(Return::ANSWER_ID).set(sent.question.id().to_wire());result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
                let mut payload=result.field(Return::RESULTS).init()?;
                payload.field(Payload::CONTENT).cast::<capnp::field_api::Struct<CapabilityResults>>().init()?.field(CapabilityResults::VALUE).set_index(0)?;
                payload.field(Payload::CAP_TABLE).init(1)?.get_mut(0).unwrap().scalar(CapDescriptor::SENDER_PROMISE).set(77);Ok(())
            }).unwrap();
            assert!(matches!(peer.receive(frame,messages,&mut [0;4],|_|(),||panic!("Return")).unwrap_or_else(|_|panic!("Return")),Received::Returned(key, _) if key==sent.question));
            assert!(matches!(peer.take_response(sent.question),Err(owned::connection::Error::PendingResponse)));
            assert!(!peer.drain_canceled_response(messages));
            let resolve=Frame::build(messages,776,Default::default(),|builder|{
                let root=builder.try_init_root::<Message>()?;let mut resolve=root.into_field(Message::RESOLVE).init()?;
                resolve.scalar(Resolve::PROMISE_ID).set(77);resolve.field(Resolve::CAP).init()?.scalar(CapDescriptor::SENDER_HOSTED).set(78);Ok(())
            }).unwrap();
            let rejected=match peer.receive(resolve,messages,&mut [0;4],|_|panic!("Resolve must wait for Return observers"),||panic!("Resolve")) {Err(rejected)=>rejected,Ok(_)=>panic!("Resolve overtook unbound pipeline")};
            assert_eq!(rejected.error,owned::connection::Error::Busy);
            if !early {
                let mut fillers=[const {None};12];
                for slot in &mut fillers {match peer.runtime.endpoint(binding::Route::local(999),clients::Location::Local){Ok(client)=>*slot=Some(client),Err(_)=>break}}
                assert!(matches!(peer.poll_invocation_pipeline(0),Err(owned::connection::Error::Outgoing(binding::outgoing::Error::Graph(clients::Error::Full)))));
                assert!(!peer.drain_canceled_response(messages));
                for client in fillers.into_iter().flatten(){peer.runtime.release(client)}
                while let Some(owner)=peer.drain_endpoint(){assert_eq!(owner,Some(999));}
                assert_eq!(peer.poll_invocation_pipeline(0).unwrap(),Propagation::Resolved);
            }
            assert_eq!(peer.poll_outgoing(0,messages).unwrap(),Propagation::Resolved);
            if cancel{assert!(peer.drain_canceled_response(messages));}
            else{let response=peer.take_response(sent.question).unwrap();peer.release_response(response,messages);}
            assert!(matches!(peer.receive(rejected.frame,messages,&mut [0;4],|_|(),||panic!("Resolve")).unwrap_or_else(|rejected|panic!("Resolve {}",rejected.error)),Received::Resolved));
            assert!(matches!(peer.runtime.inspect(&pending).unwrap(),clients::Destination::Endpoint{owner:binding::Route::Capability(binding::Endpoint::Imported(remote)),..}if remote.id()==ImportId::from_wire(78)));
            if !early{
                let child=peer.send_invocation(messages,&mut queue,776,Default::default(),&mut [0;4]).unwrap().unwrap();assert_eq!(child.task,second);
                let frame=super::owning::exchange(messages,&mut queue,1);
                frame.with(messages,|root|{let rpc_capnp::message::Call(call)=root.which().unwrap()else{panic!("Call")};assert!(matches!(call.unwrap().get_target().unwrap().which().unwrap(),rpc_capnp::message_target::ImportedCap(78)));}).unwrap();frame.release(messages);
            }
            peer.runtime.release(pending);peer.runtime.release(target);
            assert_eq!(close(peer,messages,&mut queue),0);
            let writers=core::array::from_fn::<_,12,_>(|_|messages.reserve(776).unwrap());for writer in writers{messages.discard(writer)}
        });
    }).unwrap();
}

pub fn local_invocation_pipeline(cancel: bool) {
    let mut words = [[ZERO; 97]; 8];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut slots = [None];
        let mut queue = super::driver::message_queue(&mut slots).unwrap();
        with_owned_peer_tasks::<4, _>(|mut peer| {
            let service = peer
                .runtime
                .endpoint(binding::Route::local(41), clients::Location::Local)
                .unwrap();
            let result = peer
                .runtime
                .endpoint(binding::Route::local(43), clients::Location::Local)
                .unwrap();
            let first = peer
                .enqueue_call(
                    &service,
                    echo::Owned::CAPABILITY,
                    &[],
                    messages,
                    776,
                    Default::default(),
                    |_| Ok(()),
                )
                .unwrap();
            let pending = peer
                .invocation_pipeline(first, pipeline::Path::new(&[0]), messages)
                .unwrap();
            let second = peer
                .enqueue_call(
                    &pending,
                    echo::Owned::ECHO,
                    &[],
                    messages,
                    776,
                    Default::default(),
                    |_| Ok(()),
                )
                .unwrap();
            assert!(matches!(
                peer.poll_local_invocation_pipeline(0, first, |_, _| panic!("not yet executing")),
                Err(owned::connection::Error::Busy)
            ));
            if cancel {
                assert!(peer.cancel_invocation(first, messages));
                assert_eq!(
                    peer.poll_invocation_pipeline(0).unwrap(),
                    Propagation::Resolved
                );
            } else {
                let ready = peer.runtime.next_ready().unwrap().unwrap();
                assert_eq!(ready.key(), first);
                let running = ready.accept();
                assert_eq!(
                    peer.poll_local_invocation_pipeline(0, first, |path, graph| {
                        assert_eq!(path.fields(), &[0]);
                        Ok(graph.retain(&result).unwrap())
                    })
                    .unwrap(),
                    Propagation::Resolved
                );
                let Task::Invoke(invocation) = peer.runtime.complete(running) else {
                    panic!("Invoke")
                };
                invocation.release(messages, &mut peer.runtime);
            }
            assert_eq!(
                peer.poll_invocation_pipeline(0).unwrap(),
                Propagation::Unrelated
            );
            assert!(peer
                .send_invocation(messages, &mut queue, 776, Default::default(), &mut [0; 4])
                .unwrap()
                .is_none());
            let ready = peer.runtime.next_ready().unwrap().unwrap();
            assert_eq!(ready.key(), second);
            if cancel {
                assert!(matches!(
                    ready.destination(),
                    clients::Destination::Endpoint {
                        owner: binding::Route::Capability(binding::Endpoint::Broken(
                            capnp::ErrorKind::Failed
                        )),
                        ..
                    }
                ));
            } else {
                assert!(matches!(
                    ready.destination(),
                    clients::Destination::Endpoint {
                        owner: binding::Route::Capability(binding::Endpoint::Local(43)),
                        ..
                    }
                ));
            }
            let running = ready.accept();
            let Task::Invoke(invocation) = peer.runtime.complete(running) else {
                panic!("Invoke")
            };
            invocation.release(messages, &mut peer.runtime);
            peer.runtime.release(pending);
            peer.runtime.release(service);
            peer.runtime.release(result);
            assert_eq!(close(peer, messages, &mut queue), 84);
            let writers = core::array::from_fn::<_, 8, _>(|_| messages.reserve(776).unwrap());
            for writer in writers {
                messages.discard(writer)
            }
        });
    })
    .unwrap();
}
