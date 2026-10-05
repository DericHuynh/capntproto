//! Native multiparty sessions over mutually authenticated TCP/TLS 1.3.
//! TLS pins stable peer keys and the reservation context before publishing IO.
use super::*;
use crate::native_shutdown::{Frame, Protocol, FRAME_BYTES};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};
use tokio::{
    net::{TcpSocket, TcpStream},
    sync::{mpsc, Notify},
};

pub async fn connect(
    address: SocketAddr,
    identity: &Identity,
    peer: [u8; 32],
    secret: Option<[u8; 32]>,
    context: &[u8],
    timeout: Duration,
) -> io::Result<AuthenticatedSession> {
    let bind = SocketAddr::new(
        if address.is_ipv4() {
            std::net::Ipv4Addr::UNSPECIFIED.into()
        } else {
            std::net::Ipv6Addr::UNSPECIFIED.into()
        },
        0,
    );
    connect_bound(bind, address, identity, peer, secret, context, timeout).await
}

/// Connect from a specified local interface and port, completing mutual TLS
/// before exposing a native session. Port zero selects an ephemeral port.
pub async fn connect_bound(
    bind: SocketAddr,
    address: SocketAddr,
    identity: &Identity,
    peer: [u8; 32],
    secret: Option<[u8; 32]>,
    context: &[u8],
    timeout: Duration,
) -> io::Result<AuthenticatedSession> {
    let (client, _) = identity.tls_configs(peer, secret, context)?;
    tokio::time::timeout(timeout, async {
        let socket = if address.is_ipv4() {
            TcpSocket::new_v4()?
        } else {
            TcpSocket::new_v6()?
        };
        socket.bind(bind)?;
        let socket = socket.connect(address).await?;
        socket.set_nodelay(true)?;
        let stream = tokio_rustls::TlsConnector::from(Arc::new(client))
            .connect("reproto.invalid".try_into().unwrap(), socket)
            .await?;
        if stream.get_ref().1.alpn_protocol() != Some(b"reproto/2") {
            return Err(io::Error::other("native ALPN mismatch"));
        }
        Ok(session(stream, identity.public_key(), peer))
    })
    .await?
}
pub async fn accept(
    socket: TcpStream,
    identity: &Identity,
    peer: [u8; 32],
    secret: Option<[u8; 32]>,
    context: &[u8],
    timeout: Duration,
) -> io::Result<AuthenticatedSession> {
    let (_, server) = identity.tls_configs(peer, secret, context)?;
    socket.set_nodelay(true)?;
    tokio::time::timeout(timeout, async {
        let stream = tokio_rustls::TlsAcceptor::from(Arc::new(server))
            .accept(socket)
            .await?;
        if stream.get_ref().1.alpn_protocol() != Some(b"reproto/2") {
            return Err(io::Error::other("native ALPN mismatch"));
        }
        Ok(session(stream, identity.public_key(), peer))
    })
    .await?
}
fn session(
    stream: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + 'static,
    local: [u8; 32],
    peer: [u8; 32],
) -> AuthenticatedSession {
    let (app, io) = crate::rpc::local_io::pair(64 * 1024);
    let shutdown = Control::new();
    let control = shutdown.clone();
    let mobility = Mobility::unavailable();
    let (scheduling, schedule_driver) = scheduling::pair();
    AuthenticatedSession {
        peer,
        local,
        io: Some(app),
        datagrams: None,
        mobility,
        scheduling,
        shutdown,
        driver: tokio::task::spawn_local(async move {
            let _guard = DriverGuard(control.clone());
            let _schedule = schedule_driver;
            let result = tokio::select! {
                r = bridge(stream, io, control.clone()) => r,
                _ = control.expired() => Err(io::Error::new(io::ErrorKind::TimedOut, "TCP receipt deadline elapsed")),
            };
            if let Err(e) = &result {
                control.finish(Err(io::Error::new(e.kind(), e.to_string())));
            }
            result
        }),
    }
}
// Bounded framed multiplexing keeps receipts separate from Cap'n Proto bytes.
// The receive bridge acknowledges only bytes actually delivered to RPC input.
async fn bridge(
    stream: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    app: crate::rpc::local_io::Stream,
    control: Control,
) -> io::Result<()> {
    let (mut input, mut output) = tokio::io::split(stream);
    let (mut app_read, mut app_write) = app.into_split();
    let (queue, mut frames) = mpsc::channel::<(u8, Vec<u8>)>(8);
    let protocol = Rc::new(RefCell::new(Protocol::default()));
    let changed = Rc::new(Notify::new());
    let written = Rc::new(Cell::new(0u64));
    let acknowledged = Rc::new(Cell::new(false));
    let send = async {
        while let Some((kind, bytes)) = frames.recv().await {
            output.write_u8(kind).await?;
            output.write_u32(bytes.len() as u32).await?;
            output.write_all(&bytes).await?;
            output.flush().await?;
            if kind == 1 && bytes[4] != 1 {
                acknowledged.set(true);
                changed.notify_one();
            }
        }
        Ok::<_, io::Error>(())
    };
    let application = async {
        let mut bytes = [0; 16384];
        loop {
            let n = app_read.read(&mut bytes).await?;
            if n == 0 {
                if !control.requested() {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "RPC writer closed without receipt request",
                    ));
                }
                let request = protocol.borrow_mut().request(cid(), written.get())?;
                queue
                    .send((1, request.encode().to_vec()))
                    .await
                    .map_err(io::Error::other)?;
                changed.notify_one();
                return std::future::pending::<io::Result<()>>().await;
            }
            written.set(
                written
                    .get()
                    .checked_add(n as u64)
                    .ok_or_else(|| io::Error::other("RPC byte counter overflow"))?,
            );
            queue
                .send((0, bytes[..n].to_vec()))
                .await
                .map_err(io::Error::other)?;
        }
    };
    let receive = async {
        loop {
            let kind = input.read_u8().await?;
            let n = input.read_u32().await? as usize;
            if !matches!((kind, n), (0, 1..=16384) | (1, FRAME_BYTES)) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid native TCP frame",
                ));
            }
            let mut bytes = vec![0; n];
            input.read_exact(&mut bytes).await?;
            if kind == 0 {
                // Check the fence before copying any bytes beyond it into RPC.
                {
                    let p = protocol.borrow();
                    if p.peer.is_some_and(|f| p.delivered + n as u64 > f.bytes) {
                        return Err(io::Error::other("RPC bytes exceed receipt fence"));
                    }
                }
                app_write.write_all(&bytes).await?;
                protocol.borrow_mut().deliver(n)?;
            } else {
                let frame = Frame::decode(bytes.try_into().unwrap())?;
                protocol.borrow_mut().receive(frame)?;
                if frame.kind == 1 {
                    control.peer_closing();
                }
            }
            changed.notify_one();
        }
    };
    let receipts = async {
        loop {
            let notified = changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !control.requested() || protocol.borrow().sent.is_some() {
                let ack = protocol.borrow_mut().acknowledge();
                if let Some(ack) = ack {
                    queue
                        .send((1, ack.encode().to_vec()))
                        .await
                        .map_err(io::Error::other)?;
                }
            }
            if protocol.borrow().ready(acknowledged.get()) {
                return Ok::<_, io::Error>(());
            }
            notified.await;
        }
    };
    let result = tokio::select! { r=send=>r, r=application=>r, r=receive=>r, r=receipts=>r };
    // A peer close can win the select after its valid receipt was processed.
    // It cannot substitute for any part of the reciprocal fence.
    if protocol.borrow().ready(acknowledged.get()) {
        let _ = output.shutdown().await;
        control.finish(Ok(Receipt {
            bytes: written.get(),
        }));
        return Ok(());
    }
    result
}
