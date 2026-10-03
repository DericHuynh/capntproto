use capntproto::{
    bulk::{Config, Receiver, Sender, Status},
    bulk_capnp::transfer,
};
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Copy, Debug)]
struct Raw(u64, u32, u32, u32);
impl Raw {
    fn checked(self) -> capnp::Result<Config> {
        Config::new(self.0, self.1, self.2, self.3)
    }
    fn json(self) -> String {
        format!(
            r#"{{"length":{},"max_chunk_bytes":{},"window_bytes":{},"max_chunks":{}}}"#,
            self.0, self.1, self.2, self.3
        )
    }
}
fn raw(config: &Config) -> (u64, u32, u32, u32) {
    (
        config.length(),
        config.max_chunk_bytes(),
        config.window_bytes(),
        config.max_chunks(),
    )
}
const INVALID: &[Raw] = &[
    Raw(0, 0, 1, 1),
    Raw(0, 1, 0, 1),
    Raw(0, 2, 1, 1),
    Raw(0, 1, 1, 0),
    Raw(3, 1, 1, 2),
    Raw(67_108_865, 1_048_576, 16_777_216, 65_536),
    Raw(0, 1_048_577, 16_777_216, 1),
    Raw(0, 1, 16_777_217, 1),
    Raw(0, 1, 1, 65_537),
    Raw(67_108_864, 1_048_576, 1_048_576, 63),
    Raw(u64::MAX, 1, 1, 1),
    Raw(0, u32::MAX, u32::MAX, u32::MAX),
];

#[test]
fn checked_limits_cover_empty_exact_capacity_and_full_width_boundaries() {
    for input in INVALID {
        assert_eq!(input.checked().unwrap_err().kind, capnp::ErrorKind::Failed);
        assert!(serde_json::from_str::<Config>(&input.json()).is_err());
    }
    for input in [
        Raw(0, 1, 1, 1),
        Raw(1, 1, 1, 1),
        Raw(4, 2, 2, 2),
        Raw(67_108_864, 1_048_576, 1_048_576, 64),
        Raw(67_108_864, 1_048_576, 16_777_216, 65_536),
    ] {
        let limits = input.checked().unwrap();
        assert_eq!(raw(&limits), (input.0, input.1, input.2, input.3));
        assert_eq!(serde_json::to_string(&limits).unwrap(), input.json());
        let imported: Config = serde_json::from_str(&input.json()).unwrap();
        assert_eq!(imported, limits);
        let (receiver, _client) = Receiver::new(imported);
        assert_eq!(receiver.status(), Status::Receiving);
        assert_eq!(receiver.staged_bytes(), 0);
        assert!(receiver.completed().is_none());
        // Creating a receiver does not reserve the entire declared payload.
    }
}

#[test]
fn deserialization_cannot_bypass_validation_or_silently_change_the_limits() {
    const VALID: &str = r#"{"length":2,"max_chunk_bytes":1,"window_bytes":1,"max_chunks":2}"#;
    assert_eq!(
        raw(&serde_json::from_str::<Config>(VALID).unwrap()),
        (2, 1, 1, 2)
    );
    for invalid in [
        "{}".to_owned(),
        VALID.replace("\"max_chunks\":2", "\"max_chunks\":2,\"extra\":1"),
        VALID.replace("\"max_chunks\":2", "\"max_chunks\":2,\"max_chunks\":1"),
        VALID.replace("\"max_chunks\":2", "\"max_chunks\":null"),
        VALID.replace("\"max_chunks\":2", "\"max_chunks\":4294967296"),
        VALID.replace("\"length\":2", "\"length\":18446744073709551616"),
        VALID.replace("\"length\":2", "\"length\":-1"),
        VALID.replace("\"length\":2", "\"length\":2.0"),
        VALID.replace("\"length\":2", "\"length\":\"2\""),
    ] {
        assert!(
            serde_json::from_str::<Config>(&invalid).is_err(),
            "{invalid}"
        );
    }
}

struct Advertised {
    limits: Raw,
    calls: Rc<Cell<usize>>,
}
impl transfer::Server for Advertised {
    async fn describe(
        self: Rc<Self>,
        _: transfer::DescribeParams,
        mut out: transfer::DescribeResults,
    ) -> capnp::Result<()> {
        let mut config = out.get().init_config();
        config.set_length(self.limits.0);
        config.set_max_chunk_bytes(self.limits.1);
        config.set_window_bytes(self.limits.2);
        config.set_max_chunks(self.limits.3);
        Ok(())
    }
    async fn write(
        self: Rc<Self>,
        _: transfer::WriteParams,
        _: transfer::WriteResults,
    ) -> capnp::Result<()> {
        self.calls.set(self.calls.get() + 1);
        Ok(())
    }
    async fn done(
        self: Rc<Self>,
        _: transfer::DoneParams,
        _: transfer::DoneResults,
    ) -> capnp::Result<()> {
        self.calls.set(self.calls.get() + 1);
        Ok(())
    }
    async fn cancel(
        self: Rc<Self>,
        _: transfer::CancelParams,
        _: transfer::CancelResults,
    ) -> capnp::Result<()> {
        self.calls.set(self.calls.get() + 1);
        Ok(())
    }
}
fn advertised(limits: Raw) -> (transfer::Client, Rc<Cell<usize>>) {
    let calls = Rc::new(Cell::new(0));
    let client = capnp_rpc::new_client(Advertised {
        limits,
        calls: calls.clone(),
    });
    (client, calls)
}

#[tokio::test(flavor = "current_thread")]
async fn hostile_wire_limits_are_rejected_before_any_transfer_calls() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for input in INVALID {
                let (service, calls) = advertised(*input);
                let (a, b) = tokio::io::duplex(4096);
                let server = capntproto::rpc::serve(b, service.client);
                let (remote, driver) = capntproto::rpc::client(a);
                let error = Sender::connect(remote)
                    .await
                    .err()
                    .expect("invalid wire limits accepted");
                assert_eq!(error.kind, capnp::ErrorKind::Failed);
                assert_eq!(calls.get(), 0);
                server.abort();
                driver.abort();
                let _ = server.await;
                let _ = driver.await;
            }
            // Limits described by a real receiver survive framing and validate at
            // the sender. The exact byte/chunk budget can then publish successfully.
            let limits = Config::new(3, 2, 2, 2).unwrap();
            let (receiver, service) = Receiver::new(limits.clone());
            let (a, b) = tokio::io::duplex(4096);
            let server = capntproto::rpc::serve(b, service.client);
            let (remote, driver) = capntproto::rpc::client(a);
            let mut sender = Sender::connect(remote).await.unwrap();
            assert_eq!(sender.config(), &limits);
            sender.write(b"ab").await.unwrap();
            sender.write(b"c").await.unwrap();
            assert_eq!(sender.done().await.unwrap().bytes, 3);
            assert_eq!(&*receiver.completed().unwrap(), b"abc");
            server.abort();
            driver.abort();
            let _ = server.await;
            let _ = driver.await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_bulk_config_import_and_receiver_traces() {
    use capntproto_test_support::verification::exploration;
    const MODEL: &str = "verification/BulkConfigBoundary.tla";
    const CONFIG: &str = include_str!("../verification/BulkConfigBoundary.cfg");
    let paths = exploration::traces(MODEL, "bulk-config-boundary", CONFIG).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for path in &paths {
                let mut transfer = None;
                for state in path {
                    let result = match state["event"] {
                        1 => {
                            let input = Raw(
                                state["length"],
                                state["chunk"] as u32,
                                state["window"] as u32,
                                state["count"] as u32,
                            );
                            let imported = match state["source"] {
                                1 => input.checked().ok(),
                                2 => serde_json::from_str::<Config>(&input.json()).ok(),
                                3 => {
                                    let (client, calls) = advertised(input);
                                    let result = Sender::connect(client)
                                        .await
                                        .ok()
                                        .map(|s| s.config().clone());
                                    assert_eq!(calls.get(), 0);
                                    result
                                }
                                _ => panic!("unknown import: {state:?}"),
                            };
                            let accepted = imported.is_some();
                            transfer = imported.map(|c| {
                                assert_eq!(raw(&c), (input.0, input.1, input.2, input.3));
                                Receiver::new(c)
                            });
                            accepted
                        }
                        2 => transfer
                            .as_ref()
                            .unwrap()
                            .0
                            .write(state["sequence"], &vec![b'x'; state["size"] as usize])
                            .is_ok(),
                        3 => transfer.as_ref().unwrap().0.done().is_ok(),
                        4 => {
                            transfer.as_ref().unwrap().0.cancel();
                            true
                        }
                        _ => panic!("unknown event: {state:?}"),
                    };
                    assert_eq!(u64::from(result), state["result"], "{path:?}");
                    assert_eq!(u64::from(transfer.is_some()), state["accepted"], "{path:?}");
                    if let Some((receiver, _client)) = &transfer {
                        let status = match receiver.status() {
                            Status::Receiving => 1,
                            Status::Complete => 2,
                            Status::Canceled => 3,
                            Status::Failed => 4,
                        };
                        for (name, actual) in [
                            ("status", status),
                            ("bytes", receiver.progress().bytes),
                            ("chunks", receiver.progress().chunks),
                            ("staged", receiver.staged_bytes() as u64),
                            ("published", u64::from(receiver.completed().is_some())),
                            ("failed", u64::from(receiver.failure().is_some())),
                        ] {
                            assert_eq!(actual, state[name], "{name}: {path:?}");
                        }
                        if let Some(published) = receiver.completed() {
                            assert_eq!(published.len() as u64, state["length"]);
                            assert!(published.iter().all(|b| *b == b'x'));
                        }
                    }
                }
            }
        })
        .await;
    exploration::controls(
        MODEL,
        "bulk-config-boundary",
        CONFIG,
        &[
            ("length", "ExpectedImport"),
            ("chunk", "ExpectedImport"),
            ("window", "ExpectedImport"),
            ("count", "ExpectedImport"),
            ("capacity", "ExpectedImport"),
            ("write", "ExpectedOperation"),
            ("publish", "ExpectedOperation"),
        ],
        None,
    )
    .unwrap();
    eprintln!(
        "{} bulk config and receiver edge-prefix replays",
        paths.len()
    );
}
