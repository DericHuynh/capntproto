//! Architecture probes against unmodified quiche, not a production stream map.
//! Hold encrypted packets to distinguish stream isolation from shared credits.
use super::{config, Identity};

struct Packet {
    bytes: Vec<u8>,
    info: quiche::RecvInfo,
}
fn packets(connection: &mut quiche::Connection) -> Vec<Packet> {
    let mut result = Vec::new();
    let mut bytes = [0; 1350];
    loop {
        match connection.send(&mut bytes) {
            Ok((length, info)) => result.push(Packet {
                bytes: bytes[..length].to_vec(),
                info: quiche::RecvInfo {
                    from: info.from,
                    to: info.to,
                },
            }),
            Err(quiche::Error::Done) => return result,
            Err(error) => panic!("packet generation failed: {error}"),
        }
    }
}
fn deliver(connection: &mut quiche::Connection, packets: Vec<Packet>) {
    for mut packet in packets {
        connection.recv(&mut packet.bytes, packet.info).unwrap();
    }
}
fn pair(connection_credit: u64, stream_credit: u64) -> (quiche::Connection, quiche::Connection) {
    let client = Identity::generate();
    let server = Identity::generate();
    let mut ac = config(&client, server.public_key(), None, b"stream plane probe").unwrap();
    let mut bc = config(&server, client.public_key(), None, b"stream plane probe").unwrap();
    for config in [&mut ac, &mut bc] {
        config.set_initial_max_data(connection_credit);
        config.set_initial_max_stream_data_bidi_local(stream_credit);
        config.set_initial_max_stream_data_bidi_remote(stream_credit);
        config.discover_pmtu(false);
    }
    let aa = "127.0.0.1:1234".parse().unwrap();
    let ba = "127.0.0.1:4321".parse().unwrap();
    let mut a = quiche::connect(
        None,
        &quiche::ConnectionId::from_ref(&[1; 16]),
        aa,
        ba,
        &mut ac,
    )
    .unwrap();
    let mut b = quiche::accept(
        &quiche::ConnectionId::from_ref(&[2; 16]),
        None,
        ba,
        aa,
        &mut bc,
    )
    .unwrap();
    for _ in 0..20 {
        deliver(&mut b, packets(&mut a));
        deliver(&mut a, packets(&mut b));
    }
    assert!(a.is_established() && b.is_established());
    (a, b)
}

#[test]
fn split_stream_control_survives_a_bulk_packet_gap() {
    for split in [false, true] {
        let (mut a, mut b) = pair(4096, 4096);
        let bulk_stream = if split { 4 } else { 0 };
        assert_eq!(a.stream_send(bulk_stream, &[7; 128], false), Ok(128));
        let held = packets(&mut a);
        assert!(!held.is_empty());
        assert_eq!(a.stream_send(0, b"control", false), Ok(7));
        deliver(&mut b, packets(&mut a));
        let mut bytes = [0; 256];
        if split {
            assert_eq!(b.stream_recv(0, &mut bytes), Ok((7, false)));
            assert_eq!(&bytes[..7], b"control");
            assert!(!b.stream_readable(bulk_stream));
        } else {
            assert_eq!(b.stream_recv(0, &mut bytes), Err(quiche::Error::Done));
        }
        // Deliver the original held ciphertext; no timeout, sleep or latency
        // assumption is necessary to demonstrate independent stream progress.
        deliver(&mut b, held);
        if split {
            assert_eq!(b.stream_recv(4, &mut bytes), Ok((128, false)));
            assert_eq!(&bytes[..128], &[7; 128]);
        } else {
            assert_eq!(b.stream_recv(0, &mut bytes), Ok((135, false)));
            assert_eq!(&bytes[..128], &[7; 128]);
            assert_eq!(&bytes[128..135], b"control");
        }
    }
}

#[test]
fn stream_credit_isolation_requires_connection_credit_headroom() {
    for connection_credit in [256, 512] {
        let (mut a, mut b) = pair(connection_credit, 256);
        a.stream_priority(0, 0, false).unwrap();
        a.stream_priority(4, 200, true).unwrap();
        assert_eq!(a.stream_send(4, &[7; 256], false), Ok(256));
        deliver(&mut b, packets(&mut a));
        // Leave the bulk receiver unread, consuming its entire stream window.
        assert_eq!(a.stream_send(4, b"more", false), Err(quiche::Error::Done));
        let control = a.stream_send(0, b"control", false);
        if connection_credit == 256 {
            assert_eq!(control, Err(quiche::Error::Done));
            // Giving control the highest urgency cannot restore exhausted
            // connection credit. Consuming bulk bytes permits a credit update.
            assert_eq!(b.stream_recv(4, &mut [0; 256]), Ok((256, false)));
            deliver(&mut a, packets(&mut b));
            assert_eq!(a.stream_send(0, b"control", false), Ok(7));
        } else {
            assert_eq!(control, Ok(7));
        }
        deliver(&mut b, packets(&mut a));
        let mut control = [0; 7];
        assert_eq!(b.stream_recv(0, &mut control), Ok((7, false)));
        assert_eq!(&control, b"control");
    }
}
