//! One caller-owned future interface for queued calls and local service results.
use super::{
    driver::with_owned_peer_tasks,
    driver_invocation::close,
    portable_capnp::{
        api::echo::{CapabilityParams, CapabilityResults, EchoResults},
        echo,
    },
    ZERO,
};
use capntproto_rpc_engine::clients::Capabilities as _;
use capntproto_rpc_engine::{clients, completion, messages};
use capntproto_rpc_wire::{
    clients as binding,
    owned::connection::{CallOutput, CallProgress, Error, InvocationSlot, Propagation},
    pipeline,
};
use core::{
    future::Future,
    task::{Context, Poll, Waker},
};

pub fn queued_local_reply_bindings(mode: u8) {
    let mut words = [[ZERO; 97]; 8];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut slots = [None];
        let mut queue = super::driver::message_queue(&mut slots).unwrap();
        with_owned_peer_tasks::<4, _>(|mut peer| {
            let service = peer
                .capabilities()
                .endpoint(binding::Route::local(41), clients::Location::Local)
                .unwrap();
            let capability = peer
                .capabilities()
                .endpoint(binding::Route::local(43), clients::Location::Local)
                .unwrap();
            let first_slot = completion::Slot::new();
            let second_slot = completion::Slot::new();
            let extra_slot = completion::Slot::new();
            let (first_sender, first_receiver) = super::driver::reply_pair(&first_slot);
            let mut first_receiver = Some(first_receiver);
            let (second_sender, mut second_receiver) = super::driver::reply_pair(&second_slot);
            let (extra_sender, mut extra_receiver) = super::driver::reply_pair(&extra_slot);
            let mut bindings = [const { InvocationSlot::EMPTY }; 2];
            let mut replies = super::driver::invocations(&mut bindings).unwrap();
            let mut cx = Context::from_waker(Waker::noop());
            let first = replies
                .enqueue(
                    &mut peer,
                    first_sender,
                    &service,
                    echo::Owned::CAPABILITY,
                    &[&capability],
                    messages,
                    776,
                    Default::default(),
                    |mut params| params.field(CapabilityParams::VALUE).set_index(0),
                )
                .unwrap_or_else(|_| panic!("enqueue first"));
            let pending = peer
                .invocation_pipeline(first, pipeline::Path::new(&[0]), messages)
                .unwrap();
            let second = replies
                .enqueue(
                    &mut peer,
                    second_sender,
                    &pending,
                    echo::Owned::ECHO,
                    &[],
                    messages,
                    776,
                    Default::default(),
                    |_| Ok(()),
                )
                .unwrap_or_else(|_| panic!("enqueue second"));
            let rejected = replies
                .enqueue(
                    &mut peer,
                    extra_sender,
                    &service,
                    echo::Owned::ECHO,
                    &[],
                    messages,
                    776,
                    Default::default(),
                    |_| panic!("full bindings must not invoke builder"),
                )
                .err()
                .unwrap();
            assert_eq!(rejected.error, Error::Busy);
            assert!(extra_receiver.try_take().unwrap().is_none());
            drop(rejected.sender);
            assert_eq!(
                replies.poll_one(0, &mut peer, messages, &mut cx).unwrap(),
                CallProgress::Pending
            );
            assert!(replies
                .send_one(
                    &mut peer,
                    messages,
                    &mut queue,
                    776,
                    Default::default(),
                    &mut [0; 4]
                )
                .unwrap()
                .is_none());
            if mode == 5 {
                drop(first_receiver.take());
                assert_eq!(
                    replies.poll_one(0, &mut peer, messages, &mut cx).unwrap(),
                    CallProgress::Canceled
                );
                assert_eq!(
                    peer.poll_invocation_pipeline(0).unwrap(),
                    Propagation::Resolved
                );
            } else {
                let mut call = replies.accept_local(&mut peer).unwrap().unwrap();
                assert_eq!(call.task(), first);
                call.params(messages, |params| {
                    params.params(echo::Owned::CAPABILITY).unwrap();
                })
                .unwrap();
                {
                    let expected = capability.key();
                    let mut future = core::pin::pin!(call.params_async(messages, async |params| {
                        let pointer = params
                            .params(echo::Owned::CAPABILITY)
                            .unwrap()
                            .value()
                            .unwrap();
                        let mut yielded = false;
                        core::future::poll_fn(|cx| {
                            if yielded {
                                Poll::Ready(())
                            } else {
                                yielded = true;
                                cx.waker().wake_by_ref();
                                Poll::Pending
                            }
                        })
                        .await;
                        assert_eq!(params.capability(pointer).unwrap().key(), expected);
                        83
                    }));
                    assert!(future.as_mut().poll(&mut cx).is_pending());
                    let independent = peer
                        .local_result(
                            echo::Owned::ECHO,
                            &[],
                            messages,
                            776,
                            Default::default(),
                            |mut result| {
                                result.scalar(EchoResults::VALUE).set(89);
                                Ok(())
                            },
                        )
                        .unwrap();
                    independent.release(messages, &mut peer.capabilities());
                    assert!(matches!(future.as_mut().poll(&mut cx), Poll::Ready(Ok(83))));
                }
                if mode == 4 {
                    assert_eq!(replies.terminate(&mut peer, messages), 1);
                    call.release(&mut peer, messages);
                    assert!(matches!(
                        first_receiver.as_mut().unwrap().try_take(),
                        Err(completion::Error::Canceled)
                    ));
                    assert!(matches!(
                        second_receiver.try_take(),
                        Err(completion::Error::Canceled)
                    ));
                    peer.capabilities().release(pending);
                    peer.capabilities().release(service);
                    peer.capabilities().release(capability);
                    assert_eq!(close(peer, messages, &mut queue), 84);
                    let writers =
                        core::array::from_fn::<_, 8, _>(|_| messages.reserve(776).unwrap());
                    for writer in writers {
                        messages.discard(writer);
                    }
                    return;
                }
                if mode == 1 {
                    drop(first_receiver.take());
                    // poll_one cooperatively scans at most one pipeline per
                    // call. Advanced can mean a partial scan, not completion.
                    // This fixture supplies at most four pipeline slots.
                    let mut pending = false;
                    for _ in 0..4 {
                        match replies.poll_one(0, &mut peer, messages, &mut cx).unwrap() {
                            CallProgress::Advanced => (),
                            CallProgress::Pending => {
                                pending = true;
                                break;
                            }
                            progress => panic!(
                                "running service still owes its pipelines a result: {progress:?}"
                            ),
                        }
                    }
                    assert!(
                        pending,
                        "pipeline scan must become idle within its capacity"
                    );
                    assert!(replies.accept_local(&mut peer).unwrap().is_none());
                }
                if mode == 2 {
                    let wrong = peer
                        .local_result(
                            echo::Owned::ECHO,
                            &[],
                            messages,
                            776,
                            Default::default(),
                            |_| Ok(()),
                        )
                        .unwrap();
                    let rejected = replies
                        .complete_local(&mut peer, messages, call, Ok(wrong))
                        .expect_err("wrong generated method");
                    assert!(matches!(rejected.error, Error::Input(_)));
                    rejected
                        .result
                        .unwrap()
                        .release(messages, &mut peer.capabilities());
                    call = rejected.call;
                }
                let result = if mode == 3 {
                    Err(capnp::ErrorKind::Unimplemented)
                } else {
                    Ok(peer
                        .local_result(
                            echo::Owned::CAPABILITY,
                            &[&capability],
                            messages,
                            776,
                            Default::default(),
                            |mut result| result.field(CapabilityResults::VALUE).set_index(0),
                        )
                        .unwrap())
                };
                replies
                    .complete_local(&mut peer, messages, call, result)
                    .unwrap_or_else(|_| panic!("complete first"));
                assert_eq!(
                    replies.poll_one(0, &mut peer, messages, &mut cx).unwrap(),
                    CallProgress::Advanced,
                    "settle pipeline before releasing local result"
                );
                if let Some(receiver) = first_receiver.as_mut() {
                    assert!(receiver.try_take().unwrap().is_none());
                }
                let progress = replies.poll_one(0, &mut peer, messages, &mut cx).unwrap();
                if mode == 1 {
                    assert_eq!(progress, CallProgress::Reclaimed);
                } else {
                    assert_eq!(progress, CallProgress::Delivered);
                    match first_receiver
                        .as_mut()
                        .unwrap()
                        .try_take()
                        .unwrap()
                        .unwrap()
                    {
                        Ok(output) => {
                            assert!(matches!(output, CallOutput::Local(_)));
                            peer.release_output(output, messages);
                        }
                        Err(kind) => {
                            assert_eq!(mode, 3);
                            assert_eq!(kind, capnp::ErrorKind::Unimplemented);
                        }
                    }
                    assert_eq!(
                        replies.poll_one(0, &mut peer, messages, &mut cx).unwrap(),
                        CallProgress::Consumed
                    );
                }
            }
            while peer.capabilities().advance_dispatch_one().unwrap() {}
            if mode == 3 || mode == 5 {
                assert!(replies.accept_local(&mut peer).unwrap().is_none());
                assert_eq!(
                    replies.poll_one(1, &mut peer, messages, &mut cx).unwrap(),
                    CallProgress::Advanced
                );
            } else {
                let call = replies.accept_local(&mut peer).unwrap().unwrap();
                assert_eq!(call.task(), second);
                let result = peer
                    .local_result(
                        echo::Owned::ECHO,
                        &[],
                        messages,
                        776,
                        Default::default(),
                        |mut result| {
                            result.scalar(EchoResults::VALUE).set(79);
                            Ok(())
                        },
                    )
                    .unwrap();
                replies
                    .complete_local(&mut peer, messages, call, Ok(result))
                    .unwrap_or_else(|_| panic!("complete second"));
            }
            assert_eq!(
                replies.poll_one(1, &mut peer, messages, &mut cx).unwrap(),
                CallProgress::Delivered
            );
            match second_receiver.try_take().unwrap().unwrap() {
                Ok(output) => {
                    let CallOutput::Local(result) = output else {
                        panic!("local result")
                    };
                    result
                        .with(messages, |payload| {
                            assert_eq!(payload.results(echo::Owned::ECHO).unwrap().value(), 79)
                        })
                        .unwrap();
                    peer.release_output(CallOutput::Local(result), messages);
                }
                Err(kind) => assert_eq!(
                    kind,
                    if mode == 3 {
                        capnp::ErrorKind::Unimplemented
                    } else {
                        capnp::ErrorKind::Failed
                    }
                ),
            }
            assert_eq!(
                replies.poll_one(1, &mut peer, messages, &mut cx).unwrap(),
                CallProgress::Consumed
            );
            assert!(replies.is_empty());
            assert_eq!(replies.terminate(&mut peer, messages), 0);
            peer.capabilities().release(pending);
            peer.capabilities().release(service);
            peer.capabilities().release(capability);
            assert_eq!(close(peer, messages, &mut queue), 84);
            let writers = core::array::from_fn::<_, 8, _>(|_| messages.reserve(776).unwrap());
            for writer in writers {
                messages.discard(writer);
            }
        });
    })
    .unwrap();
}

pub fn queued_remote_reply_bindings(mode: u8) {
    use capntproto_rpc_engine::ids::{AnswerId, ImportId, WireId};
    use capntproto_rpc_wire::{
        calls,
        owned::{connection::Received, Frame, Reply},
        rpc_capnp::api::{CapDescriptor, Message, Payload, Return},
    };
    let mut words = [[ZERO; 97]; 8];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut sends = [None]; let mut queue = super::driver::message_queue(&mut sends).unwrap();
        with_owned_peer_tasks::<4,_>(|mut peer| {
            let target = super::driver_invocation::remote(&mut peer, ImportId::from_wire(91));
            let storage = completion::Slot::new();
            let (sender, receiver) = super::driver::reply_pair(&storage); let mut receiver = Some(receiver);
            let mut slots = [InvocationSlot::EMPTY]; let mut replies = super::driver::invocations(&mut slots).unwrap();
            let task = replies.enqueue(&mut peer, sender, &target, echo::Owned::CAPABILITY, &[], messages, 776, Default::default(), |_|Ok(())).unwrap_or_else(|_|panic!("enqueue remote"));
            let mut cx = Context::from_waker(Waker::noop());
            assert_eq!(replies.poll_one(0, &mut peer, messages, &mut cx).unwrap(), CallProgress::Pending);
            if mode == 5 {
                drop(receiver.take());
                assert!(replies.send_one(&mut peer, messages, &mut queue, 776, Default::default(), &mut [0;4]).unwrap().is_none());
                assert_eq!(replies.poll_one(0, &mut peer, messages, &mut cx).unwrap(), CallProgress::Canceled);
                assert!(replies.is_empty());
                assert_eq!(replies.terminate(&mut peer, messages), 0);
            } else {
                let blocked = peer.send(messages, &mut queue, 776, Default::default(), calls::bootstrap).unwrap();
                assert!(replies.send_one(&mut peer, messages, &mut queue, 776, Default::default(), &mut [0;4]).is_err());
                assert!(receiver.as_mut().unwrap().try_take().unwrap().is_none(), "rejected output keeps the reply pending");
                super::owning::exchange(messages, &mut queue, 1).release(messages); peer.cancel(blocked).unwrap();
                let sent = replies.send_one(&mut peer, messages, &mut queue, 776, Default::default(), &mut [0;4]).unwrap().unwrap(); assert_eq!(sent.task, task);
                super::owning::exchange(messages, &mut queue, 1).release(messages);
                let mut replies = Some(replies);
                if mode == 1 {
                    drop(receiver.take());
                    assert_eq!(replies.as_mut().unwrap().poll_one(0, &mut peer, messages, &mut cx).unwrap(), CallProgress::Canceled);
                } else if mode == 4 {
                    assert_eq!(replies.take().unwrap().terminate(&mut peer, messages), 0);
                    assert!(matches!(receiver.as_mut().unwrap().try_take(), Err(completion::Error::Canceled)));
                }
                let frame = Frame::build(messages, 776, Default::default(), |builder| {
                    calls::results(builder, AnswerId::from_wire(sent.question.id().to_wire()), echo::Owned::CAPABILITY, |mut result| result.field(CapabilityResults::VALUE).set_index(0))?;
                    builder.try_get_root::<Message>()?.field(Message::RETURN).edit()?.field(Return::RESULTS).edit()?.field(Payload::CAP_TABLE).init(1)?.get_mut(0).unwrap().scalar(CapDescriptor::SENDER_HOSTED).set(101);
                    Ok(())
                }).unwrap();
                assert!(matches!(peer.receive(frame, messages, &mut [0;4], |_|(), ||panic!("Return")).unwrap_or_else(|_|panic!("Return")), Received::Returned(key, _) if key == sent.question));
                if mode == 1 || mode == 4 {assert!(peer.drain_canceled_response(messages));}
                else {
                    assert_eq!(replies.as_mut().unwrap().poll_one(0, &mut peer, messages, &mut cx).unwrap(), CallProgress::Delivered);
                    match mode {
                        0 => {
                            let output = receiver.as_mut().unwrap().try_take().unwrap().unwrap().unwrap();
                            let CallOutput::Remote(response) = output else {panic!("remote output")};
                            response.with(messages, |reply| {let Reply::Results(payload) = reply else {panic!("result")};
                                let pointer = echo::Owned::CAPABILITY.read_results(payload.content()).unwrap().value().unwrap();
                                assert!(matches!(payload.capability(pointer).unwrap(), capntproto_rpc_wire::reception::full::Capability::Remote(reference) if peer.ledgers().imports.wire_id(reference) == ImportId::from_wire(101)));
                            }).unwrap();
                            peer.release_response(response, messages);
                            assert_eq!(replies.as_mut().unwrap().poll_one(0, &mut peer, messages, &mut cx).unwrap(), CallProgress::Consumed);
                        }
                        2 => {
                            drop(receiver.take());
                            assert_eq!(replies.as_mut().unwrap().poll_one(0, &mut peer, messages, &mut cx).unwrap(), CallProgress::Reclaimed);
                        }
                        3 => {core::mem::forget(receiver.take()); assert_eq!(replies.take().unwrap().terminate(&mut peer, messages), 0);}
                        _ => panic!("remote reply scenario"),
                    }
                    let mut released = 0;
                    for i in 0..4 {if let Some(release) = peer.pending_import_release(i) {assert_eq!(release.id(), ImportId::from_wire(101)); released += release.count();}}
                    assert_eq!(released, 1, "queued remote reply releases its capability grant exactly once");
                }
                if let Some(replies) = replies {assert!(replies.is_empty()); assert_eq!(replies.terminate(&mut peer, messages), 0);}
            }
            peer.capabilities().release(target); assert_eq!(close(peer, messages, &mut queue), 0);
            let writers = core::array::from_fn::<_,8,_>(|_| messages.reserve(776).unwrap()); for writer in writers {messages.discard(writer);}
        });
    }).unwrap();
}
