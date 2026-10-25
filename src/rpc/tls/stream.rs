//! Tokio IO over rustls's Unbuffered API. Ciphertext storage is retained across
//! records; plaintext goes straight to the reader unless it asks for a partial
//! record. rustls 0.23 still allocates plaintext records internally.
#![forbid(unsafe_code)]

use rustls::{
    client::UnbufferedClientConnection,
    pki_types::ServerName,
    server::UnbufferedServerConnection,
    unbuffered::{ConnectionState, EncodeError, EncryptError, UnbufferedStatus, WriteTraffic},
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

// Enough for rustls's largest handshake message and the following record.
// This buffer never shrinks when a record has been consumed.
const INPUT_BYTES: usize = 64 * 1024 + 18 * 1024;
const WRITE_BYTES: usize = 128 * 1024;
const RECORD_BYTES: usize = 16 * 1024;
const OUTPUT_BYTES: usize = WRITE_BYTES + 1024;

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

enum Connection {
    Client(UnbufferedClientConnection),
    Server(UnbufferedServerConnection),
}
impl Connection {
    fn common(&self) -> &CommonState {
        match self {
            Self::Client(c) => c,
            Self::Server(c) => c,
        }
    }
}

// Offsets avoid moving unconsumed ciphertext after each TLS record. Only a
// socket refill that needs room compacts the buffer.
#[derive(Default)]
struct Buffer {
    bytes: Vec<u8>,
    start: usize,
    end: usize,
    flushed: bool,
}
impl Buffer {
    fn new(size: usize) -> Self {
        Self {
            bytes: vec![0; size],
            ..Self::default()
        }
    }
    fn pending(&self) -> &[u8] {
        &self.bytes[self.start..self.end]
    }
    fn consume(&mut self, size: usize) {
        self.start += size;
        if self.start == self.end {
            self.start = 0;
            self.end = 0;
        }
    }
    fn append(&mut self, bytes: &[u8]) {
        if self.end + bytes.len() > self.bytes.len() {
            self.bytes.resize(self.end + bytes.len(), 0);
        }
        self.bytes[self.end..self.end + bytes.len()].copy_from_slice(bytes);
        self.end += bytes.len();
    }
}

enum Operation<'a, 'b> {
    Handshake,
    Read(&'a mut ReadBuf<'b>),
    Write(&'a [u8]),
    WriteVectored(&'a [io::IoSlice<'b>]),
    Flush,
    Shutdown,
}
enum Action {
    Continue,
    Read,
    Transmit,
    Ready(usize),
}

// Encrypt contiguous payloads directly. Only records crossing a slice boundary
// need a plaintext copy; ciphertext still accumulates into one bounded write.
fn encrypt<Data>(
    traffic: &mut WriteTraffic<'_, Data>,
    bytes: &[u8],
    outgoing: &mut Buffer,
) -> io::Result<()> {
    let size = match traffic.encrypt(bytes, &mut outgoing.bytes[outgoing.end..]) {
        Ok(size) => size,
        Err(EncryptError::InsufficientSize(required)) => {
            outgoing
                .bytes
                .resize(outgoing.end + required.required_size, 0);
            traffic
                .encrypt(bytes, &mut outgoing.bytes[outgoing.end..])
                .map_err(io::Error::other)?
        }
        Err(error) => return Err(io::Error::other(error)),
    };
    outgoing.end += size;
    outgoing.flushed = false;
    Ok(())
}

fn encrypt_vectored<Data>(
    traffic: &mut WriteTraffic<'_, Data>,
    buffers: &[io::IoSlice<'_>],
    outgoing: &mut Buffer,
    batch: &mut Vec<u8>,
) -> io::Result<usize> {
    // Most RPC writes fit in one record. Copy that batch once without walking
    // the general cross-record cursor for every slice.
    let length = buffers.iter().try_fold(0usize, |length, b| {
        length.checked_add(b.len()).filter(|n| *n <= RECORD_BYTES)
    });
    if let Some(length) = length {
        batch.clear();
        batch.reserve_exact(length);
        for buffer in buffers {
            batch.extend_from_slice(buffer);
        }
        encrypt(traffic, batch, outgoing)?;
        return Ok(length);
    }
    let mut slices = buffers
        .iter()
        .map(|b| &b[..])
        .filter(|b| !b.is_empty())
        .peekable();
    let mut current = slices.next().unwrap_or_default();
    let mut accepted = 0;
    while !current.is_empty() && accepted < WRITE_BYTES {
        let limit = (WRITE_BYTES - accepted).min(current.len());
        if limit >= RECORD_BYTES || slices.peek().is_none() {
            // Keep a partial record for the next slice, unless this is the end.
            let size = if slices.peek().is_none() {
                limit
            } else {
                limit / RECORD_BYTES * RECORD_BYTES
            };
            encrypt(traffic, &current[..size], outgoing)?;
            accepted += size;
            current = &current[size..];
            if current.is_empty() {
                current = slices.next().unwrap_or_default();
            }
        } else {
            batch.clear();
            let limit = RECORD_BYTES.min(WRITE_BYTES - accepted);
            while batch.len() < limit && !current.is_empty() {
                let size = current.len().min(limit - batch.len());
                batch.extend_from_slice(&current[..size]);
                current = &current[size..];
                if current.is_empty() {
                    current = slices.next().unwrap_or_default();
                }
            }
            encrypt(traffic, batch, outgoing)?;
            accepted += batch.len();
        }
    }
    Ok(accepted)
}

// Handling a state and applying *all* its discard bytes is one operation. No
// borrowed rustls state survives an await or an underlying IO poll.
fn handle<Data>(
    status: UnbufferedStatus<'_, '_, Data>,
    operation: &mut Operation<'_, '_>,
    outgoing: &mut Buffer,
    plaintext: &mut Buffer,
    write_batch: &mut Vec<u8>,
    peer_closed: &mut bool,
    write_closed: &mut bool,
) -> io::Result<(usize, Action)> {
    let mut discard = status.discard;
    let action = match status.state.map_err(io::Error::other)? {
        ConnectionState::ReadTraffic(mut traffic) => {
            if let Some(record) = traffic.next_record() {
                let record = record.map_err(io::Error::other)?;
                discard += record.discard;
                if let Operation::Read(buffer) = operation {
                    let size = buffer.remaining().min(record.payload.len());
                    buffer.put_slice(&record.payload[..size]);
                    plaintext.append(&record.payload[size..]);
                    if size == 0 {
                        Action::Continue
                    } else {
                        Action::Ready(size)
                    }
                } else {
                    // A writer may have to consume a record already in incoming
                    // before rustls offers WriteTraffic. It never reads more
                    // application ciphertext from the socket while doing so.
                    plaintext.append(record.payload);
                    Action::Continue
                }
            } else {
                Action::Continue
            }
        }
        ConnectionState::EncodeTlsData(mut encode) => {
            let size = match encode.encode(&mut outgoing.bytes[outgoing.end..]) {
                Ok(size) => size,
                Err(EncodeError::InsufficientSize(required)) => {
                    outgoing
                        .bytes
                        .resize(outgoing.end + required.required_size, 0);
                    encode
                        .encode(&mut outgoing.bytes[outgoing.end..])
                        .map_err(io::Error::other)?
                }
                Err(error) => return Err(io::Error::other(error)),
            };
            outgoing.end += size;
            outgoing.flushed = false;
            Action::Continue
        }
        ConnectionState::TransmitTlsData(transmit) => {
            if outgoing.pending().is_empty() && outgoing.flushed {
                transmit.done();
                Action::Continue
            } else {
                Action::Transmit
            }
        }
        ConnectionState::BlockedHandshake => Action::Read,
        ConnectionState::WriteTraffic(mut traffic) => match operation {
            Operation::Handshake | Operation::Flush => Action::Ready(0),
            Operation::Read(_) => {
                if *peer_closed {
                    Action::Ready(0)
                } else {
                    Action::Read
                }
            }
            Operation::Write(bytes) => {
                let bytes = &bytes[..bytes.len().min(WRITE_BYTES)];
                encrypt(&mut traffic, bytes, outgoing)?;
                Action::Ready(bytes.len())
            }
            Operation::WriteVectored(buffers) => Action::Ready(encrypt_vectored(
                &mut traffic,
                buffers,
                outgoing,
                write_batch,
            )?),
            Operation::Shutdown => {
                if !*write_closed {
                    let size = traffic
                        .queue_close_notify(&mut outgoing.bytes[outgoing.end..])
                        .map_err(io::Error::other)?;
                    outgoing.end += size;
                    outgoing.flushed = false;
                    *write_closed = true;
                }
                Action::Ready(0)
            }
        },
        ConnectionState::PeerClosed => {
            *peer_closed = true;
            Action::Continue
        }
        ConnectionState::Closed => Action::Ready(0),
        ConnectionState::ReadEarlyData(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "TLS early data is disabled",
            ))
        }
        _ => return Err(io::Error::other("unsupported rustls connection state")),
    };
    Ok((discard, action))
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
        mut operation: Operation<'_, '_>,
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
        operation: Operation<'_, '_>,
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
