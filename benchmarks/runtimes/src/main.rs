mod measure;
mod transports;
#[allow(dead_code)]
mod service_capnp {
    include!(concat!(env!("OUT_DIR"), "/service_capnp.rs"));
}
use capnp_rpc::{rpc_twoparty_capnp::Side, twoparty::TwoPartyClient};
use capntproto_transport::rustls;
use futures::{AsyncRead, AsyncWrite};
use service_capnp::service;
use std::{
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[global_allocator]
static ALLOCATOR: measure::Allocator = measure::Allocator;

#[derive(Clone)]
pub struct Args {
    case: String,
    iterations: usize,
    warmup: usize,
    certificates: PathBuf,
}
impl Args {
    fn parse() -> Result<Self> {
        let mut args = Self {
            case: "unary".into(),
            iterations: 32,
            warmup: 4,
            certificates: PathBuf::from("certificates"),
        };
        let mut input = std::env::args().skip(1);
        while let Some(key) = input.next() {
            let value = input.next().ok_or("every option requires a value")?;
            match key.as_str() {
                "--case" => args.case = value,

                "--iterations" => args.iterations = value.parse()?,
                "--warmup" => args.warmup = value.parse()?,
                "--certificates" => args.certificates = value.into(),
                "--init-certificates" => {
                    certificates(Path::new(&value))?;
                    std::process::exit(0);
                }
                _ => return Err(format!("unknown option {key}").into()),
            }
        }
        if args.iterations == 0 {
            return Err("iterations must be positive".into());
        }
        Ok(args)
    }

    fn root_certificate(&self) -> std::io::Result<Vec<u8>> {
        let ca = self.certificates.join("ca.der");
        std::fs::read(if ca.is_file() {
            ca
        } else {
            self.certificates.join("server.der")
        })
    }
    fn client_config(&self) -> Result<Arc<rustls::ClientConfig>> {
        let mut roots = rustls::RootCertStore::empty();
        roots.add(self.root_certificate()?.into())?;
        let mut config =
            rustls::client_config(roots, None, Arc::new(capntproto_transport::SystemClock))?;
        Arc::get_mut(&mut config).unwrap().resumption =
            rustls::implementation::client::Resumption::disabled();
        Ok(config)
    }
    fn server_config(&self) -> Result<Arc<rustls::ServerConfig>> {
        let identity = rustls::Identity {
            certificates: vec![std::fs::read(self.certificates.join("server.der"))?.into()],
            private_key: rustls::pki_types::PrivateKeyDer::try_from(std::fs::read(
                self.certificates.join("server-key.der"),
            )?)
            .map_err(std::io::Error::other)?,
        };
        let mut config =
            rustls::server_config(identity, None, Arc::new(capntproto_transport::SystemClock))?;
        Arc::get_mut(&mut config).unwrap().send_tls13_tickets = 0;
        Ok(config)
    }
}
fn certificates(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    let pair = rcgen::generate_simple_self_signed(vec![
        "localhost".into(),
        "127.0.0.1".into(),
        "::1".into(),
    ])?;
    std::fs::write(path.join("server.der"), pair.cert.der())?;
    std::fs::write(
        path.join("server-key.der"),
        pair.signing_key.serialize_der(),
    )?;
    Ok(())
}
struct Service;
impl service::Server for Service {
    fn read(
        self: Rc<Self>,
        context: service::ReadContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        context.complete(|params, mut results| {
            results.set_value(params.get_value());
            let bytes = params.get_bytes() as usize;
            if bytes == 0 {
                results.set_payload(&[]);
            } else {
                results.set_payload(&vec![0x5a; bytes]);
            }
            Ok(())
        })
    }
    fn child(
        self: Rc<Self>,
        context: service::ChildContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        context.complete(|_, mut results| {
            results.set_service(capnp_rpc::new_client::<service::Client, _>(Service));
            Ok(())
        })
    }
    fn callback(
        self: Rc<Self>,
        context: service::CallbackContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let (params, mut results) = context.into_owned();
        capnp::capability::Promise::from_future(async move {
            let client = params.get()?.get_service()?;
            let value = read(&client, 37, 0).await?;
            results.get().set_value(value);
            Ok(())
        })
    }
}
async fn read(client: &service::Client, value: u64, bytes: u32) -> capnp::Result<u64> {
    let mut request = client.read_request();
    request.get().set_value(value);
    request.get().set_bytes(bytes);
    let response = request.send().promise.await?;
    let response = response.get()?;
    assert_eq!(response.get_value(), value);
    let payload = response.get_payload()?;
    assert_eq!(payload.len(), bytes as usize);
    assert!(payload.iter().all(|&b| b == 0x5a));
    Ok(response.get_value())
}
async fn dependent(mut client: service::Client, depth: usize) -> capnp::Result<()> {
    for _ in 0..depth {
        client = client
            .child_request()
            .send()
            .promise
            .await?
            .get()?
            .get_service()?;
    }
    read(&client, 37, 0).await?;
    Ok(())
}
async fn pipelined(mut client: service::Client, depth: usize) -> capnp::Result<()> {
    let mut pending = Vec::with_capacity(depth);
    for _ in 0..depth {
        let child = client.child_request().send();
        client = child.pipeline.get_service();
        pending.push(child.promise);
    }
    let (value, parents) =
        futures::join!(read(&client, 37, 0), futures::future::try_join_all(pending));
    value?;
    parents?;
    Ok(())
}
async fn perform(client: &service::Client, case: &str) -> capnp::Result<()> {
    match case {
        "unary" => {
            read(client, 37, 0).await?;
        }
        "large" => {
            read(client, 37, 100_000).await?;
        }
        "dependent" => dependent(client.clone(), 32).await?,
        "pipelined" => pipelined(client.clone(), 32).await?,
        "concurrent-32" | "fanout-128" => {
            let width = if case == "fanout-128" { 128 } else { 32 };
            futures::future::try_join_all((0..width).map(|n| read(client, n, 0))).await?;
        }
        "deep-32-concurrent-32" => {
            futures::future::try_join_all((0..32).map(|_| pipelined(client.clone(), 32))).await?;
        }
        "callback" => {
            let mut request = client.callback_request();
            request
                .get()
                .set_service(capnp_rpc::new_client::<service::Client, _>(Service));
            assert_eq!(request.send().promise.await?.get()?.get_value(), 37);
        }
        _ => return Err(capnp::Error::failed("unknown benchmark case".into())),
    }
    Ok(())
}
pub async fn serve<S: AsyncRead + AsyncWrite + Unpin + 'static>(
    stream: S,
    _args: &Args,
) -> Result<()> {
    let bootstrap: service::Client = capnp_rpc::new_client(Service);
    TwoPartyClient::with_bootstrap(stream, Some(bootstrap.client), Side::Server).await?;
    Ok(())
}
pub async fn call<S: AsyncRead + AsyncWrite + Unpin + 'static>(
    stream: S,
    args: &Args,
) -> Result<measure::Sample> {
    let mut rpc = TwoPartyClient::with_bootstrap(stream, None, Side::Client);
    let client: service::Client = rpc.bootstrap();
    let disconnect = rpc.get_disconnector();
    let application = async {
        for _ in 0..args.warmup {
            perform(&client, &args.case).await?;
        }
        let measure = measure::Region::start();
        for _ in 0..args.iterations {
            perform(&client, &args.case).await?;
        }
        let sample = measure.stop();
        disconnect.await?;
        Ok::<_, Box<dyn std::error::Error>>(sample)
    };
    let (driver, output) = futures::join!(rpc, application);
    driver?;
    output
}
fn main() -> Result<()> {
    let args = Args::parse()?;
    let sample = transports::run(&args)?;
    println!(
        "{}",
        serde_json::json!({"runtime":"tokio","transport":"tls","case":args.case,"driver":"legacy",
        "iterations":args.iterations,"warmup":args.warmup,"io_level":"ciphertext-completions","sample":sample,"version":1})
    );
    Ok(())
}
