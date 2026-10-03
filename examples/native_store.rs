//! Self-contained localhost example: cargo run --example native_store -- /tmp/example.rp
use capntproto::storage::ObjectKey;
use capntproto::{
    authority::{Grant, ObjectGeneration, ObjectId, Rights},
    native_rpc::Vat,
    orm::{ObjectServer, ObjectState},
    storage::Store,
    store_capnp::{document, object},
    transport::{self, Identity},
};
use std::{cell::RefCell, rc::Rc, time::Duration};
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
            let (a, b) = tokio::join!(
                transport::connect_authenticated(
                    a,
                    remote,
                    &client_id,
                    server_id.public_key(),
                    None,
                    b"example"
                ),
                transport::accept_authenticated(
                    b,
                    &server_id,
                    client_id.public_key(),
                    None,
                    b"example"
                ),
            );
            let server = Vat::builder(server_id.public_key())
                .bootstrap(service)
                .start()?;
            let caller = Vat::builder(client_id.public_key()).start()?;
            server.attach(b?)?;
            caller.attach(a?)?;
            let client: object::Client<document::Owned> = caller.bootstrap(server_id.public_key());
            let mut put = client.put_request();
            put.get()
                .set_expected_head(store.borrow().head(ObjectKey::new(1)).get());
            put.get()
                .init_value()
                .set_text("Hello from Native-backed Cap'n Proto");
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
            let timeout = Duration::from_secs(3);
            for (_, receipt) in caller.shutdown(timeout).await? {
                receipt?;
            }
            server.shutdown(timeout).await?;
            Ok::<_, Box<dyn std::error::Error>>(())
        })
        .await
}
