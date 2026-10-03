//! Script an authenticated peer through quiche's real encrypted streams. These
//! tests check the local receipt boundary, not the peer's honesty about delivery.
use super::{
    tests::{pair_identities, pair_profile, pump},
    *,
};
use crate::native_shutdown::DriverGuard;
use futures::FutureExt;
use std::time::Duration;

struct PacketCase {
    a: quiche::Connection,
    b: quiche::Connection,
    driver: ShutdownDriver,
    frame: [u8; FRAME_BYTES],
    failed: bool,
    confirm_close: bool,
}
impl PacketCase {
    fn new(crossed: bool, valid: bool, reply_fits: bool, psk: bool) -> Self {
        let pair = pair_profile(
            128,
            if reply_fits { 128 } else { 1 },
            psk.then_some([7; 32]),
        );
        Self::from_pair(pair, crossed, valid)
    }
    fn from_pair(
        (mut a, mut b): (quiche::Connection, quiche::Connection),
        crossed: bool,
        valid: bool,
    ) -> Self {
        let control = Control::new();
        control.begin(Duration::from_secs(60)).unwrap();
        let mut driver = ShutdownDriver::new(control, false);
        // Open the production RPC stream and consume its native preface. Input
        // delivery below uses actual decrypted stream bytes, with a scripted
        // consumer in place of the runtime's bounded duplex bridge.
        let mut payload = [b'p'; 42];
        payload[0] = b'R';
        assert_eq!(a.stream_send(0, &payload, false).unwrap(), payload.len());
        driver.step(&mut a, 41, true).unwrap();
        pump(&mut a, &mut b, &mut false);
        let mut received = [0; 42];
        assert_eq!(b.stream_recv(0, &mut received).unwrap(), (42, false));
        assert_eq!(received, payload);
        let mut frame = Frame {
            kind: if crossed { 3 } else { 2 },
            ..driver.protocol.sent.unwrap()
        }
        .encode();
        if !valid {
            frame[5] ^= 1;
        }
        Self {
            a,
            b,
            driver,
            frame,
            failed: false,
            confirm_close: true,
        }
    }
    fn step(&mut self) {
        if let Err(error) = self.driver.step(&mut self.a, 41, true) {
            self.failed = true;
            self.driver.control.finish(Err(error));
        }
    }
    fn outcome(&self) -> u64 {
        match self.driver.control.wait().now_or_never() {
            None => 0,
            Some(Ok(receipt)) => {
                assert_eq!(receipt.bytes, 41);
                1
            }
            Some(Err(_)) => 2,
        }
    }
    fn close(&mut self, app: bool, code: u64) {
        let reason = if self.confirm_close {
            self.driver
                .ack_frame
                .map(|f| f.encode().to_vec())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        self.b.close(app, code, &reason).unwrap();
        pump(&mut self.b, &mut self.a, &mut false);
        assert!(self.a.is_draining());
        self.driver.finish_on_close(&self.a);
        // This is the guard used by the outer driver on every exit, including
        // closes that could not establish a valid receipt.
        drop(DriverGuard(self.driver.control.clone()));
    }
    fn apply(&mut self, event: u64) {
        match event {
            1 | 2 => {
                let (bytes, fin) = if event == 1 {
                    (&self.frame[..20], false)
                } else {
                    (&self.frame[20..], true)
                };
                assert_eq!(self.b.stream_send(7, bytes, fin).unwrap(), bytes.len());
                pump(&mut self.b, &mut self.a, &mut false);
                self.step();
            }
            3 => {
                let request = Frame {
                    kind: 1,
                    nonce: [9; 16],
                    bytes: 2,
                }
                .encode();
                assert_eq!(self.b.stream_send(3, &request, true).unwrap(), FRAME_BYTES);
                pump(&mut self.b, &mut self.a, &mut false);
                self.step();
            }
            4 => {
                assert_eq!(self.b.stream_send(0, b"x", false).unwrap(), 1);
                pump(&mut self.b, &mut self.a, &mut false);
                let mut byte = [0];
                assert_eq!(self.a.stream_recv(0, &mut byte).unwrap(), (1, false));
                assert_eq!(byte, *b"x");
                self.driver.delivered(1).unwrap();
                self.step();
            }
            5 => self.close(true, ACKNOWLEDGED_CLOSE),
            6 => self.close(true, 42),
            7 => {
                self.failed = true;
                self.driver.control.finish(Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "earlier failure",
                )));
            }
            _ => panic!("unexpected packet event {event}"),
        }
    }
}

#[test]
fn encrypted_close_requires_complete_crossed_fences_and_graceful_application_code() {
    for psk in [false, true] {
        for crossed in [false, true] {
            for reply_fits in [false, true] {
                for (app, code) in [
                    (true, ACKNOWLEDGED_CLOSE),
                    (true, 42),
                    (false, ACKNOWLEDGED_CLOSE),
                ] {
                    let mut case = PacketCase::new(crossed, true, reply_fits, psk);
                    case.apply(1);
                    assert!(!case.driver.protocol.acknowledged);
                    case.apply(2);
                    if crossed {
                        case.apply(3);
                        case.apply(4);
                        case.apply(4);
                        assert!(case.driver.protocol.ack_sent);
                        assert_eq!(case.driver.ack_written, reply_fits);
                        // No outbound reply packets are delivered to the peer;
                        // the fixture deliberately tests only local validation.
                        assert!(!case.driver.ack_delivered);
                    }
                    assert_eq!(case.outcome(), 0);
                    case.close(app, code);
                    let success = app && code == ACKNOWLEDGED_CLOSE && (!crossed || reply_fits);
                    assert_eq!(
                        case.outcome(),
                        if success { 1 } else { 2 },
                        "psk={psk} crossed={crossed} credit={reply_fits} app={app} code={code}"
                    );
                }
            }
        }
    }
}

#[test]
fn close_code_requires_an_exact_authenticated_receipt_echo() {
    for fault in 0..5 {
        let mut case = PacketCase::new(true, true, true, true);
        for event in [1, 2, 3, 4, 4] {
            case.apply(event);
        }
        let mut reason = case.driver.ack_frame.unwrap().encode().to_vec();
        match fault {
            0 => reason.clear(),
            1 => reason[5] ^= 1,
            2 => reason[21] ^= 1,
            3 => {
                reason.pop();
            }
            4 => reason.push(0),
            _ => unreachable!(),
        }
        case.b.close(true, ACKNOWLEDGED_CLOSE, &reason).unwrap();
        pump(&mut case.b, &mut case.a, &mut false);
        case.driver.finish_on_close(&case.a);
        drop(DriverGuard(case.driver.control.clone()));
        assert_eq!(case.outcome(), 2, "fault={fault}");
    }
}

#[test]
fn reset_reciprocal_stream_without_confirmation_cannot_be_replaced_by_close_code() {
    let mut case = PacketCase::new(true, true, true, true);
    for event in [1, 2, 3, 4, 4] {
        case.apply(event);
    }
    assert!(case.driver.ack_written && !case.driver.ack_delivered);
    case.a
        .stream_shutdown(6, quiche::Shutdown::Write, 42)
        .unwrap();
    case.confirm_close = false;
    case.close(true, ACKNOWLEDGED_CLOSE);
    assert_eq!(case.outcome(), 2);
}

#[test]
fn malformed_or_reset_authenticated_receipts_cannot_complete() {
    for psk in [false, true] {
        for fault in 0..9 {
            let mut case = PacketCase::new(false, true, true, psk);
            let mut frame = case.frame.to_vec();
            let mut stream = 7;
            match fault {
                0 => frame[5] ^= 1,  // nonce
                1 => frame[21] ^= 1, // exact byte count
                2 => frame[4] = 1,   // request on the receipt stream
                3 => frame[0] = 0,   // version/magic
                4 => {
                    frame.pop();
                }
                5 => frame.push(0),
                6 => stream = 3, // receipt on the request stream
                7 | 8 => (),     // incomplete FIN or reset after prefix
                _ => unreachable!(),
            }
            let fin = fault < 7;
            assert_eq!(
                case.b.stream_send(stream, &frame, fin).unwrap(),
                frame.len()
            );
            pump(&mut case.b, &mut case.a, &mut false);
            case.step();
            assert!(!case.driver.protocol.acknowledged, "fault={fault}");
            if fault == 8 {
                case.b
                    .stream_shutdown(stream, quiche::Shutdown::Write, 42)
                    .unwrap();
                pump(&mut case.b, &mut case.a, &mut false);
                case.step();
            }
            assert_eq!(case.failed, fault != 7, "fault={fault}");
            case.close(true, ACKNOWLEDGED_CLOSE);
            assert_eq!(case.outcome(), 2, "fault={fault}");
        }
    }
}

#[test]
fn damaged_and_previous_session_ciphertext_cannot_supply_a_receipt() {
    for psk in [false, true] {
        let a = crate::transport::Identity::generate();
        let b = crate::transport::Identity::generate();
        let new_case = || {
            PacketCase::from_pair(
                pair_identities(128, 128, psk.then_some([7; 32]), &a, &b),
                false,
                true,
            )
        };
        let mut old = new_case();
        old.b.stream_send(7, &old.frame, true).unwrap();
        let mut saved = Vec::new();
        let mut buffer = [0; 1350];
        while let Ok((n, info)) = old.b.send(&mut buffer) {
            saved.push((buffer[..n].to_vec(), info));
        }
        assert!(!saved.is_empty());
        for (packet, info) in &saved {
            let mut damaged = packet.clone();
            *damaged.last_mut().unwrap() ^= 1;
            let accepted = old.a.stats().recv;
            old.a
                .recv(
                    &mut damaged,
                    quiche::RecvInfo {
                        from: info.from,
                        to: info.to,
                    },
                )
                .unwrap();
            // recv() consumes datagrams containing silently discarded packets.
            // The authenticated packet counter and stream state are the oracle.
            assert_eq!(old.a.stats().recv, accepted);
        }
        old.step();
        assert!(!old.driver.protocol.acknowledged && !old.failed);
        for (packet, info) in &saved {
            old.a
                .recv(
                    &mut packet.clone(),
                    quiche::RecvInfo {
                        from: info.from,
                        to: info.to,
                    },
                )
                .unwrap();
        }
        old.step();
        assert!(old.driver.acknowledged());

        // Same pinned identities, addresses, CIDs and PSK, but a fresh Native
        // handshake and fence nonce. Neither packet replay nor its duplicate
        // may authenticate into the replacement connection.
        let mut fresh = new_case();
        for _ in 0..2 {
            for (packet, info) in &saved {
                let accepted = fresh.a.stats().recv;
                fresh
                    .a
                    .recv(
                        &mut packet.clone(),
                        quiche::RecvInfo {
                            from: info.from,
                            to: info.to,
                        },
                    )
                    .unwrap();
                assert_eq!(fresh.a.stats().recv, accepted);
            }
            fresh.step();
            assert!(!fresh.driver.protocol.acknowledged && !fresh.failed);
            assert_eq!(fresh.outcome(), 0);
        }
        fresh.apply(1);
        fresh.apply(2);
        fresh.close(true, ACKNOWLEDGED_CLOSE);
        assert_eq!(fresh.outcome(), 1);
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn outer_transport_driver_preserves_close_validation_and_deadline_errors() {
    use crate::transport::{drive, scheduling, PacketSocket, SessionDrivers};
    for expired in [false, true] {
        for complete in [false, true] {
            for graceful in [false, true] {
                let mut case = PacketCase::new(true, true, true, true);
                case.apply(1);
                case.apply(2);
                if complete {
                    case.apply(3);
                    case.apply(4);
                    case.apply(4);
                }
                case.b
                    .close(
                        true,
                        if graceful { ACKNOWLEDGED_CLOSE } else { 42 },
                        &case
                            .driver
                            .ack_frame
                            .map(|f| f.encode().to_vec())
                            .unwrap_or_default(),
                    )
                    .unwrap();
                pump(&mut case.b, &mut case.a, &mut false);
                let control = case.driver.control.clone();
                assert_eq!(case.outcome(), 0);
                if expired {
                    tokio::time::advance(Duration::from_secs(61)).await;
                }
                let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let (_app, io) = tokio::io::duplex(64);
                let (_scheduling, scheduling) = scheduling::pair();
                // The production outer driver receives a real authenticated
                // connection already carrying peer close. This tests its close
                // branch and guard without inventing an AuthenticatedSession.
                let result = drive(
                    PacketSocket::Dedicated(socket.into()),
                    Box::new(case.a),
                    io,
                    SessionDrivers {
                        established: None,
                        datagrams: None,
                        shutdown: Some(case.driver),
                        mobility: None,
                        scheduling,
                    },
                )
                .await;
                let receipt = control.wait().await;
                if expired {
                    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
                    assert_eq!(receipt.unwrap_err().kind(), io::ErrorKind::TimedOut);
                } else {
                    result.unwrap();
                    if complete && graceful {
                        assert_eq!(receipt.unwrap().bytes, 41);
                    } else {
                        assert_eq!(
                            receipt.unwrap_err().kind(),
                            io::ErrorKind::ConnectionAborted
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn replay_tlc_encrypted_shutdown_packet_fences() {
    use capntproto_test_support::verification::exploration;
    const MODEL: &str = "verification/NativeShutdownPacketFence.tla";
    const CONFIG: &str = include_str!("../../../verification/NativeShutdownPacketFence.cfg");
    let mut total = 0;
    for (crossed, valid, reply_fits, reply_intact) in [
        (false, true, true, true),
        (true, true, true, true),
        (true, true, false, true),
        (true, false, true, true),
        (true, true, true, false),
    ] {
        let config = CONFIG
            .replace(
                "Crossed = TRUE",
                &format!("Crossed = {}", if crossed { "TRUE" } else { "FALSE" }),
            )
            .replace(
                "Valid = TRUE",
                &format!("Valid = {}", if valid { "TRUE" } else { "FALSE" }),
            )
            .replace(
                "ReplyFits = TRUE",
                &format!("ReplyFits = {}", if reply_fits { "TRUE" } else { "FALSE" }),
            )
            .replace(
                "ReplyIntact = TRUE",
                &format!(
                    "ReplyIntact = {}",
                    if reply_intact { "TRUE" } else { "FALSE" }
                ),
            );
        let report =
            format!("native-shutdown-packet-fence/{crossed}-{valid}-{reply_fits}-{reply_intact}");
        let paths = exploration::traces(MODEL, &report, &config).unwrap();
        for psk in [false, true] {
            for path in &paths {
                let mut case = PacketCase::new(crossed, valid, reply_fits, psk);
                for state in path {
                    if !reply_intact && matches!(state["event"], 5 | 6) && case.driver.ack_written {
                        case.a
                            .stream_shutdown(6, quiche::Shutdown::Write, 42)
                            .unwrap();
                        case.confirm_close = false;
                    }
                    case.apply(state["event"]);
                    let protocol = &case.driver.protocol;
                    for (field, actual) in [
                        ("receipt", u64::from(protocol.acknowledged)),
                        ("peer", u64::from(protocol.peer.is_some())),
                        ("data", protocol.delivered),
                        ("reply", u64::from(case.driver.ack_written)),
                        ("failed", u64::from(case.failed)),
                        ("outcome", case.outcome()),
                    ] {
                        assert_eq!(actual, state[field], "{field}: psk={psk} {report} {path:?}");
                    }
                    let fragment = match case.driver.receive_offset[1] {
                        0 => 0,
                        20 => 1,
                        FRAME_BYTES => 2,
                        n => panic!("unexpected receipt offset {n}"),
                    };
                    assert_eq!(fragment, state["fragment"], "{path:?}");
                    assert_eq!(case.a.is_draining(), state["closing"] != 0, "{path:?}");
                    // Packet duplication in pump must not re-apply a FIN or
                    // count input twice. No reciprocal reply packet is sent.
                    assert!(!case.driver.ack_delivered);
                }
                total += 1;
            }
        }
        let faults: &[(&str, &str)] = if !reply_intact {
            &[("reset", "ResetFence")]
        } else if !valid {
            &[("binding", "Binding")]
        } else if !reply_fits {
            &[("reply", "ReplyFence")]
        } else if crossed {
            &[
                ("skipFin", "CompleteFrame"),
                ("crossed", "CrossedFence"),
                ("anyClose", "GracefulClose"),
                ("resurrect", "Terminal"),
            ]
        } else {
            &[]
        };
        let live = if valid && (!crossed || (reply_fits && reply_intact)) {
            config.replace("SPECIFICATION Spec", "SPECIFICATION SuccessSpec")
                + "\nPROPERTY Success\n"
        } else {
            config.replace("SPECIFICATION Spec", "SPECIFICATION LiveSpec") + "\nPROPERTY Progress\n"
        };
        exploration::controls(MODEL, &report, &config, faults, Some(&live)).unwrap();
    }
    eprintln!("{total} native encrypted shutdown packet edge-prefix replays");
}
