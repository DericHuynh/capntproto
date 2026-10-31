//! A real greeting service: open a per-client capability, then pipeline echo.
pub mod greeting_capnp {
    include!(concat!(env!("OUT_DIR"), "/greeting_capnp.rs"));
}
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
use capnp_rpc::{rpc_twoparty_capnp::Side, twoparty::RpcDriver};
#[cfg(target_arch = "wasm32")]
use capntproto::rpc::webtransport;
use capntproto::{capnp, capnp_rpc};

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
    let mut driver =
        capnp_rpc::twoparty::TwoPartyClient::with_driver(stream, None, Side::Client, protocol()?)?;
    let service: greeting_capnp::greeting::Client = driver.bootstrap();
    let disconnect = driver.get_disconnector();
    let application = async {
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
        disconnect.await?;
        Ok::<_, Box<dyn std::error::Error>>(text)
    };
    let (driver, result) = futures::join!(driver, application);
    driver?;
    result
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub async fn call(
    url: String,
    prefix: String,
    value: String,
    certificate_sha256: Option<Vec<u8>>,
) -> std::result::Result<String, wasm_bindgen::JsValue> {
    use wasm_bindgen::JsValue;
    let client = match certificate_sha256 {
        Some(hash) => {
            let hash: [u8; 32] = hash
                .try_into()
                .map_err(|_| JsValue::from_str("SHA-256 must contain 32 bytes"))?;
            webtransport::Client::with_server_certificate_hashes(&[hash])
        }
        None => webtransport::Client::new(),
    };
    let mut session = client
        .connect(&url)
        .await
        .map_err(|err| JsValue::from_str(&err.to_string()))?;
    let stream = session
        .open_bi()
        .await
        .map_err(|err| JsValue::from_str(&err.to_string()))?;
    greeting(stream, &prefix, &value)
        .await
        .map_err(|err| JsValue::from_str(&err.to_string()))
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
