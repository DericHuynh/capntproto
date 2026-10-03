// AFL++ 4.40c's runtime exports a weak zero-valued flag. cargo-afl's
// stable sancov mode does not run the C/C++ IJON compiler pass, so provide
// its documented runtime flag here. The C runtime owns this mutable symbol;
// Rust never reads or writes it. CI checks the forkserver's IJON handshake.
#[unsafe(export_name = "__afl_ijon_enabled")]
static mut AFL_IJON_ENABLED: u32 = 1;

fn main() {
    capntproto_native_fuzz::rpc_lifecycle::check(&[]);
    afl::fuzz!(|data: &[u8]| {
        // Each input creates and tears down its own RPC machine. No mutable
        // state or IJON context survives between persistent iterations.
        capntproto_native_fuzz::rpc_lifecycle::run_with_feedback(data, |progress| {
            // Bounded semantic state, rather than attacker-controlled IDs or
            // input length, rewards reachable capability/pending-call states.
            afl::ijon_set!(progress.state);
            afl::ijon_max_at!(0x4350_0001, progress.completed as u64);
            afl::ijon_max_at!(0x4350_0002, progress.effects as u64);
        });
    });
}
