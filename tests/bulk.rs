use futures::{channel::oneshot, FutureExt};
use reproto::{
    bulk::{Config, CreditWindow, Receiver, Sender, Settlement, Status, Summary},
    bulk_capnp::transfer,
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
    time::Duration,
};

fn config(length: u64, window: u32) -> Config {
    Config::new(length, window.min(2), window, 16).unwrap()
}

#[test]
fn credit_is_reserved_until_acknowledgment_and_duplicates_are_idempotent() {
    let mut w = CreditWindow::new(3, 3).unwrap();
    assert!(w.reserve(0).is_err());
    assert!(w.reserve(4).is_err());
    let one = w.reserve(1).unwrap().unwrap();
    let two = w.reserve(2).unwrap().unwrap();
    assert_eq!((one.sequence(), one.bytes()), (1, 1));
    assert_eq!((two.sequence(), two.bytes()), (2, 2));
    assert!(w.reserve(1).unwrap().is_none());
    assert_eq!(w.issued(), 2);
    assert_eq!(w.in_flight(), 3);
    assert_eq!(w.settle(&two).unwrap(), Settlement::Released);
    assert_eq!(w.settle(&two).unwrap(), Settlement::AlreadySettled);
    assert_eq!(w.available(), 2);
    let three = w.reserve(2).unwrap().unwrap();
    assert_eq!(three.sequence(), 3);
    assert_eq!(w.settle(&one).unwrap(), Settlement::Released);
    assert_eq!(w.settle(&three).unwrap(), Settlement::Released);
    assert_eq!(w.available(), 3);
    assert!(w.reserve(1).is_err());
}

#[test]
fn atomic_publication_sticky_failure_and_transfer_isolation() {
    let (a, _ac) = Receiver::new(config(3, 3));
    let (b, _bc) = Receiver::new(config(3, 3));
    a.write(1, b"a").unwrap();
    b.write(1, b"b").unwrap();
    assert!(a.completed().is_none());
    let error = a.write(1, b"duplicate").unwrap_err();
    assert_eq!(a.status(), Status::Failed);
    assert_eq!(a.staged_bytes(), 0);
    assert_eq!(a.write(2, b"aa").unwrap_err().extra, error.extra);
    assert_eq!(a.done().unwrap_err().extra, error.extra);
    assert_eq!(a.cancel(), Status::Canceled);
    assert_eq!(a.done().unwrap_err().extra, error.extra);
    assert_eq!(a.cancel(), Status::Canceled);
    a.fail(capnp::Error::failed("later failure".into()));
    assert_eq!(a.failure().unwrap().extra, error.extra);
    assert_eq!(a.staged_bytes(), 0);
    assert!(a.completed().is_none());
    b.write(2, b"bb").unwrap();
    assert_eq!(
        b.done().unwrap(),
        Summary {
            bytes: 3,
            chunks: 2
        }
    );
    let value = b.completed().unwrap();
    assert_eq!(&*value, b"bbb");
    b.done().unwrap();
    assert_eq!(b.cancel(), Status::Complete);
    b.fail(capnp::Error::failed("too late".into()));
    assert!(Rc::ptr_eq(&value, &b.completed().unwrap()));
    assert!(b.write(3, b"x").is_err());
    assert_eq!(b.status(), Status::Complete);
}

#[test]
fn incomplete_and_invalid_transfers_cannot_publish() {
    for (sequence, bytes) in [(0, b"a".as_slice()), (2, b"a"), (1, b""), (1, b"abc")] {
        let (r, _client) = Receiver::new(config(3, 3));
        assert!(r.write(sequence, bytes).is_err());
        assert!(r.done().is_err());
        assert_eq!(r.staged_bytes(), 0);
        assert!(r.completed().is_none());
    }
    let (r, _client) = Receiver::new(config(3, 3));
    r.write(1, b"x").unwrap();
    assert!(r.done().is_err());
    assert!(r.write(2, b"yz").is_err());
    assert_eq!(r.staged_bytes(), 0);
    let (empty, _client) = Receiver::new(config(0, 2));
    assert_eq!(
        empty.done().unwrap(),
        Summary {
            bytes: 0,
            chunks: 0
        }
    );
    assert_eq!(&*empty.completed().unwrap(), b"");
    let (r, _client) = Receiver::new(config(1, 2));
    assert!(r.write(1, b"ab").is_err());
    assert_eq!(r.staged_bytes(), 0);
    let (r, _client) = Receiver::new(Config::new(3, 2, 2, 2).unwrap());
    r.write(1, b"a").unwrap();
    r.write(2, b"b").unwrap();
    assert!(r.write(3, b"c").is_err());
    assert_eq!(r.staged_bytes(), 0);
}

struct Tasks(Vec<tokio::task::JoinHandle<capnp::Result<()>>>);
impl Drop for Tasks {
    fn drop(&mut self) {
        for t in &self.0 {
            t.abort();
        }
    }
}
fn wire(client: transfer::Client) -> (transfer::Client, Tasks) {
    let (a, b) = tokio::io::duplex(4096);
    let server = reproto::rpc::serve(b, client.client);
    let (client, driver) = reproto::rpc::client(a);
    (client, Tasks(vec![server, driver]))
}
async fn until(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !predicate() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
struct Gate {
    inner: transfer::Client,
    reply: RefCell<Option<oneshot::Receiver<()>>>,
    done: bool,
    wrong_ack: bool,
}
impl Gate {
    async fn wait(&self) {
        let rx = self.reply.borrow_mut().take();
        if let Some(rx) = rx {
            rx.await.unwrap();
        }
    }
}
impl transfer::Server for Gate {
    async fn describe(
        self: Rc<Self>,
        _: transfer::DescribeParams,
        mut out: transfer::DescribeResults,
    ) -> capnp::Result<()> {
        let r = self.inner.describe_request().send().promise.await?;
        out.get().set_config(r.get()?.get_config()?)?;
        Ok(())
    }
    async fn write(
        self: Rc<Self>,
        p: transfer::WriteParams,
        mut out: transfer::WriteResults,
    ) -> capnp::Result<()> {
        let mut request = self.inner.write_request();
        request.set(p.get()?)?;
        drop(p);
        let r = request.send().promise.await?;
        if !self.done {
            self.wait().await;
        }
        out.get().set_sequence(if self.wrong_ack {
            0
        } else {
            r.get()?.get_sequence()
        });
        Ok(())
    }
    async fn done(
        self: Rc<Self>,
        _: transfer::DoneParams,
        mut out: transfer::DoneResults,
    ) -> capnp::Result<()> {
        let r = self.inner.done_request().send().promise.await?;
        if self.done {
            self.wait().await;
        }
        out.get().set_summary(r.get()?.get_summary()?)?;
        Ok(())
    }
    async fn cancel(
        self: Rc<Self>,
        _: transfer::CancelParams,
        mut out: transfer::CancelResults,
    ) -> capnp::Result<()> {
        out.get().set_status(
            self.inner
                .cancel_request()
                .send()
                .promise
                .await?
                .get()?
                .get_status()?,
        );
        Ok(())
    }
}

#[tokio::test(flavor = "current_thread")]
async fn rpc_backpressure_retains_credit_when_a_wait_is_canceled() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (r, inner) = Receiver::new(config(3, 2));
            let (tx, rx) = oneshot::channel();
            let (client, _tasks) = wire(capnp_rpc::new_client(Gate {
                inner,
                reply: RefCell::new(Some(rx)),
                done: false,
                wrong_ack: false,
            }));
            let mut sender = Sender::connect(client).await.unwrap();
            sender.write(b"ab").await.unwrap();
            until(|| r.staged_bytes() == 2).await;
            assert_eq!(sender.in_flight(), 2);
            assert!(
                tokio::time::timeout(Duration::from_millis(5), sender.write(b"c"))
                    .await
                    .is_err()
            );
            assert_eq!(sender.sent_bytes(), 2);
            assert_eq!(sender.in_flight(), 2);
            tx.send(()).unwrap();
            sender.write(b"c").await.unwrap();
            assert_eq!(
                sender.done().await.unwrap(),
                Summary {
                    bytes: 3,
                    chunks: 2
                }
            );
            assert_eq!(sender.in_flight(), 0);
            assert_eq!(&*r.completed().unwrap(), b"abc");
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn delayed_done_receipt_is_retained_and_cancel_cannot_undo_publication() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (r, inner) = Receiver::new(config(1, 2));
            let (tx, rx) = oneshot::channel();
            let (client, _tasks) = wire(capnp_rpc::new_client(Gate {
                inner,
                reply: RefCell::new(Some(rx)),
                done: true,
                wrong_ack: false,
            }));
            let mut sender = Sender::connect(client).await.unwrap();
            sender.write(b"x").await.unwrap();
            sender.flush().await.unwrap();
            assert!(sender.done().now_or_never().is_none());
            until(|| r.status() == Status::Complete).await;
            assert!(
                tokio::time::timeout(Duration::from_millis(5), sender.done())
                    .await
                    .is_err()
            );
            assert_eq!(sender.cancel().await.unwrap(), Status::Complete);
            tx.send(()).unwrap();
            assert_eq!(
                sender.done().await.unwrap(),
                Summary {
                    bytes: 1,
                    chunks: 1
                }
            );
            assert_eq!(&*r.completed().unwrap(), b"x");
            assert!(sender.write(b"x").await.is_err());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn rpc_errors_are_sticky_and_cancellation_releases_staging() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (r, client) = Receiver::new(config(3, 3));
            let (client, _tasks) = wire(client);
            let mut sender = Sender::connect(client.clone()).await.unwrap();
            sender.write(b"a").await.unwrap();
            sender.flush().await.unwrap();
            r.fail(capnp::Error::failed("application rejected transfer".into()));
            sender.write(b"bc").await.unwrap();
            let error = sender.flush().await.unwrap_err();
            assert!(sender.write(b"x").await.is_err());
            assert_eq!(sender.done().await.unwrap_err().extra, error.extra);
            assert_eq!(sender.cancel().await.unwrap(), Status::Canceled);
            assert_eq!(sender.in_flight(), 0);
            assert_eq!(r.staged_bytes(), 0);
            assert!(r.completed().is_none());
            let (r, client) = Receiver::new(config(3, 3));
            let (client, _tasks) = wire(client);
            let mut sender = Sender::connect(client).await.unwrap();
            sender.write(b"a").await.unwrap();
            assert_eq!(sender.cancel().await.unwrap(), Status::Canceled);
            assert_eq!(r.staged_bytes(), 0);
            assert_eq!(sender.in_flight(), 0);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn mismatched_receipt_and_owner_drop_fail_closed() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (r, inner) = Receiver::new(config(2, 2));
            let (client, _tasks) = wire(capnp_rpc::new_client(Gate {
                inner,
                reply: RefCell::new(None),
                done: false,
                wrong_ack: true,
            }));
            let mut sender = Sender::connect(client).await.unwrap();
            sender.write(b"a").await.unwrap();
            assert!(sender
                .flush()
                .await
                .unwrap_err()
                .extra
                .contains("mismatched"));
            assert!(sender.write(b"b").await.is_err());
            assert_eq!(sender.cancel().await.unwrap(), Status::Canceled);
            assert_eq!(r.staged_bytes(), 0);
            let (r, client) = Receiver::new(config(1, 2));
            let (client, _tasks) = wire(client);
            let mut sender = Sender::connect(client).await.unwrap();
            drop(r);
            sender.write(b"x").await.unwrap();
            assert!(sender.done().await.is_err());
            assert_eq!(sender.cancel().await.unwrap(), Status::Canceled);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn disconnect_settles_outstanding_calls_without_claiming_completion() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (r, inner) = Receiver::new(config(3, 3));
            let (_tx, rx) = oneshot::channel();
            let (client, tasks) = wire(capnp_rpc::new_client(Gate {
                inner,
                reply: RefCell::new(Some(rx)),
                done: false,
                wrong_ack: false,
            }));
            let mut sender = Sender::connect(client).await.unwrap();
            sender.write(b"a").await.unwrap();
            sender.write(b"bc").await.unwrap();
            until(|| r.progress().bytes == 3).await;
            tasks.0[0].abort();
            assert!(tokio::time::timeout(Duration::from_secs(2), sender.flush())
                .await
                .unwrap()
                .is_err());
            assert_eq!(sender.in_flight(), 0);
            assert!(sender.done().await.is_err());
            assert!(r.completed().is_none());
            assert_eq!(r.cancel(), Status::Canceled);
            assert_eq!(r.staged_bytes(), 0);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn bulk_over_authenticated_blake3_noise() {
    use reproto::{
        noise_rpc::Network,
        transport::{self, Identity},
    };
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let a = Identity::generate();
                let b = Identity::generate();
                let left = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let right = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let address = right.local_addr().unwrap();
                let (aa, bb) = tokio::join!(
                    transport::connect_authenticated(
                        left,
                        address,
                        &a,
                        b.public_key(),
                        Some([7; 32]),
                        b"bulk"
                    ),
                    transport::accept_authenticated(
                        right,
                        &b,
                        a.public_key(),
                        Some([7; 32]),
                        b"bulk"
                    )
                );
                let (an, ah) = Network::new(a.public_key());
                let (bn, bh) = Network::new(b.public_key());
                ah.attach(aa.unwrap()).unwrap();
                bh.attach(bb.unwrap()).unwrap();
                let (r, service) = Receiver::new(config(3, 3));
                let server = capnp_rpc::RpcSystem::new(Box::new(bn), Some(service.client));
                let mut client = capnp_rpc::RpcSystem::new(Box::new(an), None);
                let transfer = client.bootstrap(b.public_key());
                let _tasks = Tasks(vec![
                    tokio::task::spawn_local(server),
                    tokio::task::spawn_local(client),
                ]);
                let mut sender = Sender::connect(transfer).await.unwrap();
                sender.write(b"a").await.unwrap();
                sender.write(b"bc").await.unwrap();
                assert_eq!(
                    sender.done().await.unwrap(),
                    Summary {
                        bytes: 3,
                        chunks: 2
                    }
                );
                assert_eq!(&*r.completed().unwrap(), b"abc");
            })
            .await
            .unwrap();
        })
        .await;
}

#[derive(serde::Deserialize)]
struct Trace {
    window: u32,
    fail_at: u64,
    steps: Vec<Step>,
}
#[derive(serde::Deserialize)]
struct Step {
    action: String,
    item: u64,
    state: Vec<u64>,
}
#[test]
fn replay_tlc_bulk_traces() {
    let path = reproto_test_support::verification::input("REPROTO_BULK_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for trace in traces {
        let mut credit = CreditWindow::new(trace.window, 2).unwrap();
        let mut reservations = std::collections::BTreeMap::new();
        let (r, _client) = Receiver::new(config(3, trace.window));
        let mut requests = VecDeque::new();
        let mut replies = VecDeque::new();
        let mut results = [0, 0];
        for step in trace.steps {
            match step.action.as_str() {
                "send" => {
                    let reservation = credit.reserve(step.item as u32).unwrap().unwrap();
                    assert_eq!(reservation.sequence(), step.item);
                    reservations.insert(step.item, reservation);
                    requests.push_back((1, step.item));
                }
                "done" => requests.push_back((2, 0)),
                "cancel" => requests.push_back((3, 0)),
                "process_write" => {
                    assert_eq!(requests.pop_front(), Some((1, step.item)));
                    if step.item == trace.fail_at {
                        r.fail(capnp::Error::failed("modeled failure".into()));
                    }
                    let result = if r
                        .write(step.item, &vec![step.item as u8; step.item as usize])
                        .is_ok()
                    {
                        1
                    } else {
                        2
                    };
                    results[step.item as usize - 1] = result;
                    replies.push_back((1, step.item, result));
                }
                "process_done" => {
                    assert_eq!(requests.pop_front(), Some((2, 0)));
                    let result = if r.done().is_ok() { 1 } else { 2 };
                    replies.push_back((2, 0, result));
                }
                "process_cancel" => {
                    assert_eq!(requests.pop_front(), Some((3, 0)));
                    replies.push_back((3, 0, if r.cancel() == Status::Complete { 4 } else { 3 }));
                }
                "ack" => {
                    let at = replies
                        .iter()
                        .position(|&(kind, n, _)| kind == 1 && n == step.item)
                        .unwrap();
                    let (kind, n, result) = replies.remove(at).unwrap();
                    assert_eq!((kind, n), (1, step.item));
                    assert_eq!(result, results[n as usize - 1]);
                    let _ = credit.settle(&reservations[&n]).unwrap();
                }
                "done_reply" => {
                    let at = replies.iter().position(|&(kind, _, _)| kind == 2).unwrap();
                    let (kind, _, result) = replies.remove(at).unwrap();
                    assert_eq!(kind, 2);
                    assert_eq!(result, step.state[13]);
                }
                "cancel_reply" => {
                    let at = replies.iter().position(|&(kind, _, _)| kind == 3).unwrap();
                    let (kind, _, result) = replies.remove(at).unwrap();
                    assert_eq!(kind, 3);
                    assert_eq!(result, step.state[14]);
                }
                "duplicate" => replies.push_front(*replies.front().unwrap()),
                _ => panic!("unknown action {}", step.action),
            }
            let status = match r.status() {
                Status::Receiving => 0,
                Status::Complete => 1,
                Status::Canceled => 2,
                Status::Failed => 3,
            };
            assert_eq!(credit.issued(), step.state[1]);
            assert_eq!(
                credit.issued() - credit.pending_chunks() as u64,
                step.state[2]
            );
            assert_eq!(u64::from(credit.available()), step.state[3]);
            assert_eq!(status, step.state[4]);
            assert_eq!(r.staged_bytes() as u64, step.state[5]);
            assert_eq!(r.completed().map_or(0, |b| b.len()) as u64, step.state[6]);
            assert_eq!(
                r.progress(),
                Summary {
                    bytes: step.state[7],
                    chunks: step.state[8]
                }
            );
            assert_eq!(results, [step.state[9], step.state[10]]);
            assert_eq!(r.failure().is_some(), step.state[11] != 0);
            if let Some(bytes) = r.completed() {
                assert_eq!(&*bytes, &[1, 2, 2]);
            }
        }
    }
}

struct Ticket {
    kind: u64,
    sequence: u64,
    run: Option<oneshot::Sender<()>>,
    reply: oneshot::Sender<()>,
    processed: Rc<Cell<bool>>,
}
struct Controlled {
    inner: transfer::Client,
    calls: Rc<RefCell<VecDeque<Ticket>>>,
}
impl Controlled {
    fn register(
        &self,
        kind: u64,
        sequence: u64,
    ) -> (oneshot::Receiver<()>, oneshot::Receiver<()>, Rc<Cell<bool>>) {
        let (run, running) = oneshot::channel();
        let (reply, replying) = oneshot::channel();
        let processed = Rc::new(Cell::new(false));
        self.calls.borrow_mut().push_back(Ticket {
            kind,
            sequence,
            run: Some(run),
            reply,
            processed: processed.clone(),
        });
        (running, replying, processed)
    }
}
fn lost(_: oneshot::Canceled) -> capnp::Error {
    capnp::Error::disconnected("test controller dropped".into())
}
impl transfer::Server for Controlled {
    async fn describe(
        self: Rc<Self>,
        _: transfer::DescribeParams,
        mut out: transfer::DescribeResults,
    ) -> capnp::Result<()> {
        let r = self.inner.describe_request().send().promise.await?;
        out.get().set_config(r.get()?.get_config()?)?;
        Ok(())
    }
    async fn write(
        self: Rc<Self>,
        p: transfer::WriteParams,
        mut out: transfer::WriteResults,
    ) -> capnp::Result<()> {
        let sequence = p.get()?.get_sequence();
        let mut request = self.inner.write_request();
        request.set(p.get()?)?;
        drop(p);
        let (run, reply, processed) = self.register(1, sequence);
        run.await.map_err(lost)?;
        let result = request.send().promise.await;
        processed.set(true);
        reply.await.map_err(lost)?;
        out.get().set_sequence(result?.get()?.get_sequence());
        Ok(())
    }
    async fn done(
        self: Rc<Self>,
        _: transfer::DoneParams,
        mut out: transfer::DoneResults,
    ) -> capnp::Result<()> {
        let (run, reply, processed) = self.register(2, 0);
        run.await.map_err(lost)?;
        let result = self.inner.done_request().send().promise.await;
        processed.set(true);
        reply.await.map_err(lost)?;
        out.get().set_summary(result?.get()?.get_summary()?)?;
        Ok(())
    }
    async fn cancel(
        self: Rc<Self>,
        _: transfer::CancelParams,
        mut out: transfer::CancelResults,
    ) -> capnp::Result<()> {
        let (run, reply, processed) = self.register(3, 0);
        run.await.map_err(lost)?;
        let result = self.inner.cancel_request().send().promise.await;
        processed.set(true);
        reply.await.map_err(lost)?;
        out.get().set_status(result?.get()?.get_status()?);
        Ok(())
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_bulk_wire_traces() {
    let path = reproto_test_support::verification::input("REPROTO_BULK_WIRE_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for trace in traces {
                let (receiver, inner) = Receiver::new(config(3, trace.window));
                let calls = Rc::new(RefCell::new(VecDeque::new()));
                let (client, _tasks) = wire(capnp_rpc::new_client(Controlled {
                    inner,
                    calls: calls.clone(),
                }));
                let mut sender = Sender::connect(client).await.unwrap();
                let mut replies: VecDeque<Ticket> = VecDeque::new();
                let mut cancel_sent = false;
                let mut done_received = false;
                let mut cancel_completed = false;
                for step in trace.steps {
                    match step.action.as_str() {
                        "send" => sender
                            .write(&vec![step.item as u8; step.item as usize])
                            .await
                            .unwrap(),
                        "done" => assert!(sender.done().now_or_never().is_none()),
                        "cancel" => {
                            cancel_sent = true;
                            assert!(sender.cancel().now_or_never().is_none());
                        }
                        "process_write" | "process_done" | "process_cancel" => {
                            until(|| !calls.borrow().is_empty()).await;
                            let mut ticket = calls.borrow_mut().pop_front().unwrap();
                            let kind = match step.action.as_str() {
                                "process_write" => 1,
                                "process_done" => 2,
                                _ => 3,
                            };
                            assert_eq!((ticket.kind, ticket.sequence), (kind, step.item));
                            if kind == 1 && step.item == trace.fail_at {
                                receiver.fail(capnp::Error::failed("modeled failure".into()));
                            }
                            ticket.run.take().unwrap().send(()).unwrap();
                            until(|| ticket.processed.get()).await;
                            replies.push_back(ticket);
                        }
                        "ack" => {
                            let at = replies
                                .iter()
                                .position(|t| t.kind == 1 && t.sequence == step.item)
                                .unwrap();
                            let ticket = replies.remove(at).unwrap();
                            assert_eq!((ticket.kind, ticket.sequence), (1, step.item));
                            ticket.reply.send(()).unwrap();
                            tokio::time::timeout(Duration::from_secs(2), async {
                                while u64::from(sender.in_flight())
                                    != u64::from(trace.window) - step.state[3]
                                {
                                    let _ = sender.flush().now_or_never();
                                    tokio::task::yield_now().await;
                                }
                            })
                            .await
                            .unwrap();
                        }
                        "done_reply" => {
                            let at = replies.iter().position(|t| t.kind == 2).unwrap();
                            let ticket = replies.remove(at).unwrap();
                            done_received = true;
                            assert_eq!(ticket.kind, 2);
                            ticket.reply.send(()).unwrap();
                            if !cancel_sent || cancel_completed {
                                let result =
                                    tokio::time::timeout(Duration::from_secs(2), sender.done())
                                        .await
                                        .unwrap();
                                assert_eq!(result.is_ok(), step.state[13] == 1);
                            }
                        }
                        "cancel_reply" => {
                            let at = replies.iter().position(|t| t.kind == 3).unwrap();
                            let ticket = replies.remove(at).unwrap();
                            assert_eq!(ticket.kind, 3);
                            ticket.reply.send(()).unwrap();
                            let result =
                                tokio::time::timeout(Duration::from_secs(2), sender.cancel())
                                    .await
                                    .unwrap()
                                    .unwrap();
                            assert_eq!(
                                result,
                                if step.state[14] == 4 {
                                    Status::Complete
                                } else {
                                    Status::Canceled
                                }
                            );
                            cancel_completed = result == Status::Complete;
                            if cancel_completed && done_received {
                                assert_eq!(
                                    sender.done().await.unwrap(),
                                    Summary {
                                        bytes: 3,
                                        chunks: 2
                                    }
                                );
                            }
                        }
                        _ => panic!("unsupported wire event {}", step.action),
                    }
                    assert_eq!(
                        u64::from(sender.in_flight()),
                        u64::from(trace.window) - step.state[3]
                    );
                    assert_eq!(
                        sender.sent_bytes(),
                        match step.state[1] {
                            0 => 0,
                            1 => 1,
                            2 => 3,
                            _ => unreachable!(),
                        }
                    );
                    assert_eq!(receiver.staged_bytes() as u64, step.state[5]);
                    assert_eq!(
                        receiver.completed().map_or(0, |b| b.len()) as u64,
                        step.state[6]
                    );
                    assert_eq!(
                        receiver.progress(),
                        Summary {
                            bytes: step.state[7],
                            chunks: step.state[8]
                        }
                    );
                    let status = match receiver.status() {
                        Status::Receiving => 0,
                        Status::Complete => 1,
                        Status::Canceled => 2,
                        Status::Failed => 3,
                    };
                    assert_eq!(status, step.state[4]);
                }
            }
        })
        .await;
}
