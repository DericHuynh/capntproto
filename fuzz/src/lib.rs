//! Serialization, RPC and Noise fuzz harnesses shared with native Cargo tests.
//! Cryptography and OS entropy remain enabled; no quiche `fuzzing` feature.
#![forbid(unsafe_code)]

pub mod framing;
pub mod pointers;
pub mod rpc_lifecycle;
pub mod schema;
mod wire;

pub mod lifecycle_capnp {
    include!(concat!(env!("OUT_DIR"), "/lifecycle_capnp.rs"));
}

use quiche::{Connection, ConnectionId, Error, RecvInfo};
use snow::{
    params::DHChoice,
    resolvers::{CryptoResolver, DefaultResolver},
};
use std::net::SocketAddr;

const MARKER: &[u8] = b"authenticated Noise fuzz payload";
pub const MAX_INPUT: usize = 4096;

struct Packet {
    bytes: Vec<u8>,
    info: RecvInfo,
}

struct Pair {
    peers: [Connection; 2],
}

impl Pair {
    fn new(flags: u8) -> Self {
        let addresses: [SocketAddr; 2] = [
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:4321".parse().unwrap(),
        ];
        let peers = [false, true].map(|server| {
            let index = usize::from(server);
            let private = if server { [0x22; 32] } else { [0x11; 32] };
            let remote = if server { [0x11; 32] } else { [0x22; 32] };
            let mut dh = DefaultResolver.resolve_dh(&DHChoice::Curve25519).unwrap();
            dh.set(&remote);
            let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();
            config
                .set_noise_identity(
                    private,
                    dh.pubkey().try_into().unwrap(),
                    (flags & 1 != 0).then_some([7; 32]),
                    b"noise-fuzz/v1",
                )
                .unwrap();
            config.set_application_protos(&[b"reproto/1"]).unwrap();
            config
                .set_cc_algorithm_name(if flags & 8 == 0 {
                    "cubic"
                } else {
                    "bbr2_gcongestion"
                })
                .unwrap();
            config.set_initial_max_data(32768);
            config.set_initial_max_stream_data_bidi_local(16384);
            config.set_initial_max_stream_data_bidi_remote(16384);
            config.set_initial_max_streams_bidi(2);
            config.enable_dgram(true, 4, 4);
            let cid = ConnectionId::from_vec(vec![index as u8 + 1; 16]);
            if server {
                quiche::accept(
                    &cid,
                    None,
                    addresses[index],
                    addresses[1 - index],
                    &mut config,
                )
            } else {
                quiche::connect(
                    None,
                    &cid,
                    addresses[index],
                    addresses[1 - index],
                    &mut config,
                )
            }
            .unwrap()
        });
        Self { peers }
    }

    fn packets(&mut self, sender: usize) -> Vec<Packet> {
        let mut packets = Vec::new();
        let mut out = [0; 1350];
        for _ in 0..64 {
            match self.peers[sender].send(&mut out) {
                Ok((n, info)) => packets.push(Packet {
                    bytes: out[..n].to_vec(),
                    info: RecvInfo {
                        from: info.from,
                        to: info.to,
                    },
                }),
                Err(Error::Done) => return packets,
                other => panic!("valid peer failed to send: {other:?}"),
            }
        }
        panic!("unbounded send loop");
    }

    fn deliver(&mut self, sender: usize, mut packet: Packet) {
        let result = self.peers[1 - sender].recv(&mut packet.bytes, packet.info);
        assert!(
            result.is_ok() || result == Err(Error::Done),
            "valid packet: {result:?}"
        );
    }

    fn settle(&mut self) {
        for _ in 0..32 {
            let mut count = 0;
            for sender in 0..2 {
                let packets = self.packets(sender);
                count += packets.len();
                for packet in packets {
                    self.deliver(sender, packet);
                }
            }
            if count == 0 {
                return;
            }
        }
        panic!("lossless exchange did not settle");
    }

    fn establish(&mut self) {
        self.settle();
        assert!(self
            .peers
            .iter()
            .all(|p| p.is_established() && !p.is_closed() && !p.is_in_early_data()));
        // Confirm the server and open stream zero for a possible server reply.
        self.peers[0].stream_send(0, b"", false).unwrap();
    }

    fn open_for_reply(&mut self) {
        self.peers[0].stream_send(0, b"?", true).unwrap();
        self.settle();
        assert_eq!(self.peers[1].stream_recv(0, &mut [0; 1]), Ok((1, true)));
    }
}

fn read(
    peer: &mut Connection,
    received: &mut Vec<u8>,
    finished: &mut bool,
    expected: &[u8],
    capacity: usize,
) {
    let mut out = [0; MAX_INPUT];
    for id in peer.readable() {
        assert_eq!(id, 0);
        loop {
            let (n, fin) = match peer.stream_recv(id, &mut out[..capacity]) {
                Ok(value) => value,
                Err(Error::Done) => break,
                other => panic!("readable stream: {other:?}"),
            };
            assert!(n > 0 || fin, "read made no progress");
            assert!(!*finished, "duplicate stream delivery after FIN");
            received.extend_from_slice(&out[..n]);
            assert!(
                expected.starts_with(received),
                "stream contents changed or repeated"
            );
            if fin {
                assert_eq!(received, expected);
            }
            *finished = fin;
            if fin {
                break;
            }
        }
    }
}

/// Mutate one real datagram (or replace it with raw input) before/after IK.
/// Invalid packets may fail or close a connection; a successful application read
/// must still equal the sole authentic message sent by the peer.
pub fn packet(input: &[u8]) {
    let input = &input[..input.len().min(MAX_INPUT)];
    let flags = input.first().copied().unwrap_or(0);
    let sender = usize::from(flags & 2 != 0);
    let established = flags & 4 != 0;
    let mut pair = Pair::new(flags);
    if established {
        pair.establish();
        if sender == 1 {
            pair.open_for_reply();
        }
        pair.peers[sender].stream_send(0, MARKER, true).unwrap();
    } else if sender == 1 {
        for packet in pair.packets(0) {
            pair.deliver(0, packet);
        }
    }
    let mut packets = pair.packets(sender);
    assert!(!packets.is_empty());
    let selector = input.get(2).copied().unwrap_or(0) as usize;
    let mut packet = packets.swap_remove(selector % packets.len());
    let body = input.get(3..).unwrap_or_default();
    match input.get(1).copied().unwrap_or(0) % 4 {
        0 => packet.bytes = body.to_vec(),
        1 => {
            for (i, byte) in body.iter().enumerate() {
                let index = (selector + i) % packet.bytes.len();
                packet.bytes[index] ^= byte;
            }
        }
        2 => packet.bytes.truncate(selector.min(packet.bytes.len())),
        3 => packet.bytes.extend_from_slice(body),
        _ => unreachable!(),
    }
    let mut received = Vec::new();
    let mut finished = false;
    // Replay the same candidate too. Errors are normal fuzz outcomes; stream
    // observations remain checked even if recv reports a later packet error.
    for _ in 0..2 {
        let _ = pair.peers[1 - sender].recv(&mut packet.bytes.clone(), packet.info);
        read(
            &mut pair.peers[1 - sender],
            &mut received,
            &mut finished,
            if established { MARKER } else { b"" },
            MAX_INPUT,
        );
    }
}

/// Exercise real authenticated streams with variable payload/read boundaries,
/// reordered datagrams, duplicates and bad-tag copies. Original packets remain
/// available, so recovery needs no wall-clock timeout or sleeping.
pub fn stream(input: &[u8]) {
    stream_with_stats(input);
}

fn stream_with_stats(input: &[u8]) -> usize {
    let input = &input[..input.len().min(MAX_INPUT)];
    let flags = input.first().copied().unwrap_or(0);
    let sender = usize::from(flags & 2 != 0);
    let chunk = 16 + input.get(1).copied().unwrap_or(0) as usize;
    let read_capacity = 1 + usize::from(flags);
    let payload = input.get(2..).unwrap_or_default();
    let mut pair = Pair::new(flags);
    pair.establish();
    if sender == 1 {
        pair.open_for_reply();
    }
    let mut sent = 0;
    let mut received = Vec::new();
    let mut finished = false;
    let mut reversed_batches = 0;
    for turn in 0..300 {
        if sent < payload.len() || turn == 0 {
            // A reversed batch must contain multiple datagrams to test ordering.
            // Queue the bounded payload before emitting it in this mode.
            let writes = if flags & 16 != 0 { 256 } else { 1 };
            for _ in 0..writes {
                let end = (sent + chunk).min(payload.len());
                let n = pair.peers[sender]
                    .stream_send(0, &payload[sent..end], end == payload.len())
                    .unwrap();
                assert_eq!(n, end - sent);
                sent = end;
                if sent == payload.len() {
                    break;
                }
            }
        }
        let mut packets = pair.packets(sender);
        if flags & 16 != 0 {
            reversed_batches += usize::from(packets.len() > 1);
            packets.reverse();
        }
        for packet in packets {
            if flags & 32 != 0 {
                let mut forged = packet.bytes.clone();
                *forged.last_mut().unwrap() ^= 0x80;
                let _ = pair.peers[1 - sender].recv(&mut forged, packet.info);
                read(
                    &mut pair.peers[1 - sender],
                    &mut received,
                    &mut finished,
                    payload,
                    read_capacity,
                );
            }
            if flags & 64 != 0 {
                let mut duplicate = packet.bytes.clone();
                let result = pair.peers[1 - sender].recv(&mut duplicate, packet.info);
                assert!(result.is_ok() || result == Err(Error::Done));
                read(
                    &mut pair.peers[1 - sender],
                    &mut received,
                    &mut finished,
                    payload,
                    read_capacity,
                );
            }
            pair.deliver(sender, packet);
            read(
                &mut pair.peers[1 - sender],
                &mut received,
                &mut finished,
                payload,
                read_capacity,
            );
        }
        for packet in pair.packets(1 - sender) {
            pair.deliver(1 - sender, packet);
        }
        if finished {
            break;
        }
    }
    assert!(finished, "stream did not complete within bound");
    assert_eq!(received, payload);
    assert!(pair
        .peers
        .iter()
        .all(|p| p.is_established() && !p.is_closed()));
    reversed_batches
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(target: &str, index: usize, input: &[u8]) -> usize {
        if let Some(directory) = std::env::var_os("REPROTO_NOISE_FUZZ_CORPUS") {
            let directory = std::path::PathBuf::from(directory).join(target);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join(format!("seed-{index}")), input).unwrap();
        }
        match target {
            "noise_packet" => {
                packet(input);
                0
            }
            "noise_stream" => stream_with_stats(input),
            "capnp_framing" => {
                framing::check(input);
                0
            }
            "capnp_pointers" => {
                pointers::check(input);
                0
            }
            "capnp_schema" => {
                schema::check(input);
                0
            }
            "rpc_lifecycle" => {
                rpc_lifecycle::check(input);
                0
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn packet_stage_and_mutation_matrix() {
        for flags in 0..16 {
            for mode in 0..4 {
                let index = usize::from(flags) * 8 + usize::from(mode) * 2;
                seed("noise_packet", index, &[flags, mode, 0, 0xff, 0, 0x80]);
                seed("noise_packet", index + 1, &[flags, mode, 255]);
            }
        }
        for (index, (flags, mode)) in [(0, 0), (15, 1), (4, 3)].into_iter().enumerate() {
            let mut input = vec![0xff; MAX_INPUT];
            input[..3].copy_from_slice(&[flags, mode, 255]);
            seed("noise_packet", 128 + index, &input);
        }
    }

    #[test]
    fn stream_fragment_and_fault_matrix() {
        for flags in 0..128 {
            seed("noise_stream", usize::from(flags) * 2, &[flags]);
            let mut input = vec![flags, 3];
            input.extend((0..1024).map(|n| (n % 251) as u8));
            seed("noise_stream", usize::from(flags) * 2 + 1, &input);
        }
        for (index, flags) in [0, 127].into_iter().enumerate() {
            let mut input = vec![0x5a; MAX_INPUT];
            input[..2].copy_from_slice(&[flags, 0]);
            let reversed = seed("noise_stream", 256 + index, &input);
            if flags & 16 != 0 {
                assert!(reversed > 0, "reordering scenario was not exercised");
            }
        }
    }

    #[test]
    fn forged_tag_cannot_deliver_and_original_can() {
        for flags in 0..16 {
            let mut pair = Pair::new(flags);
            pair.establish();
            pair.peers[0].stream_send(0, MARKER, true).unwrap();
            let packets = pair.packets(0);
            assert!(!packets.is_empty());
            for packet in &packets {
                assert_eq!(packet.bytes[0] & 0x80, 0, "expected a Short packet");
                let mut bad = packet.bytes.clone();
                *bad.last_mut().unwrap() ^= 0x80;
                let _ = pair.peers[1].recv(&mut bad, packet.info);
            }
            assert_eq!(pair.peers[1].readable().count(), 0);
            for packet in packets {
                pair.deliver(0, packet);
            }
            let mut received = Vec::new();
            let mut finished = false;
            read(
                &mut pair.peers[1],
                &mut received,
                &mut finished,
                MARKER,
                MAX_INPUT,
            );
            assert!(finished);
        }
    }

    #[test]
    fn wire_seed_matrix() {
        for (target, inputs) in [
            ("capnp_framing", framing::seeds()),
            ("capnp_pointers", pointers::seeds()),
            ("capnp_schema", schema::seeds()),
            ("rpc_lifecycle", rpc_lifecycle::seeds()),
        ] {
            for (index, input) in inputs.iter().enumerate() {
                assert!(
                    std::panic::catch_unwind(|| seed(target, index, input)).is_ok(),
                    "{target} seed {index}: {input:?}"
                );
            }
            eprintln!("{target}: {} seeds", inputs.len());
        }
    }

    #[test]
    fn replay_saved_fuzz_input() {
        let Some(path) = std::env::var_os("REPROTO_NOISE_FUZZ_REPLAY") else {
            packet(&[5, 1, 0]);
            stream(&[127, 0, 1, 2, 3]);
            return;
        };
        let input = std::fs::read(path).unwrap();
        match std::env::var("REPROTO_NOISE_FUZZ_TARGET").unwrap().as_str() {
            "noise_packet" => packet(&input),
            "noise_stream" => stream(&input),
            "capnp_framing" => framing::check(&input),
            "capnp_pointers" => pointers::check(&input),
            "capnp_schema" => schema::check(&input),
            "rpc_lifecycle" => rpc_lifecycle::check(&input),
            target => panic!("unknown target {target}"),
        }
    }
}
