//! IO-free rustls record processing shared by direct readiness and completion IO.
use rustls_impl::{
    client::UnbufferedClientConnection,
    server::UnbufferedServerConnection,
    unbuffered::{ConnectionState, EncodeError, EncryptError, UnbufferedStatus, WriteTraffic},
    CommonState,
};
use std::io;

pub(crate) trait ReadBuffer {
    fn remaining(&self) -> usize;
    fn put_slice(&mut self, bytes: &[u8]);
}
pub(crate) struct SliceRead<'a> {
    pub bytes: &'a mut [u8],
    pub filled: usize,
}
impl ReadBuffer for SliceRead<'_> {
    fn remaining(&self) -> usize {
        self.bytes.len() - self.filled
    }
    fn put_slice(&mut self, bytes: &[u8]) {
        self.bytes[self.filled..self.filled + bytes.len()].copy_from_slice(bytes);
        self.filled += bytes.len();
    }
}

/// Progress requested by the IO-free record layer. Yield means more local work
/// is ready after the bounded processing allowance, not socket readiness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress<T> {
    Ready(T),
    Input,
    Output,
    Yield,
}

/// A TLS state machine with no socket, executor, task, timer, or waker. Native
/// rustls owns allocating record storage; this is not the embedded TLS profile.
pub struct RecordLayer {
    pub(crate) connection: Connection,
    pub(crate) incoming: Buffer,
    pub(crate) outgoing: Buffer,
    pub(crate) plaintext: Buffer,
    pub(crate) write_batch: Vec<u8>,
    pub(crate) peer_closed: bool,
    pub(crate) write_closed: bool,
}
impl RecordLayer {
    pub fn client(
        config: std::sync::Arc<rustls_impl::ClientConfig>,
        name: rustls_impl::pki_types::ServerName<'static>,
    ) -> io::Result<Self> {
        if config.enable_early_data {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "TLS early data is disabled",
            ));
        }
        Ok(Self::new(Connection::Client(
            UnbufferedClientConnection::new(config, name).map_err(io::Error::other)?,
        )))
    }
    pub fn server(config: std::sync::Arc<rustls_impl::ServerConfig>) -> io::Result<Self> {
        if config.max_early_data_size != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "TLS early data is disabled",
            ));
        }
        Ok(Self::new(Connection::Server(
            UnbufferedServerConnection::new(config).map_err(io::Error::other)?,
        )))
    }
    fn new(connection: Connection) -> Self {
        Self {
            connection,
            incoming: Buffer::new(INPUT_BYTES),
            outgoing: Buffer::new(OUTPUT_BYTES),
            plaintext: Buffer::default(),
            write_batch: Vec::new(),
            peer_closed: false,
            write_closed: false,
        }
    }
    pub fn common(&self) -> &CommonState {
        self.connection.common()
    }

    /// Lend unused ciphertext input. Commit only the bytes actually received.
    /// Existing incomplete records are compacted only when refill needs room.
    pub fn input(&mut self) -> io::Result<&mut [u8]> {
        let b = &mut self.incoming;
        if b.bytes.is_empty() {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        if b.bytes.len() - b.end < 18 * 1024 && b.start != 0 {
            b.bytes.copy_within(b.start..b.end, 0);
            b.end -= b.start;
            b.start = 0;
        }
        if b.end == b.bytes.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "TLS input limit exceeded",
            ));
        }
        Ok(&mut b.bytes[b.end..])
    }
    pub fn commit_input(&mut self, count: usize) -> io::Result<()> {
        if self.incoming.bytes.is_empty() || count > self.incoming.bytes.len() - self.incoming.end {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "TLS receive count exceeds the lent buffer",
            ));
        }
        if count == 0 && !self.peer_closed {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "TLS peer closed without close_notify",
            ));
        }
        self.incoming.end += count;
        Ok(())
    }
    pub fn output(&self) -> &[u8] {
        self.outgoing.pending()
    }
    pub fn consume_output(&mut self, count: usize) -> io::Result<()> {
        if count > self.outgoing.pending().len() {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        self.outgoing.consume(count);
        Ok(())
    }
    /// Mark physical/local flush completion only after all accepted output.
    pub fn flushed(&mut self) -> io::Result<()> {
        if !self.outgoing.pending().is_empty() {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        self.outgoing.flushed = true;
        Ok(())
    }
    pub fn handshake(&mut self) -> io::Result<Progress<usize>> {
        self.advance(Operation::Handshake)
    }
    pub fn read(&mut self, bytes: &mut [u8]) -> io::Result<Progress<usize>> {
        if bytes.is_empty() {
            return Ok(Progress::Ready(0));
        }
        if !self.plaintext.pending().is_empty() {
            let n = bytes.len().min(self.plaintext.pending().len());
            bytes[..n].copy_from_slice(&self.plaintext.pending()[..n]);
            self.plaintext.consume(n);
            return Ok(Progress::Ready(n));
        }
        if self.peer_closed {
            return Ok(Progress::Ready(0));
        }
        self.advance(Operation::Read(&mut SliceRead { bytes, filled: 0 }))
    }
    pub fn write(&mut self, bytes: &[u8]) -> io::Result<Progress<usize>> {
        if self.write_closed {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        if bytes.is_empty() {
            return Ok(Progress::Ready(0));
        }
        // Accept a bounded batch before the completion driver submits IO.
        // AsyncWrite acceptance does not require flushing the previous write.
        if self.outgoing.end >= WRITE_BYTES {
            return Ok(Progress::Output);
        }
        self.advance(Operation::Write(bytes))
    }
    pub fn write_vectored(&mut self, bytes: &[io::IoSlice<'_>]) -> io::Result<Progress<usize>> {
        if self.write_closed {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        if bytes.iter().all(|b| b.is_empty()) {
            return Ok(Progress::Ready(0));
        }
        // Accept a bounded batch before the completion driver submits IO.
        // AsyncWrite acceptance does not require flushing the previous write.
        if self.outgoing.end >= WRITE_BYTES {
            return Ok(Progress::Output);
        }
        self.advance(Operation::WriteVectored(bytes))
    }
    pub fn flush(&mut self) -> io::Result<Progress<usize>> {
        self.advance(Operation::Flush)
    }
    pub fn shutdown(&mut self) -> io::Result<Progress<usize>> {
        self.advance(Operation::Shutdown)
    }
    fn advance(
        &mut self,
        mut operation: Operation<'_, '_, SliceRead<'_>>,
    ) -> io::Result<Progress<usize>> {
        for _ in 0..128 {
            let bytes = if self.incoming.bytes.is_empty() {
                &mut [][..]
            } else {
                &mut self.incoming.bytes[self.incoming.start..self.incoming.end]
            };
            let (discard, action) = match &mut self.connection {
                Connection::Client(c) => handle(
                    c.process_tls_records(bytes),
                    &mut operation,
                    &mut self.outgoing,
                    &mut self.plaintext,
                    &mut self.write_batch,
                    &mut self.peer_closed,
                    &mut self.write_closed,
                )?,
                Connection::Server(c) => handle(
                    c.process_tls_records(bytes),
                    &mut operation,
                    &mut self.outgoing,
                    &mut self.plaintext,
                    &mut self.write_batch,
                    &mut self.peer_closed,
                    &mut self.write_closed,
                )?,
            };
            self.incoming.consume(discard);
            match action {
                Action::Continue => (),
                Action::Read => return Ok(Progress::Input),
                Action::Transmit => return Ok(Progress::Output),
                Action::Ready(n) => return Ok(Progress::Ready(n)),
            }
        }
        Ok(Progress::Yield)
    }
}
// Enough for rustls's largest handshake message and the following record.
// This buffer never shrinks when a record has been consumed.
pub(crate) const INPUT_BYTES: usize = 64 * 1024 + 18 * 1024;
pub(crate) const WRITE_BYTES: usize = 128 * 1024;
pub(crate) const RECORD_BYTES: usize = 16 * 1024;
pub(crate) const OUTPUT_BYTES: usize = WRITE_BYTES + 1024;

pub(crate) enum Connection {
    Client(UnbufferedClientConnection),
    Server(UnbufferedServerConnection),
}
impl Connection {
    pub(crate) fn common(&self) -> &CommonState {
        match self {
            Self::Client(c) => c,
            Self::Server(c) => c,
        }
    }
}

// Offsets avoid moving unconsumed ciphertext after each TLS record. Only a
// socket refill that needs room compacts the buffer.
#[derive(Default)]
pub(crate) struct Buffer {
    pub(crate) bytes: Vec<u8>,
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) flushed: bool,
}
impl Buffer {
    pub(crate) fn new(size: usize) -> Self {
        Self {
            bytes: vec![0; size],
            ..Self::default()
        }
    }
    pub(crate) fn pending(&self) -> &[u8] {
        &self.bytes[self.start..self.end]
    }
    pub(crate) fn consume(&mut self, size: usize) {
        self.start += size;
        if self.start == self.end {
            self.start = 0;
            self.end = 0;
        }
    }
    pub(crate) fn append(&mut self, bytes: &[u8]) {
        if self.end + bytes.len() > self.bytes.len() {
            self.bytes.resize(self.end + bytes.len(), 0);
        }
        self.bytes[self.end..self.end + bytes.len()].copy_from_slice(bytes);
        self.end += bytes.len();
    }
}

pub(crate) enum Operation<'a, 'b, R: ?Sized> {
    Handshake,
    Read(&'a mut R),
    Write(&'a [u8]),
    WriteVectored(&'a [io::IoSlice<'b>]),
    Flush,
    Shutdown,
}
pub(crate) enum Action {
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
pub(crate) fn handle<Data, R: ReadBuffer + ?Sized>(
    status: UnbufferedStatus<'_, '_, Data>,
    operation: &mut Operation<'_, '_, R>,
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
