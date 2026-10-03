use capntproto::{
    native_discovery::{self, Binding, Directory, DiscoveryConnector, DiscoveryGeneration},
    native_listener::{self, Limits, Listener},
    native_provisioning::Provisioner,
    native_rpc::{Connector, Network},
    transport::Identity,
};
use capntproto_test_support::runtime_test_capnp::harness;
use std::{cell::RefCell, net::SocketAddr, rc::Rc, time::Duration};
use tokio::net::UdpSocket;

fn local() -> SocketAddr {
    "127.0.0.1:0".parse().unwrap()
}
async fn socket() -> UdpSocket {
    UdpSocket::bind(local()).await.unwrap()
}
struct Task<T>(tokio::task::JoinHandle<T>);
impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
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
}
async fn echo(cap: &harness::Client, value: u32) {
    let mut request = cap.echo_request();
    request.get().set_value(value);
    assert_eq!(
        request
            .send()
            .promise
            .await
            .unwrap()
            .get()
            .unwrap()
            .get_value(),
        value
    );
}
async fn stun() -> (SocketAddr, Task<()>, Rc<RefCell<Vec<SocketAddr>>>) {
    let server = socket().await;
    let address = server.local_addr().unwrap();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    let task = Task(tokio::task::spawn_local(async move {
        let mut packet = [0; 100];
        loop {
            let (n, from) = server.recv_from(&mut packet).await.unwrap();
            assert_eq!(n, 20);
            log.borrow_mut().push(from);
            let std::net::IpAddr::V4(ip) = from.ip() else {
                panic!()
            };
            let mut out = packet[..20].to_vec();
            out[0] = 1;
            out[2..4].copy_from_slice(&12u16.to_be_bytes());
            out.extend_from_slice(&[0, 0x20, 0, 8, 0, 1]);
            out.extend_from_slice(&(from.port() ^ 0x2112).to_be_bytes());
            out.extend(
                ip.octets()
                    .into_iter()
                    .zip([0x21, 0x12, 0xa4, 0x42])
                    .map(|(a, b)| a ^ b),
            );
            server.send_to(&out, from).await.unwrap();
            // Duplicate discovery replies can arrive after the caller hands
            // this socket to Native. They must not fail that handshake/session.
            server.send_to(&out, from).await.unwrap();
        }
    }));
    (address, task, seen)
}

#[tokio::test(flavor = "current_thread")]
async fn discovered_rendezvous_establishes_native_rpc_and_follows_name_key_rotation() {
    tokio::task::LocalSet::new().run_until(async {
        tokio::time::timeout(Duration::from_secs(8),async {
            let (stun,_stun,seen)=stun().await;
            let recipient=Rc::new(Identity::generate());
            let directory=Directory::default();
            // Exercise generated discovery calls over an RPC connection.
            let (a,b)=tokio::io::duplex(4096);
            let _directory=Task(capntproto::rpc::serve(b,directory.client(recipient.public_key()).client));
            let (reader,driver):(capntproto::native_discovery_capnp::directory::Client,_)=capntproto::rpc::client(a);
            let _reader=Task(driver);
            let connector=Rc::new(DiscoveryConnector::new(recipient.clone(),local(),reader.clone(),Some(stun)).unwrap());
            let (network,handle)=Network::with_connector(recipient.public_key(),connector.clone());
            let mut rpc=capnp_rpc::RpcSystem::new(Box::new(network),None);
            // bootstrap() needs the mutable RpcSystem only while creating each
            // client; its event loop is polled alongside those clients below.
            let mut generation=None;let mut old_host=None;
            for value in [41,42] {
                let host=Rc::new(Identity::generate());
                let (listener,advertised)=Listener::bind_discovered(local(),host.clone(),Limits::default(),stun).await.unwrap();
                assert_eq!(advertised,listener.local_addr().unwrap());
                let (hn,hh)=Network::new(host.public_key());
                let provider=Provisioner::with_rendezvous(listener.clone(),hh,recipient.public_key(),advertised,b"directory test").unwrap();
                let previous=generation;
                generation=Some(directory.publish("object-service",Binding {host:host.public_key(),recipient:recipient.public_key(),address:advertised,context:b"directory test".to_vec(),provider:provider.client()},generation,Duration::from_secs(30)).unwrap());
                assert!(generation>previous);
                if let Some(previous) = previous { assert!(!directory.revoke("object-service",recipient.public_key(),previous)); }
                let resolved=native_discovery::resolve(&reader,recipient.public_key(),"object-service").await.unwrap();
                assert_eq!(resolved.binding().host,host.public_key());assert_eq!(Some(resolved.generation()),generation);
                if let Some(old)=old_host { assert!(connector.connect(old).await.is_err()); }
                let cap:harness::Client=rpc.bootstrap(resolved.binding().host);
                let service:harness::Client=capnp_rpc::new_client(Echo);
                let _host=Task(tokio::task::spawn_local(capnp_rpc::RpcSystem::new(Box::new(hn),Some(service.client))));
                tokio::select! {result=&mut rpc=>panic!("caller ended: {result:?}"), _=echo(&cap,value)=>()}
                assert_eq!(listener.stats().authenticated,1);
                assert!(handle.mobility(host.public_key()).is_some());
                old_host=Some(host.public_key());
                handle.disconnect(host.public_key());
            }
            // One mapping for each host listener and each recipient dial.
            assert_eq!(seen.borrow().len(),4);
        }).await.unwrap();
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn path_migration_and_cid_rotation_preserve_rpc_generation_and_datagrams() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(8), async {
                let host = Rc::new(Identity::generate());
                let client = Identity::generate();
                let listener = Listener::bind(local(), host.clone(), Limits::default())
                    .await
                    .unwrap();
                let reservation = listener
                    .reserve(client.public_key(), None, b"mobility")
                    .unwrap();
                let target = reservation.target();
                let (a, b) = tokio::join!(
                    native_listener::connect(socket().await, target, &client, None, b"mobility"),
                    reservation.accept()
                );
                let (mut a, mut b) = (a.unwrap(), b.unwrap());
                let ap = a.take_datagrams().unwrap();
                let mut bp = b.take_datagrams().unwrap();
                let server_control = b.mobility();
                let (an, ah) = Network::new(client.public_key());
                let (bn, bh) = Network::new(host.public_key());
                ah.attach(a).unwrap();
                bh.attach(b).unwrap();
                let generation = ah.observe_route(host.public_key()).unwrap().generation();
                let control = ah.mobility(host.public_key()).unwrap();
                let service: harness::Client = capnp_rpc::new_client(Echo);
                let _server = Task(tokio::task::spawn_local(capnp_rpc::RpcSystem::new(
                    Box::new(bn),
                    Some(service.client),
                )));
                let mut rpc = capnp_rpc::RpcSystem::new(Box::new(an), None);
                let cap = rpc.bootstrap(host.public_key());
                let _client = Task(tokio::task::spawn_local(rpc));
                echo(&cap, 1).await;
                control.rotate_connection_id().await.unwrap();
                server_control.rotate_connection_id().await.unwrap();
                // RPC forces advertised IDs and retirements through both drivers.
                echo(&cap, 2).await;
                let candidate = socket().await;
                let new_address = candidate.local_addr().unwrap();
                let path = control
                    .migrate(candidate, target.address, Duration::from_secs(2))
                    .await
                    .unwrap();
                assert_eq!(path.local, new_address);
                assert_eq!(path.peer, target.address);
                echo(&cap, 3).await;
                assert_eq!(
                    ah.observe_route(host.public_key()).unwrap().generation(),
                    generation
                );
                ap.sender().try_send(b"same authenticated session").unwrap();
                assert_eq!(bp.recv().await.unwrap(), b"same authenticated session");
                // A silent candidate cannot replace the validated route.
                let sink = socket().await;
                assert!(control
                    .migrate(
                        socket().await,
                        sink.local_addr().unwrap(),
                        Duration::from_millis(80)
                    )
                    .await
                    .is_err());
                echo(&cap, 4).await;
                // Cancellation before proof closes the candidate socket and leaves
                // the current RPC generation usable.
                use futures::FutureExt;
                let mut canceled = Box::pin(control.migrate(
                    socket().await,
                    sink.local_addr().unwrap(),
                    Duration::from_secs(1),
                ));
                assert!(canceled.as_mut().now_or_never().is_none());
                for _ in 0..16 {
                    tokio::task::yield_now().await;
                }
                drop(canceled);
                tokio::time::sleep(Duration::from_millis(30)).await;
                echo(&cap, 5).await;
                assert!(server_control
                    .migrate(socket().await, target.address, Duration::from_secs(1))
                    .await
                    .is_err());
                listener.close();
                assert_eq!(listener.stats().authenticated, 0);
            })
            .await
            .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn maintained_mapping_refresh_failure_recovery_and_stop_preserve_rpc() {
    use capntproto::nat::{MappingOptions, MappingStatus};
    use std::cell::Cell;
    tokio::task::LocalSet::new()
        .run_until(async {
            let server = socket().await;
            let stun_address = server.local_addr().unwrap();
            // 0: real socket mapping; 1: blackhole; 2: report a changed address.
            let mode = Rc::new(Cell::new(0));
            let requests = Rc::new(RefCell::new(Vec::new()));
            let m = mode.clone();
            let seen = requests.clone();
            let changed_address: SocketAddr = "127.0.0.1:32123".parse().unwrap();
            let _stun = Task(tokio::task::spawn_local(async move {
                let mut packet = [0; 1025];
                loop {
                    let (n, from) = server.recv_from(&mut packet).await.unwrap();
                    assert_eq!(n, 20);
                    seen.borrow_mut().push((from, packet[8..20].to_vec()));
                    if m.get() == 1 {
                        continue;
                    }
                    let address = if m.get() == 2 { changed_address } else { from };
                    let std::net::IpAddr::V4(ip) = address.ip() else {
                        panic!()
                    };
                    let mut out = packet[..20].to_vec();
                    out[0] = 1;
                    out[2..4].copy_from_slice(&12u16.to_be_bytes());
                    out.extend_from_slice(&[0, 0x20, 0, 8, 0, 1]);
                    out.extend_from_slice(&(address.port() ^ 0x2112).to_be_bytes());
                    out.extend(
                        ip.octets()
                            .into_iter()
                            .zip([0x21, 0x12, 0xa4, 0x42])
                            .map(|(a, b)| a ^ b),
                    );
                    server.send_to(&out, from).await.unwrap();
                }
            }));
            let host = Rc::new(Identity::generate());
            let client = Identity::generate();
            let listener = Listener::bind(local(), host.clone(), Limits::default())
                .await
                .unwrap();
            let mut mapping = listener
                .maintain_mapping(stun_address, MappingOptions::default())
                .unwrap();
            assert!(listener
                .maintain_mapping(stun_address, MappingOptions::default())
                .is_err());
            assert!(matches!(
                tokio::time::timeout(Duration::from_secs(2), mapping.changed())
                    .await
                    .unwrap(),
                MappingStatus::Observed { .. }
            ));
            assert_eq!(mapping.address(), Some(listener.local_addr().unwrap()));
            let reservation = listener
                .reserve(client.public_key(), None, b"maintenance")
                .unwrap();
            let target = reservation.target();
            let (a, b) = tokio::join!(
                native_listener::connect(socket().await, target, &client, None, b"maintenance"),
                reservation.accept()
            );
            let (an, ah) = Network::new(client.public_key());
            let (bn, bh) = Network::new(host.public_key());
            ah.attach(a.unwrap()).unwrap();
            bh.attach(b.unwrap()).unwrap();
            let generation = ah.observe_route(host.public_key()).unwrap().generation();
            let service: harness::Client = capnp_rpc::new_client(Echo);
            let _server = Task(tokio::task::spawn_local(capnp_rpc::RpcSystem::new(
                Box::new(bn),
                Some(service.client),
            )));
            let mut rpc = capnp_rpc::RpcSystem::new(Box::new(an), None);
            let cap = rpc.bootstrap(host.public_key());
            let _client = Task(tokio::task::spawn_local(rpc));
            echo(&cap, 0).await;
            // Jump the logical refresh clock while regularly exchanging actual UDP
            // RPC packets, keeping the existing transport's 10s idle lease alive.
            // Resume time before waiting on OS I/O to avoid Tokio auto-advance races.
            for phase in 0..3 {
                mode.set(phase);
                for value in 0..11 {
                    tokio::time::pause();
                    tokio::time::advance(Duration::from_secs(2)).await;
                    for _ in 0..32 {
                        tokio::task::yield_now().await;
                    }
                    tokio::time::resume();
                    tokio::time::timeout(Duration::from_secs(2), echo(&cap, value))
                        .await
                        .unwrap();
                }
                match phase {
                    0 => assert_eq!(mapping.address(), Some(target.address)),
                    1 => assert_eq!(mapping.status(), MappingStatus::Unavailable),
                    _ => assert_eq!(mapping.address(), Some(changed_address)),
                }
                assert_eq!(
                    ah.observe_route(host.public_key()).unwrap().generation(),
                    generation
                );
            }
            {
                let seen = requests.borrow();
                assert!(seen.len() >= 4);
                assert!(seen.iter().all(|(from, _)| *from == target.address));
                assert!(seen.windows(2).any(|p| p[0].1 != p[1].1));
                assert!(seen.windows(2).any(|p| p[0].1 == p[1].1)); // retransmissions
            }
            mapping.close();
            // A socket error from an auxiliary probe must not close RPC. This
            // IPv4 socket cannot send to an IPv6 STUN destination.
            let mut broken = listener
                .maintain_mapping("[::1]:12345".parse().unwrap(), MappingOptions::default())
                .unwrap();
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), broken.changed())
                    .await
                    .unwrap(),
                MappingStatus::Unavailable
            );
            echo(&cap, 98).await;
            drop(broken);
            let mut next = listener
                .maintain_mapping(stun_address, MappingOptions::default())
                .unwrap();
            drop(mapping); // stale owner must not stop the replacement
            assert!(matches!(
                tokio::time::timeout(Duration::from_secs(2), next.changed())
                    .await
                    .unwrap(),
                MappingStatus::Observed { .. }
            ));
            listener.stop_accepting();
            assert!(listener
                .maintain_mapping(stun_address, MappingOptions::default())
                .is_err());
            assert!(next.address().is_some());
            echo(&cap, 99).await;
            listener.close();
            assert_eq!(next.changed().await, MappingStatus::Stopped);
            assert_eq!(next.address(), None);
            let other = Listener::bind(local(), host, Limits::default())
                .await
                .unwrap();
            let mut observing = other
                .maintain_mapping(stun_address, MappingOptions::default())
                .unwrap();
            drop(other);
            assert_eq!(observing.changed().await, MappingStatus::Stopped);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn mapped_advertisements_follow_outages_and_new_endpoints_without_replaying_calls() {
    use capntproto::{
        nat::MappingOptions,
        native_discovery::{
            AdvertisementOptions, AdvertisementStatus, AdvertisementStop, MappedService,
        },
    };
    use std::cell::Cell;
    tokio::task::LocalSet::new().run_until(async {
        let host=Rc::new(Identity::generate());
        let a=Rc::new(Identity::generate()); let b=Identity::generate(); let c=Rc::new(Identity::generate());
        let listener=Listener::bind(local(),host.clone(),Limits { reservation_timeout: Duration::from_secs(60), ..Limits::default() }).await.unwrap();
        let endpoint=listener.local_addr().unwrap();
        let advertised=Rc::new(Cell::new(Some(endpoint)));
        let reporting=advertised.clone();
        let stun=socket().await;
        let stun_address=stun.local_addr().unwrap();
        let _stun=Task(tokio::task::spawn_local(async move {
            let mut packet=[0;1025];
            loop {
                let (n,from)=stun.recv_from(&mut packet).await.unwrap();
                assert_eq!(n,20); assert_eq!(from,endpoint);
                let Some(address)=reporting.get() else { continue };
                let std::net::IpAddr::V4(ip)=address.ip() else { panic!() };
                let mut response=packet[..20].to_vec(); response[0]=1;
                response[2..4].copy_from_slice(&12u16.to_be_bytes());
                response.extend_from_slice(&[0,0x20,0,8,0,1]);
                response.extend_from_slice(&(address.port()^0x2112).to_be_bytes());
                response.extend(ip.octets().into_iter().zip([0x21,0x12,0xa4,0x42]).map(|(a,b)|a^b));
                stun.send_to(&response,from).await.unwrap();
            }
        }));
        let mut mapping=listener.maintain_mapping(stun_address,MappingOptions::default()).unwrap();
        let (hn,hh)=Network::new(host.public_key());
        let service=MappedService::new(listener.clone(),hh,&mapping).unwrap();
        let directory=Directory::default();
        assert!(service.advertise(&directory,"service",a.public_key(),b"mapped",AdvertisementOptions::default()).is_err());
        tokio::time::timeout(Duration::from_secs(2),mapping.changed()).await.unwrap();
        let other=Listener::bind(local(),host.clone(),Limits::default()).await.unwrap();
        let (_wrong_network,wrong_handle)=Network::new(host.public_key());
        assert!(MappedService::new(other,wrong_handle,&mapping).is_err());
        let options=AdvertisementOptions { lifetime: Duration::from_secs(60), rendezvous:true, ..AdvertisementOptions::default() };
        let ad_a=service.advertise(&directory,"service",a.public_key(),b"mapped",options).unwrap();
        let ad_b=service.advertise(&directory,"service",b.public_key(),b"mapped",options).unwrap();
        let ad_c=service.advertise(&directory,"service",c.public_key(),b"mapped",options).unwrap();
        // Real generated directory RPC over an independent control connection.
        let (left,right)=tokio::io::duplex(4096);
        let _directory=Task(capntproto::rpc::serve(right,directory.client(a.public_key()).client));
        let (reader,driver):(capntproto::native_discovery_capnp::directory::Client,_)=capntproto::rpc::client(left);
        let _reader=Task(driver);
        let connector=Rc::new(DiscoveryConnector::new(a.clone(),local(),reader.clone(),None).unwrap());
        let (an,ah)=Network::with_connector(a.public_key(),connector);
        let mut rpc=capnp_rpc::RpcSystem::new(Box::new(an),None);
        let cap=rpc.bootstrap(host.public_key());
        let _caller=Task(tokio::task::spawn_local(rpc));
        let echo_server:harness::Client=capnp_rpc::new_client(Echo);
        let _host=Task(tokio::task::spawn_local(capnp_rpc::RpcSystem::new(Box::new(hn),Some(echo_server.client))));
        tokio::time::timeout(Duration::from_secs(3),echo(&cap,1)).await.unwrap();
        let generation=ah.observe_route(host.public_key()).unwrap().generation();
        let old_b=native_discovery::resolve(&directory.client(b.public_key()),b.public_key(),"service").await.unwrap().binding().provider.clone();
        let pending=old_b.reserve_request().send().promise.await.unwrap();
        let pending=pending.get().unwrap().get_lease().unwrap();
        let reserved_at=tokio::time::Instant::now();
        advertised.set(None);
        for value in 0..11 {
            tokio::time::pause(); tokio::time::advance(Duration::from_secs(2)).await;
            for _ in 0..32 { tokio::task::yield_now().await; }
            tokio::time::resume();
            tokio::time::timeout(Duration::from_secs(2),echo(&cap,value)).await.unwrap();
        }
        assert!(matches!(ad_a.status(),AdvertisementStatus::Suspended { .. }));
        assert!(matches!(ad_b.status(),AdvertisementStatus::Suspended { .. }));
        assert!(native_discovery::resolve(&reader,a.public_key(),"service").await.is_err());
        assert!(native_discovery::resolve(&directory.client(b.public_key()),b.public_key(),"service").await.is_err());
        assert!(pending.ready_request().send().promise.await.is_err());
        assert!(tokio::time::Instant::now()-reserved_at<Duration::from_secs(60));
        assert!(old_b.reserve_request().send().promise.await.is_err());
        assert!(directory.revoke("service",b.public_key(),ad_b.generation()));
        for _ in 0..16 { tokio::task::yield_now().await; }
        assert!(matches!(ad_b.status(),AdvertisementStatus::Stopped(AdvertisementStop::Publication(_))));

        // A loopback UDP forwarder gives the replacement mapping a genuinely
        // different, usable endpoint. Native still pins the original host key.
        let proxy=socket().await;
        let proxy_address=proxy.local_addr().unwrap();
        let _proxy=Task(tokio::task::spawn_local(async move {
            let mut packet=[0;1500]; let mut peer=None;
            loop {
                let (n,from)=proxy.recv_from(&mut packet).await.unwrap();
                if from==endpoint {
                    if let Some(peer)=peer { proxy.send_to(&packet[..n],peer).await.unwrap(); }
                } else {
                    peer=Some(from); proxy.send_to(&packet[..n],endpoint).await.unwrap();
                }
            }
        }));
        advertised.set(Some(proxy_address));
        for value in 11..22 {
            tokio::time::pause(); tokio::time::advance(Duration::from_secs(2)).await;
            for _ in 0..32 { tokio::task::yield_now().await; }
            tokio::time::resume();
            tokio::time::timeout(Duration::from_secs(2),echo(&cap,value)).await.unwrap();
        }
        assert!(matches!(ad_a.status(),AdvertisementStatus::Published { address, .. } if address==proxy_address));
        assert!(matches!(ad_c.status(),AdvertisementStatus::Published { address, .. } if address==proxy_address));
        assert!(native_discovery::resolve(&directory.client(b.public_key()),b.public_key(),"service").await.is_err());
        let current=native_discovery::resolve(&directory.client(c.public_key()),c.public_key(),"service").await.unwrap();
        assert_eq!(current.binding().address,proxy_address);
        let provider_c=current.binding().provider.clone();
        let connector=Rc::new(DiscoveryConnector::new(c.clone(),local(),directory.client(c.public_key()),None).unwrap());
        let (cn,ch)=Network::with_connector(c.public_key(),connector);
        let mut rpc=capnp_rpc::RpcSystem::new(Box::new(cn),None);
        let direct=rpc.bootstrap(host.public_key());
        let _c=Task(tokio::task::spawn_local(rpc));
        tokio::time::timeout(Duration::from_secs(3),echo(&direct,42)).await.unwrap();
        assert!(ch.mobility(host.public_key()).is_some());
        assert_eq!(ah.observe_route(host.public_key()).unwrap().generation(),generation);
        drop(ad_a);
        assert!(native_discovery::resolve(&reader,a.public_key(),"service").await.is_err());
        echo(&cap,99).await;
        assert!(native_discovery::resolve(&directory.client(c.public_key()),c.public_key(),"service").await.is_ok());
        drop(mapping);
        for _ in 0..16 { tokio::task::yield_now().await; }
        assert_eq!(ad_c.status(),AdvertisementStatus::Stopped(AdvertisementStop::Mapping));
        assert!(native_discovery::resolve(&directory.client(c.public_key()),c.public_key(),"service").await.is_err());
        assert!(provider_c.reserve_request().send().promise.await.is_err());
        echo(&direct,100).await;
        assert_eq!(listener.stats().authenticated,2);
    }).await;
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn directory_expiry_cas_revocation_recipient_scope_and_capacity() {
    tokio::task::LocalSet::new()
        .run_until(async {
            struct Never;
            impl capntproto::native_provisioning_capnp::provisioner::Server for Never {}
            let directory = Directory::default();
            let binding = Binding {
                host: [1; 32],
                recipient: [2; 32],
                address: "127.0.0.1:12345".parse().unwrap(),
                context: vec![],
                provider: capnp_rpc::new_client(Never),
            };
            let first = directory
                .publish("service", binding.clone(), None, Duration::from_secs(1))
                .unwrap();
            assert!(
                native_discovery::resolve(&directory.client([3; 32]), [3; 32], "service")
                    .await
                    .is_err()
            );
            let _ = native_discovery::resolve(&directory.client([2; 32]), [2; 32], "service")
                .await
                .unwrap();
            tokio::time::advance(Duration::from_secs(1)).await;
            assert!(
                native_discovery::resolve(&directory.client([2; 32]), [2; 32], "service")
                    .await
                    .is_err()
            );
            assert!(directory
                .publish("service", binding.clone(), None, Duration::from_secs(1))
                .is_err());
            let next = directory
                .publish(
                    "service",
                    binding.clone(),
                    Some(first),
                    Duration::from_secs(1),
                )
                .unwrap();
            assert!(!directory.revoke("service", [2; 32], first));
            assert!(directory.revoke("service", [2; 32], next));
            for i in 1..=64 {
                let mut b = binding.clone();
                b.host[0] = i;
                b.host[1] = 3;
                let _ = directory
                    .publish(&format!("service/{i}"), b, None, Duration::from_secs(1))
                    .unwrap();
            }
            assert!(directory
                .publish("overflow", binding, None, Duration::from_secs(1))
                .is_err());
            directory.close();
            assert!(
                native_discovery::resolve(&directory.client([2; 32]), [2; 32], "service/1")
                    .await
                    .is_err()
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn advertisement_shutdown_and_replacement_notify_and_preserve_successors() {
    use capntproto::{
        nat::MappingOptions,
        native_discovery::{
            AdvertisementOptions, AdvertisementStatus, AdvertisementStop, MappedService,
        },
    };
    tokio::task::LocalSet::new()
        .run_until(async {
            let (stun, _stun, _seen) = stun().await;
            for cause in 0..3 {
                let identity = Rc::new(Identity::generate());
                let recipient = Identity::generate();
                let listener = Listener::bind(local(), identity.clone(), Limits::default())
                    .await
                    .unwrap();
                let mut mapping = listener
                    .maintain_mapping(stun, MappingOptions::default())
                    .unwrap();
                tokio::time::timeout(Duration::from_secs(2), mapping.changed())
                    .await
                    .unwrap();
                let (network, handle) = Network::new(identity.public_key());
                let directory = Directory::default();
                let service =
                    MappedService::new(listener.clone(), handle.clone(), &mapping).unwrap();
                let mut owner = service
                    .advertise(
                        &directory,
                        "service",
                        recipient.public_key(),
                        b"lifecycle",
                        AdvertisementOptions::default(),
                    )
                    .unwrap();
                let old = native_discovery::resolve(
                    &directory.client(recipient.public_key()),
                    recipient.public_key(),
                    "service",
                )
                .await
                .unwrap()
                .binding()
                .provider
                .clone();
                let mut punch = old.reserve_request();
                punch.get().set_rendezvous_address("127.0.0.1:12345");
                assert!(punch.send().promise.await.is_err()); // opt-in authority
                let lease = old.reserve_request().send().promise.await.unwrap();
                let lease = lease.get().unwrap().get_lease().unwrap();
                let expected = match cause {
                    0 => {
                        listener.stop_accepting();
                        AdvertisementStop::Listener
                    }
                    1 => {
                        handle.shutdown_all(Duration::from_secs(1)).await.unwrap();
                        AdvertisementStop::Network
                    }
                    _ => {
                        let provider = Provisioner::new(
                            listener.clone(),
                            handle.clone(),
                            recipient.public_key(),
                            listener.local_addr().unwrap(),
                            b"successor",
                        )
                        .unwrap();
                        let _ = directory
                            .publish(
                                "service",
                                Binding {
                                    host: identity.public_key(),
                                    recipient: recipient.public_key(),
                                    address: listener.local_addr().unwrap(),
                                    context: b"successor".to_vec(),
                                    provider: provider.client(),
                                },
                                Some(owner.generation()),
                                Duration::from_secs(30),
                            )
                            .unwrap();
                        AdvertisementStop::Publication(
                            native_discovery::PublicationStatus::Superseded,
                        )
                    }
                };
                assert_eq!(
                    tokio::time::timeout(Duration::from_secs(2), owner.changed())
                        .await
                        .unwrap(),
                    AdvertisementStatus::Stopped(expected)
                );
                assert!(lease.ready_request().send().promise.await.is_err());
                assert!(old.reserve_request().send().promise.await.is_err());
                drop(owner);
                let successor = native_discovery::resolve(
                    &directory.client(recipient.public_key()),
                    recipient.public_key(),
                    "service",
                )
                .await;
                if cause == 2 {
                    let successor = successor.unwrap();
                    assert_eq!(successor.binding().context, b"successor");
                    let lease = successor
                        .binding()
                        .provider
                        .reserve_request()
                        .send()
                        .promise
                        .await
                        .unwrap();
                    lease
                        .get()
                        .unwrap()
                        .get_lease()
                        .unwrap()
                        .cancel_request()
                        .send()
                        .promise
                        .await
                        .unwrap();
                } else {
                    assert!(successor.is_err());
                }
                assert!(mapping.address().is_some());
                drop(network);
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn replay_tlc_discovery_publication_and_rotation() {
    use capntproto_test_support::verification::exploration;
    let config = include_str!("../verification/NativeDiscovery.cfg");
    let live =
        config.replace("SPECIFICATION Spec", "SPECIFICATION LiveSpec") + "\nPROPERTY LeaseEnds\n";
    exploration::controls(
        "verification/NativeDiscovery.tla",
        "native-discovery",
        config,
        &[
            ("staleRevoke", "Generation"),
            ("expiry", "Expiry"),
            ("recipient", "Recipient"),
        ],
        Some(&live),
    )
    .unwrap();
    let traces = exploration::traces(
        "verification/NativeDiscovery.tla",
        "native-discovery",
        config,
    )
    .unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            struct Never;
            impl capntproto::native_provisioning_capnp::provisioner::Server for Never {}
            for trace in traces {
                let directory = Directory::default();
                let mut generation = None;
                let mut host = 1;
                let binding = |host| Binding {
                    host: [host; 32],
                    recipient: [3; 32],
                    address: "127.0.0.1:1234".parse().unwrap(),
                    context: vec![],
                    provider: capnp_rpc::new_client(Never),
                };
                let mut read = 0;
                for state in trace {
                    match state["event"] {
                        1 => {
                            generation = Some(
                                directory
                                    .publish("service", binding(1), None, Duration::from_secs(1))
                                    .unwrap(),
                            )
                        }
                        2 | 3 => {
                            host = if state["event"] == 2 { 1 } else { 2 };
                            generation = Some(
                                directory
                                    .publish(
                                        "service",
                                        binding(host),
                                        generation,
                                        Duration::from_secs(1),
                                    )
                                    .unwrap(),
                            );
                        }
                        4 => tokio::time::advance(Duration::from_secs(1)).await,
                        5 => assert!(directory.revoke("service", [3; 32], generation.unwrap())),
                        6 => assert!(!directory.revoke(
                            "service",
                            [3; 32],
                            DiscoveryGeneration::new(1).unwrap()
                        )),
                        7 => {
                            read = native_discovery::resolve(
                                &directory.client([3; 32]),
                                [3; 32],
                                "service",
                            )
                            .await
                            .map_or(0, |r| r.binding().host[0] as u64)
                        }
                        8 => directory.close(),
                        9 => {
                            read = native_discovery::resolve(
                                &directory.client([4; 32]),
                                [4; 32],
                                "service",
                            )
                            .await
                            .map_or(0, |r| r.binding().host[0] as u64)
                        }
                        _ => panic!("unexpected discovery action"),
                    }
                    assert_eq!(
                        generation.map_or(0, DiscoveryGeneration::get),
                        state["generation"]
                    );
                    assert_eq!(read, state["read"]);
                    let result =
                        native_discovery::resolve(&directory.client([3; 32]), [3; 32], "service")
                            .await;
                    assert_eq!(
                        result.is_ok(),
                        state["active"] == 1 && state["expired"] == 0 && state["closed"] == 0
                    );
                    if let Ok(result) = result {
                        assert_eq!(result.binding().host, [host; 32]);
                        assert_eq!(Some(result.generation()), generation);
                    }
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_rendezvous_consent_authentication_and_retirement() {
    use capntproto_test_support::verification::exploration;
    let config = include_str!("../verification/NativeRendezvous.cfg");
    let live =
        config.replace("SPECIFICATION Spec", "SPECIFICATION LiveSpec") + "\nPROPERTY Settles\n";
    exploration::controls(
        "verification/NativeRendezvous.tla",
        "native-rendezvous",
        config,
        &[
            ("punchAuthenticates", "Authentication"),
            ("resurrect", "NoResurrection"),
            ("closeInstalled", "InstalledSurvives"),
        ],
        Some(&live),
    )
    .unwrap();
    let traces = exploration::traces(
        "verification/NativeRendezvous.tla",
        "native-rendezvous",
        config,
    )
    .unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for trace in traces {
                tokio::time::timeout(Duration::from_secs(3), async {
                    let host = Rc::new(Identity::generate());
                    let recipient = Identity::generate();
                    let listener = Listener::bind(
                        local(),
                        host.clone(),
                        Limits {
                            reservation_timeout: Duration::from_millis(200),
                            ..Limits::default()
                        },
                    )
                    .await
                    .unwrap();
                    let (_network, handle) = Network::new(host.public_key());
                    let denied = Provisioner::new(
                        listener.clone(),
                        handle.clone(),
                        recipient.public_key(),
                        listener.local_addr().unwrap(),
                        b"nat",
                    )
                    .unwrap();
                    let mut owned = Some(socket().await);
                    let mut request = denied.client().reserve_request();
                    request.get().set_rendezvous_address(
                        owned.as_ref().unwrap().local_addr().unwrap().to_string(),
                    );
                    assert!(
                        request.send().promise.await.is_err(),
                        "punching requires explicit authority"
                    );
                    let provider = Provisioner::with_rendezvous(
                        listener.clone(),
                        handle.clone(),
                        recipient.public_key(),
                        listener.local_addr().unwrap(),
                        b"nat",
                    )
                    .unwrap();
                    let mut request = provider.client().reserve_request();
                    request.get().set_rendezvous_address(
                        owned.as_ref().unwrap().local_addr().unwrap().to_string(),
                    );
                    let response = request.send().promise.await.unwrap();
                    let lease = response.get().unwrap().get_lease().unwrap();
                    let ticket = response.get().unwrap().get_ticket().unwrap();
                    let target = native_listener::Target {
                        address: listener.local_addr().unwrap(),
                        host: host.public_key(),
                        connection_id: ticket.get_connection_id().unwrap().try_into().unwrap(),
                    };
                    let psk: [u8; 32] = ticket.get_psk().unwrap().try_into().unwrap();
                    let mut session = None;
                    for state in trace {
                        match state["event"] {
                            1 => {
                                let mut bytes = [0; 100];
                                let (n, from) =
                                    owned.as_ref().unwrap().recv_from(&mut bytes).await.unwrap();
                                assert_eq!(from, target.address);
                                assert_eq!(n, 20);
                                assert_eq!(&bytes[..8], &[0, 0x11, 0, 0, 0x21, 0x12, 0xa4, 0x42]);
                            }
                            2 => {
                                let (connected, ready) = tokio::join!(
                                    native_listener::connect(
                                        owned.take().unwrap(),
                                        target,
                                        &recipient,
                                        Some(psk),
                                        b"nat"
                                    ),
                                    lease.ready_request().send().promise
                                );
                                session = Some(connected.unwrap());
                                ready.unwrap();
                            }
                            3 => {
                                lease.cancel_request().send().promise.await.unwrap();
                            }
                            4 => {
                                assert!(lease.ready_request().send().promise.await.is_err());
                            }
                            5 => provider.close(),
                            6 => {
                                let late = native_listener::connect(
                                    owned.take().unwrap(),
                                    target,
                                    &recipient,
                                    Some(psk),
                                    b"nat",
                                );
                                assert!(tokio::time::timeout(Duration::from_millis(30), late)
                                    .await
                                    .is_err());
                            }
                            _ => panic!("unexpected rendezvous action"),
                        }
                        for _ in 0..16 {
                            tokio::task::yield_now().await;
                        }
                        assert_eq!(listener.stats().pending, usize::from(state["phase"] == 1));
                        assert_eq!(
                            listener.stats().authenticated,
                            usize::from(state["phase"] == 2)
                        );
                        assert_eq!(session.is_some(), state["proof"] == 1);
                    }
                })
                .await
                .unwrap();
            }
        })
        .await;
}
