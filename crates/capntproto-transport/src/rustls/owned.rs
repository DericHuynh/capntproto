//! A task-free TLS driver for ownership/completion IO. Ciphertext vectors move
//! into operations and return at completion; no relay pipe or ciphertext copy.
use super::{Progress, RecordLayer};
use crate::io::{Close, Read};
use futures::{future::poll_fn, AsyncRead, AsyncWrite, FutureExt};
use rustls_impl::{pki_types::ServerName, ClientConfig, CommonState, ServerConfig};
use std::{
    cell::RefCell,
    io,
    ops::Range,
    pin::Pin,
    rc::Rc,
    sync::Arc,
    task::{Context, Poll, Waker},
};

/// Owned initialized storage with a selected IO window. Native adapters can
/// transfer the vector without copying. Counts always refer to this window.
pub struct Buffer {
    pub(crate) bytes: Vec<u8>,
    pub(crate) range: Range<usize>,
}
impl Buffer {
    pub fn new(bytes: Vec<u8>, range: Range<usize>) -> io::Result<Self> {
        if range.start > range.end || range.end > bytes.len() {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        Ok(Self { bytes, range })
    }
    pub fn into_parts(self) -> (Vec<u8>, Range<usize>) {
        (self.bytes, self.range)
    }
}
impl AsRef<[u8]> for Buffer {
    fn as_ref(&self) -> &[u8] {
        &self.bytes[self.range.clone()]
    }
}
impl AsMut<[u8]> for Buffer {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.bytes[self.range.clone()]
    }
}

struct State {
    tls: RecordLayer,
    authenticated: bool,
    input: bool,
    output: bool,
    closing: bool,
    receiving: bool,
    sending: bool,
    write_finished: bool,
    dropped: bool,
    draining: bool,
    control: bool,
    failure: Option<(io::ErrorKind, String)>,
    reader: Option<Waker>,
    writer: Option<Waker>,
    driver: Option<Waker>,
}
impl State {
    fn error(&self) -> io::Result<()> {
        match &self.failure {
            Some((kind, message)) => Err(io::Error::new(*kind, message.clone())),
            None => Ok(()),
        }
    }
    fn notify(slot: &mut Option<Waker>) {
        if let Some(w) = slot.take() {
            w.wake();
        }
    }
    fn request(
        &mut self,
        progress: Progress<usize>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<usize>> {
        match progress {
            Progress::Ready(n) => {
                if !self.tls.outgoing.flushed {
                    self.output = true;
                    Self::notify(&mut self.driver);
                }
                Poll::Ready(Ok(n))
            }
            Progress::Input => {
                self.input = true;
                Self::notify(&mut self.driver);
                Poll::Pending
            }
            Progress::Output => {
                self.output = true;
                Self::notify(&mut self.driver);
                Poll::Pending
            }
            Progress::Yield => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }
    fn handshake(&mut self) -> io::Result<()> {
        if self.authenticated {
            return Ok(());
        }
        match self.tls.handshake()? {
            Progress::Ready(_) => {
                if self.tls.common().alpn_protocol() != Some(crate::ALPN) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "RPC ALPN was not negotiated",
                    ));
                }
                self.authenticated = true;
                Self::notify(&mut self.reader);
                Self::notify(&mut self.writer);
            }
            Progress::Input => self.input = true,
            Progress::Output => self.output = true,
            Progress::Yield => {
                self.control = true;
                Self::notify(&mut self.driver);
            }
        }
        Ok(())
    }
    fn fail(&mut self, error: &io::Error) {
        if self.failure.is_none() {
            self.failure = Some((error.kind(), error.to_string()));
        }
        Self::notify(&mut self.reader);
        Self::notify(&mut self.writer);
        Self::notify(&mut self.driver);
    }
}

/// Plaintext futures IO driven by the separately returned transport future.
/// Dropping the driver breaks pending reads/writes; dropping this stream stops
/// its driver. Close performs TLS close_notify and a physical write half-close.
pub struct Stream {
    state: Rc<RefCell<State>>,
}
impl Stream {
    pub async fn ready(&mut self) -> io::Result<()> {
        poll_fn(|cx| {
            let mut s = self.state.borrow_mut();
            s.error()?;
            if s.authenticated {
                Poll::Ready(Ok(()))
            } else {
                s.reader = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }
    pub fn inspect<T>(&self, inspect: impl FnOnce(&CommonState) -> T) -> T {
        inspect(self.state.borrow().tls.common())
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        let mut s = self.state.borrow_mut();
        s.dropped = true;
        State::notify(&mut s.driver);
    }
}
impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let mut s = self.state.borrow_mut();
        s.error()?;
        // An in-flight receive owns all ciphertext storage. Reprocessing an
        // empty slice cannot advance TLS; only buffered plaintext/EOF is ready.
        let waiting = s.receiving && s.tls.plaintext.pending().is_empty() && !s.tls.peer_closed;
        let result = if s.authenticated && !waiting {
            let progress = s.tls.read(bytes)?;
            s.request(progress, cx)
        } else {
            Poll::Pending
        };
        if result.is_pending() {
            s.reader = Some(cx.waker().clone());
        }
        result
    }
}
impl Stream {
    fn write_operation(
        &mut self,
        cx: &mut Context<'_>,
        write: impl FnOnce(&mut RecordLayer) -> io::Result<Progress<usize>>,
    ) -> Poll<io::Result<usize>> {
        let mut s = self.state.borrow_mut();
        s.error()?;
        if s.closing {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        let result = if !s.authenticated || s.sending {
            Poll::Pending
        } else {
            let progress = write(&mut s.tls)?;
            s.request(progress, cx)
        };
        if result.is_pending() {
            s.writer = Some(cx.waker().clone());
        }
        result
    }
}
impl AsyncWrite for Stream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.write_operation(cx, |tls| tls.write(bytes))
    }
    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        self.write_operation(cx, |tls| tls.write_vectored(bytes))
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut s = self.state.borrow_mut();
        s.error()?;
        let result = if !s.authenticated || s.sending {
            Poll::Pending
        } else {
            let progress = s.tls.flush()?;
            match s.request(progress, cx) {
                Poll::Ready(Ok(_)) if !s.tls.outgoing.flushed => {
                    s.output = true;
                    State::notify(&mut s.driver);
                    Poll::Pending
                }
                other => other.map_ok(|_| ()),
            }
        };
        if result.is_pending() {
            s.writer = Some(cx.waker().clone());
        }
        result
    }
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut s = self.state.borrow_mut();
        s.error()?;
        if s.write_finished {
            return Poll::Ready(Ok(()));
        }
        s.closing = true;
        if s.authenticated && !s.sending {
            let progress = s.tls.shutdown()?;
            let _ = s.request(progress, cx);
            s.output = true;
            State::notify(&mut s.driver);
        }
        s.writer = Some(cx.waker().clone());
        Poll::Pending
    }
}

struct Guard(Rc<RefCell<State>>);
impl Drop for Guard {
    fn drop(&mut self) {
        self.0.borrow_mut().fail(&io::Error::new(
            io::ErrorKind::ConnectionAborted,
            "TLS driver dropped",
        ));
    }
}

async fn read_once<R: Read<Buffer = Buffer, Error = io::Error>>(
    reader: Option<R>,
    buffer: Option<Buffer>,
) -> (R, Buffer, io::Result<usize>) {
    let Some(mut reader) = reader else {
        return core::future::pending().await;
    };
    let (result, buffer) = reader.read(buffer.unwrap()).await;
    (reader, buffer, result)
}
async fn write_once<W: Close<Buffer = Buffer, Error = io::Error>>(
    writer: Option<W>,
    buffer: Option<Buffer>,
    close: bool,
) -> (W, Buffer, io::Result<()>, bool) {
    let Some(mut writer) = writer else {
        return core::future::pending().await;
    };
    let mut buffer = buffer.unwrap();
    while !buffer.as_ref().is_empty() {
        let length = buffer.as_ref().len();
        let (result, returned) = writer.write(buffer).await;
        buffer = returned;
        let n = match result {
            Ok(0) => return (writer, buffer, Err(io::ErrorKind::WriteZero.into()), close),
            Ok(n) if n <= length => n,
            Ok(_) => {
                return (
                    writer,
                    buffer,
                    Err(io::ErrorKind::InvalidData.into()),
                    close,
                )
            }
            Err(error) => return (writer, buffer, Err(error), close),
        };
        buffer.range.start += n;
    }
    let result = match writer.flush().await {
        Ok(()) if close => writer.close().await,
        other => other,
    };
    (writer, buffer, result, close)
}

fn start<R, W>(
    reader: R,
    writer: W,
    tls: RecordLayer,
) -> (Stream, impl crate::driven::Driver<Error = io::Error>)
where
    R: Read<Buffer = Buffer, Error = io::Error>,
    W: Close<Buffer = Buffer, Error = io::Error>,
{
    let state = Rc::new(RefCell::new(State {
        tls,
        authenticated: false,
        input: false,
        output: false,
        closing: false,
        receiving: false,
        sending: false,
        write_finished: false,
        dropped: false,
        draining: false,
        control: false,
        failure: None,
        reader: None,
        writer: None,
        driver: None,
    }));
    let guard = Guard(state.clone());
    let stream = Stream {
        state: state.clone(),
    };
    let shutdown_state = state.clone();
    let driver = async move {
        let _guard = guard;
        let result = run(reader, writer, state.clone()).await;
        if let Err(error) = &result {
            state.borrow_mut().fail(error);
        }
        result
    };
    (
        stream,
        crate::driven::Owned::new(driver, move || {
            let mut s = shutdown_state.borrow_mut();
            s.draining = true;
            s.control = true;
            if !s.dropped {
                s.closing = true;
            }
            State::notify(&mut s.driver);
        }),
    )
}

async fn run<R, W>(reader: R, writer: W, state: Rc<RefCell<State>>) -> io::Result<()>
where
    R: Read<Buffer = Buffer, Error = io::Error>,
    W: Close<Buffer = Buffer, Error = io::Error>,
{
    let mut reader = Some(reader);
    let mut writer = Some(writer);
    let read = read_once::<R>(None, None).fuse();
    let write = write_once::<W>(None, None, false).fuse();
    futures::pin_mut!(read, write);
    loop {
        {
            let mut s = state.borrow_mut();
            s.error()?;
            if s.draining {
                if !s.authenticated
                    || s.dropped
                    || s.write_finished
                    || (s.tls.peer_closed && !s.sending && s.tls.outgoing.pending().is_empty())
                {
                    return Ok(());
                }
                if !s.sending {
                    match s.tls.shutdown()? {
                        Progress::Ready(_) => {
                            s.tls.write_closed = true;
                            s.output = true;
                        }
                        Progress::Input => s.input = true,
                        Progress::Output => s.output = true,
                        Progress::Yield => s.control = true,
                    }
                }
            } else if s.dropped {
                return Ok(());
            }
            s.handshake()?;
            if s.input && !s.receiving && !s.tls.peer_closed {
                s.tls.input()?;
                let end = s.tls.incoming.end;
                let bytes = std::mem::take(&mut s.tls.incoming.bytes);
                let length = bytes.len();
                s.receiving = true;
                s.input = false;
                read.set(
                    read_once(
                        reader.take(),
                        Some(Buffer {
                            bytes,
                            range: end..length,
                        }),
                    )
                    .fuse(),
                );
            }
            if (s.output || (s.closing && s.tls.write_closed)) && !s.sending && !s.write_finished {
                let range = s.tls.outgoing.start..s.tls.outgoing.end;
                let bytes = std::mem::take(&mut s.tls.outgoing.bytes);
                s.tls.outgoing.start = 0;
                s.tls.outgoing.end = 0;
                let close = s.closing && s.tls.write_closed;
                s.sending = true;
                s.output = false;
                write.set(write_once(writer.take(), Some(Buffer { bytes, range }), close).fuse());
            }
        }
        let notified = poll_fn(|cx| {
            let mut s = state.borrow_mut();
            s.driver = Some(cx.waker().clone());
            if (s.dropped && !s.draining)
                || std::mem::take(&mut s.control)
                || s.failure.is_some()
                || (s.input && !s.receiving && !s.tls.peer_closed)
                || (s.output && !s.sending && !s.write_finished)
            {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .fuse();
        futures::pin_mut!(notified);
        futures::select_biased! {
            (returned,buffer,result)=read => {
                reader=Some(returned);
                let mut s=state.borrow_mut(); s.receiving=false; s.input=false;
                s.tls.incoming.bytes=buffer.bytes;
                let n=result?; s.tls.commit_input(n)?;
                State::notify(&mut s.reader); State::notify(&mut s.writer);
            },
            (returned,buffer,result,closed)=write => {
                writer=Some(returned);
                let mut s=state.borrow_mut(); s.sending=false; s.output=false;
                result?;
                if s.tls.outgoing.pending().is_empty() {
                    s.tls.outgoing.bytes=buffer.bytes;
                    s.tls.outgoing.start=0; s.tls.outgoing.end=0; s.tls.outgoing.flushed=true;
                } else { s.output=true; }
                s.write_finished=closed;
                State::notify(&mut s.reader); State::notify(&mut s.writer);
            },
            _=notified => (),
        }
    }
}

/// Construct client state without performing IO or spawning. Drive the returned
/// future alongside Stream::ready and application/RPC work in one scope.
pub fn client<R, W>(
    reader: R,
    writer: W,
    config: Arc<ClientConfig>,
    name: ServerName<'static>,
) -> io::Result<(Stream, impl crate::driven::Driver<Error = io::Error>)>
where
    R: Read<Buffer = Buffer, Error = io::Error>,
    W: Close<Buffer = Buffer, Error = io::Error>,
{
    Ok(start(reader, writer, RecordLayer::client(config, name)?))
}
pub fn server<R, W>(
    reader: R,
    writer: W,
    config: Arc<ServerConfig>,
) -> io::Result<(Stream, impl crate::driven::Driver<Error = io::Error>)>
where
    R: Read<Buffer = Buffer, Error = io::Error>,
    W: Close<Buffer = Buffer, Error = io::Error>,
{
    Ok(start(reader, writer, RecordLayer::server(config)?))
}
