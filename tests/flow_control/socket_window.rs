use super::*;
use capnp_rpc::{rpc_twoparty_capnp::Side, twoparty, VatNetwork};
use reproto_test_support::verification::{command, cpp, exploration, root, run};

fn hint(value: u64) -> Option<usize> {
    match value {
        0 => None,
        1 => Some(0),
        2 => Some(16 * KIB),
        3 => Some(64 * KIB),
        _ => panic!("invalid hint"),
    }
}
fn harness(connection: &mut dyn capnp_rpc::Connection<Side>) -> Harness {
    let (controller, driver) = connection.new_stream();
    Harness::with_controller(
        controller,
        driver,
        Rc::new(Cell::new(0)),
        Rc::new(Cell::new(Duration::ZERO)),
    )
}

#[test]
fn pending_acks_and_window_queries_do_not_retain_output() {
    struct Output(Rc<Cell<usize>>);
    impl Drop for Output {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    impl futures::AsyncWrite for Output {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
            bytes: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            std::task::Poll::Ready(Ok(bytes.len()))
        }
        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }
        fn poll_close(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }
    }
    let dropped = Rc::new(Cell::new(0));
    let queries = Rc::new(Cell::new(0));
    let count = queries.clone();
    let mut network = twoparty::VatNetwork::new_with_send_buffer(
        futures::io::Cursor::new(Vec::<u8>::new()),
        Output(dropped.clone()),
        Side::Client,
        Default::default(),
        move |_| {
            count.set(count.get() + 1);
            Some(0)
        },
    );
    let diagnostics = network.outgoing_queue();
    let mut connection = network.connect(Side::Server).unwrap();
    let mut flow = harness(&mut *connection);
    flow.send(1);
    flow.send(1);
    assert_eq!(flow.outcomes, [1, 2]);
    assert_eq!(queries.get(), 1);
    drop(connection);
    drop(network);
    assert_eq!(dropped.get(), 1, "the controller cannot retain output");
    assert_eq!(diagnostics.snapshot().message_count, 0);
    flow.ack(0, false);
    assert_eq!(flow.outcomes, [1, 1]);
    flow.send(1); // More than the largest message needs the now-missing output.
    assert_eq!(flow.outcomes, [1, 1, 1], "uses the fallback window");
    assert_eq!(queries.get(), 1, "never query a destroyed transport");
    flow.drop_controller();
    assert!(!flow.done.get());
    flow.ack(1, false);
    flow.ack(2, false);
    assert!(flow.done.get());
}

#[test]
fn failed_stream_skips_queries_even_for_later_successful_acks() {
    let queries = Rc::new(Cell::new(0));
    let count = queries.clone();
    let window = twoparty::SendBufferWindow::new(move || {
        count.set(count.get() + 1);
        Some(0)
    });
    let (controller, driver) = window.new_stream();
    let mut flow = Harness::with_controller(
        controller,
        driver,
        Rc::new(Cell::new(0)),
        Rc::new(Cell::new(Duration::ZERO)),
    );
    for _ in 0..3 {
        flow.send(1);
    }
    assert_eq!(queries.get(), 2, "first message bypasses the window");
    flow.ack(0, true);
    flow.ack(1, false); // two charged messages still exceed the largest message
    flow.send(1);
    assert_eq!(flow.outcomes, [1, 3, 3, 3]);
    assert_eq!(queries.get(), 2, "failed streams never sample again");
    flow.ack(2, false);
    flow.ack(3, false);
    flow.drop_controller();
    assert!(flow.done.get());
}

// Observe real SO_SNDBUF through public credit promises. The fake outgoing
// message reports sizes without allocating/sending bulk data, isolating the
// automatic connection policy from kernel write readiness.
#[cfg(target_os = "linux")]
fn check_live_socket_window(
    connection: &mut dyn capnp_rpc::Connection<Side>,
    socket: &impl std::os::fd::AsFd,
) {
    let socket = socket2::SockRef::from(socket);
    socket.set_send_buffer_size(4096).unwrap();
    let small = socket.send_buffer_size().unwrap();
    assert_eq!(small % 8, 0);
    let mut flow = harness(connection);
    flow.send(1024 * KIB / 8);
    flow.send(small / 8);
    assert_eq!(
        flow.outcomes,
        [1, 2],
        "must sample the actual small socket buffer"
    );
    socket.set_send_buffer_size(64 * KIB).unwrap();
    assert!(socket.send_buffer_size().unwrap() > small + 8);
    flow.poll();
    assert_eq!(flow.outcomes, [1, 2], "resizing does not wake credit");
    flow.send(1);
    assert_eq!(flow.outcomes, [1, 2, 1]);
    flow.ack(2, false);
    assert_eq!(flow.outcomes, [1, 1, 1], "ack must refresh the window");
    socket.set_send_buffer_size(4096).unwrap();
    flow.send(small / 8);
    assert_eq!(flow.outcomes[3], 2, "shrink applies to an existing stream");
    flow.ack(1, false);
    assert_eq!(flow.outcomes[3], 2);
    flow.ack(3, false);
    assert_eq!(flow.outcomes[3], 1);
    flow.ack(0, false);
    flow.drop_controller();
    assert!(flow.done.get());
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "current_thread")]
async fn unix_network_automatically_samples_live_send_buffer_without_retaining_socket() {
    use std::os::fd::{AsFd, AsRawFd};
    let (socket, peer) = tokio::net::UnixStream::pair().unwrap();
    let probe = socket.as_fd().try_clone_to_owned().unwrap();
    let raw = socket.as_raw_fd();
    let mut network = reproto::unix_rpc::VatNetwork::new(socket, Side::Client, Default::default());
    let mut connection = network.connect(Side::Server).unwrap();
    check_live_socket_window(&mut *connection, &probe);
    let mut pending = harness(&mut *connection);
    pending.send(1);
    drop(connection);
    drop(network);
    // SAFETY: fcntl only checks descriptor validity; it neither adopts nor closes it.
    assert_eq!(unsafe { libc::fcntl(raw, libc::F_GETFD) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EBADF)
    );
    drop(probe);
    pending.send(1); // safely falls back after socket destruction
    assert_eq!(pending.outcomes, [1, 1]);
    pending.ack(0, false);
    pending.ack(1, false);
    pending.drop_controller();
    assert!(pending.done.get());
    drop(peer);
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "current_thread")]
async fn tcp_network_automatically_samples_live_send_buffer_and_releases_output() {
    use std::os::fd::{AsFd, AsRawFd};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (socket, peer) = tokio::join!(
        tokio::net::TcpStream::connect(listener.local_addr().unwrap()),
        listener.accept()
    );
    let socket = socket.unwrap();
    let probe = socket.as_fd().try_clone_to_owned().unwrap();
    let raw = socket.as_raw_fd();
    let mut network = reproto::rpc::tcp::network(socket, Side::Client, Default::default());
    let mut connection = network.connect(Side::Server).unwrap();
    check_live_socket_window(&mut *connection, &probe);
    let mut pending = harness(&mut *connection);
    pending.send(1);
    drop(connection);
    drop(network);
    // SAFETY: same descriptor validity check as the Unix case, without FD reuse.
    assert_eq!(unsafe { libc::fcntl(raw, libc::F_GETFD) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EBADF)
    );
    drop(probe);
    pending.send(1);
    assert_eq!(pending.outcomes, [1, 1]);
    pending.ack(0, false);
    pending.ack(1, false);
    pending.drop_controller();
    assert!(pending.done.get());
    drop(peer.unwrap());
}

#[test]
fn socket_window_model_matches_rust_network_and_pinned_cpp() {
    const MODEL: &str = "verification/RpcSocketWindow.tla";
    const CONFIG: &str = include_str!("../../verification/RpcSocketWindow.cfg");
    exploration::controls(
        MODEL,
        "socket-window",
        CONFIG,
        &[
            ("eagerQuery", "QueryContract"),
            ("retryUnavailable", "QueryContract"),
            ("perStreamCache", "QueryContract"),
            ("skipAckQuery", "QueryContract"),
            ("staleWindow", "SampledWindow"),
            ("zeroUnavailable", "SampledWindow"),
            ("wakeOnResize", "ResizePreservesCredit"),
        ],
        None,
    )
    .unwrap();
    let traces = exploration::traces(MODEL, "socket-window", CONFIG).unwrap();
    let mut script = String::new();
    let mut expected = String::new();
    let mut coverage = std::collections::BTreeSet::new();
    let mut observations = 0;
    for trace in &traces {
        script += "new 0\n";
        let size = Rc::new(Cell::new(hint(2)));
        let queries = Rc::new(Cell::new(0u64));
        let (source, count) = (size.clone(), queries.clone());
        let mut network = twoparty::VatNetwork::new_with_send_buffer(
            futures::io::Cursor::new(Vec::<u8>::new()),
            futures::io::sink(),
            Side::Client,
            Default::default(),
            move |_| {
                count.set(count.get() + 1);
                source.get()
            },
        );
        let mut connection = network.connect(Side::Server).unwrap();
        let mut streams: [Option<Harness>; 2] = [None, None];
        for state in trace {
            let event = state["event"];
            coverage.insert(event);
            script += &format!("step {event}\n");
            match event {
                1 | 2 => {
                    let stream = streams[event as usize - 1]
                        .get_or_insert_with(|| harness(&mut *connection));
                    stream.send(16 * KIB / 8);
                }
                4..=7 => streams[(event as usize - 4) / 2]
                    .as_mut()
                    .unwrap()
                    .ack(0, event % 2 == 1),
                10..=13 => size.set(hint(event - 10)),
                _ => panic!("{state:?}"),
            }
            for stream in streams.iter_mut().flatten() {
                stream.poll();
            }
            let outcome = |s: usize, n: usize| {
                streams[s]
                    .as_ref()
                    .and_then(|h| h.outcomes.get(n))
                    .copied()
                    .unwrap_or(0)
            };
            assert_eq!(queries.get(), state["queries"], "{trace:?}");
            assert_eq!(
                [
                    outcome(0, 0),
                    outcome(0, 1),
                    outcome(0, 2),
                    outcome(1, 0),
                    outcome(1, 1)
                ],
                [
                    state["p11"],
                    state["p12"],
                    state["p13"],
                    state["p21"],
                    state["p22"]
                ],
                "{trace:?}"
            );
            expected += &format!(
                "{}:{}:{}:{}:{}:{}\n",
                queries.get(),
                outcome(0, 0),
                outcome(0, 1),
                outcome(0, 2),
                outcome(1, 0),
                outcome(1, 1)
            );
            observations += 1;
        }
    }
    assert_eq!(
        coverage,
        [1, 2, 4, 5, 6, 7, 10, 11, 12, 13].into_iter().collect()
    );
    let build = cpp::build(&["capnp-rpc"]).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("socket-window");
    let logs = root().join("target/verification/socket-window-cpp");
    std::fs::create_dir_all(&logs).unwrap();
    let cases = logs.join("cases.txt");
    std::fs::write(&cases, script).unwrap();
    let mut compile = command("g++");
    compile
        .args([
            "-std=c++23",
            "-Ivendor/capnproto/c++/src",
            "tests/cpp/socket-window.c++",
        ])
        .arg(build.join("c++/src/capnp/libcapnp-rpc.a"))
        .arg(build.join("c++/src/capnp/libcapnp.a"))
        .arg(build.join("c++/src/kj/libkj-async.a"))
        .arg(build.join("c++/src/kj/libkj.a"))
        .args(["-pthread", "-o"])
        .arg(&executable);
    run(&mut compile, &logs.join("compile.log"), 0).unwrap();
    let actual = run(
        command(executable).arg(cases),
        &logs.join("reference.log"),
        0,
    )
    .unwrap();
    assert_eq!(actual.lines().count(), observations);
    for (index, (actual, expected)) in actual.lines().zip(expected.lines()).enumerate() {
        assert_eq!(actual, expected, "observation {index}");
    }
    eprintln!("{observations} matching Rust/C++ socket-window observations");
}
