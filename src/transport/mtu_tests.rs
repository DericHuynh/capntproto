//! Real encrypted traffic through a UDP relay that silently drops oversize
//! packets. This exercises PMTU fallback without changing host network settings.
use super::*;
use std::{cell::Cell, rc::Rc};

async fn roundtrip(a: &mut AuthenticatedSession, b: &mut AuthenticatedSession, value: u8) {
    let bytes = vec![value; 150_000];
    let mut received = vec![0; bytes.len()];
    let (sent, read) = tokio::join!(
        a.io.as_mut().unwrap().write_all(&bytes),
        b.io.as_mut().unwrap().read_exact(&mut received),
    );
    sent.unwrap();
    read.unwrap();
    assert_eq!(received, bytes);
    let (sent, read) = tokio::join!(
        b.io.as_mut().unwrap().write_all(&bytes),
        a.io.as_mut().unwrap().read_exact(&mut received),
    );
    sent.unwrap();
    read.unwrap();
    assert_eq!(received, bytes);
}

async fn through_relay(shrink: bool) {
    tokio::time::timeout(Duration::from_secs(30), async {
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let client_addr = client.local_addr().unwrap();
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server.local_addr().unwrap();
        let relay = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let relay_addr = relay.local_addr().unwrap();
        let limit = Rc::new(Cell::new(if shrink { 16384 } else { 1280 }));
        let dropped = Rc::new(Cell::new(0));
        let large = Rc::new(Cell::new(0));
        let relay_task = {
            let limit = limit.clone();
            let dropped = dropped.clone();
            let large = large.clone();
            crate::rpc::task::Task::spawn(async move {
                let mut bytes = vec![0; 65535];
                loop {
                    let (n, from) = relay.recv_from(&mut bytes).await?;
                    let to = if from == client_addr {
                        server_addr
                    } else {
                        assert_eq!(from, server_addr);
                        client_addr
                    };
                    if n > limit.get() {
                        dropped.set(dropped.get() + 1);
                    } else {
                        if n > 1350 {
                            large.set(large.get() + 1);
                        }
                        relay.send_to(&bytes[..n], to).await?;
                    }
                }
            })
        };
        let a = Identity::generate();
        let b = Identity::generate();
        let (a, b) = tokio::join!(
            connect_authenticated(
                client,
                relay_addr,
                &a,
                b.public_key(),
                Some([71; 32]),
                b"mtu"
            ),
            accept_authenticated(server, &b, a.public_key(), Some([71; 32]), b"mtu"),
        );
        let (mut a, mut b) = (a.unwrap(), b.unwrap());
        roundtrip(&mut a, &mut b, 42).await;
        if shrink {
            assert!(
                large.get() > 4,
                "larger packets must work before shrinking the path"
            );
            limit.set(1280);
        }
        roundtrip(&mut a, &mut b, 17).await;
        assert!(
            dropped.get() > 0,
            "the relay must actually discard oversize packets"
        );
        let (a, b) = tokio::join!(
            a.shutdown(Duration::from_secs(5)),
            b.shutdown(Duration::from_secs(5)),
        );
        assert_eq!(a.unwrap().bytes, 300_000);
        assert_eq!(b.unwrap().bytes, 300_000);
        drop(relay_task);
    })
    .await
    .expect("PMTU recovery must preserve reliable data and receipts");
}

#[tokio::test(flavor = "current_thread")]
async fn oversize_probes_fall_back_without_losing_authenticated_stream_data() {
    tokio::task::LocalSet::new()
        .run_until(through_relay(false))
        .await;
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "current_thread")]
async fn established_large_path_recovers_from_a_silent_mtu_reduction() {
    tokio::task::LocalSet::new()
        .run_until(through_relay(true))
        .await;
}
