//! Tokio IO over rustls's Unbuffered API. Ciphertext storage is retained across
//! records; plaintext goes straight to the reader unless it asks for a partial
//! record. rustls 0.23 still allocates plaintext records internally.
#![forbid(unsafe_code)]

use super::record::*;
use rustls_impl::{
    client::UnbufferedClientConnection, pki_types::ServerName, server::UnbufferedServerConnection,
    ClientConfig, CommonState, ServerConfig,
};
use std::{
    future::poll_fn,
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// An authenticated TLS stream using rustls's Unbuffered connection API.
/// The handshake constructors use the supplied verification and ALPN policy;
/// callers must impose their own setup deadline and validate negotiated ALPN.
/// Application early data is not supported.
pub struct TlsStream<S> {
    io: S,
    connection: Connection,
    incoming: Buffer,
    outgoing: Buffer,
    plaintext: Buffer,
    write_batch: Vec<u8>,
    peer_closed: bool,
    write_closed: bool,
    eof: bool,
    progressed: bool,
    reader: Option<Waker>,
    writer: Option<Waker>,
}

impl ReadBuffer for ReadBuf<'_> {
    fn remaining(&self) -> usize {
        self.remaining()
    }
    fn put_slice(&mut self, bytes: &[u8]) {
        self.put_slice(bytes);
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> TlsStream<S> {
    /// Complete a client handshake with the supplied certificate/name verifier.
    pub async fn connect(
        io: S,
        config: Arc<ClientConfig>,
        name: ServerName<'static>,
    ) -> io::Result<Self> {
        let connection = UnbufferedClientConnection::new(config, name).map_err(io::Error::other)?;
        Self::handshake(io, Connection::Client(connection)).await
    }
    /// Complete a server handshake with the supplied client-authentication policy.
    pub async fn accept(io: S, config: Arc<ServerConfig>) -> io::Result<Self> {
        if config.max_early_data_size != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "TLS early data is disabled",
            ));
        }
        let connection = UnbufferedServerConnection::new(config).map_err(io::Error::other)?;
        Self::handshake(io, Connection::Server(connection)).await
    }
    async fn handshake(io: S, connection: Connection) -> io::Result<Self> {
        let mut stream = Self {
            io,
            connection,
            incoming: Buffer::new(INPUT_BYTES),
            outgoing: Buffer::new(OUTPUT_BYTES),
            plaintext: Buffer::default(),
            write_batch: Vec::new(),
            peer_closed: false,
            write_closed: false,
            eof: false,
            progressed: false,
            reader: None,
            writer: None,
        };
        poll_fn(|cx| stream.poll_drive(cx, Operation::Handshake)).await?;
        Ok(stream)
    }
    /// Inspect the transport and authenticated TLS metadata without mutating the
    /// record state. This includes peer certificates, TLS version and ALPN.
    pub fn get_ref(&self) -> (&S, &CommonState) {
        (&self.io, self.connection.common())
    }

    fn poll_transmit(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.outgoing.flushed {
            return Poll::Ready(Ok(()));
        }
        while !self.outgoing.pending().is_empty() {
            let size =
                std::task::ready!(Pin::new(&mut self.io).poll_write(cx, self.outgoing.pending()))?;
            if size == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }
            self.outgoing.consume(size);
            self.progressed = true;
        }
        std::task::ready!(Pin::new(&mut self.io).poll_flush(cx))?;
        self.outgoing.flushed = true;
        self.progressed = true;
        Poll::Ready(Ok(()))
    }
    fn poll_receive(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.eof {
            return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
        }
        let input = &mut self.incoming;
        if input.bytes.len() - input.end < 18 * 1024 && input.start != 0 {
            input.bytes.copy_within(input.start..input.end, 0);
            input.end -= input.start;
            input.start = 0;
        }
        if input.end == input.bytes.len() {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "TLS input limit exceeded",
            )));
        }
        let mut buffer = ReadBuf::new(&mut input.bytes[input.end..]);
        std::task::ready!(Pin::new(&mut self.io).poll_read(cx, &mut buffer))?;
        let size = buffer.filled().len();
        if size == 0 {
            self.eof = true;
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "TLS peer closed without close_notify",
            )));
        }
        input.end += size;
        self.progressed = true;
        Poll::Ready(Ok(()))
    }
    fn poll_drive(
        &mut self,
        cx: &mut Context<'_>,
        mut operation: Operation<'_, '_, ReadBuf<'_>>,
    ) -> Poll<io::Result<usize>> {
        if matches!(
            operation,
            Operation::Write(_)
                | Operation::WriteVectored(_)
                | Operation::Shutdown
                | Operation::Flush
        ) {
            std::task::ready!(self.poll_transmit(cx))?;
        }
        loop {
            let input = &mut self.incoming;
            let incoming = &mut input.bytes[input.start..input.end];
            let (discard, action) = match &mut self.connection {
                Connection::Client(c) => handle(
                    c.process_tls_records(incoming),
                    &mut operation,
                    &mut self.outgoing,
                    &mut self.plaintext,
                    &mut self.write_batch,
                    &mut self.peer_closed,
                    &mut self.write_closed,
                )?,
                Connection::Server(c) => handle(
                    c.process_tls_records(incoming),
                    &mut operation,
                    &mut self.outgoing,
                    &mut self.plaintext,
                    &mut self.write_batch,
                    &mut self.peer_closed,
                    &mut self.write_closed,
                )?,
            };
            input.consume(discard);
            match action {
                Action::Continue => {
                    self.progressed = true;
                }
                Action::Transmit => {
                    std::task::ready!(self.poll_transmit(cx))?;
                }
                Action::Read => {
                    // Read progress must remain possible under write backpressure:
                    // both peers may be sending messages larger than socket buffers.
                    if let Poll::Ready(result) = self.poll_transmit(cx) {
                        result?;
                    }
                    std::task::ready!(self.poll_receive(cx))?;
                }
                Action::Ready(size) => {
                    self.progressed |= size != 0;
                    return Poll::Ready(Ok(size));
                }
            }
        }
    }
    fn poll_write_operation(
        &mut self,
        cx: &mut Context<'_>,
        operation: Operation<'_, '_, ReadBuf<'_>>,
        empty: bool,
    ) -> Poll<io::Result<usize>> {
        if self.write_closed {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        if empty {
            return Poll::Ready(Ok(0));
        }
        self.progressed = false;
        let result = self.poll_drive(cx, operation);
        if self.progressed {
            self.wake_reader();
        }
        if result.is_pending() {
            self.writer = Some(cx.waker().clone());
        }
        result
    }
    fn wake_reader(&mut self) {
        if let Some(waker) = self.reader.take() {
            waker.wake();
        }
    }
    fn wake_writer(&mut self) {
        if let Some(waker) = self.writer.take() {
            waker.wake();
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for TlsStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if buffer.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if !this.plaintext.pending().is_empty() {
            let size = buffer.remaining().min(this.plaintext.pending().len());
            buffer.put_slice(&this.plaintext.pending()[..size]);
            this.plaintext.consume(size);
            return Poll::Ready(Ok(()));
        }
        if this.peer_closed {
            return Poll::Ready(Ok(()));
        }
        this.progressed = false;
        let result = this.poll_drive(cx, Operation::Read(buffer)).map_ok(|_| ());
        if this.progressed {
            this.wake_writer();
        }
        if result.is_pending() {
            this.reader = Some(cx.waker().clone());
        }
        result
    }
}
impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for TlsStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.get_mut()
            .poll_write_operation(cx, Operation::Write(bytes), bytes.is_empty())
    }
    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffers: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        self.get_mut().poll_write_operation(
            cx,
            Operation::WriteVectored(buffers),
            buffers.iter().all(|b| b.is_empty()),
        )
    }
    fn is_write_vectored(&self) -> bool {
        true
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        this.progressed = false;
        let result = (|| {
            std::task::ready!(this.poll_drive(cx, Operation::Flush))?;
            this.poll_transmit(cx)
        })();
        if this.progressed {
            this.wake_reader();
        }
        if result.is_pending() {
            this.writer = Some(cx.waker().clone());
        }
        result
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        this.progressed = false;
        let result = (|| {
            std::task::ready!(this.poll_drive(cx, Operation::Shutdown))?;
            std::task::ready!(this.poll_transmit(cx))?;
            Pin::new(&mut this.io).poll_shutdown(cx)
        })();
        if this.progressed {
            this.wake_reader();
        }
        if result.is_pending() {
            this.writer = Some(cx.waker().clone());
        }
        result
    }
}
