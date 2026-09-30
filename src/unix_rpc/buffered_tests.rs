use super::tests::{body, descriptor};
use super::*;
use std::io::Read;

#[path = "scratch_tests.rs"]
mod scratch;

fn control(id: u32) -> Vec<u8> {
    let mut msg = Builder::new_default();
    msg.init_root::<capnp_rpc::rpc_capnp::message::Builder>()
        .init_finish()
        .set_question_id(id);
    capnp::serialize::write_message_to_words(&msg)
}
fn id(message: &Input) -> u32 {
    let capnp_rpc::rpc_capnp::message::Finish(finish) = message
        .body
        .get_root::<capnp_rpc::rpc_capnp::message::Reader>()
        .unwrap()
        .which()
        .unwrap()
    else {
        panic!("not Finish");
    };
    finish.unwrap().get_question_id()
}
fn fd_value(fd: &OwnedFd) -> u8 {
    let mut byte = 0u8;
    // SAFETY: a live owned descriptor and one writable byte; pread preserves offsets.
    assert_eq!(
        unsafe { libc::pread(fd.as_raw_fd(), (&mut byte as *mut u8).cast(), 1, 0) },
        1
    );
    assert_ne!(
        unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
        0
    );
    byte
}
fn witnessed_fd() -> (Rc<OwnedFd>, std::os::unix::net::UnixStream) {
    let (passed, witness) = std::os::unix::net::UnixStream::pair().unwrap();
    witness.set_nonblocking(true).unwrap();
    (Rc::new(passed.into()), witness)
}
fn open(witness: &mut std::os::unix::net::UnixStream) {
    assert_eq!(
        witness.read(&mut [0]).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
}
fn closed(witness: &mut std::os::unix::net::UnixStream) {
    assert_eq!(witness.read(&mut [0]).unwrap(), 0);
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "current_thread")]
async fn coalesced_bare_and_fd_frames_keep_descriptors_for_the_last_frame() {
    let (sender, receiver) = UnixStream::pair().unwrap();
    write_message(&sender, &control(1), &[]).await.unwrap();
    write_message(&sender, &control(2), &[descriptor(22)])
        .await
        .unwrap();
    let mut reader = FdReader::new(&receiver, Options::default());
    let first = reader.read().await.unwrap().unwrap();
    assert_eq!(id(&first), 1);
    assert!(first.fds.is_empty());
    assert!(first.body.get_segments().is_shared_buffer());
    assert_eq!(reader.reader.get_ref().ancillary.borrow().reads, 1);
    assert_eq!(reader.reader.get_ref().ancillary.borrow().fds.len(), 1);
    // Rejected reads must not discard descriptors prefetched for a later frame.
    assert!(reader.read().await.is_err());
    assert_eq!(reader.reader.get_ref().ancillary.borrow().fds.len(), 1);
    drop(first);
    let mut second = reader.read().await.unwrap().unwrap();
    assert_eq!(id(&second), 2);
    let fds = second.take_fds();
    assert_eq!(fds.len(), 1);
    assert_eq!(fd_value(&fds[0]), 22);
    assert!(second.take_fds().is_empty());
    assert_eq!(reader.reader.get_ref().ancillary.borrow().reads, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn partial_ancillary_headers_and_bodies_do_not_spend_the_next_frames_limit() {
    let bytes = capnp::serialize::write_message_to_words(&*body(64));
    for prefix in [1, 5, 8, 15, bytes.len() - 1] {
        let (sender, receiver) = UnixStream::pair().unwrap();
        write_message(&sender, &bytes[..prefix], &[descriptor(11)])
            .await
            .unwrap();
        write_message(&sender, &bytes[prefix..], &[descriptor(12)])
            .await
            .unwrap();
        write_message(&sender, &control(2), &[descriptor(22)])
            .await
            .unwrap();
        let mut reader = FdReader::new(
            &receiver,
            Options {
                max_fds: 1,
                ..Default::default()
            },
        );
        let first = reader.read().await.unwrap().unwrap();
        assert_eq!(
            first.body.get_root::<capnp::data::Reader>().unwrap(),
            &[19; 64]
        );
        assert!(!first.body.get_segments().is_shared_buffer());
        assert_eq!(first.fds.len(), 1);
        assert_eq!(fd_value(&first.fds[0]), 11);
        let second = reader.read().await.unwrap().unwrap();
        assert_eq!(id(&second), 2);
        assert_eq!(
            second.fds.len(),
            1,
            "descriptor budget was not reset after prefix {prefix}"
        );
        assert_eq!(fd_value(&second.fds[0]), 22);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn truncation_closes_excess_and_retained_descriptors_have_one_owner() {
    for limit in [0, 1, 2] {
        let (sender, receiver) = UnixStream::pair().unwrap();
        let (a, mut aw) = witnessed_fd();
        let (b, mut bw) = witnessed_fd();
        let (c, mut cw) = witnessed_fd();
        write_message(&sender, &control(1), &[a, b, c])
            .await
            .unwrap();
        let mut reader = FdReader::new(
            &receiver,
            Options {
                max_fds: limit,
                ..Default::default()
            },
        );
        let mut message = reader.read().await.unwrap().unwrap();
        assert_eq!(message.fds.len(), limit);
        for (i, witness) in [&mut aw, &mut bw, &mut cw].into_iter().enumerate() {
            if i < limit {
                open(witness);
            } else {
                closed(witness);
            }
        }
        let owned = message.take_fds();
        drop(message);
        drop(reader);
        for witness in [&mut aw, &mut bw, &mut cw].into_iter().take(limit) {
            open(witness);
        }
        drop(owned);
        for witness in [&mut aw, &mut bw, &mut cw] {
            closed(witness);
        }
    }
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "current_thread")]
async fn close_releases_prefetched_descriptors_with_connection_handles_alive() {
    use capnp_rpc::VatNetwork as _;
    let (sender, receiver) = UnixStream::pair().unwrap();
    let (fd, mut witness) = witnessed_fd();
    write_message(&sender, &control(1), &[]).await.unwrap();
    write_message(&sender, &control(2), &[fd]).await.unwrap();
    let mut network = VatNetwork::new(receiver, Side::Server, Options::default());
    let mut connection = network.connect(Side::Client).unwrap();
    let first = connection
        .receive_incoming_message()
        .await
        .unwrap()
        .unwrap();
    open(&mut witness);
    // Holding the prior control message is a recoverable API misuse.
    assert!(connection.receive_incoming_message().await.is_err());
    assert!(!network.inner.closed.get());
    open(&mut witness);
    network.inner.close();
    closed(&mut witness);
    assert!(first.get_body().is_ok());
    assert!(connection.receive_incoming_message().await.is_err());
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "current_thread")]
async fn cancel_unpolled_read_closes_prefetched_descriptors() {
    use capnp_rpc::VatNetwork as _;
    let (sender, receiver) = UnixStream::pair().unwrap();
    let (fd, mut witness) = witnessed_fd();
    write_message(&sender, &control(1), &[]).await.unwrap();
    write_message(&sender, &control(2), &[fd]).await.unwrap();
    let mut network = VatNetwork::new(receiver, Side::Server, Options::default());
    let mut connection = network.connect(Side::Client).unwrap();
    drop(
        connection
            .receive_incoming_message()
            .await
            .unwrap()
            .unwrap(),
    );
    open(&mut witness);
    drop(connection.receive_incoming_message());
    assert!(network.inner.closed.get());
    closed(&mut witness);
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_prefetched_frame_and_partial_eof_close_its_descriptors() {
    use capnp_rpc::VatNetwork as _;
    for eof in [false, true] {
        let (sender, receiver) = UnixStream::pair().unwrap();
        let (fd, mut witness) = witnessed_fd();
        write_message(&sender, &control(1), &[]).await.unwrap();
        let bad = if eof {
            control(2)[..7].to_vec()
        } else {
            vec![255; 8]
        };
        write_message(&sender, &bad, &[fd]).await.unwrap();
        drop(sender);
        let mut network = VatNetwork::new(receiver, Side::Server, Options::default());
        let mut connection = network.connect(Side::Client).unwrap();
        drop(
            connection
                .receive_incoming_message()
                .await
                .unwrap()
                .unwrap(),
        );
        open(&mut witness);
        assert!(connection.receive_incoming_message().await.is_err());
        closed(&mut witness);
        assert!(network.inner.closed.get());
    }
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "current_thread")]
async fn many_bare_control_frames_share_one_recvmsg_and_reuse_storage() {
    let (sender, receiver) = UnixStream::pair().unwrap();
    let bytes: Vec<_> = (0..32).flat_map(control).collect();
    write_message(&sender, &bytes, &[]).await.unwrap();
    let mut reader = FdReader::new(&receiver, Options::default());
    for value in 0..32 {
        let message = reader.read().await.unwrap().unwrap();
        assert_eq!(id(&message), value);
        assert!(message.body.get_segments().is_shared_buffer());
    }
    assert_eq!(reader.reader.get_ref().ancillary.borrow().reads, 1);
}

// Observe descriptor identity without reading from it or relying on its reused
// process-local number. Each fixture descriptor is a different Unix socket.
#[cfg(target_os = "linux")]
fn inode(fd: &OwnedFd) -> u64 {
    use std::os::unix::fs::MetadataExt;
    std::fs::File::from(fd.try_clone().unwrap())
        .metadata()
        .unwrap()
        .ino()
}

#[cfg(target_os = "linux")]
async fn replay(path: &[reproto_test_support::verification::exploration::State], prefix: usize) {
    use capnp_rpc::VatNetwork as _;
    let (sender, receiver) = UnixStream::pair().unwrap();
    let mut sender = Some(sender);
    let mut network = VatNetwork::new(
        receiver,
        Side::Server,
        Options {
            max_fds: 1,
            ..Default::default()
        },
    );
    let mut connection = network.connect(Side::Client).unwrap();
    let (a, mut aw) = witnessed_fd();
    let (b, mut bw) = witnessed_fd();
    let (x, mut xw) = witnessed_fd();
    let identities = [inode(&a), inode(&b)];
    let mut a = Some(a);
    let mut b = Some(b);
    let mut x = Some(x);
    let second = control(2);
    let end = if prefix == 0 { second.len() } else { prefix };
    let mut read = None;
    let mut held: [Option<OwnedFd>; 2] = [None, None];
    let mut received = [0; 3];
    let mut delivered = 0;
    for state in path {
        match state["event"] {
            1 => write_message(sender.as_ref().unwrap(), &control(1), &[])
                .await
                .unwrap(),
            2 => write_message(
                sender.as_ref().unwrap(),
                &second[..end],
                &[a.take().unwrap(), x.take().unwrap()],
            )
            .await
            .unwrap(),
            3 => write_message(sender.as_ref().unwrap(), &second[end..], &[])
                .await
                .unwrap(),
            4 => write_message(sender.as_ref().unwrap(), &control(3), &[b.take().unwrap()])
                .await
                .unwrap(),
            5 => read = Some(Box::pin(connection.receive_incoming_message())),
            6 => {
                let now = [state["r1"], state["r2"], state["r3"]];
                // Synchronize Tokio's readiness cache only when the model says
                // there are new bytes (or EOF). Pending empty reads must not wait.
                if now != received || state["ended"] == 1 {
                    network.inner.socket.readable().await.unwrap();
                }
                received = now;
                match state["result"] {
                    1 => {
                        let mut message = read.take().unwrap().await.unwrap().unwrap();
                        delivered += 1;
                        let capnp_rpc::rpc_capnp::message::Finish(finish) = message
                            .get_body()
                            .unwrap()
                            .get_as::<capnp_rpc::rpc_capnp::message::Reader>()
                            .unwrap()
                            .which()
                            .unwrap()
                        else {
                            panic!("unexpected frame: {state:?}");
                        };
                        assert_eq!(finish.unwrap().get_question_id(), delivered);
                        let mut fds = message.take_fds();
                        assert!(message.take_fds().is_empty());
                        if delivered == 1 {
                            assert!(
                                fds.is_empty(),
                                "descriptor attached to preceding bare frame: {state:?}"
                            );
                        } else {
                            assert_eq!(fds.len(), 1, "{state:?}");
                            let fd = fds.pop().unwrap();
                            let index = delivered as usize - 2;
                            assert_eq!(inode(&fd), identities[index], "{state:?}");
                            held[index] = Some(fd);
                        }
                    }
                    2 => assert!(
                        futures::poll!(read.as_mut().unwrap()).is_pending(),
                        "{state:?}"
                    ),
                    3 => assert!(read.take().unwrap().await.unwrap().is_none(), "{state:?}"),
                    4 => assert!(read.take().unwrap().await.is_err(), "{state:?}"),
                    _ => panic!("{state:?}"),
                }
            }
            7 => drop(read.take().unwrap()),
            8 => drop(held[0].take()),
            9 => drop(held[1].take()),
            10 => drop(sender.take()),
            _ => panic!("{state:?}"),
        }
        assert_eq!(
            network.inner.closed.get(),
            state["closed"] == 1,
            "{state:?}"
        );
        assert_eq!(read.is_some(), state["reading"] == 1, "{state:?}");
        assert_eq!(u64::from(delivered), state["delivered"], "{state:?}");
        for (key, fd) in ["a", "b"].into_iter().zip(&held) {
            assert_eq!(fd.is_some(), state[key] == 3, "{state:?}");
        }
        for (key, witness) in [("a", &mut aw), ("b", &mut bw), ("x", &mut xw)] {
            if state[key] == 4 {
                closed(witness);
            } else if state["closed"] == 0 || state[key] != 1 {
                // Shutdown may release unread kernel-queued descriptors. The
                // model specifies lifetime only after recvmsg transfers them.
                open(witness);
            }
        }
    }
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "current_thread")]
async fn tlc_buffered_fds_replays_boundaries_budgets_cancellation_and_eof() {
    use reproto_test_support::verification::exploration::{controls, traces};
    const MODEL: &str = "verification/RpcBufferedFds.tla";
    const CONFIG: &str = include_str!("../../verification/RpcBufferedFds.cfg");
    let mut outcomes = [false; 5];
    for partial in [0, 1] {
        let config = CONFIG.replace("Partial = 0", &format!("Partial = {partial}"));
        let report = format!("buffered-fds-{partial}");
        let paths = traces(MODEL, &report, &config).unwrap();
        let prefixes = if partial == 0 {
            vec![0]
        } else {
            vec![1, 8, control(2).len() - 1]
        };
        for path in &paths {
            for state in path {
                if state["event"] == 6 {
                    outcomes[state["result"] as usize] = true;
                }
            }
            for &prefix in &prefixes {
                tokio::time::timeout(std::time::Duration::from_secs(2), replay(path, prefix))
                    .await
                    .unwrap_or_else(|_| panic!("stalled replay, prefix {prefix}: {path:?}"));
            }
        }
        let live = config.replace("SPECIFICATION Spec", "SPECIFICATION LiveSpec")
            + "\nPROPERTY EndProgress\n";
        let faults = if partial == 1 {
            &[
                ("earlyAttach", "CorrectOwner"),
                ("reuseBudget", "SeparateBudgets"),
                ("truncateLeak", "BoundedDescriptors"),
                ("leakCancel", "NoRetainedAfterClose"),
            ][..]
        } else {
            &[]
        };
        controls(MODEL, &report, &config, faults, Some(&live)).unwrap();
    }
    assert!(outcomes[1..].iter().all(|seen| *seen));
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "current_thread")]
async fn buffered_ancillary_boundaries_and_limits_match_pinned_cpp() {
    use reproto_test_support::verification::{command, cpp, root, run};
    let build = cpp::build(&["capnp-rpc"]).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("buffered-fds");
    let logs = root().join("target/verification/buffered-fds-cpp");
    let mut compile = command("g++");
    compile
        .args([
            "-std=c++23",
            "-Ivendor/capnproto/c++/src",
            "tests/cpp/buffered-fds.c++",
        ])
        .arg(build.join("c++/src/capnp/libcapnp-rpc.a"))
        .arg(build.join("c++/src/capnp/libcapnp.a"))
        .arg(build.join("c++/src/kj/libkj-async.a"))
        .arg(build.join("c++/src/kj/libkj.a"))
        .args(["-pthread", "-o"])
        .arg(&executable);
    run(&mut compile, &logs.join("compile.log"), 0).unwrap();
    let frames = [control(1), control(2), control(3)];
    assert!(frames.iter().all(|f| f.len() == frames[0].len()));
    let file = directory.path().join("frames.bin");
    std::fs::write(&file, frames.concat()).unwrap();
    let reference = run(
        command(&executable).arg(file),
        &logs.join("reference.log"),
        0,
    )
    .unwrap();
    let mut observed = String::new();
    for limit in [0, 1, 2] {
        for prefix in [0, 1, 8, frames[1].len() - 1] {
            for split_fds in [false, true] {
                if prefix == 0 && split_fds {
                    continue;
                }
                let (sender, receiver) = UnixStream::pair().unwrap();
                write_message(&sender, &frames[0], &[]).await.unwrap();
                let count = if prefix == 0 { frames[1].len() } else { prefix };
                let fds = if split_fds {
                    vec![descriptor(11)]
                } else {
                    vec![descriptor(11), descriptor(12)]
                };
                write_message(&sender, &frames[1][..count], &fds)
                    .await
                    .unwrap();
                if prefix != 0 {
                    let fds = if split_fds {
                        vec![descriptor(12)]
                    } else {
                        vec![]
                    };
                    write_message(&sender, &frames[1][count..], &fds)
                        .await
                        .unwrap();
                }
                write_message(&sender, &frames[2], &[descriptor(22)])
                    .await
                    .unwrap();
                drop(sender);
                let mut reader = FdReader::new(
                    &receiver,
                    Options {
                        max_fds: limit,
                        ..Default::default()
                    },
                );
                for expected_id in 1..=3 {
                    let message = reader.read().await.unwrap().unwrap();
                    assert_eq!(id(&message), expected_id);
                    observed += &format!(
                        "{limit} {prefix} {} {expected_id} {} {}",
                        u8::from(split_fds),
                        u8::from(message.body.get_segments().is_shared_buffer()),
                        message.fds.len()
                    );
                    for fd in &message.fds {
                        observed += &format!(" {}", fd_value(fd));
                    }
                    observed.push('\n');
                }
                assert!(reader.read().await.unwrap().is_none());
            }
        }
    }
    assert_eq!(observed, reference);
    eprintln!(
        "{} ancillary-buffer observations match pinned C++",
        observed.lines().count()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn frame_bounded_reads_keep_fds_on_their_frame_with_trailing_bare_messages() {
    // Force Darwin's conservative read policy on Linux too. Queue the entire
    // sequence first, including a descriptor at a split frame boundary.
    for limit in [0, 1, 2] {
        let (sender, receiver) = UnixStream::pair().unwrap();
        write_message(&sender, &control(1), &[]).await.unwrap();
        let frame = control(2);
        write_message(&sender, &frame[..5], &[descriptor(22), descriptor(23)])
            .await
            .unwrap();
        write_message(&sender, &frame[5..], &[]).await.unwrap();
        write_message(&sender, &control(3), &[]).await.unwrap();
        write_message(&sender, &control(4), &[descriptor(44)])
            .await
            .unwrap();
        write_message(&sender, &control(5), &[]).await.unwrap();
        let mut reader = FdReader::new(
            &receiver,
            Options {
                max_fds: limit,
                ..Default::default()
            },
        );
        reader.reader.set_read_ahead_policy(|_| false);
        for number in 1..=5 {
            let message = reader.read().await.unwrap().unwrap();
            assert_eq!(id(&message), number);
            let expected: &[u8] = match number {
                2 => &[22, 23],
                4 => &[44],
                _ => &[],
            };
            assert_eq!(
                message.fds.iter().map(fd_value).collect::<Vec<_>>(),
                expected.iter().copied().take(limit).collect::<Vec<_>>()
            );
        }
    }
}
