//! A real greeting service: open a per-client capability, then pipeline echo.
pub mod greeting_capnp {
    include!(concat!(env!("OUT_DIR"), "/greeting_capnp.rs"));
}
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
use capnp_rpc::{rpc_twoparty_capnp::Side, twoparty::RpcDriver};
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

pub async fn greeting(stream: webtransport::Stream, prefix: &str, value: &str) -> Result<String> {
    let mut driver = webtransport::driver(stream, None, Side::Client, protocol()?)?;
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
