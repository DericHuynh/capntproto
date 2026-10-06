use super::*;

#[tokio::test]
async fn rejected_send_progress_preserves_pending_bytes_and_counter() {
    let mut stream = SendStream::default();
    assert!(stream.read(1).is_err());
    assert!(stream
        .read_owned(Bytes::from(vec![0; BUFFER_BYTES + 1]))
        .is_err());
    assert!(stream.can_read());
    stream.read_owned(Bytes::from_static(b"abc")).unwrap();
    assert!(stream.read_buffer().is_err());
    assert!(stream.read(0).is_err());
    assert!(stream.read_owned(Bytes::new()).is_err());
    assert!(stream
        .read_from(&mut CopyInput(&b"replacement"[..]))
        .await
        .is_err());
    assert!(stream.sent(4).is_err());
    assert_eq!(
        stream.pending_owned(false),
        Some((Bytes::from_static(b"abc"), false))
    );
    assert_eq!(stream.written(), 0);

    stream.written = u64::MAX - 2;
    assert!(stream.sent(3).is_err());
    assert_eq!(stream.written(), u64::MAX - 2);
    assert_eq!(stream.pending(false), Some((&b"abc"[..], false)));
    stream.sent(2).unwrap();
    assert_eq!(stream.written(), u64::MAX);
    assert_eq!(stream.pending(false), Some((&b"c"[..], false)));
    assert!(stream.sent(1).is_err());
    assert!(!stream.can_read());
}

#[tokio::test]
async fn receive_bounds_and_owned_output_reject_invalid_states_without_losing_data() {
    let mut stream = ReceiveStream::default();
    let (local, mut peer) = crate::rpc::local_io::pair(BUFFER_BYTES);
    let (_, mut writer) = local.into_split();
    assert!(writer.write_from(&mut stream).await.is_err());
    assert!(stream.received(1, false, 0).is_err());
    assert!(stream.received(0, false, 1).is_err());
    assert!(stream.delivered(1).is_err());
    assert_eq!(
        stream.delivered(0).unwrap_err().kind(),
        io::ErrorKind::WriteZero
    );
    assert!(stream.closed().is_err());
    stream.receive_buffer().unwrap()[..4].copy_from_slice(b"\0abc");
    assert!(stream.received(BUFFER_BYTES + 1, false, 0).is_err());
    stream.received(4, true, 1).unwrap();
    assert!(stream.receive_buffer().is_err());
    assert!(stream.received(0, false, 0).is_err());
    assert!(stream.delivered(4).is_err());
    assert!(stream.closed().is_err());
    assert_eq!(stream.pending(), b"abc");
    let n = writer.write_from(&mut stream).await.unwrap();
    stream.delivered(n).unwrap();
    assert!(stream.needs_shutdown());
    Output::shutdown(&mut writer).await.unwrap();
    stream.closed().unwrap();
    assert!(stream.closed().is_err());
    assert!(writer.write_from(&mut stream).await.is_err());
    let mut received = Vec::new();
    peer.read_to_end(&mut received).await.unwrap();
    assert_eq!(received, b"abc");
}

#[tokio::test]
async fn copying_output_delivers_bytes_before_shutdown() {
    let (writer, mut reader) = tokio::io::duplex(16);
    let mut writer = CopyOutput(writer);
    let mut stream = ReceiveStream::default();
    stream.receive_buffer().unwrap()[..3].copy_from_slice(b"rpc");
    stream.received(3, true, 0).unwrap();
    let n = writer.write_from(&mut stream).await.unwrap();
    stream.delivered(n).unwrap();
    writer.shutdown().await.unwrap();
    stream.closed().unwrap();
    let mut received = Vec::new();
    reader.read_to_end(&mut received).await.unwrap();
    assert_eq!(received, b"rpc");
}

#[test]
fn quic_receive_distinguishes_no_data_reset_and_invalid_local_progress() {
    use crate::transport::engine_tests::{packets, pair};
    let (mut a, mut b) = pair();
    let mut stream = ReceiveStream::default();
    assert_eq!(stream.receive_from(&mut b.conn).unwrap(), None);
    let now = tokio::time::Instant::now();
    for _ in 0..100 {
        a.step(|| now).unwrap();
        b.step(|| now).unwrap();
        packets(&mut a, &mut b);
        packets(&mut b, &mut a);
        if a.ready() && b.ready() {
            break;
        }
    }
    assert!(a.ready() && b.ready());
    assert_eq!(stream.receive_from(&mut b.conn).unwrap(), None);
    a.conn
        .stream_shutdown(0, quiche::Shutdown::Write, 7)
        .unwrap();
    packets(&mut a, &mut b);
    assert!(stream.receive_from(&mut b.conn).is_err());
    stream.received(0, true, 0).unwrap();
    assert_eq!(
        stream.receive_from(&mut b.conn).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
}
