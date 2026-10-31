//! Desktop socket adapter and the actual bounded telemetry service.
use crate::{telemetry_capnp, Acknowledgement, Reading};
use capntproto::rpc::{security::rustls, tcp::tls};
use std::{boxed::Box, io, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
pub struct Socket {
    pub io: TcpStream,
    pub fragment: usize,
}
impl crate::embedded_io_async::ErrorType for Socket {
    type Error = io::Error;
}
impl crate::Read for Socket {
    async fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let limit = bytes.len().min(self.fragment.max(1));
        self.io.read(&mut bytes[..limit]).await
    }
}
impl crate::Write for Socket {
    async fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.io
            .write(&bytes[..bytes.len().min(self.fragment.max(1))])
            .await
    }
    async fn flush(&mut self) -> io::Result<()> {
        self.io.flush().await
    }
}
pub async fn serve(
    socket: TcpStream,
    config: Arc<rustls::ServerConfig>,
    fragment: usize,
) -> Result<Reading, Box<dyn std::error::Error>> {
    let mut tls = tls::accept(socket, config, Duration::from_secs(10)).await?;
    let mut words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 64];
    let mut receiver = capnp::serialize::FrameReceiver::new(
        capnp::Word::words_to_bytes_mut(&mut words),
        Default::default(),
    )?;
    while let Some(mut input) = receiver.input()? {
        let count = tls.read(input.bytes()).await?;
        input.commit(count)?;
    }
    let message = receiver
        .message()?
        .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))?;
    let input = message.get_root::<telemetry_capnp::reading::Reader>()?;
    let reading = Reading {
        device_id: input.get_device_id(),
        sequence: input.get_sequence(),
        milli_celsius: input.get_milli_celsius(),
    };
    let ack = Acknowledgement {
        sequence: reading.sequence,
        accepted: (-40_000..=125_000).contains(&reading.milli_celsius),
    };
    let mut message = capnp::message::Builder::new_default();
    let mut root = message.init_root::<telemetry_capnp::acknowledgement::Builder>();
    root.set_sequence(ack.sequence);
    root.set_accepted(ack.accepted);
    for chunk in capnp::serialize::write_message_to_words(&message).chunks(fragment.max(1)) {
        tls.write_all(chunk).await?;
    }
    // Do not close TCP before the device's close-notify or race its final flush.
    let mut trailing = [0; 1];
    if tls.read(&mut trailing).await? != 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "unexpected second request").into());
    }
    tls.shutdown().await?;
    Ok(reading)
}
