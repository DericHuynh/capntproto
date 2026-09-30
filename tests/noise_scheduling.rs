use reproto::{
    noise_listener::{self, Listener},
    noise_rpc::{Handle, Network},
    transport::{AuthenticatedSession, DatagramPacing, Identity, Schedule},
};
use reproto_test_support::runtime_test_capnp::harness;
use std::{net::SocketAddr, rc::Rc, time::Duration};
use tokio::time::Instant;

fn local() -> SocketAddr {
    "127.0.0.1:0".parse().unwrap()
}
async fn socket() -> tokio::net::UdpSocket {
    tokio::net::UdpSocket::bind(local()).await.unwrap()
}
struct Task<T>(tokio::task::JoinHandle<T>);
impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn pair(
    listener: &Listener,
    caller: &Identity,
) -> (AuthenticatedSession, AuthenticatedSession) {
    let reservation = listener
        .reserve(caller.public_key(), None, b"scheduling")
        .unwrap();
    let (a, b) = tokio::join!(
        noise_listener::connect(
            socket().await,
            reservation.target(),
            caller,
            None,
            b"scheduling"
        ),
        reservation.accept()
    );
    (a.unwrap(), b.unwrap())
}
struct Echo;
impl harness::Server for Echo {
    async fn echo(
        self: Rc<Self>,
        p: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        r.get().set_value(p.get()?.get_value());
        Ok(())
    }
    async fn generic(
        self: Rc<Self>,
        _: harness::GenericParams,
        mut r: harness::GenericResults,
    ) -> capnp::Result<()> {
        r.get().set_value(vec![42; 256 * 1024].as_slice())?;
        Ok(())
    }
}
async fn echo(cap: &harness::Client, value: u32) {
    let mut r = cap.echo_request();
    r.get().set_value(value);
    assert_eq!(
        r.send().promise.await.unwrap().get().unwrap().get_value(),
        value
    );
}
async fn until(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while !condition() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
fn network(identity: [u8; 32], arbitrated: bool) -> (Network, Handle) {
    if arbitrated {
        Network::with_arbitration(identity, None, Default::default()).unwrap()
    } else {
        Network::new(identity)
    }
}
fn paced() -> Schedule {
    Schedule {
        packet_burst: 1,
        datagrams: Some(DatagramPacing {
            interval: Duration::from_secs(1),
            burst: 1,
        }),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn paced_datagrams_preserve_rpc_mobility_and_policy_wakeup() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(30), async {
                for arbitrated in [false, true] {
                    let caller = Identity::generate();
                    let host = Rc::new(Identity::generate());
                    let listener = Listener::bind(local(), host.clone(), Default::default())
                        .await
                        .unwrap();
                    let (a, b) = pair(&listener, &caller).await;
                    let client_schedule = a.scheduling();
                    let server_schedule = b.scheduling();
                    client_schedule.configure(paced()).unwrap();
                    server_schedule.configure(paced()).unwrap();
                    let (an, ah) = network(caller.public_key(), arbitrated);
                    let (bn, bh) = network(host.public_key(), arbitrated);
                    assert!(ah.scheduling(host.public_key()).is_none());
                    ah.attach(a).unwrap();
                    bh.attach(b).unwrap();
                    let service: harness::Client = capnp_rpc::new_client(Echo);
                    let _host = Task(tokio::task::spawn_local(capnp_rpc::RpcSystem::new(
                        Box::new(bn),
                        Some(service.client),
                    )));
                    let mut rpc = capnp_rpc::RpcSystem::new(Box::new(an), None);
                    let cap: harness::Client = rpc.bootstrap(host.public_key());
                    let _caller = Task(tokio::task::spawn_local(rpc));
                    echo(&cap, 0).await;
                    assert_eq!(
                        ah.scheduling(host.public_key()).unwrap().policy().unwrap(),
                        paced()
                    );
                    let lane = ah.take_datagrams(host.public_key()).unwrap();
                    let mut incoming = bh.take_datagrams(caller.public_key()).unwrap();
                    let sender = lane.sender();
                    let started = Instant::now();
                    for _ in 0..64 {
                        sender.try_send(&[7; 1024]).unwrap();
                    }
                    assert_eq!(
                        sender.try_send(b"full").unwrap_err().kind(),
                        std::io::ErrorKind::WouldBlock
                    );
                    until(|| client_schedule.stats().unwrap().datagram_attempts > 0).await;
                    for value in 1..16 {
                        echo(&cap, value).await;
                    }
                    let response = cap.generic_request().send().promise.await.unwrap();
                    assert_eq!(
                        response.get().unwrap().get_value().unwrap(),
                        &vec![42; 256 * 1024]
                    );
                    assert!(server_schedule.stats().unwrap().burst_yields > 0);
                    // Account for elapsed time instead of assuming a latency bound
                    // from the scheduler, OS or CI worker.
                    let attempts = client_schedule.stats().unwrap().datagram_attempts;
                    assert!(attempts <= 1 + started.elapsed().as_secs());
                    assert!(attempts < 64, "pacing did not constrain admission");
                    assert_eq!(incoming.recv().await.unwrap(), vec![7; 1024]);
                    let generation = ah.observe_route(host.public_key()).unwrap().generation();
                    let mobility = ah.mobility(host.public_key()).unwrap();
                    mobility.rotate_connection_id().await.unwrap();
                    mobility
                        .migrate(
                            socket().await,
                            listener.local_addr().unwrap(),
                            Duration::from_secs(2),
                        )
                        .await
                        .unwrap();
                    echo(&cap, 99).await;
                    assert_eq!(
                        ah.observe_route(host.public_key()).unwrap().generation(),
                        generation
                    );
                    // Wake an idle credit waiter and drain admission without any
                    // subsequent RPC/UDP input being needed to notice this update.
                    client_schedule
                        .configure(Schedule {
                            packet_burst: 1,
                            datagrams: None,
                        })
                        .unwrap();
                    until(|| client_schedule.stats().unwrap().datagram_attempts == 64).await;
                    assert_eq!(
                        ah.scheduling(host.public_key()).unwrap().stats().unwrap(),
                        client_schedule.stats().unwrap()
                    );
                }
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn queued_datagrams_do_not_delay_shutdown_and_old_controls_cannot_configure_reconnect() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(8), async {
                let caller = Identity::generate();
                let host = Rc::new(Identity::generate());
                let listener = Listener::bind(local(), host.clone(), Default::default())
                    .await
                    .unwrap();
                let (a, b) = pair(&listener, &caller).await;
                let old = a.scheduling();
                old.configure(paced()).unwrap();
                let (an, ah) = Network::new(caller.public_key());
                let (bn, bh) = Network::new(host.public_key());
                ah.attach(a).unwrap();
                bh.attach(b).unwrap();
                let service: harness::Client = capnp_rpc::new_client(Echo);
                let _host = Task(tokio::task::spawn_local(capnp_rpc::RpcSystem::new(
                    Box::new(bn),
                    Some(service.client),
                )));
                let mut rpc = capnp_rpc::RpcSystem::new(Box::new(an), None);
                let cap: harness::Client = rpc.bootstrap(host.public_key());
                let _caller = Task(tokio::task::spawn_local(rpc));
                echo(&cap, 1).await;
                let lane = ah.take_datagrams(host.public_key()).unwrap();
                for _ in 0..64 {
                    lane.sender().try_send(&[9; 1024]).unwrap();
                }
                until(|| old.stats().unwrap().datagram_attempts > 0).await;
                ah.shutdown(host.public_key(), Duration::from_secs(2))
                    .await
                    .unwrap();
                until(|| old.stats().is_err()).await;
                assert!(ah.scheduling(host.public_key()).is_none());
                assert_eq!(
                    lane.sender().try_send(b"closed").unwrap_err().kind(),
                    std::io::ErrorKind::BrokenPipe
                );
                // Both endpoints explicitly forget the old route before reattaching.
                ah.disconnect(host.public_key());
                bh.disconnect(caller.public_key());
                let (a, b) = pair(&listener, &caller).await;
                let fresh = a.scheduling();
                assert_eq!(fresh.policy().unwrap(), Schedule::default());
                assert_eq!(
                    old.configure(paced()).unwrap_err().kind(),
                    std::io::ErrorKind::BrokenPipe
                );
                ah.attach(a).unwrap();
                bh.attach(b).unwrap();
                assert_eq!(
                    ah.scheduling(host.public_key()).unwrap().policy().unwrap(),
                    Schedule::default()
                );
                ah.disconnect(host.public_key());
                bh.disconnect(caller.public_key());
                until(|| fresh.stats().is_err()).await;
                // Retaining either control does not keep listener routes alive.
                until(|| listener.stats().authenticated == 0).await;
            })
            .await
            .unwrap();
        })
        .await;
}
