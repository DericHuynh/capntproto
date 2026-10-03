use super::stream::{ReceiveStream, SendStream};

#[test]
fn replay_tlc_stream_progress() {
    use capntproto_test_support::verification::exploration;
    let config = include_str!("../../verification/RpcStreamProgress.cfg");
    exploration::controls(
        "verification/RpcStreamProgress.tla",
        "rpc-stream-progress",
        config,
        &[
            ("loseWrite", "ByteConservation"),
            ("earlyFin", "FinSound"),
            ("earlyClose", "CloseSound"),
        ],
        None,
    )
    .unwrap();
    let traces = exploration::traces(
        "verification/RpcStreamProgress.tla",
        "rpc-stream-progress",
        config,
    )
    .unwrap();
    let data = [0x32, 0x75];
    for trace in traces {
        let mut tx = SendStream::default();
        let mut rx = ReceiveStream::default();
        let mut graceful = false;
        for state in trace {
            let n = state["amount"] as usize;
            match state["event"] {
                1 => {
                    let end = state["read"] as usize;
                    tx.read_buffer().unwrap()[..n].copy_from_slice(&data[end - n..end]);
                    tx.read(n).unwrap();
                }
                2 => {
                    let (bytes, fin) = tx.pending(graceful).unwrap();
                    assert!(!fin);
                    let end = state["accepted"] as usize;
                    assert_eq!(&bytes[..n], &data[end - n..end]);
                    tx.sent(n).unwrap();
                }
                3 => tx.read(0).unwrap(),
                4 => {
                    assert_eq!(tx.pending(graceful), Some((&[][..], true)));
                    tx.sent(0).unwrap();
                }
                5 => graceful = true,
                6 => {
                    let end = state["received"] as usize;
                    rx.receive_buffer().unwrap()[..n].copy_from_slice(&data[end - n..end]);
                    rx.received(n, state["rxFin"] == 1, 0).unwrap();
                }
                7 => {
                    let end = state["delivered"] as usize;
                    assert_eq!(&rx.pending()[..n], &data[end - n..end]);
                    rx.delivered(n).unwrap();
                }
                8 => rx.closed().unwrap(),
                9 => {
                    let _ = (tx.pending(graceful), rx.pending());
                }
                other => panic!("unknown stream event {other}"),
            }
            assert_eq!(tx.written(), state["sent"]);
            assert_eq!(
                tx.can_read(),
                state["read"] == state["sent"] && state["txEof"] == 0
            );
            assert_eq!(tx.drained(), state["txEof"] == 1);
            assert_eq!(
                tx.pending(graceful).is_some(),
                state["read"] > state["sent"]
                    || (state["txEof"] == 1 && state["fin"] == 0 && !graceful)
            );
            assert_eq!(
                rx.pending().len() as u64,
                state["received"] - state["delivered"]
            );
            assert_eq!(
                rx.can_receive(),
                state["received"] == state["delivered"] && state["rxFin"] == 0
            );
            assert_eq!(
                rx.needs_shutdown(),
                state["received"] == state["delivered"]
                    && state["rxFin"] == 1
                    && state["closed"] == 0
            );
        }
    }
}

#[test]
fn invalid_progress_cannot_consume_or_overwrite_pending_bytes() {
    let mut tx = SendStream::default();
    tx.read_buffer().unwrap()[..3].copy_from_slice(b"abc");
    tx.read(3).unwrap();
    assert!(tx.read(1).is_err());
    assert!(tx.read_buffer().is_err());
    assert!(tx.sent(4).is_err());
    tx.sent(1).unwrap();
    assert_eq!(tx.pending(false), Some((&b"bc"[..], false)));
    assert_eq!(tx.written(), 1);
    let mut rx = ReceiveStream::default();
    rx.receive_buffer().unwrap()[..4].copy_from_slice(b"Rabc");
    rx.received(4, true, 1).unwrap();
    assert!(!rx.needs_shutdown());
    assert!(rx.receive_buffer().is_err());
    assert!(rx.closed().is_err());
    assert!(rx.delivered(4).is_err());
    assert!(rx.delivered(0).is_err());
    assert!(rx.received(0, true, 0).is_err());
    assert_eq!(rx.pending(), b"abc");
    rx.delivered(3).unwrap();
    assert!(rx.needs_shutdown());
    rx.closed().unwrap();
    assert!(!rx.can_receive());
    assert!(rx.received(0, true, 0).is_err());
}
