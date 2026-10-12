//! Reply registry integration, including abandonment while transport IO is idle.
use super::{
    driver::with_owned_peer,
    driver_invocation::{close, remote},
    portable_capnp::{api::echo::CapabilityResults, echo},
    ZERO,
};
use capntproto_rpc_engine::{
    completion,
    ids::{AnswerId, ImportId, WireId},
    messages,
};
use capntproto_rpc_wire::{
    calls,
    owned::{
        self,
        connection::{Received, Replies, ReplyError, ReplyProgress, ReplySlot},
        Frame,
    },
    rpc_capnp::api::{CapDescriptor, Message, Payload, Return},
};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

pub fn tracked_reply_delivery(mode: u8) {
    let mut words = [[ZERO; 97]; 6];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut sends = [None]; let mut queue = super::driver::message_queue(&mut sends).unwrap();
        with_owned_peer(|mut peer| {
            let target = remote(&mut peer, ImportId::from_wire(91));
            let mut reply = completion::Slot::new();
            let mut duplicate = completion::Slot::new();
            let mut second = completion::Slot::new();
            {
            let (sender, mut receiver) = reply.pair().unwrap();
            let (duplicate_sender, mut duplicate_receiver) = duplicate.pair().unwrap();
            let (second_sender, mut second_receiver) = second.pair().unwrap();
            let mut bindings = [ReplySlot::EMPTY];
            let mut replies = Replies::new(&mut bindings).unwrap();
            assert!(replies.is_empty()); assert_eq!(replies.capacity(), 1);
            let mut cx = Context::from_waker(Waker::noop());
            assert!(Pin::new(&mut receiver).poll(&mut cx).is_pending());
            let mut receiver = Some(receiver);
            peer.enqueue_call(&target, echo::Owned::CAPABILITY, &[], messages, 776, Default::default(), |_| Ok(())).unwrap();
            let sent = peer.send_invocation(messages, &mut queue, 776, Default::default(), &mut [0;4]).unwrap().unwrap();
            super::owning::exchange(messages, &mut queue, 1).release(messages);
            let index = replies.track(&peer, sent.question, sender).unwrap_or_else(|_| panic!("track first reply"));
            assert_eq!(index, 0);
            let rejected = replies.track(&peer, sent.question, duplicate_sender).expect_err("duplicate reply binding");
            assert_eq!(rejected.error, ReplyError::AlreadyTracked);
            assert!(duplicate_receiver.try_take().unwrap().is_none(), "rejection must preserve sender");
            drop(rejected.sender);
            assert!(matches!(duplicate_receiver.try_take(), Err(completion::Error::Canceled)));
            peer.enqueue_call(&target, echo::Owned::CAPABILITY, &[], messages, 776, Default::default(), |_| Ok(())).unwrap();
            let second_sent = peer.send_invocation(messages, &mut queue, 776, Default::default(), &mut [0;4]).unwrap().unwrap();
            super::owning::exchange(messages, &mut queue, 1).release(messages);
            let rejected = replies.track(&peer, second_sent.question, second_sender).expect_err("full reply bindings");
            assert_eq!(rejected.error, ReplyError::Full);
            assert!(second_receiver.try_take().unwrap().is_none());
            assert_eq!(replies.poll_one(index, &mut peer, messages, &mut cx).unwrap(), ReplyProgress::Pending);
            if mode == 1 {
                drop(receiver.take());
                assert_eq!(replies.poll_one(index, &mut peer, messages, &mut cx).unwrap(), ReplyProgress::Canceled);
            } else if mode == 4 {
                assert!(replies.cancel(index, &mut peer, messages).unwrap());
                assert!(matches!(receiver.as_mut().unwrap().try_take(), Err(completion::Error::Canceled)));
            }
            let frame = Frame::build(messages, 776, Default::default(), |builder| {
                calls::results(builder, AnswerId::from_wire(sent.question.id().to_wire()), echo::Owned::CAPABILITY, |mut result| result.field(CapabilityResults::VALUE).set_index(0))?;
                builder.try_get_root::<Message>()?.field(Message::RETURN).edit()?.field(Return::RESULTS).edit()?.field(Payload::CAP_TABLE).init(1)?.get_mut(0).unwrap().scalar(CapDescriptor::SENDER_HOSTED).set(101);
                Ok(())
            }).unwrap();
            assert!(matches!(peer.receive(frame, messages, &mut [0;4], |_| (), || panic!("Return")).unwrap_or_else(|_| panic!("Return")), Received::Returned(key) if key == sent.question));
            if mode == 1 || mode == 4 {
                assert!(peer.drain_canceled_response(messages));
            } else {
                assert_eq!(replies.poll_one(index, &mut peer, messages, &mut cx).unwrap(), ReplyProgress::Delivered);
                assert_eq!(replies.poll_one(index, &mut peer, messages, &mut cx).unwrap(), ReplyProgress::Pending);
                for i in 0..4 { assert!(peer.ledgers.imports.pending_release(i).is_none(), "delivered response retains capability receipts"); }
                match mode {
                    0 | 6 => {
                        let Poll::Ready(Ok(response)) = Pin::new(receiver.as_mut().unwrap()).poll(&mut cx) else {panic!("reply future")};
                        response.with(messages, |reply| {
                            let owned::Reply::Results(payload) = reply else {panic!("result")};
                            let pointer = echo::Owned::CAPABILITY.read_results(payload.content()).unwrap().value().unwrap();
                            assert!(matches!(payload.capability(pointer).unwrap(), capntproto_rpc_wire::reception::full::Capability::Remote(reference) if peer.ledgers.imports.wire_id(reference) == ImportId::from_wire(101)));
                        }).unwrap();
                        if mode == 6 {assert!(replies.cancel(index, &mut peer, messages).unwrap());}
                        else {assert_eq!(replies.poll_one(index, &mut peer, messages, &mut cx).unwrap(), ReplyProgress::Consumed);}
                        peer.release_response(response, messages);
                    }
                    2 => {
                        drop(receiver.take());
                        assert_eq!(replies.poll_one(index, &mut peer, messages, &mut cx).unwrap(), ReplyProgress::Reclaimed);
                    }
                    3 => {
                        core::mem::forget(receiver.take());
                        assert!(replies.cancel(index, &mut peer, messages).unwrap());
                    }
                    5 => {
                        assert!(replies.cancel(index, &mut peer, messages).unwrap());
                        assert!(matches!(receiver.as_mut().unwrap().try_take(), Err(completion::Error::Canceled)));
                    }
                    _ => panic!("reply scenario"),
                }
                let mut released = 0;
                for i in 0..4 { if let Some(release) = peer.ledgers.imports.pending_release(i) { assert_eq!(release.id(), ImportId::from_wire(101)); released += release.count(); } }
                assert_eq!(released, 1, "reclaimed response releases its exact capability grant");
            }
            assert!(replies.is_empty());
            assert_eq!(replies.poll_one(index, &mut peer, messages, &mut cx).unwrap(), ReplyProgress::Idle);
            assert!(!replies.cancel(index, &mut peer, messages).unwrap());
            // The sender rejected on capacity remains valid and can be retried.
            let second_index = replies.track(&peer, second_sent.question, rejected.sender).unwrap_or_else(|_| panic!("retry reply binding"));
            assert!(replies.cancel(second_index, &mut peer, messages).unwrap());
            assert!(matches!(second_receiver.try_take(), Err(completion::Error::Canceled)));
            assert!(replies.is_empty());
            }
            assert!(reply.take_undelivered().is_none());
            let (_sender, _receiver) = reply.pair().unwrap();
            peer.runtime.release(target);
            assert_eq!(close(peer, messages, &mut queue), 0);
            let writers = core::array::from_fn::<_,6,_>(|_| messages.reserve(776).unwrap());
            for writer in writers {messages.discard(writer)}
        });
    }).unwrap();
}
