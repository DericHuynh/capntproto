fn main() {
    // Initialize immutable fixture caches before AFL's persistent forkserver.
    capntproto_native_fuzz::framing::check(&[]);
    afl::fuzz!(|data: &[u8]| {
        capntproto_native_fuzz::framing::check(data);
    });
}
