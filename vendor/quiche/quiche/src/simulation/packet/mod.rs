// Copyright (C) 2026, ReProto contributors.
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are
// met:
//
//     * Redistributions of source code must retain the above copyright notice,
//       this list of conditions and the following disclaimer.
//
//     * Redistributions in binary form must reproduce the above copyright
//       notice, this list of conditions and the following disclaimer in the
//       documentation and/or other materials provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS
// IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO,
// THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
// PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR
// CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
// EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
// PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
// PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
// LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
// NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
// SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use super::*;
use crate::test_utils::Pipe;
use crate::Connection;
use crate::ConnectionId;
use crate::Error;
use crate::RecoveryOps;
use crate::RecvInfo;
use serde::Deserialize;
use serde::Serialize;
use std::net::SocketAddr;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
enum Action {
    Emit { server: bool },
    Advance { nanos: u64 },
    Timeout { server: bool },
    Write { server: bool },
    Deliver { id: u64 },
    Drop { id: u64 },
    Duplicate { id: u64 },
    Corrupt { id: u64 },
    AdvertiseCid { server: bool },
    Probe,
    Migrate,
    Partition { alternate: bool, blocked: bool },
    RestartServer,
    ReconnectClient,
    Replay { id: u64 },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct Packet {
    id: u64,
    server: bool,
    at: u64,
    bytes: Vec<u8>,
    from: SocketAddr,
    to: SocketAddr,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
enum Effect {
    Packet(Packet),
    PartitionDrop {
        id: u64,
    },
    Read {
        server: bool,
        bytes: Vec<u8>,
        fin: bool,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct Observation {
    established: [bool; 2],
    confirmed: [bool; 2],
    verified: bool,
    received: [usize; 2],
    finished: [bool; 2],
    closed: [bool; 2],
    timeout: [Option<u64>; 2],
    cwnd: [usize; 2],
    lost: [usize; 2],
    generation: [u8; 2],
    paths: [Vec<PathObservation>; 2],
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct PathObservation {
    local: SocketAddr,
    peer: SocketAddr,
    active: bool,
    validated: bool,
    verified: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct Record {
    input: Action,
    effects: Vec<Effect>,
    observation: Option<Observation>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct Report {
    format: u32,
    seed: u64,
    cc: String,
    psk: bool,
    completion_generation: Option<u8>,
    scenario: String,
    records: Vec<Record>,
}

pub(super) struct Machine {
    peers: [Connection; 2],
    pending: Vec<Packet>,
    next_id: u64,
    received: [Vec<u8>; 2],
    finished: [bool; 2],
    generation: [u8; 2],
    blocked: [bool; 2],
    report: Report,
    persist_report: bool,
    // Drop the connections before restoring the real clock and entropy.
    scope: Scope,
}

impl Machine {
    fn new(seed: u64, cc: &str, psk: bool) -> Self {
        let scope = Scope::new(seed);
        let peers = [false, true].map(|server| {
            Self::new_peer(cc, psk, server, Pipe::client_addr(), None)
        });
        Self {
            peers,
            pending: Vec::new(),
            next_id: 0,
            received: [Vec::new(), Vec::new()],
            finished: [false; 2],
            generation: [0; 2],
            blocked: [false; 2],
            report: Report {
                format: 3,
                seed,
                cc: cc.into(),
                psk,
                completion_generation: None,
                scenario: "packet-loss".into(),
                records: Vec::new(),
            },
            persist_report: true,
            scope,
        }
    }

    fn new_peer(
        cc: &str, psk: bool, server: bool, client_addr: SocketAddr,
        cid: Option<Vec<u8>>,
    ) -> Connection {
        use snow::params::DHChoice;
        use snow::resolvers::CryptoResolver;
        use snow::resolvers::DefaultResolver;

        let mut config = Pipe::default_config(cc).unwrap();
        config.set_initial_max_data(131072);
        config.set_initial_max_stream_data_bidi_local(65536);
        config.set_initial_max_stream_data_bidi_remote(65536);
        config.set_max_idle_timeout(10000);
        config.set_active_connection_id_limit(3);
        let mut dh = DefaultResolver.resolve_dh(&DHChoice::Curve25519).unwrap();
        let (private, remote) = if server {
            ([0x22; 32], [0x11; 32])
        } else {
            ([0x11; 32], [0x22; 32])
        };
        dh.set(&remote);
        config
            .set_noise_identity(
                private,
                dh.pubkey().try_into().unwrap(),
                psk.then_some([3; 32]),
                b"simulation/v1",
            )
            .unwrap();
        let cid = cid.unwrap_or_else(|| {
            let mut cid = vec![0; 16];
            crate::rand::rand_bytes(&mut cid);
            cid
        });
        let cid = ConnectionId::from_ref(&cid);
        if server {
            crate::accept(
                &cid,
                None,
                Pipe::server_addr(),
                client_addr,
                &mut config,
            )
        } else {
            crate::connect(
                None,
                &cid,
                client_addr,
                Pipe::server_addr(),
                &mut config,
            )
        }
        .unwrap()
    }

    fn observation(&self) -> Observation {
        Observation {
            established: self.peers.each_ref().map(|p| p.is_established()),
            confirmed: self.peers.each_ref().map(|p| p.handshake_confirmed),
            verified: self.peers[1]
                .paths
                .get_active()
                .unwrap()
                .verified_peer_address,
            received: self.received.each_ref().map(Vec::len),
            finished: self.finished,
            closed: self.peers.each_ref().map(|p| p.is_closed()),
            timeout: self
                .peers
                .each_ref()
                .map(|p| p.timeout_instant().map(|t| self.scope.offset(t))),
            cwnd: self
                .peers
                .each_ref()
                .map(|p| p.paths.get_active().unwrap().recovery.cwnd()),
            lost: self.peers.each_ref().map(|p| p.stats().lost),
            generation: self.generation,
            paths: self.peers.each_ref().map(|peer| {
                let mut paths: Vec<_> = peer
                    .paths
                    .iter()
                    .map(|(_, path)| PathObservation {
                        local: path.local_addr(),
                        peer: path.peer_addr(),
                        active: path.active(),
                        validated: peer
                            .is_path_validated(
                                path.local_addr(),
                                path.peer_addr(),
                            )
                            .unwrap(),
                        verified: path.verified_peer_address,
                    })
                    .collect();
                paths.sort_by_key(|path| (path.local, path.peer));
                paths
            }),
        }
    }

    fn packet(
        &mut self, server: bool, at: u64, bytes: Vec<u8>, from: SocketAddr,
        to: SocketAddr,
    ) {
        assert!(self.pending.len() < 256, "packet queue exceeded bound");
        let packet = Packet {
            id: self.next_id,
            server,
            at,
            bytes,
            from,
            to,
        };
        self.next_id += 1;
        self.pending.push(packet.clone());
        self.report
            .records
            .last_mut()
            .unwrap()
            .effects
            .push(Effect::Packet(packet));
    }

    fn step(&mut self, input: Action) {
        assert!(self.report.records.len() < 4096, "event bound exceeded");
        self.report.records.push(Record {
            input: input.clone(),
            effects: Vec::new(),
            observation: None,
        });
        match input {
            Action::Emit { server } => {
                let mut out = [0; 1350];
                for _ in 0..64 {
                    match self.peers[server as usize].send(&mut out) {
                        Ok((n, info)) => {
                            let at = self.scope.offset(info.at) + 1_000_000;
                            self.packet(
                                server,
                                at,
                                out[..n].to_vec(),
                                info.from,
                                info.to,
                            );
                        },
                        Err(Error::Done) => break,
                        error => panic!("send failed: {error:?}"),
                    }
                }
            },
            Action::Advance { nanos } =>
                self.scope.advance_to(Duration::from_nanos(nanos)),
            Action::Timeout { server } => {
                let peer = &mut self.peers[server as usize];
                assert!(peer
                    .timeout_instant()
                    .is_some_and(|t| t <= crate::clock::now()));
                peer.on_timeout();
            },
            Action::Write { server } => {
                let data = payload(server, self.generation[server as usize]);
                assert_eq!(
                    self.peers[server as usize].stream_send(0, &data, true),
                    Ok(data.len())
                );
            },
            Action::Duplicate { id } => {
                let p = self.pending.iter().find(|p| p.id == id).unwrap().clone();
                self.packet(p.server, p.at, p.bytes, p.from, p.to);
            },
            Action::Replay { id } => {
                let packet = self
                    .report
                    .records
                    .iter()
                    .flat_map(|record| &record.effects)
                    .find_map(|effect| match effect {
                        Effect::Packet(packet) if packet.id == id =>
                            Some(packet.clone()),
                        _ => None,
                    })
                    .expect("missing packet history");
                self.packet(
                    packet.server,
                    self.scope.elapsed().as_nanos() as u64,
                    packet.bytes,
                    packet.from,
                    packet.to,
                );
            },
            Action::AdvertiseCid { server } => {
                let (cid, token) =
                    crate::test_utils::create_cid_and_reset_token(16);
                self.peers[server as usize]
                    .new_scid(&cid, token, false)
                    .unwrap();
            },
            Action::Probe => {
                self.peers[0]
                    .probe_path(alternate_addr(), Pipe::server_addr())
                    .unwrap();
            },
            Action::Migrate => {
                self.peers[0]
                    .migrate(alternate_addr(), Pipe::server_addr())
                    .unwrap();
            },
            Action::Partition { alternate, blocked } =>
                self.blocked[alternate as usize] = blocked,
            Action::RestartServer | Action::ReconnectClient => {
                let server = matches!(input, Action::RestartServer);
                let index = server as usize;
                let client_addr =
                    self.peers[0].paths.get_active().unwrap().local_addr();
                // Deliberately reuse the old active CID. Stale ciphertext must
                // fail authentication even if a router sends it to this peer.
                let cid = if server {
                    self.peers[0].destination_id().as_ref().to_vec()
                } else {
                    self.peers[0].source_id().as_ref().to_vec()
                };
                self.peers[index] = Self::new_peer(
                    &self.report.cc,
                    self.report.psk,
                    server,
                    client_addr,
                    Some(cid),
                );
                self.generation[index] += 1;
                self.received[index].clear();
                self.finished[index] = false;
            },
            Action::Deliver { id } |
            Action::Corrupt { id } |
            Action::Drop { id } => {
                let index = self.pending.iter().position(|p| p.id == id).unwrap();
                let mut packet = self.pending.remove(index);
                if !matches!(input, Action::Drop { .. }) {
                    assert!(
                        packet.at <= self.scope.elapsed().as_nanos() as u64,
                        "delivery before pacing/propagation deadline"
                    );
                    if matches!(input, Action::Corrupt { .. }) {
                        *packet.bytes.last_mut().unwrap() ^= 0x80;
                    }
                    let alternate = packet.from == alternate_addr() ||
                        packet.to == alternate_addr();
                    if self.blocked[alternate as usize] {
                        self.report
                            .records
                            .last_mut()
                            .unwrap()
                            .effects
                            .push(Effect::PartitionDrop { id });
                    } else {
                        let (from, to) = (packet.from, packet.to);
                        let result = self.peers[!packet.server as usize]
                            .recv(&mut packet.bytes, RecvInfo { from, to });
                        assert!(
                            result.is_ok() || result == Err(Error::Done),
                            "receive failed: {result:?}"
                        );
                    }
                }
            },
        }
        for server in [false, true] {
            let index = server as usize;
            let mut out = [0; 8192];
            for id in self.peers[index].readable() {
                assert_eq!(id, 0);
                let (n, fin) =
                    self.peers[index].stream_recv(id, &mut out).unwrap();
                assert!(!self.finished[index], "duplicate delivery after FIN");
                self.received[index].extend_from_slice(&out[..n]);
                let expected = payload(!server, self.generation[index]);
                assert!(
                    expected.starts_with(&self.received[index]),
                    "stream bytes reordered or duplicated"
                );
                if fin {
                    assert_eq!(self.received[index], expected);
                }
                self.finished[index] = fin;
                self.report.records.last_mut().unwrap().effects.push(
                    Effect::Read {
                        server,
                        bytes: out[..n].to_vec(),
                        fin,
                    },
                );
            }
        }
        assert!(self.peers.iter().all(|p| p.local_error().is_none()));
        let observation = self.observation();
        self.report.records.last_mut().unwrap().observation = Some(observation);
    }

    fn advance(&mut self, at: u64) {
        self.step(Action::Advance {
            nanos: at.max(self.scope.elapsed().as_nanos() as u64),
        });
    }

    fn verify_complete(&self) {
        assert_eq!(
            self.finished, [true; 2],
            "did not recover within the event bound"
        );
        assert!(self.pending.is_empty());
        assert!(self.scope.elapsed() < Duration::from_secs(10));
    }

    fn timer(&mut self) {
        let (at, server) = [false, true]
            .into_iter()
            .filter_map(|server| {
                self.peers[server as usize]
                    .timeout_instant()
                    .map(|t| (self.scope.offset(t), server))
            })
            .min()
            .expect("stalled without a timer");
        self.advance(at);
        self.step(Action::Timeout { server });
    }

    fn deliver_pending(&mut self, server: bool) {
        let packets: Vec<_> = self
            .pending
            .iter()
            .filter(|p| p.server == server)
            .cloned()
            .collect();
        assert!(!packets.is_empty());
        for packet in packets {
            self.advance(packet.at);
            self.step(Action::Deliver { id: packet.id });
        }
    }

    fn recovery_step(&mut self, event: u64) {
        match event {
            1 => {
                self.deliver_pending(false);
                self.step(Action::Emit { server: true });
            },
            2 | 4 => {
                let server = event == 4;
                let ids: Vec<_> = self
                    .pending
                    .iter()
                    .filter(|p| p.server == server)
                    .map(|p| p.id)
                    .collect();
                assert!(!ids.is_empty());
                for id in ids {
                    self.step(Action::Drop { id });
                }
            },
            3 | 5 => {
                let server = event == 5;
                for _ in 0..16 {
                    self.timer();
                    for server in [false, true] {
                        self.step(Action::Emit { server });
                    }
                    if server && self.pending.iter().any(|p| !p.server) {
                        self.deliver_pending(false);
                        self.step(Action::Emit { server: true });
                    }
                    let mut has_crypto = false;
                    for packet in &self.pending {
                        if packet.server != server ||
                            packet.bytes[0] & 0xf0 != 0xc0
                        {
                            continue;
                        }
                        if !server {
                            has_crypto = true;
                            break;
                        }
                        // A client PTO can elicit an ACK-only Initial. It is
                        // not the lost IK response: require actual CRYPTO.
                        let frames = crate::test_utils::decode_pkt(
                            &mut self.peers[0],
                            &mut packet.bytes.clone(),
                        )
                        .unwrap();
                        has_crypto |= frames.iter().any(|frame| {
                            matches!(frame, crate::frame::Frame::Crypto { .. })
                        });
                    }
                    if has_crypto {
                        return;
                    }
                }
                panic!("PTO did not reproduce the lost Initial flight");
            },
            6 => {
                self.deliver_pending(true);
            },
            7 => {
                // HANDSHAKE_DONE may still need its own PTO after recovering
                // the IK response. The model settles this bounded exchange
                // with no more loss; every timer/packet remains in the log.
                for _ in 0..32 {
                    for server in [false, true] {
                        self.step(Action::Emit { server });
                        if self.pending.iter().any(|p| p.server == server) {
                            self.deliver_pending(server);
                        }
                    }
                    if self.observation().verified &&
                        self.peers[0].handshake_confirmed
                    {
                        return;
                    }
                    self.timer();
                }
                panic!("handshake confirmation did not recover");
            },
            _ => panic!("unknown recovery model event {event}"),
        }
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        if !self.persist_report {
            return;
        }
        let directory = std::env::var_os("REPROTO_NOISE_SIM_REPORT_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::thread::panicking().then(|| {
                    std::env::temp_dir().join("reproto-noise-simulation")
                })
            });
        if let Some(directory) = directory {
            let path = directory.join(format!(
                "{}-{}-{}-{}.json",
                self.report.scenario,
                self.report.seed,
                self.report.cc,
                self.report.psk
            ));
            let result = std::fs::create_dir_all(directory).and_then(|()| {
                std::fs::write(&path, serde_json::to_vec(&self.report).unwrap())
            });
            if let Err(error) = result {
                eprintln!("could not persist simulation: {error}");
            }
            if std::thread::panicking() {
                eprintln!(
                    "replay with REPROTO_NOISE_SIM_REPLAY={}",
                    path.display()
                );
            }
        }
    }
}

fn alternate_addr() -> SocketAddr {
    "127.0.0.1:5678".parse().unwrap()
}

fn payload(server: bool, generation: u8) -> Vec<u8> {
    (0..if server { 1536 } else { 4096 })
        .map(|i| {
            ((i * 31 + usize::from(server) + usize::from(generation) * 17) % 251)
                as u8
        })
        .collect()
}

fn run(seed: u64, cc: &str, psk: bool) -> Report {
    let mut machine = Machine::new(seed, cc, psk);
    machine.report.completion_generation = Some(0);
    let mut schedule = Random(seed ^ 0x7061636b657473);
    let mut dropped_initial = false;
    let mut dropped_reply = false;
    let mut dropped_data = false;
    let mut duplicated = false;
    let mut corrupted = false;
    let mut written = [false; 2];
    for _ in 0..400 {
        if machine.peers.iter().all(|p| p.is_established()) &&
            machine.observation().verified &&
            !written[0]
        {
            machine.step(Action::Write { server: false });
            written[0] = true;
        }
        if machine.finished[1] && !written[1] {
            machine.step(Action::Write { server: true });
            written[1] = true;
        }
        for server in [false, true] {
            machine.step(Action::Emit { server });
        }
        if machine.pending.is_empty() {
            if machine.finished == [true; 2] {
                break;
            }
            machine.timer();
            continue;
        }
        let index = schedule.next() as usize % machine.pending.len();
        let packet = machine.pending[index].clone();
        // Choosing among all queued packets (not just the earliest) creates
        // seeded reordering and delay. The recorded clock includes pacing.
        machine.advance(packet.at);
        let id = packet.id;
        if !packet.server && !dropped_initial {
            machine.step(Action::Drop { id });
            dropped_initial = true;
        } else if packet.server && !dropped_reply {
            machine.step(Action::Drop { id });
            dropped_reply = true;
        } else if !packet.server && written[0] && !dropped_data {
            machine.step(Action::Drop { id });
            dropped_data = true;
        } else {
            if !packet.server && written[0] && !corrupted {
                machine.step(Action::Duplicate { id });
                machine.step(Action::Corrupt {
                    id: machine.next_id - 1,
                });
                corrupted = true;
            }
            if !packet.server && written[0] && !duplicated {
                machine.step(Action::Duplicate { id });
                duplicated = true;
            }
            machine.step(Action::Deliver { id });
        }
    }
    assert!(
        dropped_initial &&
            dropped_reply &&
            dropped_data &&
            duplicated &&
            corrupted
    );
    machine.verify_complete();
    machine.report.clone()
}

fn replay(expected: &Report) {
    replay_with_persistence(expected, true);
}

fn replay_with_persistence(expected: &Report, persist: bool) {
    assert_eq!(expected.format, 3);
    let mut machine = Machine::new(expected.seed, &expected.cc, expected.psk);
    machine.persist_report = persist;
    machine.report.completion_generation = expected.completion_generation;
    machine.report.scenario = expected.scenario.clone();
    for (index, record) in expected.records.iter().enumerate() {
        machine.step(record.input.clone());
        assert_eq!(
            &machine.report.records[index], record,
            "replay diverged at event {index}"
        );
    }
    if let Some(generation) = expected.completion_generation {
        assert_eq!(
            machine.generation, [generation; 2],
            "wrong completion generation"
        );
        machine.verify_complete();
    }
}

#[test]
fn seeded_packet_replay() {
    if let Some(path) = std::env::var_os("REPROTO_NOISE_SIM_REPLAY") {
        let report =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        replay(&report);
        return;
    }
    let mut count = 0;
    for cc in ["cubic", "bbr2_gcongestion"] {
        for psk in [false, true] {
            for seed in [0, 1, 42, 0xdeadbeef] {
                let report = run(seed, cc, psk);
                replay(&report);
                count += 1;
            }
        }
    }
    println!("Noise packet simulation: {count} schedules replayed");
}

#[test]
fn scope_restores_after_unwind_and_is_thread_local() {
    let result = std::panic::catch_unwind(|| {
        let scope = Scope::new(7);
        let before = crate::clock::now();
        scope.advance_to(Duration::from_secs(3));
        assert_eq!(crate::clock::now() - before, Duration::from_secs(3));
        assert!(std::thread::spawn(|| now().is_none()).join().unwrap());
        panic!("scope unwind test");
    });
    assert!(result.is_err());
    assert!(now().is_none());
    assert!(!fill_bytes(&mut [0; 8]));
    let _scope = Scope::new(7);
    let mut first = [0; 16];
    assert!(fill_bytes(&mut first));
    assert_ne!(first, [0; 16]);
}

#[test]
fn handshake_recovery_trace_replay() {
    // Without Java retain both the lossless and two-lost-flight regressions.
    // The root gate supplies every edge prefix from a fresh TLC graph.
    let traces = match std::env::var_os("REPROTO_NOISE_RECOVERY_TRACES") {
        Some(path) => std::fs::read_to_string(path).unwrap(),
        None => concat!(
            "1,0,1,0,1;6,1,1,0,1;7,1,1,1,0\n",
            "2,0,0,0,0;3,0,0,0,0;1,0,1,0,1;4,0,1,0,1;5,0,1,0,1;6,1,1,0,1;7,1,1,1,0\n",
        ).into(),
    };
    let mut count = 0;
    for cc in ["cubic", "bbr2_gcongestion"] {
        for psk in [false, true] {
            for trace in traces.lines() {
                let mut machine = Machine::new(42, cc, psk);
                machine.step(Action::Emit { server: false });
                for step in trace.split(';') {
                    let values: Vec<u64> =
                        step.split(',').map(|v| v.parse().unwrap()).collect();
                    assert_eq!(values.len(), 5);
                    machine.recovery_step(values[0]);
                    assert_eq!(
                        &[
                            machine.peers[0].is_established() as u64,
                            machine.peers[1].is_established() as u64,
                            machine.observation().verified as u64,
                            machine.peers[1].crypto_ctx
                                [crate::packet::Epoch::Initial]
                                .has_keys() as u64,
                        ],
                        &values[1..],
                        "recovery trace {trace}, event {}",
                        values[0]
                    );
                }
                count += 1;
            }
        }
    }
    println!("Noise recovery replay: {count} traces");
}

mod lifecycle;
mod property;
