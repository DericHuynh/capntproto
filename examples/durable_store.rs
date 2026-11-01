//! Self-contained localhost example: cargo run --example durable_store -- /tmp/example.rp
use capntproto::storage::ObjectKey;
use capntproto::{
    authority::{Grant, ObjectGeneration, ObjectId, Rights},
    orm::{ObjectServer, ObjectState},
    storage::Store,
    store_capnp::document,
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
            let client = ObjectServer::<document::Owned>::client(
                state,
                Grant::root(
                    ObjectId::new(1).unwrap(),
                    ObjectGeneration::new(1).unwrap(),
                    [1; 32],
                    Rights::ALL,
                ),
            )?;
            let mut put = client.put_request();
            put.get()
                .set_expected_head(store.borrow().head(ObjectKey::new(1)).get());
            put.get()
                .init_value()
                .set_text("Hello from legacy Cap'n Proto");
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
            Ok::<_, Box<dyn std::error::Error>>(())
        })
        .await
}
