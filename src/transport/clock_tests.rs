//! One Tokio time domain for the production transport and its runtime controls.
use super::{engine::Engine, engine_tests, mobility, scheduling, PacketSocket};
use crate::native_shutdown::Control;
use futures::FutureExt;
use std::{io, time::Duration};
use tokio::{net::UdpSocket, sync::oneshot, time::Instant};

fn establish(a: &mut Engine, b: &mut Engine) {
    for _ in 0..50 {
        a.step(Instant::now()).unwrap();
        b.step(Instant::now()).unwrap();
        engine_tests::packets(a, b);
        engine_tests::packets(b, a);
    }
    assert!(a.ready() && b.ready());
}

fn discard_packets(engine: &mut Engine) -> usize {
    let mut count = 0;
    loop {
        match engine.conn.send(&mut [0; 1350]) {
            Ok((_, info)) => {
                // Quiche's pacing timestamps must inhabit the runtime's time
                // domain even when the test starts minutes ahead of wall time.
                assert!(info.at >= Instant::now().into_std());
                count += 1;
            }
            Err(quiche::Error::Done) => return count,
            other => panic!("send failed: {other:?}"),
        }
    }
}

#[tokio::test(start_paused = true)]
async fn recovery_and_packet_pacing_follow_runtime_time_after_loss() {
    tokio::time::advance(Duration::from_secs(600)).await;
    let (mut a, mut b) = engine_tests::pair();
    establish(&mut a, &mut b);
    a.tx.read_buffer().unwrap()[..4].copy_from_slice(b"lost");
    a.tx.read(4).unwrap();
    a.step(Instant::now()).unwrap();
    assert!(discard_packets(&mut a) > 0);
    let timeout = a.conn.timeout().unwrap();
    assert!(timeout > Duration::ZERO && timeout < Duration::from_secs(1));
    // The first probe must become due in virtual time, without wall-clock sleep.
    tokio::time::advance(timeout).await;
    assert_eq!(a.conn.timeout(), Some(Duration::ZERO));
    a.conn.on_timeout();
    for _ in 0..50 {
        engine_tests::packets(&mut a, &mut b);
        b.step(Instant::now()).unwrap();
        engine_tests::packets(&mut b, &mut a);
        a.step(Instant::now()).unwrap();
        if b.rx.pending() == b"lost" {
            break;
        }
    }
    assert_eq!(b.rx.pending(), b"lost");
    b.delivered(4).unwrap();
    // Duplicated recovery traffic must not duplicate bytes in the RPC bridge.
    engine_tests::packets(&mut a, &mut b);
    b.step(Instant::now()).unwrap();
    assert!(b.rx.pending().is_empty());
}

#[tokio::test(start_paused = true)]
async fn idle_expiry_is_virtual_and_does_not_expire_a_new_generation() {
    let (mut a, mut b) = engine_tests::pair();
    establish(&mut a, &mut b);
    // The fixture drains the handshake/ACK flights, leaving only idle expiry.
    let timeout = a.conn.timeout().unwrap();
    assert_eq!(timeout, Duration::from_secs(10));
    tokio::time::advance(timeout - Duration::from_millis(1)).await;
    a.conn.on_timeout();
    assert!(!a.conn.is_closed());
    let (mut replacement, mut peer) = engine_tests::pair();
    establish(&mut replacement, &mut peer);
    tokio::time::advance(Duration::from_millis(1)).await;
    a.conn.on_timeout();
    assert!(a.conn.is_closed() && a.conn.is_timed_out());
    assert_eq!(
        a.step(Instant::now()).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    replacement.conn.on_timeout();
    assert!(replacement.step(Instant::now()).unwrap());
    assert!(!replacement.conn.is_closed());
}

#[tokio::test(start_paused = true)]
async fn pacing_migration_and_shutdown_deadlines_share_recovery_time() {
    tokio::time::advance(Duration::from_secs(600)).await;
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let peer_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let old = socket.local_addr().unwrap();
    let peer = peer_socket.local_addr().unwrap();
    let (mut a, mut b) = engine_tests::pair_at([old, peer], |_| {});
    establish(&mut a, &mut b);
    let mut socket = PacketSocket::Dedicated(socket.into());
    let (_mobility, mut migration) = mobility::pair();
    // Exchange CID allowances before requesting a new path.
    migration
        .step(&mut a.conn, &mut socket, Instant::now())
        .unwrap();
    engine_tests::packets(&mut a, &mut b);
    let mut peer_path = PacketSocket::Dedicated(peer_socket.into());
    let (_, mut peer_migration) = mobility::pair();
    peer_migration
        .step(&mut b.conn, &mut peer_path, Instant::now())
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
            candidate.into(),
            peer,
            Instant::now() + Duration::from_millis(30),
            reply,
        )),
        &mut a.conn,
        &socket,
        Instant::now(),
    );
    assert!(result.try_recv().is_err());
    let origin = Instant::now();
    for millis in [19, 20, 29, 30, 39, 40] {
        tokio::time::advance(origin + Duration::from_millis(millis) - Instant::now()).await;
        let now = Instant::now();
        migration.step(&mut a.conn, &mut socket, now).unwrap();
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
async fn outer_driver_observes_virtual_idle_and_shutdown_deadline_precedence() {
    for shutdown in [false, true] {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let peer = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (mut a, mut b) = engine_tests::pair_at(
            [socket.local_addr().unwrap(), peer.local_addr().unwrap()],
            |_| {},
        );
        establish(&mut a, &mut b);
        let deadline = a.conn.timeout().unwrap();
        assert_eq!(deadline, Duration::from_secs(10));
        let control = Control::new();
        if shutdown {
            control.begin(deadline).unwrap();
        }
        let (_application, io) = tokio::io::duplex(64);
        let driver = super::drive(
            PacketSocket::Dedicated(socket.into()),
            a.conn,
            io,
            super::SessionDrivers {
                established: None,
                datagrams: None,
                shutdown: Some(super::shutdown::ShutdownDriver::new(control.clone(), false)),
                mobility: None,
                scheduling: scheduling::pair().1,
            },
        );
        tokio::pin!(driver);
        assert!(driver.as_mut().now_or_never().is_none());
        tokio::time::advance(deadline).await;
        let error = driver
            .as_mut()
            .now_or_never()
            .expect("driver did not expire in virtual time")
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(
            error.to_string(),
            if shutdown {
                "Native shutdown acknowledgement timed out"
            } else {
                "Native session timed out"
            }
        );
        assert_eq!(
            control.wait().await.unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
    }
}

#[test]
fn replay_tlc_runtime_clock_and_replacement_deadlines() {
    use reproto_test_support::verification::exploration;
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
                let (mut old, mut peer) = engine_tests::pair();
                establish(&mut old, &mut peer);
                let mut replacement = None;
                for state in trace {
                    match state["event"] {
                        1 => tokio::time::advance(Duration::from_secs(5)).await,
                        2 => {
                            let (mut new, mut peer) = engine_tests::pair();
                            establish(&mut new, &mut peer);
                            replacement = Some(new);
                        }
                        3 => old.conn.on_timeout(),
                        4 => replacement.as_mut().unwrap().conn.on_timeout(),
                        other => panic!("unknown clock event: {other}"),
                    }
                    assert_eq!((Instant::now() - origin).as_secs(), 5 * state["now"]);
                    assert_eq!(old.conn.is_closed(), state["oldClosed"] == 1);
                    assert_eq!(old.conn.is_timed_out(), state["oldClosed"] == 1);
                    assert_eq!(replacement.is_some(), state["replacement"] == 1);
                    if let Some(new) = &replacement {
                        assert_eq!(new.conn.is_closed(), state["newClosed"] == 1);
                        assert_eq!(new.conn.is_timed_out(), state["newClosed"] == 1);
                    }
                }
            });
    }
}
