use futures::{AsyncRead, AsyncWrite};
use js_sys::{Object, Promise, Reflect, Uint8Array};
use std::{
    future::{poll_fn, Future},
    io,
    pin::Pin,
    rc::Rc,
    task::{ready, Context, Poll},
};
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    ReadableStream, ReadableStreamDefaultReader, WritableStream, WritableStreamDefaultWriter,
};

// Only the WebTransport-specific surface is bound here. Readable/WritableStream
// bindings are stable, so consumers do not need web_sys_unstable_apis flags.
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = WebTransport)]
    type RawTransport;
    #[wasm_bindgen(constructor, catch, js_class = WebTransport)]
    fn new(url: &str, options: &JsValue) -> Result<RawTransport, JsValue>;
    #[wasm_bindgen(method, getter)]
    fn ready(this: &RawTransport) -> Promise;
    #[wasm_bindgen(method, getter)]
    fn closed(this: &RawTransport) -> Promise;
    #[wasm_bindgen(method, getter, js_name = incomingBidirectionalStreams)]
    fn incoming(this: &RawTransport) -> ReadableStream;
    #[wasm_bindgen(method, catch, js_name = createBidirectionalStream)]
    fn open(this: &RawTransport) -> Result<Promise, JsValue>;
    #[wasm_bindgen(method, catch)]
    fn close(this: &RawTransport, info: &JsValue) -> Result<(), JsValue>;
}
fn error(value: JsValue) -> io::Error {
    io::Error::other(format!("WebTransport: {value:?}"))
}
fn field(object: &JsValue, name: &str) -> io::Result<JsValue> {
    Reflect::get(object, &name.into()).map_err(error)
}
fn set(object: &JsValue, name: &str, value: &JsValue) -> io::Result<()> {
    if !Reflect::set(object, &name.into(), value).map_err(error)? {
        return Err(io::Error::other("WebTransport option is not writable"));
    }
    Ok(())
}
fn observe(promise: Promise) {
    drop(JsFuture::from(promise));
}

/// Browser-managed certificate verification. Clock, roots and TLS implementation
/// are browser policies. `new` uses browser trust; hashes explicitly pin a
/// self-signed certificate subject to the browser's validity/key constraints.
#[derive(Default)]
pub struct Client {
    hashes: Vec<[u8; 32]>,
}
impl Client {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_server_certificate_hashes(hashes: &[[u8; 32]]) -> Self {
        Self {
            hashes: hashes.to_vec(),
        }
    }
    pub async fn connect(&self, url: &str) -> io::Result<Session> {
        let url = super::checked_url(url)?;
        let options = Object::new();
        set(&options, "allowPooling", &JsValue::FALSE)?;
        set(&options, "requireUnreliable", &JsValue::TRUE)?;
        if !self.hashes.is_empty() {
            let hashes = js_sys::Array::new();
            for hash in &self.hashes {
                let entry = Object::new();
                set(&entry, "algorithm", &"sha-256".into())?;
                set(&entry, "value", &Uint8Array::from(&hash[..]).buffer())?;
                hashes.push(&entry);
            }
            set(&options, "serverCertificateHashes", &hashes)?;
        }
        let transport = RawTransport::new(url.as_str(), &options).map_err(error)?;
        // Observe closed immediately, including setup failures. Owner Drop closes
        // a canceled handshake; no abandoned connect leaves a live session.
        let closed = JsFuture::from(transport.closed());
        let owner = Rc::new(Owner {
            transport,
            _closed: closed,
        });
        JsFuture::from(owner.transport.ready())
            .await
            .map_err(error)?;
        let incoming = reader(&owner.transport.incoming())?;
        Ok(Session {
            owner,
            incoming,
            opening: None,
            accepting: None,
        })
    }
}
struct Owner {
    transport: RawTransport,
    _closed: JsFuture,
}
impl Owner {
    fn close(&self, code: u32, reason: &str) -> io::Result<()> {
        let info = Object::new();
        set(&info, "closeCode", &JsValue::from(code))?;
        set(&info, "reason", &reason.into())?;
        self.transport.close(&info).map_err(error)
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.close(0, "");
    }
}

pub struct Session {
    owner: Rc<Owner>,
    incoming: ReadableStreamDefaultReader,
    opening: Option<JsFuture>,
    accepting: Option<JsFuture>,
}
impl Session {
    /// Cancellation retains the in-flight browser operation for the next poll.
    pub async fn open_bi(&mut self) -> io::Result<Stream> {
        poll_fn(|cx| {
            if self.opening.is_none() {
                self.opening = Some(JsFuture::from(self.owner.transport.open().map_err(error)?));
            }
            let result = ready!(Pin::new(self.opening.as_mut().unwrap()).poll(cx));
            self.opening = None;
            Poll::Ready(Stream::new(result.map_err(error)?, self.owner.clone()))
        })
        .await
    }
    pub async fn accept_bi(&mut self) -> io::Result<Stream> {
        poll_fn(|cx| {
            let future = self
                .accepting
                .get_or_insert_with(|| JsFuture::from(self.incoming.read()));
            let result = ready!(Pin::new(future).poll(cx));
            self.accepting = None;
            let result = result.map_err(error)?;
            if field(&result, "done")?.as_bool() == Some(true) {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "WebTransport session closed",
                )));
            }
            Poll::Ready(Stream::new(field(&result, "value")?, self.owner.clone()))
        })
        .await
    }
    pub fn close(&self, code: u32, reason: &str) -> io::Result<()> {
        self.owner.close(code, reason)
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        observe(self.incoming.cancel());
        self.incoming.release_lock();
        for (future, accepted) in [(self.opening.take(), false), (self.accepting.take(), true)] {
            if let Some(future) = future {
                // Cleanup retains the promise, never Owner. The last stream can
                // still close the session while an abandoned open is pending.
                spawn_local(async move {
                    if let Ok(raw) = future.await {
                        let raw = if accepted {
                            field(&raw, "value").ok()
                        } else {
                            Some(raw)
                        };
                        if let Some(raw) = raw {
                            let _ = abort_raw(raw);
                        }
                    }
                });
            }
        }
    }
}
fn reader(stream: &ReadableStream) -> io::Result<ReadableStreamDefaultReader> {
    stream
        .get_reader()
        .dyn_into()
        .map_err(|value| error(value.into()))
}
fn parts(raw: &JsValue) -> io::Result<(ReadableStreamDefaultReader, WritableStreamDefaultWriter)> {
    let read: ReadableStream = field(raw, "readable")?.dyn_into().map_err(error)?;
    let write: WritableStream = field(raw, "writable")?.dyn_into().map_err(error)?;
    Ok((reader(&read)?, write.get_writer().map_err(error)?))
}
fn abort_raw(raw: JsValue) -> io::Result<()> {
    let (read, write) = parts(&raw)?;
    observe(read.cancel());
    observe(write.abort());
    read.release_lock();
    write.release_lock();
    Ok(())
}

/// Browser stream operations stay subscribed across Pending/cancellation. A
/// write acknowledges copied browser-owned bytes; flush waits for their write
/// promise, and close waits for FIN while leaving the read half available.
pub struct Stream {
    reader: ReadableStreamDefaultReader,
    writer: WritableStreamDefaultWriter,
    reading: Option<JsFuture>,
    writing: Option<JsFuture>,
    closing: Option<JsFuture>,
    buffer: Vec<u8>,
    offset: usize,
    eof: bool,
    write_closed: bool,
    close_done: bool,
    write_failed: bool,
    _session: Rc<Owner>,
}
impl Stream {
    fn new(raw: JsValue, owner: Rc<Owner>) -> io::Result<Self> {
        let (reader, writer) = parts(&raw)?;
        Ok(Self {
            reader,
            writer,
            reading: None,
            writing: None,
            closing: None,
            buffer: Vec::new(),
            offset: 0,
            eof: false,
            write_closed: false,
            close_done: false,
            write_failed: false,
            _session: owner,
        })
    }
    fn flush_write(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.write_failed {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "WebTransport write failed",
            )));
        }
        if let Some(future) = self.writing.as_mut() {
            let result = ready!(Pin::new(future).poll(cx));
            self.writing = None;
            if let Err(err) = result {
                self.write_failed = true;
                return Poll::Ready(Err(error(err)));
            }
        }
        Poll::Ready(Ok(()))
    }
}
impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        loop {
            if this.offset < this.buffer.len() {
                let n = bytes.len().min(this.buffer.len() - this.offset);
                bytes[..n].copy_from_slice(&this.buffer[this.offset..this.offset + n]);
                this.offset += n;
                return Poll::Ready(Ok(n));
            }
            if this.eof {
                return Poll::Ready(Ok(0));
            }
            let future = this
                .reading
                .get_or_insert_with(|| JsFuture::from(this.reader.read()));
            let result = ready!(Pin::new(future).poll(cx));
            this.reading = None;
            let result = result.map_err(error)?;
            if field(&result, "done")?.as_bool() == Some(true) {
                this.eof = true;
                continue;
            }
            let chunk = field(&result, "value")?
                .dyn_into::<Uint8Array>()
                .map_err(error)?;
            this.buffer = chunk.to_vec();
            this.offset = 0;
        }
    }
}
impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if this.write_closed {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "WebTransport write half closed",
            )));
        }
        ready!(this.flush_write(cx))?;
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        // Bound each accepted browser chunk, including a caller's oversized write.
        let n = bytes.len().min(64 * 1024);
        let copy = Uint8Array::from(&bytes[..n]);
        this.writing = Some(JsFuture::from(this.writer.write_with_chunk(&copy)));
        Poll::Ready(Ok(n))
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.get_mut().flush_write(cx)
    }
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.close_done {
            return Poll::Ready(Ok(()));
        }
        this.write_closed = true;
        ready!(this.flush_write(cx))?;
        let future = this
            .closing
            .get_or_insert_with(|| JsFuture::from(this.writer.close()));
        let result = ready!(Pin::new(future).poll(cx));
        this.closing = None;
        if let Err(err) = result {
            this.write_failed = true;
            return Poll::Ready(Err(error(err)));
        }
        this.close_done = true;
        Poll::Ready(Ok(()))
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        observe(self.reader.cancel());
        if !self.close_done {
            observe(self.writer.abort());
        }
        self.reader.release_lock();
        self.writer.release_lock();
    }
}
