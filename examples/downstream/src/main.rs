use std::rc::Rc;
pub mod preview_capnp {
    include!(concat!(env!("OUT_DIR"), "/preview_capnp.rs"));
}
struct Echo;
impl preview_capnp::echo::Server for Echo {
    async fn echo(
        self: Rc<Self>,
        params: preview_capnp::echo::EchoParams,
        mut results: preview_capnp::echo::EchoResults,
    ) -> capnp::Result<()> {
        results.get().set_value(params.get()?.get_value());
        Ok(())
    }
    async fn child(
        self: Rc<Self>,
        _: preview_capnp::echo::ChildParams,
        mut results: preview_capnp::echo::ChildResults,
    ) -> capnp::Result<()> {
        results.get().set_cap(capnp_rpc::new_client(Echo));
        Ok(())
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> capnp::Result<()> {
    tokio::task::LocalSet::new()
        .run_until(async {
            let bootstrap: preview_capnp::echo::Client = capnp_rpc::new_client(Echo);
            let (a, b) = tokio::io::duplex(4096);
            let server = reproto::rpc::serve(b, bootstrap.client);
            let (client, driver) = reproto::rpc::client::<preview_capnp::echo::Client>(a);
            let pending = client.child_call().send();
            let child = pending.pipeline.get_cap();
            let mut call = child.echo_call();
            call.edit().value().set(42);
            assert_eq!(call.send().await?.read()?.value(), 42);
            drop(pending.await?);
            server.abort();
            driver.abort();
            #[cfg(all(feature = "storage", unix))]
            {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("preview.store");
                let mut store = reproto::storage::Store::open(&path).unwrap();
                store
                    .commit(&[reproto::storage::Update {
                        object: reproto::storage::ObjectKey::new(1),
                        expected_head: reproto::storage::Revision::INITIAL,
                        expected_published: Some(reproto::storage::Revision::INITIAL),
                        value: b"durable preview",
                    }])
                    .unwrap();
                drop(store);
                let store = reproto::storage::Store::open(&path).unwrap();
                assert_eq!(
                    store
                        .get(reproto::storage::ObjectKey::new(1))
                        .unwrap()
                        .bytes(),
                    b"durable preview"
                );
            }
            println!("generated field API, returned capability and pipelined wire RPC passed");
            Ok(())
        })
        .await
}
