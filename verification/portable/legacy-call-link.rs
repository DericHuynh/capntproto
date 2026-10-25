//! One caller-owned legacy invocation from a real Call payload to a Return.
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
use capnp::{
    capability::{CallContext, PreparedCall, Promise},
    field_api::Struct,
    message,
    private::capability::ParamsHook,
};
use capntproto_rpc_wire::rpc_capnp::{self, api};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};
const ZERO: capnp::Word = capnp::word(0, 0, 0, 0, 0, 0, 0, 0);
struct Input<S: message::ReaderSegments>(message::Reader<S>);
impl<S: message::ReaderSegments> ParamsHook for Input<S> {
    fn get(&self) -> capnp::Result<capnp::any_pointer::Reader<'_>> {
        let root = self.0.get_root::<rpc_capnp::message::Reader<'_>>()?;
        let rpc_capnp::message::Call(call) = root.which()? else {
            return Err(capnp::Error::from_kind(capnp::ErrorKind::Failed));
        };
        Ok(call?.get_params()?.get_content())
    }
}
fn exercise() {
    let mut input_words = [ZERO; 32];
    let mut input = message::FixedBuilder::new(&mut input_words).unwrap();
    {
        let root = input.try_init_root::<api::Message>().unwrap();
        let mut call = root.into_field(api::Message::CALL).init().unwrap();
        call.scalar(api::Call::QUESTION_ID).set(37);
        call.scalar(api::Call::INTERFACE_ID).set(42);
        call.scalar(api::Call::METHOD_ID).set(3);
        let payload = call.into_field(api::Call::PARAMS).init().unwrap();
        payload
            .into_field(api::Payload::CONTENT)
            .cast::<Struct<api::Exception>>()
            .init()
            .unwrap()
            .field(api::Exception::REASON)
            .copy_from("kept")
            .unwrap();
    }
    let segments = [input.into_segment()];
    let reader = Input(message::Reader::new(&segments[..], Default::default()));
    let mut output_words = [ZERO; 32];
    let mut output = message::FixedBuilder::new(&mut output_words).unwrap();
    {
        let root = output.try_init_root::<api::Message>().unwrap();
        let mut returned = root.into_field(api::Message::RETURN).init().unwrap();
        returned.scalar(api::Return::ANSWER_ID).set(37);
        returned.scalar(api::Return::RELEASE_PARAM_CAPS).set(false);
        let content = returned
            .into_field(api::Return::RESULTS)
            .init()
            .unwrap()
            .into_field(api::Payload::CONTENT)
            .cast::<Struct<api::Exception>>();
        let call =
            CallContext::<rpc_capnp::exception::Owned, rpc_capnp::exception::Owned, _>::from_owner(
                PreparedCall::new(reader, content),
            );
        let (params, mut results) = call.into_owned();
        let mut gate = capntproto_rpc_engine::completion::Slot::new();
        let (ready, wait) = gate.pair().unwrap();
        let operation = async move {
            wait.await
                .map_err(|_| capnp::Error::from_kind(capnp::ErrorKind::Disconnected))?;
            results
                .try_get()?
                .field(api::Exception::REASON)
                .copy_from(params.get()?.get_reason()?.to_str()?)
        };
        let mut tasks = core::pin::pin!(capntproto_rpc_engine::tasks::PinnedSlots::<_, 1>::new());
        let [place] = tasks.as_mut().places();
        let mut completion = Promise::from_owner(place.insert_erased(operation).ok().unwrap());
        let mut cx = Context::from_waker(Waker::noop());
        assert!(Pin::new(&mut completion).poll(&mut cx).is_pending());
        assert!(ready.send(()).is_ok());
        assert!(matches!(
            Pin::new(&mut completion).poll(&mut cx),
            Poll::Ready(Ok(()))
        ));
        drop(completion);
        assert!(tasks.is_empty());
    }
    let root = output
        .get_root_as_reader::<rpc_capnp::message::Reader<'_>>()
        .unwrap();
    let rpc_capnp::message::Return(returned) = root.which().unwrap() else {
        panic!("Return lost")
    };
    let returned = returned.unwrap();
    assert_eq!(returned.get_answer_id(), 37);
    assert!(!returned.get_release_param_caps());
    let rpc_capnp::return_::Results(payload) = returned.which().unwrap() else {
        panic!("results lost")
    };
    assert_eq!(
        payload
            .unwrap()
            .get_content()
            .get_as::<rpc_capnp::exception::Reader<'_>>()
            .unwrap()
            .get_reason()
            .unwrap(),
        "kept"
    );
}
#[cfg(not(target_os = "none"))]
fn main() {
    exercise();
}
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    exercise();
    loop {
        core::hint::spin_loop();
    }
}
#[cfg(target_os = "none")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
