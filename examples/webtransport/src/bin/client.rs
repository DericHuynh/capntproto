use capntproto::rpc::{security, webtransport, SystemClock};
use capntproto_webtransport_example::{greeting, Result};
use std::sync::Arc;
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tokio::task::LocalSet::new()
        .run_until(async {
            let args: Vec<_> = std::env::args().collect();
            if args.len() != 5 {
                return Err("usage: client HTTPS_URL CA_PEM PREFIX VALUE".into());
            }
            let roots = security::roots_from_pem_file(&args[2])?;
            let roots = webtransport::tls::RootCertStore { roots: roots.roots };
            let client = webtransport::Client::new(roots, Arc::new(SystemClock))?;
            let mut session = client.connect(&args[1]).await?;
            let stream = session.open_bi().await?;
            println!("{}", greeting(stream, &args[3], &args[4]).await?);
            Ok(())
        })
        .await
}
