fn main() {
    // Initialize immutable fixture caches before AFL's persistent forkserver.
    capntproto_native_fuzz::schema::check(&[1]);
    afl::fuzz!(|data: &[u8]| {
        capntproto_native_fuzz::schema::check(data);
    });
}
