//! Embassy/embedded async IO with borrowed storage. No std, allocator, Send,
//! static socket lifetime, global task, or Tokio compatibility layer is needed.
pub use crate::embedded as tls;
use crate::io::{Completion, Read, Timer, Write};
pub use capntproto_rpc_engine as engine;
pub use capntproto_rpc_wire as wire;
use core::{marker::PhantomData, time::Duration};
#[cfg(feature = "embassy-net")]
pub use embassy_net as net;

pub struct Reader<R, B> {
    io: R,
    buffer: PhantomData<fn(B) -> B>,
}
pub struct Writer<W, B> {
    io: W,
    buffer: PhantomData<fn(B) -> B>,
}
impl<R, B> Reader<R, B> {
    pub fn new(io: R) -> Self {
        Self {
            io,
            buffer: PhantomData,
        }
    }
    pub fn into_inner(self) -> R {
        self.io
    }
}
impl<W, B> Writer<W, B> {
    pub fn new(io: W) -> Self {
        Self {
            io,
            buffer: PhantomData,
        }
    }
    pub fn into_inner(self) -> W {
        self.io
    }
}
impl<R: embedded_io_async::Read, B: AsMut<[u8]>> Read for Reader<R, B> {
    type Buffer = B;
    type Error = R::Error;
    async fn read(&mut self, mut buffer: B) -> Completion<B, R::Error> {
        let result = self.io.read(buffer.as_mut()).await;
        (result, buffer)
    }
}
impl<W: embedded_io_async::Write, B: AsRef<[u8]>> Write for Writer<W, B> {
    type Buffer = B;
    type Error = W::Error;
    async fn write(&mut self, buffer: B) -> Completion<B, W::Error> {
        let result = self.io.write(buffer.as_ref()).await;
        (result, buffer)
    }
    async fn flush(&mut self) -> Result<(), W::Error> {
        self.io.flush().await
    }
}

pub struct Clock(embassy_time::Instant);
impl Clock {
    pub fn new(epoch: embassy_time::Instant) -> Self {
        Self(epoch)
    }
}
impl Timer for Clock {
    fn now(&self) -> Duration {
        Duration::from_micros((embassy_time::Instant::now() - self.0).as_micros())
    }
    async fn wait_until(&self, deadline: Duration) {
        let remaining = deadline.saturating_sub(self.now());
        let micros = remaining.as_nanos().div_ceil(1_000).min(u64::MAX as u128) as u64;
        let at = embassy_time::Duration::try_from_micros(micros)
            .and_then(|duration| embassy_time::Instant::now().checked_add(duration));
        match at {
            Some(at) => embassy_time::Timer::at(at).await,
            None => core::future::pending().await,
        }
    }
}

/// Fill the existing bounded RPC frame receiver through async embedded IO.
/// Each completed prefix is committed before the next await. Framing, segment,
/// alignment and reader-limit checks remain in the shared Cap'n Proto codec.
/// Canceling while the underlying IO itself is pending follows that IO's
/// cancellation contract; TLS scopes must destroy partially processed sessions.
pub async fn receive<R: embedded_io_async::Read>(
    reader: &mut R,
    receiver: &mut capnp::serialize::FrameReceiver<'_>,
) -> Result<bool, wire::io::ReceiveError<R::Error>> {
    use wire::io::ReceiveError;
    let mut turns = 0;
    loop {
        turns += 1;
        if turns == 128 {
            yield_now().await;
            turns = 0;
        }
        let Some(mut window) = receiver.input().map_err(|e| ReceiveError::Wire(e.kind))? else {
            return receiver
                .message()
                .map(|m| m.is_some())
                .map_err(|e| ReceiveError::Wire(e.kind));
        };
        let count = match reader.read(window.bytes()).await {
            Ok(n) => n,
            Err(error) => {
                receiver.abort(capnp::ErrorKind::Disconnected);
                return Err(ReceiveError::Io(error));
            }
        };
        window
            .commit(count)
            .map_err(|e| ReceiveError::Wire(e.kind))?;
    }
}

/// Send an already framed message without intermediate allocation. A short
/// write advances only its accepted prefix. Flush is explicit to permit RPC
/// batching. Callers retain frame/storage ownership until this future completes.
#[derive(Debug, PartialEq, Eq)]
pub enum SendError<E> {
    Io(E),
    WriteZero,
    InvalidCount,
}
/// Caller-owned progress for a send that may be canceled between completed IO
/// operations. Keep this cursor and resume it instead of resending the prefix.
/// The underlying writer must itself support cancellation; embedded TLS sessions
/// must be discarded if cancellation interrupts an operation inside that engine.
pub struct SendCursor<'a> {
    remaining: &'a [u8],
}
impl<'a> SendCursor<'a> {
    pub fn new(frame: &'a [u8]) -> Self {
        Self { remaining: frame }
    }
    pub fn remaining(&self) -> &'a [u8] {
        self.remaining
    }
}

pub async fn send_pending<W: embedded_io_async::Write>(
    writer: &mut W,
    cursor: &mut SendCursor<'_>,
) -> Result<(), SendError<W::Error>> {
    let mut turns = 0;
    while !cursor.remaining.is_empty() {
        let n = writer
            .write(cursor.remaining)
            .await
            .map_err(SendError::Io)?;
        if n == 0 {
            return Err(SendError::WriteZero);
        }
        if n > cursor.remaining.len() {
            return Err(SendError::InvalidCount);
        }
        cursor.remaining = &cursor.remaining[n..];
        turns += 1;
        if turns == 128 {
            yield_now().await;
            turns = 0;
        }
    }
    Ok(())
}

/// Convenience for an uninterrupted send. For cancellation and resumption use
/// send_pending with a caller-owned SendCursor. Flush remains explicit.
pub async fn send<W: embedded_io_async::Write>(
    writer: &mut W,
    frame: &[u8],
) -> Result<(), SendError<W::Error>> {
    send_pending(writer, &mut SendCursor::new(frame)).await
}

async fn yield_now() {
    let mut yielded = false;
    core::future::poll_fn(|cx| {
        if core::mem::replace(&mut yielded, true) {
            core::task::Poll::Ready(())
        } else {
            cx.waker().wake_by_ref();
            core::task::Poll::Pending
        }
    })
    .await
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use core::{
        future::Future,
        pin::{pin, Pin},
        task::{Context, Poll},
    };
    use embedded_io_async::{ErrorType, Read, Write};
    struct Io {
        bytes: [u8; 512],
        position: usize,
        limit: usize,
        mode: u8,
    }
    impl ErrorType for Io {
        type Error = embedded_io::ErrorKind;
    }
    impl Write for Io {
        async fn write(&mut self, bytes: &[u8]) -> Result<usize, Self::Error> {
            if self.position >= self.limit {
                core::future::pending::<()>().await;
            }
            let n = match self.mode {
                1 => 0,
                2 => bytes.len() + 1,
                _ => bytes.len().min(1),
            };
            if n <= bytes.len() {
                self.bytes[self.position..self.position + n].copy_from_slice(&bytes[..n]);
                self.position += n;
            }
            Ok(n)
        }
        async fn flush(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }
    impl Read for Io {
        async fn read(&mut self, bytes: &mut [u8]) -> Result<usize, Self::Error> {
            if self.mode == 3 {
                return Err(embedded_io::ErrorKind::Other);
            }
            if self.position >= self.limit {
                return Ok(0);
            }
            let n = bytes.len().min(self.limit - self.position).min(1);
            bytes[..n].copy_from_slice(&self.bytes[self.position..self.position + n]);
            self.position += n;
            Ok(n)
        }
    }
    fn poll<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
        future.poll(&mut Context::from_waker(std::task::Waker::noop()))
    }
    fn io() -> Io {
        Io {
            bytes: [0; 512],
            position: 0,
            limit: 512,
            mode: 0,
        }
    }
    #[test]
    fn partial_send_can_be_canceled_and_resumed_without_repeating_bytes() {
        let mut writer = io();
        writer.limit = 3;
        let mut cursor = SendCursor::new(b"abcdef");
        {
            let mut sending = pin!(send_pending(&mut writer, &mut cursor));
            assert!(poll(sending.as_mut()).is_pending());
        }
        assert_eq!(cursor.remaining(), b"def");
        writer.limit = 512;
        assert_eq!(
            poll(pin!(send_pending(&mut writer, &mut cursor))),
            Poll::Ready(Ok(()))
        );
        assert_eq!(&writer.bytes[..writer.position], b"abcdef");
    }
    #[test]
    fn sends_reject_zero_and_excess_counts_and_cooperate_without_allocating() {
        for (mode, error) in [(1, SendError::WriteZero), (2, SendError::InvalidCount)] {
            let mut writer = io();
            writer.mode = mode;
            assert_eq!(
                poll(pin!(send(&mut writer, b"abc"))),
                Poll::Ready(Err(error))
            );
        }
        let mut writer = io();
        let bytes = [7; 257];
        let measured = allocation_counter::measure(|| {
            let mut sending = pin!(send(&mut writer, &bytes));
            assert!(poll(sending.as_mut()).is_pending());
            assert!(poll(sending.as_mut()).is_pending());
            assert_eq!(poll(sending.as_mut()), Poll::Ready(Ok(())));
        });
        assert_eq!(measured.count_total, 0);
        assert_eq!(writer.position, 257);
    }
    #[test]
    fn receiver_uses_shared_framing_limits_and_rejects_truncation() {
        #[repr(align(8))]
        struct Aligned([u8; 64]);
        for limit in [0, 1, 7, 8, 15, 16] {
            let mut reader = io();
            reader.bytes[4] = 1;
            reader.limit = limit;
            let mut storage = Aligned([0; 64]);
            let mut frame =
                capnp::serialize::FrameReceiver::new(&mut storage.0, Default::default()).unwrap();
            let result = poll(pin!(receive(&mut reader, &mut frame)));
            match limit {
                0 => assert_eq!(result, Poll::Ready(Ok(false))),
                16 => assert_eq!(result, Poll::Ready(Ok(true))),
                _ => assert!(matches!(
                    result,
                    Poll::Ready(Err(wire::io::ReceiveError::Wire(_)))
                )),
            }
        }
        let mut reader = io();
        reader.mode = 3;
        let mut storage = Aligned([0; 64]);
        let mut frame =
            capnp::serialize::FrameReceiver::new(&mut storage.0, Default::default()).unwrap();
        assert!(matches!(
            poll(pin!(receive(&mut reader, &mut frame))),
            Poll::Ready(Err(wire::io::ReceiveError::Io(_)))
        ));
        assert!(frame.message().is_err());
    }
}

/// Embassy TCP buffers stay borrowed for the connection lifetime. The caller
/// must poll the network stack's Runner independently, as for any Embassy socket.
#[cfg(feature = "embassy-net")]
pub struct TcpBuffers<'a> {
    pub receive: &'a mut [u8],
    pub transmit: &'a mut [u8],
}
#[cfg(feature = "embassy-net")]
#[derive(Debug)]
pub enum ConnectError {
    Tcp(net::tcp::ConnectError),
    Tls(tls::TlsError),
}

/// Connect TCP and authenticate TLS without tasks or allocation. Address
/// resolution, entropy, certificate time, storage and the whole-operation
/// deadline belong to the caller. Canceling requires dropping the connection.
#[cfg(feature = "embassy-net")]
pub async fn connect<'a, R: tls::CryptoRngCore>(
    stack: net::Stack<'a>,
    peer: net::IpEndpoint,
    config: &tls::ClientConfig<'_>,
    rng: R,
    tcp_buffers: TcpBuffers<'a>,
    tls_buffers: tls::Buffers<'a, '_>,
) -> Result<tls::TlsConnection<'a, net::tcp::TcpSocket<'a>, tls::Aes128GcmSha256>, ConnectError> {
    let mut socket = net::tcp::TcpSocket::new(stack, tcp_buffers.receive, tcp_buffers.transmit);
    socket.connect(peer).await.map_err(ConnectError::Tcp)?;
    tls::connect(socket, config, rng, tls_buffers)
        .await
        .map_err(ConnectError::Tls)
}
