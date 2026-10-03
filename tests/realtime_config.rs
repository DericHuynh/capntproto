use capntproto::{
    realtime::{Clock, Config, Outcome, Receiver, Sender},
    realtime_capnp::{self as wire, snapshots},
};
use futures::FutureExt;
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Debug)]
struct Raw {
    domain: Vec<u8>,
    skew: u64,
    keys: u32,
    capacity: u32,
    sequence: u64,
    payload: u32,
    waiters: u32,
}
impl Raw {
    fn small() -> Self {
        Self {
            domain: b"ticks".to_vec(),
            skew: 0,
            keys: 1,
            capacity: 1,
            sequence: 2,
            payload: 1,
            waiters: 2,
        }
    }
    fn checked(&self) -> capnp::Result<Config> {
        Config::new(
            std::str::from_utf8(&self.domain)
                .map_err(|_| capnp::Error::failed("invalid UTF-8".into()))?,
            self.skew,
            self.keys,
            self.capacity,
            self.sequence,
            self.payload,
            self.waiters,
        )
    }
    fn write(&self, mut out: wire::config::Builder<'_>) {
        out.set_clock_domain(capnp::text::Reader(&self.domain));
        out.set_clock_skew(self.skew);
        out.set_keys(self.keys);
        out.set_capacity(self.capacity);
        out.set_max_sequence(self.sequence);
        out.set_max_payload_bytes(self.payload);
        out.set_max_waiters(self.waiters);
    }
    fn assert_matches(&self, c: &Config) {
        assert_eq!(c.clock_domain().as_bytes(), self.domain);
        assert_eq!(c.clock_skew(), self.skew);
        assert_eq!(c.keys(), self.keys);
        assert_eq!(c.capacity(), self.capacity);
        assert_eq!(c.max_sequence(), self.sequence);
        assert_eq!(c.max_payload_bytes(), self.payload);
        assert_eq!(c.max_waiters(), self.waiters);
    }
}
fn invalid() -> Vec<Raw> {
    vec![
        Raw {
            domain: vec![],
            ..Raw::small()
        },
        Raw {
            domain: vec![b'a'; 129],
            ..Raw::small()
        },
        Raw {
            domain: "é".repeat(65).into_bytes(),
            ..Raw::small()
        },
        Raw {
            domain: vec![0xff],
            ..Raw::small()
        },
        Raw {
            keys: 0,
            ..Raw::small()
        },
        Raw {
            keys: 1025,
            ..Raw::small()
        },
        Raw {
            capacity: 0,
            ..Raw::small()
        },
        Raw {
            capacity: 2,
            ..Raw::small()
        },
        Raw {
            sequence: 0,
            ..Raw::small()
        },
        Raw {
            sequence: 65537,
            ..Raw::small()
        },
        Raw {
            payload: 0,
            ..Raw::small()
        },
        Raw {
            payload: 1048577,
            ..Raw::small()
        },
        Raw {
            waiters: 0,
            ..Raw::small()
        },
        Raw {
            waiters: 65537,
            ..Raw::small()
        },
        Raw {
            keys: 33,
            capacity: 32,
            payload: 1048576,
            ..Raw::small()
        },
        Raw {
            keys: 1024,
            capacity: 1024,
            payload: 32769,
            ..Raw::small()
        },
        Raw {
            keys: u32::MAX,
            capacity: u32::MAX,
            ..Raw::small()
        },
        Raw {
            sequence: u64::MAX,
            ..Raw::small()
        },
        Raw {
            payload: u32::MAX,
            ..Raw::small()
        },
        Raw {
            waiters: u32::MAX,
            ..Raw::small()
        },
    ]
}
struct TestClock;
impl Clock for TestClock {
    fn now(&self) -> u64 {
        0
    }
}

#[test]
fn checked_limits_cover_exact_boundaries_and_preserve_owned_clock_metadata() {
    for raw in invalid() {
        assert!(raw.checked().is_err(), "{raw:?}");
    }
    for raw in [
        Raw::small(),
        Raw {
            domain: "é".repeat(64).into_bytes(),
            skew: u64::MAX,
            ..Raw::small()
        },
        Raw {
            keys: 32,
            capacity: 32,
            sequence: 65536,
            payload: 1048576,
            waiters: 65536,
            ..Raw::small()
        },
        Raw {
            keys: 1024,
            capacity: 1024,
            payload: 32768,
            ..Raw::small()
        },
    ] {
        let config = raw.checked().unwrap();
        raw.assert_matches(&config);
        assert_eq!(config.clone(), config);
        let (receiver, _client) = Receiver::new(config, Rc::new(TestClock));
        assert!(receiver.pending().is_empty());
        assert_eq!(receiver.waiter_count(), 0);
        assert!(!receiver.is_closed());
    }
    let mut domain = String::from("original");
    let config = Config::new(&domain, 0, 1, 1, 1, 1, 1).unwrap();
    domain.clear();
    assert_eq!(config.clock_domain(), "original");
}

struct Advertised {
    raw: Raw,
    calls: Rc<Cell<usize>>,
}
impl snapshots::Server for Advertised {
    async fn describe(
        self: Rc<Self>,
        _: snapshots::DescribeParams,
        mut out: snapshots::DescribeResults,
    ) -> capnp::Result<()> {
        self.raw.write(out.get().init_config());
        Ok(())
    }
    async fn offer(
        self: Rc<Self>,
        _: snapshots::OfferParams,
        _: snapshots::OfferResults,
    ) -> capnp::Result<()> {
        self.calls.set(self.calls.get() + 1);
        Ok(())
    }
    async fn cancel(
        self: Rc<Self>,
        _: snapshots::CancelParams,
        _: snapshots::CancelResults,
    ) -> capnp::Result<()> {
        self.calls.set(self.calls.get() + 1);
        Ok(())
    }
    async fn close(
        self: Rc<Self>,
        _: snapshots::CloseParams,
        _: snapshots::CloseResults,
    ) -> capnp::Result<()> {
        self.calls.set(self.calls.get() + 1);
        Ok(())
    }
}
fn advertised(raw: Raw) -> (snapshots::Client, Rc<Cell<usize>>) {
    let calls = Rc::new(Cell::new(0));
    (
        capnp_rpc::new_client(Advertised {
            raw,
            calls: calls.clone(),
        }),
        calls,
    )
}

#[tokio::test(flavor = "current_thread")]
async fn hostile_wire_limits_fail_before_offers_and_valid_limits_roundtrip() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for raw in invalid() {
                let (service, calls) = advertised(raw.clone());
                let (a, b) = tokio::io::duplex(4096);
                let server = capntproto::rpc::serve(b, service.client);
                let (remote, driver) = capntproto::rpc::client(a);
                assert!(Sender::connect(remote).await.is_err(), "{raw:?}");
                assert_eq!(calls.get(), 0);
                server.abort();
                driver.abort();
                let _ = server.await;
                let _ = driver.await;
            }
            let config = Config::new("é", 0, 1, 1, 1, 1, 1).unwrap();
            let (receiver, service) = Receiver::new(config.clone(), Rc::new(TestClock));
            let (a, b) = tokio::io::duplex(4096);
            let server = capntproto::rpc::serve(b, service.client);
            let (remote, driver) = capntproto::rpc::client(a);
            let sender = Sender::connect(remote).await.unwrap();
            assert_eq!(sender.config(), &config);
            assert!(sender.offer(1, 2, b"a").is_err());
            assert!(sender.offer(0, 2, b"ab").is_err());
            let receipt = sender.offer(0, 2, b"a").unwrap();
            assert_eq!(receipt.sequence(), 1);
            assert!(sender.offer(0, 2, b"b").is_err());
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                while receiver.pending().is_empty() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert_eq!(receiver.apply(1).unwrap(), Outcome::Applied);
            assert_eq!(receipt.outcome().await.unwrap(), Outcome::Applied);
            assert_eq!(receiver.get(0).unwrap().bytes(), b"a");
            server.abort();
            driver.abort();
            let _ = server.await;
            let _ = driver.await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_realtime_config_import_and_admission_traces() {
    use capntproto_test_support::verification::exploration;
    const MODEL: &str = "verification/RealtimeConfigBoundary.tla";
    const CONFIG: &str = include_str!("../verification/RealtimeConfigBoundary.cfg");
    let paths = exploration::traces(MODEL, "realtime-config-boundary", CONFIG).unwrap();
    for path in &paths {
        let mut receiver = None;
        let mut receipt = None;
        let mut duplicate = None;
        for state in path {
            let result = match state["event"] {
                1 => {
                    let raw = Raw {
                        domain: vec![
                            if state["utf8"] == 1 { b'x' } else { 0xff };
                            state["domain"] as usize
                        ],
                        skew: state["skew"],
                        keys: state["keys"] as u32,
                        capacity: state["capacity"] as u32,
                        sequence: state["limit"],
                        payload: state["payload"] as u32,
                        waiters: state["waiters"] as u32,
                    };
                    let imported = if state["source"] == 1 {
                        raw.checked().ok()
                    } else {
                        let (service, calls) = advertised(raw.clone());
                        let imported = Sender::connect(service)
                            .await
                            .ok()
                            .map(|s| s.config().clone());
                        assert_eq!(calls.get(), 0);
                        imported
                    };
                    receiver = imported.map(|c| {
                        raw.assert_matches(&c);
                        Receiver::new(c, Rc::new(TestClock))
                    });
                    u64::from(receiver.is_some())
                }
                2 | 3 => {
                    let r = &receiver.as_ref().unwrap().0;
                    let mut p = r.offer(
                        state["sequence"],
                        state["key"] as u32,
                        2,
                        &vec![b'x'; state["size"] as usize],
                    );
                    match (&mut p).now_or_never() {
                        None => {
                            if state["event"] == 2 {
                                receipt = Some(p);
                            } else {
                                duplicate = Some(p);
                            }
                            1
                        }
                        Some(Ok(Outcome::Expired)) => 1,
                        Some(Err(error)) => match error.kind {
                            capnp::ErrorKind::Failed => 0,
                            capnp::ErrorKind::Overloaded => 2,
                            _ => panic!("unexpected error: {error:?}"),
                        },
                        Some(other) => panic!("unexpected receipt: {other:?}"),
                    }
                }
                4 => {
                    let r = &receiver.as_ref().unwrap().0;
                    let applied = r.apply(state["sequence"]);
                    for pending in [receipt.take(), duplicate.take()].into_iter().flatten() {
                        assert_eq!(pending.now_or_never().unwrap().unwrap(), Outcome::Applied);
                    }
                    u64::from(applied.is_ok())
                }
                _ => panic!("unknown event: {state:?}"),
            };
            assert_eq!(result, state["result"], "{path:?}");
            assert_eq!(u64::from(receiver.is_some()), state["accepted"], "{path:?}");
            if let Some((r, _)) = &receiver {
                assert_eq!(r.waiter_count() as u64, state["held"], "{path:?}");
                assert_eq!(
                    r.pending().len() as u64,
                    u64::from(state["outcome"] == 1),
                    "{path:?}"
                );
                let outcome = match r.status(state["sequence"]) {
                    None => {
                        if r.pending().is_empty() {
                            0
                        } else {
                            1
                        }
                    }
                    Some(Outcome::Applied) => 2,
                    Some(Outcome::Expired) => 3,
                    other => panic!("unexpected status: {other:?}"),
                };
                assert_eq!(outcome, state["outcome"], "{path:?}");
                let published = r.get(state["key"] as u32);
                assert_eq!(
                    u64::from(published.is_some()),
                    state["published"],
                    "{path:?}"
                );
                if let Some(snapshot) = published {
                    assert_eq!(snapshot.sequence(), state["sequence"]);
                    assert_eq!(snapshot.key() as u64, state["key"]);
                    assert_eq!(snapshot.not_after(), 2);
                    assert_eq!(snapshot.bytes(), vec![b'x'; state["size"] as usize]);
                }
            }
        }
    }
    exploration::controls(
        MODEL,
        "realtime-config-boundary",
        CONFIG,
        &[
            ("domain", "ExpectedImport"),
            ("utf8", "ExpectedImport"),
            ("keys", "ExpectedImport"),
            ("capacity", "ExpectedImport"),
            ("sequence", "ExpectedImport"),
            ("payload", "ExpectedImport"),
            ("waiters", "ExpectedImport"),
            ("budget", "ExpectedImport"),
            ("admission", "ExpectedOperation"),
            ("quota", "ExpectedOperation"),
            ("deadline", "DeadlineSafety"),
        ],
        None,
    )
    .unwrap();
    eprintln!(
        "{} realtime config and admission edge-prefix replays",
        paths.len()
    );
}
