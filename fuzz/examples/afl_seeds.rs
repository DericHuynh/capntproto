//! Deterministic, bounded AFL startup corpus from the shared regression seeds.
use capntproto_native_fuzz::{framing, pointers, rpc_lifecycle, schema};
use std::{fs, path::PathBuf};
fn main() {
    let output = PathBuf::from(std::env::args_os().nth(1).expect("corpus directory"));
    for (name, inputs) in [
        ("capnp_framing", framing::seeds()),
        ("capnp_pointers", pointers::seeds()),
        ("capnp_schema", schema::seeds()),
        ("rpc_lifecycle", rpc_lifecycle::seeds()),
    ] {
        let directory = output.join(name);
        fs::create_dir_all(&directory).unwrap();
        // Spread 64 seeds across each matrix. The libFuzzer qualification still
        // exercises every regression seed; keep AFL's calibration bounded.
        for i in 0..64.min(inputs.len()) {
            let seed = &inputs[i * inputs.len() / 64.min(inputs.len())];
            if !seed.is_empty() {
                fs::write(directory.join(format!("seed-{i}")), seed).unwrap();
            }
        }
    }
}
