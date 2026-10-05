use super::*;
use futures::FutureExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn owned_reads_preserve_backpressure_order_cancellation_and_eof() {
    let (a, b) = pair(8);
    let (_, mut writer) = a.into_split();
    let (mut reader, _) = b.into_split();
    assert!(reader.read_owned().now_or_never().is_none());
    assert_eq!(
        writer
            .write_vectored(&[
                io::IoSlice::new(b"abc"),
                io::IoSlice::new(b"12345"),
                io::IoSlice::new(b"excess"),
            ])
            .await
            .unwrap(),
        8
    );
    assert!(writer.write_all(b"XY").now_or_never().is_none());
    let mut empty = [];
    reader.read_exact(&mut empty).await.unwrap();
    let mut prefix = [0; 2];
    reader.read_exact(&mut prefix).await.unwrap();
    assert_eq!(&prefix, b"ab");
    writer.write_all(b"XY").await.unwrap();
    let pointer = reader.0.borrow().bytes.as_ptr();
    let retained = reader.read_owned().await.unwrap();
    assert_eq!(
        retained.as_ptr(),
        pointer,
        "handoff must not copy the payload"
    );
    assert_eq!(&retained[..], b"c12345XY");
    assert!(reader.read_owned().now_or_never().is_none());
    writer.write_all(b"new data").await.unwrap();
    writer.shutdown().await.unwrap();
    assert_eq!(&reader.read_owned().await.unwrap()[..], b"new data");
    assert!(reader.read_owned().await.unwrap().is_empty());
    assert_eq!(&retained[..], b"c12345XY");

    let (a, b) = pair(8);
    let (_, mut writer) = a.into_split();
    let (reader, _) = b.into_split();
    drop(reader);
    assert_eq!(
        writer.write_all(b"x").await.unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
}

#[test]
fn outstanding_tiny_views_share_slabs_without_per_message_allocation() {
    let (a, b) = pair(crate::rpc::QUIC_BUFFER_BYTES);
    let (_, mut writer) = a.into_split();
    let (mut reader, _) = b.into_split();
    let mut retained = Vec::with_capacity(1024);
    let counts = allocation_counter::measure(|| {
        for _ in 0..1024 {
            writer.write_all(b"x").now_or_never().unwrap().unwrap();
            retained.push(reader.read_owned().now_or_never().unwrap().unwrap());
        }
    });
    assert!(counts.count_total < 16, "{counts:?}");
    assert!(
        counts.bytes_total < 2 * crate::rpc::QUIC_BUFFER_BYTES as u64,
        "{counts:?}"
    );
    drop(reader);
    drop(writer);
    assert!(retained.iter().all(|view| &view[..] == b"x"));
}
