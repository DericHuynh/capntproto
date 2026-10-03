//! Short-lived stateless Retry tokens bound to address, version and both CIDs.
use ring::{
    hmac,
    rand::{SecureRandom, SystemRandom},
};
use std::net::SocketAddr;
use tokio::time::Instant;
pub(super) struct Tokens {
    key: hmac::Key,
    origin: Instant,
}
impl Tokens {
    pub fn new() -> Self {
        let mut bytes = zeroize::Zeroizing::new([0; 32]);
        SystemRandom::new()
            .fill(bytes.as_mut())
            .expect("OS randomness");
        Self {
            key: hmac::Key::new(hmac::HMAC_SHA256, bytes.as_ref()),
            origin: Instant::now(),
        }
    }
    fn input(&self, address: SocketAddr, version: u32, payload: &[u8]) -> Vec<u8> {
        let mut bytes = b"capntproto QUIC Retry v1\0".to_vec();
        bytes.extend_from_slice(address.to_string().as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&version.to_be_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }
    pub fn mint(
        &self,
        address: SocketAddr,
        version: u32,
        original: &[u8],
        retry: &[u8; 16],
    ) -> Vec<u8> {
        let mut bytes = (self.origin.elapsed().as_millis() as u64)
            .to_be_bytes()
            .to_vec();
        bytes.push(original.len() as u8);
        bytes.extend_from_slice(original);
        bytes.extend_from_slice(retry);
        let tag = hmac::sign(&self.key, &self.input(address, version, &bytes));
        bytes.extend_from_slice(tag.as_ref());
        bytes
    }
    pub fn validate<'a>(
        &self,
        address: SocketAddr,
        version: u32,
        destination: &[u8],
        token: &'a [u8],
    ) -> Option<&'a [u8]> {
        if token.len() < 8 + 1 + 8 + 16 + 32 {
            return None;
        }
        let n = token[8] as usize;
        if !(8..=20).contains(&n) || token.len() != 9 + n + 16 + 32 {
            return None;
        }
        let (payload, tag) = token.split_at(token.len() - 32);
        hmac::verify(&self.key, &self.input(address, version, payload), tag).ok()?;
        let issued = u64::from_be_bytes(token[..8].try_into().ok()?);
        let age = (self.origin.elapsed().as_millis() as u64).checked_sub(issued)?;
        if age > 10_000 || &token[9 + n..9 + n + 16] != destination {
            return None;
        }
        Some(&token[9..9 + n])
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn tokens_reject_mutation_expiry_cross_version_address_and_destination() {
        let tokens = Tokens::new();
        let address = "127.0.0.1:4000".parse().unwrap();
        let token = tokens.mint(address, 1, &[3; 8], &[4; 16]);
        assert_eq!(
            tokens.validate(address, 1, &[4; 16], &token),
            Some(&[3; 8][..])
        );
        assert!(tokens
            .validate(address, 0x6b3343cf, &[4; 16], &token)
            .is_none());
        assert!(tokens
            .validate("127.0.0.1:4001".parse().unwrap(), 1, &[4; 16], &token)
            .is_none());
        assert!(tokens.validate(address, 1, &[5; 16], &token).is_none());
        for i in 0..token.len() {
            let mut tampered = token.clone();
            tampered[i] ^= 1;
            assert!(tokens.validate(address, 1, &[4; 16], &tampered).is_none());
        }
        tokio::time::advance(std::time::Duration::from_secs(11)).await;
        assert!(tokens.validate(address, 1, &[4; 16], &token).is_none());
    }
}
