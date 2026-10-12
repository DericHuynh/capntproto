//! Local service completion uses the same graph and caller-owned reply slots.
use super::{
    driver::with_owned_peer_tasks,
    driver_invocation::close,
    portable_capnp::{api::echo::CapabilityResults, echo},
    ZERO,
};
use capntproto_rpc_engine::{clients, completion, messages};
use capntproto_rpc_wire::{
    clients as binding,
    owned::{
        self,
        connection::{Propagation, Task},
    },
    pipeline,
};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

pub fn local_reply_ownership(mode: u8) {
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
            let capability = peer
                .runtime
                .endpoint(binding::Route::local(43), clients::Location::Local)
                .unwrap();
            let expected = capability.key();
            let mut storage = completion::Slot::new();
            {
                let (sender, mut receiver) = storage.pair().unwrap();
                let mut delivery = sender.track();
                let mut cx = Context::from_waker(Waker::noop());
                assert!(Pin::new(&mut receiver).poll(&mut cx).is_pending());
                let mut receiver = Some(receiver);
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
                let ready = peer.runtime.next_ready().unwrap().unwrap();
                assert_eq!(ready.key(), first);
                let running = ready.accept();
                // Invalid capability tables roll back the new message, not the live call.
                assert!(peer
                    .local_result(
                        echo::Owned::CAPABILITY,
                        &[],
                        messages,
                        776,
                        Default::default(),
                        |mut result| result.field(CapabilityResults::VALUE).set_index(0)
                    )
                    .is_err());
                let reply = peer
                    .local_result(
                        echo::Owned::CAPABILITY,
                        &[&capability],
                        messages,
                        776,
                        Default::default(),
                        |mut result| result.field(CapabilityResults::VALUE).set_index(0),
                    )
                    .unwrap();
                let other = peer
                    .local_result(
                        echo::Owned::CAPABILITY,
                        &[&capability],
                        messages,
                        776,
                        Default::default(),
                        |mut result| result.field(CapabilityResults::VALUE).set_index(0),
                    )
                    .unwrap();
                other
                    .with(messages, |foreign| {
                        let pointer = foreign
                            .results(echo::Owned::CAPABILITY)
                            .unwrap()
                            .value()
                            .unwrap();
                        reply
                            .with(messages, |local| {
                                assert!(
                                    matches!(
                                        local.capability(pointer),
                                        Err(owned::Error::WrongMessage)
                                    ),
                                    "same index from another message is not authority"
                                );
                                assert!(
                                    local.results(echo::Owned::ECHO).is_err(),
                                    "generated method identity is preserved"
                                );
                            })
                            .unwrap();
                    })
                    .unwrap();
                other.release(messages, &mut peer.runtime);
                peer.runtime.release(capability);
                // The pipeline keeps working even if the original reply is canceled.
                if mode == 1 {
                    drop(receiver.take());
                }
                assert_eq!(
                    peer.poll_local_invocation_pipeline_with(0, first, |_, _| {
                        Err(owned::connection::Error::Busy)
                    }),
                    Err(owned::connection::Error::Busy)
                );
                assert_eq!(
                    peer.poll_local_result(0, first, &reply, messages).unwrap(),
                    Propagation::Resolved
                );
                let Task::Invoke(invocation) = peer.runtime.complete(running) else {
                    panic!("Invoke")
                };
                invocation.release(messages, &mut peer.runtime);
                assert!(delivery.poll_observed(&mut cx).is_ready() == (mode == 1));
                match delivery.try_send(reply) {
                    Err(reply) => {
                        assert_eq!(mode, 1);
                        reply.release(messages, &mut peer.runtime);
                    }
                    Ok(()) => match mode {
                        0 => {
                            let Poll::Ready(Ok(reply)) =
                                Pin::new(receiver.as_mut().unwrap()).poll(&mut cx)
                            else {
                                panic!("local reply future")
                            };
                            reply
                                .with(messages, |payload| {
                                    let pointer = payload
                                        .results(echo::Owned::CAPABILITY)
                                        .unwrap()
                                        .value()
                                        .unwrap();
                                    assert_eq!(
                                        payload.capability(pointer).unwrap().key(),
                                        expected
                                    );
                                })
                                .unwrap();
                            reply.release(messages, &mut peer.runtime);
                        }
                        2 => {
                            drop(receiver.take());
                            delivery
                                .take_abandoned()
                                .unwrap()
                                .release(messages, &mut peer.runtime);
                        }
                        3 => {
                            core::mem::forget(receiver.take());
                            delivery
                                .cancel()
                                .unwrap()
                                .release(messages, &mut peer.runtime);
                        }
                        _ => panic!("local reply scenario"),
                    },
                }
                assert!(delivery.poll_observed(&mut cx).is_ready());
                let ready = peer.runtime.next_ready().unwrap().unwrap();
                assert_eq!(ready.key(), second);
                assert!(matches!(
                    ready.destination(),
                    clients::Destination::Endpoint {
                        owner: binding::Route::Capability(binding::Endpoint::Local(43)),
                        ..
                    }
                ));
                let running = ready.accept();
                let Task::Invoke(invocation) = peer.runtime.complete(running) else {
                    panic!("Invoke")
                };
                invocation.release(messages, &mut peer.runtime);
                peer.runtime.release(pending);
            }
            assert!(storage.take_undelivered().is_none());
            peer.runtime.release(service);
            assert_eq!(close(peer, messages, &mut queue), 84);
            let writers = core::array::from_fn::<_, 8, _>(|_| messages.reserve(776).unwrap());
            for writer in writers {
                messages.discard(writer);
            }
        });
    })
    .unwrap();
}
