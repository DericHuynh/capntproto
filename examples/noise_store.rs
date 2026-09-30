//! Self-contained localhost example: cargo run --example noise_store -- /tmp/example.rp
use reproto::storage::ObjectKey;
use reproto::{
    authority::{Grant, ObjectGeneration, ObjectId, Rights},
    orm::{ObjectServer, ObjectState},
    rpc,
    storage::Store,
    store_capnp::{document, object},
    transport::{self, Identity},
};
use std::{cell::RefCell, rc::Rc};
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::task::LocalSet::new()
        .run_until(async {
            let path = std::env::args()
                .nth(1)
                .ok_or("supply a new or existing storage path")?;
            let store = Rc::new(RefCell::new(Store::open(path)?));
            let state = ObjectState::new(store.clone(), ObjectId::new(1).unwrap());
            let client_id = Identity::generate();
            let server_id = Identity::generate();
            let service = ObjectServer::<document::Owned>::client(
                state,
                Grant::root(
                    ObjectId::new(1).unwrap(),
                    ObjectGeneration::new(1).unwrap(),
                    client_id.public_key(),
                    Rights::ALL,
                ),
            )?;
            let a = tokio::net::UdpSocket::bind("127.0.0.1:0").await?;
            let b = tokio::net::UdpSocket::bind("127.0.0.1:0").await?;
            let remote = b.local_addr()?;
            let mut ac = transport::config(&client_id, server_id.public_key(), None, b"example")?;
            let mut bc = transport::config(&server_id, client_id.public_key(), None, b"example")?;
            let (a, at) = transport::connect(a, remote, &mut ac).await?;
            let (b, bt) = transport::accept(b, &mut bc).await?;
            let server = rpc::serve(b, service.client);
            let (client, task) = rpc::client::<object::Client<document::Owned>>(a);
            let mut put = client.put_request();
            put.get()
                .set_expected_head(store.borrow().head(ObjectKey::new(1)).get());
            put.get()
                .init_value()
                .set_text("Hello from Noise-backed Cap'n Proto");
            let revision = put.send().promise.await?.get()?.get_revision();
            let mut publish = client.publish_request();
            publish.get().set_revision(revision);
            publish
                .get()
                .set_expected_published(store.borrow().published(ObjectKey::new(1)).get());
            publish.send().promise.await?;
            let reply = client.get_request().send().promise.await?;
            let value = reply.get()?;
            println!(
                "revision {}: {}",
                value.get_revision(),
                value.get_value()?.get_text()?.to_str()?
            );
            for t in [
                at.abort_handle(),
                bt.abort_handle(),
                server.abort_handle(),
                task.abort_handle(),
            ] {
                t.abort();
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        })
        .await
}
