// Copyright (C) 2018-2019, Cloudflare, Inc.
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are
// met:
//
//     * Redistributions of source code must retain the above copyright notice,
//       this list of conditions and the following disclaimer.
//
//     * Redistributions in binary form must reproduce the above copyright
//       notice, this list of conditions and the following disclaimer in the
//       documentation and/or other materials provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS
// IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO,
// THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
// PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR
// CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
// EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
// PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
// PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
// LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
// NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
// SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

// Native packet protection for the experimental Noise profile; no TLS FFI.
use super::*;
use ring::aead;
use std::sync::Arc;
use subtle::ConstantTimeEq;

pub(crate) struct PacketKey {
    key: aead::LessSafeKey,
    iv: Vec<u8>,
}
impl PacketKey {
    pub fn new(
        alg: Algorithm, key: Vec<u8>, iv: Vec<u8>, _enc: u32,
    ) -> Result<Self> {
        if iv.len() != 12 {
            return Err(Error::CryptoFail);
        }
        let alg = match alg {
            Algorithm::AES128_GCM => &aead::AES_128_GCM,
            Algorithm::AES256_GCM => &aead::AES_256_GCM,
            Algorithm::ChaCha20_Poly1305 => &aead::CHACHA20_POLY1305,
        };
        Ok(Self {
            key: aead::LessSafeKey::new(
                aead::UnboundKey::new(alg, &key)
                    .map_err(|_| Error::CryptoFail)?,
            ),
            iv,
        })
    }

    pub fn from_secret(alg: Algorithm, secret: &[u8], enc: u32) -> Result<Self> {
        let mut key = vec![0; alg.key_len()];
        let mut iv = vec![0; 12];
        derive_pkt_key(alg, secret, &mut key)?;
        derive_pkt_iv(alg, secret, &mut iv)?;
        Self::new(alg, key, iv, enc)
    }

    pub fn open_with_u64_counter(
        &self, counter: u64, ad: &[u8], buf: &mut [u8],
    ) -> Result<usize> {
        let nonce =
            aead::Nonce::assume_unique_for_key(make_nonce(&self.iv, counter));
        self.key
            .open_in_place(nonce, aead::Aad::from(ad), buf)
            .map(|p| p.len())
            .map_err(|_| Error::CryptoFail)
    }

    pub fn seal_with_u64_counter(
        &mut self, counter: u64, ad: &[u8], buf: &mut [u8], in_len: usize,
        extra: Option<&[u8]>,
    ) -> Result<usize> {
        let extra = extra.unwrap_or(&[]);
        let len = in_len.checked_add(extra.len()).ok_or(Error::CryptoFail)?;
        if len.checked_add(16).ok_or(Error::CryptoFail)? > buf.len() {
            return Err(Error::CryptoFail);
        }
        buf[in_len..len].copy_from_slice(extra);
        let nonce =
            aead::Nonce::assume_unique_for_key(make_nonce(&self.iv, counter));
        let tag = self
            .key
            .seal_in_place_separate_tag(
                nonce,
                aead::Aad::from(ad),
                &mut buf[..len],
            )
            .map_err(|_| Error::CryptoFail)?;
        buf[len..len + 16].copy_from_slice(tag.as_ref());
        Ok(len + 16)
    }
}
#[derive(Clone)]
pub(crate) struct HeaderProtectionKey(Arc<aead::quic::HeaderProtectionKey>);
impl HeaderProtectionKey {
    pub fn new(alg: Algorithm, key: Vec<u8>) -> Result<Self> {
        let alg = match alg {
            Algorithm::AES128_GCM => &aead::quic::AES_128,
            Algorithm::AES256_GCM => &aead::quic::AES_256,
            Algorithm::ChaCha20_Poly1305 => &aead::quic::CHACHA20,
        };
        Ok(Self(Arc::new(
            aead::quic::HeaderProtectionKey::new(alg, &key)
                .map_err(|_| Error::CryptoFail)?,
        )))
    }

    pub fn new_mask(&self, sample: &[u8]) -> Result<HeaderProtectionMask> {
        self.0.new_mask(sample).map_err(|_| Error::CryptoFail)
    }
}
pub(crate) fn hkdf_extract(
    alg: Algorithm, out: &mut [u8], secret: &[u8], salt: &[u8],
) -> Result<()> {
    let bytes = match alg {
        Algorithm::AES256_GCM =>
            hkdf::Hkdf::<sha2::Sha384>::extract(Some(salt), secret)
                .0
                .to_vec(),
        _ => hkdf::Hkdf::<sha2::Sha256>::extract(Some(salt), secret)
            .0
            .to_vec(),
    };
    if out.len() != bytes.len() {
        return Err(Error::CryptoFail);
    }
    out.copy_from_slice(&bytes);
    Ok(())
}
pub(crate) fn hkdf_expand(
    alg: Algorithm, out: &mut [u8], prk: &[u8], info: &[u8],
) -> Result<()> {
    match alg {
        Algorithm::AES256_GCM => hkdf::Hkdf::<sha2::Sha384>::from_prk(prk)
            .map_err(|_| Error::CryptoFail)?
            .expand(info, out)
            .map_err(|_| Error::CryptoFail),
        _ => hkdf::Hkdf::<sha2::Sha256>::from_prk(prk)
            .map_err(|_| Error::CryptoFail)?
            .expand(info, out)
            .map_err(|_| Error::CryptoFail),
    }
}
pub fn verify_slices_are_equal(a: &[u8], b: &[u8]) -> Result<()> {
    if bool::from(a.ct_eq(b)) {
        Ok(())
    } else {
        Err(Error::CryptoFail)
    }
}
