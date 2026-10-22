//! Third-party Return races with authorization, cancellation and ID reuse.
use super::{driver::with_owned_peer_storage, driver_invocation::close, ZERO};
use capntproto_rpc_engine::{
    ids::{AnswerId, QuestionId, WireId},
    messages, questions,
};
use capntproto_rpc_wire::{
    calls, network,
    owned::{
        connection::{AdoptionError, Error, NetworkRequest, Received},
        controls, Frame, Reply,
    },
    pipeline,
    rpc_capnp::{
        self,
        api::{Message, ThirdPartyAnswer},
    },
};
const ID: u32 = (1 << 30) + 7;
fn announcement<'p>(messages: &mut messages::Messages<'p, '_, '_>, id: u32) -> Frame<'p> {
    Frame::build(messages, 776, Default::default(), |b| {
        network::third_party_answer(b, AnswerId::from_wire(id), |token| {
            token
                .cast::<capnp::field_api::Data>()
                .copy_from(b"authenticated completion")
        })
    })
    .unwrap()
}
fn result<'p>(messages: &mut messages::Messages<'p, '_, '_>) -> Frame<'p> {
    Frame::build(messages, 776, Default::default(), |b| {
        calls::exception(
            b,
            AnswerId::from_wire(ID),
            capnp::ErrorKind::Overloaded,
            "adopted result",
        )
    })
    .unwrap()
}
/// 0 Return first, 1 authorization first, 2 deny after Return, 3 cancel before
/// Return, 4 failed authentication then retry, 5 close pending metadata,
/// 6 cancel while output is full, 7 retain a pipeline before Return.
pub fn owning_network_adoption(mode: u8) {
    let mut words = [[ZERO; 97]; 8];
    let mut buffers = words
        .each_mut()
        .map(|w| messages::Buffer::new(capnp::Word::words_to_bytes_mut(w)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut slots = [None];
        let mut queue = super::driver::message_queue(&mut slots).unwrap();
        with_owned_peer_storage::<2, 1, _>(|mut peer| {
            for bad in [0, (1 << 30) - 1, 1 << 31, u32::MAX] {
                assert!(Frame::build(messages, 776, Default::default(), |b| {
                    network::third_party_answer(b, AnswerId::from_wire(bad), |_| panic!("invalid ID runs no callback"))
                }).is_err());
                let frame = Frame::build(messages, 776, Default::default(), |b| {
                    let root = b.try_init_root::<Message>()?;
                    root.into_field(Message::THIRD_PARTY_ANSWER).init()?.scalar(ThirdPartyAnswer::ANSWER_ID).set(bad);
                    Ok(())
                }).unwrap();
                let rejected = peer.receive(frame, messages, &mut [0;4], |_|panic!("metadata"), ||panic!("adoption needs no producer")).err().unwrap();
                assert!(matches!(rejected.error, Error::Input(capntproto_rpc_wire::owned::Error::Admission(capntproto_rpc_wire::inbound::Error::Question(questions::Error::InvalidAdoptedId)))));
                rejected.frame.release(messages);
            }
            let malformed = Frame::build(messages, 776, Default::default(), |b| network::third_party_answer(b, AnswerId::from_wire(ID), |token| token.set_capability_index(0))).unwrap();
            let rejected = peer.receive(malformed,messages,&mut [0;4], |_|panic!("metadata cap"), ||panic!("metadata cap")).err().unwrap();
            rejected.frame.release(messages);
            assert!(peer.ledgers.questions.is_empty());
            let frame = announcement(messages, ID);
            assert!(matches!(peer.receive(frame,messages,&mut [0;4], |_|panic!("adoption"), ||panic!("adoption")).unwrap_or_else(|r|panic!("{}",r.error)), Received::Network));
            // An occupied inline network job must reject before changing IDs.
            let frame = announcement(messages, ID + 1);
            let rejected = peer.receive(frame,messages,&mut [0;4], |_|panic!("busy"), ||panic!("busy")).err().unwrap();
            assert_eq!(rejected.error, Error::Busy);
            rejected.frame.release(messages);
            let NetworkRequest::Adopt(mut job) = peer.take_network().unwrap() else {panic!("adoption")};
            let key = job.key();
            assert_eq!(key.id().to_wire(), ID);
            assert_eq!(peer.pipeline(key,pipeline::Path::new(&[])).err(),Some(Error::PendingAuthorization), "unmatched adoption cannot expose a pipeline");
            for (id,error) in [(ID,questions::Error::DuplicateAdoption),(ID+1,questions::Error::Full)] {
                let frame=announcement(messages,id);
                let rejected=peer.receive(frame,messages,&mut [0;4], |_|panic!("capacity"), ||panic!("capacity")).err().unwrap();
                assert_eq!(rejected.error, Error::Input(capntproto_rpc_wire::owned::Error::Admission(capntproto_rpc_wire::inbound::Error::Question(error))));
                rejected.frame.release(messages);
            }
            let early_return = matches!(mode, 0 | 2 | 4);
            if early_return {
                let frame=result(messages);
                assert!(matches!(peer.receive(frame,messages,&mut [0;4], |_|(), ||panic!("Return")).unwrap_or_else(|r|panic!("{}",r.error)),Received::Returned(k, _) if k==key));
                assert_eq!(peer.take_response(key).err(),Some(Error::PendingAuthorization), "unmatched adoption cannot expose an early Return");
                job.with_completion(messages, |token| assert_eq!(token.get_as::<capnp::data::Reader<'_>>().unwrap(),b"authenticated completion")).unwrap();
            }
            if mode == 5 {
                peer.disconnect(capnp::ErrorKind::Disconnected);
                let rejected=peer.authorize_adoption(job,messages, |_|->Result<(),()> {panic!("disconnected callback")}).err().unwrap();
                assert!(matches!(rejected.error,AdoptionError::Connection(Error::Disconnected(_))));
                peer.release_network(NetworkRequest::Adopt(rejected.adoption),messages);
                assert_eq!(close(peer,messages,&mut queue),0);
            } else {
                let denied=matches!(mode,2|3|6);
                if mode==4 {
                    let rejected=peer.authorize_adoption(job,messages, |_|Err(13u8)).err().unwrap();
                    assert!(matches!(rejected.error,AdoptionError::Network(13)));
                    job=rejected.adoption;
                    assert_eq!(peer.take_response(key).err(),Some(Error::PendingAuthorization));
                }
                if mode==6 {
                    peer.send(messages,&mut queue,776,Default::default(),calls::bootstrap).unwrap();
                }
                let observed = if denied {
                    peer.cancel(key).unwrap();
                    let rejected=peer.authorize_adoption(job,messages, |_|->Result<(),()> {panic!("canceled callback")}).err().unwrap();
                    assert!(matches!(rejected.error,AdoptionError::Connection(Error::UnknownQuestion)));
                    peer.release_network(NetworkRequest::Adopt(rejected.adoption),messages);
                    if mode==2 { assert!(peer.drain_canceled_response(messages)); }
                    None
                } else {
                    assert_eq!(peer.authorize_adoption(job,messages, |token| {
                        assert_eq!(token.get_as::<capnp::data::Reader<'_>>().unwrap(),b"authenticated completion");
                        Ok::<_,()>(())
                    }).unwrap_or_else(|_|panic!("authenticate")),key);
                    if mode==7 {Some(peer.pipeline(key,pipeline::Path::new(&[])).unwrap())} else {None}
                };
                if mode==6 {
                    assert!(controls::finish_one(&mut peer.ledgers.questions,messages,&mut queue,776,Default::default()).is_err());
                    super::owning::exchange(messages,&mut queue,1).release(messages);
                }
                if matches!(mode,3|6) {
                    assert!(controls::finish_one(&mut peer.ledgers.questions,messages,&mut queue,776,Default::default()).unwrap());
                    let finish=super::owning::exchange(messages,&mut queue,1);
                    finish.with(messages,|root| {
                        let rpc_capnp::message::Finish(f)=root.which().unwrap() else {panic!("Finish")};
                        let f=f.unwrap(); assert_eq!(f.get_question_id(),ID);assert!(f.get_release_result_caps());
                    }).unwrap();finish.release(messages);
                    assert!(peer.ledgers.questions.reserve_adoption(QuestionId::from_wire(ID)).is_err(),"early Finish cannot recycle adopted ID");
                }
                if !early_return {
                    let frame=result(messages);
                    peer.receive(frame,messages,&mut [0;4], |_|panic!("exception"), ||panic!("Return")).unwrap_or_else(|r|panic!("{}",r.error));
                }
                if let Some(observed)=observed {
                    assert_eq!(peer.take_response(key).err(),Some(Error::PendingResponse));
                    for i in 0..peer.outgoing_capacity() {peer.poll_outgoing(i,messages).unwrap();}
                    peer.runtime.release(observed);
                    while let Some(owner) = peer.drain_endpoint() { assert!(owner.is_none()); }
                }
                if denied {
                    if mode!=2 {assert!(peer.drain_canceled_response(messages));}
                } else {
                    let response=peer.take_response(key).unwrap();
                    response.with(messages,|reply| {
                        let Reply::Exception(e)=reply else {panic!("exception")};
                        assert_eq!(e.get_reason().unwrap(),"adopted result");
                    }).unwrap();
                    peer.release_response(response,messages);
                }
                if !matches!(mode,3|6) {
                    assert!(controls::finish_one(&mut peer.ledgers.questions,messages,&mut queue,776,Default::default()).unwrap());
                    let finish=super::owning::exchange(messages,&mut queue,1);
                    finish.with(messages,|root| {
                        let rpc_capnp::message::Finish(f)=root.which().unwrap() else {panic!("Finish")};
                        assert!(!f.unwrap().get_release_result_caps());
                    }).unwrap();finish.release(messages);
                }
                let frame=announcement(messages,ID);
                peer.receive(frame,messages,&mut [0;4], |_|(), ||panic!("reuse")).unwrap_or_else(|r|panic!("{}",r.error));
                let new=peer.take_network().unwrap();
                assert_ne!(new.question_key(),Some(key));
                peer.release_network(new,messages);
                assert_eq!(close(peer,messages,&mut queue),0);
            }
            let free:[_;8]=core::array::from_fn(|_| messages.reserve(776).unwrap());
            for writer in free {messages.discard(writer);}
        });
    }).unwrap();
}

/// Capability-bearing returns retain their grants before authorization. A
/// refused rendezvous releases admitted grants exactly once; cancellation before
/// Return imports nothing and delegates result cleanup to the earlier Finish.
pub fn owning_adopted_capability(mode: u8) {
    use capntproto_rpc_engine::clients::{self, Capabilities};
    use capntproto_rpc_wire::{
        clients as binding,
        rpc_capnp::api::{CapDescriptor, Payload, Return},
    };
    let mut words = [[ZERO; 97]; 6];
    let mut buffers = words
        .each_mut()
        .map(|w| messages::Buffer::new(capnp::Word::words_to_bytes_mut(w)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut output = [None];
        let mut queue = super::driver::message_queue(&mut output).unwrap();
        with_owned_peer_storage::<2, 1, _>(|mut peer| {
            let frame = announcement(messages, ID);
            peer.receive(frame,messages,&mut [0;4], |_|(), ||panic!("adoption")).unwrap_or_else(|r|panic!("{}",r.error));
            let NetworkRequest::Adopt(job) = peer.take_network().unwrap() else {panic!("adoption")};
            let key = job.key();
            let mut job = Some(job);
            if mode == 2 { peer.release_network(NetworkRequest::Adopt(job.take().unwrap()),messages); }
            let returned = Frame::build(messages,776,Default::default(),|b| {
                let root = b.try_init_root::<Message>()?;
                let mut returned = root.into_field(Message::RETURN).init()?;
                returned.scalar(Return::ANSWER_ID).set(ID);
                returned.scalar(Return::RELEASE_PARAM_CAPS).set(false);
                let mut payload=returned.field(Return::RESULTS).init()?;
                payload.field(Payload::CONTENT).set_capability_index(0)?;
                payload.field(Payload::CAP_TABLE).init(1)?.get_mut(0).unwrap().scalar(CapDescriptor::SENDER_HOSTED).set(77);
                Ok(())
            }).unwrap();
            peer.receive(returned,messages,&mut [0;4], |_|{ assert_ne!(mode,2,"canceled Return must not import grants"); }, ||panic!("Return")).unwrap_or_else(|r|panic!("{}",r.error));
            if mode != 2 {
                assert_eq!(peer.take_response(key).err(),Some(Error::PendingAuthorization),"unmatched capability Return remains private");
                assert_eq!(peer.pipeline(key,pipeline::Path::new(&[])).err(),Some(Error::PendingAuthorization),"unmatched capability Return cannot route calls");
            }
            if mode == 0 {
                peer.authorize_adoption(job.take().unwrap(),messages,|_|Ok::<_,()>(())).unwrap_or_else(|_|panic!("authentication"));
                // The same API works when Return beats the rendezvous.
                let client=peer.pipeline(key,pipeline::Path::new(&[])).unwrap();
                for index in 0..peer.outgoing_capacity() {peer.poll_outgoing(index,messages).unwrap();}
                assert!(matches!(peer.runtime.inspect(&client).unwrap(),clients::Destination::Endpoint {owner:binding::Route::Capability(binding::Endpoint::Imported(remote)), ..} if remote.id().to_wire()==77));
                let response=peer.take_response(key).unwrap();
                response.with(messages,|r| {assert!(matches!(r,Reply::Results(_)));}).unwrap();
                peer.release_response(response,messages);
                peer.runtime.release(client);
                while let Some(owner)=peer.drain_endpoint(){assert!(owner.is_none());}
            } else {
                if let Some(job)=job {peer.release_network(NetworkRequest::Adopt(job),messages);}
                assert!(peer.drain_canceled_response(messages));
            }
            assert!(controls::finish_one(&mut peer.ledgers.questions,messages,&mut queue,776,Default::default()).unwrap());
            let finish=super::owning::exchange(messages,&mut queue,1);
            finish.with(messages,|root| {
                let rpc_capnp::message::Finish(f)=root.which().unwrap() else {panic!("Finish")};
                assert_eq!(f.unwrap().get_release_result_caps(),mode==2);
            }).unwrap();finish.release(messages);
            let mut releases=0;
            for _ in 0..4 {
                if controls::release_one(&mut peer.ledgers.imports,messages,&mut queue,776,Default::default()).unwrap().is_some() {
                    releases+=1;
                    let release=super::owning::exchange(messages,&mut queue,1);
                    release.with(messages,|root| {
                        let rpc_capnp::message::Release(r)=root.which().unwrap() else {panic!("Release")};
                        let r=r.unwrap();assert_eq!(r.get_id(),77);assert_eq!(r.get_reference_count(),1);
                    }).unwrap();release.release(messages);
                }
            }
            assert_eq!(releases,usize::from(mode!=2),"exactly one result grant cleanup owner");
            assert!(peer.ledgers.questions.is_empty());
            assert_eq!(close(peer,messages,&mut queue),0);
            let free:[_;6]=core::array::from_fn(|_|messages.reserve(776).unwrap());
            for writer in free{messages.discard(writer);}
        });
    }).unwrap();
}
