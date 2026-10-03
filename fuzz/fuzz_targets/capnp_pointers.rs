#![no_main]
libfuzzer_sys::fuzz_target!(|input: &[u8]| {
    capntproto_native_fuzz::pointers::check(input);
});
