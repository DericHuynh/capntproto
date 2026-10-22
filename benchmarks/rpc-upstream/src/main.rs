//! Published upstream RPC baseline. No capntproto implementation is linked here.
#![forbid(unsafe_code)]
use capnp_rpc::{rpc_twoparty_capnp::Side, RpcSystem};
use futures::AsyncReadExt;
use std::{io::Write, rc::Rc};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::compat::TokioAsyncReadCompatExt;

mod echo_capnp {
    include!(concat!(env!("OUT_DIR"), "/echo_capnp.rs"));
}
#[path = "../../rpc/tls.rs"]
mod benchmark_tls;
#[path = "../../rpc/measurement.rs"]
mod measurement;
mod workflows;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const MEASUREMENT_VERSION: u32 = 2;

fn rpc(
    socket: tokio_rustls::TlsStream<TcpStream>,
    bootstrap: Option<capnp::capability::Client>,
    side: Side,
) -> RpcSystem<Side> {
    // Use the buffered futures IO arrangement from upstream's calculator example.
    // TLS authenticates the peer before constructing the upstream RPC driver.
    let (reader, writer) = socket.compat().split();
    let network = capnp_rpc::twoparty::VatNetwork::new(
        futures::io::BufReader::new(reader),
        futures::io::BufWriter::new(writer),
        side,
        Default::default(),
    );
    RpcSystem::new(Box::new(network), bootstrap)
}

fn ready(address: std::net::SocketAddr) -> Result<()> {
    println!(
        "{}",
        serde_json::json!({"address":address.to_string(), "public":null,"security":benchmark_tls::metadata()})
    );
    std::io::stdout().flush()?;
    Ok(())
}

struct Echo;
impl echo_capnp::echo::Server for Echo {
    async fn echo(
        self: Rc<Self>,
        params: echo_capnp::echo::EchoParams,
        mut results: echo_capnp::echo::EchoResults,
    ) -> capnp::Result<()> {
        let params = params.get()?;
        let mut results = results.get();
        results.set_sequence(params.get_sequence());
        results.set_payload(params.get_payload()?);
        Ok(())
    }
}

async fn serve() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    ready(listener.local_addr()?)?;
    let (socket, _) = listener.accept().await?;
    let socket = benchmark_tls::accept(socket, benchmark_tls::server_config()?).await?;
    let bootstrap: echo_capnp::echo::Client = capnp_rpc::new_client(Echo);
    rpc(socket, Some(bootstrap.client), Side::Server).await?;
    Ok(())
}

async fn measure(args: &[String]) -> Result<()> {
    if args.len() != 5 {
        return Err("measure ADDRESS PUBLIC_JSON BYTES WARMUP ITERATIONS".into());
    }
    if !serde_json::from_str::<serde_json::Value>(&args[1])?.is_null() {
        return Err("upstream benchmark uses certificate-authenticated TLS".into());
    }
    let bytes: usize = args[2].parse()?;
    let warmup: u64 = args[3].parse()?;
    let iterations: u64 = args[4].parse()?;
    if bytes > 1024 * 1024 || iterations == 0 || iterations > 1_000_000 || warmup > 100_000 {
        return Err("benchmark bounds exceeded".into());
    }
    let payload: Vec<_> = (0..bytes).map(|n| (n % 251) as u8).collect();
    let socket = benchmark_tls::connect(&args[0]).await?;
    let mut system = rpc(socket, None, Side::Client);
    let mut client: echo_capnp::echo::Client = system.bootstrap(Side::Server);
    tokio::task::spawn_local(system);
    measurement::measure_calls(&mut client, "capnp-rust", &payload, warmup, iterations).await
}

fn transport_metadata(_: &str) -> Result<serde_json::Value> {
    Ok(benchmark_tls::metadata())
}
fn main() -> Result<()> {
    if std::env::var("CAPNTPROTO_BENCH_TRANSPORT").is_ok_and(|v| v != "tls") {
        return Err("upstream fixture supports TLS only".into());
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(tokio::task::LocalSet::new().run_until(async move {
            tokio::task::spawn_local(async move {
                match args.first().map(String::as_str) {
                    Some("server") if args.len() == 1 => serve().await,
                    Some("measure") => measure(&args[1..]).await,
                    Some("workflow-server") if args.len() == 2 => workflows::serve(&args[1]).await,
                    Some("workflow-measure") => workflows::measure(&args[1..]).await,
                    _ => {
                        Err("expected server, measure, workflow-server or workflow-measure".into())
                    }
                }
            })
            .await?
        }))
}
