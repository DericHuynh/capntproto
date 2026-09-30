//! Owned RPC stream progress. No sockets, clocks, tasks or shared mutable state.
#![forbid(unsafe_code)]

use std::io;

const BUFFER_BYTES: usize = 16 * 1024;

enum SendPhase {
    Reading,
    Buffered { offset: usize, length: usize },
    Eof,
    Finished,
}

pub(super) struct SendStream {
    buffer: Vec<u8>,
    phase: SendPhase,
    written: u64,
}
impl Default for SendStream {
    fn default() -> Self {
        Self {
            buffer: vec![0; BUFFER_BYTES],
            phase: SendPhase::Reading,
            written: 0,
        }
    }
}
impl SendStream {
    pub(super) fn can_read(&self) -> bool {
        matches!(self.phase, SendPhase::Reading)
    }
    pub(super) fn read_buffer(&mut self) -> io::Result<&mut [u8]> {
        if !self.can_read() {
            return Err(invalid_progress());
        }
        Ok(&mut self.buffer)
    }
    pub(super) fn read(&mut self, count: usize) -> io::Result<()> {
        if !self.can_read() || count > self.buffer.len() {
            return Err(invalid_progress());
        }
        self.phase = if count == 0 {
            SendPhase::Eof
        } else {
            SendPhase::Buffered {
                offset: 0,
                length: count,
            }
        };
        Ok(())
    }
    pub(super) fn pending(&self, graceful: bool) -> Option<(&[u8], bool)> {
        match self.phase {
            SendPhase::Buffered { offset, length } => Some((&self.buffer[offset..length], false)),
            SendPhase::Eof if !graceful => Some((&[], true)),
            _ => None,
        }
    }
    pub(super) fn sent(&mut self, count: usize) -> io::Result<()> {
        let next = match self.phase {
            SendPhase::Buffered { offset, length } if count <= length - offset => {
                if offset + count == length {
                    SendPhase::Reading
                } else {
                    SendPhase::Buffered {
                        offset: offset + count,
                        length,
                    }
                }
            }
            SendPhase::Eof if count == 0 => SendPhase::Finished,
            _ => return Err(invalid_progress()),
        };
        let written = self
            .written
            .checked_add(count as u64)
            .ok_or_else(|| io::Error::other("Noise stream counter exhausted"))?;
        self.phase = next;
        self.written = written;
        Ok(())
    }
    pub(super) fn written(&self) -> u64 {
        self.written
    }
    pub(super) fn drained(&self) -> bool {
        matches!(self.phase, SendPhase::Eof | SendPhase::Finished)
    }
}

enum ReceivePhase {
    Receiving,
    Buffered {
        offset: usize,
        length: usize,
        fin: bool,
    },
    Eof,
    Closed,
}
pub(super) struct ReceiveStream {
    buffer: Vec<u8>,
    phase: ReceivePhase,
}
impl Default for ReceiveStream {
    fn default() -> Self {
        Self {
            buffer: vec![0; BUFFER_BYTES],
            phase: ReceivePhase::Receiving,
        }
    }
}
impl ReceiveStream {
    pub(super) fn can_receive(&self) -> bool {
        matches!(self.phase, ReceivePhase::Receiving)
    }
    pub(super) fn receive_buffer(&mut self) -> io::Result<&mut [u8]> {
        if !self.can_receive() {
            return Err(invalid_progress());
        }
        Ok(&mut self.buffer)
    }
    pub(super) fn received(&mut self, count: usize, fin: bool, skip: usize) -> io::Result<()> {
        if !self.can_receive() || count > self.buffer.len() || skip > count {
            return Err(invalid_progress());
        }
        self.phase = if skip < count {
            ReceivePhase::Buffered {
                offset: skip,
                length: count,
                fin,
            }
        } else if fin {
            ReceivePhase::Eof
        } else {
            ReceivePhase::Receiving
        };
        Ok(())
    }
    pub(super) fn pending(&self) -> &[u8] {
        match self.phase {
            ReceivePhase::Buffered { offset, length, .. } => &self.buffer[offset..length],
            _ => &[],
        }
    }
    pub(super) fn delivered(&mut self, count: usize) -> io::Result<()> {
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "RPC consumer closed",
            ));
        }
        let ReceivePhase::Buffered {
            offset,
            length,
            fin,
        } = self.phase
        else {
            return Err(invalid_progress());
        };
        if count > length - offset {
            return Err(invalid_progress());
        }
        self.phase = if offset + count < length {
            ReceivePhase::Buffered {
                offset: offset + count,
                length,
                fin,
            }
        } else if fin {
            ReceivePhase::Eof
        } else {
            ReceivePhase::Receiving
        };
        Ok(())
    }
    pub(super) fn needs_shutdown(&self) -> bool {
        matches!(self.phase, ReceivePhase::Eof)
    }
    pub(super) fn closed(&mut self) -> io::Result<()> {
        if !self.needs_shutdown() {
            return Err(invalid_progress());
        }
        self.phase = ReceivePhase::Closed;
        Ok(())
    }
}
fn invalid_progress() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid RPC stream progress")
}
