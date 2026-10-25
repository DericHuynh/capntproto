//! Caller-pinned legacy Promise ownership in a real Call envelope.
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
use capnp::{
    capability::{PreparedRequest, Promise, Request},
    field_api::Struct,
    message,
};
use capntproto_rpc_wire::rpc_capnp::{self, api};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};
const ZERO: capnp::Word = capnp::word(0, 0, 0, 0, 0, 0, 0, 0);
fn exercise() {
    let mut words = [ZERO; 32];
    let mut builder = message::FixedBuilder::new(&mut words).unwrap();
    {
        let mut slot = capntproto_rpc_engine::completion::Slot::new();
        let (sender, receiver) = slot.pair().unwrap();
        let operation = receiver;
        let mut tasks = core::pin::pin!(capntproto_rpc_engine::tasks::PinnedSlots::<_, 1>::new());
        let [place] = tasks.as_mut().places();
        let owner = place.insert_erased(operation).ok().unwrap();
        let mut completion = Promise::from_owner(owner);
        assert!(Pin::new(&mut completion)
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        let root = builder.try_init_root::<api::Message>().unwrap();
        let mut call = root.into_field(api::Message::CALL).init().unwrap();
        call.scalar(api::Call::QUESTION_ID).set(37);
        call.scalar(api::Call::INTERFACE_ID).set(42);
        call.scalar(api::Call::METHOD_ID).set(3);
        let payload = call.into_field(api::Call::PARAMS).init().unwrap();
        let content = payload
            .into_field(api::Payload::CONTENT)
            .cast::<Struct<api::Exception>>();
        let mut request =
            Request::<rpc_capnp::exception::Owned, rpc_capnp::exception::Owned, _>::from_owner(
                PreparedRequest::new(content, move |owner| sender.send(owner)),
            );
        request
            .try_get()
            .unwrap()
            .field(api::Exception::REASON)
            .copy_from("kept")
            .unwrap();
        assert!(request.send().is_ok());
        let Poll::Ready(Ok(_owner)) =
            Pin::new(&mut completion).poll(&mut Context::from_waker(Waker::noop()))
        else {
            panic!("request not transferred")
        };
        drop(completion);
        assert!(tasks.is_empty());
    }
    let root = builder
        .get_root_as_reader::<rpc_capnp::message::Reader<'_>>()
        .unwrap();
    let rpc_capnp::message::Call(call) = root.which().unwrap() else {
        panic!("Call lost")
    };
    let call = call.unwrap();
    assert_eq!(call.get_question_id(), 37);
    assert_eq!(call.get_interface_id(), 42);
    assert_eq!(call.get_method_id(), 3);
    assert_eq!(
        call.get_params()
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
