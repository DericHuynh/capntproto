#[allow(dead_code)]
mod support;

use capnp::ErrorKind;
use capnp_rpc::{ConnectionSnapshot, RpcDiagnostics, RpcSystem};
use capntproto_test_support::runtime_test_capnp::harness;
use futures::{Future, FutureExt};
use std::{
    cell::{Cell, RefCell},
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
    time::Duration,
};

struct Service;
impl harness::Server for Service {
    async fn echo(
        self: Rc<Self>,
        p: harness::EchoParams,
        mut r: harness::EchoResults,
    ) -> capnp::Result<()> {
        r.get().set_value(p.get()?.get_value());
        Ok(())
    }
    async fn bounce(
        self: Rc<Self>,
        p: harness::BounceParams,
        mut r: harness::BounceResults,
    ) -> capnp::Result<()> {
        let cap = p.get()?.get_cap()?;
        // A callback must still progress while the caller's budget is full.
        let mut call = cap.echo_request();
        call.get().set_value(17);
        assert_eq!(call.send().promise.await?.get()?.get_value(), 17);
        r.get().set_cap(cap);
        Ok(())
    }
    async fn pending(
        self: Rc<Self>,
        _: harness::PendingParams,
        _: harness::PendingResults,
    ) -> capnp::Result<()> {
        futures::future::pending().await
    }
    async fn stream(self: Rc<Self>, _: harness::StreamParams) -> capnp::Result<()> {
        futures::future::pending().await
    }
}

async fn settle() {
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
}
async fn run(test: impl Future<Output = ()>) {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(5), test)
                .await
                .unwrap();
        })
        .await;
}

struct Fixture {
    client: harness::Client,
    system: Rc<RefCell<RpcSystem<u8>>>,
    diagnostics: RpcDiagnostics,
    tasks: Vec<tokio::task::JoinHandle<capnp::Result<()>>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
impl Fixture {
    async fn new(limit: usize) -> Self {
        let hub = Rc::new(RefCell::new(support::Hub::default()));
        let service: harness::Client = capnp_rpc::new_client(Service);
        let mut server = RpcSystem::new(
            Box::new(support::Hub::network(&hub, 1)),
            Some(service.client),
        );
        server.set_outgoing_call_limit(1);
        let mut system = RpcSystem::new(Box::new(support::Hub::network(&hub, 2)), None);
        system.set_outgoing_call_limit(limit);
        let diagnostics = system.diagnostics();
        let client: harness::Client = system.bootstrap(1);
        let system = Rc::new(RefCell::new(system));
        let driver = system.clone();
        let tasks = vec![
            tokio::task::spawn_local(server),
            tokio::task::spawn_local(futures::future::poll_fn(move |cx| {
                Pin::new(&mut *driver.borrow_mut()).poll(cx)
            })),
        ];
        client.client.when_resolved().await.unwrap();
        settle().await;
        Self {
            client,
            system,
            diagnostics,
            tasks,
        }
    }
    fn snapshot(&self) -> ConnectionSnapshot {
        let snapshots = self.diagnostics.snapshot();
        assert_eq!(snapshots.len(), 1);
        snapshots[0]
    }
    async fn rejected(&self) {
        let error = self
            .client
            .echo_request()
            .send()
            .promise
            .await
            .err()
            .unwrap();
        assert_eq!(error.kind, ErrorKind::Overloaded);
        assert!(error.extra.contains("call was not sent"));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn unsent_requests_reserve_credit_and_limit_updates_preserve_accepted_work() {
    run(async {
        let f = Fixture::new(1).await;
        let before = f.snapshot();
        let request = f.client.echo_request();
        assert_eq!(f.snapshot().outgoing_calls, 1);
        assert_eq!(f.snapshot().questions, before.questions); // no wire state allocated
        f.rejected().await;
        f.system.borrow_mut().set_outgoing_call_limit(0);
        let response = request.send().promise.await.unwrap(); // already accepted
        assert_eq!(f.snapshot().outgoing_calls, 1);
        assert_eq!(f.snapshot().held_responses, 1);
        drop(response);
        settle().await;
        assert_eq!(f.snapshot().outgoing_calls, 0);
        assert_eq!(f.snapshot().held_responses, 0);
        f.rejected().await;
        f.system.borrow_mut().set_outgoing_call_limit(1);
        drop(f.client.echo_request());
        assert_eq!(f.snapshot().outgoing_calls, 0);
        drop(f.client.echo_request().send().promise.await.unwrap());
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn pending_pipeline_retains_credit_after_response_future_is_dropped() {
    run(async {
        let f = Fixture::new(1).await;
        let call = f.client.pending_request().send();
        let child = call.pipeline.get_cap();
        drop(call);
        settle().await;
        assert_eq!(f.snapshot().outgoing_calls, 1);
        f.rejected().await;
        drop(child);
        settle().await; // Finish and canceled Return must progress at full budget.
        assert_eq!(f.snapshot().outgoing_calls, 0);
        drop(f.client.echo_request().send().promise.await.unwrap());
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn callbacks_and_held_response_pipelines_work_at_capacity() {
    run(async {
        let f = Fixture::new(1).await;
        let mut request = f.client.bounce_request();
        let local: harness::Client = capnp_rpc::new_client(Service);
        request.get().set_cap(local);
        let call = request.send();
        let response = call.promise.await.unwrap();
        assert_eq!(f.snapshot().held_responses, 1);
        drop(response);
        assert_eq!(f.snapshot().outgoing_calls, 1); // pipeline retains response context
        f.rejected().await;
        drop(call.pipeline);
        settle().await;
        assert_eq!(f.snapshot().outgoing_calls, 0);
        assert_eq!(f.snapshot().held_responses, 0);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn streaming_calls_share_admission_and_shutdown_does_not_wait_for_credit() {
    run(async {
        let f = Fixture::new(1).await;
        f.client.stream_request().send().await.unwrap(); // window readiness, not completion
        settle().await;
        assert_eq!(f.snapshot().outgoing_calls, 1);
        f.rejected().await;
        let disconnect = f.system.borrow().get_disconnector();
        disconnect.await.unwrap();
        settle().await;
        assert!(f.diagnostics.snapshot().is_empty());
    })
    .await;
}

struct BlockedIo {
    allow: Rc<Cell<bool>>,
    fail: Rc<Cell<bool>>,
    wake: Rc<RefCell<Option<std::task::Waker>>>,
}
impl futures::AsyncRead for BlockedIo {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        Poll::Pending
    }
}
impl futures::AsyncWrite for BlockedIo {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if self.fail.get() {
            Poll::Ready(Err(std::io::ErrorKind::BrokenPipe.into()))
        } else if self.allow.get() {
            Poll::Ready(Ok(data.len()))
        } else {
            *self.wake.borrow_mut() = Some(cx.waker().clone());
            Poll::Pending
        }
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.poll_flush(cx)
    }
}

#[test]
fn pipeline_only_cancellation_keeps_credit_until_blocked_write_finishes() {
    for fail in [false, true] {
        let allow = Rc::new(Cell::new(false));
        let fault = Rc::new(Cell::new(false));
        let wake = Rc::new(RefCell::new(None));
        let mut driver = Box::pin(capnp_rpc::twoparty::TwoPartyClient::new(BlockedIo {
            allow: allow.clone(),
            fail: fault.clone(),
            wake: wake.clone(),
        }));
        driver.set_outgoing_call_limit(1);
        let client: harness::Client = driver.bootstrap();
        let diagnostics = driver.diagnostics();
        let queue = driver.outgoing_queue();
        let pipeline = client.pending_request().send_for_pipeline();
        drop(pipeline);
        let mut cx = Context::from_waker(futures::task::noop_waker_ref());
        for _ in 0..16 {
            assert!(driver.as_mut().poll(&mut cx).is_pending());
        }
        assert!(queue.output_snapshot().unwrap().active.message_count > 0);
        assert_eq!(diagnostics.snapshot()[0].outgoing_calls, 1);
        assert_eq!(
            client
                .echo_request()
                .send()
                .promise
                .now_or_never()
                .unwrap()
                .err()
                .unwrap()
                .kind,
            ErrorKind::Overloaded
        );
        fault.set(fail);
        allow.set(true);
        wake.borrow_mut().take().unwrap().wake();
        for _ in 0..32 {
            if driver.as_mut().poll(&mut cx).is_ready() {
                break;
            }
        }
        if fail {
            assert!(diagnostics.snapshot().is_empty());
        } else {
            assert_eq!(diagnostics.snapshot()[0].outgoing_calls, 0);
            drop(client.pending_request().send_for_pipeline());
        }
        drop(driver);
        assert!(diagnostics.snapshot().is_empty());
        assert_eq!(queue.output_snapshot().unwrap(), Default::default());
        // Neither observation handle keeps the canceled connection usable.
        assert_eq!(
            client
                .echo_request()
                .send()
                .promise
                .now_or_never()
                .unwrap()
                .err()
                .unwrap()
                .kind,
            ErrorKind::Disconnected
        );
    }
}
