//! Independent s2n-quic peer for the production quiche transport.
use anyhow::{ensure, Context, Result};
use s2n_quic::{
    provider::{
        event::{self, events},
        tls,
    },
    Client, Server,
};
use std::{
    cell::Cell,
    path::Path,
    process::Stdio,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
const ALPN: &[u8] = b"capntproto-rpc/1";
const SIZE: usize = 131_072;
struct Progress(Cell<&'static str>);
impl Progress {
    fn set(&self, phase: &'static str) {
        self.0.set(phase);
        eprintln!("QUIC interop: {phase}");
    }
}
#[derive(Default)]
struct Observed {
    version_one: bool,
    unexpected_version: bool,
    zero_rtt: bool,
    retries: usize,
    updates: usize,
}
#[derive(Clone, Default)]
struct Events(Arc<Mutex<Observed>>);
impl Events {
    fn packet(&self, header: &events::PacketHeader) {
        let mut stats = self.0.lock().unwrap();
        match header {
            events::PacketHeader::Initial { version, .. }
            | events::PacketHeader::Handshake { version, .. } => {
                stats.version_one |= *version == 1;
                stats.unexpected_version |= *version != 1;
            }
            events::PacketHeader::Retry { version, .. } => {
                stats.retries += 1;
                stats.unexpected_version |= *version != 1;
            }
            events::PacketHeader::ZeroRtt { .. } => stats.zero_rtt = true,
            _ => {}
        }
    }
}
impl event::Subscriber for Events {
    type ConnectionContext = ();
    fn create_connection_context(&mut self, _: &event::ConnectionMeta, _: &event::ConnectionInfo) {}
    fn on_packet_received(
        &mut self,
        _: &mut (),
        _: &event::ConnectionMeta,
        e: &events::PacketReceived,
    ) {
        self.packet(&e.packet_header);
    }
    fn on_endpoint_packet_received(
        &mut self,
        _: &events::EndpointMeta,
        e: &events::EndpointPacketReceived,
    ) {
        self.packet(&e.packet_header);
    }
    fn on_endpoint_packet_sent(
        &mut self,
        _: &events::EndpointMeta,
        e: &events::EndpointPacketSent,
    ) {
        self.packet(&e.packet_header);
    }
    fn on_key_update(&mut self, _: &mut (), _: &event::ConnectionMeta, e: &events::KeyUpdate) {
        if matches!(e.key_type,events::KeyType::OneRtt{generation,..}if generation>0) {
            self.0.lock().unwrap().updates += 1;
        }
    }
}
// s2n-quic rotates automatically near the cipher's packet limit. This test-only
// policy lowers that limit to exercise real key rotation with a small payload.
// Every encryption/decryption operation delegates unchanged to upstream AWS-LC;
// the production quiche implementation and its limits are untouched.
struct FrequentRotation;
fn algorithm() -> &'static dyn rustls::quic::Algorithm {
    rustls::crypto::aws_lc_rs::cipher_suite::TLS13_AES_128_GCM_SHA256
        .tls13()
        .unwrap()
        .quic
        .unwrap()
}
impl rustls::quic::Algorithm for FrequentRotation {
    fn packet_key(
        &self,
        key: rustls::crypto::cipher::AeadKey,
        iv: rustls::crypto::cipher::Iv,
    ) -> Box<dyn rustls::quic::PacketKey> {
        Box::new(LimitedKey(algorithm().packet_key(key, iv)))
    }
    fn header_protection_key(
        &self,
        key: rustls::crypto::cipher::AeadKey,
    ) -> Box<dyn rustls::quic::HeaderProtectionKey> {
        algorithm().header_protection_key(key)
    }
    fn aead_key_len(&self) -> usize {
        algorithm().aead_key_len()
    }
}
struct LimitedKey(Box<dyn rustls::quic::PacketKey>);
impl rustls::quic::PacketKey for LimitedKey {
    fn encrypt_in_place(
        &self,
        n: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> std::result::Result<rustls::quic::Tag, rustls::Error> {
        self.0.encrypt_in_place(n, header, payload)
    }
    fn decrypt_in_place<'a>(
        &self,
        n: u64,
        header: &[u8],
        payload: &'a mut [u8],
    ) -> std::result::Result<&'a [u8], rustls::Error> {
        self.0.decrypt_in_place(n, header, payload)
    }
    fn tag_len(&self) -> usize {
        self.0.tag_len()
    }
    fn confidentiality_limit(&self) -> u64 {
        10_032.min(self.0.confidentiality_limit())
    }
    fn integrity_limit(&self) -> u64 {
        self.0.integrity_limit()
    }
}
fn crypto() -> Arc<rustls::crypto::CryptoProvider> {
    static SUITE: OnceLock<rustls::Tls13CipherSuite> = OnceLock::new();
    let suite = SUITE.get_or_init(|| {
        let original = rustls::crypto::aws_lc_rs::cipher_suite::TLS13_AES_128_GCM_SHA256
            .tls13()
            .unwrap();
        rustls::Tls13CipherSuite {
            common: rustls::CipherSuiteCommon {
                suite: original.common.suite,
                hash_provider: original.common.hash_provider,
                confidentiality_limit: original.common.confidentiality_limit,
            },
            hkdf_provider: original.hkdf_provider,
            aead_alg: original.aead_alg,
            quic: Some(&FrequentRotation),
        }
    });
    Arc::new(rustls::crypto::CryptoProvider {
        cipher_suites: vec![rustls::SupportedCipherSuite::Tls13(suite)],
        ..rustls::crypto::aws_lc_rs::default_provider()
    })
}
fn spawn(
    binary: &Path,
    mode: &str,
    address: &str,
    directory: &Path,
    retry: bool,
) -> Result<tokio::process::Child> {
    Ok(tokio::process::Command::new(binary)
        .args([mode, address])
        .arg(directory.join("cert.der"))
        .arg(directory.join("key.der"))
        .args(["1", if retry { "retry" } else { "direct" }])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?)
}
async fn exchange(
    stream: &mut s2n_quic::stream::BidirectionalStream,
    client: bool,
    progress: &Progress,
) -> Result<()> {
    for round in 1..=2 {
        let expected = vec![round; SIZE];
        let mut received = vec![0; SIZE];
        if client {
            progress.set("send payload");
            stream.write_all(&expected).await?;
        }
        progress.set("receive payload");
        stream.read_exact(&mut received).await?;
        ensure!(received == expected, "independent peer payload mismatch");
        if !client {
            progress.set("echo payload");
            stream.write_all(&received).await?;
        }
    }
    if client {
        progress.set("send completion marker");
        stream.write_all(b"done").await?;
    } else {
        progress.set("receive completion marker");
        let mut done = [0; 4];
        stream.read_exact(&mut done).await?;
        ensure!(&done == b"done", "completion marker mismatch");
    }
    progress.set("send stream FIN");
    stream.shutdown().await?;
    progress.set("receive stream FIN");
    let mut final_byte = [0; 1];
    ensure!(
        stream.read(&mut final_byte).await? == 0,
        "unexpected trailing stream data"
    );
    if client {
        // Regression probe: the independent peer can retire while this task
        // is delayed after FIN. Shutdown interest must already be registered;
        // closing an already-retired s2n connection does not wake its endpoint.
        progress.set("allow quiche peer retirement after FIN");
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Ok(())
}
async fn case(binary: &Path, quiche_server: bool, retry: bool, progress: &Progress) -> Result<()> {
    let directory = tempfile::tempdir()?;
    let generated = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
    crate::write(directory.path().join("cert.der"), generated.cert.der())?;
    crate::write(
        directory.path().join("key.der"),
        generated.signing_key.serialize_der(),
    )?;
    let events = Events::default();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(generated.cert.der().clone())?;
    if quiche_server {
        let mut child = spawn(binary, "server", "127.0.0.1:0", directory.path(), retry)?;
        let mut reader =
            tokio::io::BufReader::new(child.stdout.take().context("missing peer output")?);
        let mut port = String::new();
        progress.set("read quiche server address");
        reader.read_line(&mut port).await?;
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], port.trim().parse::<u16>()?));
        let mut config = rustls::ClientConfig::builder_with_provider(crypto())
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_root_certificates(roots)
            .with_no_client_auth();
        config.alpn_protocols = vec![ALPN.to_vec()];
        config.enable_early_data = false;
        config.resumption = rustls::client::Resumption::disabled();
        let mut client = Client::builder()
            .with_tls(tls::rustls::Client::from(config))
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .with_io("127.0.0.1:0")?
            .with_event(events.clone())?
            .start()
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        progress.set("connect s2n client");
        let mut connection = client
            .connect(s2n_quic::client::Connect::new(address).with_server_name("localhost"))
            .await?;
        ensure!(
            connection.application_protocol()?.as_ref() == ALPN,
            "ALPN mismatch"
        );
        progress.set("open s2n stream");
        let mut stream = connection.open_bidirectional_stream().await?;
        // Register close interest before the peer can complete the exchange
        // and retire its connection. In pinned s2n-quic 1.88, CloseHandle's
        // try_recv() does not register a waker, and close() need not wake an
        // already-closed connection. Biased polling registers wait_idle()
        // before exchange() starts, even if the peer closes first after FIN.
        tokio::try_join!(
            biased;
            async { client.wait_idle().await.context("s2n client shutdown") },
            async {
                exchange(&mut stream, true, progress).await?;
                connection.close(0u32.into());
                progress.set("wait for quiche server exit");
                ensure!(child.wait().await?.success(), "quiche server failed");
                drop(stream);
                drop(connection);
                progress.set("wait for s2n client idle");
                Ok(())
            }
        )?;
    } else {
        let mut config = rustls::ServerConfig::builder_with_provider(crypto())
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_no_client_auth()
            .with_single_cert(
                vec![generated.cert.der().clone()],
                rustls::pki_types::PrivatePkcs8KeyDer::from(generated.signing_key.serialize_der())
                    .into(),
            )?;
        config.alpn_protocols = vec![ALPN.to_vec()];
        config.max_early_data_size = 0;
        let limits = s2n_quic::provider::endpoint_limits::Default::builder()
            .with_inflight_handshake_limit(if retry { 0 } else { 100 })?
            .build()?;
        let mut server = Server::builder()
            .with_tls(tls::rustls::Server::from(config))
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .with_io("127.0.0.1:0")?
            .with_endpoint_limits(limits)?
            .with_event(events.clone())?
            .start()
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut child = spawn(
            binary,
            "client",
            &server.local_addr()?.to_string(),
            directory.path(),
            retry,
        )?;
        progress.set("accept s2n connection");
        let mut connection = server.accept().await.context("s2n server closed")?;
        ensure!(
            connection.application_protocol()?.as_ref() == ALPN,
            "ALPN mismatch"
        );
        progress.set("accept s2n stream");
        let mut stream = connection
            .accept_bidirectional_stream()
            .await?
            .context("independent server saw no stream")?;
        exchange(&mut stream, false, progress).await?;
        connection.close(0u32.into());
        progress.set("wait for quiche client exit");
        ensure!(child.wait().await?.success(), "quiche client failed");
    }
    let observed = events.0.lock().unwrap();
    ensure!(
        observed.version_one && !observed.unexpected_version && !observed.zero_rtt,
        "invalid QUIC version or early-data negotiation"
    );
    ensure!(
        (observed.retries > 0) == retry,
        "Retry behavior was not observed as requested"
    );
    ensure!(
        observed.updates > 0,
        "no real 1-RTT key update was observed"
    );
    println!("s2n-quic ↔ quiche: v1, quiche {}, retry={retry}, ALPN verified, {} key updates, two {SIZE}-byte round trips and clean FIN",if quiche_server{"server"}else{"client"},observed.updates);
    Ok(())
}
pub fn run(binary: &Path) -> Result<()> {
    let binary = binary
        .canonicalize()
        .context("build the quic-interop example first")?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        for quiche_server in [true, false] {
            for retry in [false, true] {
                let progress = Progress(Cell::new("configure peers"));
                eprintln!("QUIC interop: quiche_server={quiche_server}, retry={retry}");
                tokio::time::timeout(
                    Duration::from_secs(35),
                    case(&binary, quiche_server, retry, &progress),
                )
                .await
                .with_context(|| format!("QUIC interop timed out: quiche_server={quiche_server}, retry={retry}, phase={}", progress.0.get()))?
                .with_context(|| format!("QUIC interop failed: quiche_server={quiche_server}, retry={retry}, phase={}", progress.0.get()))?;
            }
        }
        Ok(())
    })
}
