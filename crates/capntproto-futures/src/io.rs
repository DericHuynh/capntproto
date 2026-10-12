//! Adapt hosted futures IO to the same polling interfaces as bounded RPC.
#![forbid(unsafe_code)]
use capntproto_rpc_engine::io::{self, IoError};
use core::{
    pin::Pin,
    task::{Context, Poll},
};
use futures_util::{AsyncRead, AsyncWrite};

pub struct Adapter<T>(pub T);
fn compact(error: std::io::Error) -> IoError<std::io::ErrorKind> {
    if error.kind() == std::io::ErrorKind::Interrupted {
        IoError::Interrupted
    } else {
        IoError::Failed(error.kind())
    }
}
impl<R: AsyncRead + Unpin> io::Read for Adapter<R> {
    type Error = std::io::ErrorKind;
    fn poll_read(
        &mut self,
        cx: &mut Context<'_>,
        bytes: &mut [u8],
    ) -> Poll<Result<usize, IoError<Self::Error>>> {
        Pin::new(&mut self.0).poll_read(cx, bytes).map_err(compact)
    }
}
impl<W: AsyncWrite + Unpin> io::Flush for Adapter<W> {
    type Error = std::io::ErrorKind;
    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), IoError<Self::Error>>> {
        Pin::new(&mut self.0).poll_flush(cx).map_err(compact)
    }
}
impl<W: AsyncWrite + Unpin> io::Close for Adapter<W> {
    fn poll_close(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), IoError<Self::Error>>> {
        Pin::new(&mut self.0).poll_close(cx).map_err(compact)
    }
}
impl<W: AsyncWrite + Unpin> io::Write for Adapter<W> {
    fn poll_write(
        &mut self,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<Result<usize, IoError<Self::Error>>> {
        Pin::new(&mut self.0).poll_write(cx, bytes).map_err(compact)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc, task::Waker};
    #[derive(Default)]
    struct State {
        bytes: Vec<u8>,
        flushes: usize,
        closes: usize,
    }
    struct Sink(Rc<RefCell<State>>);
    impl AsyncWrite for Sink {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            self.0.borrow_mut().bytes.extend_from_slice(bytes);
            Poll::Ready(Ok(bytes.len()))
        }
        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            let mut state = self.0.borrow_mut();
            state.flushes += 1;
            match state.flushes {
                1 => {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                2 => Poll::Ready(Err(std::io::ErrorKind::Interrupted.into())),
                _ => Poll::Ready(Ok(())),
            }
        }
        fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            let mut state = self.0.borrow_mut();
            state.closes += 1;
            match state.closes {
                1 => {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                2 => Poll::Ready(Err(std::io::ErrorKind::Interrupted.into())),
                3 => Poll::Ready(Err(std::io::ErrorKind::BrokenPipe.into())),
                _ => Poll::Ready(Ok(())),
            }
        }
    }

    #[test]
    fn buffered_adapter_forwards_flush_close_pending_and_errors_without_repeating_bytes() {
        let state = Rc::new(RefCell::new(State::default()));
        let mut adapter = Adapter(futures_util::io::BufWriter::with_capacity(
            32,
            Sink(state.clone()),
        ));
        let mut cx = Context::from_waker(Waker::noop());
        assert_eq!(
            io::Write::poll_write(&mut adapter, &mut cx, b"request"),
            Poll::Ready(Ok(7))
        );
        assert!(
            state.borrow().bytes.is_empty(),
            "fixture must actually buffer"
        );
        assert_eq!(io::Flush::poll_flush(&mut adapter, &mut cx), Poll::Pending);
        assert_eq!(
            io::Flush::poll_flush(&mut adapter, &mut cx),
            Poll::Ready(Err(IoError::Interrupted))
        );
        assert_eq!(
            io::Flush::poll_flush(&mut adapter, &mut cx),
            Poll::Ready(Ok(()))
        );
        assert_eq!(io::Close::poll_close(&mut adapter, &mut cx), Poll::Pending);
        assert_eq!(
            io::Close::poll_close(&mut adapter, &mut cx),
            Poll::Ready(Err(IoError::Interrupted))
        );
        assert_eq!(
            io::Close::poll_close(&mut adapter, &mut cx),
            Poll::Ready(Err(IoError::Failed(std::io::ErrorKind::BrokenPipe)))
        );
        assert_eq!(
            io::Close::poll_close(&mut adapter, &mut cx),
            Poll::Ready(Ok(()))
        );
        assert_eq!(state.borrow().bytes, b"request");
        assert_eq!(state.borrow().closes, 4);
    }
}
