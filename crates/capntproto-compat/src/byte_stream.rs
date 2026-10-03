//! The standard ByteStream capability protocol and explicit-end output adapters.
use crate::{byte_stream_capnp::byte_stream as wire, invalid};
use capnp::{
    capability::{FromClientHook, Promise, ServerHooks},
    Result,
};
use futures::{
    future::{LocalBoxFuture, Shared},
    lock::Mutex,
    AsyncWrite, AsyncWriteExt, FutureExt,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

/// A single-writer output endpoint. Dropping without `end()` is cancellation.
/// Implementations must report truncation through `abort`; it is never clean EOF.
pub trait OutputStream: 'static {
    fn write<'a>(&'a self, bytes: &'a [u8]) -> LocalBoxFuture<'a, Result<()>>;
    fn end(&self) -> LocalBoxFuture<'_, Result<()>>;
    fn abort(&self);
    fn start_tls<'a>(&'a self, _hostname: &'a str) -> LocalBoxFuture<'a, Result<()>> {
        async {
            Err(capnp::Error::unimplemented(
                "TLS upgrade is not configured".into(),
            ))
        }
        .boxed_local()
    }
    fn shorten_path(&self) -> Option<Promise<capnp::capability::Client, capnp::Error>> {
        None
    }
}

/// Adapt any futures AsyncWrite. `on_abort` observes dropped/truncated bodies.
pub struct AsyncOutput<W> {
    writer: Mutex<Option<W>>,
    on_abort: Box<dyn Fn()>,
    ended: Cell<bool>,
}
impl<W: AsyncWrite + Unpin + 'static> AsyncOutput<W> {
    pub fn new(writer: W, on_abort: impl Fn() + 'static) -> Self {
        Self {
            writer: Mutex::new(Some(writer)),
            on_abort: Box::new(on_abort),
            ended: Cell::new(false),
        }
    }
}
impl<W: AsyncWrite + Unpin + 'static> OutputStream for AsyncOutput<W> {
    fn write<'a>(&'a self, bytes: &'a [u8]) -> LocalBoxFuture<'a, Result<()>> {
        async move {
            if self.ended.get() {
                return Err(invalid("stream ended"));
            }
            let mut w = self.writer.lock().await;
            w.as_mut()
                .ok_or_else(|| invalid("stream closed"))?
                .write_all(bytes)
                .await?;
            Ok(())
        }
        .boxed_local()
    }
    fn end(&self) -> LocalBoxFuture<'_, Result<()>> {
        async move {
            let mut w = self.writer.lock().await;
            if self.ended.get() {
                return Err(invalid("stream already ended"));
            }
            w.as_mut()
                .ok_or_else(|| invalid("stream closed"))?
                .close()
                .await?;
            self.ended.set(true);
            w.take();
            Ok(())
        }
        .boxed_local()
    }
    fn abort(&self) {
        if !self.ended.replace(true) {
            (self.on_abort)();
            if let Some(mut w) = self.writer.try_lock() {
                w.take();
            }
        }
    }
}
impl<W> Drop for AsyncOutput<W> {
    fn drop(&mut self) {
        if !self.ended.replace(true) {
            (self.on_abort)();
        }
    }
}

#[derive(Clone, Default)]
pub struct ByteStreamFactory {
    servers: Rc<RefCell<capnp_rpc::CapabilityServerSet<StreamServer, wire::Client>>>,
}
impl ByteStreamFactory {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn from_output(&self, output: Rc<dyn OutputStream>) -> wire::Client {
        let mut servers = self.servers.borrow_mut();
        servers.gc();
        servers.new_client(StreamServer {
            output,
            busy: Cell::new(false),
            ended: Cell::new(false),
            forwarded: Rc::new(Cell::new(false)),
            factory: self.clone(),
        })
    }
    /// Recognize a local round trip without serializing writes. Unresolved/remote
    /// capabilities remain RPC outputs; their standard resolution still shortens paths.
    pub fn to_output(&self, client: wire::Client) -> Rc<dyn OutputStream> {
        if let Some(server) = self.servers.borrow().get_local_server_of_resolved(&client) {
            Rc::new(LocalOutput { server })
        } else {
            Rc::new(RpcOutput {
                client: RefCell::new(Some(client)),
                ended: Cell::new(false),
            })
        }
    }
    pub fn gc(&self) {
        self.servers.borrow_mut().gc();
    }
}
struct StreamServer {
    output: Rc<dyn OutputStream>,
    busy: Cell<bool>,
    ended: Cell<bool>,
    forwarded: Rc<Cell<bool>>,
    factory: ByteStreamFactory,
}
impl StreamServer {
    fn operation(&self) -> Result<StreamOperation<'_>> {
        self.ready()?;
        Ok(StreamOperation {
            server: self,
            complete: false,
        })
    }
    fn ready(&self) -> Result<()> {
        if self.ended.get() {
            Err(invalid("stream ended"))
        } else if self.busy.get() {
            Err(invalid("substream is active"))
        } else {
            Ok(())
        }
    }
}
struct StreamOperation<'a> {
    server: &'a StreamServer,
    complete: bool,
}
impl Drop for StreamOperation<'_> {
    fn drop(&mut self) {
        if !self.complete && !self.server.ended.replace(true) {
            self.server.output.abort();
        }
    }
}
impl Drop for StreamServer {
    fn drop(&mut self) {
        if !self.ended.get() && !self.forwarded.get() {
            self.output.abort();
        }
    }
}
impl ServerHooks for StreamServer {
    fn shorten_path(&self) -> Option<Promise<capnp::capability::Client, capnp::Error>> {
        let resolution = self.output.shorten_path()?;
        let forwarded = self.forwarded.clone();
        Some(Promise::from_future(async move {
            let client = resolution.await?;
            // Ownership has transferred to the resolved capability. Destroying
            // the old forwarding server must not abort the replacement stream.
            forwarded.set(true);
            Ok(client)
        }))
    }
}
impl wire::Server for StreamServer {
    fn _capnp_server_hooks(&self) -> Option<&dyn ServerHooks> {
        Some(self)
    }
    async fn write(self: Rc<Self>, params: wire::WriteParams) -> Result<()> {
        let mut operation = self.operation()?;
        self.output.write(params.get()?.get_bytes()?).await?;
        operation.complete = true;
        Ok(())
    }
    async fn end(self: Rc<Self>, _: wire::EndParams, _: wire::EndResults) -> Result<()> {
        let mut operation = self.operation()?;
        self.output.end().await?;
        self.ended.set(true);
        operation.complete = true;
        Ok(())
    }
    async fn start_tls(self: Rc<Self>, params: wire::StartTlsParams) -> Result<()> {
        let mut operation = self.operation()?;
        self.output
            .start_tls(params.get()?.get_expected_server_hostname()?.to_str()?)
            .await?;
        operation.complete = true;
        Ok(())
    }
    async fn get_substream(
        self: Rc<Self>,
        params: wire::GetSubstreamParams,
        mut results: wire::GetSubstreamResults,
    ) -> Result<()> {
        self.ready()?;
        let params = params.get()?;
        let callback = params.get_callback()?;
        let limit = params.get_limit();
        self.busy.set(true);
        let (tx, rx) = futures::channel::oneshot::channel::<Result<wire::Client>>();
        let shortened = async move {
            rx.await
                .map_err(|_| capnp::Error::disconnected("substream dropped".into()))?
        }
        .boxed_local()
        .shared();
        let sub = Rc::new(Substream {
            parent: self.clone(),
            callback,
            limit,
            state: Mutex::new(SubState {
                count: 0,
                next: None,
                ended: false,
                failed: false,
            }),
            released: Cell::new(false),
            resolve: RefCell::new(Some(tx)),
            shortened,
        });
        let client = self.factory.from_output(sub.clone());
        results.get().set_substream(client);
        results.set_pipeline()?;
        if limit == 0 {
            let mut state = sub.state.lock().await;
            sub.redirect(&mut state).await?;
        }
        Ok(())
    }
}
struct LocalOutput {
    server: Rc<StreamServer>,
}
impl OutputStream for LocalOutput {
    fn shorten_path(&self) -> Option<Promise<capnp::capability::Client, capnp::Error>> {
        let client: wire::Client = capnp_rpc::new_client_from_rc(self.server.clone());
        Some(Promise::ok(capnp::capability::Client::new(
            client.into_client_hook(),
        )))
    }
    fn write<'a>(&'a self, b: &'a [u8]) -> LocalBoxFuture<'a, Result<()>> {
        async move {
            let mut operation = self.server.operation()?;
            self.server.output.write(b).await?;
            operation.complete = true;
            Ok(())
        }
        .boxed_local()
    }
    fn end(&self) -> LocalBoxFuture<'_, Result<()>> {
        async move {
            let mut operation = self.server.operation()?;
            self.server.output.end().await?;
            self.server.ended.set(true);
            operation.complete = true;
            Ok(())
        }
        .boxed_local()
    }
    fn abort(&self) {
        if !self.server.ended.replace(true) {
            self.server.output.abort();
        }
    }
    fn start_tls<'a>(&'a self, h: &'a str) -> LocalBoxFuture<'a, Result<()>> {
        async move {
            let mut operation = self.server.operation()?;
            self.server.output.start_tls(h).await?;
            operation.complete = true;
            Ok(())
        }
        .boxed_local()
    }
}
struct RpcOutput {
    client: RefCell<Option<wire::Client>>,
    ended: Cell<bool>,
}
impl RpcOutput {
    fn client(&self) -> Result<wire::Client> {
        self.client
            .borrow()
            .clone()
            .filter(|_| !self.ended.get())
            .ok_or_else(|| invalid("stream ended"))
    }
}
impl OutputStream for RpcOutput {
    fn shorten_path(&self) -> Option<Promise<capnp::capability::Client, capnp::Error>> {
        Some(match self.client() {
            Ok(client) => Promise::ok(capnp::capability::Client::new(client.into_client_hook())),
            Err(error) => Promise::err(error),
        })
    }
    fn write<'a>(&'a self, b: &'a [u8]) -> LocalBoxFuture<'a, Result<()>> {
        async move {
            let mut r = self.client()?.write_request();
            r.get().set_bytes(b);
            r.send().await
        }
        .boxed_local()
    }
    fn end(&self) -> LocalBoxFuture<'_, Result<()>> {
        async move {
            self.client()?.end_request().send().promise.await?;
            self.ended.set(true);
            self.client.borrow_mut().take();
            Ok(())
        }
        .boxed_local()
    }
    fn abort(&self) {
        self.ended.set(true);
        self.client.borrow_mut().take();
    }
    fn start_tls<'a>(&'a self, h: &'a str) -> LocalBoxFuture<'a, Result<()>> {
        async move {
            let mut r = self.client()?.start_tls_request();
            r.get().set_expected_server_hostname(h);
            r.send().await
        }
        .boxed_local()
    }
}
struct SubState {
    count: u64,
    next: Option<Rc<dyn OutputStream>>,
    ended: bool,
    failed: bool,
}
struct Substream {
    parent: Rc<StreamServer>,
    callback: wire::substream_callback::Client,
    limit: u64,
    state: Mutex<SubState>,
    released: Cell<bool>,
    resolve: RefCell<Option<futures::channel::oneshot::Sender<Result<wire::Client>>>>,
    shortened: Shared<LocalBoxFuture<'static, Result<wire::Client>>>,
}
impl Substream {
    fn release(&self) {
        if !self.released.replace(true) {
            self.parent.busy.set(false);
        }
    }
    async fn redirect(&self, state: &mut SubState) -> Result<()> {
        if state.next.is_some() {
            return Ok(());
        }
        // Mark poisoned before awaiting: cancellation must never repeat a callback.
        state.failed = true;
        self.release();
        let result = self
            .callback
            .reached_limit_request()
            .send()
            .promise
            .await
            .and_then(|r| r.get()?.get_next());
        if let Some(tx) = self.resolve.borrow_mut().take() {
            let _ = tx.send(result.clone());
        }
        let client = result?;
        state.next = Some(self.parent.factory.to_output(client));
        state.failed = false;
        Ok(())
    }
}
impl OutputStream for Substream {
    fn write<'a>(&'a self, b: &'a [u8]) -> LocalBoxFuture<'a, Result<()>> {
        async move {
            let mut state = self.state.lock().await;
            if state.ended || state.failed {
                return Err(invalid("substream is closed"));
            }
            if let Some(next) = &state.next {
                return next.write(b).await;
            }
            let n = usize::try_from((self.limit - state.count).min(b.len() as u64)).unwrap();
            if n > 0 {
                state.failed = true;
                self.parent.output.write(&b[..n]).await?;
                state.count += n as u64;
                state.failed = false;
            }
            if state.count == self.limit {
                self.redirect(&mut state).await?;
                if n < b.len() {
                    state.next.as_ref().unwrap().write(&b[n..]).await?;
                }
            }
            Ok(())
        }
        .boxed_local()
    }
    fn end(&self) -> LocalBoxFuture<'_, Result<()>> {
        async move {
            let mut state = self.state.lock().await;
            if state.ended || state.failed {
                return Err(invalid("substream is closed"));
            }
            state.ended = true;
            if let Some(next) = &state.next {
                return next.end().await;
            }
            if state.count == self.limit {
                self.redirect(&mut state).await?;
                return state.next.as_ref().unwrap().end().await;
            }
            let mut r = self.callback.ended_request();
            r.get().set_byte_count(state.count);
            self.release();
            r.send().promise.await?;
            Ok(())
        }
        .boxed_local()
    }
    fn abort(&self) {
        self.release();
        if let Some(mut state) = self.state.try_lock() {
            if !state.ended {
                state.failed = true;
                if let Some(next) = &state.next {
                    next.abort();
                } else {
                    self.parent.output.abort();
                    self.parent.ended.set(true);
                }
            }
        }
    }
    fn start_tls<'a>(&'a self, h: &'a str) -> LocalBoxFuture<'a, Result<()>> {
        async move {
            let state = self.state.lock().await;
            if state.ended || state.failed {
                return Err(invalid("substream closed"));
            }
            if let Some(next) = &state.next {
                next.start_tls(h).await
            } else {
                self.parent.output.start_tls(h).await
            }
        }
        .boxed_local()
    }
    fn shorten_path(&self) -> Option<Promise<capnp::capability::Client, capnp::Error>> {
        let future = self.shortened.clone();
        Some(Promise::from_future(async move {
            Ok(capnp::capability::Client::new(
                future.await?.into_client_hook(),
            ))
        }))
    }
}
impl Drop for Substream {
    fn drop(&mut self) {
        self.release();
    }
}

/// Own an output endpoint with explicit end. Drop always aborts if end did not finish.
pub struct ExplicitEndOutputStream {
    output: Rc<dyn OutputStream>,
    ended: bool,
}
impl ExplicitEndOutputStream {
    pub fn new(output: Rc<dyn OutputStream>) -> Self {
        Self {
            output,
            ended: false,
        }
    }
    pub async fn write(&mut self, bytes: &[u8]) -> Result<()> {
        if self.ended {
            return Err(invalid("stream ended"));
        }
        self.output.write(bytes).await
    }
    pub async fn end(&mut self) -> Result<()> {
        if self.ended {
            return Err(invalid("stream ended"));
        }
        self.output.end().await?;
        self.ended = true;
        Ok(())
    }
    pub async fn start_tls(&mut self, hostname: &str) -> Result<()> {
        if self.ended {
            return Err(invalid("stream ended"));
        }
        self.output.start_tls(hostname).await
    }
}
impl Drop for ExplicitEndOutputStream {
    fn drop(&mut self) {
        if !self.ended {
            self.output.abort();
        }
    }
}

/// futures I/O facade. `close()` sends explicit EOF; Drop aborts by default.
/// The optional executor supports C++'s legacy EOF-on-drop convention explicitly.
pub struct OutputIo {
    output: Rc<dyn OutputStream>,
    pending: Option<LocalBoxFuture<'static, Result<usize>>>,
    closed: bool,
    failed: bool,
    eof_on_drop: Option<Rc<dyn capnp::capability::CallExecutor>>,
}
impl OutputIo {
    pub fn new(output: Rc<dyn OutputStream>) -> Self {
        Self {
            output,
            pending: None,
            closed: false,
            failed: false,
            eof_on_drop: None,
        }
    }
    /// Opt into the legacy asynchronous destructor convention. The executor
    /// owns detached EOF and its errors. A canceled write still aborts.
    pub fn with_eof_on_drop(mut self, executor: Rc<dyn capnp::capability::CallExecutor>) -> Self {
        self.eof_on_drop = Some(executor);
        self
    }
    fn poll_pending(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<usize>> {
        if self.failed {
            return std::task::Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "ByteStream failed",
            )));
        }
        if let Some(pending) = &mut self.pending {
            let result = futures::ready!(pending.as_mut().poll(cx));
            self.pending = None;
            if let Err(error) = result {
                self.failed = true;
                self.output.abort();
                return std::task::Poll::Ready(Err(std::io::Error::other(error.to_string())));
            }
            return std::task::Poll::Ready(Ok(result.unwrap()));
        }
        std::task::Poll::Ready(Ok(0))
    }
}
impl futures::AsyncWrite for OutputIo {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        if self.closed {
            return std::task::Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "ByteStream closed",
            )));
        }
        if self.pending.is_none() && !bytes.is_empty() {
            let bytes = bytes[..bytes.len().min(64 * 1024)].to_vec();
            let output = self.output.clone();
            self.pending = Some(
                async move {
                    output.write(&bytes).await?;
                    Ok(bytes.len())
                }
                .boxed_local(),
            );
        }
        self.poll_pending(cx)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.poll_pending(cx).map(|r| r.map(|_| ()))
    }
    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        futures::ready!(self.poll_pending(cx))?;
        if !self.closed {
            self.closed = true;
            let output = self.output.clone();
            self.pending = Some(
                async move {
                    output.end().await?;
                    Ok(0)
                }
                .boxed_local(),
            );
        }
        self.poll_pending(cx).map(|r| r.map(|_| ()))
    }
}
impl Drop for OutputIo {
    fn drop(&mut self) {
        if self.failed {
            return;
        }
        if self.pending.is_some() {
            self.pending.take();
            self.output.abort();
        } else if !self.closed {
            if let Some(executor) = &self.eof_on_drop {
                let output = self.output.clone();
                if executor
                    .spawn(Promise::from_future(async move { output.end().await }))
                    .is_err()
                {
                    self.output.abort();
                }
            } else {
                self.output.abort();
            }
        }
    }
}
