//! Behavioral contract compiled separately against our and upstream Rust crates.
//! Each matrix case runs over TCP in two independent OS processes.
use crate::interop_capnp::peer;
use capnp_rpc::rpc_twoparty_capnp::Side;
use futures::{channel::oneshot, AsyncReadExt, Future};
use std::{
    cell::{Cell, RefCell},
    io::Write,
    pin::Pin,
    rc::Rc,
    time::Duration,
};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::compat::TokioAsyncReadCompatExt;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub type Disconnect = Pin<Box<dyn Future<Output = capnp::Result<()>>>>;
pub struct Connection {
    pub client: Option<peer::Client>,
    pub disconnect: Disconnect,
    pub task: tokio::task::JoinHandle<capnp::Result<()>>,
}
#[derive(Default)]
struct State {
    started: Cell<u32>,
    canceled: Cell<u32>,
    released: Cell<u32>,
    streamed: Cell<u64>,
    ordered: Cell<u32>,
    pending: RefCell<Option<oneshot::Sender<capnp::Result<peer::Client>>>>,
}
struct Server {
    state: Rc<State>,
    child: bool,
}
impl Drop for Server {
    fn drop(&mut self) {
        if self.child {
            self.state.released.set(self.state.released.get() + 1);
        }
    }
}
struct Held(Rc<State>);
impl Drop for Held {
    fn drop(&mut self) {
        self.0.canceled.set(self.0.canceled.get() + 1);
    }
}
impl peer::Server for Server {
    fn echo(
        self: Rc<Self>,
        context: peer::EchoContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (params, mut results) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            let p = params.get()?;
            let mut r = results.get();
            r.set_value(p.get_value());
            r.set_payload(p.get_payload()?);
            r.set_text(p.get_text()?);
            Ok(())
        })
    }
    fn bounce(
        self: Rc<Self>,
        context: peer::BounceContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (params, mut results) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            results.get().set_cap(params.get()?.get_cap()?);
            Ok(())
        })
    }
    fn child(
        self: Rc<Self>,
        context: peer::ChildContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (_, mut results) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            results.get().set_cap(capnp_rpc::new_client(Server {
                state: self.state.clone(),
                child: true,
            }));
            Ok(())
        })
    }
    fn tail(
        self: Rc<Self>,
        context: peer::TailContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (params, results) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            let p = params.get()?;
            let mut call = p.get_cap()?.echo_request();
            call.get().set_value(p.get_value());
            crate::tail(results, call).await
        })
    }
    fn callback(
        self: Rc<Self>,
        context: peer::CallbackContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (params, mut results) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            let p = params.get()?;
            let mut call = p.get_cap()?.tail_request();
            call.get()
                .set_cap(capnp_rpc::new_client_from_rc(self.clone()));
            call.get().set_value(p.get_value());
            let response = call.send().promise.await?;
            results.get().set_value(response.get()?.get_value());
            Ok(())
        })
    }
    fn fail(
        self: Rc<Self>,
        context: peer::FailContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (params, _) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            let mut error = capnp::Error::failed("interop failure".into());
            error.kind = error_kind(params.get()?.get_kind());
            Err(error)
        })
    }
    fn hold(
        self: Rc<Self>,
        context: peer::HoldContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (_, _) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            self.state.started.set(self.state.started.get() + 1);
            let _held = Held(self.state.clone());
            futures::future::pending().await
        })
    }
    fn stats(
        self: Rc<Self>,
        context: peer::StatsContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (_, mut results) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            let mut r = results.get();
            r.set_started(self.state.started.get());
            r.set_canceled(self.state.canceled.get());
            r.set_released(self.state.released.get());
            r.set_streamed(self.state.streamed.get());
            Ok(())
        })
    }
    fn promise(
        self: Rc<Self>,
        context: peer::PromiseContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (_, mut results) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            let (tx, rx) = oneshot::channel();
            assert!(self.state.pending.borrow_mut().replace(tx).is_none());
            results
                .get()
                .set_cap(capnp_rpc::new_future_client(async { rx.await.unwrap() }));
            Ok(())
        })
    }
    fn resolve(
        self: Rc<Self>,
        context: peer::ResolveContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (params, _) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            let result = if params.get()?.get_reject() {
                Err(capnp::Error::failed("promise rejected".into()))
            } else {
                Ok(capnp_rpc::new_client_from_rc(self.clone()))
            };
            self.state
                .pending
                .borrow_mut()
                .take()
                .unwrap()
                .send(result)
                .map_err(|_| capnp::Error::failed("promise abandoned".into()))
        })
    }
    fn stream(
        self: Rc<Self>,
        context: peer::StreamContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (params, _) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            self.state
                .streamed
                .set(self.state.streamed.get() + params.get()?.get_value());
            Ok(())
        })
    }
    fn order(
        self: Rc<Self>,
        context: peer::OrderContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (params, _) = context.into_owned();
        capnp::capability::Promise::from_future({
            // Measure dispatch order, not the executor's polling order of two
            // independent async method bodies after delivery.
            let result = params.get().map(|params| {
                assert_eq!(
                    params.get_value(),
                    self.state.ordered.get(),
                    "E-order lost during reflection"
                );
                self.state.ordered.set(self.state.ordered.get() + 1);
            });
            futures::future::ready(result)
        })
    }
}
fn error_kind(kind: u16) -> capnp::ErrorKind {
    match kind {
        0 => capnp::ErrorKind::Failed,
        1 => capnp::ErrorKind::Overloaded,
        2 => capnp::ErrorKind::Disconnected,
        3 => capnp::ErrorKind::Unimplemented,
        _ => panic!("error kind"),
    }
}
fn connect(
    socket: TcpStream,
    bootstrap: Option<capnp::capability::Client>,
    side: Side,
    driver: &str,
) -> Connection {
    socket.set_nodelay(true).unwrap();
    let (r, w) = socket.compat().split();
    crate::start(
        capnp_rpc::twoparty::VatNetwork::new(
            futures::io::BufReader::new(r),
            futures::io::BufWriter::new(w),
            side,
            Default::default(),
        ),
        bootstrap,
        driver,
    )
}
fn pass(case: &str) {
    println!("PASS {case}");
}
async fn run_client(remote: peer::Client) -> Result<()> {
    // Immediately call an unresolved bootstrap; don't accidentally test only hosted targets.
    for bytes in [0, 1, 7, 8, 1024, 65536, 262144, 1048576, 4194304] {
        let data: Vec<_> = (0..bytes).map(|n| (n % 251) as u8).collect();
        let mut call = remote.echo_request();
        call.get().set_value(u64::MAX - bytes as u64);
        call.get().set_payload(&data);
        call.get().set_text("Grüße 🌍\0end");
        let response = call.send().promise.await?;
        let r = response.get()?;
        assert_eq!(r.get_value(), u64::MAX - bytes as u64);
        assert_eq!(r.get_payload()?, data);
        assert_eq!(r.get_text()?.to_str()?, "Grüße 🌍\0end");
    }
    pass("bootstrap-payloads");
    let state = Rc::new(State::default());
    let local: peer::Client = capnp_rpc::new_client(Server {
        state: state.clone(),
        child: false,
    });
    let mut bounce = remote.bounce_request();
    bounce.get().set_cap(local.clone());
    let response = bounce.send();
    let pipelined = response.pipeline.get_cap();
    let mut call = pipelined.order_request();
    call.get().set_value(0);
    // Upstream Rust local calls are lazy futures. Drive the earlier invocation
    // concurrently instead of awaiting only the later invocation's future.
    let first = tokio::task::spawn_local(call.send().promise);
    let returned = response.promise.await?.get()?.get_cap()?;
    // E-order is guaranteed on the same reference across resolution, not
    // between two independently obtained aliases (rpc.capnp Disembargo).
    let mut call = pipelined.order_request();
    call.get().set_value(1);
    call.send().promise.await?;
    first.await??;
    let returned = capnp::capability::get_resolved_cap(returned).await;
    returned.echo_request().send().promise.await?;
    assert_eq!(state.ordered.get(), 2);
    pass("pipeline-reflection-embargo");
    let mut call = remote.tail_request();
    call.get().set_cap(local.clone());
    call.get().set_value(42);
    assert_eq!(call.send().promise.await?.get()?.get_value(), 42);
    pass("tail-to-callback");
    let mut call = remote.callback_request();
    call.get().set_cap(local);
    call.get().set_value(43);
    assert_eq!(call.send().promise.await?.get()?.get_value(), 43);
    pass("bidirectional-tail-calls");
    for reject in [false, true] {
        let promise = remote
            .promise_request()
            .send()
            .promise
            .await?
            .get()?
            .get_cap()?;
        let mut call = promise.echo_request();
        call.get().set_value(44);
        let pending = call.send().promise;
        let mut resolve = remote.resolve_request();
        resolve.get().set_reject(reject);
        resolve.send().promise.await?;
        if reject {
            let error = pending.await.err().expect("rejected promise");
            assert_eq!(error.kind, capnp::ErrorKind::Failed);
            assert!(error.extra.contains("promise rejected"));
            assert!(promise.echo_request().send().promise.await.is_err());
        } else {
            assert_eq!(pending.await?.get()?.get_value(), 44);
        }
    }
    pass("promise-resolution-rejection");
    for kind in 0..4 {
        let mut call = remote.fail_request();
        call.get().set_kind(kind);
        let error = call.send().promise.await.err().expect("remote exception");
        assert_eq!(error.kind, error_kind(kind));
        assert!(error.extra.contains("interop failure"));
    }
    for (interface, method) in [
        (
            <peer::Client as capnp::traits::HasTypeId>::TYPE_ID,
            u16::MAX,
        ),
        (0xeabc_ffff_ffff_ffff, 0),
    ] {
        let request = remote
            .client
            .new_call::<capnp::any_pointer::Owned, capnp::any_pointer::Owned>(
                interface, method, None,
            );
        assert_eq!(
            request
                .send()
                .promise
                .await
                .err()
                .expect("unknown method")
                .kind,
            capnp::ErrorKind::Unimplemented
        );
    }
    pass("exceptions-unknown-methods");
    let held = remote.hold_request().send();
    loop {
        if remote
            .stats_request()
            .send()
            .promise
            .await?
            .get()?
            .get_started()
            == 1
        {
            break;
        }
    }
    drop(held);
    loop {
        if remote
            .stats_request()
            .send()
            .promise
            .await?
            .get()?
            .get_canceled()
            == 1
        {
            break;
        }
    }
    pass("finish-cancellation");
    {
        let child = remote
            .child_request()
            .send()
            .promise
            .await?
            .get()?
            .get_cap()?;
        child.echo_request().send().promise.await?;
    }
    loop {
        if remote
            .stats_request()
            .send()
            .promise
            .await?
            .get()?
            .get_released()
            == 1
        {
            break;
        }
    }
    pass("capability-release");
    for value in 1..=32 {
        let mut call = remote.stream_request();
        call.get().set_value(value);
        call.send().await?;
    }
    assert_eq!(
        remote
            .stats_request()
            .send()
            .promise
            .await?
            .get()?
            .get_streamed(),
        528
    );
    pass("streaming");
    remote.echo_request().send().promise.await?;
    pass("connection-survives-errors");
    Ok(())
}
pub fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let driver = args.first().ok_or("driver")?.clone();
    let mode = args.get(1).ok_or("mode")?.clone();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(tokio::task::LocalSet::new().run_until(async move {
            tokio::time::timeout(Duration::from_secs(45), async {
                if mode == "server" {
                    let listener = TcpListener::bind("127.0.0.1:0").await?;
                    println!("{}", listener.local_addr()?.port());
                    std::io::stdout().flush()?;
                    let (socket, _) = listener.accept().await?;
                    let server: peer::Client = capnp_rpc::new_client(Server {
                        state: Rc::default(),
                        child: false,
                    });
                    let connection = connect(socket, Some(server.client), Side::Server, &driver);
                    // EOF/Abort is normal when the independent client disconnects.
                    let _ = connection.task.await?;
                } else {
                    assert_eq!(mode, "client");
                    let socket = TcpStream::connect(args.get(2).ok_or("address")?).await?;
                    let connection = connect(socket, None, Side::Client, &driver);
                    run_client(connection.client.expect("client bootstrap")).await?;
                    connection.disconnect.await?;
                    connection.task.await??;
                    pass("graceful-shutdown");
                }
                Ok(())
            })
            .await?
        }))
}
