//! A borrowed legacy Client builds a concrete Request and invokes a caller-pinned service.
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
use capnp::{
    capability::{
        CallContext, CallHints, Client, ClientStorage, PayloadStorage, PreparedCall, Promise,
        RequestStorage,
    },
    field_api::{FixedStruct, Struct},
    message,
    private::capability::ParamsHook,
};
use capntproto_rpc_wire::rpc_capnp::{self, api};
use core::{
    cell::{RefCell, RefMut},
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
struct Provider<'buffer>(RefCell<message::FixedBuilder<'buffer>>);
struct Draft<'call, 'buffer>(RefMut<'call, message::FixedBuilder<'buffer>>);
impl<'buffer> ClientStorage<rpc_capnp::exception::Owned, rpc_capnp::exception::Owned>
    for Provider<'buffer>
{
    type Request<'call>
        = Draft<'call, 'buffer>
    where
        Self: 'call;
    fn try_new_call(
        &self,
        interface: u64,
        method: u16,
        _: Option<capnp::MessageSize>,
        hints: CallHints,
    ) -> capnp::Result<Self::Request<'_>> {
        let mut message = self
            .0
            .try_borrow_mut()
            .map_err(|_| capnp::Error::from_kind(capnp::ErrorKind::Overloaded))?;
        let root = message.try_upgrade_root::<api::Message>()?;
        let mut call = root.into_field(api::Message::CALL).upgrade()?;
        call.scalar(api::Call::INTERFACE_ID).set(interface);
        call.scalar(api::Call::METHOD_ID).set(method);
        call.scalar(api::Call::NO_PROMISE_PIPELINING)
            .set(hints.no_promise_pipelining);
        Ok(Draft(message))
    }
}
impl PayloadStorage<rpc_capnp::exception::Owned> for Draft<'_, '_> {
    type Builder<'a>
        = FixedStruct<'a, api::Exception>
    where
        Self: 'a;
    fn try_get(&mut self, _: Option<capnp::MessageSize>) -> capnp::Result<Self::Builder<'_>> {
        let root = self.0.try_upgrade_root::<api::Message>()?;
        let content = root
            .into_field(api::Message::CALL)
            .upgrade()?
            .into_field(api::Call::PARAMS)
            .upgrade()?
            .into_field(api::Payload::CONTENT)
            .cast::<Struct<api::Exception>>();
        content.upgrade()
    }
    fn try_init(&mut self, hint: Option<capnp::MessageSize>) -> capnp::Result<Self::Builder<'_>> {
        self.try_get(hint)
    }
}
impl<'call, 'buffer> RequestStorage<rpc_capnp::exception::Owned> for Draft<'call, 'buffer> {
    type Sent = RefMut<'call, message::FixedBuilder<'buffer>>;
    fn publish(self) -> Self::Sent {
        self.0
    }
}
fn exercise() {
    let mut input_words = [ZERO; 32];
    let mut input = message::FixedBuilder::new(&mut input_words).unwrap();
    let root = input.try_init_root::<api::Message>().unwrap();
    let mut call = root.into_field(api::Message::CALL).init().unwrap();
    call.scalar(api::Call::QUESTION_ID).set(37);
    call.into_field(api::Call::PARAMS)
        .init()
        .unwrap()
        .into_field(api::Payload::CONTENT)
        .cast::<Struct<api::Exception>>()
        .init()
        .unwrap();
    let provider = Provider(RefCell::new(input));
    let client = Client::from_owner(&provider);
    let mut request = client
        .try_new_call::<rpc_capnp::exception::Owned, rpc_capnp::exception::Owned>(
            42,
            3,
            None,
            CallHints::default(),
        )
        .unwrap();
    request
        .try_get()
        .unwrap()
        .field(api::Exception::REASON)
        .copy_from("kept")
        .unwrap();
    let published = request.send();
    let root = published
        .get_root_as_reader::<rpc_capnp::message::Reader<'_>>()
        .unwrap();
    let rpc_capnp::message::Call(call) = root.which().unwrap() else {
        panic!("Call lost")
    };
    let call = call.unwrap();
    assert_eq!(call.get_question_id(), 37);
    assert_eq!(call.get_interface_id(), 42);
    assert_eq!(call.get_method_id(), 3);
    let reader = Input(message::Reader::new(&*published, Default::default()));
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
