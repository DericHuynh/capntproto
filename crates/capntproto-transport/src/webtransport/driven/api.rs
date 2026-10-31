use super::protocol::{self, Endpoint, Key, Limits};
use crate::io::{Datagram, DatagramRead, DatagramWrite, Timer};
use bytes::{Buf, BytesMut};
use futures::{future::poll_fn, AsyncRead, AsyncWrite, FutureExt};
use quinn_proto::{self as q, StreamId, VarInt};
use std::{
    cell::RefCell,
    io,
    net::SocketAddr,
    pin::Pin,
    rc::Rc,
    sync::Arc,
    task::{Context, Poll},
    time::Instant,
};

type Shared = Rc<RefCell<Endpoint>>;
struct Lifetime(Shared);
impl Drop for Lifetime {
    fn drop(&mut self) {
        self.0.borrow_mut().shutdown();
    }
}
#[derive(Clone)]
struct Owner {
    state: Shared,
    _lifetime: Rc<Lifetime>,
}
impl Owner {
    fn new(state: Shared) -> Self {
        Self {
            _lifetime: Rc::new(Lifetime(state.clone())),
            state,
        }
    }
    fn notify(&self) {
        self.state.borrow_mut().notify();
    }
}
struct ConnectionGuard {
    owner: Owner,
    key: Key,
    armed: bool,
}
impl ConnectionGuard {
    fn into_session(mut self) -> Session {
        self.armed = false;
        Session(Rc::new(SessionOwner {
            owner: self.owner.clone(),
            key: self.key,
        }))
    }
}
impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        if self.armed {
            self.owner
                .state
                .borrow_mut()
                .close(self.key, 0, "session setup canceled");
        }
    }
}
struct SessionOwner {
    owner: Owner,
    key: Key,
}
impl Drop for SessionOwner {
    fn drop(&mut self) {
        self.owner
            .state
            .borrow_mut()
            .close(self.key, 0, "RPC session dropped");
    }
}

#[derive(Clone)]
pub struct Client {
    owner: Owner,
    config: q::ClientConfig,
    address: SocketAddr,
}
impl Client {
    /// Address resolution is external. The URL still supplies HTTP authority,
    /// certificate identity, and CONNECT path independently of that address.
    pub async fn connect(&self, peer: SocketAddr, url: &str) -> io::Result<Session> {
        let key = self
            .owner
            .state
            .borrow_mut()
            .connect(self.config.clone(), peer, url)?;
        let guard = ConnectionGuard {
            owner: self.owner.clone(),
            key,
            armed: true,
        };
        poll_fn(|cx| {
            let mut state = self.owner.state.borrow_mut();
            let c = state.get(key)?;
            c.check()?;
            if c.accepted {
                Poll::Ready(Ok::<_, io::Error>(()))
            } else {
                c.ready = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await?;
        Ok(guard.into_session())
    }
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.address)
    }
}
#[derive(Clone)]
pub struct Server {
    owner: Owner,
    address: SocketAddr,
}
impl Server {
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.address)
    }
    pub async fn incoming(&self) -> io::Result<Incoming> {
        let key = poll_fn(|cx| {
            let mut state = self.owner.state.borrow_mut();
            if state.stopped {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "WebTransport driver stopped",
                )));
            }
            if let Some(key) = state.incoming.pop_front() {
                Poll::Ready(Ok(key))
            } else {
                state.accept = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await?;
        Ok(Incoming {
            guard: ConnectionGuard {
                owner: self.owner.clone(),
                key,
                armed: true,
            },
        })
    }
    pub async fn accept(&self) -> io::Result<Request> {
        self.incoming().await?.accept().await
    }
}
pub struct Incoming {
    guard: ConnectionGuard,
}
impl Incoming {
    pub async fn accept(self) -> io::Result<Request> {
        let guard = self.guard;
        let request = poll_fn(|cx| {
            let mut state = guard.owner.state.borrow_mut();
            let c = state.get(guard.key)?;
            c.check()?;
            if let Some(request) = c.request.take() {
                Poll::Ready(Ok::<_, io::Error>(request))
            } else {
                c.ready = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await?;
        Ok(Request { guard, request })
    }
}
pub struct Request {
    guard: ConnectionGuard,
    request: wtransport_proto::session::SessionRequest,
}
impl Request {
    pub fn path(&self) -> &str {
        self.request.path()
    }
    pub fn authority(&self) -> &str {
        self.request.authority()
    }
    pub fn origin(&self) -> Option<&str> {
        self.request.origin()
    }
    pub async fn accept(self) -> io::Result<Session> {
        self.guard
            .owner
            .state
            .borrow_mut()
            .get(self.guard.key)?
            .accept()?;
        self.guard.owner.notify();
        Ok(self.guard.into_session())
    }
    pub async fn reject(mut self) {
        if let Ok(c) = self.guard.owner.state.borrow_mut().get(self.guard.key) {
            c.reject();
        }
        self.guard.armed = false;
        self.guard.owner.notify();
    }
}
#[derive(Clone)]
pub struct Session(Rc<SessionOwner>);
impl Session {
    pub async fn open_bi(&mut self) -> io::Result<Stream> {
        let id = poll_fn(|cx| {
            let mut state = self.0.owner.state.borrow_mut();
            let c = state.get(self.0.key)?;
            match c.open()? {
                Some(id) => Poll::Ready(Ok::<_, io::Error>(id)),
                None => {
                    c.ready = Some(cx.waker().clone());
                    Poll::Pending
                }
            }
        })
        .await?;
        self.0.owner.notify();
        Ok(Stream {
            session: self.0.clone(),
            id,
            closing: false,
            close_done: false,
        })
    }
    pub async fn accept_bi(&mut self) -> io::Result<Stream> {
        let id = poll_fn(|cx| {
            let mut state = self.0.owner.state.borrow_mut();
            let c = state.get(self.0.key)?;
            c.check()?;
            if let Some(id) = c.available.pop_front() {
                Poll::Ready(Ok::<_, io::Error>(id))
            } else {
                c.accept_stream = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await?;
        Ok(Stream {
            session: self.0.clone(),
            id,
            closing: false,
            close_done: false,
        })
    }
    pub fn close(&self, code: u32, reason: &str) -> io::Result<()> {
        self.0
            .owner
            .state
            .borrow_mut()
            .close(self.0.key, code, reason);
        Ok(())
    }
}
pub struct Stream {
    session: Rc<SessionOwner>,
    id: StreamId,
    closing: bool,
    close_done: bool,
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
        let mut state = self.session.owner.state.borrow_mut();
        let c = state.get(self.session.key)?;
        let stream = c
            .streams
            .get_mut(&self.id)
            .ok_or_else(|| protocol::invalid("unknown RPC stream"))?;
        if !stream.prefix.is_empty() {
            let n = bytes.len().min(stream.prefix.len());
            stream.prefix.copy_to_slice(&mut bytes[..n]);
            return Poll::Ready(Ok(n));
        }
        if stream.eof {
            return Poll::Ready(Ok(0));
        }
        if !c.graceful {
            c.check()?;
        }
        let result = match protocol::read_quic(&mut c.quic, self.id, bytes) {
            Ok(None) if c.graceful => Some(0),
            Err(_) if c.graceful => Some(0),
            other => other?,
        };
        match result {
            Some(n) => {
                if n == 0 {
                    c.streams.get_mut(&self.id).unwrap().eof = true;
                }
                state.notify();
                Poll::Ready(Ok(n))
            }
            None => {
                c.streams.get_mut(&self.id).unwrap().reader = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}
impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.closing {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let mut state = self.session.owner.state.borrow_mut();
        let c = state.get(self.session.key)?;
        c.check()?;
        if c.header_pending(self.id) {
            c.streams.get_mut(&self.id).unwrap().writer = Some(cx.waker().clone());
            return Poll::Pending;
        }
        match c.quic.send_stream(self.id).write(bytes) {
            Ok(n) => {
                state.notify();
                Poll::Ready(Ok(n))
            }
            Err(q::WriteError::Blocked) => {
                c.streams.get_mut(&self.id).unwrap().writer = Some(cx.waker().clone());
                Poll::Pending
            }
            Err(error) => Poll::Ready(Err(io::Error::other(error))),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut state = self.session.owner.state.borrow_mut();
        state.get(self.session.key)?.check()?;
        state.notify();
        Poll::Ready(Ok(()))
    }
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.close_done {
            return Poll::Ready(Ok(()));
        }
        let session = self.session.clone();
        let mut state = session.owner.state.borrow_mut();
        let c = state.get(session.key)?;
        if c.graceful {
            self.close_done = true;
            return Poll::Ready(Ok(()));
        }
        c.check()?;
        if c.streams.get(&self.id).is_some_and(|s| s.finished) {
            self.close_done = true;
            return Poll::Ready(Ok(()));
        }
        if !self.closing {
            if c.header_pending(self.id) {
                c.streams.get_mut(&self.id).unwrap().writer = Some(cx.waker().clone());
                return Poll::Pending;
            }
            c.quic
                .send_stream(self.id)
                .finish()
                .map_err(io::Error::other)?;
            self.closing = true;
        }
        c.streams.get_mut(&self.id).unwrap().writer = Some(cx.waker().clone());
        state.notify();
        Poll::Pending
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        let mut state = self.session.owner.state.borrow_mut();
        if let Ok(c) = state.get(self.session.key) {
            if !self.close_done {
                let _ = c.quic.send_stream(self.id).reset(VarInt::from_u32(0));
            }
            let _ = c.quic.recv_stream(self.id).stop(VarInt::from_u32(0));
            c.streams.remove(&self.id);
        }
        state.notify();
    }
}

struct Guard(Shared);
impl Drop for Guard {
    fn drop(&mut self) {
        self.0.borrow_mut().stop();
    }
}
async fn recv<R: DatagramRead<Buffer = Vec<u8>, Error = io::Error>>(
    reader: Option<R>,
    buffer: Option<Vec<u8>>,
) -> (R, Vec<u8>, io::Result<Datagram>) {
    let Some(mut reader) = reader else {
        return core::future::pending().await;
    };
    let (result, buffer) = reader.receive(buffer.unwrap()).await;
    (reader, buffer, result)
}
async fn send<W: DatagramWrite<Buffer = Vec<u8>, Error = io::Error>>(
    writer: Option<W>,
    buffer: Option<Vec<u8>>,
    peer: SocketAddr,
) -> (W, Vec<u8>, io::Result<usize>) {
    let Some(mut writer) = writer else {
        return core::future::pending().await;
    };
    let (result, buffer) = writer.transmit(buffer.unwrap(), peer).await;
    (writer, buffer, result)
}
fn drive<R, W, T>(
    state: Shared,
    reader: R,
    writer: W,
    timer: T,
) -> impl crate::driven::Driver<Error = io::Error>
where
    R: DatagramRead<Buffer = Vec<u8>, Error = io::Error>,
    W: DatagramWrite<Buffer = Vec<u8>, Error = io::Error>,
    T: Timer,
{
    let guard = Guard(state.clone());
    let base = state.borrow().now;
    let epoch = timer.now();
    let shutdown_state = state.clone();
    let future = async move {
        let _guard = guard;
        let read = recv(Some(reader), Some(vec![0; 65536])).fuse();
        let write = send::<W>(None, None, "0.0.0.0:0".parse().unwrap()).fuse();
        futures::pin_mut!(read, write);
        let mut writer = Some(writer);
        let mut output = Some(Vec::with_capacity(1500));
        let mut turns = 0;
        loop {
            turns += 1;
            if turns == 128 {
                turns = 0;
                let mut first = true;
                poll_fn(|cx| {
                    if first {
                        first = false;
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    } else {
                        Poll::Ready(())
                    }
                })
                .await;
            }
            let elapsed = timer.now().saturating_sub(epoch);
            let now = base
                .checked_add(elapsed)
                .ok_or_else(|| protocol::invalid("monotonic timer overflow"))?;
            let deadline = {
                let mut state = state.borrow_mut();
                if state.stopped {
                    return Ok(());
                }
                state.handle_timeout(now)?;
                if let Some(mut buffer) = output.take() {
                    match state.transmit(now, &mut buffer)? {
                        Some(tx) => {
                            write.set(send(writer.take(), Some(buffer), tx.destination).fuse());
                        }
                        None => {
                            if state.closing {
                                return Ok(());
                            }
                            output = Some(buffer);
                        }
                    }
                }
                state
                    .next_timeout()
                    .map(|at| epoch + at.saturating_duration_since(base))
            };
            let sleep = async {
                match deadline {
                    Some(at) => timer.wait_until(at).await,
                    None => core::future::pending().await,
                }
            }
            .fuse();
            let notified = poll_fn(|cx| {
                let mut state = state.borrow_mut();
                state.driver = Some(cx.waker().clone());
                if std::mem::take(&mut state.notified) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .fuse();
            futures::pin_mut!(sleep, notified);
            futures::select_biased! {
                (reader,buffer,result)=read=>{
                    let packet=result?;
                    if packet.length>buffer.len(){return Err(protocol::invalid("datagram receive count exceeds buffer"));}
                    let now=base.checked_add(timer.now().saturating_sub(epoch)).ok_or_else(||protocol::invalid("monotonic timer overflow"))?;
                    state.borrow_mut().receive(now,packet.peer,None,BytesMut::from(&buffer[..packet.length]))?;
                    read.set(recv(Some(reader),Some(buffer)).fuse());
                },
                (returned,buffer,result)=write=>{
                    if result?!=buffer.len(){return Err(io::Error::new(io::ErrorKind::WriteZero,"partial UDP send"));}
                    writer=Some(returned);output=Some(buffer);
                },
                _=sleep=>(),
                _=notified=>(),
            }
        }
    };
    crate::driven::Owned::new(future, move || shutdown_state.borrow_mut().shutdown())
}

/// No socket is created and no task is spawned. IO/timer ownership moves to the
/// returned future; keep polling it alongside the client's application work.
pub fn client<R, W, T>(
    config: q::ClientConfig,
    address: SocketAddr,
    reader: R,
    writer: W,
    timer: T,
    limits: Limits,
) -> (Client, impl crate::driven::Driver<Error = io::Error>)
where
    R: DatagramRead<Buffer = Vec<u8>, Error = io::Error>,
    W: DatagramWrite<Buffer = Vec<u8>, Error = io::Error>,
    T: Timer,
{
    let state = Rc::new(RefCell::new(Endpoint::new(None, limits, Instant::now())));
    let client = Client {
        owner: Owner::new(state.clone()),
        config,
        address,
    };
    (client, drive(state, reader, writer, timer))
}
pub fn server<R, W, T>(
    config: q::ServerConfig,
    address: SocketAddr,
    reader: R,
    writer: W,
    timer: T,
    limits: Limits,
) -> (Server, impl crate::driven::Driver<Error = io::Error>)
where
    R: DatagramRead<Buffer = Vec<u8>, Error = io::Error>,
    W: DatagramWrite<Buffer = Vec<u8>, Error = io::Error>,
    T: Timer,
{
    let state = Rc::new(RefCell::new(Endpoint::new(
        Some(Arc::new(config)),
        limits,
        Instant::now(),
    )));
    let server = Server {
        owner: Owner::new(state.clone()),
        address,
    };
    (server, drive(state, reader, writer, timer))
}

/// Admission and frame limits for the runtime-neutral RPC listener.
pub struct ListenOptions {
    pub reader: capnp::message::ReaderOptions,
    pub timeout: std::time::Duration,
    pub max_pending: std::num::NonZeroUsize,
}

/// Concurrent bounded session setup using a caller-provided monotonic timer.
/// Authorization runs before accepting the CONNECT request or exposing RPC.
pub async fn listen<T: Timer>(
    rpc: &capnp_rpc::twoparty::TwoPartyServer,
    endpoint: &Server,
    options: ListenOptions,
    timer: &T,
    authorize: impl Fn(&Request) -> bool,
    mut on_rejected: impl FnMut(io::Error),
) -> capnp::Result<()> {
    use futures::{stream::FuturesUnordered, StreamExt};
    let ListenOptions {
        reader,
        timeout,
        max_pending,
    } = options;
    enum Event<I, O> {
        Incoming(I),
        Completed(O),
    }
    let mut pending = FuturesUnordered::new();
    loop {
        let can_accept = pending.len() < max_pending.get();
        let has_pending = !pending.is_empty();
        let event = {
            let incoming = async {
                if can_accept {
                    endpoint.incoming().await
                } else {
                    core::future::pending().await
                }
            }
            .fuse();
            let completed = async {
                if has_pending {
                    pending.next().await.unwrap()
                } else {
                    core::future::pending().await
                }
            }
            .fuse();
            futures::pin_mut!(incoming, completed);
            futures::select_biased! {value=completed=>Event::Completed(value),value=incoming=>Event::Incoming(value)}
        };
        match event {
            Event::Incoming(incoming) => {
                let incoming = incoming?;
                let authorize = &authorize;
                let deadline = timer
                    .now()
                    .checked_add(timeout)
                    .ok_or_else(|| capnp::Error::failed("setup deadline overflow".into()))?;
                pending.push(async move {
                    let setup = async move {
                        let request = incoming.accept().await?;
                        if !authorize(&request) {
                            request.reject().await;
                            return Err(io::Error::new(
                                io::ErrorKind::PermissionDenied,
                                "WebTransport request rejected",
                            ));
                        }
                        request.accept().await?.accept_bi().await
                    };
                    let deadline = timer.wait_until(deadline);
                    futures::pin_mut!(setup, deadline);
                    match futures::future::select(setup, deadline).await {
                        futures::future::Either::Left((result, _)) => result,
                        futures::future::Either::Right(_) => Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "WebTransport setup timed out",
                        )),
                    }
                });
            }
            Event::Completed(Ok(stream)) => rpc.accept_network(crate::rpc::network(
                stream,
                capnp_rpc::rpc_twoparty_capnp::Side::Server,
                reader,
            ))?,
            Event::Completed(Err(error)) => on_rejected(error),
        }
    }
}
