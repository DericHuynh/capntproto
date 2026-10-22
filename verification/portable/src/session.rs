//! The shared session rejects reentrancy while preserving affine retry owners.
use super::{
    driver::with_owned_peer,
    driver_invocation::{close, remote},
    portable_capnp::{api::echo::EchoResults, echo},
    ZERO,
};
use capntproto_rpc_engine::clients::Capabilities as _;
use capntproto_rpc_engine::{
    clients, completion,
    ids::{AnswerId, ImportId, WireId},
    messages,
};
use capntproto_rpc_wire::{
    calls, clients as binding,
    owned::{
        connection::{CallError, CallProgress, Error, InvocationSlot, Session},
        Frame,
    },
    rpc_capnp::{
        self,
        api::{Exception, Message, Return},
    },
};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

pub fn typed_session_calls(mode: u8) {
    let mut words = [[ZERO; 97]; 6];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut sends = [None];
        let mut queue = super::driver::message_queue(&mut sends).unwrap();
        with_owned_peer(|mut peer| {
            let remote_call = mode >= 5;
            let target = if remote_call {
                remote(&mut peer, ImportId::from_wire(91))
            } else {
                peer.capabilities()
                    .endpoint(binding::Route::local(43), clients::Location::Local)
                    .unwrap()
            };
            let slot = completion::Slot::new();
            let spare = completion::Slot::new();
            let mut bindings = [InvocationSlot::EMPTY];
            let calls = super::driver::invocations(&mut bindings).unwrap();
            let session = Session::new(peer, messages, calls, 776, Default::default());
            let mut cx = Context::from_waker(Waker::noop());
            session
                .access(|_, _, _| {
                    let rejected = session
                        .call(&slot, &target, echo::Owned::ECHO, &[], |_| {
                            panic!("busy filler")
                        })
                        .err()
                        .unwrap();
                    assert_eq!(rejected, CallError::Busy);
                    #[cfg(feature = "alloc")]
                    assert_eq!(
                        session
                            .call_owned(&target, echo::Owned::ECHO, &[], |_| panic!(
                                "busy owned filler"
                            ))
                            .err(),
                        Some(CallError::Busy)
                    );
                })
                .unwrap();
            assert!(session
                .call(&slot, &target, echo::Owned::ECHO, &[], |_| Err(
                    capnp::Error::from_kind(capnp::ErrorKind::Failed)
                ))
                .is_err());
            #[cfg(not(feature = "growing"))]
            let reply = session
                .call(&slot, &target, echo::Owned::ECHO, &[], |_| Ok(()))
                .unwrap();
            #[cfg(feature = "growing")]
            let reply = session
                .call_owned(&target, echo::Owned::ECHO, &[], |_| Ok(()))
                .unwrap();
            let mut reply = Some(reply);
            assert!(Pin::new(reply.as_mut().unwrap()).poll(&mut cx).is_pending());
            assert_eq!(
                session
                    .call(&slot, &target, echo::Owned::ECHO, &[], |_| panic!(
                        "slot filler"
                    ))
                    .err()
                    .unwrap(),
                if cfg!(feature = "growing") {
                    CallError::Connection(Error::Busy)
                } else {
                    CallError::Completion(completion::Error::Busy)
                }
            );
            assert_eq!(
                session
                    .call(&spare, &target, echo::Owned::ECHO, &[], |_| panic!(
                        "capacity filler"
                    ))
                    .err()
                    .unwrap(),
                CallError::Connection(Error::Busy)
            );
            drop(spare.try_pair().unwrap());
            if mode == 2 {
                drop(reply.take());
                session
                    .access(|driver, messages, calls| {
                        assert_eq!(
                            calls.poll_one(0, driver, messages, &mut cx).unwrap(),
                            CallProgress::Canceled
                        );
                    })
                    .unwrap();
            } else {
                session
                    .access(|driver, messages, calls| {
                        if remote_call {
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
                            super::owning::exchange(messages, &mut queue, 1).release(messages);
                            let frame =
                                Frame::build(messages, 776, Default::default(), |builder| {
                                    if mode == 6 {
                                        let root = builder.try_init_root::<Message>()?;
                                        let mut result = root.into_field(Message::RETURN).init()?;
                                        result
                                            .scalar(Return::ANSWER_ID)
                                            .set(sent.question.id().to_wire());
                                        result
                                            .into_field(Return::EXCEPTION)
                                            .init()?
                                            .scalar(Exception::TYPE)
                                            .set(rpc_capnp::api::exception::Type::Overloaded);
                                        Ok(())
                                    } else {
                                        calls::results(
                                            builder,
                                            AnswerId::from_wire(sent.question.id().to_wire()),
                                            echo::Owned::ECHO,
                                            |mut results| {
                                                results.scalar(EchoResults::VALUE).set(113);
                                                Ok(())
                                            },
                                        )
                                    }
                                })
                                .unwrap();
                            driver
                                .receive(
                                    frame,
                                    messages,
                                    &mut [0; 4],
                                    |_| (),
                                    || panic!("Return producer"),
                                )
                                .unwrap_or_else(|_| panic!("Return admission"));
                        } else {
                            let call = calls.accept_local(driver).unwrap().unwrap();
                            assert_eq!(call.task(), reply.as_ref().unwrap().task());
                            let output = if mode == 1 {
                                Err(capnp::ErrorKind::Unimplemented)
                            } else {
                                Ok(driver
                                    .local_result(
                                        echo::Owned::ECHO,
                                        &[],
                                        messages,
                                        776,
                                        Default::default(),
                                        |mut results| {
                                            results.scalar(EchoResults::VALUE).set(113);
                                            Ok(())
                                        },
                                    )
                                    .unwrap())
                            };
                            calls
                                .complete_local(driver, messages, call, output)
                                .unwrap_or_else(|_| panic!("local completion"));
                        }
                        assert_eq!(
                            calls.poll_one(0, driver, messages, &mut cx).unwrap(),
                            CallProgress::Delivered
                        );
                    })
                    .unwrap();
                if mode == 3 {
                    drop(reply.take());
                    session
                        .access(|driver, messages, calls| {
                            assert_eq!(
                                calls.poll_one(0, driver, messages, &mut cx).unwrap(),
                                CallProgress::Reclaimed
                            );
                        })
                        .unwrap();
                } else {
                    let Poll::Ready(output) = Pin::new(reply.as_mut().unwrap()).poll(&mut cx)
                    else {
                        panic!("pending delivered result")
                    };
                    match output {
                        Err(error) => {
                            assert_eq!(mode, 1);
                            assert_eq!(error, CallError::Failed(capnp::ErrorKind::Unimplemented));
                        }
                        Ok(output) => {
                            let value = session.read(&output, |results| results.value());
                            if mode == 6 {
                                assert_eq!(
                                    value,
                                    Err(CallError::Failed(capnp::ErrorKind::Overloaded))
                                );
                            } else {
                                assert_eq!(value, Ok(113));
                            }
                            let output = if mode == 4 {
                                let rejected = session
                                    .access(|_, _, _| session.release(output).err().unwrap())
                                    .unwrap();
                                assert_eq!(rejected.error, CallError::Busy);
                                assert_eq!(
                                    session.read(&rejected.value, |results| results.value()),
                                    Ok(113)
                                );
                                rejected.value
                            } else {
                                output
                            };
                            if mode == 7 {
                                let rejected = session
                                    .access(|_, _, _| {
                                        session
                                            .access(move |driver, messages, _| {
                                                driver
                                                    .release_output(output.into_output(), messages)
                                            })
                                            .err()
                                            .unwrap()
                                    })
                                    .unwrap();
                                assert_eq!(rejected.error, CallError::Busy);
                                session.access(rejected.operation).unwrap();
                            } else {
                                session
                                    .release(output)
                                    .unwrap_or_else(|_| panic!("release result"));
                            }
                        }
                    }
                    drop(reply.take());
                    session
                        .access(|driver, messages, calls| {
                            assert_eq!(
                                calls.poll_one(0, driver, messages, &mut cx).unwrap(),
                                CallProgress::Consumed
                            );
                        })
                        .unwrap();
                }
            }
            drop(slot.try_pair().unwrap());
            let (mut peer, messages, calls) = session.into_parts();
            assert!(calls.is_empty());
            assert_eq!(calls.terminate(&mut peer, messages), 0);
            peer.capabilities().release(target);
            assert_eq!(
                close(peer, messages, &mut queue),
                if remote_call { 0 } else { 43 }
            );
            let writers = core::array::from_fn::<_, 6, _>(|_| messages.reserve(776).unwrap());
            for writer in writers {
                messages.discard(writer);
            }
        });
    })
    .unwrap();
}
