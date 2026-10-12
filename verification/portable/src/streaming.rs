//! Generated streaming methods use ordered invocation ownership and empty acks.
use super::{
    driver::with_owned_peer_tasks,
    driver_invocation::{close, remote},
    portable_capnp::{
        api::echo::{WriteParams, WriteParamsRef},
        echo,
    },
    ZERO,
};
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
    pipeline,
    rpc_capnp::{
        self,
        api::{Exception, Message, Payload, Return},
    },
};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

struct Sink;
impl echo::ContextServer<u64> for Sink {
    async fn write<'call>(
        &'call mut self,
        params: WriteParamsRef<'call>,
        context: &'call mut u64,
    ) -> capnp::Result<()> {
        *context += params.value();
        Ok(())
    }
}

pub fn streaming_session_calls(mode: u8) {
    let mut words = [[ZERO; 97]; 6];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut sends = [None];
        let mut queue = super::driver::message_queue(&mut sends).unwrap();
        with_owned_peer_tasks::<4, _>(|mut peer| {
            let remote_call = matches!(mode, 2 | 3 | 6);
            let target = if remote_call {
                remote(&mut peer, ImportId::from_wire(91))
            } else {
                peer.runtime
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
                    assert_eq!(
                        session
                            .streaming_call(&slot, &target, echo::Owned::WRITE, &[], |_| panic!(
                                "busy"
                            ))
                            .err()
                            .unwrap(),
                        CallError::Busy
                    );
                })
                .unwrap();
            #[cfg(not(feature = "growing"))]
            let reply = session
                .streaming_call(&slot, &target, echo::Owned::WRITE, &[], |mut params| {
                    params.scalar(WriteParams::VALUE).set(81);
                    Ok(())
                })
                .unwrap();
            #[cfg(feature = "growing")]
            let reply = session
                .streaming_call_owned(&target, echo::Owned::WRITE, &[], |mut params| {
                    params.scalar(WriteParams::VALUE).set(81);
                    Ok(())
                })
                .unwrap();
            let mut reply = Some(reply);
            assert_eq!(
                session
                    .streaming_call(&spare, &target, echo::Owned::WRITE, &[], |_| panic!(
                        "capacity"
                    ))
                    .err()
                    .unwrap(),
                CallError::Connection(Error::Busy)
            );
            drop(spare.try_pair().unwrap());
            session
                .access(|driver, messages, _| {
                    assert_eq!(
                        driver
                            .invocation_pipeline(
                                reply.as_ref().unwrap().task(),
                                pipeline::Path::new(&[0]),
                                messages
                            )
                            .err()
                            .unwrap(),
                        Error::Input(capntproto_rpc_wire::owned::Error::Wire(
                            capnp::ErrorKind::Unimplemented
                        ))
                    );
                })
                .unwrap();
            if mode == 4 {
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
                            if mode == 6 {
                                let blocked = driver
                                    .send(
                                        messages,
                                        &mut queue,
                                        776,
                                        Default::default(),
                                        calls::bootstrap,
                                    )
                                    .unwrap();
                                assert!(calls
                                    .send_one(
                                        driver,
                                        messages,
                                        &mut queue,
                                        776,
                                        Default::default(),
                                        &mut [0; 4]
                                    )
                                    .is_err());
                                assert!(Pin::new(reply.as_mut().unwrap())
                                    .poll(&mut cx)
                                    .is_pending());
                                super::owning::exchange(messages, &mut queue, 1).release(messages);
                                driver.cancel(blocked).unwrap();
                            }
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
                            let sent_frame = super::owning::exchange(messages, &mut queue, 1);
                            sent_frame
                                .with(messages, |root| {
                                    let rpc_capnp::message::Call(call) = root.which().unwrap()
                                    else {
                                        panic!("streaming call")
                                    };
                                    let call = call.unwrap();
                                    assert!(call.get_no_promise_pipelining());
                                    assert_eq!(
                                        calls::Incoming::new(call)
                                            .dispatch_streaming(echo::Owned::WRITE)
                                            .unwrap()
                                            .params
                                            .value(),
                                        81
                                    );
                                })
                                .unwrap();
                            sent_frame.release(messages);
                            let frame =
                                Frame::build(messages, 776, Default::default(), |builder| {
                                    if mode == 3 {
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
                                        calls::acknowledge(
                                            builder,
                                            AnswerId::from_wire(sent.question.id().to_wire()),
                                        )
                                    }
                                })
                                .unwrap();
                            driver
                                .receive(frame, messages, &mut [0; 4], |_| (), || panic!("return"))
                                .unwrap_or_else(|_| panic!("ack admission"));
                        } else {
                            let call = calls.accept_local(driver).unwrap().unwrap();
                            {
                                let future = call.params_async(messages, async |params| {
                                    assert_eq!(
                                        params
                                            .streaming_params(echo::Owned::WRITE)
                                            .unwrap()
                                            .value(),
                                        81
                                    );
                                    let mut server = echo::ContextServerDispatch::new(Sink);
                                    let info = params.context_service_info(&server).unwrap();
                                    assert!(info.streaming && info.no_promise_pipelining);
                                    let mut words = [ZERO; 16];
                                    let mut output =
                                        capnp::message::FixedBuilder::new(&mut words).unwrap();
                                    let content = output
                                        .try_init_root::<Payload>()
                                        .unwrap()
                                        .into_field(Payload::CONTENT);
                                    let mut sum = 0;
                                    params
                                        .serve_with_context(&mut server, content, &mut sum)
                                        .await
                                        .unwrap();
                                    sum
                                });
                                let mut future = core::pin::pin!(future);
                                assert_eq!(future.as_mut().poll(&mut cx), Poll::Ready(Ok(81)));
                            }
                            let result = if mode == 1 {
                                Err(capnp::ErrorKind::Unimplemented)
                            } else {
                                Ok(driver
                                    .local_ack(
                                        echo::Owned::WRITE,
                                        messages,
                                        776,
                                        Default::default(),
                                    )
                                    .unwrap())
                            };
                            calls
                                .complete_local(driver, messages, call, result)
                                .unwrap_or_else(|_| panic!("stream completion"));
                        }
                        assert_eq!(
                            calls.poll_one(0, driver, messages, &mut cx).unwrap(),
                            CallProgress::Delivered
                        );
                    })
                    .unwrap();
                if mode == 5 {
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
                        panic!("ack pending")
                    };
                    match output {
                        Err(error) => {
                            assert_eq!(mode, 1);
                            assert_eq!(error, CallError::Failed(capnp::ErrorKind::Unimplemented));
                        }
                        Ok(ack) => {
                            assert_eq!(
                                session.check_ack(&ack),
                                if mode == 3 {
                                    Err(CallError::Failed(capnp::ErrorKind::Overloaded))
                                } else {
                                    Ok(())
                                }
                            );
                            session
                                .release_ack(ack)
                                .unwrap_or_else(|_| panic!("ack release"));
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
            peer.runtime.release(target);
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
