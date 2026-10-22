//! The application owns both the reply slot and polling, while the value handed
//! through it retains the actual RPC response and its capability receipts.
use super::{
    driver::with_owned_peer,
    driver_invocation::{close, remote},
    portable_capnp::{api::echo::EchoResults, echo},
    ZERO,
};
use capntproto_rpc_engine::{
    completion,
    ids::{AnswerId, ImportId, WireId},
    messages,
};
use capntproto_rpc_wire::{
    calls,
    owned::{self, connection::Received, Frame},
};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};
pub fn caller_owned_reply_future(mode: u8) {
    let cancel = mode == 1;
    let mut words = [[ZERO; 97]; 6];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers,&mut [messages::SendSlot::EMPTY],|messages|{
        let mut slots=[None];let mut queue=super::driver::message_queue(&mut slots).unwrap();
        with_owned_peer(|mut peer|{
            let target=remote(&mut peer,ImportId::from_wire(91));
            let mut reply=completion::Slot::new();
            let (sender,mut receiver)=reply.pair().unwrap();
            let mut cx=Context::from_waker(Waker::noop());
            assert!(Pin::new(&mut receiver).poll(&mut cx).is_pending());
            let mut receiver=Some(receiver);
            let task=peer.enqueue_call(&target,echo::Owned::ECHO,&[],messages,776,Default::default(),|_|Ok(())).unwrap();
            let sent=peer.send_invocation(messages,&mut queue,776,Default::default(),&mut [0;4]).unwrap().unwrap();assert_eq!(sent.task,task);
            super::owning::exchange(messages,&mut queue,1).release(messages);
            if cancel{drop(receiver.take());assert!(sender.is_closed());peer.cancel(sent.question).unwrap();}
            let frame=Frame::build(messages,776,Default::default(),|builder|calls::results(builder,AnswerId::from_wire(sent.question.id().to_wire()),echo::Owned::ECHO,|mut result|{result.scalar(EchoResults::VALUE).set(61);Ok(())})).unwrap();
            assert!(matches!(peer.receive(frame,messages,&mut [0;4],|_|(),||panic!("Return")).unwrap_or_else(|_|panic!("Return")),Received::Returned(key, _)if key==sent.question));
            let response=peer.take_response(sent.question).unwrap();
            match sender.send(response){
                Err(rejected)=>{assert!(cancel);peer.release_response(rejected,messages);}
                Ok(())=>{
                    assert!(!cancel);
                    if mode == 0 {
                    let Poll::Ready(Ok(response))=Pin::new(receiver.as_mut().unwrap()).poll(&mut cx)else{panic!("reply future")};
                    response.with(messages,|reply|{let owned::Reply::Results(payload)=reply else{panic!("result")};assert_eq!(echo::Owned::ECHO.read_results(payload.content()).unwrap().value(),61)}).unwrap();
                    peer.release_response(response,messages);
                    }
                }
            }
            if mode == 3 { core::mem::forget(receiver); } else { drop(receiver); }
            if mode >= 2 {
                assert!(matches!(reply.pair(), Err(completion::Error::Undelivered)));
                let response = reply.take_undelivered().expect("canceled receiver retains RPC receipts");
                peer.release_response(response, messages);
                assert!(reply.take_undelivered().is_none());
                let (_sender, _receiver) = reply.pair().unwrap();
            }
            peer.runtime.release(target);
            assert_eq!(close(peer,messages,&mut queue),0);
            let writers=core::array::from_fn::<_,6,_>(|_|messages.reserve(776).unwrap());for writer in writers{messages.discard(writer)}
        });
    }).unwrap();
}
