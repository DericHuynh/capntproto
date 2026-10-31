//! WebTransport over HTTP/3. Each reliable bidirectional stream can carry an
//! independent Cap'n Proto RPC connection. Session/stream owners keep the endpoint
//! alive; closing one RPC stream does not close its siblings.
//!
//! Native endpoints use rustls with the caller's clock. Browsers implement their
//! own certificate validation and clock; this API cannot override either.
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::*;
#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(target_arch = "wasm32")]
pub use browser::*;

fn checked_url(value: &str) -> std::io::Result<url::Url> {
    let url = url::Url::parse(value).map_err(std::io::Error::other)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "WebTransport requires an HTTPS URL without user information or a fragment",
        ));
    }
    Ok(url)
}

pub use crate::rpc::{driver, network};

#[cfg(test)]
mod tests {
    #[test]
    fn rejects_insecure_or_ambiguous_session_urls() {
        for url in [
            "http://localhost/rpc",
            "https://user@localhost/rpc",
            "https://user:password@localhost/rpc",
            "https://localhost/rpc#secret",
            "file:///rpc",
            "invalid",
        ] {
            assert!(super::checked_url(url).is_err(), "{url}");
        }
        for url in ["https://localhost/rpc", "https://[::1]:4433/rpc?version=1"] {
            assert!(super::checked_url(url).is_ok(), "{url}");
        }
    }
}
