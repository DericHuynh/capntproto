//! TLS 1.3 configuration for the quiche packet engine.
use super::*;
use boring::{
    pkey::PKey,
    ssl::{
        SslContextBuilder, SslMethod, SslOptions, SslSessionCacheMode, SslVerifyMode, SslVersion,
    },
    x509::X509,
};

pub struct ClientConfig(pub(super) quiche::Config);
pub struct ServerConfig(pub(super) quiche::Config, pub(super) bool);

/// Trust only these DER certificates, optionally presenting a client identity.
pub fn client_config(
    roots: Vec<CertificateDer<'static>>,
    identity: Option<tls::Identity>,
) -> io::Result<ClientConfig> {
    client_config_for_version(roots, identity, Version::V1)
}
/// Select a wire version. Unsupported versions fail; no downgrade or 0-RTT.
pub fn client_config_for_version(
    roots: Vec<CertificateDer<'static>>,
    identity: Option<tls::Identity>,
    version: Version,
) -> io::Result<ClientConfig> {
    if !quiche::version_is_supported(version.wire_id()) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "upstream Quiche does not support this QUIC version",
        ));
    }
    Ok(ClientConfig(config(identity, Some(roots), false, version)?))
}
/// A supplied client trust store makes client authentication mandatory.
pub fn server_config(
    identity: tls::Identity,
    client_roots: Option<Vec<CertificateDer<'static>>>,
) -> io::Result<ServerConfig> {
    Ok(ServerConfig(
        config(Some(identity), client_roots, true, Version::V1)?,
        false,
    ))
}
fn config(
    identity: Option<tls::Identity>,
    roots: Option<Vec<CertificateDer<'static>>>,
    server: bool,
    version: Version,
) -> io::Result<quiche::Config> {
    let mut builder = SslContextBuilder::new(SslMethod::tls()).map_err(tls::invalid_config)?;
    builder
        .set_min_proto_version(Some(SslVersion::TLS1_3))
        .map_err(tls::invalid_config)?;
    builder
        .set_max_proto_version(Some(SslVersion::TLS1_3))
        .map_err(tls::invalid_config)?;
    builder.set_session_cache_mode(SslSessionCacheMode::OFF);
    builder.set_options(SslOptions::NO_TICKET);
    if let Some(identity) = identity {
        let mut chain = identity.certificates.into_iter();
        let leaf = chain
            .next()
            .ok_or_else(|| tls::invalid_config("empty identity chain"))?;
        builder
            .set_certificate(X509::from_der(&leaf).map_err(tls::invalid_config)?.as_ref())
            .map_err(tls::invalid_config)?;
        for certificate in chain {
            builder
                .add_extra_chain_cert(X509::from_der(&certificate).map_err(tls::invalid_config)?)
                .map_err(tls::invalid_config)?;
        }
        builder
            .set_private_key(
                PKey::private_key_from_der(identity.private_key.secret_der())
                    .map_err(tls::invalid_config)?
                    .as_ref(),
            )
            .map_err(tls::invalid_config)?;
        builder.check_private_key().map_err(tls::invalid_config)?;
    }
    if let Some(roots) = roots {
        if roots.is_empty() {
            return Err(tls::invalid_config("empty trust store"));
        }
        for root in roots {
            builder
                .cert_store_mut()
                .add_cert(X509::from_der(&root).map_err(tls::invalid_config)?)
                .map_err(tls::invalid_config)?;
        }
        builder.set_verify(if server {
            SslVerifyMode::PEER | SslVerifyMode::FAIL_IF_NO_PEER_CERT
        } else {
            SslVerifyMode::PEER
        });
    } else {
        debug_assert!(server);
        builder.set_verify(SslVerifyMode::NONE);
    }
    let mut config = quiche::Config::with_boring_ssl_ctx_builder(version.wire_id(), builder)
        .map_err(tls::invalid_config)?;
    config
        .set_application_protos(&[tls::ALPN])
        .map_err(tls::invalid_config)?;
    config.set_max_idle_timeout(10_000);
    config.set_max_recv_udp_payload_size(1350);
    config.set_max_send_udp_payload_size(1350);
    config.set_initial_max_data(2 * 1024 * 1024);
    config.set_initial_max_stream_data_bidi_local(1024 * 1024);
    config.set_initial_max_stream_data_bidi_remote(1024 * 1024);
    config.set_initial_max_streams_bidi(u64::from(server));
    config.set_initial_max_streams_uni(0);
    config.set_disable_active_migration(true);
    Ok(config)
}
impl ClientConfig {
    /// Override offered application protocols. RPC admission still requires the
    /// negotiated protocol to equal this crate's RPC ALPN.
    pub fn application_protocols(&mut self, protocols: &[&[u8]]) -> io::Result<()> {
        self.0
            .set_application_protos(protocols)
            .map_err(tls::invalid_config)
    }
}

impl ServerConfig {
    /// Require stateless address validation before allocating handshake state.
    pub fn require_retry(&mut self, enabled: bool) {
        self.1 = enabled;
    }
}
