//! Deliberately invalid execution: the outer Cargo gate requires Miri to reject it.
#![cfg(miri)]

#[test]
#[ignore = "intentional undefined behavior; run only through the qualification gate"]
fn dangling_read_is_rejected() {
    let pointer = Box::into_raw(Box::new(7u64));
    // Intentionally violate the lifetime rule to check that the detector runs.
    unsafe {
        drop(Box::from_raw(pointer));
        std::hint::black_box(pointer.read());
    }
}
