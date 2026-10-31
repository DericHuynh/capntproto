//! No-std/no-alloc device client with caller-owned IO, entropy, clock and buffers.
#![no_std]
#![forbid(unsafe_code)]
#[cfg(feature = "host")]
extern crate std;
use capntproto_transport::{embassy, embedded};
pub use capntproto_transport::{embedded::Buffers, TlsClock};
pub use embedded::embedded_io_async::{self, Read, Write};
pub mod telemetry_capnp {
    include!(concat!(env!("OUT_DIR"), "/telemetry_capnp.rs"));
}
#[cfg(feature = "host")]
pub mod host;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading {
    pub device_id: u64,
    pub sequence: u64,
    pub milli_celsius: i32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Acknowledgement {
    pub sequence: u64,
    pub accepted: bool,
}
#[derive(Debug)]
pub enum Error {
    Tls(embedded::TlsError),
    Send(embassy::SendError<embedded::TlsError>),
    Receive(embassy::wire::io::ReceiveError<embedded::TlsError>),
    Wire(capnp::Error),
    UnexpectedEof,
    WrongSequence,
}
/// Encode using at least 32 words of caller scratch and a bounded output slice.
pub fn encode(
    reading: Reading,
    words: &mut [capnp::Word],
    output: &mut [u8],
) -> capnp::Result<usize> {
    if words.len() < 32 {
        return Err(capnp::Error::from_kind(capnp::ErrorKind::Overloaded));
    }
    let allocator =
        capnp::message::SingleSegmentAllocator::new(capnp::Word::words_to_bytes_mut(words));
    let mut message = capnp::message::Builder::new(allocator);
    let mut root = message.init_root::<telemetry_capnp::reading::Builder>();
    root.set_device_id(reading.device_id);
    root.set_sequence(reading.sequence);
    root.set_milli_celsius(reading.milli_celsius);
    let size = capnp::serialize::compute_serialized_size_in_words(&message) * 8;
    capnp::serialize::write_message(&mut &mut output[..], &message)?;
    Ok(size)
}
pub struct Storage {
    pub tls_read: [u8; 18 * 1024],
    pub tls_write: [u8; 18 * 1024],
    pub certificate: [u8; 2048],
    send_words: [capnp::Word; 32],
    send_bytes: [u8; 512],
    receive_words: [capnp::Word; 64],
}
impl Default for Storage {
    fn default() -> Self {
        Self::new()
    }
}
impl Storage {
    pub const fn new() -> Self {
        const ZERO: capnp::Word = capnp::word(0, 0, 0, 0, 0, 0, 0, 0);
        Self {
            tls_read: [0; 18 * 1024],
            tls_write: [0; 18 * 1024],
            certificate: [0; 2048],
            send_words: [ZERO; 32],
            send_bytes: [0; 512],
            receive_words: [ZERO; 64],
        }
    }
}
/// The caller applies an application deadline. Cancellation discards the session;
/// do not reuse the socket after a canceled TLS operation.
pub async fn publish<S, R>(
    io: S,
    config: &embedded::ClientConfig<'_>,
    rng: R,
    storage: &mut Storage,
    reading: Reading,
) -> Result<Acknowledgement, Error>
where
    S: Read + Write,
    R: rand_core::CryptoRng + rand_core::RngCore,
{
    let length =
        encode(reading, &mut storage.send_words, &mut storage.send_bytes).map_err(Error::Wire)?;
    let mut tls = embedded::connect(
        io,
        config,
        rng,
        Buffers {
            read: &mut storage.tls_read,
            write: &mut storage.tls_write,
            certificate: &mut storage.certificate,
        },
    )
    .await
    .map_err(Error::Tls)?;
    embassy::send(&mut tls, &storage.send_bytes[..length])
        .await
        .map_err(Error::Send)?;
    tls.flush().await.map_err(Error::Tls)?;
    let options = capnp::message::ReaderOptions {
        traversal_limit_in_words: Some(64),
        nesting_limit: 8,
    };
    let mut receiver = capnp::serialize::FrameReceiver::new(
        capnp::Word::words_to_bytes_mut(&mut storage.receive_words),
        options,
    )
    .map_err(Error::Wire)?;
    if !embassy::receive(&mut tls, &mut receiver)
        .await
        .map_err(Error::Receive)?
    {
        return Err(Error::UnexpectedEof);
    }
    let message = receiver
        .message()
        .map_err(Error::Wire)?
        .ok_or(Error::UnexpectedEof)?;
    let response = message
        .get_root::<telemetry_capnp::acknowledgement::Reader>()
        .map_err(Error::Wire)?;
    let result = Acknowledgement {
        sequence: response.get_sequence(),
        accepted: response.get_accepted(),
    };
    if result.sequence != reading.sequence {
        return Err(Error::WrongSequence);
    }
    tls.close().await.map_err(|(_, error)| Error::Tls(error))?;
    Ok(result)
}
/// Poll the Embassy stack Runner concurrently. TCP and TLS storage remain
/// caller-owned; no task, global clock or allocator is introduced.
#[cfg(feature = "embassy-net")]
pub async fn publish_on_embassy(
    stack: embassy::net::Stack<'_>,
    peer: embassy::net::IpEndpoint,
    tcp: embassy::TcpBuffers<'_>,
    config: &embedded::ClientConfig<'_>,
    rng: impl rand_core::CryptoRng + rand_core::RngCore,
    storage: &mut Storage,
    reading: Reading,
) -> Result<Acknowledgement, EmbassyError> {
    let mut socket = embassy::net::tcp::TcpSocket::new(stack, tcp.receive, tcp.transmit);
    socket.connect(peer).await.map_err(EmbassyError::Tcp)?;
    publish(socket, config, rng, storage, reading)
        .await
        .map_err(EmbassyError::Publish)
}
#[cfg(feature = "embassy-net")]
#[derive(Debug)]
pub enum EmbassyError {
    Tcp(embassy::net::tcp::ConnectError),
    Publish(Error),
}
