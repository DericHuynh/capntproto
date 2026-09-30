#![no_main]
libfuzzer_sys::fuzz_target!(|input: &[u8]| {
    reproto_noise_fuzz::schema::check(input);
});
