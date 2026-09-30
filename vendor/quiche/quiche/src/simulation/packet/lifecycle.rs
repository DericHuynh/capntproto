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

struct Lifecycle {
    machine: Machine,
    schedule: Random,
    old_data: [Option<u64>; 2],
}

fn alternate_validated(machine: &Machine) -> bool {
    machine.peers[0].is_path_validated(alternate_addr(), Pipe::server_addr()) ==
        Ok(true) &&
        machine.peers[1]
            .is_path_validated(Pipe::server_addr(), alternate_addr()) ==
            Ok(true)
}

impl Lifecycle {
    fn new(seed: u64, cc: &str, psk: bool) -> Self {
        let mut this = Self {
            machine: Machine::new(seed, cc, psk),
            schedule: Random(seed ^ 0x6c696665),
            old_data: [None; 2],
        };
        this.machine.report.scenario = "lifecycle".into();
        this.drive(|m| {
            m.peers.iter().all(|p| p.handshake_confirmed) &&
                m.observation().verified
        });
        for server in [false, true] {
            this.machine.step(Action::AdvertiseCid { server });
        }
        this.drive(|_| true);
        assert!(this.machine.peers.iter().all(|p| p.available_dcids() >= 1));
        this
    }

    fn drive(&mut self, done: impl Fn(&Machine) -> bool) {
        for _ in 0..400 {
            for server in [false, true] {
                self.machine.step(Action::Emit { server });
            }
            // Keep real stream ciphertext from each direction, not merely an
            // ACK/probe. It will be delivered again after both keys and stream
            // state have been replaced, with deliberately colliding CIDs.
            if self.machine.generation == [0, 0] {
                for packet in &self.machine.pending {
                    if self.old_data[packet.server as usize].is_some() ||
                        packet.bytes[0] & 0x80 != 0
                    {
                        continue;
                    }
                    let frames = crate::test_utils::decode_pkt(
                        &mut self.machine.peers[!packet.server as usize],
                        &mut packet.bytes.clone(),
                    )
                    .unwrap();
                    if frames
                        .iter()
                        .any(|f| matches!(f, crate::frame::Frame::Stream { .. }))
                    {
                        self.old_data[packet.server as usize] = Some(packet.id);
                    }
                }
            }
            if self.machine.pending.is_empty() {
                if done(&self.machine) {
                    return;
                }
                self.machine.timer();
            } else {
                let index =
                    self.schedule.next() as usize % self.machine.pending.len();
                let packet = self.machine.pending[index].clone();
                self.machine.advance(packet.at);
                self.machine.step(Action::Deliver { id: packet.id });
            }
        }
        panic!("lifecycle exchange did not settle");
    }

    fn start_probe(&mut self) {
        self.machine.step(Action::Probe);
        self.machine.step(Action::Emit { server: false });
        assert!(self
            .machine
            .pending
            .iter()
            .any(|p| p.from == alternate_addr()));
    }

    fn transfer(&mut self) {
        self.machine.step(Action::Write { server: false });
        self.drive(|m| m.finished[1]);
        self.machine.step(Action::Write { server: true });
        self.drive(|m| m.finished == [true; 2]);
        self.machine.verify_complete();
    }

    fn step(&mut self, event: u64) {
        match event {
            1 => self.start_probe(),
            2 => {
                self.machine.step(Action::Partition {
                    alternate: true,
                    blocked: true,
                });
                self.machine.deliver_pending(false);
                // Service real timers until a second challenge is sent into
                // the partition. A timeout alone must not validate a path.
                let mut retried = false;
                for _ in 0..16 {
                    self.machine.timer();
                    self.machine.step(Action::Emit { server: false });
                    if self
                        .machine
                        .pending
                        .iter()
                        .any(|p| p.from == alternate_addr())
                    {
                        self.machine.deliver_pending(false);
                        retried = true;
                        break;
                    }
                }
                assert!(retried);
                assert!(!alternate_validated(&self.machine));
                assert_eq!(
                    self.machine.peers[0]
                        .paths
                        .get_active()
                        .unwrap()
                        .local_addr(),
                    Pipe::client_addr()
                );
            },
            3 => {
                self.machine.step(Action::Partition {
                    alternate: true,
                    blocked: false,
                });
                self.start_probe();
            },
            4 => {
                self.machine.deliver_pending(false);
                self.machine.step(Action::Emit { server: true });
                assert!(self
                    .machine
                    .pending
                    .iter()
                    .any(|p| p.to == alternate_addr()));
                assert!(!alternate_validated(&self.machine));
            },
            5 => {
                let packets: Vec<_> = self
                    .machine
                    .pending
                    .iter()
                    .filter(|p| p.server && p.to == alternate_addr())
                    .cloned()
                    .collect();
                assert!(!packets.is_empty());
                let before = self.machine.observation();
                for packet in packets {
                    self.machine.advance(packet.at);
                    self.machine.step(Action::Duplicate { id: packet.id });
                    self.machine.step(Action::Corrupt {
                        id: self.machine.next_id - 1,
                    });
                }
                assert_eq!(self.machine.observation().paths, before.paths);
            },
            6 => self.drive(alternate_validated),
            7 => {
                assert!(alternate_validated(&self.machine));
                self.machine.step(Action::Migrate);
                self.machine.step(Action::Partition {
                    alternate: false,
                    blocked: true,
                });
                self.transfer();
                assert_eq!(
                    self.machine.peers[0]
                        .paths
                        .get_active()
                        .unwrap()
                        .local_addr(),
                    alternate_addr()
                );
                assert_eq!(
                    self.machine.peers[1]
                        .paths
                        .get_active()
                        .unwrap()
                        .peer_addr(),
                    alternate_addr()
                );
                assert!(self.old_data.iter().all(Option::is_some));
            },
            8 => {
                self.machine.step(Action::RestartServer);
                assert_eq!(self.machine.generation, [0, 1]);
                assert!(!self.machine.peers[1].is_established());
            },
            9 => self.machine.step(Action::ReconnectClient),
            10 => {
                self.drive(|m| {
                    m.peers.iter().all(|p| p.handshake_confirmed) &&
                        m.observation().verified
                });
                assert_eq!(self.machine.received, [Vec::<u8>::new(), Vec::new()]);
            },
            11 => {
                let before = self.machine.observation();
                for server in [false, true] {
                    let id = self.old_data[server as usize].unwrap();
                    self.machine.step(Action::Replay { id });
                    let packet = self.machine.pending.last().unwrap().clone();
                    // Establish that the test reaches cryptographic rejection:
                    // the stale packet's CID still names the new connection.
                    let mut bytes = packet.bytes.clone();
                    let receiver = &self.machine.peers[!server as usize];
                    let header = crate::Header::from_slice(
                        &mut bytes,
                        receiver.source_id().len(),
                    )
                    .unwrap();
                    assert_eq!(header.dcid, receiver.source_id());
                    self.machine.step(Action::Deliver { id: packet.id });
                }
                let after = self.machine.observation();
                assert_eq!(after.received, before.received);
                assert_eq!(after.finished, before.finished);
                assert_eq!(after.established, [true; 2]);
                assert_eq!(after.closed, [false; 2]);
            },
            12 => self.transfer(),
            _ => panic!("unknown lifecycle event {event}"),
        }
    }
}

#[test]
fn lifecycle_packet_replay() {
    let mut count = 0;
    for cc in ["cubic", "bbr2_gcongestion"] {
        for psk in [false, true] {
            for seed in [0, 1, 42, 0xdeadbeef] {
                let report = {
                    let mut case = Lifecycle::new(seed, cc, psk);
                    case.machine.report.completion_generation = Some(1);
                    for event in [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12] {
                        case.step(event);
                    }
                    case.machine.report.clone()
                };
                replay(&report);
                count += 1;
            }
        }
    }
    println!("Noise lifecycle simulation: {count} schedules replayed");
}

#[test]
fn lifecycle_trace_replay() {
    // Retain one full regression without Java. The root Cargo gate supplies
    // every edge prefix from a fresh TLC exploration, including replay both
    // before and after fresh application data and optional partition/forgery.
    let traces = match std::env::var_os("REPROTO_NOISE_LIFECYCLE_TRACES") {
        Some(path) => std::fs::read_to_string(path).unwrap(),
        None => concat!(
            "1,1,1,0,0,0,0,0,0,0,0;",
            "2,1,1,0,0,0,0,0,0,1,0;",
            "3,1,1,0,0,0,0,0,0,0,0;",
            "4,1,1,0,0,0,0,0,0,0,0;",
            "5,1,1,0,0,0,0,0,0,0,0;",
            "6,1,1,0,0,1,0,0,0,0,0;",
            "7,1,1,0,0,1,1,1,1,0,1;",
            "8,1,0,0,1,0,1,1,0,0,1;",
            "9,0,0,1,1,0,1,0,0,0,1;",
            "10,1,1,1,1,0,1,0,0,0,1;",
            "11,1,1,1,1,0,1,0,0,0,1;",
            "12,1,1,1,1,0,1,1,1,0,1\n",
        )
        .into(),
    };
    let mut count = 0;
    for cc in ["cubic", "bbr2_gcongestion"] {
        for psk in [false, true] {
            for trace in traces.lines() {
                let mut case = Lifecycle::new(42, cc, psk);
                for step in trace.split(';') {
                    let values: Vec<u64> =
                        step.split(',').map(|v| v.parse().unwrap()).collect();
                    assert_eq!(values.len(), 11);
                    case.step(values[0]);
                    let m = &case.machine;
                    assert_eq!(
                        &[
                            m.peers[0].is_established() as u64,
                            m.peers[1].is_established() as u64,
                            m.generation[0] as u64,
                            m.generation[1] as u64,
                            (m.generation == [0, 0] && alternate_validated(m))
                                as u64,
                            (m.peers[0].paths.get_active().unwrap().local_addr() ==
                                alternate_addr())
                                as u64,
                            m.finished[0] as u64,
                            m.finished[1] as u64,
                            m.blocked[1] as u64,
                            m.blocked[0] as u64,
                        ],
                        &values[1..],
                        "lifecycle trace {trace}, event {}",
                        values[0]
                    );
                }
                count += 1;
            }
        }
    }
    println!("Noise lifecycle replay: {count} traces");
}
