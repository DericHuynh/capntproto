use capnp_rpc::{new_client, twoparty::TwoPartyServer};
use capntproto::rpc::{security::Identity, webtransport, SystemClock};
use capntproto::{capnp, capnp_rpc};
use capntproto_webtransport_example::{
    greeting_capnp::{greeting, session},
    protocol, Result,
};
use std::{rc::Rc, sync::Arc, time::Duration};
struct Service;
struct Session(String);
impl greeting::Server for Service {
    fn open(
        self: Rc<Self>,
        context: greeting::OpenContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        context.complete(|params, mut results| {
            let prefix = params.get_prefix()?.to_str()?.to_owned();
            results.set_session(new_client::<session::Client, _>(Session(prefix)));
            Ok(())
        })
    }
}
impl session::Server for Session {
    fn echo(
        self: Rc<Self>,
        context: session::EchoContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        context.complete(|params, mut results| {
            let value = format!("{}{}", self.0, params.get_value()?.to_str()?);
            results.set_value(&value);
            Ok(())
        })
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tokio::task::LocalSet::new()
        .run_until(async {
            let args: Vec<_> = std::env::args().collect();
            if args.len() != 5 {
                return Err(
                    "usage: server LISTEN_ADDR CERT_PEM KEY_PEM ALLOWED_BROWSER_ORIGIN".into(),
                );
            }
            let identity = Identity::from_pem_files(&args[2], &args[3])?;
            let endpoint = webtransport::Server::bind(
                args[1].parse()?,
                identity.certificates,
                identity.private_key,
                Arc::new(SystemClock),
            )?;
            let service: greeting::Client = new_client(Service);
            let (server, driver) =
                TwoPartyServer::with_driver(service.client, protocol()?, |err| {
                    eprintln!("RPC: {err}")
                });
            let listener = webtransport::listen(
                &server,
                &endpoint,
                Default::default(),
                Duration::from_secs(10),
                64.try_into().unwrap(),
                |request| {
                    request.path() == "/rpc"
                        && request.origin().is_none_or(|origin| origin == args[4])
                },
                |err| eprintln!("WebTransport setup: {err}"),
            );
            println!("Listening on https://{}/rpc", endpoint.local_addr()?);
            futures::pin_mut!(driver, listener);
            tokio::select! {
                result = &mut listener => result?,
                result = &mut driver => result?,
                result = tokio::signal::ctrl_c() => result?,
            }
            let drain = server.drain();
            futures::pin_mut!(drain);
            tokio::time::timeout(Duration::from_secs(10), async {
                tokio::select! { result = &mut drain => result, result = &mut driver => result }
            })
            .await??;
            Ok(())
        })
        .await
}
