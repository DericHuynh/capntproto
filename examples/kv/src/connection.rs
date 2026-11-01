//! Shared authenticated connection setup for both clients.
use capntproto::{
    capnp_rpc::rpc_twoparty_capnp::Side,
    rpc::{self, security, tcp::tls},
};
use clap::Args;
use std::{path::PathBuf, sync::Arc, time::Duration};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub const TIMEOUT: Duration = Duration::from_secs(10);
#[derive(Args)]
pub struct ClientArgs {
    #[arg(long, default_value = "127.0.0.1:4000")]
    pub server: String,
    #[arg(long)]
    pub ca: PathBuf,
    #[arg(long, default_value = "localhost")]
    pub server_name: String,
}
impl ClientArgs {
    pub async fn connect(&self) -> Result<rpc::Connection> {
        let config = tls::client_config(
            security::roots_from_pem_file(&self.ca)?,
            None,
            Arc::new(rpc::SystemClock),
        )?;
        let stream = tls::connect(
            &self.server,
            self.server_name.clone().try_into()?,
            config,
            TIMEOUT,
        )
        .await?;
        Ok(rpc::Connection::spawn(tls::driver(
            stream,
            None,
            Side::Client,
            Default::default(),
        )))
    }
}
