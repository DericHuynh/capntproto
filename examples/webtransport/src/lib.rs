//! Native and browser adapters for the shared RPC service.
#[cfg(target_arch = "wasm32")]
use capntproto::rpc::webtransport;
pub use service::{greeting, greeting_capnp, protocol, Result, Service};
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
    let epoch = browser_now();
    let driver = service::rpc::twoparty::ConnectionOptions::new()
        .driver(protocol().map_err(|e| JsValue::from_str(&e.to_string()))?)
        .client_with_clock(stream, move || {
            std::time::Duration::from_secs_f64(((browser_now() - epoch) / 1000.0).max(0.0))
        })
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    service::greeting_scoped(driver, &prefix, &value)
        .await
        .map_err(|e| JsValue::from_str(&e.to_string()))
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace=performance,js_name=now)]
    fn browser_now() -> f64;
}
