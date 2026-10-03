//! Cancellable quiche packet/stream pump with bounded application buffering.
use super::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::oneshot,
    time::Instant,
};

#[derive(Default)]
pub(super) struct State {
    pub acknowledged: bool,
    pub closed: bool,
    failure: Option<(io::ErrorKind, String)>,
    pub waiter: Option<std::task::Waker>,
}
impl State {
    pub fn error(&self) -> Option<io::Error> {
        self.failure
            .as_ref()
            .map(|(kind, message)| io::Error::new(*kind, message.clone()))
            .or_else(|| {
                self.closed.then(|| {
                    io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "QUIC session closed before FIN acknowledgement",
                    )
                })
            })
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
    let (io, network) = tokio::io::duplex(64 * 1024);
    let (ready, wait) = oneshot::channel();
    let (close, canceled) = oneshot::channel();
    let state = Rc::new(RefCell::new(State::default()));
    let mut stream = Stream {
        io,
        state: state.clone(),
        close: Some(close),
        certificates: vec![],
        version: 0,
        _endpoint: core.clone(),
    };
    tokio::task::spawn_local(run(core, route, conn, network, state, ready, canceled));
    let ready = tls::deadline(timeout, async {
        wait.await
            .map_err(|_| io::Error::other("QUIC driver stopped during setup"))?
    })
    .await?;
    stream.certificates = ready.certificates;
    stream.version = ready.version;
    Ok(stream)
}
async fn flush(conn: &mut quiche::Connection, core: &endpoint::Core) -> io::Result<bool> {
    let mut out = [0; 1350];
    for _ in 0..16 {
        match conn.send(&mut out) {
            Ok((n, info)) => {
                tokio::time::sleep_until(info.at.into()).await;
                core.socket.send_to(&out[..n], info.to).await?;
            }
            Err(quiche::Error::Done) => return Ok(false),
            Err(e) => return Err(io::Error::other(e)),
        }
    }
    Ok(true)
}
#[allow(clippy::too_many_arguments)] // A single future owns every resource of this session.
async fn run(
    core: Rc<endpoint::Core>,
    mut route: endpoint::Route,
    mut conn: Box<quiche::Connection>,
    network: DuplexStream,
    state: Rc<RefCell<State>>,
    ready: oneshot::Sender<io::Result<Ready>>,
    mut canceled: oneshot::Receiver<()>,
) {
    let mut ready = Some(ready);
    let result = pump(
        &core,
        &mut route,
        &mut conn,
        network,
        &state,
        &mut ready,
        &mut canceled,
    )
    .await;
    let _ = conn.close(
        true,
        if result.is_ok() { 0 } else { 1 },
        b"RPC session closed",
    );
    let _ = flush(&mut conn, &core).await;
    let failure = result.as_ref().err().map(|e| (e.kind(), e.to_string()));
    let wake = {
        let mut state = state.borrow_mut();
        state.closed = true;
        state.failure = failure;
        state.waiter.take()
    };
    if let Some(wake) = wake {
        wake.wake();
    }
    if let Some(ready) = ready {
        let _ = ready.send(Err(result.err().unwrap_or_else(|| {
            io::Error::new(io::ErrorKind::ConnectionAborted, "QUIC setup canceled")
        })));
    }
}
async fn pump(
    core: &endpoint::Core,
    route: &mut endpoint::Route,
    conn: &mut quiche::Connection,
    network: DuplexStream,
    state: &RefCell<State>,
    ready: &mut Option<oneshot::Sender<io::Result<Ready>>>,
    canceled: &mut oneshot::Receiver<()>,
) -> io::Result<()> {
    let local = core.socket.local_addr()?;
    let (mut reader, mut writer) = tokio::io::split(network);
    let mut tx = [0; 16384];
    let (mut tx_start, mut tx_end) = (0, 0);
    let mut tx_eof = false;
    let mut fin_sent = false;
    let mut rx = [0; 16384];
    let (mut rx_start, mut rx_end) = (0, 0);
    let mut rx_fin = false;
    let mut rx_closed = false;
    let mut stream_seen = !conn.is_server();
    loop {
        if conn.is_established() {
            if conn.application_proto() != tls::ALPN {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "RPC ALPN was not negotiated",
                ));
            }
            if !rx_fin && rx_start == rx_end {
                match conn.stream_recv(0, &mut rx) {
                    Ok((n, fin)) => {
                        rx_start = 0;
                        rx_end = n;
                        rx_fin = fin;
                        stream_seen = true;
                    }
                    Err(quiche::Error::Done | quiche::Error::InvalidStreamState(_)) => (),
                    Err(e) => return Err(io::Error::other(e)),
                }
            }
            if stream_seen {
                if let Some(ready) = ready.take() {
                    let certificates = conn
                        .peer_cert_chain()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|cert| CertificateDer::from(cert.to_vec()))
                        .collect();
                    let _ = ready.send(Ok(Ready {
                        certificates,
                        version: conn
                            .peer_transport_params()
                            .and_then(|p| p.version_information.as_ref())
                            .map_or(1, |v| v[0]),
                    }));
                }
                if tx_start < tx_end || (tx_eof && !fin_sent) {
                    match conn.stream_send(0, &tx[tx_start..tx_end], tx_eof) {
                        Ok(n) => {
                            tx_start += n;
                            if tx_eof && tx_start == tx_end {
                                fin_sent = true;
                            }
                        }
                        Err(quiche::Error::Done) => (),
                        Err(e) => return Err(io::Error::other(e)),
                    }
                }
                if fin_sent && conn.stream_send_acknowledged(0).map_err(io::Error::other)? {
                    let wake = {
                        let mut state = state.borrow_mut();
                        state.acknowledged = true;
                        state.waiter.take()
                    };
                    if let Some(wake) = wake {
                        wake.wake();
                    }
                }
            }
        }
        if rx_fin && rx_start == rx_end && !rx_closed {
            writer.shutdown().await?;
            rx_closed = true;
        }
        let exhausted = flush(conn, core).await?;
        if conn.is_closed() || conn.is_draining() {
            if conn.is_timed_out() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "QUIC connection timed out",
                ));
            }
            if !state.borrow().acknowledged {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "QUIC connection closed before FIN acknowledgement",
                ));
            }
            return Ok(());
        }
        let deadline = Instant::now() + conn.timeout().unwrap_or(Duration::from_secs(10));
        tokio::select! {
            biased;
            _ = &mut *canceled => return Ok(()),
            packet = route.packets.recv() => {
                let mut packet = packet.ok_or_else(|| io::Error::other("QUIC packet router closed"))?;
                match conn.recv(&mut packet.bytes, quiche::RecvInfo { from: packet.from, to: local }) {
                    Ok(_) | Err(quiche::Error::Done | quiche::Error::CryptoFail) => (),
                    Err(e) => return Err(io::Error::other(e)),
                }
            }
            n = writer.write(&rx[rx_start..rx_end]), if rx_start < rx_end => {
                let n = n?; if n == 0 { return Err(io::Error::new(io::ErrorKind::WriteZero, "RPC consumer closed")); } rx_start += n;
            }
            n = reader.read(&mut tx), if ready.is_none() && tx_start == tx_end && !tx_eof => {
                tx_end = n?; tx_start = 0; tx_eof = tx_end == 0;
            }
            _ = tokio::time::sleep_until(deadline) => conn.on_timeout(),
            _ = tokio::task::yield_now(), if exhausted => (),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(flavor = "current_thread")]
    async fn peer_stop_sending_cannot_become_a_successful_fin_receipt() {
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
                    let mut conn = incoming.connection;
                    let mut route = incoming.route;
                    let mut stopped = false;
                    loop {
                        if conn.is_established() && !stopped {
                            let mut buf = [0; 64];
                            if conn.stream_recv(0, &mut buf).is_ok() {
                                conn.stream_shutdown(0, quiche::Shutdown::Read, 42).unwrap();
                                conn.stream_send(0, b"s", false).unwrap(); stopped = true;
                            }
                        }
                        flush(&mut conn, &server.0).await.unwrap();
                        if conn.is_closed() || conn.is_draining() { break; }
                        let timeout = conn.timeout().unwrap_or(Duration::from_secs(1));
                        tokio::select! {
                            packet = route.packets.recv() => {
                                let mut packet = packet.unwrap();
                                match conn.recv(&mut packet.bytes, quiche::RecvInfo { from: packet.from, to: server.local_addr().unwrap() }) { Ok(_) | Err(quiche::Error::Done) => (), Err(e) => panic!("{e}") }
                            }
                            _ = tokio::time::sleep(timeout) => conn.on_timeout(),
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
