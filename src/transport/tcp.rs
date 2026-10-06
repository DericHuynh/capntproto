//! Native multiparty sessions over mutually authenticated TCP/TLS 1.3.
//! TLS pins stable peer keys and the reservation context before publishing IO.
mod framing;
use super::*;
use crate::native_shutdown::{Frame, Protocol, FRAME_BYTES};
use bytes::{Bytes, BytesMut};
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
    // A 64 KiB RPC also has an envelope and segment table. Admit the whole
    // common large message, rather than parking its final bytes behind a read.
    let (app, io) = crate::rpc::local_io::pair(crate::rpc::QUIC_BUFFER_BYTES);
    let shutdown = Control::new();
    let control = shutdown.clone();
    let mobility = Mobility::unavailable();
    let (scheduling, schedule_driver) = scheduling::pair();
    AuthenticatedSession {
        bulk: None,
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
    let (queue, mut frames) = mpsc::channel::<(u8, Bytes)>(framing::BATCH_FRAMES);
    let protocol = Rc::new(RefCell::new(Protocol::default()));
    let changed = Rc::new(Notify::new());
    let written = Rc::new(Cell::new(0u64));
    let acknowledged = Rc::new(Cell::new(false));
    let send = async {
        let mut batch = Vec::with_capacity(framing::BATCH_FRAMES);
        while let Some(frame) = frames.recv().await {
            batch.push(frame);
            while batch.len() < framing::BATCH_FRAMES {
                match frames.try_recv() {
                    Ok(frame) => batch.push(frame),
                    Err(_) => break,
                }
            }
            let mut slices = [(0, &[][..]); framing::BATCH_FRAMES];
            for (slice, (kind, bytes)) in slices.iter_mut().zip(&batch) {
                *slice = (*kind, bytes);
            }
            framing::write_batch(&mut output, &slices[..batch.len()]).await?;
            // Publish acknowledgement only after the entire batch flushes.
            if batch
                .iter()
                .any(|(kind, bytes)| *kind == 1 && bytes[4] != 1)
            {
                acknowledged.set(true);
                changed.notify_one();
            }
            batch.clear();
        }
        Ok::<_, io::Error>(())
    };
    let application = async {
        loop {
            let mut bytes = app_read.read_owned().await?;
            if bytes.is_empty() {
                if !control.requested() {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "RPC writer closed without receipt request",
                    ));
                }
                let request = protocol.borrow_mut().request(cid(), written.get())?;
                queue
                    .send((1, Bytes::copy_from_slice(&request.encode())))
                    .await
                    .map_err(io::Error::other)?;
                changed.notify_one();
                return std::future::pending::<io::Result<()>>().await;
            }
            // Retain the bounded local pipe chunk through the TLS write, as
            // C++ retains queued messages. Frames are views, not staged copies.
            while !bytes.is_empty() {
                let n = bytes.len().min(16384);
                written.set(
                    written
                        .get()
                        .checked_add(n as u64)
                        .ok_or_else(|| io::Error::other("RPC byte counter overflow"))?,
                );
                queue
                    .send((0, bytes.split_to(n)))
                    .await
                    .map_err(io::Error::other)?;
            }
        }
    };
    let receive = async {
        let mut bytes = BytesMut::new();
        loop {
            let kind = input.read_u8().await?;
            let n = input.read_u32().await? as usize;
            if !matches!((kind, n), (0, 1..=16384) | (1, FRAME_BYTES)) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid native TCP frame",
                ));
            }
            framing::read_payload(&mut input, &mut bytes, n).await?;
            if kind == 0 {
                // Check the fence before copying any bytes beyond it into RPC.
                {
                    let p = protocol.borrow();
                    if p.peer.is_some_and(|f| p.delivered + n as u64 > f.bytes) {
                        return Err(io::Error::other("RPC bytes exceed receipt fence"));
                    }
                }
                let mut offset = 0;
                while offset < n {
                    let count = app_write.write_owned(&mut bytes, offset..n).await?;
                    if count == 0 {
                        return Err(io::ErrorKind::WriteZero.into());
                    }
                    offset += count;
                }
                protocol.borrow_mut().deliver(n)?;
            } else {
                let frame = Frame::decode(bytes[..].try_into().unwrap())?;
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
                        .send((1, Bytes::copy_from_slice(&ack.encode())))
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
