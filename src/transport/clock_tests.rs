//! Upstream recovery uses system time; application deadlines use Tokio time.
use super::{engine::Engine, engine_tests, mobility, scheduling, PacketSocket};
use crate::native_shutdown::Control;
use futures::FutureExt;
use std::{io, time::Duration};
use tokio::{net::UdpSocket, sync::oneshot, time::Instant};

fn establish(a: &mut Engine, b: &mut Engine) {
    for _ in 0..50 {
        a.step(Instant::now).unwrap();
        b.step(Instant::now).unwrap();
        engine_tests::packets(a, b);
        engine_tests::packets(b, a);
    }
    assert!(a.ready() && b.ready());
}

fn discard_packets(engine: &mut Engine) -> usize {
    let mut count = 0;
    loop {
        match engine.conn.send(&mut [0; 1350]) {
            Ok(_) => count += 1,
            Err(quiche::Error::Done) => return count,
            other => panic!("send failed: {other:?}"),
        }
    }
}

#[test]
fn reliable_stream_progress_does_not_sample_datagram_clock() {
    let (mut a, mut b) = engine_tests::pair();
    establish(&mut a, &mut b);
    a.tx.read_buffer().unwrap()[..4].copy_from_slice(b"data");
    a.tx.read(4).unwrap();
    a.step(|| panic!("reliable send sampled application clock"))
        .unwrap();
    engine_tests::packets(&mut a, &mut b);
    b.step(|| panic!("reliable receive sampled application clock"))
        .unwrap();
    assert_eq!(b.rx.pending(), b"data");
    assert!(a
        .datagram_deadline(|| panic!("no datagram queued"))
        .is_none());
    a.datagram(b"datagram".to_vec()).unwrap();
    let now = Instant::now();
    assert_eq!(a.datagram_deadline(|| now), Some(now));
    let sampled = std::cell::Cell::new(false);
    a.step(|| {
        sampled.set(true);
        now
    })
    .unwrap();
    assert!(sampled.get());
}

#[tokio::test]
async fn upstream_recovery_retransmits_after_real_timeout() {
    let (mut a, mut b) = engine_tests::pair();
    establish(&mut a, &mut b);
    a.tx.read_buffer().unwrap()[..4].copy_from_slice(b"lost");
    a.tx.read(4).unwrap();
    a.step(Instant::now).unwrap();
    assert!(discard_packets(&mut a) > 0);
    let timeout = a.conn.timeout().unwrap();
    assert!(timeout > Duration::ZERO && timeout < Duration::from_secs(1));
    // Upstream recovery is driven by std::time::Instant.
    tokio::time::sleep(timeout + Duration::from_millis(1)).await;
    assert_eq!(a.conn.timeout(), Some(Duration::ZERO));
    a.conn.on_timeout();
    for _ in 0..50 {
        engine_tests::packets(&mut a, &mut b);
        b.step(Instant::now).unwrap();
        engine_tests::packets(&mut b, &mut a);
        a.step(Instant::now).unwrap();
        if b.rx.pending() == b"lost" {
            break;
        }
    }
    assert_eq!(b.rx.pending(), b"lost");
    b.delivered(4).unwrap();
    // Duplicated recovery traffic must not duplicate bytes in the RPC bridge.
    engine_tests::packets(&mut a, &mut b);
    b.step(Instant::now).unwrap();
    assert!(b.rx.pending().is_empty());
}

#[tokio::test]
async fn upstream_idle_expiry_does_not_expire_a_new_generation() {
    let addresses = [
        "127.0.0.1:1234".parse().unwrap(),
        "127.0.0.1:4321".parse().unwrap(),
    ];
    let (mut a, mut b) = engine_tests::pair_at(addresses, |c| {
        c.set_max_idle_timeout(100);
        c.set_initial_rtt(Duration::from_millis(5));
    });
    establish(&mut a, &mut b);
    tokio::time::sleep(Duration::from_millis(150)).await;
    let (mut replacement, mut peer) = engine_tests::pair();
    establish(&mut replacement, &mut peer);
    a.conn.on_timeout();
    assert!(a.conn.is_closed() && a.conn.is_timed_out());
    assert_eq!(
        a.step(Instant::now).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    replacement.conn.on_timeout();
    assert!(replacement.step(Instant::now).unwrap());
    assert!(!replacement.conn.is_closed());
}

#[tokio::test(start_paused = true)]
async fn application_pacing_migration_and_shutdown_deadlines_use_runtime_time() {
    tokio::time::advance(Duration::from_secs(600)).await;
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let peer_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let old = socket.local_addr().unwrap();
    let peer = peer_socket.local_addr().unwrap();
    let (mut a, mut b) = engine_tests::pair_at([old, peer], |_| {});
    establish(&mut a, &mut b);
    let mut socket =
        PacketSocket::Dedicated(crate::transport::socket::DatagramSocket::new(socket).unwrap());
    let (_mobility, mut migration) = mobility::pair();
    assert_eq!(
        migration.timeout(|| panic!("idle migration sampled application clock")),
        Duration::from_secs(10)
    );
    // Exchange CID allowances before requesting a new path.
    migration
        .step(&mut a.conn, &mut socket, Instant::now)
        .unwrap();
    engine_tests::packets(&mut a, &mut b);
    let mut peer_path = PacketSocket::Dedicated(
        crate::transport::socket::DatagramSocket::new(peer_socket).unwrap(),
    );
    let (_, mut peer_migration) = mobility::pair();
    peer_migration
        .step(&mut b.conn, &mut peer_path, Instant::now)
        .unwrap();
    engine_tests::packets(&mut b, &mut a);
    establish(&mut a, &mut b);

    let (schedule, driver) = scheduling::pair();
    a.scheduling = driver;
    schedule
        .configure(scheduling::Schedule {
            datagrams: Some(scheduling::DatagramPacing {
                interval: Duration::from_millis(20),
                burst: 1,
            }),
            ..Default::default()
        })
        .unwrap();
    assert!(a.scheduling.admit(Instant::now()));
    let control = Control::new();
    control.begin(Duration::from_millis(40)).unwrap();
    let (reply, mut result) = oneshot::channel();
    let candidate = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    migration.command(
        Some(mobility::Command::Migrate(
            crate::transport::socket::DatagramSocket::new(candidate).unwrap(),
            peer,
            Instant::now() + Duration::from_millis(30),
            reply,
        )),
        &mut a.conn,
        &socket,
        Instant::now(),
    );
    assert!(result.try_recv().is_err());
    assert_eq!(migration.timeout(Instant::now), Duration::from_millis(20));
    let origin = Instant::now();
    for millis in [19, 20, 29, 30, 39, 40] {
        tokio::time::advance(origin + Duration::from_millis(millis) - Instant::now()).await;
        let now = Instant::now();
        assert_eq!(
            migration.timeout(|| now),
            if millis <= 30 {
                Duration::from_millis(30 - millis)
            } else {
                Duration::from_secs(10)
            }
        );
        migration.step(&mut a.conn, &mut socket, || now).unwrap();
        if millis < 20 {
            assert!(!a.scheduling.admit(now));
        } else if millis == 20 {
            assert!(a.scheduling.admit(now));
        }
        if millis < 30 {
            assert!(matches!(
                result.try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            ));
        } else if millis == 30 {
            assert_eq!(
                result.try_recv().unwrap().unwrap_err().kind(),
                io::ErrorKind::TimedOut
            );
        }
        assert_eq!(control.expired().now_or_never().is_some(), millis >= 40);
        assert_eq!(socket.local_addr().unwrap(), old);
        assert!(!a.conn.is_closed());
    }
}

#[tokio::test(start_paused = true)]
async fn outer_driver_observes_virtual_shutdown_deadline() {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let peer = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let (mut a, mut b) = engine_tests::pair_at(
        [socket.local_addr().unwrap(), peer.local_addr().unwrap()],
        |_| {},
    );
    establish(&mut a, &mut b);
    let control = Control::new();
    control.begin(Duration::from_millis(40)).unwrap();
    let (_application, io) = crate::rpc::local_io::pair(64);
    let driver = super::drive(
        PacketSocket::Dedicated(crate::transport::socket::DatagramSocket::new(socket).unwrap()),
        a.conn,
        io.into_split(),
        super::SessionDrivers {
            bulk: None,
            established: None,
            datagrams: None,
            shutdown: Some(super::shutdown::ShutdownDriver::new(control.clone(), false)),
            mobility: None,
            scheduling: scheduling::pair().1,
        },
    );
    tokio::pin!(driver);
    assert!(driver.as_mut().now_or_never().is_none());
    tokio::time::advance(Duration::from_millis(40)).await;
    let error = driver
        .as_mut()
        .now_or_never()
        .expect("shutdown deadline")
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(
        error.to_string(),
        "Native shutdown acknowledgement timed out"
    );
    assert_eq!(
        control.wait().await.unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
}

#[test]
fn replay_tlc_runtime_clock_and_replacement_deadlines() {
    use capntproto_test_support::verification::exploration;
    let config = include_str!("../../verification/NativeClockDomains.cfg");
    exploration::controls(
        "verification/NativeClockDomains.tla",
        "native-clock-domains",
        config,
        &[
            ("wallClock", "OldDeadline"),
            ("early", "OldDeadline"),
            ("inherit", "NewDeadline"),
        ],
        None,
    )
    .unwrap();
    let traces = exploration::traces(
        "verification/NativeClockDomains.tla",
        "native-clock-domains",
        config,
    )
    .unwrap();
    for trace in traces {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .unwrap()
            .block_on(async {
                tokio::time::advance(Duration::from_secs(600)).await;
                let origin = Instant::now();
                let old = Control::new();
                old.begin(Duration::from_secs(10)).unwrap();
                let mut old_closed = false;
                let mut new_closed = false;
                let mut replacement = None;
                for state in trace {
                    match state["event"] {
                        1 => tokio::time::advance(Duration::from_secs(5)).await,
                        2 => {
                            let new = Control::new();
                            new.begin(Duration::from_secs(10)).unwrap();
                            replacement = Some(new);
                        }
                        3 => old_closed = old.expired().now_or_never().is_some(),
                        4 => {
                            new_closed = replacement
                                .as_ref()
                                .unwrap()
                                .expired()
                                .now_or_never()
                                .is_some()
                        }
                        other => panic!("unknown clock event: {other}"),
                    }
                    assert_eq!((Instant::now() - origin).as_secs(), 5 * state["now"]);
                    assert_eq!(old_closed, state["oldClosed"] == 1);
                    assert_eq!(old_closed, state["oldClosed"] == 1);
                    assert_eq!(replacement.is_some(), state["replacement"] == 1);
                    if replacement.is_some() {
                        assert_eq!(new_closed, state["newClosed"] == 1);
                        assert_eq!(new_closed, state["newClosed"] == 1);
                    }
                }
            });
    }
}
