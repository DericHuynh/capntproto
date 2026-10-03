use super::*;
use crate::native_rpc::{Connector, Network, RouteStatus};
use std::{collections::BTreeSet, rc::Rc};

#[test]
fn generation_is_nonzero_monotonic_and_network_local() {
    let a = Generations::new();
    let b = Generations::new();
    let first = a.allocate().unwrap();
    assert_eq!(first.get(), 1);
    assert_eq!(first.to_string(), "1");
    assert!(a.allocate().unwrap() > first);
    assert_eq!(b.allocate().unwrap().get(), 1);
    let boundary = Generations {
        next: Cell::new(Some(fixture(u64::MAX))),
    };
    assert_eq!(boundary.allocate().unwrap().get(), u64::MAX);
    for _ in 0..2 {
        assert_eq!(
            boundary.allocate().unwrap_err().kind,
            capnp::ErrorKind::Overloaded
        );
        assert!(boundary.next.get().is_none());
    }
}

#[test]
fn replay_tlc_route_generation_exhaustion() {
    use capntproto_test_support::verification::exploration;
    const MODEL: &str = "verification/RouteGeneration.tla";
    const CONFIG: &str = include_str!("../../../verification/RouteGeneration.cfg");
    let paths = exploration::traces(MODEL, "route-generation", CONFIG).unwrap();
    let project = |id: RouteGeneration| id.get() - (u64::MAX - 3);
    for path in &paths {
        let allocator = Generations {
            next: Cell::new(Some(fixture(u64::MAX - 2))),
        };
        let mut observers = [None, None];
        let mut issued = BTreeSet::new();
        let (mut last, mut ok, mut available) = (0, 0, 0);
        for state in path {
            let event = state["event"];
            let slot = ((event - 1) % 2) as usize;
            if event <= 2 {
                assert!(observers[slot].is_none());
                available = u64::from(allocator.next.get().is_some());
                match allocator.allocate() {
                    Ok(id) => {
                        assert!(issued.insert(id), "reissued a retained or released ID");
                        observers[slot] = Some(id);
                        last = project(id);
                        ok = 1;
                    }
                    Err(error) => {
                        assert_eq!(error.kind, capnp::ErrorKind::Overloaded);
                        last = 0;
                        ok = 0;
                    }
                }
            } else {
                assert!(observers[slot].take().is_some());
            }
            let used = issued.iter().map(|id| 1u64 << (project(*id) - 1)).sum();
            for (field, actual) in [
                ("next", allocator.next.get().map_or(0, project)),
                ("a", observers[0].map_or(0, project)),
                ("b", observers[1].map_or(0, project)),
                ("used", used),
                ("last", last),
                ("ok", ok),
                ("available", available),
            ] {
                assert_eq!(actual, state[field], "{field}: {path:?}");
            }
        }
    }
    let live =
        CONFIG.replace("SPECIFICATION Spec", "SPECIFICATION LiveSpec") + "\nPROPERTY Progress\n";
    exploration::controls(
        MODEL,
        "route-generation",
        CONFIG,
        &[
            ("zero", "Nonzero"),
            ("early", "AllocationContract"),
            ("wrap", "Retired"),
            ("recycle", "NoReuse"),
        ],
        Some(&live),
    )
    .unwrap();
    eprintln!("{} native generation edge-prefix replays", paths.len());
}

struct CountingConnector(Cell<u64>);
impl Connector for CountingConnector {
    fn connect(
        &self,
        _: [u8; 32],
    ) -> capnp::capability::Promise<crate::transport::AuthenticatedSession, capnp::Error> {
        self.0.set(self.0.get() + 1);
        capnp::capability::Promise::from_future(futures::future::pending())
    }
}

#[tokio::test(flavor = "current_thread")]
async fn exhausted_network_reuses_live_routes_and_rejects_new_dials_without_side_effects() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for arbitrated in [false, true] {
                let connector = Rc::new(CountingConnector(Cell::new(0)));
                let (network, handle) = if arbitrated {
                    Network::with_arbitration([1; 32], Some(connector.clone()), Default::default())
                        .unwrap()
                } else {
                    Network::with_connector([1; 32], connector.clone())
                };
                network.state.generations.next.set(Some(fixture(u64::MAX)));
                let route = network.state.route([2; 32]).unwrap();
                let observer = route.session.observe();
                assert_eq!(observer.generation().get(), u64::MAX);
                assert!(Rc::ptr_eq(&route, &network.state.route([2; 32]).unwrap()));
                for _ in 0..2 {
                    let Err(error) = network.state.route([3; 32]) else {
                        panic!("exhausted route allocation succeeded")
                    };
                    assert_eq!(error.kind, capnp::ErrorKind::Overloaded);
                    assert!(!network.state.routes.borrow().contains_key(&[3; 32]));
                }
                tokio::time::timeout(std::time::Duration::from_secs(1), async {
                    while connector.0.get() == 0 {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
                assert_eq!(connector.0.get(), 1);
                handle.disconnect([2; 32]);
                assert_eq!(observer.status(), RouteStatus::Stopped);
                assert!(network.state.route([2; 32]).is_err());
                assert_eq!(connector.0.get(), 1);
                assert!(network.state.routes.borrow().is_empty());
            }
        })
        .await;
}
