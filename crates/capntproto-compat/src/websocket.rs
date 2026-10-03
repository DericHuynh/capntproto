//! One standard serialized Cap'n Proto message per WebSocket binary message.
use crate::invalid;
use capnp::{
    message::{Builder, HeapAllocator, Reader, ReaderOptions},
    serialize::OwnedSegments,
    Result,
};
use futures::{future::LocalBoxFuture, AsyncRead, AsyncWrite, FutureExt};
use std::{
    io,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Binary(Vec<u8>),
    Text(String),
    Close { code: Option<u16>, reason: String },
}
/// Implement with a WebSocket library after handshake/frame reassembly. Ping/pong
/// handling belongs to that library. Receive and send must operate concurrently.
pub trait WebSocket: 'static {
    fn receive(&self, max_bytes: usize) -> LocalBoxFuture<'_, Result<Message>>;
    fn send(&self, message: Message) -> LocalBoxFuture<'_, Result<()>>;
    fn send_buffer_size(&self) -> Option<usize> {
        None
    }
}
#[derive(Clone)]
pub struct WebSocketMessageStream {
    socket: Rc<dyn WebSocket>,
    options: ReaderOptions,
    max_bytes: usize,
}
impl WebSocketMessageStream {
    pub fn new(socket: Rc<dyn WebSocket>, options: ReaderOptions, max_bytes: usize) -> Self {
        Self {
            socket,
            options,
            max_bytes,
        }
    }
    pub async fn try_read_message(&self) -> Result<Option<Reader<OwnedSegments>>> {
        match self.socket.receive(self.max_bytes).await? {
            Message::Close { .. } => Ok(None),
            Message::Text(_) => Err(invalid(
                "unexpected WebSocket text; expected binary Cap'n Proto message",
            )),
            Message::Binary(bytes) => {
                self.validate(&bytes)?;
                let mut slice = bytes.as_slice();
                let reader = capnp::serialize::read_message(&mut slice, self.options)?;
                if !slice.is_empty() {
                    return Err(invalid(
                        "multiple messages/trailing bytes in WebSocket message",
                    ));
                }
                Ok(Some(reader))
            }
        }
    }
    fn validate(&self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > self.max_bytes {
            return Err(invalid("WebSocket message size limit exceeded"));
        }
        if framed_len(bytes, self.max_bytes)? != Some(bytes.len()) {
            return Err(invalid("incomplete or trailing WebSocket Cap'n Proto data"));
        }
        Ok(())
    }
    pub async fn write_message(&self, message: &Builder<HeapAllocator>) -> Result<()> {
        let bytes = capnp::serialize::write_message_to_words(message);
        self.validate(&bytes)?;
        self.socket.send(Message::Binary(bytes)).await
    }
    pub async fn write_messages(&self, messages: &[Builder<HeapAllocator>]) -> Result<()> {
        for m in messages {
            self.write_message(m).await?;
        }
        Ok(())
    }
    pub fn get_send_buffer_size(&self) -> Option<usize> {
        self.socket.send_buffer_size()
    }
    pub async fn end(&self) -> Result<()> {
        self.socket
            .send(Message::Close {
                code: None,
                reason: "Capnp connection closed".into(),
            })
            .await
    }
    /// A framing-aware futures I/O adapter usable by `capnp_rpc::twoparty::VatNetwork`.
    /// Partial writes are buffered only up to one bounded serialized message.
    pub fn into_io(self) -> MessageIo {
        MessageIo {
            stream: self,
            read: None,
            write: None,
            input: vec![],
            read_pos: 0,
            output: vec![],
            eof: false,
            closed: false,
            failed: false,
        }
    }
}
fn framed_len(bytes: &[u8], max: usize) -> Result<Option<usize>> {
    if bytes.len() < 4 {
        return Ok(None);
    }
    let count = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as u64 + 1;
    if count > 512 {
        return Err(invalid("too many message segments"));
    }
    let header = ((count as usize + 2) & !1) * 4;
    if header > max {
        return Err(invalid("message header exceeds limit"));
    }
    if bytes.len() < header {
        return Ok(None);
    }
    let mut length = header;
    for segment in 0..count as usize {
        let p = 4 + segment * 4;
        let words = u32::from_le_bytes(bytes[p..p + 4].try_into().unwrap()) as usize;
        length = length
            .checked_add(
                words
                    .checked_mul(8)
                    .ok_or_else(|| invalid("segment overflow"))?,
            )
            .ok_or_else(|| invalid("message overflow"))?;
        if length > max {
            return Err(invalid("message size exceeds limit"));
        }
    }
    Ok(Some(length))
}
/// Framing adapter; cloning/splitting is provided by futures `AsyncReadExt::split`.
pub struct MessageIo {
    stream: WebSocketMessageStream,
    read: Option<LocalBoxFuture<'static, Result<Message>>>,
    write: Option<LocalBoxFuture<'static, Result<()>>>,
    input: Vec<u8>,
    read_pos: usize,
    output: Vec<u8>,
    eof: bool,
    closed: bool,
    failed: bool,
}
fn io_error(e: capnp::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}
impl AsyncRead for MessageIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if out.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if self.read_pos == self.input.len() {
            if self.eof {
                return Poll::Ready(Ok(0));
            }
            if self.read.is_none() {
                let socket = self.stream.socket.clone();
                let max = self.stream.max_bytes;
                self.read = Some(async move { socket.receive(max).await }.boxed_local());
            }
            let msg = match self.read.as_mut().unwrap().as_mut().poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(v) => {
                    self.read = None;
                    v
                }
            };
            match msg {
                Ok(Message::Close { .. }) => {
                    self.eof = true;
                    return Poll::Ready(Ok(0));
                }
                Ok(Message::Binary(bytes)) => {
                    if let Err(e) = self.stream.validate(&bytes) {
                        self.eof = true;
                        return Poll::Ready(Err(io_error(e)));
                    }
                    self.input = bytes;
                    self.read_pos = 0;
                }
                Ok(Message::Text(_)) => {
                    self.eof = true;
                    return Poll::Ready(Err(io_error(invalid("unexpected WebSocket text"))));
                }
                Err(e) => {
                    self.eof = true;
                    return Poll::Ready(Err(io_error(e)));
                }
            }
        }
        let n = out.len().min(self.input.len() - self.read_pos);
        out[..n].copy_from_slice(&self.input[self.read_pos..self.read_pos + n]);
        self.read_pos += n;
        Poll::Ready(Ok(n))
    }
}
impl MessageIo {
    fn poll_pending(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.failed {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "WebSocket output failed",
            )));
        }
        if let Some(future) = &mut self.write {
            match future.as_mut().poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(result) => {
                    self.write = None;
                    if let Err(e) = result {
                        self.failed = true;
                        return Poll::Ready(Err(io_error(e)));
                    }
                }
            }
        }
        Poll::Ready(Ok(()))
    }
}
impl AsyncWrite for MessageIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        futures::ready!(self.poll_pending(cx))?;
        if self.closed {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "WebSocket output closed",
            )));
        }
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let desired = match framed_len(&self.output, self.stream.max_bytes) {
            Ok(Some(n)) => n,
            Ok(None) => {
                if self.output.len() < 4 {
                    4
                } else {
                    let count =
                        u32::from_le_bytes(self.output[..4].try_into().unwrap()) as usize + 1;
                    ((count + 2) & !1) * 4
                }
            }
            Err(e) => {
                self.failed = true;
                return Poll::Ready(Err(io_error(e)));
            }
        };
        let n = bytes.len().min(desired - self.output.len());
        self.output.extend_from_slice(&bytes[..n]);
        match framed_len(&self.output, self.stream.max_bytes) {
            Ok(Some(length)) if length == self.output.len() => {
                let message = Message::Binary(std::mem::take(&mut self.output));
                let socket = self.stream.socket.clone();
                self.write = Some(async move { socket.send(message).await }.boxed_local());
            }
            Err(e) => {
                self.failed = true;
                return Poll::Ready(Err(io_error(e)));
            }
            _ => (),
        }
        Poll::Ready(Ok(n))
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_pending(cx)
    }
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        futures::ready!(self.poll_pending(cx))?;
        if !self.output.is_empty() {
            self.failed = true;
            return Poll::Ready(Err(io_error(invalid(
                "close during partial Cap'n Proto message",
            ))));
        }
        if self.closed {
            return Poll::Ready(Ok(()));
        }
        self.closed = true;
        let stream = self.stream.clone();
        self.write = Some(async move { stream.end().await }.boxed_local());
        self.poll_pending(cx)
    }
}
