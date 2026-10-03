#![no_main]
libfuzzer_sys::fuzz_target!(|input: &[u8]| {
    reproto_native_fuzz::schema::check(input);
});
