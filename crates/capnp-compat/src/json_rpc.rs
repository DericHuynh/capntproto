//! Schema-based, bidirectional JSON-RPC 2.0. Poll the returned driver with the
//! application's executor. No capabilities, promise pipelines or batches cross JSON.
use crate::{
    invalid,
    json::{self, JsonCodec, Value},
};
use capnp::{
    capability::{Client, FromClientHook, Promise},
    schema_loader::{
        dynamic::{self, ServiceSchema},
        Method, Schema,
    },
    Result,
};
use futures::{
    channel::oneshot, future::LocalBoxFuture, lock::Mutex, stream::FuturesUnordered, AsyncRead,
    AsyncReadExt, AsyncWrite, AsyncWriteExt, FutureExt, StreamExt,
};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};

pub trait Transport: 'static {
    fn send<'a>(&'a self, text: &'a str) -> LocalBoxFuture<'a, Result<()>>;
    fn receive(&self) -> LocalBoxFuture<'_, Result<String>>;
}
/// VS Code / language-server framing. Header and body lengths are bounded.
pub struct ContentLengthTransport<R, W> {
    input: Mutex<futures::io::BufReader<R>>,
    output: Mutex<W>,
    pub max_message_bytes: usize,
    pub max_header_bytes: usize,
}
impl<R: AsyncRead + Unpin, W> ContentLengthTransport<R, W> {
    pub fn new(input: R, output: W) -> Self {
        Self {
            input: Mutex::new(futures::io::BufReader::new(input)),
            output: Mutex::new(output),
            max_message_bytes: 4 * 1024 * 1024,
            max_header_bytes: 16 * 1024,
        }
    }
}
impl<R: AsyncRead + Unpin + 'static, W: AsyncWrite + Unpin + 'static> Transport
    for ContentLengthTransport<R, W>
{
    fn send<'a>(&'a self, text: &'a str) -> LocalBoxFuture<'a, Result<()>> {
        async move {
            if text.len() > self.max_message_bytes {
                return Err(invalid("JSON-RPC message exceeds limit"));
            }
            let mut out = self.output.lock().await;
            out.write_all(format!("Content-Length: {}\r\n\r\n", text.len()).as_bytes())
                .await?;
            out.write_all(text.as_bytes()).await?;
            out.flush().await?;
            Ok(())
        }
        .boxed_local()
    }
    fn receive(&self) -> LocalBoxFuture<'_, Result<String>> {
        async move {
            let mut input = self.input.lock().await;
            let mut bytes = Vec::new();
            loop {
                if bytes.len() >= self.max_header_bytes {
                    return Err(invalid("JSON-RPC header exceeds limit"));
                }
                let mut byte = [0];
                input.read_exact(&mut byte).await?;
                bytes.push(byte[0]);
                if bytes.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            let text =
                std::str::from_utf8(&bytes).map_err(|_| invalid("invalid header encoding"))?;
            let mut length = None;
            for line in text[..text.len() - 4].split("\r\n") {
                let (name, value) = line
                    .split_once(':')
                    .ok_or_else(|| invalid("invalid JSON-RPC header"))?;
                if name.eq_ignore_ascii_case("content-length") {
                    if length.is_some() {
                        return Err(invalid("duplicate Content-Length"));
                    }
                    let value = value.trim();
                    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                        return Err(invalid("invalid Content-Length"));
                    }
                    length = Some(
                        value
                            .parse::<usize>()
                            .map_err(|_| invalid("Content-Length overflow"))?,
                    );
                } else if name.eq_ignore_ascii_case("transfer-encoding") {
                    return Err(invalid("Transfer-Encoding not supported"));
                }
            }
            let length = length.ok_or_else(|| invalid("missing Content-Length"))?;
            if length > self.max_message_bytes {
                return Err(invalid("JSON-RPC body exceeds limit"));
            }
            let mut bytes = vec![0; length];
            input.read_exact(&mut bytes).await?;
            String::from_utf8(bytes).map_err(|_| invalid("invalid JSON-RPC UTF-8"))
        }
        .boxed_local()
    }
}
/// A capability exposed to the JSON peer, together with owned runtime schema metadata.
pub struct Endpoint {
    pub schema: ServiceSchema,
    pub client: Client,
}
struct State {
    transport: Rc<dyn Transport>,
    writer: Mutex<()>,
    next: Cell<u64>,
    pending: RefCell<HashMap<u64, oneshot::Sender<Result<Value>>>>,
    failure: RefCell<Option<capnp::Error>>,
    stop: RefCell<Option<oneshot::Sender<capnp::Error>>>,
    max_calls: usize,
}
impl State {
    fn fail(&self, error: capnp::Error) {
        if self.failure.borrow().is_none() {
            *self.failure.borrow_mut() = Some(error.clone());
        }
        for (_, sender) in self.pending.borrow_mut().drain() {
            let _ = sender.send(Err(error.clone()));
        }
        if let Some(stop) = self.stop.borrow_mut().take() {
            let _ = stop.send(error);
        }
    }
    async fn send(&self, value: &Value) -> Result<()> {
        if let Some(e) = self.failure.borrow().clone() {
            return Err(e);
        }
        let text = JsonCodec::new().stringify(value)?;
        let _lock = self.writer.lock().await;
        if let Some(e) = self.failure.borrow().clone() {
            return Err(e);
        }
        // A canceled partially written frame cannot be followed by another frame.
        let mut guard = WriteGuard {
            state: self,
            complete: false,
        };
        if let Err(e) = self.transport.send(&text).await {
            self.fail(e.clone());
            return Err(e);
        }
        guard.complete = true;
        Ok(())
    }
}
struct WriteGuard<'a> {
    state: &'a State,
    complete: bool,
}
impl Drop for WriteGuard<'_> {
    fn drop(&mut self) {
        if !self.complete {
            self.state.fail(capnp::Error::disconnected(
                "JSON-RPC write interrupted".into(),
            ));
        }
    }
}
#[derive(Clone)]
pub struct JsonRpc {
    state: Rc<State>,
}
/// Dropping the driver rejects all outstanding calls, including unpolled ones.
pub struct Driver {
    inner: LocalBoxFuture<'static, Result<()>>,
    state: Rc<State>,
}
impl std::future::Future for Driver {
    type Output = Result<()>;
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        self.inner.as_mut().poll(cx)
    }
}
impl Drop for Driver {
    fn drop(&mut self) {
        self.state
            .fail(capnp::Error::disconnected("JSON-RPC driver stopped".into()));
    }
}
impl JsonRpc {
    pub fn new(transport: Rc<dyn Transport>, endpoint: Option<Endpoint>) -> Result<(Self, Driver)> {
        if let Some(e) = &endpoint {
            methods(e.schema.get()?)?;
        }
        let (stop, stopped) = oneshot::channel();
        let state = Rc::new(State {
            transport,
            writer: Mutex::new(()),
            next: Cell::new(0),
            pending: RefCell::new(HashMap::new()),
            failure: RefCell::new(None),
            stop: RefCell::new(Some(stop)),
            max_calls: 1024,
        });
        let driver = Driver {
            inner: run(state.clone(), endpoint.map(Rc::new), stopped).boxed_local(),
            state: state.clone(),
        };
        Ok((Self { state }, driver))
    }
    pub fn get_peer(&self, schema: ServiceSchema) -> Result<Client> {
        methods(schema.get()?)?;
        Ok(capnp_rpc::new_loaded_client(
            Peer {
                state: self.state.clone(),
            },
            schema,
        ))
    }
    pub fn error(&self) -> Option<capnp::Error> {
        self.state.failure.borrow().clone()
    }
}
struct PendingGuard {
    state: Rc<State>,
    id: u64,
}
impl Drop for PendingGuard {
    fn drop(&mut self) {
        self.state.pending.borrow_mut().remove(&self.id);
    }
}
struct Peer {
    state: Rc<State>,
}
impl dynamic::Server for Peer {
    fn allow_cancellation(&self) -> bool {
        true
    }
    fn call(self: Rc<Self>, mut context: dynamic::CallContext) -> Promise<(), capnp::Error> {
        Promise::from_future(async move {
            let method = context.method()?;
            let (name, notification) = method_info(&method)?;
            let mut codec = JsonCodec::new();
            codec.handle_by_annotation(method.params()?)?;
            codec.handle_by_annotation(method.results()?)?;
            let params = codec.encode_value(dynamic::Value::Struct(context.get_params()?))?;
            drop(codec);
            let mut message = vec![
                ("jsonrpc".into(), Value::String("2.0".into())),
                ("method".into(), Value::String(name)),
                ("params".into(), params),
            ];
            if notification {
                self.state.send(&Value::Object(message)).await?;
                return Ok(());
            }
            if self.state.pending.borrow().len() >= self.state.max_calls {
                return Err(capnp::Error::overloaded(
                    "too many pending JSON-RPC calls".into(),
                ));
            }
            let id = self.state.next.get();
            if id >= (1u64 << 53) {
                return Err(invalid("JSON-RPC call ID space exhausted"));
            }
            self.state.next.set(id + 1);
            message.insert(1, ("id".into(), Value::Number(id as f64)));
            let (tx, rx) = oneshot::channel();
            self.state.pending.borrow_mut().insert(id, tx);
            let _guard = PendingGuard {
                state: self.state.clone(),
                id,
            };
            self.state.send(&Value::Object(message)).await?;
            let result = rx
                .await
                .map_err(|_| capnp::Error::disconnected("JSON-RPC response canceled".into()))??;
            let output = context.get_results()?;
            let mut codec = JsonCodec::new();
            codec.handle_by_annotation(output.schema())?;
            codec.decode_value(&result, output)?;
            Ok(())
        })
    }
}
fn method_info(method: &Method<'_>) -> Result<(String, bool)> {
    let mut name = method.get_proto().get_name()?.to_str()?.to_owned();
    if let Some(capnp::schema_capnp::value::Text(v)) =
        json::annotation(method.annotations()?, json::NAME)?
    {
        name = v?.to_str()?.into();
    }
    Ok((
        name,
        json::annotation(method.annotations()?, json::NOTIFICATION)?.is_some(),
    ))
}
fn methods(schema: Schema<'_>) -> Result<Vec<(String, Method<'_>)>> {
    fn scan<'s>(
        s: Schema<'s>,
        seen: &mut HashSet<u64>,
        out: &mut Vec<(String, Method<'s>)>,
    ) -> Result<()> {
        if !seen.insert(s.id()) {
            return Ok(());
        }
        for m in s.methods()? {
            let (name, _) = method_info(&m)?;
            if out.iter().any(|(n, _)| n == &name) {
                return Err(invalid("ambiguous JSON-RPC method name"));
            }
            out.push((name, m));
        }
        for parent in s.superclasses()? {
            scan(parent, seen, out)?;
        }
        Ok(())
    }
    let mut out = vec![];
    scan(schema, &mut HashSet::new(), &mut out)?;
    Ok(out)
}
fn error(id: Value, code: i32, message: impl Into<String>) -> Value {
    Value::Object(vec![
        ("jsonrpc".into(), Value::String("2.0".into())),
        ("id".into(), id),
        (
            "error".into(),
            Value::Object(vec![
                ("code".into(), Value::Number(code.into())),
                ("message".into(), Value::String(message.into())),
            ]),
        ),
    ])
}
async fn dispatch(state: Rc<State>, endpoint: Option<Rc<Endpoint>>, message: Value) -> Result<()> {
    if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return state
            .send(&error(Value::Null, -32600, "expected JSON-RPC 2.0 object"))
            .await;
    }
    let id = message.get("id").cloned();
    if let Some(method) = message.get("method") {
        let Some(name) = method.as_str() else {
            return state
                .send(&error(id.unwrap_or(Value::Null), -32600, "invalid method"))
                .await;
        };
        if id
            .as_ref()
            .is_some_and(|v| !matches!(v, Value::Null | Value::Number(_) | Value::String(_)))
        {
            return state
                .send(&error(Value::Null, -32600, "invalid request ID"))
                .await;
        }
        let result: Result<std::result::Result<Value, (i32, String)>> = async {
            let endpoint = endpoint
                .as_ref()
                .ok_or_else(|| capnp::Error::unimplemented("Method not found".into()))?;
            let method = methods(endpoint.schema.get()?)?
                .into_iter()
                .find(|(n, _)| n == name)
                .map(|(_, m)| m)
                .ok_or_else(|| capnp::Error::unimplemented("Method not found".into()))?;
            let mut codec = JsonCodec::new();
            codec.handle_by_annotation(method.params()?)?;
            codec.handle_by_annotation(method.results()?)?;
            let client = endpoint
                .schema
                .reflect(Client::new(endpoint.client.as_client_hook().add_ref()))?;
            let mut request = client.new_request_for(method)?;
            if let Err(e) = codec.decode_value(
                message.get("params").unwrap_or(&Value::Object(vec![])),
                request.get()?,
            ) {
                return Ok(Err((-32602, e.to_string())));
            }
            let response = request.send()?.resolve().await?;
            Ok(Ok(
                codec.encode_value(dynamic::Value::Struct(response.get()?))?
            ))
        }
        .await;
        if let Some(id) = id {
            let response = match result {
                Ok(Ok(value)) => Value::Object(vec![
                    ("jsonrpc".into(), Value::String("2.0".into())),
                    ("id".into(), id),
                    ("result".into(), value),
                ]),
                Ok(Err((code, text))) => error(id, code, text),
                Err(e) => error(
                    id,
                    match e.kind {
                        capnp::ErrorKind::Unimplemented => -32601,
                        capnp::ErrorKind::Disconnected => -32001,
                        capnp::ErrorKind::Overloaded => -32002,
                        _ => -32000,
                    },
                    e.to_string(),
                ),
            };
            state.send(&response).await?;
        }
        Ok(())
    } else {
        let Some(Value::Number(id)) = id else {
            return Ok(());
        };
        if !id.is_finite() || id < 0.0 || id.fract() != 0.0 || id >= (1u64 << 53) as f64 {
            return Ok(());
        }
        let response = match (message.get("result"), message.get("error")) {
            (Some(v), None) => Ok(v.clone()),
            (None, Some(e)) => {
                let text = e
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("JSON-RPC peer error");
                if e.get("code") == Some(&Value::Number(-32601.0)) {
                    Err(capnp::Error::unimplemented(text.into()))
                } else {
                    Err(invalid(text))
                }
            }
            _ => Err(invalid("invalid JSON-RPC response")),
        };
        if let Some(sender) = state.pending.borrow_mut().remove(&(id as u64)) {
            let _ = sender.send(response);
        }
        Ok(())
    }
}
enum Event {
    Read(Result<String>),
    Handled(Result<()>),
}
fn receive(state: Rc<State>) -> LocalBoxFuture<'static, Event> {
    async move { Event::Read(state.transport.receive().await) }.boxed_local()
}
async fn run(
    state: Rc<State>,
    endpoint: Option<Rc<Endpoint>>,
    stopped: oneshot::Receiver<capnp::Error>,
) -> Result<()> {
    let tasks = FuturesUnordered::new();
    tasks.push(receive(state.clone()));
    tasks.push(
        async move {
            Event::Handled(Err(stopped.await.unwrap_or_else(|_| {
                capnp::Error::disconnected("JSON-RPC stopped".into())
            })))
        }
        .boxed_local(),
    );
    let result = async {
        let mut tasks = tasks;
        while let Some(event) = tasks.next().await {
            match event {
                Event::Read(text) => {
                    let text = text?;
                    if tasks.len() >= state.max_calls {
                        return Err(capnp::Error::overloaded(
                            "too many incoming JSON-RPC calls".into(),
                        ));
                    }
                    tasks.push(receive(state.clone()));
                    let state = state.clone();
                    let endpoint = endpoint.clone();
                    tasks.push(
                        async move {
                            Event::Handled(match JsonCodec::new().parse(&text) {
                                Ok(message) => dispatch(state, endpoint, message).await,
                                Err(e) => {
                                    state.send(&error(Value::Null, -32700, e.to_string())).await
                                }
                            })
                        }
                        .boxed_local(),
                    );
                }
                Event::Handled(result) => result?,
            }
        }
        Ok(())
    }
    .await;
    state.fail(
        result
            .clone()
            .err()
            .unwrap_or_else(|| capnp::Error::disconnected("JSON-RPC transport ended".into())),
    );
    result
}
