use crate::{
    checkout_capnp as api, error,
    faults::{self, FaultStage, Faults},
    store::{self, Orders, Resources},
    Config, OrderRow, ResourceClient, Result, Snapshot, State, Wire,
};
use capntproto::{
    capnp::{capability::Promise, Error},
    capnp_rpc::{new_client, twoparty::TwoPartyServer},
    rpc::{security, tcp::tls, SystemClock},
};
use clap::ValueEnum;
use std::{
    cell::RefCell, collections::HashSet, net::SocketAddr, path::PathBuf, rc::Rc, sync::Arc,
    time::Duration,
};
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Role {
    Shop,
    Inventory,
    Payments,
}
pub struct NodeConfig {
    pub role: Role,
    pub listen: SocketAddr,
    pub db: PathBuf,
    pub config: Config,

    pub cert: PathBuf,
    pub key: PathBuf,
    pub ca: PathBuf,
    pub client_cert: Option<PathBuf>,
    pub client_key: Option<PathBuf>,
    pub inventory: Option<SocketAddr>,
    pub payments: Option<SocketAddr>,
    pub faults: Option<PathBuf>,
}
struct ResourceService {
    storage: RefCell<Resources>,
    faults: Faults,
    permits: Arc<tokio::sync::Semaphore>,
}
impl ResourceService {
    async fn apply(
        self: Rc<Self>,
        method: &'static str,
        claim: crate::Claim,
    ) -> crate::capnp::Result<State> {
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| error("resource stopped"))?;
        // Accepted work outlives a lost client connection. Cancellation of a wait
        // is not a rollback; the durable idempotency ledger determines the effect.
        tokio::task::spawn_local(async move {
            let _permit = permit;
            let consume = |id: &str| {
                store::take_fault(&self.storage.borrow().0, id).map_err(|e| error(e.to_string()))
            };
            let before = self
                .faults
                .at(method, FaultStage::Before, &claim.key, consume)
                .await?;
            let state = self.storage.borrow_mut().apply(method, &claim)?;
            let after = self
                .faults
                .at(method, FaultStage::After, &claim.key, consume)
                .await?;
            faults::completed(before)?;
            faults::completed(after)?;
            Ok(state)
        })
        .await
        .map_err(|e| error(format!("resource job: {e}")))?
    }
}
impl api::resource::Server for ResourceService {
    fn reserve(self: Rc<Self>, context: api::resource::ReserveContext<'_>) -> Promise<(), Error> {
        let (params, mut results) = context.into_owned();
        Promise::from_future(async move {
            let claim = crate::Claim::read(params.get()?.get_claim()?)?;
            results
                .get()
                .set_state(self.apply("reserve", claim).await?.wire());
            Ok(())
        })
    }
    fn commit(self: Rc<Self>, context: api::resource::CommitContext<'_>) -> Promise<(), Error> {
        let (params, mut results) = context.into_owned();
        Promise::from_future(async move {
            let claim = crate::Claim::read(params.get()?.get_claim()?)?;
            results
                .get()
                .set_state(self.apply("commit", claim).await?.wire());
            Ok(())
        })
    }
    fn release(self: Rc<Self>, context: api::resource::ReleaseContext<'_>) -> Promise<(), Error> {
        let (params, mut results) = context.into_owned();
        Promise::from_future(async move {
            let claim = crate::Claim::read(params.get()?.get_claim()?)?;
            results
                .get()
                .set_state(self.apply("release", claim).await?.wire());
            Ok(())
        })
    }
    fn audit(self: Rc<Self>, context: api::resource::AuditContext<'_>) -> Promise<(), Error> {
        context.complete(|_, mut out| {
            let audit = self.storage.borrow().audit()?;
            let mut buckets = out.reborrow().init_buckets(audit.buckets.len() as u32);
            for (i, b) in audit.buckets.iter().enumerate() {
                let mut target = buckets.reborrow().get(i as u32);
                target.set_name(&b.name);
                target.set_initial(b.initial);
                target.set_available(b.available);
            }
            let mut entries = out.init_entries(audit.entries.len() as u32);
            for (i, e) in audit.entries.iter().enumerate() {
                let mut target = entries.reborrow().get(i as u32);
                e.claim.write(target.reborrow().init_claim());
                target.set_state(e.state.wire());
            }
            Ok(())
        })
    }
}
struct App {
    orders: RefCell<Orders>,
    config: Config,
    inventory: ResourceClient,
    payments: ResourceClient,
    faults: Faults,
    changed: tokio::sync::watch::Sender<u64>,
    active: RefCell<HashSet<(String, String)>>,
}
impl App {
    fn notify(&self) {
        self.changed
            .send_modify(|version| *version = version.wrapping_add(1));
    }
    fn kick(self: &Rc<Self>, row: OrderRow) {
        let id = (row.account.clone(), row.snapshot.key.clone());
        {
            let mut active = self.active.borrow_mut();
            if active.len() >= 64 || !active.insert(id.clone()) {
                return;
            }
        }
        let app = self.clone();
        tokio::task::spawn_local(async move {
            // Durable pending intent remains visible and is retried after any failure.
            let _ = app.advance(&id.0, &id.1).await;
            app.active.borrow_mut().remove(&id);
            app.notify();
        });
    }
    async fn advance(&self, account: &str, key: &str) -> crate::capnp::Result<()> {
        let row = self.orders.borrow().get(account, key)?;
        if row.snapshot.status.terminal() {
            return Ok(());
        }
        if row.intent != "buy" {
            return self.compensate(row).await;
        }
        let inventory = self.inventory.apply("reserve", &row.inventory()).await?;
        if !matches!(inventory, State::Held | State::Committed) {
            self.orders.borrow_mut().reject(account, key)?;
            self.notify();
            return Ok(());
        }
        // Recheck intent before issuing a new side effect. Resources also fence
        // delayed work with tombstones, because a timeout cannot cancel a debit.
        if self.orders.borrow().get(account, key)?.intent != "buy" {
            return Ok(());
        }
        let payment = self.payments.apply("reserve", &row.payment()).await?;
        if !matches!(payment, State::Held | State::Committed) {
            self.orders.borrow_mut().reject(account, key)?;
            self.notify();
            return Ok(());
        }
        if self.orders.borrow().get(account, key)?.intent != "buy" {
            return Ok(());
        }
        let inventory = self.inventory.apply("commit", &row.inventory()).await?;
        let payment = self.payments.apply("commit", &row.payment()).await?;
        if inventory == State::Committed && payment == State::Committed {
            self.orders.borrow_mut().finish(account, key, "buy")?;
        } else {
            self.orders.borrow_mut().reject(account, key)?;
        }
        self.notify();
        Ok(())
    }
    async fn compensate(&self, row: OrderRow) -> crate::capnp::Result<()> {
        let inventory = self.inventory.apply("release", &row.inventory()).await?;
        let payment = self.payments.apply("release", &row.payment()).await?;
        if inventory != State::Released || payment != State::Released {
            return Err(error("compensation not acknowledged"));
        }
        self.orders
            .borrow_mut()
            .finish(&row.account, &row.snapshot.key, &row.intent)?;
        self.notify();
        Ok(())
    }
    fn cancel(self: &Rc<Self>, account: &str, key: &str) -> crate::capnp::Result<Snapshot> {
        let row = self.orders.borrow_mut().cancel(account, key)?;
        self.notify();
        self.kick(row.clone());
        Ok(row.snapshot)
    }
    async fn settle(&self, account: &str, key: &str) -> crate::capnp::Result<Snapshot> {
        let mut changed = self.changed.subscribe();
        loop {
            let row = self.orders.borrow().get(account, key)?;
            if row.snapshot.status.terminal() {
                return Ok(row.snapshot);
            }
            changed
                .changed()
                .await
                .map_err(|_| error("checkout stopped"))?;
        }
    }
}
struct Shop(Rc<App>);
struct Customer {
    app: Rc<App>,
    account: String,
}
struct Order {
    app: Rc<App>,
    account: String,
    key: String,
}
impl api::shop::Server for Shop {
    fn login(self: Rc<Self>, context: api::shop::LoginContext<'_>) -> Promise<(), Error> {
        context.complete(|params, mut out| {
            let account = params.get_account()?.to_str()?;
            let token = params.get_token()?.to_str()?;
            if !self
                .0
                .config
                .accounts
                .iter()
                .any(|a| a.name == account && a.token == token)
            {
                return Err(error("unauthorized: customer credentials"));
            }
            out.set_customer(new_client::<api::customer::Client, _>(Customer {
                app: self.0.clone(),
                account: account.into(),
            }));
            Ok(())
        })
    }
    fn catalog(self: Rc<Self>, context: api::shop::CatalogContext<'_>) -> Promise<(), Error> {
        context.complete(|_, mut out| {
            out.set_unit_cents(1000);
            out.set_bulk_minimum(3);
            out.set_bulk_unit_cents(900);
            Ok(())
        })
    }
}
impl api::customer::Server for Customer {
    fn place(self: Rc<Self>, context: api::customer::PlaceContext<'_>) -> Promise<(), Error> {
        let (params, mut results) = context.into_owned();
        Promise::from_future(async move {
            let params = params.get()?;
            let key = params.get_key()?.to_str()?.to_owned();
            let quantity = params.get_quantity();
            let row = self
                .app
                .orders
                .borrow_mut()
                .place(&self.account, &key, quantity)?;
            self.app.notify();
            self.app.kick(row.clone());
            let consume = |id: &str| {
                store::take_fault(&self.app.orders.borrow().0, id).map_err(|e| error(e.to_string()))
            };
            let gate = self
                .app
                .faults
                .at("place", FaultStage::After, &row.id(), consume)
                .await?;
            let mut results = results.get();
            row.snapshot.write(results.reborrow().init_snapshot());
            results.set_order(new_client::<api::order::Client, _>(Order {
                app: self.app.clone(),
                account: self.account.clone(),
                key,
            }));
            faults::completed(gate)?;
            Ok(())
        })
    }
    fn cancel(self: Rc<Self>, context: api::customer::CancelContext<'_>) -> Promise<(), Error> {
        context.complete(|params, out| {
            self.app
                .cancel(&self.account, params.get_key()?.to_str()?)?
                .write(out.init_snapshot());
            Ok(())
        })
    }
    fn view(self: Rc<Self>, context: api::customer::ViewContext<'_>) -> Promise<(), Error> {
        context.complete(|params, out| {
            self.app
                .orders
                .borrow()
                .get(&self.account, params.get_key()?.to_str()?)?
                .snapshot
                .write(out.init_snapshot());
            Ok(())
        })
    }
    fn list(self: Rc<Self>, context: api::customer::ListContext<'_>) -> Promise<(), Error> {
        context.complete(|_, out| {
            let rows = self.app.orders.borrow().list(Some(&self.account), false)?;
            let mut list = out.init_orders(rows.len() as u32);
            for (i, row) in rows.iter().enumerate() {
                row.snapshot.write(list.reborrow().get(i as u32));
            }
            Ok(())
        })
    }
}
impl api::order::Server for Order {
    fn state(self: Rc<Self>, context: api::order::StateContext<'_>) -> Promise<(), Error> {
        context.complete(|_, out| {
            self.app
                .orders
                .borrow()
                .get(&self.account, &self.key)?
                .snapshot
                .write(out.init_snapshot());
            Ok(())
        })
    }
    fn cancel(self: Rc<Self>, context: api::order::CancelContext<'_>) -> Promise<(), Error> {
        context.complete(|_, out| {
            self.app
                .cancel(&self.account, &self.key)?
                .write(out.init_snapshot());
            Ok(())
        })
    }
    fn settled(self: Rc<Self>, context: api::order::SettledContext<'_>) -> Promise<(), Error> {
        let (_, mut out) = context.into_owned();
        Promise::from_future(async move {
            self.app
                .settle(&self.account, &self.key)
                .await?
                .write(out.get().init_snapshot());
            Ok(())
        })
    }
}
/// Run on a Tokio LocalSet. Readiness is emitted only after DB recovery and bind.
pub async fn serve(config: NodeConfig) -> Result<()> {
    if config.config.stock > i64::MAX as u64 || config.config.accounts.is_empty() {
        return Err("invalid seed configuration".into());
    }
    let mut accounts = HashSet::new();
    for account in &config.config.accounts {
        if !crate::valid_key(&account.name)
            || account.token.len() < 12
            || account.cents > i64::MAX as u64
            || !accounts.insert(account.name.clone())
        {
            return Err("invalid account configuration".into());
        }
    }
    let faults = Faults::load(config.faults.as_deref())?;
    let identity = security::Identity::from_pem_files(&config.cert, &config.key)?;
    let tls_config = tls::server_config(
        identity,
        if matches!(config.role, Role::Shop) {
            None
        } else {
            Some(security::roots_from_pem_file(&config.ca)?)
        },
        Arc::new(SystemClock),
    )?;
    let bootstrap = match config.role {
        Role::Inventory | Role::Payments => {
            let buckets = match config.role {
                Role::Inventory => vec![("tickets".into(), config.config.stock)],
                _ => config
                    .config
                    .accounts
                    .iter()
                    .map(|a| (a.name.clone(), a.cents))
                    .collect(),
            };
            new_client::<api::resource::Client, _>(ResourceService {
                storage: RefCell::new(Resources::open(&config.db, &buckets)?),
                faults,
                permits: Arc::new(tokio::sync::Semaphore::new(32)),
            })
            .client
        }
        Role::Shop => {
            let identity = || -> Result<security::Identity> {
                Ok(security::Identity::from_pem_files(
                    config
                        .client_cert
                        .as_ref()
                        .ok_or("shop needs --client-cert")?,
                    config
                        .client_key
                        .as_ref()
                        .ok_or("shop needs --client-key")?,
                )?)
            };
            let inventory = ResourceClient(Wire::new(
                config.inventory.ok_or("shop needs --inventory")?,
                &config.ca,
                Some(identity()?),
            )?);
            let payments = ResourceClient(Wire::new(
                config.payments.ok_or("shop needs --payments")?,
                &config.ca,
                Some(identity()?),
            )?);
            let (changed, _) = tokio::sync::watch::channel(0);
            let app = Rc::new(App {
                orders: RefCell::new(Orders::open(&config.db)?),
                config: config.config,
                inventory,
                payments,
                faults,
                changed,
                active: RefCell::new(HashSet::new()),
            });
            let background = Rc::downgrade(&app);
            tokio::task::spawn_local(async move {
                let mut interval = tokio::time::interval(Duration::from_millis(20));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    interval.tick().await;
                    let Some(app) = background.upgrade() else {
                        break;
                    };
                    let pending = app.orders.borrow().list(None, true);
                    if let Ok(rows) = pending {
                        for row in rows {
                            app.kick(row);
                        }
                    }
                }
            });
            new_client::<api::shop::Client, _>(Shop(app)).client
        }
    };
    let (server, driver) = TwoPartyServer::with_error_handler(bootstrap, |error| {
        if error.kind != crate::capnp::ErrorKind::Disconnected {
            eprintln!("RPC: {error}");
        }
    });
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    println!(
        "{}",
        serde_json::json!({"address":listener.local_addr()?.to_string()})
    );
    let accept = tls::listen(
        &server,
        &listener,
        tls_config,
        Default::default(),
        Default::default(),
        |_, _| {},
    );
    futures::pin_mut!(accept, driver);
    tokio::select! {result=&mut accept=>result?,result=&mut driver=>result?,result=tokio::signal::ctrl_c()=>result?}
    Ok(())
}
