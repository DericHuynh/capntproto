//! Controlled quiche peer for scripts/check_quic_interop.py.
use reproto::rpc::{
    quic::{self, Endpoint, Version},
    tls::{rustls, Identity},
};
use std::{
    io::{self, Write},
    net::SocketAddr,
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
const SIZE: usize = 131_072;
const TIMEOUT: Duration = Duration::from_secs(10);
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::task::LocalSet::new()
        .run_until(async { tokio::time::timeout(Duration::from_secs(30), run()).await? })
        .await
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let mode = &args[1];
    let address: SocketAddr = args[2].parse()?;
    let certificate = rustls::pki_types::CertificateDer::from(std::fs::read(&args[3])?);
    let key = rustls::pki_types::PrivatePkcs8KeyDer::from(std::fs::read(&args[4])?);
    let version = match args[5].as_str() {
        "1" => Version::V1,
        "2" => Version::V2,
        _ => panic!("version must be 1 or 2"),
    };
    let (endpoint, mut stream) = if mode == "server" {
        let mut config = quic::server_config(
            Identity {
                certificates: vec![certificate],
                private_key: key.into(),
            },
            None,
        )?;
        config.require_retry(args.get(6).is_some_and(|s| s == "retry"));
        let endpoint = Endpoint::server(config, address)?;
        println!("{}", endpoint.local_addr()?.port());
        io::stdout().flush()?;
        let stream =
            quic::accept(endpoint.accept().await.ok_or("endpoint closed")?, TIMEOUT).await?;
        (endpoint, stream)
    } else {
        let mut endpoint = Endpoint::client("127.0.0.1:0".parse()?)?;
        endpoint.set_default_client_config(quic::client_config_for_version(
            vec![certificate],
            None,
            version,
        )?);
        let stream = quic::connect(&endpoint, address, "localhost", TIMEOUT).await?;
        (endpoint, stream)
    };
    assert_eq!(stream.version(), version.wire_id());
    for round in 0..2u8 {
        let expected = vec![round + 1; SIZE];
        let mut received = vec![0; SIZE];
        if mode == "client" {
            stream.write_all(&expected).await?;
        }
        stream.read_exact(&mut received).await?;
        assert_eq!(received, expected);
        if mode == "server" {
            stream.write_all(&received).await?;
        }
        // The independent peer initiates key updates in each direction.
    }
    if mode == "client" {
        stream.write_all(b"done").await?;
    } else {
        let mut done = [0; 4];
        stream.read_exact(&mut done).await?;
        assert_eq!(&done, b"done");
    }
    stream.shutdown().await?;
    drop(stream);
    endpoint.wait_idle().await;
    Ok(())
}
