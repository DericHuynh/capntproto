//! Matched application workflows. Setup is excluded; validation and response
//! destruction are included. A sample is an entire workflow (a batch for fanout).
use crate::{
    echo_capnp::{callback, cell, workflows},
    Result,
};
use futures::future::try_join_all;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};
use tokio::net::{TcpListener, TcpStream};

mod application;
pub mod comparison;
mod grpc;
#[path = "../workflow-support.rs"]
pub mod support;
mod websocket;
pub use support::VERSION;
// Includes protocol metadata and segment/frame headers around the 4 MiB payload.
const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

fn portable_tcp(
    socket: TcpStream,
    bootstrap: Option<capnp::capability::Client>,
    side: capnp_rpc::rpc_twoparty_capnp::Side,
) -> Result<capnp_rpc::portable::RpcSystem<capnp_rpc::rpc_twoparty_capnp::Side>> {
    crate::portable_tcp_bounded(socket, bootstrap, side, MAX_MESSAGE_BYTES, 32 * 1024 * 1024)
}
pub fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!("../workflows.json")).expect("workflow catalog")
}
struct Service;
struct Cell(RefCell<(u64, Vec<u8>)>);
struct Sink {
    payload: Vec<u8>,
    calls: Rc<std::cell::Cell<u64>>,
}
impl workflows::Server for Service {
    async fn echo(
        self: Rc<Self>,
        params: workflows::EchoParams,
        mut results: workflows::EchoResults,
    ) -> capnp::Result<()> {
        let params = params.get()?;
        let mut results = results.get();
        results.set_sequence(params.get_sequence());
        results.set_payload(params.get_payload()?);
        Ok(())
    }
    async fn open(
        self: Rc<Self>,
        params: workflows::OpenParams,
        mut results: workflows::OpenResults,
    ) -> capnp::Result<()> {
        let client: cell::Client = capnp_rpc::new_client(Cell(RefCell::new((
            0,
            params.get()?.get_payload()?.to_vec(),
        ))));
        results.get().set_cell(client);
        Ok(())
    }
    async fn callback(
        self: Rc<Self>,
        params: workflows::CallbackParams,
        mut results: workflows::CallbackResults,
    ) -> capnp::Result<()> {
        let params = params.get()?;
        let mut request = params.get_sink()?.notify_request();
        request.get().set_sequence(params.get_sequence());
        request.get().set_payload(params.get_payload()?);
        let response = request.send().promise.await?;
        results.get().set_sequence(response.get()?.get_sequence());
        Ok(())
    }
}
impl cell::Server for Cell {
    async fn read(
        self: Rc<Self>,
        _: cell::ReadParams,
        mut results: cell::ReadResults,
    ) -> capnp::Result<()> {
        let state = self.0.borrow();
        let mut results = results.get();
        results.set_version(state.0);
        results.set_payload(&state.1);
        Ok(())
    }
    async fn write(
        self: Rc<Self>,
        params: cell::WriteParams,
        mut results: cell::WriteResults,
    ) -> capnp::Result<()> {
        let params = params.get()?;
        let mut state = self.0.borrow_mut();
        if params.get_expected_version() != state.0 {
            return Err(capnp::Error::failed("version mismatch".into()));
        }
        state.0 = state
            .0
            .checked_add(1)
            .ok_or_else(|| capnp::Error::failed("version overflow".into()))?;
        state.1 = params.get_payload()?.to_vec();
        results.get().set_version(state.0);
        Ok(())
    }
}
impl callback::Server for Sink {
    async fn notify(
        self: Rc<Self>,
        params: callback::NotifyParams,
        mut results: callback::NotifyResults,
    ) -> capnp::Result<()> {
        let params = params.get()?;
        if params.get_payload()? != self.payload || params.get_sequence() != self.calls.get() {
            return Err(capnp::Error::failed("callback payload mismatch".into()));
        }
        self.calls.set(self.calls.get() + 1);
        results.get().set_sequence(params.get_sequence());
        Ok(())
    }
}

pub async fn serve(address: &str) -> Result<()> {
    let listener = TcpListener::bind(address).await?;
    crate::ready(listener.local_addr()?, None)?;
    loop {
        let (socket, _) = listener.accept().await?;
        socket.set_nodelay(true)?;
        let bootstrap: workflows::Client = capnp_rpc::new_client(Service);
        tokio::task::spawn_local(async move {
            let result = if crate::tcp_driver()? == "portable" {
                portable_tcp(
                    socket,
                    Some(bootstrap.client),
                    capnp_rpc::rpc_twoparty_capnp::Side::Server,
                )?
                .await
            } else {
                capntproto::rpc::tcp::client(
                    socket,
                    Some(bootstrap.client),
                    capnp_rpc::rpc_twoparty_capnp::Side::Server,
                    Default::default(),
                )
                .await
            };
            if let Err(error) = result {
                eprintln!("workflow connection closed: {error}");
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        });
    }
}
async fn connect(address: &str) -> Result<workflows::Client> {
    if crate::tcp_driver()? == "portable" {
        let socket = TcpStream::connect(address).await?;
        socket.set_nodelay(true)?;
        let mut driver = portable_tcp(socket, None, capnp_rpc::rpc_twoparty_capnp::Side::Client)?;
        let client = driver.bootstrap(capnp_rpc::rpc_twoparty_capnp::Side::Server);
        tokio::task::spawn_local(driver);
        Ok(client)
    } else {
        let mut driver = capntproto::rpc::connect(address, None, Default::default()).await?;
        let client = driver.bootstrap();
        tokio::task::spawn_local(driver);
        Ok(client)
    }
}
async fn echo(client: &workflows::Client, sequence: u64, payload: &[u8]) -> Result<()> {
    let mut request = client.echo_request();
    request.get().set_sequence(sequence);
    request.get().set_payload(payload);
    let response = request.send().promise.await?;
    let response = response.get()?;
    if response.get_sequence() != sequence || response.get_payload()? != payload {
        return Err("echo mismatch".into());
    }
    Ok(())
}
async fn read(client: &cell::Client, version: u64, payload: &[u8]) -> Result<()> {
    let response = client.read_request().send().promise.await?;
    let response = response.get()?;
    if response.get_version() != version || response.get_payload()? != payload {
        return Err("read mismatch".into());
    }
    Ok(())
}
async fn operation(
    client: &workflows::Client,
    state: &cell::Client,
    sink: &callback::Client,
    callbacks: &std::cell::Cell<u64>,
    case: &str,
    sequence: u64,
    payload: &[u8],
) -> Result<()> {
    match case {
        "fanout-32" => {
            try_join_all((0..32).map(|i| echo(client, sequence * 32 + i, payload))).await?;
        }
        "dependent-read" | "pipelined-read" => {
            let mut request = client.open_request();
            request.get().set_payload(payload);
            let opened = request.send();
            if case == "pipelined-read" {
                // Send the dependent call before awaiting the capability Return.
                let cell = opened.pipeline.get_cell();
                read(&cell, 0, payload).await?;
                opened.promise.await?.get()?.get_cell()?;
            } else {
                let response = opened.promise.await?;
                read(&response.get()?.get_cell()?, 0, payload).await?;
            }
        }
        "write-read" => {
            let mut request = state.write_request();
            request.get().set_expected_version(sequence);
            request.get().set_payload(payload);
            let response = request.send().promise.await?;
            if response.get()?.get_version() != sequence + 1 {
                return Err("write mismatch".into());
            }
            drop(response);
            read(state, sequence + 1, payload).await?;
        }
        "callback" => {
            let mut request = client.callback_request();
            request.get().set_sink(sink.clone());
            request.get().set_sequence(sequence);
            request.get().set_payload(payload);
            let response = request.send().promise.await?;
            if response.get()?.get_sequence() != sequence || callbacks.get() != sequence + 1 {
                return Err("callback mismatch".into());
            }
        }
        _ => echo(client, sequence, payload).await?,
    }
    Ok(())
}
pub async fn measure(args: &[String]) -> Result<()> {
    if args.len() != 4 {
        return Err("workflow-measure ADDRESS CASE WARMUP ITERATIONS".into());
    }
    let case = cases()
        .into_iter()
        .find(|v| v["name"] == args[1])
        .ok_or("unknown workflow")?;
    let warmup: u64 = args[2].parse()?;
    let iterations: u64 = args[3].parse()?;
    if warmup > 100_000 || iterations == 0 || iterations > 100_000 {
        return Err("workflow budget exceeded".into());
    }
    let payload: Vec<_> = (0..case["payload_bytes"].as_u64().unwrap())
        .map(|i| (i % 251) as u8)
        .collect();
    let client = tokio::time::timeout(Duration::from_secs(10), connect(&args[0])).await??;
    // Resolve bootstrap, construct reusable state, and validate connectivity outside timing.
    let mut request = client.open_request();
    request.get().set_payload(&payload);
    let response = tokio::time::timeout(Duration::from_secs(10), request.send().promise).await??;
    let state = response.get()?.get_cell()?;
    drop(response);
    let callbacks = Rc::new(std::cell::Cell::new(0));
    let sink: callback::Client = capnp_rpc::new_client(Sink {
        payload: payload.clone(),
        calls: callbacks.clone(),
    });
    for sequence in 0..warmup {
        tokio::time::timeout(
            Duration::from_secs(10),
            operation(
                &client, &state, &sink, &callbacks, &args[1], sequence, &payload,
            ),
        )
        .await??;
    }
    let mut samples = Vec::with_capacity(iterations as usize);
    let start = Instant::now();
    for sequence in warmup..warmup + iterations {
        let before = Instant::now();
        tokio::time::timeout(
            Duration::from_secs(10),
            operation(
                &client, &state, &sink, &callbacks, &args[1], sequence, &payload,
            ),
        )
        .await??;
        samples.push(u64::try_from(before.elapsed().as_nanos())?);
    }
    println!(
        "{}",
        json!({"workflow_version": VERSION, "implementation": crate::tcp_driver()?, "case": case, "warmup": warmup, "iterations": iterations, "elapsed_ns": u64::try_from(start.elapsed().as_nanos())?, "latency_ns": samples})
    );
    Ok(())
}
