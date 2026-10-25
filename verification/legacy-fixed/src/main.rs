//! Link-only entry; not board startup code or a complete fixed RPC connection.
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#[cfg(not(target_os = "none"))]
fn main() {
    capntproto_legacy_fixed_check::retained_generated_invocation().unwrap();
    capntproto_legacy_fixed_check::cancel_generated_invocation().unwrap();
    capntproto_legacy_fixed_check::contextual_generated_invocation().unwrap();
    capntproto_legacy_fixed_check::reject_before_application().unwrap();
    capntproto_legacy_fixed_check::concurrent_generated_invocations().unwrap();
}
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    capntproto_legacy_fixed_check::retained_generated_invocation().unwrap();
    capntproto_legacy_fixed_check::cancel_generated_invocation().unwrap();
    capntproto_legacy_fixed_check::contextual_generated_invocation().unwrap();
    capntproto_legacy_fixed_check::reject_before_application().unwrap();
    capntproto_legacy_fixed_check::concurrent_generated_invocations().unwrap();
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
