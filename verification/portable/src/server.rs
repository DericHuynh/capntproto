//! Generated, stack-owned async server methods over borrowed messages.
use super::*;
use capnp::field_api;
use capntproto_rpc_engine::ids::{ImportId, QuestionId, WireId};
use capntproto_rpc_wire::{calls, rpc_capnp};
use core::{
    future::Future,
    task::{Context, Poll, Waker},
};
use portable_capnp::{
    api::echo::{EchoParamsRef, EchoResults},
    api::{GenericParamsRef, GenericResults},
    derived_service, echo,
};

struct Server {
    processed: u64,
}
impl echo::Server for Server {
    async fn echo<'call>(
        &'call mut self,
        params: EchoParamsRef<'call>,
        mut results: field_api::FixedStruct<'call, EchoResults>,
    ) -> Result<()> {
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
        results.scalar(EchoResults::VALUE).set(params.value() + 1);
        self.processed += 1;
        Ok(())
    }
    async fn write<'call>(
        &'call mut self,
        params: portable_capnp::api::echo::WriteParamsRef<'call>,
    ) -> Result<()> {
        self.processed += params.value();
        Ok(())
    }
}
impl derived_service::Server for Server {
    async fn call<'call>(
        &'call mut self,
        _params: GenericParamsRef<'call>,
        _results: field_api::FixedStruct<'call, GenericResults>,
    ) -> Result<()> {
        Ok(())
    }
}

pub fn generated_async_server() -> Result<()> {
    let mut source = [ZERO; 80];
    let mut output = [ZERO; 80];
    let mut request = message::FixedBuilder::new(&mut source)?;
    let mut response = message::FixedBuilder::new(&mut output)?;
    let mut server = derived_service::ServerDispatch::new(Server { processed: 0 });
    for redirected in [false, true] {
        request.reset();
        response.reset();
        calls::call(
            &mut request,
            QuestionId::from_wire(3),
            ImportId::from_wire(1),
            echo::Owned::ECHO,
            |mut params| {
                params
                    .scalar(portable_capnp::api::echo::EchoParams::VALUE)
                    .set(41);
                Ok(())
            },
        )?;
        if redirected {
            let mut root = request.try_get_root::<rpc_capnp::api::Message>()?;
            root.field(rpc_capnp::api::Message::CALL)
                .edit()?
                .group(rpc_capnp::api::Call::SEND_RESULTS_TO)
                .edit()?
                .void(rpc_capnp::api::call::SendResultsTo::YOURSELF)
                .set();
        }
        let reader = message::Reader::new(request.get_segments_for_output(), Default::default());
        let rpc_capnp::message::Call(call) = reader
            .get_root::<rpc_capnp::message::Reader<'_>>()?
            .which()
            .unwrap()
        else {
            unreachable!()
        };
        let incoming = calls::Incoming::new(call?);
        assert!(!incoming.service_info(&server)?.streaming);
        let mut cx = Context::from_waker(Waker::noop());
        let processed = server.server.processed;
        {
            let mut canceled = core::pin::pin!(incoming.serve(&mut server, &mut response));
            assert!(canceled.as_mut().poll(&mut cx).is_pending());
        }
        assert_eq!(server.server.processed, processed);
        response.reset();
        {
            let mut future = core::pin::pin!(incoming.serve(&mut server, &mut response));
            assert!(future.as_mut().poll(&mut cx).is_pending());
            let Poll::Ready(done) = future.as_mut().poll(&mut cx) else {
                unreachable!()
            };
            assert_eq!(
                done?,
                if redirected {
                    calls::ResultsTo::Yourself
                } else {
                    calls::ResultsTo::Caller
                }
            );
        }
        let reader = message::Reader::new(response.get_segments_for_output(), Default::default());
        let rpc_capnp::message::Return(ret) = reader
            .get_root::<rpc_capnp::message::Reader<'_>>()?
            .which()
            .unwrap()
        else {
            unreachable!()
        };
        let ret = ret?;
        assert_eq!(ret.get_answer_id(), 3);
        let rpc_capnp::return_::Results(payload) = ret.which().unwrap() else {
            unreachable!()
        };
        assert_eq!(
            echo::Owned::ECHO
                .read_results(payload?.get_content())?
                .value(),
            42
        );
    }
    assert_eq!(server.server.processed, 2);
    // Streaming completion uses an empty payload and its generated policy.
    request.reset();
    response.reset();
    calls::streaming_call(
        &mut request,
        QuestionId::from_wire(4),
        calls::Target::Imported(ImportId::from_wire(1)),
        echo::Owned::WRITE,
        |mut params| {
            params
                .scalar(portable_capnp::api::echo::WriteParams::VALUE)
                .set(3);
            Ok(())
        },
    )?;
    let reader = message::Reader::new(request.get_segments_for_output(), Default::default());
    let rpc_capnp::message::Call(call) = reader
        .get_root::<rpc_capnp::message::Reader<'_>>()?
        .which()
        .unwrap()
    else {
        unreachable!()
    };
    let incoming = calls::Incoming::new(call?);
    assert!(incoming.service_info(&server)?.streaming);
    {
        let mut future = core::pin::pin!(incoming.serve(&mut server, &mut response));
        assert!(matches!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Ok(calls::ResultsTo::Caller))
        ));
    }
    assert_eq!(server.server.processed, 5);
    {
        let mut root = request.try_get_root::<rpc_capnp::api::Message>()?;
        root.field(rpc_capnp::api::Message::CALL)
            .edit()?
            .scalar(rpc_capnp::api::Call::METHOD_ID)
            .set(u16::MAX);
    }
    response.reset();
    response.try_set_root_text("unchanged")?;
    let reader = message::Reader::new(request.get_segments_for_output(), Default::default());
    let rpc_capnp::message::Call(call) = reader
        .get_root::<rpc_capnp::message::Reader<'_>>()?
        .which()
        .unwrap()
    else {
        unreachable!()
    };
    let incoming = calls::Incoming::new(call?);
    assert!(incoming.service_info(&server).is_err());
    {
        let mut future = core::pin::pin!(incoming.serve(&mut server, &mut response));
        assert!(matches!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Err(_))
        ));
    }
    assert_eq!(
        response.get_root_as_reader::<capnp::text::Reader<'_>>()?,
        "unchanged"
    );
    assert_eq!(server.server.processed, 5);
    Ok(())
}

impl portable_capnp::generic_service::Server<capnp::text::Owned> for Server {
    async fn echo<'call>(
        &'call mut self,
        params: portable_capnp::api::generic_service::EchoParamsRef<'call, capnp::text::Owned>,
        mut results: field_api::FixedStruct<
            'call,
            portable_capnp::api::generic_service::EchoResults<capnp::text::Owned>,
        >,
    ) -> Result<()> {
        results
            .field(portable_capnp::api::generic_service::EchoResults::VALUE)
            .typed()
            .copy_from(params.value()?.to_str()?)?;
        self.processed += 1;
        Ok(())
    }
}
impl portable_capnp::text_service::Server for Server {}

pub fn generated_branded_server() -> Result<()> {
    let mut source = [ZERO; 40];
    let mut output = [ZERO; 40];
    let mut request = message::FixedBuilder::new(&mut source)?;
    let mut response = message::FixedBuilder::new(&mut output)?;
    let mut server = portable_capnp::text_service::ServerDispatch::new(Server { processed: 0 });
    let method = portable_capnp::generic_service::Owned::<capnp::text::Owned>::ECHO;
    calls::call(
        &mut request,
        QuestionId::from_wire(1),
        ImportId::from_wire(1),
        method,
        |mut params| {
            params
                .field(portable_capnp::api::generic_service::EchoParams::VALUE)
                .typed()
                .copy_from("borrowed")
        },
    )?;
    let reader = message::Reader::new(request.get_segments_for_output(), Default::default());
    let rpc_capnp::message::Call(call) = reader
        .get_root::<rpc_capnp::message::Reader<'_>>()?
        .which()
        .unwrap()
    else {
        unreachable!()
    };
    let incoming = calls::Incoming::new(call?);
    {
        let mut future = core::pin::pin!(incoming.serve(&mut server, &mut response));
        assert!(matches!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Ok(calls::ResultsTo::Caller))
        ));
    }
    let reader = message::Reader::new(response.get_segments_for_output(), Default::default());
    let rpc_capnp::message::Return(ret) = reader
        .get_root::<rpc_capnp::message::Reader<'_>>()?
        .which()
        .unwrap()
    else {
        unreachable!()
    };
    let rpc_capnp::return_::Results(payload) = ret?.which().unwrap() else {
        unreachable!()
    };
    assert_eq!(
        method.read_results(payload?.get_content())?.value()?,
        "borrowed"
    );
    assert_eq!(server.server.processed, 1);
    Ok(())
}
