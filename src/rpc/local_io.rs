//! Bounded byte streams for native RPC tasks sharing one local executor.
//!
//! Each direction has exactly one reader and one writer. Splitting transfers
//! those endpoints without a mutex; Rc keeps the pair confined to one thread.
#![forbid(unsafe_code)]

use std::{
    cell::RefCell,
    collections::VecDeque,
    io,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

struct Pipe {
    bytes: VecDeque<u8>,
    capacity: usize,
    reader_closed: bool,
    writer_closed: bool,
    reader: Option<Waker>,
    writer: Option<Waker>,
}

pub(crate) struct ReadHalf(Rc<RefCell<Pipe>>);
pub(crate) struct WriteHalf(Rc<RefCell<Pipe>>);
pub(crate) struct Stream {
    read: ReadHalf,
    write: WriteHalf,
}

pub(crate) fn pair(capacity: usize) -> (Stream, Stream) {
    assert!(capacity > 0);
    let direction = || {
        Rc::new(RefCell::new(Pipe {
            bytes: VecDeque::new(),
            capacity,
            reader_closed: false,
            writer_closed: false,
            reader: None,
            writer: None,
        }))
    };
    let a = direction();
    let b = direction();
    (
        Stream {
            read: ReadHalf(a.clone()),
            write: WriteHalf(b.clone()),
        },
        Stream {
            read: ReadHalf(b),
            write: WriteHalf(a),
        },
    )
}

impl Stream {
    pub(crate) fn into_split(self) -> (ReadHalf, WriteHalf) {
        (self.read, self.write)
    }
}

impl AsyncRead for ReadHalf {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let coop = std::task::ready!(tokio::task::coop::poll_proceed(cx));
        let mut replacement = None;
        loop {
            let mut pipe = self.0.borrow_mut();
            if !pipe.bytes.is_empty() {
                let count = output.remaining().min(pipe.bytes.len());
                let (front, back) = pipe.bytes.as_slices();
                let first = count.min(front.len());
                output.put_slice(&front[..first]);
                output.put_slice(&back[..count - first]);
                pipe.bytes.drain(..count);
                let wake = pipe.writer.take();
                drop(pipe);
                if let Some(wake) = wake {
                    wake.wake();
                }
                coop.made_progress();
                return Poll::Ready(Ok(()));
            }
            if pipe.writer_closed {
                coop.made_progress();
                return Poll::Ready(Ok(()));
            }
            if pipe
                .reader
                .as_ref()
                .is_some_and(|w| w.will_wake(cx.waker()))
            {
                return Poll::Pending;
            }
            if let Some(waker) = replacement.take() {
                let retired = pipe.reader.replace(waker);
                drop(pipe);
                drop(retired);
                return Poll::Pending;
            }
            drop(pipe);
            // Waker clone/drop/wake may reenter application code. None run
            // while the pipe is borrowed; recheck readiness after cloning.
            replacement = Some(cx.waker().clone());
        }
    }
}

impl AsyncWrite for WriteHalf {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.poll_write_vectored(cx, &[io::IoSlice::new(bytes)])
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffers: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let coop = std::task::ready!(tokio::task::coop::poll_proceed(cx));
        let mut replacement = None;
        loop {
            let mut pipe = self.0.borrow_mut();
            if pipe.reader_closed || pipe.writer_closed {
                coop.made_progress();
                return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
            }
            let available = pipe.capacity - pipe.bytes.len();
            if available > 0 {
                let mut count = 0;
                for buffer in buffers {
                    let length = (available - count).min(buffer.len());
                    pipe.bytes.extend(&buffer[..length]);
                    count += length;
                    if count == available {
                        break;
                    }
                }
                let wake = pipe.reader.take();
                drop(pipe);
                if let Some(wake) = wake {
                    wake.wake();
                }
                coop.made_progress();
                return Poll::Ready(Ok(count));
            }
            if pipe
                .writer
                .as_ref()
                .is_some_and(|w| w.will_wake(cx.waker()))
            {
                return Poll::Pending;
            }
            if let Some(waker) = replacement.take() {
                let retired = pipe.writer.replace(waker);
                drop(pipe);
                drop(retired);
                return Poll::Pending;
            }
            drop(pipe);
            replacement = Some(cx.waker().clone());
        }
    }

    fn is_write_vectored(&self) -> bool {
        true
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        // Like Tokio's duplex stream, admission is immediate. The native
        // protocol's delivery receipt remains the remote completion fence.
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.close();
        Poll::Ready(Ok(()))
    }
}

impl WriteHalf {
    fn close(&self) {
        let (wake, retired) = {
            let mut pipe = self.0.borrow_mut();
            pipe.writer_closed = true;
            (pipe.reader.take(), pipe.writer.take())
        };
        drop(retired);
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}
impl Drop for WriteHalf {
    fn drop(&mut self) {
        self.close();
    }
}
impl Drop for ReadHalf {
    fn drop(&mut self) {
        let (bytes, wake, retired) = {
            let mut pipe = self.0.borrow_mut();
            pipe.reader_closed = true;
            (
                std::mem::take(&mut pipe.bytes),
                pipe.writer.take(),
                pipe.reader.take(),
            )
        };
        drop((bytes, retired));
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().read).poll_read(cx, buffer)
    }
}
impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().write).poll_write(cx, bytes)
    }
    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffers: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().write).poll_write_vectored(cx, buffers)
    }
    fn is_write_vectored(&self) -> bool {
        true
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().write).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().write).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests;
