//! Stable Ed25519 identities authenticated by TLS 1.3 on every transport.
#![forbid(unsafe_code)]

use ring::{
    rand::{SecureRandom, SystemRandom},
    signature::{Ed25519KeyPair, KeyPair},
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use zeroize::Zeroizing;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IdentityError {
    #[error("Ed25519 key derivation is unavailable")]
    KeyDerivationUnavailable,
    #[error("identity public key does not match its private key")]
    KeyMismatch,
}

/// An immutable Ed25519 seed and its public key. Existing X25519 identities
/// must be replaced; importing their bytes does not preserve their identity.
/// The owned seed is zeroized on drop. Debug output contains only the public key.
///
/// ```compile_fail,E0451
/// use capntproto::transport::Identity;
/// let identity = Identity { private: [7; 32].into(), public: [9; 32] };
/// ```
/// ```compile_fail,E0616
/// let mut identity = capntproto::transport::Identity::generate();
/// identity.public = [9; 32];
/// ```
/// ```compile_fail,E0616
/// let identity = capntproto::transport::Identity::generate();
/// let secret = identity.private;
/// ```
pub struct Identity {
    private: Zeroizing<[u8; 32]>,
    public: [u8; 32],
}
impl Identity {
    #[must_use]
    pub fn generate() -> Self {
        let mut private = Zeroizing::new([0; 32]);
        SystemRandom::new()
            .fill(private.as_mut())
            .expect("OS randomness");
        Self::derive(private).expect("Ed25519 provider")
    }
    pub fn from_private_key(private: [u8; 32]) -> Result<Self, IdentityError> {
        Self::derive(Zeroizing::new(private))
    }
    fn derive(private: Zeroizing<[u8; 32]>) -> Result<Self, IdentityError> {
        let key = Ed25519KeyPair::from_seed_unchecked(private.as_ref())
            .map_err(|_| IdentityError::KeyDerivationUnavailable)?;
        let public = key.public_key().as_ref().try_into().unwrap();
        Ok(Self { private, public })
    }
    pub fn from_keypair(private: [u8; 32], public: [u8; 32]) -> Result<Self, IdentityError> {
        let identity = Self::from_private_key(private)?;
        if identity.public != public {
            return Err(IdentityError::KeyMismatch);
        }
        Ok(identity)
    }
    #[must_use]
    pub fn public_key(&self) -> [u8; 32] {
        self.public
    }

    pub(super) fn credential(&self, binding: &str) -> std::io::Result<crate::rpc::tls::Identity> {
        // RFC 8410 OneAsymmetricKey: Ed25519 seed wrapped in two OCTET STRINGs.
        let mut der = Zeroizing::new(vec![
            0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22,
            0x04, 0x20,
        ]);
        der.extend_from_slice(self.private.as_ref());
        let private = PrivatePkcs8KeyDer::from(der.to_vec());
        let key = rcgen::KeyPair::from_pkcs8_der_and_sign_algo(&private, &rcgen::PKCS_ED25519)
            .map_err(std::io::Error::other)?;
        let mut params = rcgen::CertificateParams::new(vec!["reproto.invalid".into()])
            .map_err(std::io::Error::other)?;
        params.distinguished_name = rcgen::DistinguishedName::new();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, binding);
        params.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![
            rcgen::ExtendedKeyUsagePurpose::ServerAuth,
            rcgen::ExtendedKeyUsagePurpose::ClientAuth,
        ];
        let certificate = params.self_signed(&key).map_err(std::io::Error::other)?;
        Ok(crate::rpc::tls::Identity {
            certificates: vec![certificate.der().clone()],
            private_key: PrivateKeyDer::Pkcs8(private),
        })
    }
    pub(super) fn quiche_config(
        &self,
        peer: [u8; 32],
        secret: Option<[u8; 32]>,
        context: &[u8],
        version: super::QuicVersion,
    ) -> quiche::Result<quiche::Config> {
        use boring::{
            pkey::PKey,
            ssl::{SslContextBuilder, SslMethod, SslVerifyError, SslVerifyMode, SslVersion},
            x509::X509,
        };
        let binding = binding(self.public, peer, secret, context);
        let identity = self
            .credential(&binding)
            .map_err(|_| quiche::Error::TlsFail)?;
        let mut builder =
            SslContextBuilder::new(SslMethod::tls()).map_err(|_| quiche::Error::TlsFail)?;
        builder
            .set_sigalgs_list("ed25519")
            .map_err(|_| quiche::Error::TlsFail)?;
        builder
            .set_min_proto_version(Some(SslVersion::TLS1_3))
            .map_err(|_| quiche::Error::TlsFail)?;
        builder
            .set_certificate(
                X509::from_der(identity.certificates[0].as_ref())
                    .map_err(|_| quiche::Error::TlsFail)?
                    .as_ref(),
            )
            .map_err(|_| quiche::Error::TlsFail)?;
        builder
            .set_private_key(
                PKey::private_key_from_der(identity.private_key.secret_der())
                    .map_err(|_| quiche::Error::TlsFail)?
                    .as_ref(),
            )
            .map_err(|_| quiche::Error::TlsFail)?;
        builder.set_custom_verify_callback(
            SslVerifyMode::PEER | SslVerifyMode::FAIL_IF_NO_PEER_CERT,
            move |ssl| {
                let result = ssl
                    .peer_certificate()
                    .and_then(|c| c.to_der().ok())
                    .is_some_and(|der| {
                        verify_certificate(&der, peer, &binding, rustls::pki_types::UnixTime::now())
                            .is_ok()
                    });
                if result {
                    Ok(())
                } else {
                    Err(SslVerifyError::Invalid(
                        boring::ssl::SslAlert::BAD_CERTIFICATE,
                    ))
                }
            },
        );
        // Every connection proves the current pin and admission context.
        builder.set_session_cache_mode(boring::ssl::SslSessionCacheMode::OFF);
        builder.set_options(boring::ssl::SslOptions::NO_TICKET);
        quiche::Config::with_boring_ssl_ctx_builder(version.wire_id(), builder)
    }
}
impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("public_key", &self.public)
            .finish_non_exhaustive()
    }
}

/// An admission secret binds the authorized reservation to both stable peers
/// and its context. This is an authentication condition, not a TLS traffic PSK.
fn binding(local: [u8; 32], peer: [u8; 32], secret: Option<[u8; 32]>, context: &[u8]) -> String {
    let mut input = b"reproto native TLS admission v2\0".to_vec();
    input.extend_from_slice(&local.min(peer));
    input.extend_from_slice(&local.max(peer));
    input.push(u8::from(secret.is_some()));
    input.extend_from_slice(context);
    let secret = Zeroizing::new(secret.unwrap_or([0; 32]));
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_ref());
    ring::hmac::sign(&key, &input)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn verify_certificate(
    der: &[u8],
    peer: [u8; 32],
    binding: &str,
    now: rustls::pki_types::UnixTime,
) -> Result<(), rustls::Error> {
    use x509_parser::prelude::*;
    let invalid = || {
        rustls::Error::InvalidCertificate(rustls::CertificateError::ApplicationVerificationFailure)
    };
    let (rest, cert) = X509Certificate::from_der(der).map_err(|_| invalid())?;
    let spki = cert.public_key();
    let time = ASN1Time::from_timestamp(now.as_secs().try_into().map_err(|_| invalid())?)
        .map_err(|_| invalid())?;
    if !rest.is_empty()
        || spki.algorithm.algorithm.to_id_string() != "1.3.101.112"
        || spki.algorithm.parameters.is_some()
        || spki.subject_public_key.unused_bits != 0
        || spki.subject_public_key.data.as_ref() != peer
        || !cert.validity().is_valid_at(time)
        || cert.subject().iter_common_name().count() != 1
        || cert
            .subject()
            .iter_common_name()
            .next()
            .and_then(|v| v.as_str().ok())
            != Some(binding)
    {
        return Err(invalid());
    }
    // Trust is the configured Ed25519 key, not a caller-provided CA or name.
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, peer)
        .verify(
            cert.tbs_certificate.as_ref(),
            cert.signature_value.data.as_ref(),
        )
        .map_err(|_| invalid())?;
    Ok(())
}

#[derive(Debug)]
struct Pinned {
    peer: [u8; 32],
    binding: String,
}
impl Pinned {
    fn verify(
        &self,
        cert: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        now: rustls::pki_types::UnixTime,
    ) -> Result<(), rustls::Error> {
        if !intermediates.is_empty() {
            return Err(rustls::Error::General(
                "native identity must be self-signed".into(),
            ));
        }
        verify_certificate(cert.as_ref(), self.peer, &self.binding, now)
    }
    fn signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }
}
impl rustls::client::danger::ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end: &CertificateDer<'_>,
        chain: &[CertificateDer<'_>],
        _: &rustls::pki_types::ServerName<'_>,
        _: &[u8],
        now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        self.verify(end, chain, now)?;
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.3 required".into()))
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.signature(m, c, d)
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![rustls::SignatureScheme::ED25519]
    }
}
impl rustls::server::danger::ClientCertVerifier for Pinned {
    fn root_hint_subjects(&self) -> &[rustls::DistinguishedName] {
        &[]
    }
    fn verify_client_cert(
        &self,
        end: &CertificateDer<'_>,
        chain: &[CertificateDer<'_>],
        now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::server::danger::ClientCertVerified, rustls::Error> {
        self.verify(end, chain, now)?;
        Ok(rustls::server::danger::ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.3 required".into()))
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.signature(m, c, d)
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![rustls::SignatureScheme::ED25519]
    }
}
impl Identity {
    pub(super) fn tls_configs(
        &self,
        peer: [u8; 32],
        secret: Option<[u8; 32]>,
        context: &[u8],
    ) -> std::io::Result<(rustls::ClientConfig, rustls::ServerConfig)> {
        use std::sync::Arc;
        let binding = binding(self.public, peer, secret, context);
        let identity = self.credential(&binding)?;
        let verifier = Arc::new(Pinned { peer, binding });
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut client = rustls::ClientConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(std::io::Error::other)?
            .dangerous()
            .with_custom_certificate_verifier(verifier.clone())
            .with_client_auth_cert(
                identity.certificates.clone(),
                identity.private_key.clone_key(),
            )
            .map_err(std::io::Error::other)?;
        let mut server = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(std::io::Error::other)?
            .with_client_cert_verifier(verifier)
            .with_single_cert(identity.certificates, identity.private_key)
            .map_err(std::io::Error::other)?;
        client.alpn_protocols = vec![b"reproto/2".to_vec()];
        server.alpn_protocols = client.alpn_protocols.clone();
        client.resumption = rustls::client::Resumption::disabled();
        server.send_tls13_tickets = 0;
        Ok((client, server))
    }
}
