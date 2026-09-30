use super::*;
use reproto_test_support::verification::{command, cpp, exploration, root, run};
use std::{pin::Pin, task::Poll};

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
    let case = path[0]["event"] - 10;
    let mode = case / 9;
    let server_limit = case % 9 / 3;
    let client_limit = case % 3;
    let seen = Rc::new(Cell::new(0));
    let calls = Rc::new(Cell::new(0));
    let (server, driver) = TwoPartyServer::new(
        service(&seen, &calls),
        Options {
            max_fds: server_limit as usize,
            ..Default::default()
        },
    );
    let server_task = tokio::task::spawn_local(driver);
    let (a, b) = UnixStream::pair().unwrap();
    let raw = a.as_raw_fd();
    let (mut owned_io, mut borrowed_io) = if mode == 0 {
        (Some(a), None)
    } else {
        (None, Some(a))
    };
    let mut borrow = borrowed_io.as_mut();
    let mut peer_io = Some(b);
    let mut borrowed = None;
    let mut peer_task = None;
    let mut remote = None;
    let mut returned: Option<Received> = None;
    let mut draining = None;
    let mut drained = 0;
    let mut output = String::new();
    for state in path {
        match state["event"] {
            10..=27 => {
                if mode == 0 {
                    server.accept(owned_io.take().unwrap()).unwrap();
                } else {
                    borrowed = Some(server.accept_borrowed(borrow.take().unwrap()).unwrap());
                }
                let mut client = unix_rpc::client(
                    peer_io.take().unwrap(),
                    None,
                    Side::Client,
                    Options {
                        max_fds: client_limit as usize,
                        ..Default::default()
                    },
                );
                remote = Some(client.bootstrap::<harness::Client>());
                peer_task = Some(tokio::task::spawn_local(client));
            }
            1 => {
                returned =
                    Some(drive(&mut borrowed, exchange(remote.as_ref().unwrap(), &calls)).await);
            }
            2 => {
                draining = Some(server.drain());
                drained = 1;
            }
            3 | 6 => {
                if state["event"] == 6 {
                    drop(borrowed.take());
                }
                let task = peer_task.take().unwrap();
                task.abort();
                assert!(task.await.unwrap_err().is_cancelled());
            }
            4 => {
                for cap in &returned.as_ref().unwrap().0 {
                    assert_eq!(
                        cap.echo_request().send().promise.await.err().unwrap().kind,
                        ErrorKind::Disconnected
                    );
                }
            }
            e => panic!("unexpected event {e}"),
        }
        drive(&mut borrowed, settle()).await;
        if let Some(pending) = draining.as_mut() {
            if let Some(result) = pending.now_or_never() {
                result.unwrap();
                drained = 2;
                draining.take();
            }
        }
        let fd_open = u64::from(socket_alive(raw));
        let received = returned
            .as_ref()
            .map(|(_, fds)| value(&fds[0]) + 10 * value(&fds[1]))
            .unwrap_or(0);
        let observed = [seen.get(), received, calls.get(), drained, fd_open];
        assert_eq!(
            observed,
            [
                state["serverSeen"],
                state["retained"],
                state["calls"],
                state["drain"],
                state["fdOpen"]
            ],
            "{path:?}"
        );
        output += &format!(
            "{} {} {} {} {},",
            observed[0], observed[1], observed[2], observed[3], observed[4]
        );
    }
    drop(borrowed);
    for task in peer_task.into_iter().chain([server_task]) {
        task.abort();
        let _ = task.await;
    }
    output
}

#[test]
fn tlc_descriptor_facade_traces_match_pinned_cpp() {
    const MODEL: &str = "verification/UnixFacade.tla";
    const CONFIG: &str = include_str!("../../verification/UnixFacade.cfg");
    let paths = exploration::traces(MODEL, "unix-facade", CONFIG).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut input = String::new();
    let mut expected = String::new();
    runtime.block_on(tokio::task::LocalSet::new().run_until(async {
        tokio::time::timeout(Duration::from_secs(180), async {
            for path in &paths {
                for state in path {
                    input += &format!("{} ", state["event"]);
                }
                input.push('\n');
                expected += &replay(path).await;
                expected.push('\n');
            }
        })
        .await
        .expect("Unix descriptor trace replay timed out");
    }));
    cpp_replay(&input, &expected);
    eprintln!(
        "{} matching Rust/C++ observations across {} descriptor facade scenarios",
        paths.iter().map(Vec::len).sum::<usize>(),
        paths.len()
    );
    exploration::controls(
        MODEL,
        "unix-facade",
        CONFIG,
        &[
            ("globalLimit", "DescriptorLimits"),
            ("swapDescriptors", "DescriptorLimits"),
            ("sendWhileDisabled", "DescriptorLimits"),
            ("loseCapabilities", "CallableCapabilities"),
            ("lateCall", "CallableCapabilities"),
            ("closeBorrowed", "Ownership"),
            ("retainOwned", "Ownership"),
            ("earlyDrain", "DrainContract"),
            ("countBorrowed", "DrainContract"),
            ("revokeEscaped", "EscapedAuthority"),
        ],
        None,
    )
    .unwrap();
}

fn cpp_replay(input: &str, expected: &str) {
    let build = cpp::build(&["capnpc", "capnp_tool", "capnpc_cpp", "capnp-rpc"]).unwrap();
    let bin = build.join("c++/src/capnp");
    let logs = root().join("target/verification/unix-facade-cpp");
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
    let exe = temp.path().join("unix-facade");
    run(
        command("g++")
            .args(["-std=c++23", "-Ivendor/capnproto/c++/src"])
            .arg(format!("-I{}", temp.path().display()))
            .arg("tests/cpp/unix-facade.c++")
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
    let cases = logs.join("cases.txt");
    std::fs::write(&cases, input).unwrap();
    let actual = run(command(exe).arg(cases), &logs.join("reference.log"), 0).unwrap();
    assert_eq!(actual.lines().count(), expected.lines().count());
    for ((actual, expected), case) in actual.lines().zip(expected.lines()).zip(input.lines()) {
        assert_eq!(actual, expected, "{case}");
    }
}
