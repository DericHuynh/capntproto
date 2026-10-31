use capntproto::rpc::{security::Identity, tcp::tls, SystemClock};
use std::{sync::Arc, time::Duration};
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: telemetry-server ADDRESS CERT_PEM KEY_PEM".into());
    }
    let config = tls::server_config(
        Identity::from_pem_files(&args[2], &args[3])?,
        None,
        Arc::new(SystemClock),
    )?;
    let listener = tokio::net::TcpListener::bind(&args[1]).await?;
    println!("Listening on {}", listener.local_addr()?);
    loop {
        let (io, _) = listener.accept().await?;
        match tokio::time::timeout(
            Duration::from_secs(15),
            capntproto_embedded_example::host::serve(io, config.clone(), 16 * 1024),
        )
        .await
        {
            Ok(Ok(reading)) => println!(
                "device={} sequence={} milli_celsius={}",
                reading.device_id, reading.sequence, reading.milli_celsius
            ),
            result => eprintln!("session: {result:?}"),
        }
    }
}
