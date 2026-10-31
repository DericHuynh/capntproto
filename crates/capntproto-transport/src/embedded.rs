//! Allocator-free TLS 1.3 client using caller-owned record buffers, entropy and
//! time. Certificate paths, DNS/IP identity, validity and server usage are checked
//! by webpki. The constrained crypto profile is AES-128-GCM / SHA-256 with P-256
//! ECDSA certificates; unsupported algorithms fail verification.
use crate::{TlsClock, ALPN};
pub use embedded_io_async;
pub use embedded_tls::{Aes128GcmSha256, CryptoRngCore, TlsConnection, TlsError};
use embedded_tls::{
    CertificateEntryRef, CertificateRef, CertificateVerifyRef, CryptoProvider, TlsContext,
    TlsVerifier,
};
use pki_types::{
    alg_id, AlgorithmIdentifier, CertificateDer, InvalidSignature, ServerName,
    SignatureVerificationAlgorithm, UnixTime,
};
use sha2::Digest;

/// TLS record and certificate storage supplied by the caller. Certificate
/// storage is needed only during the handshake; record storage stays borrowed
/// by the returned connection. A normal P-256 leaf fits in 2 KiB; larger leaves
/// require a larger certificate slice and fail closed when it is too small.
pub struct Buffers<'records, 'certificate> {
    pub read: &'records mut [u8],
    pub write: &'records mut [u8],
    pub certificate: &'certificate mut [u8],
}

pub struct ClientConfig<'a> {
    server_name: &'a str,
    root: &'a [u8],
    clock: &'a dyn TlsClock,
}
impl<'a> ClientConfig<'a> {
    pub fn new(
        server_name: &'a str,
        root: &'a [u8],
        clock: &'a dyn TlsClock,
    ) -> Result<Self, TlsError> {
        ServerName::try_from(server_name).map_err(|_| TlsError::InvalidCertificate)?;
        webpki::anchor_from_trusted_cert(&CertificateDer::from(root))
            .map_err(|_| TlsError::InvalidCertificate)?;
        Ok(Self {
            server_name,
            root,
            clock,
        })
    }
}

/// Authenticate a server before exposing application IO. The clock must be
/// available; there is no NoClock/NoVerify fallback. The upstream engine is a
/// client implementation. Its streams implement embedded-io-async directly.
pub async fn connect<'buf, S, R>(
    io: S,
    config: &ClientConfig<'_>,
    rng: R,
    buffers: Buffers<'buf, '_>,
) -> Result<TlsConnection<'buf, S, Aes128GcmSha256>, TlsError>
where
    S: embedded_io_async::Read + embedded_io_async::Write + 'buf,
    R: CryptoRngCore,
{
    config.clock.now().ok_or(TlsError::InvalidCertificate)?;
    let protocols = [ALPN];
    let mut tls_config = embedded_tls::TlsConfig::new().with_alpn(&protocols);
    if matches!(
        ServerName::try_from(config.server_name),
        Ok(ServerName::DnsName(_))
    ) {
        tls_config = tls_config.with_server_name(config.server_name);
    }
    let provider = Provider {
        rng,
        verifier: Verifier {
            config,
            leaf: buffers.certificate,
            leaf_len: 0,
            transcript: None,
        },
    };
    let mut connection = TlsConnection::new(io, buffers.read, buffers.write);
    connection
        .open(TlsContext::new(&tls_config, provider))
        .await?;
    Ok(connection)
}

struct Provider<'a, 'cfg, 'cert, R> {
    rng: R,
    verifier: Verifier<'a, 'cfg, 'cert>,
}
impl<R: CryptoRngCore> CryptoProvider for Provider<'_, '_, '_, R> {
    type CipherSuite = Aes128GcmSha256;
    type Signature = &'static [u8];
    fn rng(&mut self) -> impl CryptoRngCore {
        &mut self.rng
    }
    fn verifier(&mut self) -> Result<&mut impl TlsVerifier<Self::CipherSuite>, TlsError> {
        Ok(&mut self.verifier)
    }
}

struct Verifier<'a, 'cfg, 'cert> {
    config: &'a ClientConfig<'cfg>,
    leaf: &'cert mut [u8],
    leaf_len: usize,
    transcript: Option<[u8; 32]>,
}
impl TlsVerifier<Aes128GcmSha256> for Verifier<'_, '_, '_> {
    fn set_hostname_verification(&mut self, hostname: &str) -> Result<(), TlsError> {
        if hostname != self.config.server_name {
            return Err(TlsError::InvalidCertificate);
        }
        Ok(())
    }
    fn verify_certificate(
        &mut self,
        transcript: &sha2::Sha256,
        certificate: CertificateRef<'_>,
    ) -> Result<(), TlsError> {
        let now = self
            .config
            .clock
            .now()
            .ok_or(TlsError::InvalidCertificate)?;
        let Some(CertificateEntryRef::X509(leaf)) = certificate.entries.first() else {
            return Err(TlsError::InvalidCertificate);
        };
        let leaf_der = CertificateDer::from(*leaf);
        let cert =
            webpki::EndEntityCert::try_from(&leaf_der).map_err(|_| TlsError::InvalidCertificate)?;
        let root_der = CertificateDer::from(self.config.root);
        let roots = [webpki::anchor_from_trusted_cert(&root_der)
            .map_err(|_| TlsError::InvalidCertificate)?];
        let mut intermediates: heapless::Vec<CertificateDer<'_>, 15> = heapless::Vec::new();
        for entry in certificate.entries.iter().skip(1) {
            let CertificateEntryRef::X509(der) = entry else {
                return Err(TlsError::InvalidCertificate);
            };
            intermediates
                .push(CertificateDer::from(*der))
                .map_err(|_| TlsError::InsufficientSpace)?;
        }
        cert.verify_for_usage(
            &[&P256_SHA256],
            &roots,
            &intermediates,
            UnixTime::since_unix_epoch(core::time::Duration::from_secs(now)),
            webpki::KeyUsage::server_auth(),
            None,
            None,
        )
        .map_err(|_| TlsError::InvalidCertificate)?;
        let name = ServerName::try_from(self.config.server_name)
            .map_err(|_| TlsError::InvalidCertificate)?;
        cert.verify_is_valid_for_subject_name(&name)
            .map_err(|_| TlsError::InvalidCertificate)?;
        self.leaf
            .get_mut(..leaf.len())
            .ok_or(TlsError::InsufficientSpace)?
            .copy_from_slice(leaf);
        self.leaf_len = leaf.len();
        self.transcript = Some(transcript.clone().finalize().into());
        Ok(())
    }
    fn verify_signature(&mut self, signature: CertificateVerifyRef<'_>) -> Result<(), TlsError> {
        if signature.signature_scheme != embedded_tls::SignatureScheme::EcdsaSecp256r1Sha256 {
            return Err(TlsError::InvalidSignatureScheme);
        }
        let hash = self.transcript.take().ok_or(TlsError::InvalidSignature)?;
        let mut message: heapless::Vec<u8, 146> = heapless::Vec::new();
        for part in [
            &[0x20; 64][..],
            &b"TLS 1.3, server CertificateVerify\0"[..],
            &hash[..],
        ] {
            message
                .extend_from_slice(part)
                .map_err(|_| TlsError::InsufficientSpace)?;
        }
        let leaf = CertificateDer::from(&self.leaf[..self.leaf_len]);
        let cert =
            webpki::EndEntityCert::try_from(&leaf).map_err(|_| TlsError::InvalidCertificate)?;
        cert.verify_signature(&P256_SHA256, &message, signature.signature)
            .map_err(|_| TlsError::InvalidSignature)
    }
}

#[derive(Debug)]
struct P256Sha256;
static P256_SHA256: P256Sha256 = P256Sha256;
impl SignatureVerificationAlgorithm for P256Sha256 {
    fn verify_signature(
        &self,
        key: &[u8],
        message: &[u8],
        signature: &[u8],
    ) -> Result<(), InvalidSignature> {
        use p256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
        let key = VerifyingKey::from_sec1_bytes(key).map_err(|_| InvalidSignature)?;
        let signature = Signature::from_der(signature).map_err(|_| InvalidSignature)?;
        key.verify(message, &signature)
            .map_err(|_| InvalidSignature)
    }
    fn public_key_alg_id(&self) -> AlgorithmIdentifier {
        alg_id::ECDSA_P256
    }
    fn signature_alg_id(&self) -> AlgorithmIdentifier {
        alg_id::ECDSA_SHA256
    }
}

#[cfg(feature = "embedded-rpc")]
mod bridge;
#[cfg(feature = "embedded-rpc")]
pub use bridge::{with_futures_io, FuturesIo};
