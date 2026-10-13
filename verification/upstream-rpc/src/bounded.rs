//! Independent peer for the existing allocation-counted, caller-buffered client.
use crate::runtime_test_capnp::harness;
use capnp_rpc::{rpc_twoparty_capnp::Side, RpcSystem};
use futures::AsyncReadExt;
use std::{io::Write, rc::Rc, time::Duration};
use tokio_util::compat::TokioAsyncReadCompatExt;

struct Echo;
impl harness::Server for Echo {
    async fn echo(
        self: Rc<Self>,
        params: harness::EchoParams,
        mut results: harness::EchoResults,
    ) -> capnp::Result<()> {
        let value = params.get()?.get_value();
        if value == 1001 {
            // Preserve the existing C++ fixture's diagnostic contract too.
            return Err(capnp::Error::overloaded("cpp failure".into()));
        }
        results.get().set_value(100 + value);
        Ok(())
    }
    async fn bounce(
        self: Rc<Self>,
        params: harness::BounceParams,
        mut results: harness::BounceResults,
    ) -> capnp::Result<()> {
        results.get().set_cap(params.get()?.get_cap()?);
        Ok(())
    }
    async fn pending(
        self: Rc<Self>,
        _: harness::PendingParams,
        mut results: harness::PendingResults,
    ) -> capnp::Result<()> {
        results.get().set_cap(capnp_rpc::new_client(Echo));
        Ok(())
    }
}
pub fn main() -> crate::rpc_peer::Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(tokio::task::LocalSet::new().run_until(async {
            tokio::time::timeout(Duration::from_secs(30), async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
                println!("{}", listener.local_addr()?.port());
                std::io::stdout().flush()?;
                let (socket, _) = listener.accept().await?;
                socket.set_nodelay(true)?;
                let (r, w) = socket.compat().split();
                let network =
                    capnp_rpc::twoparty::VatNetwork::new(r, w, Side::Server, Default::default());
                let server: harness::Client = capnp_rpc::new_client(Echo);
                let _ = RpcSystem::new(Box::new(network), Some(server.client)).await;
                Ok(())
            })
            .await?
        }))
}
