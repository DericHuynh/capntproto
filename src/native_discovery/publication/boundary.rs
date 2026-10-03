//! Exercise the final generation values through the actual directory and lease.
use super::*;

fn binding(host: u8) -> Binding {
    struct Provider;
    impl provisioner::Server for Provider {}
    Binding {
        host: [host; 32],
        recipient: [3; 32],
        address: "127.0.0.1:12345".parse().unwrap(),
        context: vec![host],
        provider: capnp_rpc::new_client(Provider),
    }
}
fn generation(offset: u64) -> DiscoveryGeneration {
    assert!((1..=3).contains(&offset));
    DiscoveryGeneration::new(u64::MAX - 3 + offset).unwrap()
}
fn fixture() -> (Directory, Lease) {
    let directory = Directory::default();
    directory.0.borrow_mut().next = u64::MAX - 3;
    let first = directory
        .publish("service", binding(1), None, Duration::from_secs(60))
        .unwrap();
    assert_eq!(first, generation(1));
    let lease = Lease {
        directory: directory.clone(),
        key: ("service".into(), [3; 32]),
        generation: Cell::new(first),
        lifetime: Duration::from_secs(60),
        terminal: Cell::new(None),
    };
    (directory, lease)
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn final_generation_is_issued_once_and_rejection_preserves_binding_and_deadline() {
    assert!(DiscoveryGeneration::new(0).is_none());
    assert_eq!(DiscoveryGeneration::new(1).unwrap().get(), 1);
    assert_eq!(generation(3).get(), u64::MAX);
    let (directory, lease) = fixture();
    lease.renew(Instant::now()).unwrap();
    assert_eq!(lease.generation.get(), generation(2));
    lease.renew(Instant::now()).unwrap();
    assert_eq!(lease.generation.get(), generation(3));
    let entry = directory
        .0
        .borrow()
        .entries
        .values()
        .next()
        .unwrap()
        .clone();
    assert_eq!(
        lease.renew(Instant::now()).unwrap_err().kind,
        capnp::ErrorKind::Overloaded
    );
    assert_eq!(
        directory
            .publish(
                "service",
                binding(2),
                Some(generation(3)),
                Duration::from_secs(1)
            )
            .unwrap_err()
            .kind,
        capnp::ErrorKind::Overloaded
    );
    assert_eq!(
        directory
            .publish("other", binding(2), None, Duration::from_secs(1))
            .unwrap_err()
            .kind,
        capnp::ErrorKind::Overloaded
    );
    let current = directory
        .0
        .borrow()
        .entries
        .values()
        .next()
        .unwrap()
        .clone();
    assert!(Rc::ptr_eq(&entry, &current));
    assert_eq!(current.expires, entry.expires);
    assert_eq!(directory.0.borrow().next, u64::MAX);
    let resolved = resolve(&directory.client([3; 32]), [3; 32], "service")
        .await
        .unwrap();
    assert_eq!(resolved.generation(), generation(3));
    assert!(directory.revoke("service", [3; 32], generation(3)));
    assert_eq!(
        directory
            .publish("service", binding(1), None, Duration::from_secs(1))
            .unwrap_err()
            .kind,
        capnp::ErrorKind::Overloaded
    );
    assert!(directory.0.borrow().entries.is_empty());
    assert_eq!(directory.0.borrow().next, u64::MAX);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn replay_tlc_discovery_generation_boundary() {
    use reproto_test_support::verification::exploration;
    const MODEL: &str = "verification/DiscoveryGenerationBoundary.tla";
    const CONFIG: &str = include_str!("../../../verification/DiscoveryGenerationBoundary.cfg");
    let paths = exploration::traces(MODEL, "discovery-generation-boundary", CONFIG).unwrap();
    for path in &paths {
        let (directory, lease) = fixture();
        let saved = resolve(&directory.client([3; 32]), [3; 32], "service")
            .await
            .unwrap();
        let expires = saved.expires();
        for state in path {
            let result = match state["event"] {
                1 => directory
                    .publish(
                        "service",
                        binding(state["target"] as u8),
                        if state["arg"] == 0 {
                            None
                        } else {
                            Some(generation(state["arg"]))
                        },
                        Duration::from_secs(60),
                    )
                    .is_ok(),
                2 => lease.renew(Instant::now()).is_ok(),
                3 => directory.revoke("service", [3; 32], generation(state["arg"])),
                4 => {
                    lease.stop();
                    true
                }
                5 => {
                    directory.close();
                    true
                }
                _ => panic!("invalid event: {state:?}"),
            };
            assert_eq!(u64::from(result), state["result"], "{path:?}");
            {
                let registry = directory.0.borrow();
                assert_eq!(registry.next, generation(state["issued"]).get(), "{path:?}");
                assert_eq!(
                    lease.generation.get(),
                    generation(state["owned"]),
                    "{path:?}"
                );
                assert_eq!(registry.entries.len() as u64, state["active"], "{path:?}");
                assert_eq!(u64::from(registry.closed), state["closed"], "{path:?}");
                assert_eq!(
                    u64::from(lease.terminal.get().is_some()),
                    state["stopped"],
                    "{path:?}"
                );
                if let Some(entry) = registry.entries.values().next() {
                    assert_eq!(
                        entry.generation,
                        generation(state["generation"]),
                        "{path:?}"
                    );
                    assert_eq!(entry.binding.host, [state["host"] as u8; 32], "{path:?}");
                }
            }
            let current = resolve(&directory.client([3; 32]), [3; 32], "service").await;
            assert_eq!(current.is_ok(), state["active"] == 1, "{path:?}");
            if let Ok(current) = current {
                assert_eq!(current.generation(), generation(state["generation"]));
                assert_eq!(current.binding().host, [state["host"] as u8; 32]);
            }
            // Retaining a lookup never relabels it when the name renews, rotates,
            // is revoked or the directory closes. It is not a revocation watch.
            assert_eq!(saved.generation(), generation(state["savedGeneration"]));
            assert_eq!(saved.binding().host, [state["savedHost"] as u8; 32]);
            assert_eq!(saved.binding().recipient, [3; 32]);
            assert_eq!(saved.binding().context, [1]);
            assert_eq!(saved.expires(), expires);
        }
    }
    exploration::controls(
        MODEL,
        "discovery-generation-boundary",
        CONFIG,
        &[
            ("wrap", "NoReuse"),
            ("reuse", "NoReuse"),
            ("stalePublish", "ExpectedOutcome"),
            ("staleRenew", "ExpectedOutcome"),
            ("staleStop", "SuccessorSurvives"),
            ("relabel", "CapturedBinding"),
        ],
        None,
    )
    .unwrap();
    eprintln!("{} discovery generation edge-prefix replays", paths.len());
}
