use super::*;
use reproto_test_support::verification::{command, cpp, exploration, root, run};

struct CountingEcho(Rc<Cell<u64>>);
impl harness::Server for CountingEcho {
    async fn echo(
        self: Rc<Self>,
        _: harness::EchoParams,
        mut results: harness::EchoResults,
    ) -> capnp::Result<()> {
        self.0.set(self.0.get() + 1);
        results.get().set_value(73);
        Ok(())
    }
}

async fn drive<T>(borrowed: &mut Option<TwoPartyClient<'_>>, task: impl Future<Output = T>) -> T {
    futures::pin_mut!(task);
    futures::future::poll_fn(|cx| {
        if let Some(client) = borrowed.as_mut() {
            if let Poll::Ready(result) = Pin::new(client).poll(cx) {
                result.unwrap();
                borrowed.take();
            }
        }
        task.as_mut().poll(cx)
    })
    .await
}

async fn replay(path: &[exploration::State]) -> String {
    let owned_drops = Rc::new(Cell::new(0));
    let borrowed_drops = Rc::new(Cell::new(0));
    let calls = Rc::new(Cell::new(0));
    let cap: harness::Client = capnp_rpc::new_client(CountingEcho(calls.clone()));
    let (server, driver) = TwoPartyServer::new(cap.client);
    let mut server_task = Some(tokio::task::spawn_local(driver));
    let (a, b) = tokio::io::duplex(4096);
    let mut borrowed_io = Tracked {
        io: a.compat(),
        drops: borrowed_drops.clone(),
    };
    let mut borrow = Some(&mut borrowed_io);
    let mut borrowed_peer = Some(b);
    let mut borrowed = None;
    let mut peers = [None, None];
    let mut caps: [Option<harness::Client>; 2] = [None, None];
    let mut drain: Option<Promise<(), Error>> = None;
    let mut drained = 0;
    let mut output = String::new();
    for state in path {
        match state["event"] {
            1 => {
                let (a, b) = tokio::io::duplex(4096);
                server
                    .accept(Tracked {
                        io: a.compat(),
                        drops: owned_drops.clone(),
                    })
                    .unwrap();
                let mut peer = TwoPartyClient::new(b.compat());
                caps[0] = Some(peer.bootstrap());
                peers[0] = Some(tokio::task::spawn_local(peer));
            }
            2 => {
                borrowed = Some(server.accept_borrowed(borrow.take().unwrap()));
                let mut peer = TwoPartyClient::new(borrowed_peer.take().unwrap().compat());
                caps[1] = Some(peer.bootstrap());
                peers[1] = Some(tokio::task::spawn_local(peer));
            }
            3 => {
                drain = Some(server.drain());
                drained = 1;
            }
            4 | 9 => {
                let peer = peers[usize::from(state["event"] == 9)].take().unwrap();
                peer.abort();
                assert!(peer.await.unwrap_err().is_cancelled());
            }
            5 => {
                drop(borrowed.take());
            }
            6 | 7 => {
                let cap = caps[(state["event"] - 6) as usize].as_ref().unwrap();
                assert_eq!(drive(&mut borrowed, echo(cap, 73)).await.unwrap(), 73);
            }
            8 => {
                let task = server_task.take().unwrap();
                task.abort();
                assert!(task.await.unwrap_err().is_cancelled());
            }
            event => panic!("unknown event {event}"),
        }
        drive(&mut borrowed, settle()).await;
        if let Some(pending) = drain.as_mut() {
            if let Some(result) = pending.now_or_never() {
                drained = if result.is_ok() { 2 } else { 3 };
                drain.take();
            }
        }
        let seen = [
            owned_drops.get(),
            borrowed_drops.get(),
            drained,
            calls.get(),
            u64::from(borrowed.is_some()),
        ];
        assert_eq!(
            seen,
            [
                state["ownedDrops"],
                state["borrowedDrops"],
                state["drain"],
                state["calls"],
                u64::from(state["borrowed"] == 1)
            ],
            "{path:?}"
        );
        output += &format!(
            "{} {} {} {} {},",
            seen[0], seen[1], seen[2], seen[3], seen[4]
        );
    }
    drop(borrowed);
    for task in peers.into_iter().flatten().chain(server_task) {
        task.abort();
        let _ = task.await;
    }
    output
}

#[test]
fn tlc_lifecycle_traces_match_rust_and_pinned_cpp() {
    const MODEL: &str = "verification/TwoPartyFacade.tla";
    const CONFIG: &str = include_str!("../../verification/TwoPartyFacade.cfg");
    let paths = exploration::traces(MODEL, "twoparty-facade", CONFIG).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut inputs = String::new();
    let mut expected = String::new();
    runtime.block_on(tokio::task::LocalSet::new().run_until(async {
        tokio::time::timeout(Duration::from_secs(120), async {
            for path in &paths {
                for state in path {
                    inputs += &format!("{} ", state["event"]);
                }
                inputs.push('\n');
                expected += &replay(path).await;
                expected.push('\n');
            }
        })
        .await
        .expect("lifecycle trace replay timed out");
    }));
    cpp_replay(&inputs, &expected);
    eprintln!(
        "{} matching Rust/C++ observations across {} edge-prefix scenarios",
        paths.iter().map(Vec::len).sum::<usize>(),
        paths.len()
    );
    exploration::controls(
        MODEL,
        "twoparty-facade",
        CONFIG,
        &[
            ("leakOwned", "Ownership"),
            ("destroyBorrowed", "Ownership"),
            ("earlyDrain", "DrainContract"),
            ("countBorrowed", "DrainContract"),
            ("loseCapability", "Capabilities"),
            ("cancelBorrowed", "BorrowedSurvivesOwner"),
        ],
        None,
    )
    .unwrap();
}

fn cpp_replay(inputs: &str, expected: &str) {
    let build = cpp::build(&["capnpc", "capnp_tool", "capnpc_cpp", "capnp-rpc"]).unwrap();
    let bin = build.join("c++/src/capnp");
    let logs = root().join("target/verification/twoparty-facade-cpp");
    let temp = tempfile::tempdir().unwrap();
    run(
        command(bin.join("capnp")).args([
            "compile",
            &format!(
                "-o{}:{}",
                bin.join("capnpc-c++").display(),
                temp.path().display()
            ),
            "-Ivendor/capnproto/c++/src",
            "--src-prefix=schemas",
            "schemas/runtime-test.capnp",
        ]),
        &logs.join("schema.log"),
        0,
    )
    .unwrap();
    let exe = temp.path().join("twoparty-facade");
    run(
        command("g++")
            .args(["-std=c++23", "-Ivendor/capnproto/c++/src"])
            .arg(format!("-I{}", temp.path().display()))
            .arg("tests/cpp/twoparty-facade.c++")
            .arg(temp.path().join("runtime-test.capnp.c++"))
            .arg(bin.join("libcapnp-rpc.a"))
            .arg(bin.join("libcapnp.a"))
            .arg(build.join("c++/src/kj/libkj-async.a"))
            .arg(build.join("c++/src/kj/libkj.a"))
            .args(["-pthread", "-o"])
            .arg(&exe),
        &logs.join("compile.log"),
        0,
    )
    .unwrap();
    let input = logs.join("cases.txt");
    std::fs::write(&input, inputs).unwrap();
    let actual = run(command(exe).arg(input), &logs.join("reference.log"), 0).unwrap();
    assert_eq!(actual.lines().count(), expected.lines().count());
    for ((actual, expected), case) in actual.lines().zip(expected.lines()).zip(inputs.lines()) {
        assert_eq!(actual, expected, "{case}");
    }
}
