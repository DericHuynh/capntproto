use capntproto_checkout_example::{
    Account, Audit, Client, Config, Driver, FaultAction, FaultPlan, FaultStage, ResourceClient,
    Snapshot, Wire,
};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
};
pub const TIMEOUT: Duration = Duration::from_secs(25);
pub struct Node {
    child: Child,
    pub address: SocketAddr,
    output: Option<tokio::task::JoinHandle<()>>,
}
impl Node {
    pub async fn stop(&mut self) {
        if self.child.try_wait().unwrap().is_none() {
            self.child.kill().await.unwrap();
        }
        let _ = self.child.wait().await;
        if let Some(output) = self.output.take() {
            let _ = output.await;
        }
    }
    pub async fn crashed(&mut self) {
        let status = tokio::time::timeout(TIMEOUT, self.child.wait())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status.code(), Some(86));
    }
}
pub struct Cluster {
    pub directory: tempfile::TempDir,
    pub inventory: Node,
    pub payments: Node,
    pub shop: Node,
    pub client_driver: Driver,
    pub shop_driver: Driver,
    pub inventory_driver: Driver,
    pub payments_driver: Driver,
}
impl Cluster {
    pub async fn start(
        drivers: [Driver; 4],
        stock: u64,
        balance: u64,
        plans: [Vec<FaultPlan>; 3],
    ) -> Self {
        let directory = tempfile::tempdir().unwrap();
        fixtures::Pki::generate()
            .unwrap()
            .write(&directory.path().join("pki"))
            .unwrap();
        let config = Config {
            stock,
            accounts: vec![
                Account {
                    name: "alice".into(),
                    token: "alice-development-token".into(),
                    cents: balance,
                },
                Account {
                    name: "bob".into(),
                    token: "bob-development-token".into(),
                    cents: balance / 2,
                },
            ],
        };
        std::fs::write(
            directory.path().join("config.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        for (role, plan) in ["inventory", "payments", "shop"].into_iter().zip(plans) {
            std::fs::write(
                directory.path().join(format!("{role}-faults.json")),
                serde_json::to_vec(&plan).unwrap(),
            )
            .unwrap();
        }
        let inventory = Self::spawn(directory.path(), "inventory", drivers[2], None, None).await;
        let payments = Self::spawn(directory.path(), "payments", drivers[3], None, None).await;
        let shop = Self::spawn(
            directory.path(),
            "shop",
            drivers[1],
            None,
            Some((inventory.address, payments.address)),
        )
        .await;
        Self {
            directory,
            inventory,
            payments,
            shop,
            client_driver: drivers[0],
            shop_driver: drivers[1],
            inventory_driver: drivers[2],
            payments_driver: drivers[3],
        }
    }
    async fn spawn(
        path: &Path,
        role: &str,
        driver: Driver,
        address: Option<SocketAddr>,
        backends: Option<(SocketAddr, SocketAddr)>,
    ) -> Node {
        let pki = path.join("pki");
        let mut command = Command::new(env!("CARGO_BIN_EXE_checkout-node"));
        command
            .kill_on_drop(true)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .arg(role)
            .arg("--listen")
            .arg(address.map_or("127.0.0.1:0".into(), |v| v.to_string()))
            .arg("--driver")
            .arg(match driver {
                Driver::Legacy => "legacy",
                Driver::Portable => "portable",
            })
            .arg("--db")
            .arg(path.join(format!("{role}.sqlite")))
            .arg("--config")
            .arg(path.join("config.json"))
            .arg("--cert")
            .arg(pki.join("server.pem"))
            .arg("--key")
            .arg(pki.join("server-key.pem"))
            .arg("--ca")
            .arg(pki.join("ca.pem"))
            .arg("--faults")
            .arg(path.join(format!("{role}-faults.json")));
        if let Some((inventory, payments)) = backends {
            command
                .arg("--inventory")
                .arg(inventory.to_string())
                .arg("--payments")
                .arg(payments.to_string())
                .arg("--client-cert")
                .arg(pki.join("client.pem"))
                .arg("--client-key")
                .arg(pki.join("client-key.pem"));
        }
        let mut child = command.spawn().unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let ready = tokio::time::timeout(TIMEOUT, lines.next_line())
            .await
            .expect("readiness deadline")
            .unwrap()
            .expect("node exited before readiness");
        let ready: serde_json::Value = serde_json::from_str(&ready).unwrap();
        let address = ready["address"].as_str().unwrap().parse().unwrap();
        // Keep draining: RPC implementations may also emit connection diagnostics.
        // Dropping a live child's stdout would make its next println panic.
        let output = tokio::task::spawn_local(async move {
            while let Ok(Some(_line)) = lines.next_line().await {}
        });
        Node {
            child,
            address,
            output: Some(output),
        }
    }
    pub fn path(&self) -> &Path {
        self.directory.path()
    }
    pub fn public_wire(&self) -> Wire {
        Wire::new(
            self.shop.address,
            self.client_driver,
            &self.path().join("pki/ca.pem"),
            None,
        )
        .unwrap()
    }
    pub async fn client(&self, account: &str) -> Client {
        Client::connect(
            &self.public_wire(),
            account,
            &format!("{account}-development-token"),
        )
        .await
        .unwrap()
    }
    pub fn resource(&self, payments: bool) -> ResourceClient {
        let (node, driver) = if payments {
            (&self.payments, self.payments_driver)
        } else {
            (&self.inventory, self.inventory_driver)
        };
        ResourceClient(
            Wire::new(
                node.address,
                driver,
                &self.path().join("pki/ca.pem"),
                Some(
                    capntproto::rpc::security::Identity::from_pem_files(
                        self.path().join("pki/client.pem"),
                        self.path().join("pki/client-key.pem"),
                    )
                    .unwrap(),
                ),
            )
            .unwrap(),
        )
    }
    pub async fn audit(&self) -> (Audit, Audit) {
        (
            self.resource(false).audit().await.unwrap(),
            self.resource(true).audit().await.unwrap(),
        )
    }
    pub async fn restart_inventory(&mut self) {
        let address = self.inventory.address;
        self.inventory.stop().await;
        self.inventory = Self::spawn(
            self.path(),
            "inventory",
            self.inventory_driver,
            Some(address),
            None,
        )
        .await;
    }
    pub async fn restart_payments(&mut self) {
        let address = self.payments.address;
        self.payments.stop().await;
        self.payments = Self::spawn(
            self.path(),
            "payments",
            self.payments_driver,
            Some(address),
            None,
        )
        .await;
    }
    pub async fn restart_shop(&mut self) {
        let address = self.shop.address;
        self.shop.stop().await;
        self.shop = Self::spawn(
            self.path(),
            "shop",
            self.shop_driver,
            Some(address),
            Some((self.inventory.address, self.payments.address)),
        )
        .await;
    }
    pub async fn stop(&mut self) {
        self.shop.stop().await;
        self.inventory.stop().await;
        self.payments.stop().await;
    }
    pub async fn hit(&self, id: &str) {
        wait_file(self.path().join(format!("{id}.hit"))).await;
    }
    pub fn resume(&self, id: &str) {
        std::fs::write(self.path().join(format!("{id}.resume")), b"resume").unwrap();
    }
    pub async fn done(&self, id: &str) {
        wait_file(self.path().join(format!("{id}.done"))).await;
    }
    pub async fn settled(&self, client: &Client, key: &str) -> Snapshot {
        tokio::time::timeout(TIMEOUT, async {
            loop {
                let value = client.view(key).await.unwrap();
                if value.status.terminal() {
                    return value;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("order never settled")
    }
}
pub async fn wait_file(path: PathBuf) {
    tokio::time::timeout(TIMEOUT, async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("failpoint not reached: {}", path.display()));
}
pub fn fault(
    id: &str,
    method: &str,
    key: &str,
    stage: FaultStage,
    action: FaultAction,
) -> FaultPlan {
    FaultPlan {
        id: id.into(),
        method: method.into(),
        key: key.into(),
        stage,
        action,
    }
}
pub async fn local(future: impl std::future::Future<Output = ()>) {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(120), future)
                .await
                .expect("application scenario deadline");
        })
        .await;
}
