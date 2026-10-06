use super::*;
use crate::transport::{
    backend_tests::{pair, Backend},
    QuicVersion,
};
use futures::FutureExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
const TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn stalled_bulk_does_not_allocate_while_other_planes_drive_the_connection() {
    use crate::{native_shutdown::Control, transport::engine_tests};
    let addresses = [
        "127.0.0.1:1234".parse().unwrap(),
        "127.0.0.1:4321".parse().unwrap(),
    ];
    let (mut a, mut b) = engine_tests::pair_at(addresses, |config| {
        config.set_initial_max_stream_data_bidi_local(1024 * 1024);
        config.set_initial_max_stream_data_bidi_remote(1024 * 1024);
    });
    let (_, sender) = super::pair(
        &a.conn,
        [1; 32],
        [2; 32],
        Control::new(),
        Arc::new(Notify::new()),
    );
    let (receiver, driver) = super::pair(
        &b.conn,
        [2; 32],
        [1; 32],
        Control::new(),
        Arc::new(Notify::new()),
    );
    a.bulk = Some(sender);
    b.bulk = Some(driver);
    for _ in 0..50 {
        a.step(Instant::now).unwrap();
        engine_tests::packets(&mut a, &mut b);
        b.step(Instant::now).unwrap();
        engine_tests::packets(&mut b, &mut a);
    }
    assert!(a.ready() && b.ready());
    // Drive the sender's wire input directly below; it must not interpret the
    // reverse stream's consumption receipts as an unknown local transfer.
    a.bulk = None;
    let length = 3 * BUFFER;
    let (offer, mut input) = receiver.receive(length as u64, TIMEOUT).unwrap();
    a.conn.stream_send(4, &offer.header(), false).unwrap();
    let payload = vec![29; length];
    assert_eq!(a.conn.stream_send(4, &payload, true).unwrap(), length);
    // Fill the bounded application pipe and staging buffer while withholding
    // application consumption. The remainder must stay readable in quiche.
    for _ in 0..100 {
        engine_tests::packets(&mut a, &mut b);
        b.step(Instant::now).unwrap();
        engine_tests::packets(&mut b, &mut a);
        a.step(Instant::now).unwrap();
        tokio::task::yield_now().await;
    }
    assert!(b.conn.stream_readable(4));
    let allocations = allocation_counter::measure(|| {
        for _ in 0..100 {
            b.step(Instant::now).unwrap();
        }
    });
    assert_eq!(allocations.count_total, 0, "{allocations:?}");
    assert!(b.conn.stream_readable(4));
    // The stalled stream must still finish with exact data after the reader
    // resumes; avoiding allocation must not consume its readiness indication.
    let mut output = Vec::new();
    for _ in 0..100 {
        while let Some(read) = input.read_buf(&mut output).now_or_never() {
            if read.unwrap() == 0 {
                break;
            }
        }
        b.step(Instant::now).unwrap();
        engine_tests::packets(&mut b, &mut a);
        a.step(Instant::now).unwrap();
        engine_tests::packets(&mut a, &mut b);
        tokio::task::yield_now().await;
        if output.len() == length {
            break;
        }
    }
    assert_eq!(output, payload);
    assert_eq!(input.read(&mut [0; 1]).now_or_never().unwrap().unwrap(), 0);
}

async fn peers() -> (
    crate::transport::AuthenticatedSession,
    crate::transport::AuthenticatedSession,
) {
    pair(Backend::Quiche, Backend::Quiche, QuicVersion::V1).await
}
async fn scoped(f: impl std::future::Future<Output = ()>) {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(20), f)
                .await
                .unwrap();
        })
        .await;
}
#[tokio::test]
async fn bidirectional_streams_exact_bytes_and_consumption_receipts() {
    scoped(async {
        let (a, b) = peers().await;
        for length in [0, 1, 16_385, 300_000, 2_000_000] {
            for (a, b) in [
                (a.bulk().unwrap(), b.bulk().unwrap()),
                (b.bulk().unwrap(), a.bulk().unwrap()),
            ] {
                let (offer, mut input) = b.receive(length, TIMEOUT).unwrap();
                let mut output = a
                    .send(Offer::decode(&offer.encode()).unwrap(), TIMEOUT)
                    .unwrap();
                let bytes = vec![42; length as usize];
                let mut read = Vec::new();
                let (sent, received) = tokio::join!(
                    async {
                        output.write_all(&bytes).await?;
                        output.finish().await
                    },
                    input.read_to_end(&mut read)
                );
                sent.unwrap();
                received.unwrap();
                assert_eq!(read, bytes);
            }
        }
        a.shutdown(TIMEOUT).await.unwrap();
    })
    .await;
}
#[tokio::test]
async fn stalled_bulk_preserves_control_and_cancellation_unblocks_producer() {
    scoped(async {
        let (mut a, mut b) = peers().await;
        let (offer, input) = b.bulk().unwrap().receive(2_000_000, TIMEOUT).unwrap();
        let mut output = a.bulk().unwrap().send(offer, TIMEOUT).unwrap();
        let producer = tokio::task::spawn_local(async move {
            output.write_all(&vec![1; 2_000_000]).await?;
            output.finish().await
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!producer.is_finished());
        a.io.as_mut().unwrap().write_all(b"control").await.unwrap();
        let mut bytes = [0; 7];
        b.io.as_mut().unwrap().read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"control");
        let ad = a.take_datagrams().unwrap();
        let mut bd = b.take_datagrams().unwrap();
        ad.sender().try_send(b"realtime").unwrap();
        assert_eq!(bd.recv().await.unwrap(), b"realtime");
        drop(input);
        assert!(producer.await.unwrap().is_err());
        a.shutdown(TIMEOUT).await.unwrap();
    })
    .await;
}
#[tokio::test]
async fn grants_reject_replay_wrong_direction_other_session_and_invalid_limits() {
    scoped(async {
        let (a, b) = peers().await;
        let (other, _) = peers().await;
        let pa = a.bulk().unwrap();
        let pb = b.bulk().unwrap();
        let (offer, _input) = pb.receive(0, TIMEOUT).unwrap();
        let bytes = offer.encode();
        assert!(pb.send(Offer::decode(&bytes).unwrap(), TIMEOUT).is_err());
        assert!(other
            .bulk()
            .unwrap()
            .send(Offer::decode(&bytes).unwrap(), TIMEOUT)
            .is_err());
        let _sent = pa.send(offer, TIMEOUT).unwrap();
        assert!(pa.send(Offer::decode(&bytes).unwrap(), TIMEOUT).is_err());
        assert!(pb.receive(MAX_TRANSFER_BYTES + 1, TIMEOUT).is_err());
        assert!(pb.receive(0, Duration::ZERO).is_err());
        assert!(pb.receive(0, Duration::from_secs(301)).is_err());
        a.shutdown.begin(TIMEOUT).unwrap();
        assert!(pa.receive(0, TIMEOUT).is_err());
        drop(b);
        assert!(pb.receive(0, TIMEOUT).is_err());
    })
    .await;
}
#[tokio::test]
async fn unused_grants_release_slots_without_consuming_quic_stream_ids() {
    scoped(async {
        let (a, b) = peers().await;
        let plane = b.bulk().unwrap();
        for _ in 0..80 {
            let mut grants = Vec::new();
            for _ in 0..MAX_ACTIVE {
                grants.push(plane.receive(1, TIMEOUT).unwrap());
            }
            assert!(
                matches!(plane.receive(1,TIMEOUT), Err(e) if e.kind()==io::ErrorKind::WouldBlock)
            );
            drop(grants);
            while plane.0.upgrade().unwrap().borrow().active != 0 {
                tokio::task::yield_now().await;
            }
        }
        let (offer, mut input) = plane.receive(1, TIMEOUT).unwrap();
        let mut output = a.bulk().unwrap().send(offer, TIMEOUT).unwrap();
        let (sent, read) = tokio::join!(
            async {
                output.write_all(b"x").await?;
                output.finish().await
            },
            input.read_u8()
        );
        sent.unwrap();
        assert_eq!(read.unwrap(), b'x');
    })
    .await;
}
#[tokio::test]
async fn deadlines_and_producer_drop_fail_reader_without_partial_success() {
    scoped(async {
        let (a, b) = peers().await;
        let (_, mut expired) = b
            .bulk()
            .unwrap()
            .receive(10, Duration::from_millis(30))
            .unwrap();
        assert_eq!(
            expired.read(&mut [0]).await.unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        let (offer, mut input) = b.bulk().unwrap().receive(100, TIMEOUT).unwrap();
        let mut output = a.bulk().unwrap().send(offer, TIMEOUT).unwrap();
        output.write_all(b"partial").await.unwrap();
        assert!(output.shutdown().await.is_err());
        drop(output);
        assert!(input.read_to_end(&mut Vec::new()).await.is_err());
    })
    .await;
}
#[tokio::test]
async fn graceful_shutdown_waits_for_bulk_in_both_directions_and_rejects_new_grants() {
    scoped(async {
        for crossed in [false, true] {
            let (a, b) = peers().await;
            let plane = a.bulk().unwrap();
            let (grant, mut receive) = b.bulk().unwrap().receive(100_000, TIMEOUT).unwrap();
            let mut send = plane.send(grant, TIMEOUT).unwrap();
            let shutdown = a.shutdown(TIMEOUT);
            tokio::pin!(shutdown);
            assert!(shutdown.as_mut().now_or_never().is_none());
            assert!(plane.receive(0, TIMEOUT).is_err());
            let producer = async {
                send.write_all(&vec![8; 100_000]).await.unwrap();
                send.finish().await.unwrap();
            };
            let consumer = async {
                let mut bytes = Vec::new();
                receive.read_to_end(&mut bytes).await.unwrap();
                assert_eq!(bytes, vec![8; 100_000]);
            };
            if crossed {
                let (_, _, a, b) = tokio::join!(producer, consumer, shutdown, b.shutdown(TIMEOUT));
                a.unwrap();
                b.unwrap();
            } else {
                let (_, _, result) = tokio::join!(producer, consumer, shutdown);
                result.unwrap();
            }
        }
    })
    .await;
}
#[test]
fn malformed_offers_and_bounded_replay_window() {
    let offer = Offer::new([4; 32], 1, 9);
    let encoded = offer.encode();
    for n in 0..encoded.len() {
        assert!(Offer::decode(&encoded[..n]).is_err());
    }
    let mut header = offer.header();
    header[60] ^= 1;
    assert!(offer.validate(&header).is_err());
    let mut window = wire::IdWindow::default();
    for id in [5, 1, 7, 3, 1000, 999, 938] {
        window.claim(id).unwrap();
    }
    for id in [1000, 999, 938, 1, 936] {
        assert!(window.claim(id).is_err());
    }
}

#[tokio::test]
async fn malformed_stream_lengths_fail_and_forged_headers_do_not_consume_grants() {
    use crate::transport::stream_planes_probe::{deliver, packets, pair};
    for payload in [b"ab".as_slice(), b"abcd".as_slice(), b"abc".as_slice()] {
        let (mut a, mut b) = pair(2 * 1024 * 1024, 65536);
        let (_, mut ad) = driver::pair(
            &a,
            [1; 32],
            [2; 32],
            crate::native_shutdown::Control::new(),
            Arc::new(Notify::new()),
        );
        let (plane, mut bd) = driver::pair(
            &b,
            [2; 32],
            [1; 32],
            crate::native_shutdown::Control::new(),
            Arc::new(Notify::new()),
        );
        ad.step(&mut a).unwrap();
        a.stream_send(0, b"R", false).unwrap();
        deliver(&mut b, packets(&mut a));
        bd.step(&mut b).unwrap();
        let (offer, mut input) = plane.receive(3, TIMEOUT).unwrap();
        let mut forged = offer.header();
        forged[60] ^= 1;
        a.stream_send(4, &forged, true).unwrap();
        deliver(&mut b, packets(&mut a));
        bd.step(&mut b).unwrap();
        // A forged attempt must not destroy the legitimate holder's grant.
        assert_eq!(plane.stats().unwrap().active, 1);
        let mut bytes = offer.header().to_vec();
        bytes.extend_from_slice(payload);
        a.stream_send(8, &bytes, true).unwrap();
        let mut received = Vec::new();
        let mut result = None;
        for _ in 0..100 {
            deliver(&mut b, packets(&mut a));
            bd.step(&mut b).unwrap();
            deliver(&mut a, packets(&mut b));
            if let Some(read) = input.read_to_end(&mut received).now_or_never() {
                result = Some(read);
                break;
            }
            tokio::task::yield_now().await;
        }
        let result = result.expect("receiver did not settle");
        if payload.len() == 3 {
            result.unwrap();
            assert_eq!(received, payload);
        } else {
            assert!(result.is_err());
        }
    }
}

#[tokio::test]
async fn reconnect_with_same_identities_cannot_reuse_old_grants() {
    scoped(async {
        let a = crate::transport::Identity::generate();
        let b = crate::transport::Identity::generate();
        let mut sessions = Vec::new();
        for _ in 0..2 {
            let left = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let right = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let address = right.local_addr().unwrap();
            let (first, second) = tokio::join!(
                crate::transport::connect_authenticated(
                    left,
                    address,
                    &a,
                    b.public_key(),
                    None,
                    b"bulk reconnect"
                ),
                crate::transport::accept_authenticated(
                    right,
                    &b,
                    a.public_key(),
                    None,
                    b"bulk reconnect"
                )
            );
            sessions.push((first.unwrap(), second.unwrap()));
        }
        let (offer, _input) = sessions[0].1.bulk().unwrap().receive(1, TIMEOUT).unwrap();
        assert!(sessions[1].0.bulk().unwrap().send(offer, TIMEOUT).is_err());
    })
    .await;
}
