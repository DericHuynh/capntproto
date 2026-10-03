use capnp::message::{Builder, Reader, ReaderSegments};
use capnp_futures::{BufferedRead, BufferedSegments};
use futures::{AsyncRead, FutureExt};
use std::{
    cell::RefCell,
    io,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
};

#[path = "buffered_input/scratch.rs"]
mod scratch;

type Message = Reader<BufferedSegments>;
#[derive(Default)]
struct Input {
    bytes: Vec<u8>,
    position: usize,
    available: usize,
    chunk: usize,
    closed: bool,
    failed: bool,
    calls: usize,
    buffers: Vec<(usize, usize)>,
}
#[derive(Clone)]
struct Source(Rc<RefCell<Input>>);
impl AsyncRead for Source {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        out: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let mut input = self.0.borrow_mut();
        input.calls += 1;
        if input.failed {
            return Poll::Ready(Err(io::ErrorKind::ConnectionReset.into()));
        }
        let count = (input.available - input.position)
            .min(input.chunk)
            .min(out.len());
        if count == 0 && !input.closed {
            return Poll::Pending;
        }
        out[..count].copy_from_slice(&input.bytes[input.position..input.position + count]);
        input.position += count;
        input.buffers.push((out.as_ptr() as usize, out.len()));
        Poll::Ready(Ok(count))
    }
}
fn source(bytes: Vec<u8>) -> Source {
    Source(Rc::new(RefCell::new(Input {
        available: bytes.len(),
        bytes,
        chunk: usize::MAX,
        closed: true,
        ..Default::default()
    })))
}
fn data(size: usize, value: u8) -> Vec<u8> {
    let mut message = Builder::new_default();
    message
        .initn_root::<capnp::data::Builder>(size as u32)
        .fill(value);
    capnp::serialize::write_message_to_words(&message)
}
fn read(reader: &mut BufferedRead<Source>, short: bool) -> capnp::Result<Option<Message>> {
    reader
        .try_read_message(|_| Ok(short))
        .now_or_never()
        .expect("unexpectedly blocked")
}
fn assert_data(message: &Message, size: usize, value: u8) {
    assert_eq!(
        message.get_root::<capnp::data::Reader>().unwrap(),
        vec![value; size]
    );
}

#[test]
fn short_lived_reads_share_storage_and_reject_overlap_without_consuming() {
    let input = source([data(16, 1), data(16, 2)].concat());
    let mut reader = BufferedRead::new(input.clone(), Default::default());
    let first = read(&mut reader, true).unwrap().unwrap();
    assert!(first.get_segments().is_shared_buffer());
    let pointer = first.get_segments().get_segment(0).unwrap().as_ptr() as usize;
    let (buffer, size) = input.0.borrow().buffers[0];
    assert!(
        (buffer..buffer + size).contains(&pointer),
        "message borrows the actual I/O buffer"
    );
    let calls = input.0.borrow().calls;
    assert!(read(&mut reader, true).is_err());
    assert_eq!(input.0.borrow().calls, calls);
    assert_data(&first, 16, 1);
    drop(first);
    let second = read(&mut reader, true).unwrap().unwrap();
    assert_eq!(input.0.borrow().calls, 1, "second frame was prefetched");
    drop(reader);
    assert_data(&second, 16, 2); // Reader storage outlives the stream safely.
}

#[test]
fn retained_messages_survive_compaction_and_repeated_buffer_reuse() {
    let input = source((0..40).flat_map(|i| data(300, i)).collect());
    let mut reader =
        BufferedRead::with_buffer_size(input.clone(), Default::default(), 256).unwrap();
    let mut retained = Vec::new();
    for i in 0..40 {
        let msg = read(&mut reader, i % 2 == 0).unwrap().unwrap();
        assert_data(&msg, 300, i);
        assert_eq!(msg.get_segments().is_shared_buffer(), i % 2 == 0);
        if i % 2 == 1 {
            retained.push((i, msg));
        }
    }
    assert!(read(&mut reader, true).unwrap().is_none());
    let calls = input.0.borrow().calls;
    assert!(read(&mut reader, true).unwrap().is_none());
    assert_eq!(input.0.borrow().calls, calls, "EOF is sticky");
    assert!(calls < 15, "prefetch amortizes reads: {calls}");
    drop(reader);
    for (i, msg) in retained {
        assert_data(&msg, 300, i);
    }
}

#[test]
fn canceled_reads_preserve_every_header_and_body_prefix_including_direct_io() {
    for size in [0, 17, 3000] {
        let first = data(size, 7);
        let input = source([first.clone(), data(8, 9)].concat());
        input.0.borrow_mut().available = 0;
        input.0.borrow_mut().closed = false;
        let mut reader =
            BufferedRead::with_buffer_size(input.clone(), Default::default(), 256).unwrap();
        for available in 0..first.len() {
            input.0.borrow_mut().available = available;
            assert!(
                reader
                    .try_read_message(|_| Ok(true))
                    .now_or_never()
                    .is_none(),
                "prefix {available}"
            );
        }
        input.0.borrow_mut().available = first.len();
        let msg = read(&mut reader, true).unwrap().unwrap();
        assert_data(&msg, size, 7);
        assert_eq!(msg.get_segments().is_shared_buffer(), size < 3000);
        drop(msg);
        let size = input.0.borrow().bytes.len();
        input.0.borrow_mut().available = size;
        let next = read(&mut reader, true).unwrap().unwrap();
        assert_data(&next, 8, 9);
    }
}

#[test]
fn large_frames_bypass_shared_buffer_and_do_not_consume_following_frame() {
    let input = source([data(5000, 3), data(8, 4)].concat());
    let mut reader =
        BufferedRead::with_buffer_size(input.clone(), Default::default(), 256).unwrap();
    let big = reader
        .try_read_message(|_| panic!("large direct frames need no classifier"))
        .now_or_never()
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(!big.get_segments().is_shared_buffer());
    assert_eq!(input.0.borrow().position, data(5000, 3).len());
    let small = read(&mut reader, true).unwrap().unwrap();
    assert_data(&big, 5000, 3);
    assert_data(&small, 8, 4);
}

#[test]
fn multisegment_even_odd_and_maximum_tables_preserve_alignment_and_boundaries() {
    for count in [1usize, 2, 3, 4, 510, 511] {
        let table_bytes = (count / 2 + 1) * 8;
        let mut bytes = vec![0; table_bytes + count * 8];
        bytes[..4].copy_from_slice(&(count as u32 - 1).to_le_bytes());
        for i in 0..count {
            bytes[(i + 1) * 4..(i + 2) * 4].copy_from_slice(&1u32.to_le_bytes());
            bytes[table_bytes + i * 8..table_bytes + (i + 1) * 8].fill(i as u8);
        }
        let input = source([bytes, data(0, 0)].concat());
        input.0.borrow_mut().chunk = 3;
        let mut reader = BufferedRead::with_buffer_size(input, Default::default(), 256).unwrap();
        let msg = read(&mut reader, false).unwrap().unwrap();
        for i in 0..count {
            let segment = msg.get_segments().get_segment(i as u32).unwrap();
            assert_eq!(segment, &[i as u8; 8]);
            assert_eq!(segment.as_ptr() as usize % 8, 0);
        }
        assert_eq!(msg.get_segments().len(), count);
        assert_data(&read(&mut reader, true).unwrap().unwrap(), 0, 0);
    }
}

#[test]
fn malformed_truncated_and_oversized_frames_fail_stickily_before_body_allocation() {
    let mut zero_segments = vec![0; 8];
    zero_segments[..4].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut too_many = vec![0; 8];
    too_many[..4].copy_from_slice(&511u32.to_le_bytes());
    let mut oversized = vec![0; 8];
    oversized[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut invalid = vec![zero_segments, too_many, oversized];
    let frame = data(8, 0);
    for len in 1..frame.len() {
        invalid.push(frame[..len].to_vec());
    }
    for bytes in invalid {
        let input = source(bytes);
        let mut reader = BufferedRead::new(input.clone(), Default::default());
        assert!(read(&mut reader, true).is_err());
        let calls = input.0.borrow().calls;
        assert!(read(&mut reader, true).is_err());
        assert_eq!(input.0.borrow().calls, calls);
    }
    let input = source(data(16, 3));
    let mut options = capnp::message::ReaderOptions::new();
    options.traversal_limit_in_words(Some(1));
    let mut reader = BufferedRead::new(input, options);
    assert!(read(&mut reader, false).is_err());
}

#[test]
fn input_and_classifier_failures_are_terminal() {
    let input = source(data(64, 0));
    input.0.borrow_mut().available = 7;
    input.0.borrow_mut().closed = false;
    let mut reader = BufferedRead::new(input.clone(), Default::default());
    assert!(reader
        .try_read_message(|_| Ok(true))
        .now_or_never()
        .is_none());
    input.0.borrow_mut().failed = true;
    assert!(read(&mut reader, false).is_err());
    input.0.borrow_mut().failed = false;
    let calls = input.0.borrow().calls;
    assert!(read(&mut reader, false).is_err());
    assert_eq!(input.0.borrow().calls, calls);

    let mut reader = BufferedRead::new(source(data(8, 0)), Default::default());
    assert!(reader
        .try_read_message(|_| Err(capnp::Error::failed("classification failed".into())))
        .now_or_never()
        .unwrap()
        .is_err());
    assert!(read(&mut reader, true).is_err());
}

#[test]
fn buffered_prefetch_reduces_input_operations_against_unbuffered_reader() {
    let bytes: Vec<_> = (0..32).flat_map(|i| data(8, i)).collect();
    let plain = source(bytes.clone());
    for _ in 0..32 {
        futures::executor::block_on(capnp_futures::serialize::try_read_message(
            plain.clone(),
            Default::default(),
        ))
        .unwrap()
        .unwrap();
    }
    let buffered = source(bytes);
    let mut reader = BufferedRead::new(buffered.clone(), Default::default());
    for i in 0..32 {
        assert_data(&read(&mut reader, true).unwrap().unwrap(), 8, i);
    }
    assert_eq!(plain.0.borrow().calls, 64);
    assert_eq!(buffered.0.borrow().calls, 1);
    eprintln!(
        "32 small frames: {} unbuffered reads / {} buffered read",
        plain.0.borrow().calls,
        buffered.0.borrow().calls
    );
}

#[test]
fn tlc_buffered_input_replays_ownership_prefetch_cancellation_and_eof() {
    use capntproto_test_support::verification::exploration::{controls, traces};
    const MODEL: &str = "verification/RpcBufferedInput.tla";
    const CONFIG: &str = include_str!("../verification/RpcBufferedInput.cfg");
    let paths = traces(MODEL, "buffered-input", CONFIG).unwrap();
    let mut outcomes = [false; 6];
    for path in paths {
        let input = source([data(8, 11), data(16, 22)].concat());
        input.0.borrow_mut().available = 0;
        input.0.borrow_mut().closed = false;
        let mut reader =
            BufferedRead::with_buffer_size(input.clone(), Default::default(), 256).unwrap();
        let mut held: [Option<Message>; 2] = [None, None];
        let mut delivered = 0;
        for state in path {
            match state["event"] {
                1 => input.0.borrow_mut().available = state["offered"] as usize * 8,
                2 => input.0.borrow_mut().closed = true,
                3 => {
                    let result = reader
                        .try_read_message(|_| Ok(state["kind"] == 1))
                        .now_or_never();
                    let code = match result {
                        None => 2,
                        Some(Ok(None)) => 3,
                        Some(Err(error)) => {
                            if error.to_string().contains("still alive") {
                                5
                            } else {
                                4
                            }
                        }
                        Some(Ok(Some(msg))) => {
                            held[delivered] = Some(msg);
                            delivered += 1;
                            1
                        }
                    };
                    outcomes[code] = true;
                    assert_eq!(code as u64, state["result"], "{state:?}");
                }
                4 => {
                    held[0].take();
                }
                5 => {
                    held[1].take();
                }
                _ => panic!("{state:?}"),
            }
            assert_eq!(
                input.0.borrow().position as u64,
                state["pos"] * 8,
                "{state:?}"
            );
            assert_eq!(delivered as u64, state["delivered"], "{state:?}");
            for (i, key) in ["h1", "h2"].iter().enumerate() {
                let mode = held[i].as_ref().map_or(0, |msg| {
                    if msg.get_segments().is_shared_buffer() {
                        1
                    } else {
                        2
                    }
                });
                assert_eq!(mode, state[*key], "{state:?}");
                if let Some(msg) = &held[i] {
                    assert_data(msg, 8 * (i + 1), 11 * (i + 1) as u8);
                }
            }
        }
    }
    assert!(outcomes[1..].iter().all(|x| *x));
    let live =
        CONFIG.replace("SPECIFICATION Spec", "SPECIFICATION LiveSpec") + "\nPROPERTY EndProgress\n";
    controls(
        MODEL,
        "buffered-input",
        CONFIG,
        &[
            ("retainShared", "IndependentRetention"),
            ("overwriteLive", "LeaseSafety"),
            ("cancelPrefix", "PrefixSurvivesCancel"),
            ("retryFailure", "StickyFailure"),
        ],
        Some(&live),
    )
    .unwrap();
}

fn rpc_frame(tag: u16, payload: usize) -> Vec<u8> {
    let mut builder = Builder::new_default();
    let mut message = builder.init_root::<capnp_rpc::rpc_capnp::message::Builder>();
    message
        .reborrow()
        .init_abort()
        .set_reason("x".repeat(payload));
    let mut wire = capnp::serialize::write_message_to_words(&builder);
    // One-segment framing (8 bytes), root pointer (8 bytes), then Message's
    // first data word contains the union discriminant. Only classification is
    // tested here; unused union payloads are intentionally not dispatched.
    assert_eq!(&wire[..4], &[0; 4]);
    wire[16..18].copy_from_slice(&tag.to_le_bytes());
    wire
}

#[test]
fn all_rpc_discriminants_and_buffer_lifetimes_match_pinned_cpp() {
    use capntproto_test_support::verification::{command, cpp, root, run};
    let build = cpp::build(&["capnp-rpc"]).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("buffered-input");
    let logs = root().join("target/verification/buffered-input-cpp");
    let mut compile = command("g++");
    compile
        .args([
            "-std=c++23",
            "-Ivendor/capnproto/c++/src",
            "tests/cpp/buffered-input.c++",
        ])
        .arg(build.join("c++/src/capnp/libcapnp-rpc.a"))
        .arg(build.join("c++/src/capnp/libcapnp.a"))
        .arg(build.join("c++/src/kj/libkj-async.a"))
        .arg(build.join("c++/src/kj/libkj.a"))
        .args(["-pthread", "-o"])
        .arg(&executable);
    run(&mut compile, &logs.join("compile.log"), 0).unwrap();
    let tags: Vec<_> = (0u16..=14).chain([255, 65535]).collect();
    let bytes: Vec<_> = (0..6)
        .flat_map(|round| {
            tags.iter()
                .flat_map(move |&tag| rpc_frame(tag, [0, 64, 400, 1800, 6000, 8][round]))
        })
        .collect();
    let input_file = directory.path().join("frames.bin");
    std::fs::write(&input_file, &bytes).unwrap();
    let reference = run(
        command(&executable).arg(input_file),
        &logs.join("reference.log"),
        0,
    )
    .unwrap();
    let input = source(bytes);
    let mut reader =
        BufferedRead::with_buffer_size(input.clone(), Default::default(), 256).unwrap();
    let mut retained = Vec::new();
    let mut observed = String::new();
    while let Some(msg) = reader
        .try_read_message(|m| capnp_rpc::is_short_lived_rpc_message(m.get_root()?))
        .now_or_never()
        .unwrap()
        .unwrap()
    {
        let root = msg.get_root::<capnp::any_pointer::Reader>().unwrap();
        let short = capnp_rpc::is_short_lived_rpc_message(root).unwrap();
        let segments = msg.get_segments();
        let tag = u16::from_le_bytes(segments.get_segment(0).unwrap()[8..10].try_into().unwrap());
        assert_eq!(short, tag != 2 && tag != 3);
        observed += &format!(
            "{tag} {} {} {}\n",
            usize::from(short),
            usize::from(segments.is_shared_buffer()),
            input.0.borrow().calls
        );
        if segments.is_shared_buffer() {
            assert!(read(&mut reader, true).is_err());
        } else {
            retained.push((tag, msg));
        }
    }
    assert_eq!(observed, reference);
    for (tag, msg) in retained {
        assert_eq!(
            u16::from_le_bytes(
                msg.get_segments().get_segment(0).unwrap()[8..10]
                    .try_into()
                    .unwrap()
            ),
            tag
        );
    }
    eprintln!(
        "{} buffered RPC observations match pinned C++",
        observed.lines().count()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn two_party_rpc_retains_call_and_return_capabilities_across_buffer_reuse() {
    use capnp_rpc::{rpc_twoparty_capnp::Side, RpcSystem};
    use capntproto_test_support::runtime_test_capnp::harness;
    use futures::channel::oneshot;
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
    struct Service {
        offset: u32,
        gate: RefCell<Option<oneshot::Receiver<()>>>,
    }
    impl harness::Server for Service {
        async fn echo(
            self: Rc<Self>,
            p: harness::EchoParams,
            mut r: harness::EchoResults,
        ) -> capnp::Result<()> {
            r.get().set_value(self.offset + p.get()?.get_value());
            Ok(())
        }
        async fn bounce(
            self: Rc<Self>,
            p: harness::BounceParams,
            mut r: harness::BounceResults,
        ) -> capnp::Result<()> {
            let gate = self.gate.borrow_mut().take();
            if let Some(gate) = gate {
                gate.await.unwrap();
            }
            r.get().set_cap(p.get()?.get_cap()?);
            Ok(())
        }
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                let (a, b) = tokio::io::duplex(128);
                let (ar, aw) = tokio::io::split(a);
                let (br, bw) = tokio::io::split(b);
                let (release, gate) = oneshot::channel();
                let server: harness::Client = capnp_rpc::new_client(Service {
                    offset: 10,
                    gate: RefCell::new(Some(gate)),
                });
                let server = RpcSystem::new(
                    Box::new(capnp_rpc::twoparty::VatNetwork::new(
                        br.compat(),
                        bw.compat_write(),
                        Side::Server,
                        Default::default(),
                    )),
                    Some(server.client),
                );
                let mut client = RpcSystem::new(
                    Box::new(capnp_rpc::twoparty::VatNetwork::new(
                        ar.compat(),
                        aw.compat_write(),
                        Side::Client,
                        Default::default(),
                    )),
                    None,
                );
                let remote: harness::Client = client.bootstrap(Side::Server);
                let server_task = tokio::task::spawn_local(server);
                let client_task = tokio::task::spawn_local(client);
                let local: harness::Client = capnp_rpc::new_client(Service {
                    offset: 100,
                    gate: RefCell::new(None),
                });
                let mut request = remote.bounce_request();
                request.get().set_cap(local);
                let bounced = request.send();
                for _ in 0..2 {
                    let requests: Vec<_> = (0..40)
                        .map(|i| {
                            let mut request = remote.echo_request();
                            request.get().set_value(i);
                            request.send().promise
                        })
                        .collect();
                    let results = futures::future::try_join_all(requests).await.unwrap();
                    for (i, result) in results.iter().enumerate() {
                        assert_eq!(result.get().unwrap().get_value(), 10 + i as u32);
                    }
                }
                release.send(()).unwrap();
                let response = bounced.promise.await.unwrap();
                // Hold the Return's reader while more messages reuse receive storage.
                for _ in 0..40 {
                    remote.echo_request().send().promise.await.unwrap();
                }
                let cap = response.get().unwrap().get_cap().unwrap();
                let mut request = cap.echo_request();
                request.get().set_value(7);
                assert_eq!(
                    request
                        .send()
                        .promise
                        .await
                        .unwrap()
                        .get()
                        .unwrap()
                        .get_value(),
                    107
                );
                client_task.abort();
                server_task.abort();
            })
            .await
            .unwrap();
        })
        .await;
}

#[test]
fn two_party_read_contract_recovers_after_rejected_live_control_message() {
    use capnp_rpc::{rpc_twoparty_capnp::Side, VatNetwork};
    let input = source([rpc_frame(4, 8), rpc_frame(6, 8)].concat());
    let mut network = capnp_rpc::twoparty::VatNetwork::new(
        input.clone(),
        futures::io::Cursor::new(Vec::new()),
        Side::Client,
        Default::default(),
    );
    let mut connection = network.connect(Side::Server).unwrap();
    let first = connection
        .receive_incoming_message()
        .now_or_never()
        .unwrap()
        .unwrap()
        .unwrap();
    let calls = input.0.borrow().calls;
    assert!(connection
        .receive_incoming_message()
        .now_or_never()
        .unwrap()
        .is_err());
    assert_eq!(input.0.borrow().calls, calls);
    drop(first);
    let second = connection
        .receive_incoming_message()
        .now_or_never()
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(
        second
            .get_body()
            .unwrap()
            .get_as::<capnp_rpc::rpc_capnp::message::Reader>()
            .unwrap()
            .which(),
        Ok(capnp_rpc::rpc_capnp::message::Release(_))
    ));
}

#[test]
fn empty_segments_configuration_and_fully_prefetched_large_frames() {
    assert!(BufferedRead::with_buffer_size(source(vec![]), Default::default(), 255).is_err());
    assert!(
        BufferedRead::with_buffer_size(source(vec![]), Default::default(), usize::MAX).is_err()
    );
    // A zero-word segment is a legal frame, even though it has no readable root.
    let mut reader = BufferedRead::new(source(vec![0; 8]), Default::default());
    let empty = read(&mut reader, true).unwrap().unwrap();
    assert_eq!(empty.get_segments().len(), 1);
    assert!(empty.get_segments().get_segment(0).unwrap().is_empty());
    drop(empty);
    assert!(read(&mut reader, true).unwrap().is_none());
    // Large only selects direct I/O when the frame is not already complete.
    let mut reader =
        BufferedRead::with_buffer_size(source(data(1500, 6)), Default::default(), 256).unwrap();
    let msg = read(&mut reader, true).unwrap().unwrap();
    assert!(msg.get_segments().is_shared_buffer());
    assert_data(&msg, 1500, 6);
}
