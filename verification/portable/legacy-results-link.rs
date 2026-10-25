//! Fallible legacy result owners editing the payload of a real Return frame.
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
use capnp::{capability::Results, field_api::Struct, message};
use capntproto_rpc_wire::rpc_capnp::{self, api};
const ZERO: capnp::Word = capnp::word(0, 0, 0, 0, 0, 0, 0, 0);
fn exercise() {
    let mut words = [ZERO; 24];
    let mut builder = message::FixedBuilder::new(&mut words).unwrap();
    {
        let root = builder.try_init_root::<api::Message>().unwrap();
        let mut returned = root.into_field(api::Message::RETURN).init().unwrap();
        returned.scalar(api::Return::ANSWER_ID).set(73);
        returned.scalar(api::Return::RELEASE_PARAM_CAPS).set(false);
        let payload = returned.into_field(api::Return::RESULTS).init().unwrap();
        let content = payload
            .into_field(api::Payload::CONTENT)
            .cast::<Struct<api::Exception>>();
        let mut results = Results::<rpc_capnp::exception::Owned, _>::from_owner(content);
        results
            .try_get()
            .unwrap()
            .field(api::Exception::REASON)
            .copy_from("kept")
            .unwrap();
        assert!(results.try_get().unwrap().field(api::Exception::REASON).copy_from("This string deliberately exceeds the remaining fixed response capacity by a substantial margin. No fallback allocator may replace the original payload.").is_err());
    }
    let root = builder
        .get_root_as_reader::<rpc_capnp::message::Reader<'_>>()
        .unwrap();
    let rpc_capnp::message::Return(returned) = root.which().unwrap() else {
        panic!("lost Return envelope")
    };
    let returned = returned.unwrap();
    assert_eq!(returned.get_answer_id(), 73);
    assert!(!returned.get_release_param_caps());
    let rpc_capnp::return_::Results(payload) = returned.which().unwrap() else {
        panic!("lost results")
    };
    let content = payload
        .unwrap()
        .get_content()
        .get_as::<rpc_capnp::exception::Reader<'_>>()
        .unwrap();
    assert_eq!(content.get_reason().unwrap(), "kept");
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
