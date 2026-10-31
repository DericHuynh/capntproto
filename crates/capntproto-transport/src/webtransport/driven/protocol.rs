use crate::TlsClock;
use bytes::{Bytes, BytesMut};
use quinn_proto::{self as q, ConnectionHandle, Dir, StreamId, VarInt};
use std::{
    collections::{BTreeMap, VecDeque},
    fmt, io,
    net::{IpAddr, SocketAddr},
    num::NonZeroUsize,
    sync::Arc,
    task::Waker,
    time::{Duration, Instant},
};
use wtransport_proto::{
    bytes::{BytesReader, BytesWriter},
    frame::{Frame, FrameKind},
    headers::Headers,
    session::{SessionRequest, SessionResponse},
    settings::{SettingId, Settings},
    varint::VarInt as H3Int,
};

const MAX_HEADER: usize = 64 * 1024;
const H3_ERROR: u32 = 0x102;
fn error(e: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::other(e)
}
pub(crate) fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
pub(crate) fn wake(w: &mut Option<Waker>) {
    if let Some(w) = w.take() {
        w.wake();
    }
}
struct Clock(Arc<dyn TlsClock + Send + Sync>);
impl fmt::Debug for Clock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CallerTlsClock")
    }
}
impl rustls_quic::time_provider::TimeProvider for Clock {
    fn current_time(&self) -> Option<rustls_quic::pki_types::UnixTime> {
        self.0
            .now()
            .map(|n| rustls_quic::pki_types::UnixTime::since_unix_epoch(Duration::from_secs(n)))
    }
}
pub fn client_config(
    roots: rustls_quic::RootCertStore,
    clock: Arc<dyn TlsClock + Send + Sync>,
) -> io::Result<q::ClientConfig> {
    let mut tls = rustls_quic::ClientConfig::builder_with_details(
        Arc::new(rustls_quic::crypto::ring::default_provider()),
        Arc::new(Clock(clock)),
    )
    .with_protocol_versions(&[&rustls_quic::version::TLS13])
    .map_err(error)?
    .with_root_certificates(roots)
    .with_no_client_auth();
    tls.alpn_protocols = vec![b"h3".to_vec()];
    tls.enable_early_data = false;
    Ok(q::ClientConfig::new(Arc::new(
        q::crypto::rustls::QuicClientConfig::try_from(tls).map_err(error)?,
    )))
}
pub fn server_config(
    certificates: Vec<rustls_quic::pki_types::CertificateDer<'static>>,
    key: rustls_quic::pki_types::PrivateKeyDer<'static>,
    clock: Arc<dyn TlsClock + Send + Sync>,
) -> io::Result<q::ServerConfig> {
    let mut tls = rustls_quic::ServerConfig::builder_with_details(
        Arc::new(rustls_quic::crypto::ring::default_provider()),
        Arc::new(Clock(clock)),
    )
    .with_protocol_versions(&[&rustls_quic::version::TLS13])
    .map_err(error)?
    .with_no_client_auth()
    .with_single_cert(certificates, key)
    .map_err(error)?;
    tls.alpn_protocols = vec![b"h3".to_vec()];
    tls.max_early_data_size = 0;
    tls.send_tls13_tickets = 0;
    Ok(q::ServerConfig::with_crypto(Arc::new(
        q::crypto::rustls::QuicServerConfig::try_from(tls).map_err(error)?,
    )))
}

#[derive(Clone, Copy)]
pub struct Limits {
    pub connections: NonZeroUsize,
    pub pending_streams: NonZeroUsize,
    pub header_bytes: NonZeroUsize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            connections: 64.try_into().unwrap(),
            pending_streams: 64.try_into().unwrap(),
            header_bytes: MAX_HEADER.try_into().unwrap(),
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Key {
    pub handle: ConnectionHandle,
    pub generation: u64,
}
#[derive(Default)]
pub(crate) struct StreamState {
    pub reader: Option<Waker>,
    pub writer: Option<Waker>,
    pub finished: bool,
    pub eof: bool,
    pub prefix: Bytes,
}
struct Input {
    uni: bool,
    kind: Option<u64>,
    bytes: Vec<u8>,
    settings: bool,
    readable: bool,
    capsules: Vec<u8>,
}
struct Output {
    bytes: Vec<u8>,
    offset: usize,
    finish: bool,
}
pub(crate) struct Connection {
    pub key: Key,
    pub quic: q::Connection,
    pub server: bool,
    pub connected: bool,
    pub graceful: bool,
    pub accepted: bool,
    pub failed: Option<String>,
    pub request: Option<SessionRequest>,
    pub connect_stream: Option<StreamId>,
    pub ready: Option<Waker>,
    pub accept_stream: Option<Waker>,
    pub streams: BTreeMap<StreamId, StreamState>,
    pub available: VecDeque<StreamId>,
    input: BTreeMap<StreamId, Input>,
    output: BTreeMap<StreamId, Output>,
    settings_received: bool,
    control_received: bool,
    qpack_received: u8,
    request_bytes: Option<Vec<u8>>,
    initialised: bool,
}
impl Connection {
    fn new(key: Key, quic: q::Connection, server: bool, request: Option<SessionRequest>) -> Self {
        let request_bytes = request.map(|r| frame_bytes(r.headers().generate_frame()));
        Self {
            key,
            quic,
            server,
            connected: false,
            graceful: false,
            accepted: false,
            failed: None,
            request: None,
            connect_stream: None,
            ready: None,
            accept_stream: None,
            streams: BTreeMap::new(),
            available: VecDeque::new(),
            input: BTreeMap::new(),
            output: BTreeMap::new(),
            settings_received: false,
            control_received: false,
            qpack_received: 0,
            request_bytes,
            initialised: false,
        }
    }
    pub fn check(&self) -> io::Result<()> {
        match &self.failed {
            Some(e) => Err(io::Error::new(io::ErrorKind::ConnectionAborted, e.clone())),
            None if self.graceful => Err(io::ErrorKind::NotConnected.into()),
            None => Ok(()),
        }
    }
    fn fail(&mut self, now: Instant, reason: impl ToString) {
        if self.failed.is_none() {
            self.failed = Some(reason.to_string());

            self.quic
                .close(now, VarInt::from_u32(H3_ERROR), Bytes::new());
        }
        wake(&mut self.ready);
        wake(&mut self.accept_stream);
        for w in self.streams.values_mut() {
            wake(&mut w.reader);
            wake(&mut w.writer);
        }
    }
    fn queue(&mut self, id: StreamId, bytes: Vec<u8>, finish: bool) {
        self.output.insert(
            id,
            Output {
                bytes,
                offset: 0,
                finish,
            },
        );
    }
    fn initialise(&mut self) -> io::Result<()> {
        if self.initialised {
            return Ok(());
        }
        let settings = Settings::builder()
            .enable_connect_protocol()
            .enable_webtransport()
            .enable_h3_datagrams()
            .webtransport_max_sessions(H3Int::from_u32(1))
            .build();
        for kind in [0, 2, 3] {
            let id = self
                .quic
                .streams()
                .open(Dir::Uni)
                .ok_or_else(|| invalid("QUIC peer did not grant HTTP3 control streams"))?;
            let mut bytes = Vec::new();
            bytes.put_varint(H3Int::from_u32(kind)).unwrap();
            if kind == 0 {
                settings.generate_frame().write(&mut bytes).unwrap();
            }
            self.queue(id, bytes, false);
        }
        if !self.server {
            let id = self
                .quic
                .streams()
                .open(Dir::Bi)
                .ok_or_else(|| invalid("QUIC peer did not grant a CONNECT stream"))?;
            self.connect_stream = Some(id);
            self.input.insert(
                id,
                Input {
                    uni: false,
                    kind: Some(1),
                    bytes: Vec::new(),
                    settings: false,
                    readable: true,
                    capsules: Vec::new(),
                },
            );
            let request = self.request_bytes.take().unwrap();
            self.queue(id, request, false);
        }
        self.initialised = true;
        Ok(())
    }
    fn progress(&mut self, now: Instant, limits: Limits) -> io::Result<()> {
        while let Some(event) = self.quic.poll() {
            match event {
                q::Event::Connected => {
                    let handshake = self
                        .quic
                        .crypto_session()
                        .handshake_data()
                        .ok_or_else(|| invalid("missing TLS handshake data"))?;
                    let handshake = handshake
                        .downcast::<q::crypto::rustls::HandshakeData>()
                        .map_err(|_| invalid("unexpected QUIC crypto provider"))?;
                    if handshake.protocol.as_deref() != Some(b"h3") {
                        return Err(invalid("HTTP3 ALPN was not negotiated"));
                    }
                    self.connected = true;
                    self.initialise()?;
                }
                q::Event::ConnectionLost { reason } => {
                    if matches!(&reason,q::ConnectionError::ApplicationClosed(close) if close.error_code==VarInt::from_u32(0))
                        || matches!(&reason, q::ConnectionError::LocallyClosed)
                    {
                        self.graceful = true;
                        wake(&mut self.ready);
                        wake(&mut self.accept_stream);
                        for w in self.streams.values_mut() {
                            wake(&mut w.reader);
                            wake(&mut w.writer);
                        }
                    } else {
                        self.fail(now, reason);
                    }
                }
                q::Event::Stream(q::StreamEvent::Opened { dir }) => {
                    while let Some(id) = self.quic.streams().accept(dir) {
                        if self.input.len() >= limits.pending_streams.get() {
                            return Err(invalid("pending HTTP3 stream limit exceeded"));
                        }
                        self.input.insert(
                            id,
                            Input {
                                uni: dir == Dir::Uni,
                                kind: None,
                                bytes: Vec::new(),
                                settings: false,
                                readable: true,
                                capsules: Vec::new(),
                            },
                        );
                    }
                }
                q::Event::Stream(q::StreamEvent::Readable { id }) => {
                    if let Some(input) = self.input.get_mut(&id) {
                        input.readable = true;
                    }
                    if let Some(w) = self.streams.get_mut(&id) {
                        wake(&mut w.reader);
                    }
                }
                q::Event::Stream(q::StreamEvent::Writable { id }) => {
                    if let Some(w) = self.streams.get_mut(&id) {
                        wake(&mut w.writer);
                    }
                }
                q::Event::Stream(q::StreamEvent::Finished { id }) => {
                    if let Some(w) = self.streams.get_mut(&id) {
                        w.finished = true;
                        wake(&mut w.writer);
                    }
                }
                q::Event::Stream(q::StreamEvent::Stopped { id, .. }) => {
                    if let Some(w) = self.streams.get_mut(&id) {
                        wake(&mut w.writer);
                    }
                }
                q::Event::Stream(q::StreamEvent::Available { .. }) => {
                    wake(&mut self.ready);
                }
                _ => (),
            }
        }
        if self.failed.is_some() || self.graceful {
            return Ok(());
        }
        for (id, output) in &mut self.output {
            while output.offset < output.bytes.len() {
                match self
                    .quic
                    .send_stream(*id)
                    .write(&output.bytes[output.offset..])
                {
                    Ok(0) | Err(q::WriteError::Blocked) => break,
                    Ok(n) => output.offset += n,
                    Err(e) => return Err(error(e)),
                }
            }
            if output.offset == output.bytes.len() {
                if output.finish {
                    self.quic.send_stream(*id).finish().map_err(error)?;
                }
                if let Some(w) = self.streams.get_mut(id) {
                    wake(&mut w.writer);
                }
            }
        }
        self.output
            .retain(|_, output| output.offset < output.bytes.len());
        // Only protocol streams are visited here. Application streams are driven
        // directly by RPC and their readiness events, without a relay queue.
        let mut completed = Vec::new();
        let mut session_close = None;
        for (id, input) in &mut self.input {
            if !input.readable && input.bytes.is_empty() {
                continue;
            }
            loop {
                let before = input.bytes.len();
                let mut bytes = [0; 4096];
                match if input.readable {
                    read_quic(&mut self.quic, *id, &mut bytes)
                } else {
                    Ok(None)
                } {
                    Ok(Some(0))
                        if self.connect_stream == Some(*id)
                            && input.bytes.is_empty()
                            && input.capsules.is_empty() =>
                    {
                        session_close = Some((0, String::new()));
                        completed.push(*id);
                        break;
                    }
                    Ok(Some(0)) => return Err(invalid("HTTP3 protocol stream closed")),
                    Ok(Some(n)) => {
                        if input.bytes.len() + n > limits.header_bytes.get() {
                            return Err(invalid("HTTP3 header limit exceeded"));
                        }
                        input.bytes.extend_from_slice(&bytes[..n]);
                    }
                    Ok(None) => input.readable = false,
                    Err(e) => return Err(e),
                }
                if input.uni && input.kind.is_none() {
                    if let Some((kind, used)) = varint(&input.bytes) {
                        input.bytes.drain(..used);
                        input.kind = Some(kind);
                        match kind {
                            0 => {
                                if self.control_received {
                                    return Err(invalid("duplicate HTTP3 control stream"));
                                }
                                self.control_received = true;
                            }
                            2 | 3 => {
                                let flag = 1 << (kind - 2);
                                if self.qpack_received & flag != 0 {
                                    return Err(invalid("duplicate QPACK stream"));
                                }
                                self.qpack_received |= flag;
                            }
                            0x54 => {
                                // This RPC profile consumes bidirectional streams only.
                                // Stop an unused valid uni stream without killing its session.
                                let _ = self.quic.recv_stream(*id).stop(VarInt::from_u32(0));
                                completed.push(*id);
                                break;
                            }
                            _ => {
                                let _ = self.quic.recv_stream(*id).stop(VarInt::from_u32(0));
                                completed.push(*id);
                                break;
                            }
                        }
                    }
                }
                if input.uni && matches!(input.kind, Some(2 | 3)) {
                    if !input.bytes.is_empty() {
                        return Err(invalid("dynamic QPACK is disabled"));
                    }
                    break;
                }
                if !input.uni && self.connect_stream != Some(*id) {
                    if let Some((0x41, prefix)) = varint(&input.bytes) {
                        if let Some((session, used)) = varint(&input.bytes[prefix..]) {
                            if self.connect_stream.map(u64::from) != Some(session) {
                                return Err(invalid("unknown WebTransport session"));
                            }
                            if self.available.len() >= limits.pending_streams.get() {
                                return Err(invalid("pending RPC stream limit exceeded"));
                            }
                            let prefix = Bytes::copy_from_slice(&input.bytes[prefix + used..]);
                            self.streams.insert(
                                *id,
                                StreamState {
                                    prefix,
                                    ..Default::default()
                                },
                            );
                            self.available.push_back(*id);
                            wake(&mut self.accept_stream);
                            completed.push(*id);
                            break;
                        }
                    }
                }
                if input.uni && input.kind != Some(0) {
                    if input.bytes.len() == before {
                        break;
                    }
                    continue;
                }
                while let Some((kind, length)) = frame_length(&input.bytes)? {
                    let frame_bytes = &input.bytes[..length];
                    let mut reader = frame_bytes;
                    let frame = Frame::read(&mut reader);

                    if input.uni && !input.settings && kind != 4 {
                        return Err(invalid("HTTP3 SETTINGS must be first"));
                    }
                    if let Ok(Some(frame)) = frame {
                        match (input.uni, frame.kind()) {
                            (true, FrameKind::Settings) => {
                                if input.settings {
                                    return Err(invalid("duplicate HTTP3 SETTINGS"));
                                }
                                let settings = Settings::with_frame(&frame).map_err(error)?;
                                if settings
                                    .get(SettingId::EnableConnectProtocol)
                                    .map(|v| v.into_inner())
                                    != Some(1)
                                    || settings
                                        .get(SettingId::EnableWebTransport)
                                        .map(|v| v.into_inner())
                                        != Some(1)
                                {
                                    return Err(invalid("peer did not enable WebTransport"));
                                }
                                input.settings = true;
                                self.settings_received = true;
                                wake(&mut self.ready);
                            }
                            (false, FrameKind::Headers) => {
                                if !self.settings_received {
                                    break;
                                }
                                let headers = Headers::with_frame(&frame).map_err(error)?;
                                if self.server {
                                    if self.connect_stream.is_some() {
                                        return Err(invalid(
                                            "multiple WebTransport sessions are not supported",
                                        ));
                                    }
                                    let request =
                                        SessionRequest::try_from(headers).map_err(error)?;
                                    self.connect_stream = Some(*id);
                                    self.request = Some(request);
                                    wake(&mut self.ready);
                                } else {
                                    let response =
                                        SessionResponse::try_from(headers).map_err(error)?;
                                    if !response.code().is_successful() {
                                        return Err(io::Error::new(
                                            io::ErrorKind::PermissionDenied,
                                            "WebTransport CONNECT rejected",
                                        ));
                                    }
                                    self.accepted = true;
                                    wake(&mut self.ready);
                                }
                            }
                            (false, FrameKind::Data) => {
                                if self.connect_stream != Some(*id) {
                                    return Err(invalid("DATA outside CONNECT stream"));
                                }
                                if input.capsules.len() + frame.payload().len()
                                    > limits.header_bytes.get()
                                {
                                    return Err(invalid("oversized WebTransport capsule"));
                                }
                                input.capsules.extend_from_slice(frame.payload());
                                if let Some(close) = capsules(&mut input.capsules)? {
                                    session_close = Some(close);
                                }
                            }
                            (true, _) if !input.settings => {
                                return Err(invalid("HTTP3 SETTINGS must be first"))
                            }
                            (_, FrameKind::Exercise(_)) => (),
                            _ => return Err(invalid("unexpected HTTP3 frame")),
                        }
                    } else if matches!(kind, 0 | 1 | 4 | 0x41) {
                        return Err(invalid("malformed HTTP3 frame"));
                    }
                    input.bytes.drain(..length);
                }
                if input.bytes.len() == before || input.bytes.is_empty() {
                    break;
                }
            }
        }
        for id in completed {
            self.input.remove(&id);
        }
        if let Some((code, reason)) = session_close {
            if code != 0 {
                self.fail(
                    now,
                    format!("WebTransport session closed ({code}): {reason}"),
                );
            } else {
                self.graceful = true;
                self.quic.close(now, VarInt::from_u32(0), Bytes::new());
                wake(&mut self.ready);
                wake(&mut self.accept_stream);
                for stream in self.streams.values_mut() {
                    wake(&mut stream.reader);
                    wake(&mut stream.writer);
                }
            }
        }
        Ok(())
    }
    pub(crate) fn accept(&mut self) -> io::Result<()> {
        self.check()?;
        let id = self
            .connect_stream
            .ok_or_else(|| invalid("CONNECT not received"))?;
        self.queue(
            id,
            frame_bytes(SessionResponse::ok().headers().generate_frame()),
            false,
        );
        self.accepted = true;
        Ok(())
    }
    pub(crate) fn reject(&mut self) {
        if let Some(id) = self.connect_stream {
            self.queue(
                id,
                frame_bytes(SessionResponse::forbidden().headers().generate_frame()),
                true,
            );
        }
    }
    pub(crate) fn open(&mut self) -> io::Result<Option<StreamId>> {
        self.check()?;
        let Some(id) = self.quic.streams().open(Dir::Bi) else {
            return Ok(None);
        };
        let mut header = Vec::new();
        header.put_varint(H3Int::from_u32(0x41)).unwrap();
        header
            .put_varint(H3Int::try_from(u64::from(self.connect_stream.unwrap())).unwrap())
            .unwrap();
        self.queue(id, header, false);
        self.streams.insert(id, StreamState::default());
        Ok(Some(id))
    }
    pub(crate) fn header_pending(&self, id: StreamId) -> bool {
        self.output.contains_key(&id)
    }
}

// Capsule framing is a byte stream and may cross HTTP/3 DATA frame boundaries.
fn capsules(bytes: &mut Vec<u8>) -> io::Result<Option<(u32, String)>> {
    while let Some((kind, first)) = varint(bytes) {
        let Some((length, second)) = varint(&bytes[first..]) else {
            break;
        };
        let length = usize::try_from(length).map_err(|_| invalid("capsule too large"))?;
        if length > MAX_HEADER {
            return Err(invalid("capsule too large"));
        }
        let end = first + second + length;
        if bytes.len() < end {
            break;
        }
        if kind == 0x2843 {
            let payload = &bytes[first + second..end];
            let code = u32::from_be_bytes(
                payload
                    .get(..4)
                    .ok_or_else(|| invalid("short close capsule"))?
                    .try_into()
                    .unwrap(),
            );
            let reason = std::str::from_utf8(&payload[4..])
                .map_err(error)?
                .to_owned();
            bytes.drain(..end);
            return Ok(Some((code, reason)));
        }
        bytes.drain(..end);
    }
    Ok(None)
}

fn frame_bytes(frame: Frame<'_>) -> Vec<u8> {
    let mut v = Vec::with_capacity(frame.write_size());
    frame.write(&mut v).unwrap();
    v
}
pub(crate) fn varint(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut reader = bytes;
    let n = reader.get_varint()?;
    Some((n.into_inner(), bytes.len() - reader.len()))
}
fn frame_length(bytes: &[u8]) -> io::Result<Option<(u64, usize)>> {
    let Some((kind, first)) = varint(bytes) else {
        return Ok(None);
    };
    let Some((length, second)) = varint(&bytes[first..]) else {
        return Ok(None);
    };
    if kind == 0x41 {
        return Ok(Some((kind, first + second)));
    }
    let length = usize::try_from(length).map_err(|_| invalid("HTTP3 frame too large"))?;
    if length > MAX_HEADER {
        return Err(invalid("HTTP3 frame too large"));
    }
    let length = first + second + length;
    Ok((bytes.len() >= length).then_some((kind, length)))
}
pub(crate) fn read_quic(
    connection: &mut q::Connection,
    id: StreamId,
    bytes: &mut [u8],
) -> io::Result<Option<usize>> {
    let mut stream = connection.recv_stream(id);
    let mut chunks = stream.read(true).map_err(error)?;
    let mut count = 0;
    let mut eof = false;
    let mut failure = None;
    while count < bytes.len() {
        match chunks.next(bytes.len() - count) {
            Ok(Some(chunk)) => {
                bytes[count..count + chunk.bytes.len()].copy_from_slice(&chunk.bytes);
                count += chunk.bytes.len();
            }
            Ok(None) => {
                eof = true;
                break;
            }
            Err(q::ReadError::Blocked) => break,
            Err(e) => {
                failure = Some(error(e));
                break;
            }
        }
    }
    let _ = chunks.finalize();
    if count > 0 {
        Ok(Some(count))
    } else if let Some(error) = failure {
        Err(error)
    } else if eof {
        Ok(Some(0))
    } else {
        Ok(None)
    }
}

/// Native sans-IO endpoint. All timestamps and UDP packets come from its caller.
/// The protocol backend requires std/alloc; Embassy uses the separate TCP/TLS
/// profile. Constructors do not bind sockets or start runtime work.
pub(crate) struct Endpoint {
    protocol: q::Endpoint,
    pub(crate) connections: BTreeMap<ConnectionHandle, Connection>,
    pub(crate) incoming: VecDeque<Key>,
    responses: VecDeque<(q::Transmit, Vec<u8>)>,
    pub(crate) now: Instant,
    pub(crate) limits: Limits,
    generation: u64,
    pub(crate) driver: Option<Waker>,
    pub(crate) accept: Option<Waker>,
    pub(crate) stopped: bool,
    pub(crate) closing: bool,
    pub(crate) notified: bool,
}
impl Endpoint {
    pub fn new(server: Option<Arc<q::ServerConfig>>, limits: Limits, now: Instant) -> Self {
        Self {
            protocol: q::Endpoint::new(Arc::new(q::EndpointConfig::default()), server, false, None),
            connections: BTreeMap::new(),
            incoming: VecDeque::new(),
            responses: VecDeque::new(),
            now,
            limits,
            generation: 0,
            driver: None,
            accept: None,
            stopped: false,
            closing: false,
            notified: false,
        }
    }
    fn time(&mut self, now: Instant) -> io::Result<()> {
        if now < self.now {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "monotonic time moved backwards",
            ));
        }
        self.now = now;
        Ok(())
    }
    pub(crate) fn get(&mut self, key: Key) -> io::Result<&mut Connection> {
        self.connections
            .get_mut(&key.handle)
            .filter(|c| c.key == key)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotConnected,
                    "WebTransport connection released",
                )
            })
    }
    pub(crate) fn connect(
        &mut self,
        config: q::ClientConfig,
        remote: SocketAddr,
        url: &str,
    ) -> io::Result<Key> {
        if self.stopped || self.closing {
            return Err(io::ErrorKind::NotConnected.into());
        }
        if self.generation == u64::MAX {
            return Err(io::Error::other("connection generation exhausted"));
        }
        if self.connections.len() >= self.limits.connections.get() {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let url = super::super::checked_url(url)?;
        let request = SessionRequest::new(url.as_str()).map_err(error)?;
        let host = match url.host().unwrap() {
            url::Host::Domain(name) => name.to_owned(),
            url::Host::Ipv4(ip) => ip.to_string(),
            url::Host::Ipv6(ip) => ip.to_string(),
        };
        let (handle, connection) = self
            .protocol
            .connect(self.now, config, remote, &host)
            .map_err(error)?;
        let key = self.key(handle)?;
        self.connections.insert(
            handle,
            Connection::new(key, connection, false, Some(request)),
        );
        self.notify();
        Ok(key)
    }
    fn key(&mut self, handle: ConnectionHandle) -> io::Result<Key> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| io::Error::other("connection generation exhausted"))?;
        Ok(Key {
            handle,
            generation: self.generation,
        })
    }
    pub fn receive(
        &mut self,
        now: Instant,
        remote: SocketAddr,
        local: Option<IpAddr>,
        packet: BytesMut,
    ) -> io::Result<()> {
        self.time(now)?;

        let mut response = Vec::new();
        match self
            .protocol
            .handle(now, remote, local, None, packet, &mut response)
        {
            Some(q::DatagramEvent::ConnectionEvent(handle, event)) => {
                if let Some(c) = self.connections.get_mut(&handle) {
                    c.quic.handle_event(event);
                }
            }
            Some(q::DatagramEvent::NewConnection(incoming)) => {
                if self.closing
                    || self.generation == u64::MAX
                    || self.connections.len() >= self.limits.connections.get()
                {
                    if self.responses.len() < self.limits.connections.get() {
                        let tx = self.protocol.refuse(incoming, &mut response);
                        self.responses.push_back((tx, response));
                    } else {
                        self.protocol.ignore(incoming);
                    }
                } else {
                    match self.protocol.accept(incoming, now, &mut response, None) {
                        Ok((handle, connection)) => {
                            let key = self.key(handle)?;
                            self.connections
                                .insert(handle, Connection::new(key, connection, true, None));
                            self.incoming.push_back(key);
                            wake(&mut self.accept);
                        }
                        Err(error) => {
                            if let Some(tx) = error.response {
                                self.responses.push_back((tx, response));
                            }
                        }
                    }
                }
            }
            Some(q::DatagramEvent::Response(tx))
                if self.responses.len() < self.limits.connections.get() =>
            {
                self.responses.push_back((tx, response));
            }
            Some(q::DatagramEvent::Response(_)) | None => (),
        }
        self.progress();
        Ok(())
    }
    pub(crate) fn progress(&mut self) {
        for (key, connection) in &mut self.connections {
            if let Err(error) = connection.progress(self.now, self.limits) {
                connection.fail(self.now, error);
            }
            while let Some(event) = connection.quic.poll_endpoint_events() {
                if let Some(event) = self.protocol.handle_event(*key, event) {
                    connection.quic.handle_event(event);
                }
            }
        }
        let before = self.connections.len();
        self.connections.retain(|_, c| !c.quic.is_drained());
        if before != self.connections.len() {
            self.incoming.retain(|key| {
                self.connections
                    .get(&key.handle)
                    .is_some_and(|c| c.key == *key)
            });
        }
    }
    /// Writes one complete UDP payload into output. Retain it across WouldBlock
    /// instead of polling another transmit and losing this packet's ownership.
    pub fn transmit(
        &mut self,
        now: Instant,
        output: &mut Vec<u8>,
    ) -> io::Result<Option<q::Transmit>> {
        self.time(now)?;
        self.progress();
        output.clear();
        if let Some((tx, buffer)) = self.responses.pop_front() {
            *output = buffer;
            return Ok(Some(tx));
        }
        for c in self.connections.values_mut() {
            if let Some(tx) = c.quic.poll_transmit(now, 1, output) {
                return Ok(Some(tx));
            }
        }
        Ok(None)
    }
    pub fn next_timeout(&mut self) -> Option<Instant> {
        self.connections
            .values_mut()
            .filter_map(|c| c.quic.poll_timeout())
            .min()
    }
    pub fn handle_timeout(&mut self, now: Instant) -> io::Result<()> {
        self.time(now)?;
        let mut progressed = false;
        for c in self.connections.values_mut() {
            if c.quic.poll_timeout().is_some_and(|at| at <= now) {
                c.quic.handle_timeout(now);
                progressed = true;
            }
        }
        if progressed {
            self.progress();
        }
        Ok(())
    }
    pub(crate) fn close(&mut self, key: Key, code: u32, reason: &str) {
        let now = self.now;
        if let Ok(c) = self.get(key) {
            c.quic.close(
                now,
                VarInt::from_u32(code),
                Bytes::copy_from_slice(reason.as_bytes()),
            );
        }
        self.notify();
    }
    pub(crate) fn notify(&mut self) {
        self.notified = true;
        wake(&mut self.driver);
    }
    pub(crate) fn shutdown(&mut self) {
        if self.stopped || self.closing {
            return;
        }
        self.closing = true;
        for c in self.connections.values_mut() {
            c.quic.close(self.now, VarInt::from_u32(0), Bytes::new());
        }
        self.notify();
    }
    pub(crate) fn stop(&mut self) {
        self.stopped = true;
        self.notify();
        for c in self.connections.values_mut() {
            c.fail(self.now, "WebTransport driver stopped");
        }
        wake(&mut self.accept);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capsules_accept_every_fragment_boundary_and_ignore_unknown_types() {
        let close = [0x68, 0x43, 7, 0, 0, 0, 0, b'b', b'y', b'e'];
        for cut in 0..close.len() {
            let mut bytes = vec![0x01, 0x01, 42]; // Unknown capsule before close.
            bytes.extend_from_slice(&close[..cut]);
            assert_eq!(capsules(&mut bytes).unwrap(), None);
            bytes.extend_from_slice(&close[cut..]);
            assert_eq!(capsules(&mut bytes).unwrap(), Some((0, "bye".into())));
            assert!(bytes.is_empty());
        }
        assert!(capsules(&mut vec![0x68, 0x43, 3, 0, 0, 0]).is_err());
        assert!(capsules(&mut vec![0x68, 0x43, 5, 0, 0, 0, 0, 255]).is_err());
    }
    #[test]
    fn frame_prefixes_are_bounded_and_monotonic_time_cannot_rewind() {
        let now = Instant::now();
        let mut endpoint = Endpoint::new(None, Limits::default(), now);
        assert!(endpoint
            .handle_timeout(now - Duration::from_millis(1))
            .is_err());
        assert!(endpoint.handle_timeout(now).is_ok());
        for bytes in [&[][..], &[1], &[1, 4], &[1, 4, 0, 0, 0]] {
            assert!(frame_length(bytes).unwrap().is_none());
        }
        assert_eq!(frame_length(&[1, 4, 0, 0, 0, 0]).unwrap(), Some((1, 6)));
        let mut oversized = vec![1];
        oversized
            .put_varint(H3Int::from_u32(MAX_HEADER as u32 + 1))
            .unwrap();
        assert!(frame_length(&oversized).is_err());
    }
}
