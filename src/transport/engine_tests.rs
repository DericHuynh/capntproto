use super::{config, engine::Engine, scheduling, Identity};
use std::time::Duration;
use tokio::time::Instant;

pub(super) fn pair() -> (Engine, Engine) {
    pair_at(
        [
            "127.0.0.1:1234".parse().unwrap(),
            "127.0.0.1:4321".parse().unwrap(),
        ],
        |_| {},
    )
}
pub(super) fn pair_at(
    addresses: [std::net::SocketAddr; 2],
    mut configure: impl FnMut(&mut quiche::Config),
) -> (Engine, Engine) {
    let a = Identity::generate();
    let b = Identity::generate();
    let mut ac = config(&a, b.public_key(), None, b"engine progress test").unwrap();
    let mut bc = config(&b, a.public_key(), None, b"engine progress test").unwrap();
    for config in [&mut ac, &mut bc] {
        config.set_initial_max_stream_data_bidi_local(42);
        config.set_initial_max_stream_data_bidi_remote(42);
        configure(config);
    }
    let [aa, ba] = addresses;
    let a = quiche::connect(
        None,
        &quiche::ConnectionId::from_ref(&[1; 16]),
        aa,
        ba,
        &mut ac,
    )
    .unwrap();
    let b = quiche::accept(
        &quiche::ConnectionId::from_ref(&[2; 16]),
        None,
        ba,
        aa,
        &mut bc,
    )
    .unwrap();
    (
        Engine::new(Box::new(a), true, None, scheduling::pair().1),
        Engine::new(Box::new(b), true, None, scheduling::pair().1),
    )
}
pub(super) fn packets(from: &mut Engine, to: &mut Engine) {
    let mut packet = [0; 1350];
    loop {
        match from.conn.send(&mut packet) {
            Ok((n, info)) => {
                match to.conn.recv(
                    &mut packet[..n],
                    quiche::RecvInfo {
                        from: info.from,
                        to: info.to,
                    },
                ) {
                    Ok(_) | Err(quiche::Error::Done) => (),
                    result => panic!("packet receive: {result:?}"),
                }
            }
            Err(quiche::Error::Done) => break,
            result => panic!("packet send: {result:?}"),
        }
    }
}

#[test]
fn native_engine_preserves_bidirectional_bytes_preface_and_fin_under_backpressure() {
    let (mut a, mut b) = pair();
    let inputs: [Vec<u8>; 2] = [
        (0..511).map(|n| (n % 251) as u8).collect(),
        (0..337).map(|n| (250 - n % 251) as u8).collect(),
    ];
    let mut output = [Vec::new(), Vec::new()];
    let mut ended = [false; 2];
    // Queue application bytes before authentication. The responder must still
    // wait for the initiator's native-stream preface before emitting RPC bytes.
    for (engine, input) in [(&mut a, &inputs[0]), (&mut b, &inputs[1])] {
        engine.tx.read_buffer().unwrap()[..input.len()].copy_from_slice(input);
        engine.tx.read(input.len()).unwrap();
    }
    let now = Instant::now();
    let mut partial = false;
    for _ in 0..1000 {
        for (i, engine) in [&mut a, &mut b].into_iter().enumerate() {
            let before = engine.tx.written();
            assert!(engine.step(now).unwrap());
            partial |= engine.tx.written() > before
                && engine
                    .tx
                    .pending(false)
                    .is_some_and(|(bytes, _)| !bytes.is_empty());
            if engine.tx.can_read() {
                engine.tx.read(0).unwrap();
            }
            let bytes = engine.rx.pending();
            if !bytes.is_empty() {
                let count = bytes.len().min(3);
                output[i].extend_from_slice(&bytes[..count]);
                engine.delivered(count).unwrap();
            }
            if engine.rx.needs_shutdown() {
                engine.rx.closed().unwrap();
                ended[i] = true;
            }
        }
        packets(&mut a, &mut b);
        packets(&mut b, &mut a);
        if ended == [true, true] {
            break;
        }
    }
    assert_eq!(ended, [true, true]);
    assert!(a.ready() && b.ready() && partial);
    assert_eq!(output[0], inputs[1]);
    assert_eq!(output[1], inputs[0]);
    assert_eq!(a.tx.written(), inputs[0].len() as u64);
    assert_eq!(b.tx.written(), inputs[1].len() as u64);
}

#[test]
fn queued_datagram_after_shutdown_request_is_discarded_without_failing_rpc() {
    let (mut a, mut b) = pair();
    let now = Instant::now();
    for _ in 0..50 {
        a.step(now).unwrap();
        b.step(now).unwrap();
        packets(&mut a, &mut b);
        packets(&mut b, &mut a);
        if a.ready() && b.ready() {
            break;
        }
    }
    assert!(a.can_accept_datagram());
    let control = crate::native_shutdown::Control::new();
    a.shutdown = Some(super::shutdown::ShutdownDriver::new(control.clone(), false));
    control.begin(Duration::from_secs(1)).unwrap();
    a.datagram(b"late queued packet".to_vec()).unwrap();
    assert!(a.datagram_deadline(now).is_none());
}

#[test]
fn replay_tlc_receipt_survives_close_packet_burst_yield() {
    use futures::FutureExt;
    use reproto_test_support::verification::exploration;
    let config = include_str!("../../verification/NativeCloseFlush.cfg");
    exploration::controls(
        "verification/NativeCloseFlush.tla",
        "native-close-flush",
        config,
        &[
            ("forgetReceipt", "ReceiptRetained"),
            ("earlyCompletion", "CompletionSound"),
        ],
        None,
    )
    .unwrap();
    let traces = exploration::traces(
        "verification/NativeCloseFlush.tla",
        "native-close-flush",
        config,
    )
    .unwrap();
    for trace in traces {
        let (mut a, mut b) = pair();
        let now = Instant::now();
        for _ in 0..50 {
            a.step(now).unwrap();
            b.step(now).unwrap();
            packets(&mut a, &mut b);
            packets(&mut b, &mut a);
            if a.ready() && b.ready() {
                break;
            }
        }
        assert!(a.ready() && b.ready());
        let control = crate::native_shutdown::Control::new();
        a.shutdown = Some(super::shutdown::ShutdownDriver::new(control.clone(), false));
        b.shutdown = Some(super::shutdown::ShutdownDriver::new(
            crate::native_shutdown::Control::new(),
            true,
        ));
        for state in trace {
            match state["event"] {
                1 => {
                    control.begin(Duration::from_secs(1)).unwrap();
                    a.tx.read(0).unwrap();
                    for _ in 0..50 {
                        a.step(now).unwrap();
                        // Keep the locally generated CLOSE queued for event 2.
                        if a.conn.local_error().is_some() {
                            break;
                        }
                        packets(&mut a, &mut b);
                        b.step(now).unwrap();
                        packets(&mut b, &mut a);
                    }
                    assert_eq!(
                        a.conn.local_error().unwrap().error_code,
                        super::shutdown::ACKNOWLEDGED_CLOSE
                    );
                }
                2 => {
                    let mut packet = [0; 1350];
                    let (n, info) = a.conn.send(&mut packet).unwrap();
                    b.conn
                        .recv(
                            &mut packet[..n],
                            quiche::RecvInfo {
                                from: info.from,
                                to: info.to,
                            },
                        )
                        .unwrap();
                    assert!(a.conn.is_draining());
                }
                3 => assert!(
                    a.step(now).unwrap(),
                    "burst yield must retain the validated receipt"
                ),
                4 => {
                    assert!(matches!(
                        a.conn.send(&mut [0; 1350]),
                        Err(quiche::Error::Done)
                    ));
                    assert!(a.packets_drained());
                }
                other => panic!("unknown close event {other}"),
            }
            if state["done"] == 1 {
                assert_eq!(control.wait().now_or_never().unwrap().unwrap().bytes, 0);
            } else {
                assert!(control.wait().now_or_never().is_none());
            }
        }
    }
}
