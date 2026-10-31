#[path = "support/cluster.rs"]
mod cluster;
#[path = "support/model.rs"]
mod model;
use capntproto_checkout_example::{
    BusinessError, Client, Driver, FaultAction as Action, FaultStage as Stage, Snapshot, State,
    Status,
};
use cluster::{fault, local, Cluster};
use model::{Operation, Record};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    time::Duration,
};
fn profiles() -> [[Driver; 4]; 4] {
    use Driver::*;
    [
        [Legacy; 4],
        [Portable; 4],
        [Portable, Legacy, Portable, Legacy],
        [Legacy, Portable, Legacy, Portable],
    ]
}
async fn public_orders(cluster: &Cluster) -> BTreeMap<(String, String), Snapshot> {
    let mut orders = BTreeMap::new();
    for account in ["alice", "bob"] {
        let client = cluster.client(account).await;
        for row in client.list().await.unwrap() {
            let row = cluster.settled(&client, &row.key).await;
            orders.insert((account.into(), row.key.clone()), row);
        }
        client.close().await.unwrap();
    }
    orders
}
async fn check(cluster: &Cluster, accepted: &BTreeSet<(String, String)>) {
    let orders = public_orders(cluster).await;
    let (i, p) = cluster.audit().await;
    let seed: capntproto_checkout_example::Config =
        serde_json::from_slice(&std::fs::read(cluster.path().join("config.json")).unwrap())
            .unwrap();
    assert_eq!(i.buckets.len(), 1);
    assert_eq!(i.buckets[0].initial, seed.stock);
    let expected: BTreeMap<_, _> = seed
        .accounts
        .iter()
        .map(|a| (a.name.clone(), a.cents))
        .collect();
    let actual: BTreeMap<_, _> = p
        .buckets
        .iter()
        .map(|b| (b.name.clone(), b.initial))
        .collect();
    assert_eq!(
        actual, expected,
        "service changed the provisioned money supply"
    );
    model::audit(&orders, accepted, &i, &p)
        .unwrap_or_else(|e| panic!("{e}\norders={orders:#?}\ninventory={i:#?}\npayments={p:#?}"));
}
#[tokio::test(flavor = "current_thread")]
async fn customers_compete_for_limited_stock_and_money_without_duplicate_effects() {
    local(async {
        for profile in profiles() {
            let mut cluster = Cluster::start(profile, 7, 6000, [vec![], vec![], vec![]]).await;
            let alice = cluster.client("alice").await;
            let bob = cluster.client("bob").await;
            let requests = (0..24).map(|index| {
                let client = if index % 2 == 0 { &alice } else { &bob };
                async move {
                    let key = format!("sale-{index}");
                    let receipt = client.buy(&key, 1).await.unwrap();
                    let replay = client.buy(&key, 1).await.unwrap();
                    assert_eq!(receipt, replay);
                    assert!(matches!(
                        receipt.status,
                        Status::Confirmed | Status::Declined
                    ));
                    (if index % 2 == 0 { "alice" } else { "bob" }.to_owned(), key)
                }
            });
            let accepted = futures::future::join_all(requests)
                .await
                .into_iter()
                .collect();
            check(&cluster, &accepted).await;
            let orders = public_orders(&cluster).await;
            assert!(
                orders
                    .values()
                    .filter(|o| o.status == Status::Confirmed)
                    .count()
                    <= 7
            );
            assert_eq!(
                alice.place("sale-0", 2).await.unwrap_err(),
                BusinessError::Conflict
            );
            assert_eq!(
                alice.place("bad", 0).await.unwrap_err(),
                BusinessError::Invalid
            );
            assert_eq!(
                alice.place("bad", 9).await.unwrap_err(),
                BusinessError::Invalid
            );
            assert_eq!(
                alice.place("../../bad", 1).await.unwrap_err(),
                BusinessError::Invalid
            );
            assert_eq!(
                alice.view("sale-1").await.unwrap_err(),
                BusinessError::NotFound
            );
            assert!(
                Client::connect(&cluster.public_wire(), "alice", "incorrect-token")
                    .await
                    .is_err()
            );
            // Public client authority cannot directly access the mutual-TLS ledgers.
            let untrusted = capntproto_checkout_example::ResourceClient(
                capntproto_checkout_example::Wire::new(
                    cluster.inventory.address,
                    profile[0],
                    &cluster.path().join("pki/ca.pem"),
                    None,
                )
                .unwrap(),
            );
            assert!(untrusted.audit().await.is_err());
            alice.close().await.unwrap();
            bob.close().await.unwrap();
            cluster.stop().await;
        }
    })
    .await;
}
async fn observe(
    client: &Client,
    key: &str,
    operation: Operation,
    clock: Rc<Cell<u64>>,
    delay: usize,
) -> Record {
    for _ in 0..delay {
        tokio::task::yield_now().await;
    }
    let start = clock.get();
    clock.set(start + 1);
    let result = match operation {
        Operation::Place(q) => client.place(key, q).await,
        Operation::Cancel => client.cancel(key).await,
        Operation::View => client.view(key).await,
    };
    let end = clock.get();
    clock.set(end + 1);
    Record {
        start,
        end,
        operation,
        result,
    }
}
#[tokio::test(flavor = "current_thread")]
async fn concurrent_user_histories_have_a_legal_explanation() {
    local(async {
        let seeds = std::env::var("CHECKOUT_HISTORY_SEEDS")
            .unwrap_or_else(|_| "64".into())
            .parse::<usize>()
            .expect("CHECKOUT_HISTORY_SEEDS must be an integer");
        assert!((1..=2048).contains(&seeds));
        for profile in profiles() {
            let mut cluster = Cluster::start(profile, 256, 200_000, [vec![], vec![], vec![]]).await;
            let clients = [
                cluster.client("alice").await,
                cluster.client("alice").await,
                cluster.client("alice").await,
            ];
            let mut accepted = BTreeSet::new();
            for seed in 0..seeds {
                let key = format!("race-{seed}");
                let clock = Rc::new(Cell::new(0));
                let operations = [
                    Operation::Place(1),
                    Operation::Cancel,
                    Operation::View,
                    Operation::Place(1),
                    Operation::Place(2),
                    Operation::Cancel,
                    Operation::View,
                ];
                let records =
                    futures::future::join_all(operations.into_iter().enumerate().map(|(i, op)| {
                        observe(
                            &clients[i % 3],
                            &key,
                            op,
                            clock.clone(),
                            (seed.wrapping_mul(17) ^ (i * 13)) % 7,
                        )
                    }))
                    .await;
                assert!(
                    model::linearizable(&records, &key),
                    "nonlinearizable profile={profile:?} seed={seed}\n{}",
                    serde_json::to_string_pretty(&records).unwrap()
                );
                for record in &records {
                    if matches!(record.operation, Operation::Place(_)) && record.result.is_ok() {
                        accepted.insert(("alice".into(), key.clone()));
                    }
                }
                let terminal = cluster.settled(&clients[0], &key).await;
                assert_eq!(
                    clients[1].cancel(&key).await.unwrap(),
                    terminal,
                    "a terminal receipt changed"
                );
            }
            check(&cluster, &accepted).await;
            eprintln!(
                "validated profile={profile:?}: {seeds} histories, {} completed user operations",
                seeds * 7
            );
            for client in clients {
                client.close().await.unwrap();
            }
            cluster.stop().await;
        }
    })
    .await;
}
#[tokio::test(flavor = "current_thread")]
async fn late_reservation_after_cancel_cannot_resurrect_stock_or_charge() {
    local(async {
        for driver in [Driver::Legacy, Driver::Portable] {
            let plan = fault(
                "late",
                "reserve",
                "alice/undo",
                Stage::Before,
                Action::Pause,
            );
            let mut cluster =
                Cluster::start([driver; 4], 8, 20_000, [vec![plan], vec![], vec![]]).await;
            let client = cluster.client("alice").await;
            client.place("undo", 3).await.unwrap();
            cluster.hit("late").await;
            assert_eq!(
                client.cancel("undo").await.unwrap().status,
                Status::Cancelling
            );
            let cancelled = cluster.settled(&client, "undo").await;
            assert_eq!(cancelled.status, Status::Cancelled);
            cluster.resume("late");
            cluster.done("late").await;
            assert_eq!(client.view("undo").await.unwrap(), cancelled);
            let (inventory, _) = cluster.audit().await;
            assert_eq!(
                inventory
                    .entries
                    .iter()
                    .find(|e| e.claim.key == "alice/undo")
                    .unwrap()
                    .state,
                State::Released
            );
            check(&cluster, &BTreeSet::from([("alice".into(), "undo".into())])).await;
            client.close().await.unwrap();
            cluster.stop().await;
        }
    })
    .await;
}
#[tokio::test(flavor = "current_thread")]
async fn charge_committed_before_reply_loss_is_recovered_exactly_once() {
    local(async {
        for driver in [Driver::Legacy, Driver::Portable] {
            let plan = fault(
                "charge",
                "commit",
                "alice/retry",
                Stage::After,
                Action::Crash,
            );
            let mut cluster =
                Cluster::start([driver; 4], 8, 20_000, [vec![], vec![plan], vec![]]).await;
            let client = cluster.client("alice").await;
            client.place("retry", 3).await.unwrap();
            cluster.hit("charge").await;
            cluster.payments.crashed().await;
            assert!(
                !client.view("retry").await.unwrap().status.terminal(),
                "uncertain debit was falsely finalized"
            );
            cluster.restart_payments().await;
            let receipt = cluster.settled(&client, "retry").await;
            assert_eq!(receipt.status, Status::Confirmed);
            assert_eq!(receipt.cents, 2700);
            for _ in 0..5 {
                assert_eq!(client.buy("retry", 3).await.unwrap(), receipt);
            }
            check(
                &cluster,
                &BTreeSet::from([("alice".into(), "retry".into())]),
            )
            .await;
            client.close().await.unwrap();
            cluster.stop().await;
        }
    })
    .await;
}
#[tokio::test(flavor = "current_thread")]
async fn lost_acceptance_reply_survives_order_service_restart() {
    local(async {
        for driver in [Driver::Legacy, Driver::Portable] {
            let plan = fault(
                "accept",
                "place",
                "alice/accepted",
                Stage::After,
                Action::Crash,
            );
            let mut cluster =
                Cluster::start([driver; 4], 8, 20_000, [vec![], vec![], vec![plan]]).await;
            let client = cluster.client("alice").await;
            assert!(client.place("accepted", 2).await.is_err());
            cluster.hit("accept").await;
            cluster.shop.crashed().await;
            drop(client);
            cluster.restart_shop().await;
            let client = cluster.client("alice").await;
            let receipt = client.buy("accepted", 2).await.unwrap();
            assert_eq!(receipt.status, Status::Confirmed);
            assert_eq!(client.buy("accepted", 2).await.unwrap(), receipt);
            check(
                &cluster,
                &BTreeSet::from([("alice".into(), "accepted".into())]),
            )
            .await;
            client.close().await.unwrap();
            cluster.stop().await;
        }
    })
    .await;
}
#[tokio::test(flavor = "current_thread")]
async fn acknowledged_cancel_survives_partition_and_crash_during_compensation() {
    local(async {
        for driver in [Driver::Legacy, Driver::Portable] {
            let plan = fault(
                "refund",
                "release",
                "alice/refund",
                Stage::After,
                Action::Pause,
            );
            let mut cluster =
                Cluster::start([driver; 4], 8, 20_000, [vec![], vec![plan], vec![]]).await;
            cluster.payments.stop().await;
            let client = cluster.client("alice").await;
            client.place("refund", 2).await.unwrap();
            assert_eq!(
                client.cancel("refund").await.unwrap().status,
                Status::Cancelling
            );
            // An unavailable ledger must not be reported as a completed refund.
            tokio::time::sleep(Duration::from_millis(80)).await;
            assert_eq!(
                client.view("refund").await.unwrap().status,
                Status::Cancelling
            );
            cluster.restart_payments().await;
            cluster.hit("refund").await;
            cluster.shop.stop().await;
            drop(client);
            cluster.resume("refund");
            cluster.done("refund").await;
            cluster.restart_shop().await;
            let client = cluster.client("alice").await;
            assert_eq!(
                cluster.settled(&client, "refund").await.status,
                Status::Cancelled
            );
            check(
                &cluster,
                &BTreeSet::from([("alice".into(), "refund".into())]),
            )
            .await;
            client.close().await.unwrap();
            cluster.stop().await;
        }
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn cancel_after_capture_wins_before_confirmation_and_refunds_the_charge() {
    local(async {
        for driver in [Driver::Legacy, Driver::Portable] {
            let plan = fault(
                "capture",
                "commit",
                "alice/captured",
                Stage::After,
                Action::Pause,
            );
            let mut cluster =
                Cluster::start([driver; 4], 8, 20_000, [vec![], vec![plan], vec![]]).await;
            let client = cluster.client("alice").await;
            client.place("captured", 3).await.unwrap();
            cluster.hit("capture").await;
            assert_eq!(
                client.cancel("captured").await.unwrap().status,
                Status::Cancelling
            );
            cluster.resume("capture");
            cluster.done("capture").await;
            let receipt = cluster.settled(&client, "captured").await;
            assert_eq!(receipt.status, Status::Cancelled);
            assert_eq!(client.buy("captured", 3).await.unwrap(), receipt);
            check(
                &cluster,
                &BTreeSet::from([("alice".into(), "captured".into())]),
            )
            .await;
            client.close().await.unwrap();
            cluster.stop().await;
        }
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn cancellation_during_a_declined_charge_does_not_turn_into_a_purchase_or_refund() {
    local(async {
        for driver in [Driver::Legacy, Driver::Portable] {
            let plan = fault(
                "decline",
                "reserve",
                "alice/insufficient",
                Stage::After,
                Action::Pause,
            );
            let mut cluster =
                Cluster::start([driver; 4], 8, 2000, [vec![], vec![plan], vec![]]).await;
            let client = cluster.client("alice").await;
            client.place("insufficient", 3).await.unwrap();
            cluster.hit("decline").await;
            assert_eq!(
                client.cancel("insufficient").await.unwrap().status,
                Status::Cancelling
            );
            cluster.resume("decline");
            cluster.done("decline").await;
            assert_eq!(
                cluster.settled(&client, "insufficient").await.status,
                Status::Cancelled
            );
            check(
                &cluster,
                &BTreeSet::from([("alice".into(), "insufficient".into())]),
            )
            .await;
            client.close().await.unwrap();
            cluster.stop().await;
        }
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn receipts_account_isolation_and_cancel_tombstones_survive_full_restart() {
    local(async {
        for driver in [Driver::Legacy, Driver::Portable] {
            let mut cluster =
                Cluster::start([driver; 4], 8, 20_000, [vec![], vec![], vec![]]).await;
            let alice = cluster.client("alice").await;
            let bob = cluster.client("bob").await;
            let a = alice.buy("shared", 2).await.unwrap();
            let b = bob.buy("shared", 1).await.unwrap();
            assert_eq!(a.status, Status::Confirmed);
            assert_eq!(b.status, Status::Confirmed);
            let tombstone = alice.cancel("never-sent").await.unwrap();
            assert_eq!(tombstone.quantity, 0);
            assert_eq!(tombstone.status, Status::Cancelled);
            assert_eq!(alice.buy("never-sent", 8).await.unwrap(), tombstone);
            assert_eq!(
                bob.view("never-sent").await.unwrap_err(),
                BusinessError::NotFound
            );
            assert_eq!(alice.cancel("shared").await.unwrap(), a);
            let before = cluster.audit().await;
            // Internal ledger replays with changed metadata must fail atomically.
            let mut changed = before.0.entries[0].claim.clone();
            changed.units += 1;
            assert!(cluster
                .resource(false)
                .apply("release", &changed)
                .await
                .is_err());
            assert_eq!(cluster.audit().await, before);
            alice.close().await.unwrap();
            bob.close().await.unwrap();
            cluster.shop.stop().await;
            cluster.inventory.stop().await;
            cluster.payments.stop().await;
            cluster.restart_inventory().await;
            cluster.restart_payments().await;
            cluster.restart_shop().await;
            let alice = cluster.client("alice").await;
            let bob = cluster.client("bob").await;
            assert_eq!(alice.view("shared").await.unwrap(), a);
            assert_eq!(bob.view("shared").await.unwrap(), b);
            assert_eq!(alice.buy("never-sent", 8).await.unwrap(), tombstone);
            assert_eq!(cluster.audit().await, before);
            check(
                &cluster,
                &BTreeSet::from([
                    ("alice".into(), "shared".into()),
                    ("bob".into(), "shared".into()),
                    ("alice".into(), "never-sent".into()),
                ]),
            )
            .await;
            alice.close().await.unwrap();
            bob.close().await.unwrap();
            cluster.stop().await;
        }
    })
    .await;
}

#[test]
fn oracle_rejects_known_user_visible_failures() {
    let receipt = |status| Snapshot {
        key: "x".into(),
        quantity: 1,
        cents: 1000,
        status,
        revision: 1,
    };
    let records = vec![
        Record {
            start: 0,
            end: 1,
            operation: Operation::Place(1),
            result: Ok(Snapshot {
                revision: 0,
                ..receipt(Status::Processing)
            }),
        },
        Record {
            start: 2,
            end: 3,
            operation: Operation::Cancel,
            result: Ok(receipt(Status::Cancelling)),
        },
        Record {
            start: 4,
            end: 5,
            operation: Operation::View,
            result: Ok(receipt(Status::Confirmed)),
        },
    ];
    assert!(
        !model::linearizable(&records, "x"),
        "oracle accepted confirmation after acknowledged cancellation"
    );
    let mut valid = records[..1].to_vec();
    valid.push(Record {
        start: 2,
        end: 3,
        operation: Operation::View,
        result: Ok(receipt(Status::Confirmed)),
    });
    assert!(model::linearizable(&valid, "x"));
    let mut wrong = valid.clone();
    wrong[1].result.as_mut().unwrap().quantity = 2;
    assert!(!model::linearizable(&wrong, "x"));
    let mut wrong = valid.clone();
    wrong[1].result.as_mut().unwrap().cents = 1;
    assert!(!model::linearizable(&wrong, "x"));
    let missing = BTreeMap::new();
    let accepted = BTreeSet::from([("alice".into(), "x".into())]);
    let empty = capntproto_checkout_example::Audit {
        buckets: vec![],
        entries: vec![],
    };
    assert!(model::audit(&missing, &accepted, &empty, &empty).is_err());
    let orders = BTreeMap::from([(("alice".into(), "x".into()), receipt(Status::Confirmed))]);
    assert!(
        model::audit(&orders, &accepted, &empty, &empty).is_err(),
        "oracle accepted a confirmed order without a debit"
    );
}

#[test]
fn audit_oracle_rejects_double_charge_refund_omission_and_created_money() {
    use capntproto_checkout_example::{Audit, Bucket, Claim, Entry};
    let receipt = Snapshot {
        key: "x".into(),
        quantity: 1,
        cents: 1000,
        status: Status::Confirmed,
        revision: 1,
    };
    let claim = Claim {
        key: "alice/x".into(),
        bucket: "tickets".into(),
        units: 1,
        fingerprint: "alice:1:1000".into(),
    };
    let inventory = Audit {
        buckets: vec![Bucket {
            name: "tickets".into(),
            initial: 1,
            available: 0,
        }],
        entries: vec![Entry {
            claim: claim.clone(),
            state: State::Committed,
        }],
    };
    let payments = Audit {
        buckets: vec![Bucket {
            name: "alice".into(),
            initial: 2000,
            available: 1000,
        }],
        entries: vec![Entry {
            claim: Claim {
                bucket: "alice".into(),
                units: 1000,
                ..claim
            },
            state: State::Committed,
        }],
    };
    let orders = BTreeMap::from([(("alice".into(), "x".into()), receipt)]);
    let accepted = orders.keys().cloned().collect();
    assert!(model::audit(&orders, &accepted, &inventory, &payments).is_ok());
    let mut double_charge = payments.clone();
    double_charge.buckets[0].available = 0;
    assert!(model::audit(&orders, &accepted, &inventory, &double_charge).is_err());
    let mut duplicate = payments.clone();
    duplicate.entries.push(duplicate.entries[0].clone());
    assert!(model::audit(&orders, &accepted, &inventory, &duplicate).is_err());
    let mut money_created = payments.clone();
    money_created.buckets[0].available = 2000;
    assert!(model::audit(&orders, &accepted, &inventory, &money_created).is_err());
    let mut cancelled = orders.clone();
    cancelled.values_mut().next().unwrap().status = Status::Cancelled;
    assert!(model::audit(&cancelled, &accepted, &inventory, &payments).is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn command_line_customer_retries_and_lists_real_purchases() {
    local(async {
        let mut cluster = Cluster::start(
            [
                Driver::Portable,
                Driver::Legacy,
                Driver::Portable,
                Driver::Legacy,
            ],
            8,
            20_000,
            [vec![], vec![], vec![]],
        )
        .await;
        let run = |arguments: Vec<&'static str>, token: &'static str| {
            let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_checkout-client"));
            command
                .kill_on_drop(true)
                .arg("--address")
                .arg(cluster.shop.address.to_string())
                .arg("--ca")
                .arg(cluster.path().join("pki/ca.pem"))
                .arg("--account")
                .arg("alice")
                .env("CHECKOUT_TOKEN", token)
                .args(arguments);
            async move {
                tokio::time::timeout(cluster::TIMEOUT, command.output())
                    .await
                    .unwrap()
                    .unwrap()
            }
        };
        let first = run(vec!["buy", "cli-order", "3"], "alice-development-token").await;
        assert!(
            first.status.success(),
            "{}",
            String::from_utf8_lossy(&first.stderr)
        );
        let receipt: Snapshot = serde_json::from_slice(&first.stdout).unwrap();
        assert_eq!(receipt.status, Status::Confirmed);
        assert_eq!(receipt.cents, 2700);
        let retry = run(vec!["buy", "cli-order", "3"], "alice-development-token").await;
        assert!(retry.status.success());
        assert_eq!(
            serde_json::from_slice::<Snapshot>(&retry.stdout).unwrap(),
            receipt
        );
        let list = run(vec!["list"], "alice-development-token").await;
        assert!(list.status.success());
        assert_eq!(
            serde_json::from_slice::<Vec<Snapshot>>(&list.stdout).unwrap(),
            vec![receipt]
        );
        let denied = run(vec!["list"], "wrong-token").await;
        assert!(!denied.status.success());
        check(
            &cluster,
            &BTreeSet::from([("alice".into(), "cli-order".into())]),
        )
        .await;
        cluster.stop().await;
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn failed_cli_purchase_keeps_stdout_clean_and_recovers_the_same_request() {
    local(async {
        for driver in [Driver::Legacy, Driver::Portable] {
            let plan = fault(
                "cli-crash",
                "place",
                "alice/cli-lost",
                Stage::After,
                Action::Crash,
            );
            let mut cluster =
                Cluster::start([driver; 4], 8, 20_000, [vec![], vec![], vec![plan]]).await;
            let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_checkout-client"));
            command
                .kill_on_drop(true)
                .arg("--address")
                .arg(cluster.shop.address.to_string())
                .arg("--ca")
                .arg(cluster.path().join("pki/ca.pem"))
                .arg("--account")
                .arg("alice")
                .arg("--driver")
                .arg(match driver {
                    Driver::Legacy => "legacy",
                    Driver::Portable => "portable",
                })
                .env("CHECKOUT_TOKEN", "alice-development-token")
                .args(["buy", "cli-lost", "3"]);
            let failed = tokio::time::timeout(cluster::TIMEOUT, command.output())
                .await
                .unwrap()
                .unwrap();
            assert!(!failed.status.success());
            assert!(
                failed.stdout.is_empty(),
                "library diagnostics polluted machine-readable stdout: {}",
                String::from_utf8_lossy(&failed.stdout)
            );
            cluster.shop.crashed().await;
            cluster.restart_shop().await;
            let client = cluster.client("alice").await;
            assert_eq!(
                client.buy("cli-lost", 3).await.unwrap().status,
                Status::Confirmed
            );
            check(
                &cluster,
                &BTreeSet::from([("alice".into(), "cli-lost".into())]),
            )
            .await;
            client.close().await.unwrap();
            cluster.stop().await;
        }
    })
    .await;
}
