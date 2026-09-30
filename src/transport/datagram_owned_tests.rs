use super::*;

#[test]
fn owned_admission_preserves_allocation_and_returns_it_on_every_rejection() {
    let (queue, mut receiver) = tokio::sync::mpsc::channel(1);
    let sender = DatagramSender(queue);
    let mut payload = Vec::with_capacity(128);
    payload.extend_from_slice(b"owned");
    let pointer = payload.as_ptr();
    sender.try_send_owned(payload).unwrap();

    let mut retry = Vec::with_capacity(64);
    retry.extend_from_slice(b"retry");
    let retry_pointer = retry.as_ptr();
    let failure = sender.try_send_owned(retry).unwrap_err();
    assert_eq!(failure.kind(), io::ErrorKind::WouldBlock);
    let (_, retry) = failure.into_parts();
    assert_eq!(retry.as_ptr(), retry_pointer);
    assert_eq!(retry.capacity(), 64);
    assert_eq!(retry, b"retry");
    assert_eq!(
        sender.try_send(b"borrowed").unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );

    let delivered = receiver.try_recv().unwrap();
    assert_eq!(delivered, b"owned");
    assert_eq!(delivered.as_ptr(), pointer);
    assert_eq!(delivered.capacity(), 128);
    sender.try_send_owned(retry).unwrap();
    let delivered = receiver.try_recv().unwrap();
    assert_eq!(delivered.as_ptr(), retry_pointer);

    let oversized = vec![7; MAX_DATAGRAM_BYTES + 1];
    let pointer = oversized.as_ptr();
    let failure = sender.try_send_owned(oversized).unwrap_err();
    assert_eq!(failure.kind(), io::ErrorKind::InvalidInput);
    let (_, oversized) = failure.into_parts();
    assert_eq!(oversized.as_ptr(), pointer);
    assert!(receiver.try_recv().is_err());

    drop(receiver);
    let payload = b"closed".to_vec();
    let pointer = payload.as_ptr();
    let failure = sender.try_send_owned(payload).unwrap_err();
    assert_eq!(failure.kind(), io::ErrorKind::BrokenPipe);
    let (_, payload) = failure.into_parts();
    assert_eq!(payload.as_ptr(), pointer);
    assert_eq!(payload, b"closed");
}
