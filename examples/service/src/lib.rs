//! One schema-defined service shared by native and component clients.
pub use capnp_rpc as rpc;
pub mod greeting_capnp {
    include!(concat!(env!("OUT_DIR"), "/greeting_capnp.rs"));
}
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
use capnp_rpc::{
    rpc_twoparty_capnp::Side,
    twoparty::{RpcDriver, TwoPartyClient},
};
pub fn protocol() -> capnp::Result<RpcDriver> {
    Ok(RpcDriver::Portable(capnp_rpc::portable::Options::bounded(
        64.try_into().unwrap(),
        4096.try_into().unwrap(),
        (1024 * 1024).try_into().unwrap(),
        (16 * 1024 * 1024).try_into().unwrap(),
    )?))
}

pub async fn greeting<S: futures::AsyncRead + futures::AsyncWrite + Unpin + 'static>(
    stream: S,
    prefix: &str,
    value: &str,
) -> Result<String> {
    greeting_with_protocol(stream, prefix, value, protocol()?).await
}
pub async fn greeting_with_protocol<
    S: futures::AsyncRead + futures::AsyncWrite + Unpin + 'static,
>(
    stream: S,
    prefix: &str,
    value: &str,
    protocol: RpcDriver,
) -> Result<String> {
    greeting_with_driver(
        TwoPartyClient::with_driver(stream, None, Side::Client, protocol)?,
        prefix,
        value,
    )
    .await
}
pub async fn greeting_with_driver(
    mut driver: TwoPartyClient<'_>,
    prefix: &str,
    value: &str,
) -> Result<String> {
    let service: greeting_capnp::greeting::Client = driver.bootstrap();
    let disconnect = driver.get_disconnector();
    let application = async {
        let text = call(&service, prefix, value).await?;
        disconnect.await?;
        Ok::<_, Box<dyn std::error::Error>>(text)
    };
    let (driver, result) = futures::join!(driver, application);
    driver?;
    result
}

/// Verified responses followed by cancellation of the caller-owned RPC scope.
/// The transport owner closes its session separately; no graceful drain is promised.
pub async fn greeting_scoped(
    mut driver: TwoPartyClient<'_>,
    prefix: &str,
    value: &str,
) -> Result<String> {
    let service: greeting_capnp::greeting::Client = driver.bootstrap();
    let application = call(&service, prefix, value);
    futures::pin_mut!(application, driver);
    match futures::future::select(application, driver).await {
        futures::future::Either::Left((result, _)) => result,
        futures::future::Either::Right((result, _)) => {
            result?;
            Err("connection ended before the greeting completed".into())
        }
    }
}
async fn call(
    service: &greeting_capnp::greeting::Client,
    prefix: &str,
    value: &str,
) -> Result<String> {
    let mut open = service.open_request();
    open.get().set_prefix(prefix);
    let opened = open.send();
    // The child call is sent before the parent Return is observed.
    let session = opened.pipeline.get_session();
    let mut echo = session.echo_request();
    echo.get().set_value(value);
    let response = echo.send().promise.await?;
    let text = response.get()?.get_value()?.to_str()?.to_owned();
    opened.promise.await?;
    Ok(text)
}
use capnp_rpc::new_client;
use greeting_capnp::{greeting, session};
use std::rc::Rc;
pub struct Service;
struct Session(String);
impl greeting::Server for Service {
    fn open(
        self: Rc<Self>,
        context: greeting::OpenContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        context.complete(|params, mut results| {
            let prefix = params.get_prefix()?.to_str()?.to_owned();
            results.set_session(new_client::<session::Client, _>(Session(prefix)));
            Ok(())
        })
    }
}
impl session::Server for Session {
    fn echo(
        self: Rc<Self>,
        context: session::EchoContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        context.complete(|params, mut results| {
            let value = format!("{}{}", self.0, params.get_value()?.to_str()?);
            results.set_value(&value);
            Ok(())
        })
    }
}
