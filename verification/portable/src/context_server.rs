//! Generated context adapters can await nested RPC without borrowing the pump.
use super::{
    driver::with_owned_peer_tasks,
    driver_invocation::close,
    portable_capnp::{
        api::echo::{CapabilityParams, CapabilityParamsRef, CapabilityResults, EchoResults},
        derived_service, echo,
    },
    ZERO,
};
use capnp::{field_api::FixedStruct, wire_capability::Reader};
use capntproto_rpc_engine::clients::Capabilities as _;
use capntproto_rpc_engine::{clients, completion, messages};
use capntproto_rpc_wire::{
    clients as binding,
    owned::connection::{CallProgress, InvocationSlot, Session},
    rpc_capnp,
};
use core::{
    cell::Cell,
    future::Future,
    task::{Context, Poll, Waker},
};

trait CapabilityContext {
    fn invoke<'a>(
        &'a mut self,
        pointer: Reader<'a, echo::Owned>,
        results: FixedStruct<'a, CapabilityResults>,
    ) -> impl Future<Output = capnp::Result<()>> + 'a;
}
struct Callback<F>(F);
impl<F> CapabilityContext for Callback<F>
where
    F: for<'a> core::ops::AsyncFnMut(
        Reader<'a, echo::Owned>,
        FixedStruct<'a, CapabilityResults>,
    ) -> capnp::Result<()>,
{
    async fn invoke<'a>(
        &'a mut self,
        pointer: Reader<'a, echo::Owned>,
        results: FixedStruct<'a, CapabilityResults>,
    ) -> capnp::Result<()> {
        (self.0)(pointer, results).await
    }
}
struct Server;
impl<C: CapabilityContext> echo::ContextServer<C> for Server {
    async fn capability<'call>(
        &'call mut self,
        params: CapabilityParamsRef<'call>,
        results: FixedStruct<'call, CapabilityResults>,
        context: &'call mut C,
    ) -> capnp::Result<()> {
        context.invoke(params.value()?, results).await
    }
}
impl<C: CapabilityContext> derived_service::ContextServer<C> for Server {}

pub fn generated_context_nested_call() {
    let mut words = [[ZERO; 97]; 8];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut sends = [None];
        let mut queue = super::driver::message_queue(&mut sends).unwrap();
        with_owned_peer_tasks::<4, _>(|mut peer| {
            let service = peer
                .capabilities()
                .endpoint(binding::Route::local(41), clients::Location::Local)
                .unwrap();
            let callback = peer
                .capabilities()
                .endpoint(binding::Route::local(43), clients::Location::Local)
                .unwrap();
            let expected = callback.key();
            let first_slot = completion::Slot::new();
            let nested_slot = completion::Slot::new();
            let (sender, mut receiver) = first_slot.try_pair().unwrap();
            let mut bindings = [const { InvocationSlot::EMPTY }; 2];
            let mut replies = super::driver::invocations(&mut bindings).unwrap();
            let first = replies
                .enqueue(
                    &mut peer,
                    sender,
                    &service,
                    echo::Owned::CAPABILITY,
                    &[&callback],
                    messages,
                    776,
                    Default::default(),
                    |mut params| params.field(CapabilityParams::VALUE).set_index(0),
                )
                .unwrap_or_else(|_| panic!("enqueue"));
            let call = replies.accept_local(&mut peer).unwrap().unwrap();
            assert_eq!(call.task(), first);
            let mut draft = peer
                .local_result_draft(echo::Owned::CAPABILITY, messages, 776, Default::default())
                .unwrap();
            let session = Session::new(peer, messages, replies, 776, Default::default());
            let nested_task = Cell::new(None);
            let observed = Cell::new(0);
            let mut cx = Context::from_waker(Waker::noop());
            let nested_slot = &nested_slot;
            {
                let future = session.access(|_, messages, _| call.params_async(messages, async |params| {
                    let writing = session.access(|_, messages, _|
                        draft.write_async(messages, async |mut content, capabilities| {
                            let mut server = derived_service::ContextServerDispatch::new(Server);
                            let mut context = Callback(
                                async |pointer: Reader<'_, echo::Owned>,
                                       mut results: FixedStruct<'_, CapabilityResults>|
                                       -> capnp::Result<()> {
                                    let target = params.capability(pointer).unwrap();
                                    assert_eq!(target.key(), expected);
                                    let reply = session.call(nested_slot, target, echo::Owned::ECHO, &[], |_| Ok(())).unwrap();
                                    nested_task.set(Some(reply.task()));
                                    let output = reply.await.unwrap();
                                    observed.set(session.read(&output, |results| results.value()).unwrap());
                                    session.release(output).unwrap_or_else(|_| panic!("nested reply release"));
                                    let client = session.access(|peer, _, _| peer.capabilities().retain(target)).unwrap().unwrap();
                                    if let Err(rejected) = capabilities
                                        .set(results.field(CapabilityResults::VALUE), client)
                                    {
                                        session.access(|peer, _, _| peer.capabilities().release(rejected.client)).unwrap();
                                        return Err(capnp::Error::from_kind(
                                            capnp::ErrorKind::Failed,
                                        ));
                                    }
                                    Ok(())
                                },
                            );
                            assert!(!params.context_service_info(&server).unwrap().streaming);
                            params
                                .serve_with_context(&mut server, content.reborrow(), &mut context)
                                .await
                        })).unwrap();
                    writing.await
                })).unwrap();
                let mut future = core::pin::pin!(future);
                assert!(future.as_mut().poll(&mut cx).is_pending());
                assert!(receiver.try_take().unwrap().is_none());
                // The parent future is suspended while its peer, binding table
                // and message pool are all available to execute the nested call.
                session.access(|peer, messages, replies| {
                    let nested = replies.accept_local(peer).unwrap().unwrap();
                    assert_eq!(Some(nested.task()), nested_task.get());
                    let result = peer
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
                        .unwrap();
                    replies
                        .complete_local(peer, messages, nested, Ok(result))
                        .unwrap_or_else(|_| panic!("nested completion"));
                    assert_eq!(
                        replies
                            .poll_one(1, peer, messages, &mut cx)
                            .unwrap(),
                        CallProgress::Delivered
                    );
                }).unwrap();
                assert!(matches!(
                    future.as_mut().poll(&mut cx),
                    Poll::Ready(Ok(Ok(())))
                ));
            }
            assert_eq!(observed.get(), 113);
            let (mut peer, messages, mut replies) = session.into_parts();
            assert_eq!(
                replies.poll_one(1, &mut peer, messages, &mut cx).unwrap(),
                CallProgress::Consumed
            );
            let result = draft.finish(messages, &mut peer.capabilities()).unwrap();
            result
                .with(messages, |payload| {
                    let pointer = payload
                        .results(echo::Owned::CAPABILITY)
                        .unwrap()
                        .value()
                        .unwrap();
                    assert_eq!(pointer.index(), 0);
                    assert_eq!(payload.capability(pointer).unwrap().key(), expected);
                })
                .unwrap();
            replies
                .complete_local(&mut peer, messages, call, Ok(result))
                .unwrap_or_else(|_| panic!("parent completion"));
            assert_eq!(
                replies.poll_one(0, &mut peer, messages, &mut cx).unwrap(),
                CallProgress::Delivered
            );
            peer.release_output(receiver.try_take().unwrap().unwrap().unwrap(), messages);
            assert_eq!(
                replies.poll_one(0, &mut peer, messages, &mut cx).unwrap(),
                CallProgress::Consumed
            );
            assert!(replies.is_empty());
            assert_eq!(replies.terminate(&mut peer, messages), 0);
            peer.capabilities().release(service);
            peer.capabilities().release(callback);
            assert_eq!(close(peer, messages, &mut queue), 84);
            let writers = core::array::from_fn::<_, 8, _>(|_| messages.reserve(776).unwrap());
            for writer in writers {
                messages.discard(writer);
            }
        });
    })
    .unwrap();
}

struct BasicContext {
    sum: u64,
}
struct BasicServer;
impl echo::ContextServer<BasicContext> for BasicServer {
    async fn echo<'call>(
        &'call mut self,
        params: super::portable_capnp::api::echo::EchoParamsRef<'call>,
        mut results: FixedStruct<'call, EchoResults>,
        context: &'call mut BasicContext,
    ) -> capnp::Result<()> {
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
        context.sum += params.value();
        results.scalar(EchoResults::VALUE).set(context.sum);
        Ok(())
    }
    async fn write<'call>(
        &'call mut self,
        params: super::portable_capnp::api::echo::WriteParamsRef<'call>,
        context: &'call mut BasicContext,
    ) -> capnp::Result<()> {
        context.sum += params.value();
        Ok(())
    }
}
impl derived_service::ContextServer<BasicContext> for BasicServer {}
impl super::portable_capnp::generic_service::ContextServer<BasicContext, capnp::text::Owned>
    for BasicServer
{
    async fn echo<'call>(
        &'call mut self,
        params: super::portable_capnp::api::generic_service::EchoParamsRef<
            'call,
            capnp::text::Owned,
        >,
        mut results: FixedStruct<
            'call,
            super::portable_capnp::api::generic_service::EchoResults<capnp::text::Owned>,
        >,
        context: &'call mut BasicContext,
    ) -> capnp::Result<()> {
        results
            .field(super::portable_capnp::api::generic_service::EchoResults::VALUE)
            .typed()
            .copy_from(params.value()?.to_str()?)?;
        context.sum += 1;
        Ok(())
    }
}
impl super::portable_capnp::text_service::ContextServer<BasicContext> for BasicServer {}

pub fn generated_context_wire_dispatch() -> capnp::Result<()> {
    use capntproto_rpc_engine::ids::{ImportId, QuestionId, WireId};
    use capntproto_rpc_wire::{
        calls,
        rpc_capnp::api::{Call, Message},
    };
    let mut source = [ZERO; 80];
    let mut result = [ZERO; 80];
    let mut request = capnp::message::FixedBuilder::new(&mut source)?;
    let mut response = capnp::message::FixedBuilder::new(&mut result)?;
    let mut context = BasicContext { sum: 7 };
    let mut server = derived_service::ContextServerDispatch::new(BasicServer);
    let mut cx = Context::from_waker(Waker::noop());
    calls::call(
        &mut request,
        QuestionId::from_wire(3),
        ImportId::from_wire(1),
        echo::Owned::ECHO,
        |mut params| {
            params
                .scalar(super::portable_capnp::api::echo::EchoParams::VALUE)
                .set(11);
            Ok(())
        },
    )?;
    {
        let reader =
            capnp::message::Reader::new(request.get_segments_for_output(), Default::default());
        let rpc_capnp::message::Call(call) = reader
            .get_root::<rpc_capnp::message::Reader<'_>>()?
            .which()
            .unwrap()
        else {
            unreachable!()
        };
        let incoming = calls::Incoming::new(call?);
        assert!(!incoming.context_service_info(&server)?.streaming);
        {
            let mut canceled = core::pin::pin!(incoming.serve_with_context(
                &mut server,
                &mut response,
                &mut context
            ));
            assert!(canceled.as_mut().poll(&mut cx).is_pending());
        }
        assert_eq!(context.sum, 7);
        response.reset();
        {
            let mut future = core::pin::pin!(incoming.serve_with_context(
                &mut server,
                &mut response,
                &mut context
            ));
            assert!(future.as_mut().poll(&mut cx).is_pending());
            assert!(matches!(
                future.as_mut().poll(&mut cx),
                Poll::Ready(Ok(calls::ResultsTo::Caller))
            ));
        }
        assert_eq!(context.sum, 18);
        let reader =
            capnp::message::Reader::new(response.get_segments_for_output(), Default::default());
        let rpc_capnp::message::Return(reply) = reader
            .get_root::<rpc_capnp::message::Reader<'_>>()?
            .which()
            .unwrap()
        else {
            unreachable!()
        };
        let rpc_capnp::return_::Results(payload) = reply?.which().unwrap() else {
            unreachable!()
        };
        assert_eq!(
            echo::Owned::ECHO
                .read_results(payload?.get_content())?
                .value(),
            18
        );
    }
    request.reset();
    response.reset();
    calls::streaming_call(
        &mut request,
        QuestionId::from_wire(4),
        calls::Target::Imported(ImportId::from_wire(1)),
        echo::Owned::WRITE,
        |mut params| {
            params
                .scalar(super::portable_capnp::api::echo::WriteParams::VALUE)
                .set(13);
            Ok(())
        },
    )?;
    {
        let reader =
            capnp::message::Reader::new(request.get_segments_for_output(), Default::default());
        let rpc_capnp::message::Call(call) = reader
            .get_root::<rpc_capnp::message::Reader<'_>>()?
            .which()
            .unwrap()
        else {
            unreachable!()
        };
        let incoming = calls::Incoming::new(call?);
        assert!(incoming.context_service_info(&server)?.streaming);
        let mut future =
            core::pin::pin!(incoming.serve_with_context(&mut server, &mut response, &mut context));
        assert!(matches!(
            future.as_mut().poll(&mut cx),
            Poll::Ready(Ok(calls::ResultsTo::Caller))
        ));
    }
    assert_eq!(context.sum, 31);
    request
        .try_get_root::<Message>()?
        .field(Message::CALL)
        .edit()?
        .scalar(Call::METHOD_ID)
        .set(u16::MAX);
    response.reset();
    response.try_set_root_text("unchanged")?;
    {
        let reader =
            capnp::message::Reader::new(request.get_segments_for_output(), Default::default());
        let rpc_capnp::message::Call(call) = reader
            .get_root::<rpc_capnp::message::Reader<'_>>()?
            .which()
            .unwrap()
        else {
            unreachable!()
        };
        let incoming = calls::Incoming::new(call?);
        assert!(incoming.context_service_info(&server).is_err());
        let mut future =
            core::pin::pin!(incoming.serve_with_context(&mut server, &mut response, &mut context));
        assert!(matches!(future.as_mut().poll(&mut cx), Poll::Ready(Err(_))));
    }
    assert_eq!(
        response.get_root_as_reader::<capnp::text::Reader<'_>>()?,
        "unchanged"
    );
    assert_eq!(context.sum, 31);
    request.reset();
    response.reset();
    let method = super::portable_capnp::generic_service::Owned::<capnp::text::Owned>::ECHO;
    calls::call(
        &mut request,
        QuestionId::from_wire(5),
        ImportId::from_wire(1),
        method,
        |mut params| {
            params
                .field(super::portable_capnp::api::generic_service::EchoParams::VALUE)
                .typed()
                .copy_from("contextual")
        },
    )?;
    let mut branded = super::portable_capnp::text_service::ContextServerDispatch::new(BasicServer);
    {
        let reader =
            capnp::message::Reader::new(request.get_segments_for_output(), Default::default());
        let rpc_capnp::message::Call(call) = reader
            .get_root::<rpc_capnp::message::Reader<'_>>()?
            .which()
            .unwrap()
        else {
            unreachable!()
        };
        let incoming = calls::Incoming::new(call?);
        let mut future =
            core::pin::pin!(incoming.serve_with_context(&mut branded, &mut response, &mut context));
        assert!(matches!(
            future.as_mut().poll(&mut cx),
            Poll::Ready(Ok(calls::ResultsTo::Caller))
        ));
    }
    let reader =
        capnp::message::Reader::new(response.get_segments_for_output(), Default::default());
    let rpc_capnp::message::Return(reply) = reader
        .get_root::<rpc_capnp::message::Reader<'_>>()?
        .which()
        .unwrap()
    else {
        unreachable!()
    };
    let rpc_capnp::return_::Results(payload) = reply?.which().unwrap() else {
        unreachable!()
    };
    assert_eq!(
        method.read_results(payload?.get_content())?.value()?,
        "contextual"
    );
    assert_eq!(context.sum, 32);
    Ok(())
}
