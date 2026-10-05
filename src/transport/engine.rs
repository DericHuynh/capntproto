//! Synchronous connection transitions. The adapter supplies time, application
//! read/write completions and packets; it performs all socket IO and waiting.
#![forbid(unsafe_code)]

use super::{
    error, scheduling, shutdown,
    stream::{ReceiveStream, SendStream},
};
use crate::native_shutdown::{Control, Receipt};
use crate::semantics::{NativeStreamGate, StreamRole};
use std::io;
use tokio::time::Instant;

pub(super) struct Engine {
    pub conn: Box<quiche::Connection>,
    pub tx: SendStream,
    pub rx: ReceiveStream,
    pub shutdown: Option<shutdown::ShutdownDriver>,
    pub scheduling: scheduling::Driver,
    gate: Option<NativeStreamGate>,
    pending_datagram: Option<Vec<u8>>,
    phase: Phase,
}
enum Phase {
    Running,
    // A bounded packet burst may yield after sending CONNECTION_CLOSE. Quiche
    // then reports draining, but the validated receipt must survive that turn.
    Flushing { receipt: Receipt, control: Control },
}
impl Engine {
    pub(super) fn new(
        conn: Box<quiche::Connection>,
        authenticate_stream: bool,
        shutdown: Option<shutdown::ShutdownDriver>,
        scheduling: scheduling::Driver,
    ) -> Self {
        let gate = authenticate_stream.then(|| {
            NativeStreamGate::new(if conn.is_server() {
                StreamRole::Responder
            } else {
                StreamRole::Initiator
            })
        });
        Self {
            conn,
            tx: SendStream::default(),
            rx: ReceiveStream::default(),
            shutdown,
            scheduling,
            gate,
            pending_datagram: None,
            phase: Phase::Running,
        }
    }
    pub(super) fn ready(&self) -> bool {
        self.conn.is_established() && self.gate.as_ref().is_none_or(|g| g.ready())
    }
    pub(super) fn closing(&self) -> bool {
        self.shutdown.as_ref().is_some_and(|s| s.closing())
    }
    pub(super) fn can_accept_datagram(&self) -> bool {
        self.ready() && !self.closing() && self.pending_datagram.is_none()
    }
    pub(super) fn datagram(&mut self, bytes: Vec<u8>) -> io::Result<()> {
        // A shared shutdown control can change while the adapter awaits queue
        // readiness. Closing discards unreliable work instead of failing RPC.
        if self.closing() {
            return Ok(());
        }
        if !self.can_accept_datagram() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unexpected queued datagram",
            ));
        }
        self.pending_datagram = Some(bytes);
        Ok(())
    }
    pub(super) fn datagram_deadline(&self, now: impl FnOnce() -> Instant) -> Option<Instant> {
        self.pending_datagram.as_ref().map(|_| {
            let now = now();
            self.scheduling.deadline(now).unwrap_or(now)
        })
    }
    /// Report output exhaustion, not merely consumption of one packet burst.
    pub(super) fn packets_drained(&self) -> bool {
        if let Phase::Flushing { receipt, control } = &self.phase {
            control.finish(Ok(*receipt));
            true
        } else {
            false
        }
    }
    /// False means the connection has terminated. The adapter supplies a Tokio
    /// clock for application deadlines; upstream Quiche uses the system clock.
    /// Read it only when a datagram needs admission, not for reliable traffic.
    pub(super) fn step(&mut self, now: impl FnOnce() -> Instant) -> io::Result<bool> {
        if matches!(self.phase, Phase::Flushing { .. }) {
            return Ok(true);
        }
        if self.conn.is_closed() || self.conn.is_draining() {
            if let Some(shutdown) = &self.shutdown {
                shutdown.finish_on_close(&self.conn);
            }
            return if self.conn.is_timed_out() {
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Native session timed out",
                ))
            } else {
                Ok(false)
            };
        }
        if !self.conn.is_established() {
            return Ok(true);
        }
        if let Some(gate) = &mut self.gate {
            gate.authenticate();
        }
        if self.gate.as_ref().is_some_and(|g| g.needs_send()) {
            match self.conn.stream_send(0, b"R", false) {
                Ok(1) => {
                    assert!(self.gate.as_mut().unwrap().sent());
                }
                Ok(_) | Err(quiche::Error::Done) => (),
                Err(e) => return Err(error(e)),
            }
        }
        let graceful = self.shutdown.as_ref().is_some_and(|s| s.requested());
        if self.ready() {
            if let Some((bytes, fin)) = self.tx.pending(graceful) {
                match self.conn.stream_send(0, bytes, fin) {
                    Ok(n) => self.tx.sent(n)?,
                    Err(quiche::Error::Done) => (),
                    Err(e) => return Err(error(e)),
                }
            }
        }
        if self.rx.can_receive() {
            match self.conn.stream_recv(0, self.rx.receive_buffer()?) {
                Ok((n, fin)) => {
                    let skip = if self.gate.as_ref().is_some_and(|g| g.needs_receive()) && n > 0 {
                        if !self
                            .gate
                            .as_mut()
                            .unwrap()
                            .receive(self.rx.receive_buffer()?[0])
                        {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "invalid native RPC stream preface",
                            ));
                        }
                        1
                    } else {
                        0
                    };
                    if fin && !self.ready() {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "missing native RPC stream preface",
                        ));
                    }
                    self.rx.received(n, fin, skip)?;
                }
                Err(quiche::Error::Done) | Err(quiche::Error::InvalidStreamState(_)) => (),
                Err(e) => return Err(error(e)),
            }
        }
        if self.ready() {
            if let Some(shutdown) = &mut self.shutdown {
                shutdown.step(&mut self.conn, self.tx.written(), self.tx.drained())?;
                if shutdown.closing() {
                    self.pending_datagram = None;
                }
                if shutdown.acknowledged() {
                    self.conn
                        .close(true, shutdown::ACKNOWLEDGED_CLOSE, &shutdown.close_reason())
                        .map_err(error)?;
                    self.phase = Phase::Flushing {
                        receipt: Receipt {
                            bytes: self.tx.written(),
                        },
                        control: shutdown.control.clone(),
                    };
                }
            }
        }
        if self.pending_datagram.is_some() && self.scheduling.admit(now()) {
            let bytes = self.pending_datagram.take().unwrap();
            match self.conn.dgram_send(&bytes) {
                Ok(())
                | Err(quiche::Error::Done)
                | Err(quiche::Error::BufferTooShort)
                | Err(quiche::Error::InvalidState) => (),
                Err(e) => return Err(error(e)),
            }
        }
        Ok(true)
    }
    pub(super) fn delivered(&mut self, count: usize) -> io::Result<()> {
        self.rx.delivered(count)?;
        if let Some(shutdown) = &mut self.shutdown {
            shutdown.delivered(count)?;
        }
        Ok(())
    }
}
