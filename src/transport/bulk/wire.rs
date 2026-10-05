use super::*;
use ring::hmac;

pub(super) const HEADER: usize = 88;
/// A single-use receive authorization. Its encoding is a bearer secret and must
/// travel over the authorized RPC path. Deliberately neither Clone nor Debug.
#[must_use = "deliver the grant through an authorized RPC"]
pub struct Offer {
    pub(super) binding: [u8; 32],
    pub(super) id: u64,
    pub(super) length: u64,
    key: [u8; 32],
}
impl Offer {
    pub fn length(&self) -> u64 {
        self.length
    }
    pub(super) fn new(binding: [u8; 32], id: u64, length: u64) -> Self {
        let mut key = [0; 32];
        key[..16].copy_from_slice(&crate::transport::cid());
        key[16..].copy_from_slice(&crate::transport::cid());
        Self {
            binding,
            id,
            length,
            key,
        }
    }
    pub fn encode(&self) -> [u8; HEADER] {
        let mut bytes = [0; HEADER];
        bytes[..8].copy_from_slice(b"CTPBULK1");
        bytes[8..40].copy_from_slice(&self.binding);
        bytes[40..48].copy_from_slice(&self.id.to_be_bytes());
        bytes[48..56].copy_from_slice(&self.length.to_be_bytes());
        bytes[56..].copy_from_slice(&self.key);
        bytes
    }
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() != HEADER || &bytes[..8] != b"CTPBULK1" {
            return Err(invalid("unsupported bulk offer"));
        }
        let result = Self {
            binding: bytes[8..40].try_into().unwrap(),
            id: u64::from_be_bytes(bytes[40..48].try_into().unwrap()),
            length: u64::from_be_bytes(bytes[48..56].try_into().unwrap()),
            key: bytes[56..].try_into().unwrap(),
        };
        if result.length > MAX_TRANSFER_BYTES || result.id >= 1 << 62 {
            return Err(invalid("bulk offer exceeds limits"));
        }
        Ok(result)
    }
    pub(super) fn header(&self) -> [u8; HEADER] {
        let mut header = self.encode();
        header[..8].copy_from_slice(b"CTPDATA1");
        let tag = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, &self.key), &header[..56]);
        header[56..].copy_from_slice(tag.as_ref());
        header
    }
    pub(super) fn validate(&self, header: &[u8; HEADER]) -> io::Result<()> {
        let expected = self.header();
        if header[..56] != expected[..56]
            || hmac::verify(
                &hmac::Key::new(hmac::HMAC_SHA256, &self.key),
                &header[..56],
                &header[56..],
            )
            .is_err()
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "invalid bulk stream grant",
            ));
        }
        Ok(())
    }
}
impl Drop for Offer {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.key.zeroize();
    }
}

/// A bounded replay window accepts out-of-order RPC grant replies. Old grants
/// outside the window are rejected, never re-authorized after metadata eviction.
#[derive(Default)]
pub(super) struct IdWindow {
    high: Option<u64>,
    used: u64,
}
impl IdWindow {
    pub fn claim(&mut self, id: u64) -> io::Result<()> {
        match self.high {
            None => {
                self.high = Some(id);
                self.used = 1;
            }
            Some(high) if id > high => {
                self.used = if id - high >= 64 {
                    0
                } else {
                    self.used << (id - high)
                } | 1;
                self.high = Some(id);
            }
            Some(high) if id <= high && high - id < 64 && self.used & (1 << (high - id)) == 0 => {
                self.used |= 1 << (high - id);
            }
            _ => return Err(invalid("replayed or out-of-window bulk grant")),
        }
        Ok(())
    }
}
