//! Capability-negotiated bulk streams on an authenticated native QUIC session.
//!
//! Stream grants travel through authorized RPC methods. This module never
//! interprets a stream ID as authority and never reorders capability RPC bytes.
#![forbid(unsafe_code)]

use crate::rpc::local_io;
use std::{
    cell::{Cell, RefCell},
    future::Future,
    io,
    pin::Pin,
    rc::{Rc, Weak},
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    sync::Notify,
    time::Instant,
};

mod driver;
#[cfg(test)]
mod tests;
mod wire;
pub(super) use driver::{pair, Driver};
pub use wire::Offer;

const MAX_ACTIVE: usize = 8;
const BUFFER: usize = 16 * 1024;
const MAX_OUTSTANDING: u64 = 256 * 1024;
const CONTROL_RESERVE: u64 = 64 * 1024;
pub const MAX_TRANSFER_BYTES: u64 = 64 * 1024 * 1024;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn closed() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "bulk session closed")
}

#[derive(Clone)]
enum Outcome {
    Active,
    Complete,
    Failed(io::ErrorKind, String),
}
struct Progress {
    outcome: RefCell<Outcome>,
    consumed: Cell<u64>,
    input_finished: Cell<bool>,
    canceled: Cell<bool>,
    changed: Notify,
    wake: Arc<Notify>,
}
impl Progress {
    fn expire(&self) {
        self.finish(Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "bulk transfer deadline elapsed",
        )));
        self.cancel();
    }
    fn poll_deadline(
        &self,
        timer: Pin<&mut tokio::time::Sleep>,
        cx: &mut Context<'_>,
    ) -> io::Result<()> {
        let active = matches!(*self.outcome.borrow(), Outcome::Active);
        if active && timer.poll(cx).is_ready() {
            self.expire();
        }
        self.check()
    }
    fn new(wake: Arc<Notify>) -> Rc<Self> {
        Rc::new(Self {
            outcome: RefCell::new(Outcome::Active),
            consumed: Cell::new(0),
            input_finished: Cell::new(false),
            canceled: Cell::new(false),
            changed: Notify::new(),
            wake,
        })
    }
    fn check(&self) -> io::Result<()> {
        match &*self.outcome.borrow() {
            Outcome::Failed(kind, message) => Err(io::Error::new(*kind, message.clone())),
            _ => Ok(()),
        }
    }
    fn finish(&self, result: io::Result<()>) {
        if !matches!(*self.outcome.borrow(), Outcome::Active) {
            return;
        }
        *self.outcome.borrow_mut() = match result {
            Ok(()) => Outcome::Complete,
            Err(e) => Outcome::Failed(e.kind(), e.to_string()),
        };
        self.changed.notify_waiters();
    }
    async fn wait(&self) -> io::Result<()> {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            self.check()?;
            if matches!(*self.outcome.borrow(), Outcome::Complete) {
                return Ok(());
            }
            changed.await;
        }
    }
    fn cancel(&self) {
        self.canceled.set(true);
        self.wake.notify_one();
    }
}

struct Shared {
    control: crate::native_shutdown::Control,
    live: bool,
    closing: bool,
    binding: Option<[u8; 32]>,
    next_receive: u64,
    server: bool,
    sent_ids: wire::IdWindow,
    active: usize,
    pending: Vec<driver::Entry>,
    wake: Arc<Notify>,
    stats: Stats,
}

/// Payload counters exclude grants, framing and consumption receipts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Stats {
    pub active: usize,
    pub sent_bytes: u64,
    pub received_bytes: u64,
    pub outstanding_bytes: u64,
    pub abandoned_bytes: u64,
}

/// A weak handle to one authenticated session. Reconnect creates a new binding.
#[derive(Clone)]
pub struct Plane(Weak<RefCell<Shared>>);
impl Plane {
    pub fn stats(&self) -> Option<Stats> {
        let shared = self.0.upgrade()?;
        let state = shared.borrow();
        Some(Stats {
            active: state.active,
            ..state.stats
        })
    }
    fn shared(&self) -> io::Result<Rc<RefCell<Shared>>> {
        let shared = self.0.upgrade().ok_or_else(closed)?;
        {
            let state = shared.borrow();
            if !state.live || state.closing || !state.control.accepting() {
                return Err(closed());
            }
            if state.binding.is_none() {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "peer has insufficient bulk/control credit",
                ));
            }
            if state.active == MAX_ACTIVE {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "bulk stream limit reached",
                ));
            }
        }
        Ok(shared)
    }

    /// Issue one receive grant. Deliver the offer only through an authorized RPC.
    /// Dropping the reader revokes the grant. The deadline bounds stalled peers.
    pub fn receive(&self, length: u64, timeout: Duration) -> io::Result<(Offer, Receive)> {
        let deadline = deadline(length, timeout)?;
        let shared = self.shared()?;
        let (offer, progress) = {
            let mut state = shared.borrow_mut();
            let id = state.next_receive;
            state.next_receive = id
                .checked_add(2)
                .filter(|id| *id < (1 << 62))
                .ok_or_else(|| invalid("bulk stream IDs exhausted"))?;
            let offer = Offer::new(state.binding.unwrap(), id, length);
            (offer, Progress::new(state.wake.clone()))
        };
        let (app, driver) = local_io::pair(BUFFER);
        let (read, unused) = app.into_split();
        drop(unused);
        let (unused, write) = driver.into_split();
        drop(unused);
        let entry = driver::Entry::receive(&offer, write, progress.clone(), deadline);
        {
            let mut state = shared.borrow_mut();
            state.active += 1;
            state.pending.push(entry);
        }
        progress.wake.notify_one();
        Ok((
            offer,
            Receive {
                read,
                progress,
                length,
                expiry: Box::pin(tokio::time::sleep_until(deadline)),
            },
        ))
    }

    /// Consume a peer's offer. Reject cross-session grants, replay and reserved IDs.
    pub fn send(&self, offer: Offer, timeout: Duration) -> io::Result<Send> {
        let deadline = deadline(offer.length, timeout)?;
        let shared = self.shared()?;
        let progress = {
            let mut state = shared.borrow_mut();
            if Some(offer.binding) != state.binding {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "bulk grant belongs to another session",
                ));
            }
            if offer.id % 2 == u64::from(state.server) {
                return Err(invalid("bulk grant has wrong direction"));
            }
            state.sent_ids.claim(offer.id / 2)?;
            Progress::new(state.wake.clone())
        };
        let (app, driver) = local_io::pair(BUFFER);
        let (unused, write) = app.into_split();
        drop(unused);
        let (read, unused) = driver.into_split();
        drop(unused);
        let length = offer.length;
        let entry = driver::Entry::send(offer, read, progress.clone(), deadline);
        {
            let mut state = shared.borrow_mut();
            state.active += 1;
            state.pending.push(entry);
        }
        progress.wake.notify_one();
        Ok(Send {
            write,
            progress,
            length,
            written: 0,
            expiry: Box::pin(tokio::time::sleep_until(deadline)),
        })
    }
}
fn deadline(length: u64, timeout: Duration) -> io::Result<Instant> {
    if length > MAX_TRANSFER_BYTES || timeout.is_zero() || timeout > Duration::from_secs(300) {
        return Err(invalid("invalid bulk length or deadline"));
    }
    Ok(Instant::now() + timeout)
}

/// One ordered producer, with a fixed declared length and bounded buffering.
#[must_use = "write the declared bytes and finish, or drop to cancel"]
pub struct Send {
    expiry: Pin<Box<tokio::time::Sleep>>,
    write: local_io::WriteHalf,
    progress: Rc<Progress>,
    length: u64,
    written: u64,
}
impl Send {
    /// Seal the stream and await peer consumption. This does not acknowledge
    /// application execution or durable commit; those require an RPC receipt.
    pub async fn finish(mut self) -> io::Result<()> {
        use tokio::io::AsyncWriteExt;
        self.shutdown().await?;
        tokio::select! {
            biased;
            result = self.progress.wait() => result,
            _ = &mut self.expiry => { self.progress.expire(); self.progress.check() },
        }
    }
}
impl AsyncWrite for Send {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.as_mut().get_mut();
        this.progress.poll_deadline(this.expiry.as_mut(), cx)?;
        if bytes.len() as u64 > self.length - self.written {
            return Poll::Ready(Err(invalid("bulk length exceeded")));
        }
        let count = std::task::ready!(Pin::new(&mut self.write).poll_write(cx, bytes))?;
        self.written += count as u64;
        Poll::Ready(Ok(count))
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.as_mut().get_mut();
        this.progress.poll_deadline(this.expiry.as_mut(), cx)?;
        Pin::new(&mut self.write).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.as_mut().get_mut();
        this.progress.poll_deadline(this.expiry.as_mut(), cx)?;
        if self.written != self.length {
            return Poll::Ready(Err(invalid("incomplete bulk stream")));
        }
        Pin::new(&mut self.write).poll_shutdown(cx)
    }
}
impl Drop for Send {
    fn drop(&mut self) {
        if !matches!(*self.progress.outcome.borrow(), Outcome::Complete) {
            self.progress.cancel();
        }
    }
}

/// A grant's sole consumer. Credit returns only as this reader consumes bytes.
#[must_use = "consume the transfer or drop to revoke its grant"]
pub struct Receive {
    expiry: Pin<Box<tokio::time::Sleep>>,
    read: local_io::ReadHalf,
    progress: Rc<Progress>,
    length: u64,
}
impl AsyncRead for Receive {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.as_mut().get_mut();
        this.progress.poll_deadline(this.expiry.as_mut(), cx)?;
        let before = out.filled().len();
        std::task::ready!(Pin::new(&mut self.read).poll_read(cx, out))?;
        let count = out.filled().len() - before;
        if count != 0 {
            self.progress
                .consumed
                .set(self.progress.consumed.get() + count as u64);
            self.progress.wake.notify_one();
        }
        Poll::Ready(Ok(()))
    }
}
impl Drop for Receive {
    fn drop(&mut self) {
        if self.progress.consumed.get() != self.length || !self.progress.input_finished.get() {
            self.progress.cancel();
        }
    }
}
