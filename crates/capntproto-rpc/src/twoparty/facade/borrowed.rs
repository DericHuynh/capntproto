//! A scoped IO pump. Only owned buffers and wakers enter the `'static` RPC tasks;
//! the stream itself never leaves the caller's borrowed driver. At most one
//! operation per direction and 16 KiB per direction can be outstanding.
use futures::{AsyncRead, AsyncWrite};
use std::{
    cell::RefCell,
    io,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

const BUFFER_SIZE: usize = 16 * 1024;

pub(super) trait Pump {
    /// Return whether any operation completed, allowing RPC to be polled again.
    fn poll_io(&mut self, cx: &mut Context<'_>) -> bool;
}

enum WriteOp {
    Write,
    Flush,
    Close,
}

struct State {
    alive: bool,
    driver: Option<Waker>,
    reader: Option<Waker>,
    writer: Option<Waker>,
    read: Vec<u8>,
    read_len: usize,
    read_result: Option<io::Result<usize>>,
    read_offset: usize,
    write: Vec<u8>,
    write_op: Option<WriteOp>,
    write_result: Option<io::Result<usize>>,
}

pub(super) struct Proxy(Rc<RefCell<State>>);
struct Borrowed<'a, T> {
    stream: &'a mut T,
    state: Rc<RefCell<State>>,
}

pub(super) fn new<T: AsyncRead + AsyncWrite + Unpin>(stream: &mut T) -> (Proxy, impl Pump + '_) {
    let state = Rc::new(RefCell::new(State {
        alive: true,
        driver: None,
        reader: None,
        writer: None,
        read: vec![0; BUFFER_SIZE],
        read_len: 0,
        read_result: None,
        read_offset: 0,
        write: Vec::with_capacity(BUFFER_SIZE),
        write_op: None,
        write_result: None,
    }));
    (Proxy(state.clone()), Borrowed { stream, state })
}

fn canceled() -> io::Error {
    io::Error::new(io::ErrorKind::NotConnected, "borrowed RPC driver canceled")
}

impl AsyncRead for Proxy {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let mut state = self.0.borrow_mut();
        if !state.alive {
            return Poll::Ready(Err(canceled()));
        }
        if let Some(result) = state.read_result.take() {
            return Poll::Ready(match result {
                Err(error) => Err(error),
                Ok(end) => {
                    let count = buf.len().min(end - state.read_offset);
                    buf[..count]
                        .copy_from_slice(&state.read[state.read_offset..state.read_offset + count]);
                    state.read_offset += count;
                    if state.read_offset < end {
                        state.read_result = Some(Ok(end));
                    }
                    Ok(count)
                }
            });
        }
        state.reader = Some(cx.waker().clone());
        let wake = if state.read_len == 0 {
            state.read_len = buf.len().min(BUFFER_SIZE);
            state.driver.clone()
        } else {
            None
        };
        drop(state);
        if let Some(wake) = wake {
            wake.wake();
        }
        Poll::Pending
    }
}

impl Proxy {
    fn write_operation(
        &self,
        cx: &mut Context<'_>,
        op: WriteOp,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut state = self.0.borrow_mut();
        if !state.alive {
            return Poll::Ready(Err(canceled()));
        }
        if let Some(result) = state.write_result.take() {
            return Poll::Ready(result);
        }
        state.writer = Some(cx.waker().clone());
        let wake = if state.write_op.is_none() {
            state.write.clear();
            state
                .write
                .extend_from_slice(&buf[..buf.len().min(BUFFER_SIZE)]);
            state.write_op = Some(op);
            state.driver.clone()
        } else {
            None
        };
        drop(state);
        if let Some(wake) = wake {
            wake.wake();
        }
        Poll::Pending
    }
}

impl AsyncWrite for Proxy {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.write_operation(cx, WriteOp::Write, buf)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.write_operation(cx, WriteOp::Flush, &[]).map_ok(|_| ())
    }
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.write_operation(cx, WriteOp::Close, &[]).map_ok(|_| ())
    }
}

impl<T: AsyncRead + AsyncWrite + Unpin> Pump for Borrowed<'_, T> {
    fn poll_io(&mut self, cx: &mut Context<'_>) -> bool {
        let mut state = self.state.borrow_mut();
        state.driver = Some(cx.waker().clone());
        let mut wakes = [None, None];
        if state.read_len != 0 {
            let len = state.read_len;
            if let Poll::Ready(result) =
                Pin::new(&mut *self.stream).poll_read(cx, &mut state.read[..len])
            {
                state.read_len = 0;
                state.read_offset = 0;
                state.read_result = Some(result);
                wakes[0] = state.reader.take();
            }
        }
        if let Some(op) = &state.write_op {
            let result = match op {
                WriteOp::Write => Pin::new(&mut *self.stream).poll_write(cx, &state.write),
                WriteOp::Flush => Pin::new(&mut *self.stream).poll_flush(cx).map_ok(|()| 0),
                WriteOp::Close => Pin::new(&mut *self.stream).poll_close(cx).map_ok(|()| 0),
            };
            if let Poll::Ready(result) = result {
                state.write_op = None;
                state.write_result = Some(result);
                wakes[1] = state.writer.take();
            }
        }
        drop(state);
        let progress = wakes.iter().any(Option::is_some);
        for wake in wakes.into_iter().flatten() {
            wake.wake();
        }
        progress
    }
}

impl<T> Drop for Borrowed<'_, T> {
    fn drop(&mut self) {
        let mut state = self.state.borrow_mut();
        state.alive = false;
        let wakes = [
            state.reader.take(),
            state.writer.take(),
            state.driver.take(),
        ];
        drop(state);
        for wake in wakes.into_iter().flatten() {
            wake.wake();
        }
    }
}
