use capnp::message::{Builder, HeapAllocator};
use capnp_futures::{OutgoingQueue, QueueSnapshot, Sender};
use futures::{Future, FutureExt};
use std::{
    cell::RefCell,
    io::{self, IoSlice},
    pin::Pin,
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    task::{Context, Poll},
    time::Duration,
};

type Message = Builder<HeapAllocator>;
type Receipt<T> = Pin<Box<dyn Future<Output = capnp::Result<T>>>>;

#[derive(Default)]
struct Output {
    bytes: Vec<u8>,
    budget: usize,
    chunk: usize,
    flush: bool,
    single_flush: bool,
    fault: u8, // 1: write error, 2: flush error, 3: WriteZero
    writes: usize,
    flushes: usize,
}
#[derive(Clone)]
struct Writer(Rc<RefCell<Output>>);
impl futures::AsyncWrite for Writer {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.poll_write_vectored(cx, &[IoSlice::new(bytes)])
    }
    fn poll_write_vectored(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buffers: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let mut out = self.0.borrow_mut();
        if out.fault == 1 {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        if out.fault == 3 {
            return Poll::Ready(Ok(0));
        }
        if out.budget == 0 {
            return Poll::Pending;
        }
        let count = buffers
            .iter()
            .map(|b| b.len())
            .sum::<usize>()
            .min(out.budget)
            .min(out.chunk);
        let mut left = count;
        for buffer in buffers {
            let size = buffer.len().min(left);
            out.bytes.extend_from_slice(&buffer[..size]);
            left -= size;
        }
        out.budget -= count;
        out.writes += 1;
        Poll::Ready(Ok(count))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut out = self.0.borrow_mut();
        if out.fault == 2 {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        if !out.flush {
            return Poll::Pending;
        }
        out.flushes += 1;
        if out.single_flush {
            out.flush = false;
        }
        Poll::Ready(Ok(()))
    }
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_flush(cx)
    }
}

fn message(size: usize) -> Message {
    let mut result = Builder::new_default();
    result
        .initn_root::<capnp::data::Builder>(size as u32)
        .fill(size as u8);
    result
}
fn poll<T>(future: Pin<&mut dyn Future<Output = T>>) -> Poll<T> {
    future.poll(&mut Context::from_waker(futures::task::noop_waker_ref()))
}
struct Harness {
    sender: Sender<Message>,
    driver: Option<Receipt<()>>,
    queue: OutgoingQueue,
    output: Rc<RefCell<Output>>,
    time: Arc<AtomicU64>,
    receipts: Vec<Option<Receipt<Message>>>,
    outcomes: Vec<u64>, // 1 pending, 2 success, 3 failure, 4 canceled
    termination: Option<Receipt<()>>,
    terminated: bool,
    result: u64, // 0 pending, 1 success, 2 failure
}
impl Harness {
    fn new() -> Self {
        let output = Rc::new(RefCell::new(Output {
            chunk: usize::MAX,
            ..Default::default()
        }));
        let time = Arc::new(AtomicU64::new(0));
        let clock = time.clone();
        let (sender, driver) =
            capnp_futures::write_queue_with_clock(Writer(output.clone()), move || {
                Duration::from_secs(clock.load(Ordering::SeqCst))
            });
        let queue = sender.outgoing_queue();
        Self {
            sender,
            driver: Some(Box::pin(driver)),
            queue,
            output,
            time,
            receipts: vec![],
            outcomes: vec![],
            termination: None,
            terminated: false,
            result: 0,
        }
    }
    fn send(&mut self, size: usize) {
        self.receipts
            .push(Some(Box::pin(self.sender.send(message(size)))));
        self.outcomes.push(1);
        self.observe();
    }
    fn observe(&mut self) {
        for (receipt, status) in self.receipts.iter_mut().zip(&mut self.outcomes) {
            if let Some(future) = receipt {
                if let Poll::Ready(result) = poll(future.as_mut()) {
                    *status = if result.is_ok() { 2 } else { 3 };
                    receipt.take();
                }
            }
        }
        if let Some(future) = &mut self.termination {
            if let Poll::Ready(result) = poll(future.as_mut()) {
                self.terminated = result.is_ok();
                self.termination.take();
            }
        }
    }
    fn pump(&mut self) {
        if let Some(future) = &mut self.driver {
            if let Poll::Ready(result) = poll(future.as_mut()) {
                self.result = if result.is_ok() { 1 } else { 2 };
                self.driver.take();
            }
        }
        self.observe();
    }
    fn cancel(&mut self, index: usize) {
        self.receipts[index].take();
        self.outcomes[index] = 4;
    }
    fn terminate(&mut self) {
        self.termination = Some(Box::pin(self.sender.terminate(Ok(()))));
    }
    fn snapshot(&self) -> QueueSnapshot {
        self.queue.snapshot()
    }
}

#[test]
fn batch_metrics_exclude_active_io_and_restart_age_after_idle() {
    let mut h = Harness::new();
    h.send(0);
    h.time.store(2, Ordering::SeqCst);
    h.send(8);
    assert_eq!(
        h.snapshot(),
        QueueSnapshot {
            message_count: 2,
            bytes: 24,
            wait_time: Duration::from_secs(2)
        }
    );
    h.cancel(0);
    h.pump(); // Both messages become active, even with no writer capacity.
    assert_eq!(h.snapshot(), QueueSnapshot::default());
    h.time.store(10, Ordering::SeqCst);
    h.send(16);
    h.time.store(11, Ordering::SeqCst);
    assert_eq!(
        h.snapshot(),
        QueueSnapshot {
            message_count: 1,
            bytes: 24,
            wait_time: Duration::from_secs(1)
        }
    );
    h.output.borrow_mut().budget = 40; // First batch, including framing.
    h.pump();
    assert_eq!(
        h.outcomes,
        [4, 1, 1],
        "writing without flushing is not completion"
    );
    h.output.borrow_mut().flush = true;
    h.pump();
    assert_eq!(h.outcomes, [4, 2, 1]);
    assert_eq!(h.snapshot(), QueueSnapshot::default());
    assert_eq!(
        h.output.borrow().writes,
        1,
        "one vectored write spans two messages"
    );
    h.output.borrow_mut().budget = usize::MAX;
    h.terminate();
    h.pump();
    assert_eq!(h.outcomes, [4, 2, 2]);
    assert!(h.terminated);
    assert_eq!(h.result, 1);
    assert_eq!(h.output.borrow().flushes, 2);
    let output = h.output.borrow();
    let mut bytes = &output.bytes[..];
    for size in [0, 8, 16] {
        let decoded = capnp::serialize::read_message(&mut bytes, Default::default()).unwrap();
        assert_eq!(
            decoded.get_root::<capnp::data::Reader>().unwrap(),
            vec![size as u8; size]
        );
    }
    assert!(bytes.is_empty());
}

#[test]
fn shutdown_rejects_all_clones_before_poll_and_drain_error_is_preserved() {
    let mut h = Harness::new();
    let mut clone = h.sender.clone();
    h.send(8);
    let mut terminated = Box::pin(
        h.sender
            .terminate(Err(capnp::Error::failed("original shutdown".into()))),
    );
    assert!(clone.send(message(0)).now_or_never().unwrap().is_err());
    assert!(clone.terminate(Ok(())).now_or_never().unwrap().is_err());
    assert_eq!(h.sender.len(), 1);
    h.output.borrow_mut().budget = usize::MAX;
    h.output.borrow_mut().flush = true;
    let result = poll(h.driver.as_mut().unwrap().as_mut());
    assert!(matches!(result, Poll::Ready(Err(e)) if e.to_string().contains("original shutdown")));
    assert!(matches!(poll(terminated.as_mut()), Poll::Ready(Ok(()))));
    h.observe();
    assert_eq!(h.outcomes, [2]);
    assert!(h.sender.is_empty());
}

#[test]
fn failure_and_driver_cancellation_clear_metrics_and_fail_every_receipt() {
    for fault in 0..4 {
        for started in [false, true] {
            let mut h = Harness::new();
            h.send(8);
            if started {
                h.pump();
            }
            h.send(16);
            h.terminate();
            if fault == 0 {
                h.driver.take(); // Includes dropping an unpolled future.
                h.observe();
            } else {
                h.output.borrow_mut().fault = fault;
                h.output.borrow_mut().budget = usize::MAX;
                h.pump();
                assert_eq!(h.result, 2);
            }
            assert_eq!(h.outcomes, [3, 3]);
            assert!(!h.terminated);
            assert_eq!(h.snapshot(), QueueSnapshot::default());
            assert!(h.sender.send(message(0)).now_or_never().unwrap().is_err());
        }
    }
}

#[test]
fn last_sender_drop_drains_and_observer_does_not_keep_driver_alive() {
    let output = Rc::new(RefCell::new(Output {
        budget: usize::MAX,
        chunk: usize::MAX,
        flush: true,
        ..Default::default()
    }));
    let (mut sender, driver) = capnp_futures::write_queue(Writer(output.clone()));
    let observer = sender.outgoing_queue();
    let receipt = sender.send(message(8));
    drop(sender);
    futures::executor::block_on(driver).unwrap();
    futures::executor::block_on(receipt).unwrap();
    assert_eq!(observer.snapshot(), QueueSnapshot::default());
    assert_eq!(output.borrow().bytes.len(), 24);
}

#[test]
fn vectored_batches_preserve_multisegment_framing_under_partial_writes() {
    let mut messages: Vec<_> = (0..30)
        .map(|_| {
            Builder::new(
                HeapAllocator::new()
                    .first_segment_words(1)
                    .allocation_strategy(capnp::message::AllocationStrategy::FixedSize),
            )
        })
        .collect();
    // Populated messages with both even and odd segment-table padding.
    for (n, message) in messages.iter_mut().enumerate() {
        let mut list = message.initn_root::<capnp::data_list::Builder>(n as u32 + 1);
        for i in 0..=n {
            list.set(i as u32, &vec![i as u8; 9 + n]);
        }
    }
    let expected: Vec<u8> = messages
        .iter()
        .flat_map(capnp::serialize::write_message_to_words)
        .collect();
    for chunk in [1, 7, 8, 19, usize::MAX] {
        let output = Rc::new(RefCell::new(Output {
            budget: usize::MAX,
            chunk,
            ..Default::default()
        }));
        futures::executor::block_on(capnp_futures::serialize::write_messages(
            Writer(output.clone()),
            &messages,
        ))
        .unwrap();
        assert_eq!(output.borrow().bytes, expected, "chunk {chunk}");
        assert_eq!(output.borrow().flushes, 0);
    }
    let mut scalar = futures::io::Cursor::new(Vec::new());
    futures::executor::block_on(capnp_futures::serialize::write_messages(
        &mut scalar,
        &messages,
    ))
    .unwrap();
    assert_eq!(scalar.into_inner(), expected);
    futures::executor::block_on(capnp_futures::serialize::write_messages(
        Writer(Rc::new(RefCell::new(Output::default()))),
        &[] as &[Message],
    ))
    .unwrap();
}

#[test]
fn tlc_queue_traces_replay_batching_flush_failure_cancellation_and_shutdown() {
    use reproto_test_support::verification::exploration::{controls, traces};
    const MODEL: &str = "verification/RpcOutgoingQueue.tla";
    const CONFIG: &str = include_str!("../verification/RpcOutgoingQueue.cfg");
    let paths = traces(MODEL, "outgoing-queue", CONFIG).unwrap();
    let mut covered = [false; 13];
    for path in paths {
        let mut h = Harness::new();
        for state in path {
            let event = state["event"] as usize;
            covered[event] = true;
            match event {
                1 => h.send(if h.receipts.is_empty() { 0 } else { 8 }),
                2 => h.time.store(state["now"], Ordering::SeqCst),
                3 | 12 => h.pump(),
                4 => {
                    h.output.borrow_mut().budget = usize::MAX;
                    h.output.borrow_mut().flush = false;
                    h.pump();
                    h.output.borrow_mut().budget = 0;
                }
                5 => {
                    h.output.borrow_mut().flush = true;
                    h.pump();
                    h.output.borrow_mut().flush = false;
                }
                6 => h.terminate(),
                7 => assert!(h.sender.send(message(0)).now_or_never().unwrap().is_err()),
                8 => h.cancel(1),
                9 | 10 => {
                    h.output.borrow_mut().fault = if event == 9 { 1 } else { 2 };
                    h.pump();
                }
                11 => {
                    h.driver.take();
                    h.result = 2;
                    h.observe();
                }
                _ => panic!("{state:?}"),
            }
            let snapshot = h.snapshot();
            assert_eq!(snapshot.message_count as u64, state["count"], "{state:?}");
            assert_eq!(snapshot.bytes as u64, state["bytes"], "{state:?}");
            assert_eq!(
                snapshot.wait_time.as_secs(),
                if state["count"] == 0 {
                    0
                } else {
                    state["now"] - state["oldest"]
                },
                "{state:?}"
            );
            for (i, key) in ["r1", "r2"].iter().enumerate() {
                assert_eq!(
                    h.outcomes.get(i).copied().unwrap_or(0),
                    state[*key],
                    "{state:?}"
                );
            }
            assert_eq!(h.result, state["done"], "{state:?}");
            assert_eq!(h.terminated, state["done"] == 1, "{state:?}");
            // Decode the actual byte stream to verify message identity and FIFO,
            // including messages whose receipt was canceled.
            let output = h.output.borrow();
            let mut wire = &output.bytes[..];
            let mut received = Vec::new();
            while !wire.is_empty() {
                let msg = capnp::serialize::read_message(&mut wire, Default::default()).unwrap();
                received.push(msg.get_root::<capnp::data::Reader>().unwrap().len());
            }
            assert!(received.is_empty() || received == [0] || received == [0, 8]);
            if state["m1"] == 3 || state["m1"] == 4 {
                assert!(!received.is_empty());
            }
            if state["m2"] == 3 || state["m2"] == 4 {
                assert_eq!(received, [0, 8]);
            }
        }
    }
    assert!(covered[1..].iter().all(|x| *x));
    let live = CONFIG.replace("SPECIFICATION Spec", "SPECIFICATION LiveSpec")
        + "\nPROPERTY DrainProgress\n";
    controls(
        MODEL,
        "outgoing-queue",
        CONFIG,
        &[
            ("activeCounts", "Metrics"),
            ("earlyReceipt", "FlushFence"),
            ("lateSend", "Admission"),
            ("staleFailure", "Metrics"),
            ("staleAge", "QueueAge"),
            ("cancelMessage", "CancellationOwnership"),
        ],
        Some(&live),
    )
    .unwrap();
}

#[test]
fn queue_metrics_and_batch_boundaries_match_pinned_cpp_network() {
    use capnp_rpc::{rpc_twoparty_capnp::Side, VatNetwork};
    use reproto_test_support::verification::{command, cpp, root, run};
    let build = cpp::build(&["capnp-rpc"]).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("outgoing-queue");
    let logs = root().join("target/verification/outgoing-queue-cpp");
    let mut compile = command("g++");
    compile
        .args([
            "-std=c++23",
            "-Ivendor/capnproto/c++/src",
            "tests/cpp/outgoing-queue.c++",
        ])
        .arg(build.join("c++/src/capnp/libcapnp-rpc.a"))
        .arg(build.join("c++/src/capnp/libcapnp.a"))
        .arg(build.join("c++/src/kj/libkj-async.a"))
        .arg(build.join("c++/src/kj/libkj.a"))
        .args(["-pthread", "-o"])
        .arg(&executable);
    run(&mut compile, &logs.join("compile.log"), 0).unwrap();
    let mut script = String::new();
    let mut time = 0;
    for round in 0..40 {
        time += 100;
        script += &format!("time {time}\n");
        for n in 0..1 + round % 5 {
            script += &format!("send {}\n", n * 8);
            time += 1;
            script += &format!("time {time}\n");
        }
        script += "poll\n";
        for n in 0..1 + round % 3 {
            script += &format!("send {}\n", n * 8);
            time += 1;
            script += &format!("time {time}\n");
        }
        script += "poll\ncomplete\ncomplete\n";
    }
    let input = directory.path().join("commands");
    std::fs::write(&input, &script).unwrap();
    let reference = run(
        command(&executable).arg(input),
        &logs.join("reference.log"),
        0,
    )
    .unwrap();
    let output = Rc::new(RefCell::new(Output {
        budget: usize::MAX,
        chunk: usize::MAX,
        single_flush: true,
        ..Default::default()
    }));
    let time = Arc::new(AtomicU64::new(0));
    let clock = time.clone();
    let mut network = capnp_rpc::twoparty::VatNetwork::new_with_clock(
        futures::io::Cursor::new(Vec::<u8>::new()),
        Writer(output.clone()),
        Side::Client,
        Default::default(),
        move || Duration::from_secs(clock.load(Ordering::SeqCst)),
    );
    let observer = network.outgoing_queue();
    let mut connection = network.connect(Side::Server).unwrap();
    let mut driver = Box::pin(network.drive_until_shutdown());
    let mut observed = String::new();
    for line in script.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        match fields[0] {
            "time" => time.store(fields[1].parse().unwrap(), Ordering::SeqCst),
            "send" => {
                let size: u32 = fields[1].parse().unwrap();
                let mut message = connection.new_outgoing_message(64);
                message
                    .get_body()
                    .unwrap()
                    .initn_as::<capnp::data::Builder>(size)
                    .fill(size as u8);
                let (_receipt, _body) = message.send(); // Dropped receipts still write.
            }
            "poll" => {
                assert!(poll(driver.as_mut()).is_pending());
            }
            "complete" => {
                output.borrow_mut().flush = true;
                assert!(poll(driver.as_mut()).is_pending());
            }
            _ => unreachable!(),
        }
        let snapshot = observer.snapshot();
        assert_eq!(network.get_current_queue_count(), snapshot.message_count);
        assert_eq!(network.get_current_queue_size(), snapshot.bytes);
        assert_eq!(network.get_outgoing_message_wait_time(), snapshot.wait_time);
        observed += &format!(
            "{} {} {} {}\n",
            snapshot.message_count,
            snapshot.bytes,
            snapshot.wait_time.as_secs(),
            output.borrow().writes
        );
    }
    assert_eq!(observed, reference);
    eprintln!(
        "{} timed queue observations match pinned C++",
        observed.lines().count()
    );
    drop(connection);
    drop(network);
    drop(driver);
    assert_eq!(observer.snapshot(), QueueSnapshot::default());
}

#[test]
fn cloned_senders_wake_driver_and_preserve_each_threads_fifo() {
    let mut writer = futures::io::Cursor::new(Vec::new());
    std::thread::scope(|scope| {
        let (sender, driver) = capnp_futures::write_queue(&mut writer);
        scope.spawn(move || futures::executor::block_on(driver).unwrap());
        for thread in 0u32..4 {
            let mut sender = sender.clone();
            scope.spawn(move || {
                for sequence in 0u32..64 {
                    let mut msg = message(8);
                    let data = msg.get_root::<capnp::data::Builder>().unwrap();
                    data[..4].copy_from_slice(&thread.to_le_bytes());
                    data[4..].copy_from_slice(&sequence.to_le_bytes());
                    futures::executor::block_on(sender.send(msg)).unwrap();
                }
            });
        }
        drop(sender);
    });
    let mut next = [0u32; 4];
    let bytes = writer.into_inner();
    let mut input = &bytes[..];
    while !input.is_empty() {
        let message = capnp::serialize::read_message(&mut input, Default::default()).unwrap();
        let data = message.get_root::<capnp::data::Reader>().unwrap();
        let thread = u32::from_le_bytes(data[..4].try_into().unwrap()) as usize;
        let sequence = u32::from_le_bytes(data[4..].try_into().unwrap());
        assert_eq!(next[thread], sequence);
        next[thread] += 1;
    }
    assert_eq!(next, [64; 4]);
}

#[test]
fn cancel_after_partial_write_fails_active_and_queued_messages() {
    let mut h = Harness::new();
    h.send(16);
    h.output.borrow_mut().budget = 7; // In the middle of the framing table.
    h.pump();
    h.send(8);
    assert_eq!(h.output.borrow().bytes.len(), 7);
    assert_eq!(h.sender.len(), 1);
    h.driver.take();
    h.observe();
    assert_eq!(h.outcomes, [3, 3]);
    assert_eq!(h.snapshot(), QueueSnapshot::default());
    assert_eq!(h.output.borrow().bytes.len(), 7);
}

#[test]
fn diagnostics_survive_moving_network_into_rpc_system() {
    use capnp_rpc::rpc_twoparty_capnp::Side;
    let output = Rc::new(RefCell::new(Output::default()));
    let network = capnp_rpc::twoparty::VatNetwork::new(
        futures::io::Cursor::new(Vec::<u8>::new()),
        Writer(output),
        Side::Client,
        Default::default(),
    );
    let observer = network.outgoing_queue();
    let mut rpc = capnp_rpc::RpcSystem::new(Box::new(network), None);
    let capability: capnp::capability::Client = rpc.bootstrap(Side::Server);
    assert_eq!(observer.snapshot().message_count, 1);
    assert!(observer.snapshot().bytes > 0);
    drop(capability);
    drop(rpc);
    assert_eq!(observer.snapshot(), QueueSnapshot::default());
}

#[test]
fn batch_writer_uses_scalar_fallback_without_vectored_support() {
    struct Scalar(Writer);
    impl futures::AsyncWrite for Scalar {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            Pin::new(&mut self.0).poll_write(cx, bytes)
        }
        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.0).poll_flush(cx)
        }
        fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.0).poll_close(cx)
        }
    }
    let output = Rc::new(RefCell::new(Output {
        budget: usize::MAX,
        chunk: 3,
        ..Default::default()
    }));
    let messages = [message(0), message(8), message(24)];
    futures::executor::block_on(capnp_futures::serialize::write_messages(
        Scalar(Writer(output.clone())),
        &messages,
    ))
    .unwrap();
    let expected: Vec<_> = messages
        .iter()
        .flat_map(capnp::serialize::write_message_to_words)
        .collect();
    assert_eq!(output.borrow().bytes, expected);
}

#[test]
fn dropped_shutdown_receipt_still_drains_and_terminates() {
    let mut h = Harness::new();
    h.send(8);
    drop(h.sender.terminate(Ok(())));
    assert!(h.sender.send(message(0)).now_or_never().unwrap().is_err());
    h.output.borrow_mut().budget = usize::MAX;
    h.output.borrow_mut().flush = true;
    h.pump();
    assert_eq!(h.result, 1);
    assert_eq!(h.outcomes, [2]);
    assert_eq!(h.output.borrow().bytes.len(), 24);
    assert_eq!(h.snapshot(), QueueSnapshot::default());
}

#[test]
fn canceled_driver_releases_registered_task_waker_with_senders_alive() {
    struct Wake;
    impl futures::task::ArcWake for Wake {
        fn wake_by_ref(_: &Arc<Self>) {}
    }
    let task = Arc::new(Wake);
    let weak = Arc::downgrade(&task);
    let waker = futures::task::waker(task);
    let (sender, driver) =
        capnp_futures::write_queue::<_, Message>(futures::io::Cursor::new(Vec::new()));
    let mut driver = Box::pin(driver);
    assert!(driver
        .as_mut()
        .poll(&mut Context::from_waker(&waker))
        .is_pending());
    drop(waker);
    assert!(weak.upgrade().is_some());
    drop(driver);
    assert!(weak.upgrade().is_none());
    assert!(sender.is_empty());
}
