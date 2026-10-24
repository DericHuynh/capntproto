//! Quiche owns the stream buffers; the task handles packets, recovery and pacing.
use super::*;
use tokio::{
    sync::{oneshot, Notify},
    time::Instant,
};

/// The physical frame writer closes a batch through Stream::poll_flush. A
/// packet flush can also be requested by input, flow control or recovery: an
/// incomplete application batch must never hold those obligations hostage.
#[derive(Default)]
struct OutputTurn {
    changed: Notify,
    pending: std::cell::Cell<bool>,
    dirty: std::cell::Cell<bool>,
    #[cfg(test)]
    notifications: std::cell::Cell<usize>,
}
impl OutputTurn {
    fn request(&self) {
        if !self.pending.replace(true) {
            #[cfg(test)]
            self.notifications.set(self.notifications.get() + 1);
            self.changed.notify_one();
        }
    }
    fn wrote(&self) {
        self.dirty.set(true);
        // Raw stream users need progress even when they never call flush.
        self.request();
    }
    fn finish(&self) {
        if self.dirty.replace(false) {
            self.request();
        }
    }
    fn begin_flush(&self) {
        // No await separates consuming this permit from clearing pending. All
        // producers are local to this task domain. Requests arriving while the
        // socket or pacing await is pending create the next permit.
        use futures::FutureExt;
        let _ = self.changed.notified().now_or_never();
        self.pending.set(false);
    }
}

pub(super) struct State {
    pub conn: RefCell<Box<quiche::Connection>>,
    pub write_closed: std::cell::Cell<bool>,
    pub read_closed: std::cell::Cell<bool>,
    pub closed: std::cell::Cell<bool>,
    failure: RefCell<Option<(io::ErrorKind, String)>>,
    pub reader: RefCell<Option<std::task::Waker>>,
    pub writer: RefCell<Option<std::task::Waker>>,
    turn: OutputTurn,
    defer_flush: std::cell::Cell<bool>,
}
impl State {
    pub fn request_flush(&self) {
        self.turn.request();
    }
    pub fn wrote(&self) {
        self.turn.wrote();
    }
    pub fn finish_output_turn(&self) {
        self.turn.finish();
    }

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
    pub fn receive(&self, bytes: &mut [u8], from: SocketAddr, local: SocketAddr) {
        if self.closed.get() || self.failure.borrow().is_some() {
            return;
        }
        let result = self
            .conn
            .borrow_mut()
            .recv(bytes, quiche::RecvInfo { from, to: local });
        match result {
            Ok(_) | Err(quiche::Error::Done | quiche::Error::CryptoFail) => (),
            Err(error) => {
                *self.failure.borrow_mut() = Some((io::ErrorKind::Other, error.to_string()));
            }
        }
        // Schedule the RPC reader before its packet driver so a response can
        // share the next flight with ACKs. All connection borrows end first.
        let parked = self.reader.borrow().is_some();
        let read = (parked && self.conn.borrow().stream_readable(0))
            .then(|| self.reader.borrow_mut().take())
            .flatten();
        if let Some(waker) = read {
            self.defer_flush.set(true);
            waker.wake();
        }
        self.turn.request();
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
    mut route: endpoint::Route,
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
        turn: OutputTurn::default(),
        defer_flush: false.into(),
    });
    route.activate(&state, core.local);
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
    _route: endpoint::Route,
    state: Rc<State>,
    ready: oneshot::Sender<io::Result<Ready>>,
    mut canceled: oneshot::Receiver<()>,
) {
    let mut ready = Some(ready);
    let result = pump(&core, &state, &mut ready, &mut canceled).await;
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
async fn pump(
    core: &endpoint::Core,
    state: &State,
    ready: &mut Option<oneshot::Sender<io::Result<Ready>>>,
    canceled: &mut oneshot::Receiver<()>,
) -> io::Result<()> {
    let mut detached = false;
    let mut batch = crate::rpc::packet_batch::Batch::new();
    let sender = crate::rpc::packet_batch::Sender::default();
    let mut mtu_recovery = crate::rpc::packet_mtu::Recovery::default();
    loop {
        if let Some(error) = state.error() {
            return Err(error);
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
        if state.defer_flush.replace(false) {
            // The router already woke the reader. Preserve a deferred executor
            // turn for the RPC work it schedules before flushing ACK/credit.
            tokio::task::yield_now().await;
        }
        // Batch completion does not cancel the bounded coalescing fence above.
        // Adjacent RPC turns and endpoint receive bursts can share this flight.
        state.turn.begin_flush();
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
            _ = state.turn.changed.notified() => (),
            _ = tokio::time::sleep_until(deadline) => mtu_recovery.on_timeout(&mut state.conn.borrow_mut()),
            _ = tokio::task::yield_now(), if exhausted => (),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[test]
    fn output_turn_deduplicates_a_large_batch_and_preserves_independent_progress() {
        let turn = OutputTurn::default();
        for _ in 0..140 {
            turn.wrote();
            turn.request(); // receive/credit changes can share the same flight
        }
        turn.finish();
        assert!(!turn.dirty.get());
        assert_eq!(turn.notifications.get(), 1);
        turn.begin_flush();
        assert!(!turn.pending.get());
        // An unfinished output batch, including a flow-blocked raw stream,
        // never suppresses the next recovery/credit wake.
        turn.wrote();
        turn.begin_flush();
        assert!(turn.dirty.get());
        turn.request();
        assert!(turn.pending.get());
        turn.finish();
        assert_eq!(turn.notifications.get(), 3);
        turn.begin_flush();
        // ACK-only traffic has no application output turn to close.
        turn.request();
        assert_eq!(turn.notifications.get(), 4);
        turn.begin_flush();
        turn.finish();
        assert!(
            !turn.pending.get(),
            "clean completion cannot reschedule packets"
        );
    }

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
                                && conn.borrow_mut().stream_recv_buf(0, &mut start[..]).is_ok()
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
                            if conn.borrow_mut().stream_recv_buf(0, &mut buf[..]).is_ok() {
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
