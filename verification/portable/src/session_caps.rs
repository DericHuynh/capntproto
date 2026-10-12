//! Generated capability paths and returned fields preserve callable interface types.
use super::{
    driver::with_owned_peer_tasks,
    driver_invocation::{close, remote},
    portable_capnp::{
        api::echo::{CapabilityResults, EchoResults},
        echo,
    },
    ZERO,
};
#[cfg(not(feature = "growing"))]
use capntproto_rpc_engine::completion;
use capntproto_rpc_engine::{
    clients,
    ids::{AnswerId, ImportId, WireId},
    messages,
};
use capntproto_rpc_wire::{
    calls,
    clients::{self as binding, TypedClient},
    owned::{
        connection::{CallProgress, InvocationSlot, Session},
        Frame,
    },
    rpc_capnp::api::{CapDescriptor, Message, Payload, Return},
};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

pub fn typed_capability_calls(remote_case: bool) {
    let mut words = [[ZERO; 97]; 8];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut sends = [None];
        let mut queue = super::driver::message_queue(&mut sends).unwrap();
        with_owned_peer_tasks::<4, _>(|mut peer| {
            let target = TypedClient::<echo::Owned>::from_client(if remote_case {
                remote(&mut peer, ImportId::from_wire(91))
            } else {
                peer.runtime
                    .endpoint(binding::Route::local(41), clients::Location::Local)
                    .unwrap()
            });
            let returned = peer
                .runtime
                .endpoint(binding::Route::local(43), clients::Location::Local)
                .unwrap();
            #[cfg(not(feature = "growing"))]
            let first_slot = completion::Slot::new();
            #[cfg(not(feature = "growing"))]
            let second_slot = completion::Slot::new();
            let mut bindings = [const { InvocationSlot::EMPTY }; 2];
            let calls = super::driver::invocations(&mut bindings).unwrap();
            let session = Session::new(peer, messages, calls, 776, Default::default());
            let mut cx = Context::from_waker(Waker::noop());
            #[cfg(not(feature = "growing"))]
            let mut first = session
                .call(&first_slot, &target, echo::Owned::CAPABILITY, &[], |_| {
                    Ok(())
                })
                .unwrap();
            #[cfg(feature = "growing")]
            let mut first = session
                .call_owned(&target, echo::Owned::CAPABILITY, &[], |_| Ok(()))
                .unwrap();
            let mut path_storage = [0];
            let path = echo::Owned::CAPABILITY
                .pipeline(&mut path_storage)
                .capability(CapabilityResults::VALUE)
                .unwrap();
            let pipelined = session.pipeline(&first, path).unwrap();
            #[cfg(not(feature = "growing"))]
            let mut second = session
                .call(&second_slot, &pipelined, echo::Owned::ECHO, &[], |_| Ok(()))
                .unwrap();
            #[cfg(feature = "growing")]
            let mut second = session
                .call_owned(&pipelined, echo::Owned::ECHO, &[], |_| Ok(()))
                .unwrap();
            assert!(Pin::new(&mut second).poll(&mut cx).is_pending());
            session
                .access(|driver, messages, calls| {
                    if remote_case {
                        let sent = calls
                            .send_one(
                                driver,
                                messages,
                                &mut queue,
                                776,
                                Default::default(),
                                &mut [0; 4],
                            )
                            .unwrap()
                            .unwrap();
                        assert_eq!(sent.task, first.task());
                        super::owning::exchange(messages, &mut queue, 1).release(messages);
                        driver.poll_invocation_pipeline(0).unwrap();
                        let frame = Frame::build(messages, 776, Default::default(), |builder| {
                            calls::results(
                                builder,
                                AnswerId::from_wire(sent.question.id().to_wire()),
                                echo::Owned::CAPABILITY,
                                |mut results| results.field(CapabilityResults::VALUE).set_index(0),
                            )?;
                            builder
                                .try_get_root::<Message>()?
                                .field(Message::RETURN)
                                .edit()?
                                .field(Return::RESULTS)
                                .edit()?
                                .field(Payload::CAP_TABLE)
                                .init(1)?
                                .get_mut(0)
                                .unwrap()
                                .scalar(CapDescriptor::SENDER_HOSTED)
                                .set(101);
                            Ok(())
                        })
                        .unwrap();
                        driver
                            .receive(
                                frame,
                                messages,
                                &mut [0; 4],
                                |_| (),
                                || panic!("return producer"),
                            )
                            .unwrap_or_else(|_| panic!("return"));
                        for index in 0..driver.outgoing_capacity() {
                            driver.poll_outgoing(index, messages).unwrap();
                        }
                    } else {
                        let call = calls.accept_local(driver).unwrap().unwrap();
                        assert_eq!(call.task(), first.task());
                        let result = driver
                            .local_result(
                                echo::Owned::CAPABILITY,
                                &[&returned],
                                messages,
                                776,
                                Default::default(),
                                |mut results| results.field(CapabilityResults::VALUE).set_index(0),
                            )
                            .unwrap();
                        calls
                            .complete_local(driver, messages, call, Ok(result))
                            .unwrap_or_else(|_| panic!("completion"));
                    }
                    let mut delivered = false;
                    for _ in 0..8 {
                        if calls.poll_one(0, driver, messages, &mut cx).unwrap()
                            == CallProgress::Delivered
                        {
                            delivered = true;
                            break;
                        }
                    }
                    assert!(delivered);
                })
                .unwrap();
            let Poll::Ready(Ok(output)) = Pin::new(&mut first).poll(&mut cx) else {
                panic!("first reply")
            };
            drop(first);
            let extracted = session
                .capability(&output, |results| results.value())
                .unwrap();
            let retained = session.retain_client(&extracted).unwrap();
            session
                .release(output)
                .unwrap_or_else(|_| panic!("first release"));
            session
                .access(|driver, messages, calls| {
                    assert_eq!(
                        calls.poll_one(0, driver, messages, &mut cx).unwrap(),
                        CallProgress::Consumed
                    );
                    if remote_case {
                        let sent = calls
                            .send_one(
                                driver,
                                messages,
                                &mut queue,
                                776,
                                Default::default(),
                                &mut [0; 4],
                            )
                            .unwrap()
                            .unwrap();
                        assert_eq!(sent.task, second.task());
                        super::owning::exchange(messages, &mut queue, 1).release(messages);
                        let frame = Frame::build(messages, 776, Default::default(), |builder| {
                            calls::results(
                                builder,
                                AnswerId::from_wire(sent.question.id().to_wire()),
                                echo::Owned::ECHO,
                                |mut results| {
                                    results.scalar(EchoResults::VALUE).set(137);
                                    Ok(())
                                },
                            )
                        })
                        .unwrap();
                        driver
                            .receive(
                                frame,
                                messages,
                                &mut [0; 4],
                                |_| (),
                                || panic!("return producer"),
                            )
                            .unwrap_or_else(|_| panic!("echo return"));
                    } else {
                        assert_eq!(extracted.as_client().key(), returned.key());
                        let call = calls.accept_local(driver).unwrap().unwrap();
                        assert_eq!(call.task(), second.task());
                        let result = driver
                            .local_result(
                                echo::Owned::ECHO,
                                &[],
                                messages,
                                776,
                                Default::default(),
                                |mut results| {
                                    results.scalar(EchoResults::VALUE).set(137);
                                    Ok(())
                                },
                            )
                            .unwrap();
                        calls
                            .complete_local(driver, messages, call, Ok(result))
                            .unwrap_or_else(|_| panic!("second completion"));
                    }
                    assert_eq!(
                        calls.poll_one(1, driver, messages, &mut cx).unwrap(),
                        CallProgress::Delivered
                    );
                })
                .unwrap();
            let Poll::Ready(Ok(output)) = Pin::new(&mut second).poll(&mut cx) else {
                panic!("second reply")
            };
            drop(second);
            assert_eq!(session.read(&output, |results| results.value()), Ok(137));
            session
                .release(output)
                .unwrap_or_else(|_| panic!("second release"));
            session
                .access(|driver, messages, calls| {
                    assert_eq!(
                        calls.poll_one(1, driver, messages, &mut cx).unwrap(),
                        CallProgress::Consumed
                    );
                })
                .unwrap();
            // This client still calls after the original reply and both pipeline
            // replies have been released. Cancel before output to recover it.
            #[cfg(not(feature = "growing"))]
            let third = session
                .call(&first_slot, &retained, echo::Owned::ECHO, &[], |_| Ok(()))
                .unwrap();
            #[cfg(feature = "growing")]
            let third = session
                .call_owned(&retained, echo::Owned::ECHO, &[], |_| Ok(()))
                .unwrap();
            drop(third);
            session
                .access(|driver, messages, calls| {
                    assert_eq!(
                        calls.poll_one(0, driver, messages, &mut cx).unwrap(),
                        CallProgress::Canceled
                    );
                })
                .unwrap();
            for client in [pipelined, extracted, retained, target] {
                session
                    .release_client(client)
                    .unwrap_or_else(|_| panic!("client release"));
            }
            let (mut peer, messages, calls) = session.into_parts();
            assert!(calls.is_empty());
            assert_eq!(calls.terminate(&mut peer, messages), 0);
            peer.runtime.release(returned);
            assert_eq!(
                close(peer, messages, &mut queue),
                if remote_case { 43 } else { 84 }
            );
            let writers = core::array::from_fn::<_, 8, _>(|_| messages.reserve(776).unwrap());
            for writer in writers {
                messages.discard(writer);
            }
        });
    })
    .unwrap();
}
