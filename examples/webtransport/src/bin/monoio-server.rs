//! Tokio-free execution of the same schema/service, with one owned driver per connection.
use capntproto::{
    capnp_rpc::{self, rpc_twoparty_capnp::Side},
    rpc::{
        driven::DriverExt,
        monoio::{runtime, tls},
        SystemClock,
    },
};
use capntproto_webtransport_example::{greeting_capnp::greeting, protocol, Result, Service};
use std::{sync::Arc, time::Duration};
fn main() -> Result<()> {
    runtime::RuntimeBuilder::<runtime::FusionDriver>::new()
        .enable_timer()
        .build()?
        .block_on(async {
            let args: Vec<_> = std::env::args().collect();
            if args.len() != 4 {
                return Err("usage: monoio-server LISTEN_ADDR CERT_PEM KEY_PEM".into());
            }
            let config = tls::server_config(
                tls::Identity::from_pem_files(&args[2], &args[3])?,
                None,
                Arc::new(SystemClock),
            )?;
            let listener = runtime::net::TcpListener::bind(&args[1])?;
            println!("Listening on {}", listener.local_addr()?);
            loop {
                let (socket, _) = listener.accept().await?;
                let config = config.clone();
                drop(runtime::spawn(async move {
                    let result = async {
                        let (mut stream, driver) = tls::server(socket, config)?;
                        driver
                            .run_until(async {
                                runtime::time::timeout(Duration::from_secs(10), stream.ready())
                                    .await??;
                                let bootstrap: greeting::Client = capnp_rpc::new_client(Service);
                                capnp_rpc::twoparty::TwoPartyClient::with_driver(
                                    stream,
                                    Some(bootstrap.client),
                                    Side::Server,
                                    protocol()?,
                                )?
                                .await?;
                                Ok::<_, Box<dyn std::error::Error>>(())
                            })
                            .await
                            .map_err(|e| std::io::Error::other(format!("TLS driver: {e:?}")))?
                    }
                    .await;
                    if let Err(error) = result {
                        eprintln!("Connection: {error}");
                    }
                }));
            }
        })
}
