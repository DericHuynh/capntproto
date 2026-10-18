//! Matched, validated, sequential loopback round trips. Setup is not timed.
#![forbid(unsafe_code)]
use capntproto::{
    native_rpc::Network,
    transport::{self, Identity},
};
use futures::{SinkExt, StreamExt};
use serde_json::json;
use std::{io::Write, rc::Rc, time::Duration};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

#[allow(dead_code)]
mod echo_capnp {
    include!(concat!(env!("OUT_DIR"), "/echo_capnp.rs"));
}
mod grpc {
    tonic::include_proto!("echo");
}
mod topology;
mod workflows;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const CONTEXT: &[u8] = b"reproto isolated loopback benchmark v1";
// Version 2 matches bulk payload validation, response cleanup and per-call
// deadlines between Rust and C++. Old trials must not enter this comparison.
const MEASUREMENT_VERSION: u32 = 2;

struct Echo;
impl echo_capnp::echo::Server for Echo {
    fn echo(
        self: Rc<Self>,
        context: echo_capnp::echo::EchoContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        context.complete(|params, mut results| {
            results.set_sequence(params.get_sequence());
            results.set_payload(params.get_payload()?);
            Ok(())
        })
    }
}
#[tonic::async_trait]
impl grpc::echo_server::Echo for Echo {
    async fn echo(
        &self,
        request: tonic::Request<grpc::Frame>,
    ) -> std::result::Result<tonic::Response<grpc::Frame>, tonic::Status> {
        Ok(tonic::Response::new(request.into_inner()))
    }
}
fn ready(address: std::net::SocketAddr, public: Option<[u8; 32]>) -> Result<()> {
    println!(
        "{}",
        json!({"address":address.to_string(), "public":public})
    );
    std::io::stdout().flush()?;
    Ok(())
}
async fn serve(protocol: &str) -> Result<()> {
    if matches!(protocol, "native" | "native-tcp") {
        // Only this benchmark's loopback client uses the published fixture key.
        // The server gets a fresh key and the client pins it from the readiness pipe.
        let identity = Identity::generate();
        let client = Identity::from_private_key([1; 32])?;
        let session = if protocol == "native" {
            let socket = UdpSocket::bind("127.0.0.1:0").await?;
            ready(socket.local_addr()?, Some(identity.public_key()))?;
            transport::accept_authenticated(socket, &identity, client.public_key(), None, CONTEXT)
                .await?
        } else {
            let socket = TcpListener::bind("127.0.0.1:0").await?;
            ready(socket.local_addr()?, Some(identity.public_key()))?;
            transport::tcp::accept(
                socket.accept().await?.0,
                &identity,
                client.public_key(),
                None,
                CONTEXT,
                Duration::from_secs(10),
            )
            .await?
        };
        let (network, handle) = Network::new(identity.public_key());
        handle.attach(session)?;
        let bootstrap: echo_capnp::echo::Client = capnp_rpc::new_client(Echo);
        capnp_rpc::RpcSystem::new(Box::new(network), Some(bootstrap.client)).await?;
    } else {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        ready(listener.local_addr()?, None)?;
        match protocol {
            "tcp" => {
                let (socket, _) = listener.accept().await?;
                socket.set_nodelay(true)?;
                let bootstrap: echo_capnp::echo::Client = capnp_rpc::new_client(Echo);
                if tcp_driver()? == "portable" {
                    portable_tcp(
                        socket,
                        Some(bootstrap.client),
                        capnp_rpc::rpc_twoparty_capnp::Side::Server,
                    )?
                    .run()
                    .await?;
                } else {
                    capntproto::rpc::tcp::client(
                        socket,
                        Some(bootstrap.client),
                        capnp_rpc::rpc_twoparty_capnp::Side::Server,
                        Default::default(),
                    )
                    .await?;
                }
            }
            "grpc" => {
                // serve_with_incoming bypasses tonic's TCP listener settings.
                let incoming =
                    tokio_stream::wrappers::TcpListenerStream::new(listener).map(|socket| {
                        socket.and_then(|socket| {
                            socket.set_nodelay(true)?;
                            Ok(socket)
                        })
                    });
                tonic::transport::Server::builder()
                    .add_service(grpc::echo_server::EchoServer::new(Echo))
                    .serve_with_incoming(incoming)
                    .await?
            }
            "websocket" => {
                let (socket, _) = listener.accept().await?;
                socket.set_nodelay(true)?;
                let mut stream = tokio_tungstenite::accept_async(socket).await?;
                while let Some(frame) = stream.next().await {
                    match frame? {
                        frame @ Message::Binary(_) => stream.send(frame).await?,
                        Message::Close(_) => break,
                        _ => return Err("unexpected benchmark WebSocket frame".into()),
                    }
                }
            }
            _ => return Err("unknown benchmark protocol".into()),
        }
    }
    Ok(())
}

enum Client {
    Native(echo_capnp::echo::Client),
    Portable(
        echo_capnp::echo::Client,
        capnp_rpc::portable::RpcSystem<capnp_rpc::rpc_twoparty_capnp::Side>,
    ),
    Grpc(grpc::echo_client::EchoClient<tonic::transport::Channel>),
    WebSocket(Box<WebSocketStream<TcpStream>>),
}
impl Client {
    async fn connect(protocol: &str, address: &str, public: &str) -> Result<Self> {
        match protocol {
            "tcp" => {
                if tcp_driver()? == "portable" {
                    let socket = TcpStream::connect(address).await?;
                    socket.set_nodelay(true)?;
                    let mut driver =
                        portable_tcp(socket, None, capnp_rpc::rpc_twoparty_capnp::Side::Client)?;
                    let client = driver.bootstrap(capnp_rpc::rpc_twoparty_capnp::Side::Server);
                    return Ok(Self::Portable(client, driver));
                }
                let mut driver =
                    capntproto::rpc::connect(address, None, Default::default()).await?;
                let client = driver.bootstrap();
                tokio::task::spawn_local(driver);
                Ok(Self::Native(client))
            }
            "native" | "native-tcp" => {
                let identity = Identity::from_private_key([1; 32])?;
                let public: [u8; 32] = serde_json::from_str(public)?;
                let session = if protocol == "native" {
                    transport::connect_authenticated(
                        UdpSocket::bind("127.0.0.1:0").await?,
                        address.parse()?,
                        &identity,
                        public,
                        None,
                        CONTEXT,
                    )
                    .await?
                } else {
                    transport::tcp::connect(
                        address.parse()?,
                        &identity,
                        public,
                        None,
                        CONTEXT,
                        Duration::from_secs(10),
                    )
                    .await?
                };
                let (network, handle) = Network::new(identity.public_key());
                handle.attach(session)?;
                let mut system = capnp_rpc::RpcSystem::new(Box::new(network), None);
                let client = system.bootstrap(public);
                tokio::task::spawn_local(system);
                Ok(Self::Native(client))
            }
            "grpc" => Ok(Self::Grpc(
                grpc::echo_client::EchoClient::connect(format!("http://{address}")).await?,
            )),
            "websocket" => {
                let socket = TcpStream::connect(address).await?;
                socket.set_nodelay(true)?;
                let (stream, _) =
                    tokio_tungstenite::client_async(format!("ws://{address}/"), socket).await?;
                Ok(Self::WebSocket(Box::new(stream)))
            }
            _ => Err("unknown benchmark protocol".into()),
        }
    }
}

// Select the protocol once, outside the measured loop. Otherwise every TCP
// timeout moves a future large enough to contain tonic's entire gRPC operation.
// Each protocol keeps the same IO, response validation and timing policy.
#[path = "../measurement.rs"]
mod measurement;
use measurement::{measure_calls, Roundtrip};
impl Roundtrip for grpc::echo_client::EchoClient<tonic::transport::Channel> {
    async fn roundtrip(&mut self, sequence: u64, payload: &[u8]) -> Result<()> {
        let response = self
            .echo(grpc::Frame {
                sequence,
                payload: payload.into(),
            })
            .await?
            .into_inner();
        if response.sequence != sequence || response.payload != payload {
            return Err("gRPC response mismatch".into());
        }
        Ok(())
    }
}
impl Roundtrip for WebSocketStream<TcpStream> {
    async fn roundtrip(&mut self, sequence: u64, payload: &[u8]) -> Result<()> {
        let mut frame = sequence.to_le_bytes().to_vec();
        frame.extend_from_slice(payload);
        self.send(Message::Binary(frame.clone().into())).await?;
        let response = self.next().await.ok_or("WebSocket closed")??;
        if response != Message::Binary(frame.into()) {
            return Err("WebSocket response mismatch".into());
        }
        Ok(())
    }
}
async fn measure(args: &[String]) -> Result<()> {
    if args.len() != 6 {
        return Err("measure PROTOCOL ADDRESS PUBLIC_JSON BYTES WARMUP ITERATIONS".into());
    }
    let bytes = args[3].parse::<usize>()?;
    let warmup = args[4].parse::<u64>()?;
    let iterations = args[5].parse::<u64>()?;
    if bytes > 1024 * 1024 || iterations == 0 || iterations > 1_000_000 || warmup > 100_000 {
        return Err("benchmark bounds exceeded".into());
    }
    let payload: Vec<_> = (0..bytes).map(|n| (n % 251) as u8).collect();
    let client = Client::connect(&args[0], &args[1], &args[2]).await?;
    match client {
        Client::Portable(mut client, driver) => {
            driver
                .run_with(measure_calls(
                    &mut client,
                    &args[0],
                    &payload,
                    warmup,
                    iterations,
                ))
                .await?
        }
        Client::Native(mut client) => {
            measure_calls(&mut client, &args[0], &payload, warmup, iterations).await
        }
        Client::Grpc(mut client) => {
            measure_calls(&mut client, &args[0], &payload, warmup, iterations).await
        }
        Client::WebSocket(mut client) => {
            measure_calls(&mut *client, &args[0], &payload, warmup, iterations).await
        }
    }
}
pub fn run(protocol: Option<&str>) -> Result<()> {
    let mut args: Vec<_> = std::env::args()
        .skip(1)
        .filter(|a| a != "--bench")
        .collect();
    if protocol.is_none() && args.as_slice() == ["clock-reads"] {
        println!("{}", driver::clock_reads());
        return Ok(());
    }
    if let Some(protocol) = protocol {
        if args.is_empty() {
            return driver::individual(protocol);
        }
        args.insert(1, protocol.to_owned());
    }
    if args.first().map(String::as_str) == Some("compare") && args.len() == 3 {
        return driver::compare(
            std::path::Path::new(&args[1]),
            std::path::Path::new(&args[2]),
        );
    }
    if args.first().map(String::as_str) == Some("compare-portable") && args.len() == 3 {
        return driver::portable::compare(
            std::path::Path::new(&args[1]),
            std::path::Path::new(&args[2]),
        );
    }
    if matches!(
        args.first().map(String::as_str),
        Some("workflow-host" | "workflow-client")
    ) && (args.len() == 4 || (args.len() == 5 && args[4] == "--smoke"))
    {
        let run = if args[0] == "workflow-host" {
            driver::workflows::host
        } else {
            driver::workflows::client
        };
        return run(
            std::path::Path::new(&args[1]),
            &args[2],
            std::path::Path::new(&args[3]),
            args.len() == 5,
        );
    }
    if args.first().map(String::as_str) == Some("compare-tail") && args.len() == 6 {
        return driver::tail::compare(
            [1, 2, 3, 4].map(|i| std::path::Path::new(&args[i])),
            std::path::Path::new(&args[5]),
        );
    }
    if args.first().map(String::as_str) == Some("compare-tcp") && args.len() == 4 {
        return driver::compare_tcp(
            std::path::Path::new(&args[1]),
            std::path::Path::new(&args[2]),
            std::path::Path::new(&args[3]),
        );
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(tokio::task::LocalSet::new().run_until(async move {
        tokio::task::spawn_local(async move {
            match args.first().map(String::as_str) {
                Some("topology-server") => topology::serve(&args[1..]).await,
                Some("topology-measure") => topology::measure(&args[1..]).await,
                Some("workflow-server") if args.len() == 2 => workflows::serve(&args[1]).await,
                Some("workflow-measure") => workflows::measure(&args[1..]).await,
                Some("comparison-server") if args.len() == 3 => {
                    workflows::comparison::serve(&args[1], &args[2]).await
                }
                Some("comparison-measure") if args.len() == 6 => {
                    workflows::comparison::measure(&args[1], &args[2..]).await
                }
                Some("server") if args.len() == 2 => serve(&args[1]).await,
                Some("measure") => measure(&args[1..]).await,
                _ => Err("expected server or measure".into()),
            }
        })
        .await?
    }))
}

pub mod driver;

pub(crate) fn tcp_driver() -> Result<&'static str> {
    match std::env::var("CAPNTPROTO_BENCH_PORTABLE").as_deref() {
        Ok("1") => Ok("portable"),
        Ok("0") | Err(std::env::VarError::NotPresent) => Ok("legacy"),
        _ => Err("CAPNTPROTO_BENCH_PORTABLE must be 0 or 1".into()),
    }
}
fn portable_tcp(
    socket: TcpStream,
    bootstrap: Option<capnp::capability::Client>,
    side: capnp_rpc::rpc_twoparty_capnp::Side,
) -> Result<capnp_rpc::portable::RpcSystem<capnp_rpc::rpc_twoparty_capnp::Side>> {
    portable_tcp_bounded(socket, bootstrap, side, 512 * 1024, 8 * 1024 * 1024)
}
fn portable_tcp_bounded(
    socket: TcpStream,
    bootstrap: Option<capnp::capability::Client>,
    side: capnp_rpc::rpc_twoparty_capnp::Side,
    frame_bytes: usize,
    buffer_bytes: usize,
) -> Result<capnp_rpc::portable::RpcSystem<capnp_rpc::rpc_twoparty_capnp::Side>> {
    let options = capnp_rpc::portable::Options::bounded(
        1.try_into()?,
        256.try_into()?,
        frame_bytes.try_into()?,
        buffer_bytes.try_into()?,
    )?;
    Ok(capnp_rpc::portable::RpcSystem::new(
        Box::new(capntproto::rpc::tcp::network(
            socket,
            side,
            Default::default(),
        )),
        bootstrap,
        options,
    )?)
}
