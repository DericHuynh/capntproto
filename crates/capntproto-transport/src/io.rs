//! Completion IO contracts shared by runtime adapters. Buffers travel with an
//! operation and are returned on both success and failure. They need not be
//! allocated, Send, or static: those restrictions belong to individual adapters.
use core::net::SocketAddr;
use core::{future::Future, time::Duration};

pub type Completion<B, E> = (Result<usize, E>, B);

/// A receive half. The returned count describes initialized bytes in Buffer.
/// An adapter must retain ownership until completion or safe cancellation;
/// dropping an operation must never leave the device accessing released memory.
pub trait Read {
    type Buffer: AsMut<[u8]>;
    type Error;
    fn read(
        &mut self,
        buffer: Self::Buffer,
    ) -> impl Future<Output = Completion<Self::Buffer, Self::Error>>;
}

/// A transmit half, separate from Read so both directions can progress under
/// backpressure. A short write accepts only the returned prefix. Completion is
/// local IO acceptance, not peer acknowledgement.
pub trait Write {
    type Buffer: AsRef<[u8]>;
    type Error;
    fn write(
        &mut self,
        buffer: Self::Buffer,
    ) -> impl Future<Output = Completion<Self::Buffer, Self::Error>>;
    fn flush(&mut self) -> impl Future<Output = Result<(), Self::Error>>;
}

/// Optional physical write-half closure. Embedded writers which only support
/// flush do not falsely claim this capability.
pub trait Close: Write {
    fn close(&mut self) -> impl Future<Output = Result<(), Self::Error>>;
}

/// Monotonic elapsed time in a caller-selected epoch. This is deliberately
/// distinct from TlsClock's Unix certificate-verification time. Neither a global
/// clock nor an executor is selected by this trait.
pub trait Timer {
    fn now(&self) -> Duration;
    fn wait_until(&self, deadline: Duration) -> impl Future<Output = ()>;
}

/// One received datagram. UDP completion must preserve packet boundaries.
pub struct Datagram {
    pub length: usize,
    pub peer: SocketAddr,
}
pub trait DatagramRead {
    type Buffer: AsMut<[u8]>;
    type Error;
    fn receive(
        &mut self,
        buffer: Self::Buffer,
    ) -> impl Future<Output = (Result<Datagram, Self::Error>, Self::Buffer)>;
}
pub trait DatagramWrite {
    type Buffer: AsRef<[u8]>;
    type Error;
    fn transmit(
        &mut self,
        buffer: Self::Buffer,
        peer: SocketAddr,
    ) -> impl Future<Output = Completion<Self::Buffer, Self::Error>>;
}
