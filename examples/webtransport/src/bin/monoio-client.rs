//! The same pipelined greeting service over TCP/TLS on a completion runtime.
use capntproto::rpc::{
    driven::DriverExt,
    monoio::{runtime, tls},
    SystemClock,
};
use capntproto_webtransport_example::{greeting, Result};
use std::{sync::Arc, time::Duration};
fn main() -> Result<()> {
    runtime::RuntimeBuilder::<runtime::FusionDriver>::new()
        .enable_timer()
        .build()?
        .block_on(async {
            let args: Vec<_> = std::env::args().collect();
            if args.len() != 6 {
                return Err(
                    "usage: monoio-client SOCKET_ADDR CA_PEM SERVER_NAME PREFIX VALUE".into(),
                );
            }
            let config = tls::client_config(
                tls::roots_from_pem_file(&args[2])?,
                None,
                Arc::new(SystemClock),
            )?;
            runtime::time::timeout(Duration::from_secs(30), async {
                let socket = tls::tcp_connect(args[1].parse()?).await?;
                let (mut stream, driver) =
                    tls::client(socket, config, args[3].clone().try_into()?)?;
                let result = driver
                    .run_until(async {
                        stream.ready().await?;
                        greeting(stream, &args[4], &args[5]).await
                    })
                    .await
                    .map_err(|e| std::io::Error::other(format!("TLS driver: {e:?}")))??;
                println!("{result}");
                Ok(())
            })
            .await?
        })
}
