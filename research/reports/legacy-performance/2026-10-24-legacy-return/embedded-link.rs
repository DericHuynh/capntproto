//! Link-only Return ownership probe. No global allocator or hosted runtime.
#![no_std]
#![no_main]
use capntproto_rpc_engine::completion::return_guard::ReturnOwnership;
use core::cell::Cell;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let guards = Cell::new(0);
    let claimed = Cell::new(false);
    let ownership = ReturnOwnership::new(
        core::hint::black_box(&guards),
        core::hint::black_box(&claimed),
    );
    ownership.retain();
    ownership.retain();
    assert!(!ownership.release());
    assert!(!claimed.get());
    assert!(ownership.release());
    assert!(claimed.get());
    // A reentrant context cannot claim publication again.
    ownership.retain();
    assert!(!ownership.release());
    assert_eq!(guards.get(), 0);
    // Caller storage reuse starts only after all prior guards have ended.
    claimed.set(core::hint::black_box(false));
    ownership.retain();
    ownership.mark_sent();
    ownership.retain();
    assert!(!ownership.release());
    assert!(!ownership.release());
    assert_eq!(guards.get(), 0);
    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
