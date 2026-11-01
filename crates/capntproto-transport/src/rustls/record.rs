//! rustls record processing for the direct Tokio stream.
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
    // A single effective slice already has contiguous plaintext. Feed it
    // directly to rustls's caller-owned ciphertext output instead of copying
    // through (and initially allocating) the scatter/gather staging buffer.
    let mut nonempty = buffers.iter().filter(|buffer| !buffer.is_empty());
    if let Some(first) = nonempty.next() {
        if nonempty.next().is_none() {
            let size = first.len().min(WRITE_BYTES);
            encrypt(traffic, &first[..size], outgoing)?;
            return Ok(size);
        }
    }
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
