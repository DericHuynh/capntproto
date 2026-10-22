//! Quiche owns the stream buffers; the task handles packets, recovery and pacing.
use super::*;
use tokio::{
    sync::{oneshot, Notify},
    time::Instant,
};

pub(super) struct State {
    pub conn: RefCell<Box<quiche::Connection>>,
    pub write_closed: std::cell::Cell<bool>,
    pub read_closed: std::cell::Cell<bool>,
    pub closed: std::cell::Cell<bool>,
    failure: RefCell<Option<(io::ErrorKind, String)>>,
    pub reader: RefCell<Option<std::task::Waker>>,
    pub writer: RefCell<Option<std::task::Waker>>,
    pub changed: Notify,
}
impl State {
    pub fn error(&self) -> Option<io::Error> {
        self.failure
            .borrow()
            .as_ref()
            .map(|(kind, message)| io::Error::new(*kind, message.clone()))
            .or_else(|| {
                self.closed.get().then(|| {
                    io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "QUIC session closed before submitting FIN",
                    )
                })
            })
    }
    fn wake(&self) {
        // Take wakers before invoking them: a wake can schedule application work.
        let read = self.reader.borrow_mut().take();
        let write = self.writer.borrow_mut().take();
        if let Some(wake) = read {
            wake.wake();
        }
        if let Some(wake) = write {
            wake.wake();
        }
    }
}
struct Ready {
    certificates: Vec<CertificateDer<'static>>,
    version: u32,
}
pub(super) async fn spawn(
    core: Rc<endpoint::Core>,
    route: endpoint::Route,
    conn: Box<quiche::Connection>,
    timeout: Duration,
) -> io::Result<Stream> {
    let (ready, wait) = oneshot::channel();
    let (close, canceled) = oneshot::channel();
    let state = Rc::new(State {
        conn: RefCell::new(conn),
        write_closed: false.into(),
        read_closed: false.into(),
        closed: false.into(),
        failure: RefCell::new(None),
        reader: RefCell::new(None),
        writer: RefCell::new(None),
        changed: Notify::new(),
    });
    let mut stream = Stream {
        state: state.clone(),
        close: Some(close),
        certificates: vec![],
        version: 0,
        _endpoint: core.clone(),
    };
    tokio::task::spawn_local(run(core, route, state, ready, canceled));
    let ready = tls::deadline(timeout, async {
        wait.await
            .map_err(|_| io::Error::other("QUIC driver stopped during setup"))?
    })
    .await?;
    stream.certificates = ready.certificates;
    stream.version = ready.version;
    Ok(stream)
}
async fn flush(
    conn: &RefCell<Box<quiche::Connection>>,
    core: &endpoint::Core,
    batch: &mut crate::rpc::packet_batch::Batch,
    sender: &crate::rpc::packet_batch::Sender,
) -> io::Result<bool> {
    for _ in 0..16 {
        let start = batch.len();
        // No connection borrow crosses a socket/pacing await. The application
        // can consume or enqueue stream bytes while a datagram is waiting.
        let result = conn.borrow_mut().send(batch.output_buffer());
        match result {
            Ok((n, info)) => {
                let quantum = conn.borrow().send_quantum();
                if !batch.push_prepared(n, info, quantum) {
                    batch.send(sender, &core.socket).await?;
                    batch.restart(start, n, info, quantum);
                }
            }
            Err(quiche::Error::Done) => {
                batch.send(sender, &core.socket).await?;
                sender.check_path_mtu(&mut conn.borrow_mut());
                return Ok(false);
            }
            Err(e) => return Err(io::Error::other(e)),
        }
    }
    batch.send(sender, &core.socket).await?;
    sender.check_path_mtu(&mut conn.borrow_mut());
    Ok(true)
}
async fn run(
    core: Rc<endpoint::Core>,
    mut route: endpoint::Route,
    state: Rc<State>,
    ready: oneshot::Sender<io::Result<Ready>>,
    mut canceled: oneshot::Receiver<()>,
) {
    let mut ready = Some(ready);
    let result = pump(&core, &mut route, &state, &mut ready, &mut canceled).await;
    {
        let mut conn = state.conn.borrow_mut();
        if !conn.is_closed() && !conn.is_draining() {
            let _ = conn.close(
                true,
                if result.is_ok() { 0 } else { 1 },
                b"RPC session closed",
            );
        }
    }
    let _ = flush(
        &state.conn,
        &core,
        &mut crate::rpc::packet_batch::Batch::new(),
        &crate::rpc::packet_batch::Sender::default(),
    )
    .await;
    *state.failure.borrow_mut() = result.as_ref().err().map(|e| (e.kind(), e.to_string()));
    state.closed.set(true);
    state.wake();
    if let Some(ready) = ready {
        let _ = ready.send(Err(result.err().unwrap_or_else(|| {
            io::Error::new(io::ErrorKind::ConnectionAborted, "QUIC setup canceled")
        })));
    }
}
fn receive(state: &State, mut packet: endpoint::Packet, local: SocketAddr) -> io::Result<()> {
    match state.conn.borrow_mut().recv(
        &mut packet.bytes,
        quiche::RecvInfo {
            from: packet.from,
            to: local,
        },
    ) {
        Ok(_) | Err(quiche::Error::Done | quiche::Error::CryptoFail) => Ok(()),
        Err(e) => Err(io::Error::other(e)),
    }
}
async fn pump(
    core: &endpoint::Core,
    route: &mut endpoint::Route,
    state: &State,
    ready: &mut Option<oneshot::Sender<io::Result<Ready>>>,
    canceled: &mut oneshot::Receiver<()>,
) -> io::Result<()> {
    let local = core.socket.local_addr()?;
    let mut detached = false;
    let mut batch = crate::rpc::packet_batch::Batch::new();
    let sender = crate::rpc::packet_batch::Sender::default();
    let mut mtu_recovery = crate::rpc::packet_mtu::Recovery::default();
    loop {
        for _ in 0..16 {
            let Ok(packet) = route.packets.try_recv() else {
                break;
            };
            receive(state, packet, local)?;
        }
        {
            let mut conn = state.conn.borrow_mut();
            if conn.is_established() {
                if ready.is_some() && conn.application_proto() != tls::ALPN {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "RPC ALPN was not negotiated",
                    ));
                }
                if !conn.is_server() || conn.stream_readable(0) {
                    if let Some(ready) = ready.take() {
                        let certificates = conn
                            .peer_cert_chain()
                            .unwrap_or_default()
                            .into_iter()
                            .map(|cert| CertificateDer::from(cert.to_vec()))
                            .collect();
                        let _ = ready.send(Ok(Ready {
                            certificates,
                            version: quiche::PROTOCOL_VERSION,
                        }));
                    }
                }
            }
            if conn.is_closed() || conn.is_draining() {
                // The Stream keeps the connection alive to drain authenticated
                // bytes even after the packet task and endpoint route finish.
                if conn.is_timed_out() {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "QUIC connection timed out",
                    ));
                }
                if !state.write_closed.get() {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "QUIC connection closed before submitting FIN",
                    ));
                }
                return Ok(());
            }
            if detached
                && matches!(
                    conn.stream_capacity(0),
                    Err(quiche::Error::InvalidStreamState(_))
                )
            {
                return Ok(());
            }
        }
        let readable = state.conn.borrow().stream_readable(0);
        let read = readable.then(|| state.reader.borrow_mut().take()).flatten();
        if let Some(wake) = read {
            wake.wake();
            // Give the newly ready RPC work one turn to enqueue its response.
            // Quiche can then piggyback ACK/credit on application packets.
            tokio::task::yield_now().await;
        }
        let exhausted = flush(&state.conn, core, &mut batch, &sender).await?;
        // A flow-blocked writer must not wake itself on its own notification.
        let writable = !matches!(state.conn.borrow_mut().stream_capacity(0), Ok(0));
        let write = writable.then(|| state.writer.borrow_mut().take()).flatten();
        if let Some(wake) = write {
            wake.wake();
        }
        let deadline = Instant::now()
            + state
                .conn
                .borrow()
                .timeout()
                .unwrap_or(Duration::from_secs(10));
        tokio::select! {
            biased;
            _ = &mut *canceled, if !detached => {
                if !state.write_closed.get() { return Ok(()); }
                detached = true;
                match state.conn.borrow_mut().stream_shutdown(0, quiche::Shutdown::Read, 0) {
                    Ok(()) | Err(quiche::Error::Done | quiche::Error::InvalidStreamState(_)) => (),
                    Err(error) => return Err(io::Error::other(error)),
                }
            },
            packet = route.packets.recv() => {
                receive(state, packet.ok_or_else(|| io::Error::other("QUIC packet router closed"))?, local)?;
            }
            _ = state.changed.notified() => (),
            _ = tokio::time::sleep_until(deadline) => mtu_recovery.on_timeout(&mut state.conn.borrow_mut()),
            _ = tokio::task::yield_now(), if exhausted => (),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[tokio::test(flavor = "current_thread")]
    async fn flush_budget_preserves_queued_packets_and_final_short_datagram() {
        tokio::task::LocalSet::new()
            .run_until(async {
                tokio::time::timeout(Duration::from_secs(5), async {
                    let cert =
                        rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
                    let mut config = server_config(
                        tls::Identity {
                            certificates: vec![cert.cert.der().clone()],
                            private_key: rustls::pki_types::PrivatePkcs8KeyDer::from(
                                cert.signing_key.serialize_der(),
                            )
                            .into(),
                        },
                        None,
                    )
                    .unwrap();
                    // Make the packet budget, rather than congestion or pacing, the
                    // limiting factor in this adapter test. Production keeps both.
                    config.0.set_max_send_udp_payload_size(1350);
                    config.0.set_initial_congestion_window_packets(64);
                    config.0.enable_pacing(false);
                    let server = Endpoint::server(config, "127.0.0.1:0".parse().unwrap()).unwrap();
                    let mut endpoint = Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
                    endpoint.set_default_client_config(
                        client_config(vec![cert.cert.der().clone()], None).unwrap(),
                    );
                    let payload = vec![0xa5; 64 * 1024 + 17];
                    let peer = async {
                        let incoming = server.accept().await.unwrap();
                        let conn = RefCell::new(incoming.connection);
                        let mut route = incoming.route;
                        let mut batch = crate::rpc::packet_batch::Batch::new();
                        let sender = crate::rpc::packet_batch::Sender::default();
                        loop {
                            let mut start = [0; 1];
                            if conn.borrow_mut().is_established()
                                && conn.borrow_mut().stream_recv(0, &mut start).is_ok()
                            {
                                assert_eq!(
                                    conn.borrow_mut().stream_send(0, &payload, true).unwrap(),
                                    payload.len()
                                );
                                assert!(flush(&conn, &server.0, &mut batch, &sender)
                                    .await
                                    .unwrap());
                                while flush(&conn, &server.0, &mut batch, &sender).await.unwrap() {}
                                assert!(batch.bytes().is_empty());
                                break;
                            }
                            flush(&conn, &server.0, &mut batch, &sender).await.unwrap();
                            let mut packet = route.packets.recv().await.unwrap();
                            conn.borrow_mut()
                                .recv(
                                    &mut packet.bytes,
                                    quiche::RecvInfo {
                                        from: packet.from,
                                        to: server.local_addr().unwrap(),
                                    },
                                )
                                .unwrap();
                        }
                        // Keep the route alive until the client has drained every byte.
                        std::future::pending::<()>().await;
                    };
                    let client = async {
                        let mut stream = connect(
                            &endpoint,
                            server.local_addr().unwrap(),
                            "localhost",
                            Duration::from_secs(2),
                        )
                        .await
                        .unwrap();
                        stream.write_all(b"s").await.unwrap();
                        let mut received = Vec::new();
                        stream.read_to_end(&mut received).await.unwrap();
                        assert_eq!(received, payload);
                    };
                    tokio::select! { _ = peer => unreachable!(), _ = client => {} }
                })
                .await
                .unwrap();
            })
            .await;
    }
    #[tokio::test(flavor = "current_thread")]
    async fn peer_stop_sending_rejects_subsequent_shutdown() {
        tokio::task::LocalSet::new().run_until(async {
            tokio::time::timeout(Duration::from_secs(5), async {
                let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
                let server = Endpoint::server(server_config(tls::Identity {
                    certificates: vec![cert.cert.der().clone()],
                    private_key: rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()).into(),
                }, None).unwrap(), "127.0.0.1:0".parse().unwrap()).unwrap();
                let mut endpoint = Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
                endpoint.set_default_client_config(client_config(vec![cert.cert.der().clone()], None).unwrap());
                let peer = async {
                    let incoming = server.accept().await.unwrap();
                    let conn = RefCell::new(incoming.connection);
                    let mut route = incoming.route;
                    let mut stopped = false;
                    loop {
                        if conn.borrow_mut().is_established() && !stopped {
                            let mut buf = [0; 64];
                            if conn.borrow_mut().stream_recv(0, &mut buf).is_ok() {
                                conn.borrow_mut().stream_shutdown(0, quiche::Shutdown::Read, 42).unwrap();
                                conn.borrow_mut().stream_send(0, b"s", false).unwrap(); stopped = true;
                            }
                        }
                        flush(&conn, &server.0, &mut crate::rpc::packet_batch::Batch::new(), &crate::rpc::packet_batch::Sender::default()).await.unwrap();
                        if conn.borrow_mut().is_closed() || conn.borrow_mut().is_draining() { break; }
                        let timeout = conn.borrow_mut().timeout().unwrap_or(Duration::from_secs(1));
                        tokio::select! {
                            packet = route.packets.recv() => {
                                let mut packet = packet.unwrap();
                                match conn.borrow_mut().recv(&mut packet.bytes, quiche::RecvInfo { from: packet.from, to: server.local_addr().unwrap() }) { Ok(_) | Err(quiche::Error::Done) => (), Err(e) => panic!("{e}") }
                            }
                            _ = tokio::time::sleep(timeout) => conn.borrow_mut().on_timeout(),
                        }
                    }
                };
                let client = async {
                    let mut stream = connect(&endpoint, server.local_addr().unwrap(), "localhost", Duration::from_secs(2)).await.unwrap();
                    stream.write_all(b"start").await.unwrap();
                    assert_eq!(stream.read_u8().await.unwrap(), b's');
                    assert!(stream.shutdown().await.is_err());
                    assert!(stream.shutdown().await.is_err());
                };
                tokio::join!(peer, client);
            }).await.unwrap();
        }).await;
    }
}
