use super::*;
use futures::{AsyncRead as _, AsyncWrite as _};
use std::task::Waker;

#[derive(Default)]
struct Probe {
    calls: usize,
    bytes: Vec<u8>,
    flushes: usize,
    closes: usize,
    pending: bool,
}
impl AsyncWrite for Probe {
    fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, _: &[u8]) -> Poll<io::Result<usize>> {
        panic!("vectored writes must not fall back to scalar writes")
    }
    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffers: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        self.calls += 1;
        if std::mem::take(&mut self.pending) {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        // Complete in the middle of the second nonempty slice.
        let mut count = 0;
        for buffer in buffers {
            let size = buffer.len().min(5 - count);
            self.bytes.extend_from_slice(&buffer[..size]);
            count += size;
        }
        Poll::Ready(Ok(count))
    }
    fn poll_flush(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.flushes += 1;
        Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()))
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.closes += 1;
        Poll::Ready(Ok(()))
    }
}
impl AsyncRead for Probe {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if std::mem::take(&mut self.pending) {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        let size = self.bytes.len().min(buffer.remaining());
        buffer.put_slice(&self.bytes[..size]);
        self.bytes.drain(..size);
        Poll::Ready(Ok(()))
    }
}
#[test]
fn vectored_writes_preserve_pending_partial_counts_empty_slices_and_lifecycle_errors() {
    let mut io = FuturesIo::new(Probe {
        pending: true,
        ..Probe::default()
    });
    let mut cx = Context::from_waker(Waker::noop());
    let buffers = [
        io::IoSlice::new(&[]),
        io::IoSlice::new(b"abc"),
        io::IoSlice::new(b"defgh"),
    ];
    assert!(Pin::new(&mut io)
        .poll_write_vectored(&mut cx, &buffers)
        .is_pending());
    assert_eq!(
        Pin::new(&mut io)
            .poll_write_vectored(&mut cx, &buffers)
            .map(|r| r.unwrap()),
        Poll::Ready(5)
    );
    assert_eq!(io.get_ref().calls, 2);
    assert_eq!(io.get_ref().bytes, b"abcde");
    assert_eq!(
        Pin::new(&mut io)
            .poll_write_vectored(&mut cx, &[])
            .map(|r| r.unwrap()),
        Poll::Ready(0)
    );
    let Poll::Ready(Err(error)) = Pin::new(&mut io).poll_flush(&mut cx) else {
        panic!("flush failure was hidden")
    };
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert!(matches!(
        Pin::new(&mut io).poll_close(&mut cx),
        Poll::Ready(Ok(()))
    ));
    let probe = io.into_inner();
    assert_eq!((probe.flushes, probe.closes), (1, 1));
}
#[test]
fn reads_preserve_pending_short_reads_zero_length_and_eof() {
    let mut io = FuturesIo::new(Probe {
        pending: true,
        bytes: b"abc".to_vec(),
        ..Probe::default()
    });
    let mut cx = Context::from_waker(Waker::noop());
    let mut buffer = [0; 2];
    assert!(Pin::new(&mut io)
        .poll_read(&mut cx, &mut buffer)
        .is_pending());
    assert_eq!(
        Pin::new(&mut io)
            .poll_read(&mut cx, &mut buffer)
            .map(|r| r.unwrap()),
        Poll::Ready(2)
    );
    assert_eq!(&buffer, b"ab");
    assert_eq!(
        Pin::new(&mut io)
            .poll_read(&mut cx, &mut [])
            .map(|r| r.unwrap()),
        Poll::Ready(0)
    );
    assert_eq!(
        Pin::new(&mut io)
            .poll_read(&mut cx, &mut buffer)
            .map(|r| r.unwrap()),
        Poll::Ready(1)
    );
    assert_eq!(buffer[0], b'c');
    assert_eq!(
        Pin::new(&mut io)
            .poll_read(&mut cx, &mut buffer)
            .map(|r| r.unwrap()),
        Poll::Ready(0)
    );
    io.get_mut().bytes.push(b'd');
    assert_eq!(
        Pin::new(&mut io)
            .poll_read(&mut cx, &mut buffer)
            .map(|r| r.unwrap()),
        Poll::Ready(1)
    );
    assert_eq!(buffer[0], b'd');
}
