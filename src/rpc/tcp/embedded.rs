//! TCP with allocator-free embedded-tls cryptography and caller-owned records.
//! The handshake authenticates the server using the supplied clock and CA.
use capntproto_transport::embedded::{
    self, Aes128GcmSha256, CryptoRngCore, FuturesIo, TlsConnection,
};
pub use embedded::{with_futures_io, Buffers, ClientConfig};
use std::{io, time::Duration};
use tokio::net::{TcpStream, ToSocketAddrs};
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};

pub type Connection<'a> = TlsConnection<'a, FuturesIo<Compat<TcpStream>>, Aes128GcmSha256>;

/// DNS, TCP and TLS share one setup deadline. Supply P-256 trust material,
/// cryptographic entropy and clock explicitly. No insecure provider is used.
pub async fn connect<'buf, R: CryptoRngCore>(
    address: impl ToSocketAddrs,
    config: &ClientConfig<'_>,
    rng: R,
    buffers: Buffers<'buf, '_>,
    timeout: Duration,
) -> io::Result<Connection<'buf>> {
    super::super::io::deadline(timeout, async move {
        let socket = TcpStream::connect(address).await?;
        socket.set_nodelay(true)?;
        embedded::connect(FuturesIo::new(socket.compat()), config, rng, buffers)
            .await
            .map_err(io::Error::other)
    })
    .await
}
