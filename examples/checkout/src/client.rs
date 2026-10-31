use crate::{
    checkout_capnp as api, Audit, Bucket, BusinessError, Claim, Driver, Entry, Result, Snapshot,
    State,
};
use capntproto::{
    capnp,
    capnp_rpc::twoparty::ConnectionOptions,
    rpc::{security, tcp::tls, Connection, SystemClock},
};
use std::{net::SocketAddr, path::Path, sync::Arc};
use tokio_util::compat::TokioAsyncReadCompatExt;
#[derive(Clone)]
pub struct Wire {
    pub address: SocketAddr,
    pub driver: Driver,
    pub tls: Arc<security::rustls::ClientConfig>,
}
impl Wire {
    pub fn new(
        address: SocketAddr,
        driver: Driver,
        ca: &Path,
        identity: Option<security::Identity>,
    ) -> Result<Self> {
        Ok(Self {
            address,
            driver,
            tls: tls::client_config(
                security::roots_from_pem_file(ca)?,
                identity,
                Arc::new(SystemClock),
            )?,
        })
    }
    async fn connect(&self) -> Result<Connection> {
        let io = tls::connect(
            self.address,
            "localhost".try_into()?,
            self.tls.clone(),
            std::time::Duration::from_secs(2),
        )
        .await?;
        Ok(Connection::spawn(
            ConnectionOptions::new()
                .driver(self.driver.protocol()?)
                .client(io.compat())?,
        ))
    }
}
/// Keep this owner alive for the full lifetime of its customer/order capabilities.
/// Call on a Tokio LocalSet; server-side business authority is account-scoped.
pub struct Client {
    connection: Connection,
    customer: api::customer::Client,
}
impl Client {
    pub async fn connect(wire: &Wire, account: &str, token: &str) -> Result<Self> {
        let connection = wire.connect().await?;
        let shop: api::shop::Client = connection.bootstrap();
        let mut request = shop.login_request();
        request.get().set_account(account);
        request.get().set_token(token);
        let customer = request.send().promise.await?.get()?.get_customer()?;
        Ok(Self {
            connection,
            customer,
        })
    }
    pub async fn place(
        &self,
        key: &str,
        quantity: u32,
    ) -> std::result::Result<Snapshot, BusinessError> {
        let mut request = self.customer.place_request();
        request.get().set_key(key);
        request.get().set_quantity(quantity);
        let response = request.send().promise.await?;
        Ok(Snapshot::read(response.get()?.get_snapshot()?)?)
    }
    /// Pipeline settlement through the promised Order capability before place returns.
    pub async fn buy(
        &self,
        key: &str,
        quantity: u32,
    ) -> std::result::Result<Snapshot, BusinessError> {
        let mut request = self.customer.place_request();
        request.get().set_key(key);
        request.get().set_quantity(quantity);
        let pending = request.send();
        let order = pending.pipeline.get_order();
        let settled = order.settled_request().send();
        pending.promise.await?;
        let response = settled.promise.await?;
        Ok(Snapshot::read(response.get()?.get_snapshot()?)?)
    }
    pub async fn cancel(&self, key: &str) -> std::result::Result<Snapshot, BusinessError> {
        let mut request = self.customer.cancel_request();
        request.get().set_key(key);
        let response = request.send().promise.await?;
        Ok(Snapshot::read(response.get()?.get_snapshot()?)?)
    }
    pub async fn view(&self, key: &str) -> std::result::Result<Snapshot, BusinessError> {
        let mut request = self.customer.view_request();
        request.get().set_key(key);
        let response = request.send().promise.await?;
        Ok(Snapshot::read(response.get()?.get_snapshot()?)?)
    }
    pub async fn list(&self) -> std::result::Result<Vec<Snapshot>, BusinessError> {
        let response = self.customer.list_request().send().promise.await?;
        response
            .get()?
            .get_orders()?
            .iter()
            .map(Snapshot::read)
            .collect::<capnp::Result<Vec<_>>>()
            .map_err(Into::into)
    }
    pub async fn close(self) -> capnp::Result<()> {
        self.connection
            .shutdown(std::time::Duration::from_secs(2))
            .await
    }
}
#[derive(Clone)]
pub struct ResourceClient(pub Wire);
impl ResourceClient {
    pub async fn apply(&self, method: &str, claim: &Claim) -> capnp::Result<State> {
        let (connection, state) = tokio::time::timeout(crate::RPC_TIMEOUT, async {
            let connection = self
                .0
                .connect()
                .await
                .map_err(|e| crate::error(e.to_string()))?;
            let remote: api::resource::Client = connection.bootstrap();
            let state = match method {
                "reserve" => {
                    let mut r = remote.reserve_request();
                    claim.write(r.get().init_claim());
                    r.send().promise.await?.get()?.get_state()?
                }
                "commit" => {
                    let mut r = remote.commit_request();
                    claim.write(r.get().init_claim());
                    r.send().promise.await?.get()?.get_state()?
                }
                "release" => {
                    let mut r = remote.release_request();
                    claim.write(r.get().init_claim());
                    r.send().promise.await?.get()?.get_state()?
                }
                _ => return Err(crate::error("invalid: internal method")),
            };
            Ok((connection, State::read(state)))
        })
        .await
        .map_err(|_| crate::error("resource deadline: outcome unknown"))??;
        // A valid reply resolves the business outcome. Give cleanup its own
        // deadline so it cannot turn an acknowledged effect into an unknown one.
        let _ = connection
            .shutdown(std::time::Duration::from_millis(100))
            .await;
        Ok(state)
    }
    pub async fn audit(&self) -> Result<Audit> {
        let connection = self.0.connect().await?;
        let remote: api::resource::Client = connection.bootstrap();
        let response = remote.audit_request().send().promise.await?;
        let result = response.get()?;
        let buckets = result
            .get_buckets()?
            .iter()
            .map(|b| {
                Ok(Bucket {
                    name: b.get_name()?.to_str()?.into(),
                    initial: b.get_initial(),
                    available: b.get_available(),
                })
            })
            .collect::<capnp::Result<Vec<_>>>()?;
        let entries = result
            .get_entries()?
            .iter()
            .map(|e| {
                Ok(Entry {
                    claim: Claim::read(e.get_claim()?)?,
                    state: State::read(e.get_state()?),
                })
            })
            .collect::<capnp::Result<Vec<_>>>()?;
        connection
            .shutdown(std::time::Duration::from_secs(2))
            .await?;
        Ok(Audit { buckets, entries })
    }
}
