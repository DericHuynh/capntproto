use super::*;
use crate::{
    bulk::{Receiver, Status},
    native_rpc::{Handle, Network},
    transport::{
        backend_tests::{pair, Backend},
        QuicVersion,
    },
};
const TIMEOUT: Duration = Duration::from_secs(5);
struct Fixture {
    receiver: Receiver,
    client: wire::transfer::Client,
    plane: Option<Plane>,
    _handles: [Handle; 2],
    tasks: Vec<tokio::task::JoinHandle<capnp::Result<()>>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
async fn fixture(length: u64, backend: Backend, split: bool) -> Fixture {
    let (a, b) = pair(backend, backend, QuicVersion::V1).await;
    let ap = a.local;
    let bp = b.local;
    let (an, ah) = Network::new(ap);
    let (bn, bh) = Network::new(bp);
    ah.attach(a).unwrap();
    bh.attach(b).unwrap();
    let plane = ah.bulk(bp);
    let config = Config::new(length, 16 * 1024, 64 * 1024, 65536).unwrap();
    let (receiver, mut service) = Receiver::new(config);
    if split {
        service = enable(service, bh.bulk(ap).unwrap(), TIMEOUT).unwrap();
    }
    let server = capnp_rpc::RpcSystem::new(Box::new(bn), Some(service.client));
    let mut system = capnp_rpc::RpcSystem::new(Box::new(an), None);
    let client = system.bootstrap(bp);
    Fixture {
        receiver,
        client,
        plane,
        _handles: [ah, bh],
        tasks: vec![
            tokio::task::spawn_local(server),
            tokio::task::spawn_local(system),
        ],
    }
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
async fn native_rpc_negotiates_split_plane_with_old_peer_and_tcp_fallback() {
    scoped(async {
        for (backend, split) in [
            (Backend::Quiche, true),
            (Backend::Quiche, false),
            (Backend::Tcp, false),
        ] {
            for length in [0, 3, 500_000] {
                let f = fixture(length, backend, split).await;
                let bytes = vec![29; length as usize];
                let summary = send(f.client.clone(), f.plane.clone(), bytes.as_slice(), TIMEOUT)
                    .await
                    .unwrap();
                assert_eq!(summary.bytes, length);
                assert_eq!(summary.chunks, length.div_ceil(16384));
                assert_eq!(f.receiver.completed().unwrap().as_ref(), bytes);
                if let Some(plane) = &f.plane {
                    assert_eq!(
                        plane.stats().unwrap().sent_bytes,
                        if split { length } else { 0 }
                    );
                }
            }
        }
    })
    .await;
}
#[tokio::test]
async fn existing_sender_remains_compatible_with_split_receiver() {
    scoped(async {
        let f = fixture(3, Backend::Quiche, true).await;
        let mut sender = Sender::connect(f.client.clone()).await.unwrap();
        sender.write(b"a").await.unwrap();
        sender.write(b"bc").await.unwrap();
        assert!(f.client.open_stream_request().send().promise.await.is_err());
        assert_eq!(
            sender.done().await.unwrap(),
            Summary {
                bytes: 3,
                chunks: 2
            }
        );
        assert_eq!(f.plane.as_ref().unwrap().stats().unwrap().sent_bytes, 0);
    })
    .await;
}
#[tokio::test]
async fn stream_receipt_does_not_publish_and_plane_selection_is_single_use() {
    scoped(async {
        let f = fixture(3, Backend::Quiche, true).await;
        let response = f.client.open_stream_request().send().promise.await.unwrap();
        let offer = Offer::decode(response.get().unwrap().get_offer().unwrap()).unwrap();
        assert!(f.client.open_stream_request().send().promise.await.is_err());
        let mut write = f.client.write_request();
        write.get().set_sequence(1);
        write.get().set_data(b"a");
        assert!(write.send().promise.await.is_err());
        let mut stream = f.plane.as_ref().unwrap().send(offer, TIMEOUT).unwrap();
        stream.write_all(b"abc").await.unwrap();
        stream.finish().await.unwrap();
        assert!(f.receiver.completed().is_none());
        f.client.done_request().send().promise.await.unwrap();
        assert_eq!(f.receiver.completed().unwrap().as_ref(), b"abc");
        assert_eq!(
            f.client
                .cancel_request()
                .send()
                .promise
                .await
                .unwrap()
                .get()
                .unwrap()
                .get_status()
                .unwrap(),
            Status::Complete
        );
    })
    .await;
}
#[tokio::test]
async fn short_and_excess_sources_cancel_without_publication() {
    scoped(async {
        for split in [false, true] {
            for source in [b"ab".as_slice(), b"abcd".as_slice()] {
                let f = fixture(3, Backend::Quiche, split).await;
                assert!(send(f.client.clone(), f.plane.clone(), source, TIMEOUT)
                    .await
                    .is_err());
                assert!(f.receiver.completed().is_none());
                assert_eq!(f.receiver.status(), Status::Canceled);
                assert_eq!(f.receiver.staged_bytes(), 0);
            }
        }
    })
    .await;
}
#[tokio::test]
async fn cancellation_interrupts_blocked_stream_and_never_downgrades_rejected_grant() {
    scoped(async {
        let f = fixture(1_000_000, Backend::Quiche, true).await;
        let response = f.client.open_stream_request().send().promise.await.unwrap();
        let offer = Offer::decode(response.get().unwrap().get_offer().unwrap()).unwrap();
        let mut stream = f.plane.as_ref().unwrap().send(offer, TIMEOUT).unwrap();
        stream.write_all(b"partial").await.unwrap();
        f.client.cancel_request().send().promise.await.unwrap();
        assert!(stream.write_all(&vec![0; 999_993]).await.is_err());
        assert!(send(f.client.clone(), f.plane.clone(), &b""[..], TIMEOUT)
            .await
            .is_err());
        assert!(f.receiver.completed().is_none());
        assert_eq!(f.receiver.staged_bytes(), 0);
    })
    .await;
}
