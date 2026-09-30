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

type Flight = Vec<(Vec<u8>, SendInfo)>;

// The model schedules individual protected packets, including packets that
// quiche coalesces into one UDP datagram. Preserve their bytes and addresses.
fn split_flight(flight: Flight, cid_len: usize) -> Flight {
    let mut packets = Vec::new();
    for (mut datagram, info) in flight {
        let mut offset = 0;
        while offset < datagram.len() {
            let mut b = octets::OctetsMut::with_slice(&mut datagram[offset..]);
            let header = Header::from_bytes(&mut b, cid_len).unwrap();
            let len = if header.ty == Type::Short {
                b.off() + b.cap()
            } else {
                let payload = b.get_varint().unwrap() as usize;
                b.off() + payload
            };
            packets.push((datagram[offset..offset + len].to_vec(), info));
            offset += len;
        }
    }
    packets
}

struct Confirmation {
    pipe: test_utils::Pipe,
    initial: Flight,
    reply: Flight,
    confirmation: Flight,
    received: u64,
}

impl Confirmation {
    fn new() -> Self {
        Self {
            pipe: test_utils::Pipe::new("cubic").unwrap(),
            initial: Vec::new(),
            reply: Vec::new(),
            confirmation: Vec::new(),
            received: 0,
        }
    }

    fn step(&mut self, event: u64) {
        match event {
            1 => {
                self.initial =
                    test_utils::emit_flight(&mut self.pipe.client).unwrap();
                test_utils::process_flight(
                    &mut self.pipe.server,
                    self.initial.clone(),
                )
                .unwrap();
                self.reply =
                    test_utils::emit_flight(&mut self.pipe.server).unwrap();
            },
            2 => test_utils::process_flight(
                &mut self.pipe.client,
                self.reply.clone(),
            )
            .unwrap(),
            3 | 12 => {
                if event == 12 {
                    assert_eq!(
                        self.pipe.client.stream_send(0, b"hello", true),
                        Ok(5)
                    );
                }
                self.confirmation = split_flight(
                    test_utils::emit_flight(&mut self.pipe.client).unwrap(),
                    self.pipe.server.source_id().len(),
                );
                assert!(!self.confirmation.is_empty());
            },
            4 =>
                for (mut packet, info) in self.confirmation.clone() {
                    *packet.last_mut().unwrap() ^= 0x80;
                    let result = self.pipe.server.recv(&mut packet, RecvInfo {
                        from: info.from,
                        to: info.to,
                    });
                    assert!(result.is_ok() || result == Err(Error::Done));
                },
            5..=7 =>
                for (mut packet, info) in self.confirmation.clone() {
                    let from = if event == 6 {
                        "127.0.0.1:1235".parse().unwrap()
                    } else {
                        info.from
                    };
                    let result = self
                        .pipe
                        .server
                        .recv(&mut packet, RecvInfo { from, to: info.to });
                    assert!(result.is_ok() || result == Err(Error::Done));
                },
            // The responder flight remains withheld from the initiator.
            8 => {},
            9 =>
                for (mut packet, info) in self.initial.clone() {
                    let result = self.pipe.server.recv(&mut packet, RecvInfo {
                        from: info.from,
                        to: info.to,
                    });
                    assert!(result.is_ok() || result == Err(Error::Done));
                },
            10 | 11 => {
                let packets = split_flight(
                    self.reply.clone(),
                    self.pipe.client.source_id().len(),
                );
                let mut delivered = 0;
                for (mut packet, info) in packets {
                    let ty = Header::from_slice(
                        &mut packet,
                        self.pipe.client.source_id().len(),
                    )
                    .unwrap()
                    .ty;
                    if (event == 10 && ty == Type::Initial) ||
                        (event == 11 && ty == Type::Short)
                    {
                        test_utils::process_flight(&mut self.pipe.client, vec![
                            (packet, info),
                        ])
                        .unwrap();
                        delivered += 1;
                    }
                }
                assert!(delivered > 0);
            },
            _ => panic!("unknown confirmation event: {event}"),
        }
        // Observe application delivery independently of packet/key state.
        // Replayed packets must not make the same stream payload readable again.
        let mut payload = [0; 16];
        for id in self.pipe.server.readable() {
            assert_eq!(id, 0);
            assert_eq!(
                self.pipe.server.stream_recv(id, &mut payload),
                Ok((5, true))
            );
            assert_eq!(&payload[..5], b"hello");
            self.received += 1;
        }
    }

    fn observation(&self) -> [u64; 8] {
        let verified = |peer| {
            self.pipe
                .server
                .paths
                .path_id_from_addrs(&(test_utils::Pipe::server_addr(), peer))
                .is_some_and(|id| {
                    self.pipe
                        .server
                        .paths
                        .get(id)
                        .unwrap()
                        .verified_peer_address
                })
        };
        [
            self.pipe.client.is_established() as u64,
            self.pipe.server.is_established() as u64,
            self.pipe.client.crypto_ctx[packet::Epoch::Initial].has_keys() as u64,
            self.pipe.server.crypto_ctx[packet::Epoch::Initial].has_keys() as u64,
            verified(test_utils::Pipe::client_addr()) as u64,
            verified("127.0.0.1:1235".parse().unwrap()) as u64,
            self.pipe.client.handshake_confirmed as u64,
            self.received,
        ]
    }
}

#[test]
fn handshake_confirmation_trace_replay() {
    // Fixed regressions also run without Java. The root Cargo gate supplies
    // every fresh TLC edge-prefix trace through this same real-packet driver.
    let fixed = concat!(
        "1,0,1,1,1,0,0,0,0;8,0,1,1,1,0,0,0,0;2,1,1,1,1,0,0,1,0;",
        "3,1,1,0,1,0,0,1,0;4,1,1,0,1,0,0,1,0;5,1,1,0,0,1,0,1,0;",
        "7,1,1,0,0,1,0,1,0;9,1,1,0,0,1,0,1,0\n",
        "1,0,1,1,1,0,0,0,0;2,1,1,1,1,0,0,1,0;3,1,1,0,1,0,0,1,0;",
        "6,1,1,0,0,0,0,1,0;7,1,1,0,0,0,0,1,0;9,1,1,0,0,0,0,1,0\n",
        "1,0,1,1,1,0,0,0,0;10,1,1,1,1,0,0,0,0;12,1,1,0,1,0,0,0,0;",
        "4,1,1,0,1,0,0,0,0;5,1,1,0,0,1,0,0,1;7,1,1,0,0,1,0,0,1;",
        "11,1,1,0,0,1,0,1,1;9,1,1,0,0,1,0,1,1\n",
    );
    let traces = match std::env::var_os("REPROTO_NOISE_CONFIRMATION_TRACES") {
        Some(path) => std::fs::read_to_string(path).unwrap(),
        None => fixed.to_owned(),
    };
    let mut count = 0;
    for trace in traces.lines() {
        let mut case = Confirmation::new();
        assert_eq!(case.observation(), [0, 0, 1, 0, 0, 0, 0, 0]);
        for step in trace.split(';') {
            let values: Vec<u64> = step
                .split(',')
                .map(|value| value.parse().unwrap())
                .collect();
            assert_eq!(values.len(), 9);
            case.step(values[0]);
            assert_eq!(
                case.observation().as_slice(),
                &values[1..],
                "trace {count}, event {}, {trace}",
                values[0]
            );
        }
        count += 1;
    }
    assert!(count > 0);
    println!("Noise confirmation replay: {count} traces");
}

#[test]
fn tls_configuration_and_early_application_data_are_rejected() {
    let mut config = Config::new(PROTOCOL_VERSION).unwrap();
    assert_eq!(
        config.set_application_protos(&[b"h3"]),
        Err(Error::InvalidState)
    );
    assert_eq!(
        config.load_cert_chain_from_pem_file("examples/cert.crt"),
        Err(Error::InvalidState)
    );
    assert_eq!(
        config.load_priv_key_from_pem_file("examples/cert.key"),
        Err(Error::InvalidState)
    );
    config.enable_early_data();
    config.verify_peer(false);
    assert!(matches!(
        connect(
            None,
            &ConnectionId::from_ref(&[1; 16]),
            test_utils::Pipe::client_addr(),
            test_utils::Pipe::server_addr(),
            &mut config
        ),
        Err(Error::InvalidState)
    ));

    let mut config = test_utils::Pipe::default_config("cubic").unwrap();
    config.enable_early_data();
    let mut pipe = test_utils::Pipe::with_config(&mut config).unwrap();
    assert!(pipe.client.set_session(b"TLS session").is_err());
    assert!(!pipe.client.is_in_early_data());
    assert_eq!(
        pipe.client.stream_send(0, b"early", false),
        Err(Error::StreamLimit)
    );
    assert_eq!(pipe.handshake(), Ok(()));
    assert_eq!(pipe.client.stream_send(0, b"ready", true), Ok(5));
}
