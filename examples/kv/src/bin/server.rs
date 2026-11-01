//! A shared key/value service with a schema, concurrent clients and TLS authentication.
use capntproto_kv_example::{connection::Result, key_value_capnp};
#[path = "../service.rs"]
mod service;
use capntproto::{
    capnp_rpc,
    rpc::{security, tcp::tls, SystemClock},
};
use clap::Parser;
use std::{path::PathBuf, sync::Arc};
#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:4000")]
    listen: String,
    #[arg(long)]
    cert: PathBuf,
    #[arg(long)]
    key: PathBuf,
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Args::parse();
    tokio::task::LocalSet::new().run_until(async move {
        let bootstrap=capnp_rpc::new_client::<key_value_capnp::directory::Client,_>(service::Directory::default());
        let(server,driver)=capnp_rpc::twoparty::TwoPartyServer::new(bootstrap.client);
        let listener=tokio::net::TcpListener::bind(args.listen).await?;
        let config=tls::server_config(security::Identity::from_pem_files(args.cert,args.key)?,None,Arc::new(SystemClock))?;
        println!("Listening on tls://{}",listener.local_addr()?);
        tokio::select! {
            result=driver=>result?,
            result=tls::listen(&server,&listener,config,Default::default(),Default::default(),|peer,error|eprintln!("Rejected {peer}: {error}"))=>result?,
            result=tokio::signal::ctrl_c()=>result?,
        }
        Ok(())
    }).await
}
