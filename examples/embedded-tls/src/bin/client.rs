use capntproto::rpc::{security, SystemClock};
use capntproto_embedded_example::{host::Socket, publish, Reading, Storage};
use std::{io, time::Duration};
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: telemetry-client ADDRESS CA_PEM SERVER_NAME".into());
    }
    let roots = security::certificates_from_pem_file(&args[2])?;
    let config = capntproto_transport::embedded::ClientConfig::new(
        &args[3],
        roots.first().ok_or("empty CA file")?.as_ref(),
        &SystemClock,
    )
    .map_err(|e| io::Error::other(format!("TLS config: {e:?}")))?;
    let result = tokio::time::timeout(Duration::from_secs(15), async {
        let io = Socket {
            io: tokio::net::TcpStream::connect(&args[1]).await?,
            fragment: 16 * 1024,
        };
        publish(
            io,
            &config,
            rand::rngs::OsRng,
            &mut Storage::new(),
            Reading {
                device_id: 42,
                sequence: 1,
                milli_celsius: 21_500,
            },
        )
        .await
        .map_err(|e| io::Error::other(format!("publish: {e:?}")))
    })
    .await??;
    println!("sequence={} accepted={}", result.sequence, result.accepted);
    Ok(())
}
