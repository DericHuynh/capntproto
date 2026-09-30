//! Immutable, checked X25519 identities for the pinned Noise transport.
#![forbid(unsafe_code)]

use ring::rand::{SecureRandom, SystemRandom};
use snow::resolvers::CryptoResolver;
use zeroize::Zeroizing;

/// Construction failed without publishing an identity or disclosing key material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IdentityError {
    #[error("X25519 key derivation is unavailable")]
    KeyDerivationUnavailable,
    #[error("identity public key does not match its private key")]
    KeyMismatch,
}

/// An owned X25519 private key and its derived public identity.
///
/// Key material cannot be mutated after construction. Sharing an identity via
/// `Rc` or `Arc` does not duplicate its private key. Debug output contains only
/// the public key; the owned secret is zeroized when the final owner is dropped.
/// Callers remain responsible for any copies made before importing a key.
///
/// ```compile_fail,E0451
/// use reproto::transport::Identity;
/// let identity = Identity { private: [7; 32].into(), public: [9; 32] };
/// ```
/// ```compile_fail,E0616
/// let mut identity = reproto::transport::Identity::generate();
/// identity.public = [9; 32];
/// ```
/// ```compile_fail,E0616
/// let identity = reproto::transport::Identity::generate();
/// let secret = identity.private;
/// ```
pub struct Identity {
    private: Zeroizing<[u8; 32]>,
    public: [u8; 32],
}

// Snow's pinned resolver has no secret-erasure API or Drop implementation.
// Overwrite its stored scalar on all exits from derivation, including unwinding.
struct DerivationKey(Box<dyn snow::types::Dh>);
impl Drop for DerivationKey {
    fn drop(&mut self) {
        self.0.set(&[0; 32]);
    }
}

impl Identity {
    #[must_use]
    pub fn generate() -> Self {
        let mut private = Zeroizing::new([0; 32]);
        SystemRandom::new()
            .fill(private.as_mut())
            .expect("OS randomness");
        Self::derive(private).expect("pinned X25519 resolver")
    }

    /// Import a 32-byte X25519 scalar and derive its public key using Snow's
    /// configured Curve25519 provider. This does not assess the scalar's entropy.
    pub fn from_private_key(private: [u8; 32]) -> Result<Self, IdentityError> {
        Self::derive(Zeroizing::new(private))
    }

    /// Import a stored pair, rejecting a public key that does not exactly match
    /// the provider's derived encoding. A rejected owned secret is zeroized too.
    pub fn from_keypair(private: [u8; 32], public: [u8; 32]) -> Result<Self, IdentityError> {
        let identity = Self::from_private_key(private)?;
        if identity.public != public {
            return Err(IdentityError::KeyMismatch);
        }
        Ok(identity)
    }

    fn derive(private: Zeroizing<[u8; 32]>) -> Result<Self, IdentityError> {
        let mut dh = DerivationKey(
            snow::resolvers::DefaultResolver
                .resolve_dh(&snow::params::DHChoice::Curve25519)
                .ok_or(IdentityError::KeyDerivationUnavailable)?,
        );
        dh.0.set(private.as_ref());
        let public =
            dh.0.pubkey()
                .try_into()
                .map_err(|_| IdentityError::KeyDerivationUnavailable)?;
        Ok(Self { private, public })
    }

    #[must_use]
    pub fn public_key(&self) -> [u8; 32] {
        self.public
    }

    pub(super) fn configure(
        &self,
        config: &mut quiche::Config,
        peer: [u8; 32],
        psk: Option<[u8; 32]>,
        context: &[u8],
    ) -> quiche::Result<()> {
        config.set_noise_identity(*self.private, peer, psk, context)
    }
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("public_key", &self.public)
            .finish_non_exhaustive()
    }
}
