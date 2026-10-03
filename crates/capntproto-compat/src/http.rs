//! Level-2 HTTP-over-Cap'n-Proto, with streamed bodies, upgrades and CONNECT.
use crate::{
    byte_stream::{ByteStreamFactory, OutputStream},
    http_over_capnp_capnp as wire, invalid,
    websocket::Message,
};
use capnp::Result;
use futures::{
    channel::{mpsc, oneshot},
    future::LocalBoxFuture,
    lock::Mutex,
    AsyncRead, AsyncReadExt, FutureExt, StreamExt,
};
use std::{
    cell::Cell,
    io,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
};
pub use wire::HttpMethod;
pub type Headers = Vec<(String, String)>;
pub type Body = Box<dyn AsyncRead + Unpin>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodySize {
    Unknown,
    Fixed(u64),
}
#[derive(Clone, Debug)]
pub struct Request {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Headers,
    pub body_size: BodySize,
}
#[derive(Clone, Debug)]
pub struct Response {
    pub status_code: u16,
    pub status_text: String,
    pub headers: Headers,
    pub body_size: BodySize,
}
#[derive(Clone, Debug)]
pub struct ConnectRequest {
    pub host: String,
    pub headers: Headers,
    pub use_tls: bool,
}
/// One direction of a WebSocket. Receivers must honor backpressure and close.
pub trait WebSocketSink: 'static {
    fn send(&self, message: Message) -> LocalBoxFuture<'_, Result<()>>;
}
pub trait ResponseSender: 'static {
    fn start_response(
        &self,
        response: Response,
    ) -> LocalBoxFuture<'_, Result<Rc<dyn OutputStream>>>;
    fn start_websocket(
        &self,
        headers: Headers,
        up: Rc<dyn WebSocketSink>,
    ) -> LocalBoxFuture<'_, Result<Rc<dyn WebSocketSink>>>;
}
pub trait ConnectResponseSender: 'static {
    fn start_connect(&self, response: Response) -> LocalBoxFuture<'_, Result<()>>;
    fn start_error(&self, response: Response) -> LocalBoxFuture<'_, Result<Rc<dyn OutputStream>>>;
}
pub struct Connection {
    pub up: Rc<dyn OutputStream>,
    pub completion: LocalBoxFuture<'static, Result<()>>,
}
pub trait HttpService: 'static {
    fn request(
        &self,
        request: Request,
        body: Body,
        response: Rc<dyn ResponseSender>,
    ) -> LocalBoxFuture<'_, Result<()>>;
    fn connect(
        &self,
        _request: ConnectRequest,
        _down: Rc<dyn OutputStream>,
        _response: Rc<dyn ConnectResponseSender>,
    ) -> LocalBoxFuture<'_, Result<Connection>> {
        async {
            Err(capnp::Error::unimplemented(
                "CONNECT not implemented".into(),
            ))
        }
        .boxed_local()
    }
}
#[derive(Clone, Default)]
pub struct HttpOverCapnpFactory {
    pub streams: ByteStreamFactory,
}
impl HttpOverCapnpFactory {
    pub fn new(streams: ByteStreamFactory) -> Self {
        Self { streams }
    }
    pub fn from_service(&self, service: Rc<dyn HttpService>) -> wire::http_service::Client {
        capnp_rpc::new_client(ServiceServer {
            service,
            factory: self.clone(),
        })
    }
    pub fn to_service(&self, client: wire::http_service::Client) -> Rc<dyn HttpService> {
        Rc::new(RpcService {
            client,
            factory: self.clone(),
        })
    }
}
fn body_size(v: wire::http_request::body_size::Reader<'_>) -> Result<BodySize> {
    Ok(match v.which()? {
        wire::http_request::body_size::Unknown(()) => BodySize::Unknown,
        wire::http_request::body_size::Fixed(n) => BodySize::Fixed(n),
    })
}
fn read_request(v: wire::http_request::Reader<'_>) -> Result<Request> {
    Ok(Request {
        method: v.get_method()?,
        url: v.get_url()?.to_str()?.into(),
        headers: decode_headers(v.get_headers()?)?,
        body_size: body_size(v.get_body_size())?,
    })
}
fn write_request(v: &Request, mut out: wire::http_request::Builder<'_>) -> Result<()> {
    out.set_method(v.method);
    out.set_url(&v.url);
    encode_headers(
        &v.headers,
        out.reborrow().init_headers(
            v.headers
                .len()
                .try_into()
                .map_err(|_| invalid("too many headers"))?,
        ),
    )?;
    match v.body_size {
        BodySize::Unknown => out.init_body_size().set_unknown(()),
        BodySize::Fixed(n) => out.init_body_size().set_fixed(n),
    }
    Ok(())
}
fn read_response(v: wire::http_response::Reader<'_>) -> Result<Response> {
    Ok(Response {
        status_code: v.get_status_code(),
        status_text: v.get_status_text()?.to_str()?.into(),
        headers: decode_headers(v.get_headers()?)?,
        body_size: match v.get_body_size().which()? {
            wire::http_response::body_size::Unknown(()) => BodySize::Unknown,
            wire::http_response::body_size::Fixed(n) => BodySize::Fixed(n),
        },
    })
}
fn write_response(v: &Response, mut out: wire::http_response::Builder<'_>) -> Result<()> {
    out.set_status_code(v.status_code);
    out.set_status_text(&v.status_text);
    encode_headers(
        &v.headers,
        out.reborrow().init_headers(
            v.headers
                .len()
                .try_into()
                .map_err(|_| invalid("too many headers"))?,
        ),
    )?;
    match v.body_size {
        BodySize::Unknown => out.init_body_size().set_unknown(()),
        BodySize::Fixed(n) => out.init_body_size().set_fixed(n),
    }
    Ok(())
}
fn check_response(response: &Response, head: bool) -> Result<()> {
    if (head || matches!(response.status_code, 204 | 205 | 304))
        && response.body_size != BodySize::Fixed(0)
    {
        return Err(invalid(
            "HEAD/204/205/304 response must have zero body size",
        ));
    }
    Ok(())
}

const HEADER_NAMES: &[&str] = &[
    "",
    "Accept-Charset",
    "Accept-Encoding",
    "Accept-Language",
    "Accept-Ranges",
    "Accept",
    "Access-Control-Allow-Origin",
    "Age",
    "Allow",
    "Authorization",
    "Cache-Control",
    "Content-Disposition",
    "Content-Encoding",
    "Content-Language",
    "Content-Length",
    "Content-Location",
    "Content-Range",
    "Content-Type",
    "Cookie",
    "Date",
    "ETag",
    "Expect",
    "Expires",
    "From",
    "Host",
    "If-Match",
    "If-Modified-Since",
    "If-None-Match",
    "If-Range",
    "If-Unmodified-Since",
    "Last-Modified",
    "Link",
    "Location",
    "Max-Forwards",
    "Proxy-Authenticate",
    "Proxy-Authorization",
    "Range",
    "Referer",
    "Refresh",
    "Retry-After",
    "Server",
    "Set-Cookie",
    "Strict-Transport-Security",
    "Transfer-Encoding",
    "User-Agent",
    "Vary",
    "Via",
    "WWW-Authenticate",
];
fn check_header(name: &str, value: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
        || value.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0)
    {
        Err(invalid("invalid HTTP header"))
    } else {
        Ok(())
    }
}
pub fn encode_headers(
    headers: &Headers,
    mut out: capnp::struct_list::Builder<'_, wire::http_header::Owned>,
) -> Result<()> {
    if headers.len() != out.len() as usize || headers.len() > 4096 {
        return Err(invalid("header count mismatch/limit"));
    }
    for (i, (name, value)) in headers.iter().enumerate() {
        check_header(name, value)?;
        let item = out.reborrow().get(i as u32);
        if let Some(n) = HEADER_NAMES
            .iter()
            .position(|h| h.eq_ignore_ascii_case(name))
        {
            let mut common = item.init_common();
            common.set_name((n as u16).try_into()?);
            if value == "gzip, deflate" {
                common.set_common_value(wire::CommonHeaderValue::GzipDeflate);
            } else {
                common.set_value(value);
            }
        } else {
            let mut h = item.init_uncommon();
            h.set_name(name);
            h.set_value(value);
        }
    }
    Ok(())
}
pub fn decode_headers(
    headers: capnp::struct_list::Reader<'_, wire::http_header::Owned>,
) -> Result<Headers> {
    if headers.len() > 4096 {
        return Err(invalid("too many HTTP headers"));
    }
    let mut result = vec![];
    for h in headers {
        let (name, value): (String, String) = match h.which()? {
            wire::http_header::Common(h) => {
                let n = h.get_name()? as usize;
                let name = HEADER_NAMES
                    .get(n)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| invalid("invalid common header name"))?
                    .to_string();
                let value = match h.which()? {
                    wire::http_header::common::CommonValue(v) => match v? {
                        wire::CommonHeaderValue::GzipDeflate => "gzip, deflate".into(),
                        wire::CommonHeaderValue::Invalid => {
                            return Err(invalid("invalid common header value"))
                        }
                    },
                    wire::http_header::common::Value(v) => v?.to_str()?.into(),
                };
                (name, value)
            }
            wire::http_header::Uncommon(h) => {
                let h = h?;
                (
                    h.get_name()?.to_str()?.into(),
                    h.get_value()?.to_str()?.into(),
                )
            }
        };
        check_header(&name, &value)?;
        result.push((name, value));
    }
    Ok(result)
}
struct ServiceServer {
    service: Rc<dyn HttpService>,
    factory: HttpOverCapnpFactory,
}
impl wire::http_service::Server for ServiceServer {
    async fn request(
        self: Rc<Self>,
        params: wire::http_service::RequestParams,
        mut results: wire::http_service::RequestResults,
    ) -> Result<()> {
        let p = params.get()?;
        let request = read_request(p.get_request()?)?;
        let context = p.get_context()?;
        let response = Rc::new(RpcResponse {
            client: context,
            factory: self.factory.clone(),
            started: Cell::new(false),
            head: request.method == HttpMethod::Head,
        });
        let body = if request.body_size == BodySize::Fixed(0) {
            Box::new(futures::io::Cursor::new(Vec::<u8>::new())) as Body
        } else {
            let (body, output) = body_pipe(request.body_size);
            results
                .get()
                .set_request_body(self.factory.streams.from_output(output));
            body
        };
        results.set_pipeline()?;
        self.service
            .request(request, body, response.clone())
            .await?;
        if !response.started.get() {
            return Err(invalid("HTTP service completed without response"));
        }
        Ok(())
    }
    async fn connect(
        self: Rc<Self>,
        params: wire::http_service::ConnectParams,
        mut results: wire::http_service::ConnectResults,
    ) -> Result<()> {
        let p = params.get()?;
        let request = ConnectRequest {
            host: p.get_host()?.to_str()?.into(),
            headers: decode_headers(p.get_headers()?)?,
            use_tls: p.get_settings()?.get_use_tls(),
        };
        let response = Rc::new(RpcConnectResponse {
            client: p.get_context()?,
            factory: self.factory.clone(),
            started: Cell::new(false),
        });
        let connection = self
            .service
            .connect(
                request,
                self.factory.streams.to_output(p.get_down()?),
                response.clone(),
            )
            .await?;
        results
            .get()
            .set_up(self.factory.streams.from_output(connection.up));
        results.set_pipeline()?;
        connection.completion.await?;
        if !response.started.get() {
            return Err(invalid("CONNECT completed without response"));
        }
        Ok(())
    }
}
struct RpcService {
    client: wire::http_service::Client,
    factory: HttpOverCapnpFactory,
}
impl HttpService for RpcService {
    fn request(
        &self,
        request: Request,
        mut body: Body,
        response: Rc<dyn ResponseSender>,
    ) -> LocalBoxFuture<'_, Result<()>> {
        async move {
            let mut r = self.client.request_request();
            write_request(&request, r.get().init_request())?;
            r.get().set_context(capnp_rpc::new_client(ResponseServer {
                inner: response,
                factory: self.factory.clone(),
                started: Cell::new(false),
                head: request.method == HttpMethod::Head,
            }));
            let call = r.send();
            let up = call.pipeline.get_request_body();
            let pump = async {
                if request.body_size == BodySize::Fixed(0) {
                    return Ok(());
                }
                let output = self.factory.streams.to_output(up);
                let mut output = crate::byte_stream::ExplicitEndOutputStream::new(output);
                let mut bytes = [0; 64 * 1024];
                let mut count = 0u64;
                loop {
                    let n = body.read(&mut bytes).await?;
                    if n == 0 {
                        break;
                    }
                    count = count
                        .checked_add(n as u64)
                        .ok_or_else(|| invalid("body size overflow"))?;
                    if matches!(request.body_size,BodySize::Fixed(size) if count>size) {
                        return Err(invalid("request body exceeds declared length"));
                    }
                    output.write(&bytes[..n]).await?;
                }
                if matches!(request.body_size,BodySize::Fixed(size) if count!=size) {
                    return Err(invalid("truncated request body"));
                }
                output.end().await
            };
            futures::try_join!(pump, async { call.promise.await.map(|_| ()) })?;
            Ok(())
        }
        .boxed_local()
    }
    fn connect(
        &self,
        request: ConnectRequest,
        down: Rc<dyn OutputStream>,
        response: Rc<dyn ConnectResponseSender>,
    ) -> LocalBoxFuture<'_, Result<Connection>> {
        async move {
            let mut r = self.client.connect_request();
            {
                let mut p = r.get();
                p.set_host(&request.host);
                encode_headers(
                    &request.headers,
                    p.reborrow().init_headers(
                        request
                            .headers
                            .len()
                            .try_into()
                            .map_err(|_| invalid("too many headers"))?,
                    ),
                )?;
                p.reborrow().init_settings().set_use_tls(request.use_tls);
                p.set_down(self.factory.streams.from_output(down));
                p.set_context(capnp_rpc::new_client(ConnectResponseServer {
                    inner: response,
                    factory: self.factory.clone(),
                    started: Cell::new(false),
                }));
            }
            let call = r.send();
            Ok(Connection {
                up: self.factory.streams.to_output(call.pipeline.get_up()),
                completion: async move {
                    call.promise.await?;
                    Ok(())
                }
                .boxed_local(),
            })
        }
        .boxed_local()
    }
}
fn start(started: &Cell<bool>) -> Result<()> {
    if started.replace(true) {
        Err(invalid("response already started"))
    } else {
        Ok(())
    }
}
struct RpcResponse {
    client: wire::http_service::client_request_context::Client,
    factory: HttpOverCapnpFactory,
    started: Cell<bool>,
    head: bool,
}
impl ResponseSender for RpcResponse {
    fn start_response(
        &self,
        response: Response,
    ) -> LocalBoxFuture<'_, Result<Rc<dyn OutputStream>>> {
        async move {
            check_response(&response, self.head)?;
            start(&self.started)?;
            let mut r = self.client.start_response_request();
            write_response(&response, r.get().init_response())?;
            let result = r.send().promise.await?;
            Ok(if response.body_size == BodySize::Fixed(0) {
                Rc::new(EmptyOutput) as Rc<dyn OutputStream>
            } else {
                checked_output(
                    self.factory.streams.to_output(result.get()?.get_body()?),
                    response.body_size,
                )
            })
        }
        .boxed_local()
    }
    fn start_websocket(
        &self,
        headers: Headers,
        up: Rc<dyn WebSocketSink>,
    ) -> LocalBoxFuture<'_, Result<Rc<dyn WebSocketSink>>> {
        async move {
            start(&self.started)?;
            let mut r = self.client.start_web_socket_request();
            encode_headers(
                &headers,
                r.get().init_headers(
                    headers
                        .len()
                        .try_into()
                        .map_err(|_| invalid("too many headers"))?,
                ),
            )?;
            r.get().set_up_socket(capnp_rpc::new_client(SocketServer {
                inner: up,
                closed: Cell::new(false),
            }));
            Ok(Rc::new(RpcSocket {
                client: r.send().promise.await?.get()?.get_down_socket()?,
                closed: Cell::new(false),
            }) as Rc<dyn WebSocketSink>)
        }
        .boxed_local()
    }
}
struct ResponseServer {
    inner: Rc<dyn ResponseSender>,
    factory: HttpOverCapnpFactory,
    started: Cell<bool>,
    head: bool,
}
impl wire::http_service::client_request_context::Server for ResponseServer {
    async fn start_response(
        self: Rc<Self>,
        params: wire::http_service::client_request_context::StartResponseParams,
        mut results: wire::http_service::client_request_context::StartResponseResults,
    ) -> Result<()> {
        let response = read_response(params.get()?.get_response()?)?;
        check_response(&response, self.head)?;
        start(&self.started)?;
        let size = response.body_size;
        let output = self.inner.start_response(response).await?;
        if size != BodySize::Fixed(0) {
            results.get().set_body(
                self.factory
                    .streams
                    .from_output(checked_output(output, size)),
            );
        } else {
            output.end().await?;
        }
        Ok(())
    }
    async fn start_web_socket(
        self: Rc<Self>,
        params: wire::http_service::client_request_context::StartWebSocketParams,
        mut results: wire::http_service::client_request_context::StartWebSocketResults,
    ) -> Result<()> {
        start(&self.started)?;
        let p = params.get()?;
        let down = self
            .inner
            .start_websocket(
                decode_headers(p.get_headers()?)?,
                Rc::new(RpcSocket {
                    client: p.get_up_socket()?,
                    closed: Cell::new(false),
                }),
            )
            .await?;
        results
            .get()
            .set_down_socket(capnp_rpc::new_client(SocketServer {
                inner: down,
                closed: Cell::new(false),
            }));
        Ok(())
    }
}
struct RpcConnectResponse {
    client: wire::http_service::connect_client_request_context::Client,
    factory: HttpOverCapnpFactory,
    started: Cell<bool>,
}
impl ConnectResponseSender for RpcConnectResponse {
    fn start_connect(&self, response: Response) -> LocalBoxFuture<'_, Result<()>> {
        async move {
            start(&self.started)?;
            let mut r = self.client.start_connect_request();
            write_response(&response, r.get().init_response())?;
            r.send().promise.await?;
            Ok(())
        }
        .boxed_local()
    }
    fn start_error(&self, response: Response) -> LocalBoxFuture<'_, Result<Rc<dyn OutputStream>>> {
        async move {
            start(&self.started)?;
            let mut r = self.client.start_error_request();
            write_response(&response, r.get().init_response())?;
            let result = r.send().promise.await?;
            Ok(if response.body_size == BodySize::Fixed(0) {
                Rc::new(EmptyOutput) as Rc<dyn OutputStream>
            } else {
                checked_output(
                    self.factory.streams.to_output(result.get()?.get_body()?),
                    response.body_size,
                )
            })
        }
        .boxed_local()
    }
}
struct ConnectResponseServer {
    inner: Rc<dyn ConnectResponseSender>,
    factory: HttpOverCapnpFactory,
    started: Cell<bool>,
}
impl wire::http_service::connect_client_request_context::Server for ConnectResponseServer {
    async fn start_connect(
        self: Rc<Self>,
        params: wire::http_service::connect_client_request_context::StartConnectParams,
        _: wire::http_service::connect_client_request_context::StartConnectResults,
    ) -> Result<()> {
        start(&self.started)?;
        self.inner
            .start_connect(read_response(params.get()?.get_response()?)?)
            .await
    }
    async fn start_error(
        self: Rc<Self>,
        params: wire::http_service::connect_client_request_context::StartErrorParams,
        mut results: wire::http_service::connect_client_request_context::StartErrorResults,
    ) -> Result<()> {
        start(&self.started)?;
        let r = read_response(params.get()?.get_response()?)?;
        let size = r.body_size;
        let output = self.inner.start_error(r).await?;
        if size != BodySize::Fixed(0) {
            results.get().set_body(
                self.factory
                    .streams
                    .from_output(checked_output(output, size)),
            );
        } else {
            output.end().await?;
        }
        Ok(())
    }
}
struct RpcSocket {
    client: wire::web_socket::Client,
    closed: Cell<bool>,
}
impl WebSocketSink for RpcSocket {
    fn send(&self, message: Message) -> LocalBoxFuture<'_, Result<()>> {
        async move {
            if self.closed.get() {
                return Err(invalid("WebSocket is closed"));
            }
            match message {
                Message::Text(text) => {
                    let mut r = self.client.send_text_request();
                    r.get().set_text(&text);
                    r.send().await
                }
                Message::Binary(data) => {
                    let mut r = self.client.send_data_request();
                    r.get().set_data(&data);
                    r.send().await
                }
                Message::Close { code, reason } => {
                    self.closed.set(true);
                    let mut r = self.client.close_request();
                    r.get().set_code(code.unwrap_or(1005));
                    r.get().set_reason(&reason);
                    r.send().promise.await?;
                    Ok(())
                }
            }
        }
        .boxed_local()
    }
}
struct SocketServer {
    inner: Rc<dyn WebSocketSink>,
    closed: Cell<bool>,
}
impl wire::web_socket::Server for SocketServer {
    async fn send_text(self: Rc<Self>, p: wire::web_socket::SendTextParams) -> Result<()> {
        if self.closed.get() {
            return Err(invalid("WebSocket is closed"));
        }
        self.inner
            .send(Message::Text(p.get()?.get_text()?.to_str()?.into()))
            .await
    }
    async fn send_data(self: Rc<Self>, p: wire::web_socket::SendDataParams) -> Result<()> {
        if self.closed.get() {
            return Err(invalid("WebSocket is closed"));
        }
        self.inner
            .send(Message::Binary(p.get()?.get_data()?.into()))
            .await
    }
    async fn close(
        self: Rc<Self>,
        p: wire::web_socket::CloseParams,
        _: wire::web_socket::CloseResults,
    ) -> Result<()> {
        start(&self.closed)?;
        let p = p.get()?;
        self.inner
            .send(Message::Close {
                code: if p.get_code() == 1005 {
                    None
                } else {
                    Some(p.get_code())
                },
                reason: p.get_reason()?.to_str()?.into(),
            })
            .await
    }
}
struct EmptyOutput;
impl OutputStream for EmptyOutput {
    fn write<'a>(&'a self, b: &'a [u8]) -> LocalBoxFuture<'a, Result<()>> {
        async move {
            if b.is_empty() {
                Ok(())
            } else {
                Err(invalid("zero-length HTTP body"))
            }
        }
        .boxed_local()
    }
    fn end(&self) -> LocalBoxFuture<'_, Result<()>> {
        async { Ok(()) }.boxed_local()
    }
    fn abort(&self) {}
}
struct CheckedOutput {
    inner: Rc<dyn OutputStream>,
    size: BodySize,
    state: Mutex<(u64, bool)>,
}
fn checked_output(inner: Rc<dyn OutputStream>, size: BodySize) -> Rc<dyn OutputStream> {
    Rc::new(CheckedOutput {
        inner,
        size,
        state: Mutex::new((0, false)),
    })
}
impl OutputStream for CheckedOutput {
    fn write<'a>(&'a self, b: &'a [u8]) -> LocalBoxFuture<'a, Result<()>> {
        async move {
            let mut state = self.state.lock().await;
            if state.1 {
                return Err(invalid("HTTP body closed"));
            }
            let n = state
                .0
                .checked_add(b.len() as u64)
                .ok_or_else(|| invalid("body length overflow"))?;
            if matches!(self.size,BodySize::Fixed(size) if n>size) {
                return Err(invalid("HTTP body exceeds declared size"));
            }
            state.1 = true;
            self.inner.write(b).await?;
            *state = (n, false);
            Ok(())
        }
        .boxed_local()
    }
    fn end(&self) -> LocalBoxFuture<'_, Result<()>> {
        async move {
            let mut state = self.state.lock().await;
            if state.1 {
                return Err(invalid("HTTP body closed"));
            }
            state.1 = true;
            if matches!(self.size,BodySize::Fixed(size) if state.0!=size) {
                return Err(invalid("truncated HTTP body"));
            }
            self.inner.end().await
        }
        .boxed_local()
    }
    fn abort(&self) {
        self.inner.abort();
    }
}
struct Chunk {
    bytes: Vec<u8>,
    ack: Option<oneshot::Sender<()>>,
}
struct PipeOutput {
    sender: mpsc::UnboundedSender<Result<Option<Chunk>>>,
    serial: Mutex<()>,
    ended: Cell<bool>,
}
impl OutputStream for PipeOutput {
    fn write<'a>(&'a self, bytes: &'a [u8]) -> LocalBoxFuture<'a, Result<()>> {
        async move {
            let _lock = self.serial.lock().await;
            if self.ended.get() {
                return Err(invalid("body closed"));
            }
            for bytes in bytes.chunks(64 * 1024) {
                let (tx, rx) = oneshot::channel();
                self.sender
                    .unbounded_send(Ok(Some(Chunk {
                        bytes: bytes.into(),
                        ack: Some(tx),
                    })))
                    .map_err(|_| invalid("body reader dropped"))?;
                rx.await.map_err(|_| invalid("body reader dropped"))?;
            }
            Ok(())
        }
        .boxed_local()
    }
    fn end(&self) -> LocalBoxFuture<'_, Result<()>> {
        async move {
            let _lock = self.serial.lock().await;
            if self.ended.replace(true) {
                return Err(invalid("body already closed"));
            }
            self.sender
                .unbounded_send(Ok(None))
                .map_err(|_| invalid("body reader dropped"))
        }
        .boxed_local()
    }
    fn abort(&self) {
        if !self.ended.replace(true) {
            let _ = self
                .sender
                .unbounded_send(Err(invalid("HTTP body aborted before explicit end")));
        }
    }
}
impl Drop for PipeOutput {
    fn drop(&mut self) {
        self.abort();
    }
}
struct PipeBody {
    receiver: mpsc::UnboundedReceiver<Result<Option<Chunk>>>,
    chunk: Option<Chunk>,
    pos: usize,
    done: bool,
}
impl AsyncRead for PipeBody {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if out.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if self.done {
            return Poll::Ready(Ok(0));
        }
        if self.chunk.is_none() {
            match futures::ready!(self.receiver.poll_next_unpin(cx)) {
                Some(Ok(Some(chunk))) => {
                    self.chunk = Some(chunk);
                    self.pos = 0;
                }
                Some(Ok(None)) => {
                    self.done = true;
                    return Poll::Ready(Ok(0));
                }
                error => {
                    self.done = true;
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        match error {
                            Some(Err(e)) => e.to_string(),
                            _ => "body sender disconnected before end".into(),
                        },
                    )));
                }
            }
        }
        let chunk = self.chunk.as_ref().unwrap();
        let n = out.len().min(chunk.bytes.len() - self.pos);
        out[..n].copy_from_slice(&chunk.bytes[self.pos..self.pos + n]);
        self.pos += n;
        if self.pos == self.chunk.as_ref().unwrap().bytes.len() {
            if let Some(ack) = self.chunk.take().unwrap().ack {
                let _ = ack.send(());
            }
        }
        Poll::Ready(Ok(n))
    }
}
/// Bounded, backpressured body pipe. The reader observes missing explicit end as truncation.
pub fn body_pipe(size: BodySize) -> (Body, Rc<dyn OutputStream>) {
    let (sender, receiver) = mpsc::unbounded();
    (
        Box::new(PipeBody {
            receiver,
            chunk: None,
            pos: 0,
            done: false,
        }),
        checked_output(
            Rc::new(PipeOutput {
                sender,
                serial: Mutex::new(()),
                ended: Cell::new(false),
            }),
            size,
        ),
    )
}
