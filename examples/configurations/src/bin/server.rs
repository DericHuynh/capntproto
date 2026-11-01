use capntproto::{
    capnp_rpc,
    rpc::{security, tcp::tls, SystemClock},
};
use capntproto_configuration_example::{self as example, Result, DEADLINE};
use clap::Parser;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:4433")]
    address: SocketAddr,
    #[arg(long)]
    cert: PathBuf,
    #[arg(long)]
    key: PathBuf,
    /// Require a client certificate from this CA when supplied.
    #[arg(long)]
    client_ca: Option<PathBuf>,
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Args::parse();
    tokio::task::LocalSet::new().run_until(async move {
        let identity=security::Identity::from_pem_files(args.cert,args.key)?;
        let roots=args.client_ca.map(security::roots_from_pem_file).transpose()?;
        let config=tls::server_config(identity,roots,Arc::new(SystemClock))?;
        let listener=tokio::net::TcpListener::bind(args.address).await?;
        println!("Listening on tls://{}",listener.local_addr()?);
        let bootstrap=capnp_rpc::new_client::<example::greeting_capnp::greeting::Client,_>(example::Service);
        let(server,driver)=capnp_rpc::twoparty::TwoPartyServer::new(bootstrap.client);
        let options=capntproto::rpc::AcceptOptions { timeout:DEADLINE,..Default::default() };
        tokio::select! {
            result=tls::listen(&server,&listener,config,Default::default(),options,|peer,error|eprintln!("Rejected {peer}: {error}"))=>result?,
            result=driver=>result?,
            result=tokio::signal::ctrl_c()=>result?,
        }
        Ok(())
    }).await
}
