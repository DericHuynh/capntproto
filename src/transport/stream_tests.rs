use super::stream::{CopyInput, ReceiveStream, SendStream};

#[tokio::test]
async fn owned_send_views_survive_partial_sends_reuse_and_canceled_reads() {
    use futures::FutureExt;
    use tokio::io::AsyncWriteExt;
    let mut tx = SendStream::default();
    let (mut writer, mut reader) = tokio::io::duplex(16);
    assert!(tx
        .read_from(&mut CopyInput(&mut reader))
        .now_or_never()
        .is_none());
    assert!(tx.can_read());
    writer.write_all(b"old bytes").await.unwrap();
    let bytes = tx.read_from(&mut CopyInput(&mut reader)).await.unwrap();
    tx.read_owned(bytes).unwrap();
    let (retained, fin) = tx.pending_owned(false).unwrap();
    assert!(!fin);
    tx.sent(4).unwrap();
    assert_eq!(&tx.pending_owned(false).unwrap().0[..], b"bytes");
    tx.sent(5).unwrap();
    assert_eq!(tx.written(), 9);

    // A lost packet can retain the original view while the next read reserves
    // storage. The full bridge capacity must not modify that immutable view.
    let input = vec![0x72; crate::rpc::QUIC_BUFFER_BYTES + 1];
    let mut input = &input[..];
    let bytes = tx.read_from(&mut CopyInput(&mut input)).await.unwrap();
    let count = bytes.len();
    assert!((4096..=crate::rpc::QUIC_BUFFER_BYTES).contains(&count));
    tx.read_owned(bytes).unwrap();
    let (next, _) = tx.pending_owned(false).unwrap();
    assert_eq!(&retained[..], b"old bytes");
    assert!(next.iter().all(|b| *b == 0x72));
    tx.sent(count).unwrap();
    drop(retained);
    drop(next);

    writer.shutdown().await.unwrap();
    let bytes = tx.read_from(&mut CopyInput(&mut reader)).await.unwrap();
    assert!(bytes.is_empty());
    tx.read_owned(bytes).unwrap();
    assert!(tx.pending_owned(true).is_none());
    assert_eq!(
        tx.pending_owned(false).unwrap(),
        (bytes::Bytes::new(), true)
    );
    tx.sent(0).unwrap();
    assert!(tx.drained());
}

#[test]
fn retained_small_writes_do_not_allocate_one_slab_per_write() {
    use futures::FutureExt;
    let mut tx = SendStream::default();
    let mut retained = Vec::with_capacity(1024);
    let counts = allocation_counter::measure(|| {
        for _ in 0..1024 {
            let mut input = &b"x"[..];
            let bytes = tx
                .read_from(&mut CopyInput(&mut input))
                .now_or_never()
                .unwrap()
                .unwrap();
            let n = bytes.len();
            assert_eq!(n, 1);
            tx.read_owned(bytes).unwrap();
            retained.push(tx.pending_owned(false).unwrap().0);
            tx.sent(n).unwrap();
        }
    });
    assert!(
        counts.bytes_total < 2 * crate::rpc::QUIC_BUFFER_BYTES as u64,
        "{counts:?}"
    );
    assert!(retained.iter().all(|view| &view[..] == b"x"));
    assert_eq!(tx.written(), 1024);
}

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
