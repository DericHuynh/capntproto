//! CPU work in maintained serialization code; no sockets or timing claims.
use capnp::message::{Builder, HeapAllocator, ReaderOptions};
use gungraun::{library_benchmark, library_benchmark_group, main};
use std::hint::black_box;

fn message(bytes: usize) -> Builder<HeapAllocator> {
    let mut message = Builder::new_default();
    message
        .initn_root::<capnp::data::Builder<'_>>(bytes as u32)
        .fill(0x5a);
    message
}

fn encoded(bytes: usize) -> Vec<u8> {
    capnp::serialize::write_message_to_words(&message(bytes))
}

#[library_benchmark]
#[bench::small(encoded(64))]
#[bench::medium(encoded(1024))]
#[bench::large(encoded(65536))]
fn decode(bytes: Vec<u8>) -> usize {
    let mut input = bytes.as_slice();
    let reader =
        capnp::serialize::read_message_from_flat_slice_no_alloc(&mut input, ReaderOptions::new())
            .unwrap();
    let data = reader.get_root::<capnp::data::Reader<'_>>().unwrap();
    assert!(data.iter().all(|&byte| byte == 0x5a));
    black_box(data.len())
}

fn encoder(bytes: usize) -> (Builder<HeapAllocator>, Vec<u8>) {
    (message(bytes), Vec::with_capacity(bytes + 1024))
}

#[library_benchmark]
#[bench::small(encoder(64))]
#[bench::medium(encoder(1024))]
#[bench::large(encoder(65536))]
fn encode((message, mut output): (Builder<HeapAllocator>, Vec<u8>)) -> Vec<u8> {
    capnp::serialize::write_message(&mut output, &message).unwrap();
    black_box(output)
}

library_benchmark_group!(name = serialization; benchmarks = decode, encode);
main!(library_benchmark_groups = serialization);
