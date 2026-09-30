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

// Noise IK / IKpsk2 replaces TLS. Initial framing is retained for reliability.
// Compatibility shims reject unsupported TLS settings; key pinning is
// mandatory.
#![allow(dead_code)] // Shared Connection ABI retains TLS-only accessors/fields.
use crate::crypto;
use crate::packet;
use crate::ConnectionError;
use crate::Error;
use crate::Result;
use std::io::Write;
use zeroize::Zeroizing;

#[derive(Clone)]
struct Identity {
    private: Zeroizing<[u8; 32]>,
    peer: [u8; 32],
    psk: Option<Zeroizing<[u8; 32]>>,
    context: Vec<u8>,
}
pub struct Context {
    identity: Option<Identity>,
}
impl Context {
    pub fn new() -> Result<Self> {
        Ok(Self { identity: None })
    }

    pub fn configure(
        &mut self, private: [u8; 32], peer: [u8; 32], psk: Option<[u8; 32]>,
        context: &[u8],
    ) -> Result<()> {
        if context.len() > 4096 {
            return Err(Error::InvalidState);
        }
        self.identity = Some(Identity {
            private: Zeroizing::new(private),
            peer,
            psk: psk.map(Zeroizing::new),
            context: context.to_vec(),
        });
        Ok(())
    }

    pub fn new_handshake(&mut self) -> Result<Handshake> {
        Ok(Handshake {
            identity: self.identity.clone().ok_or(Error::InvalidState)?,
            state: None,
            input: Vec::new(),
            local_params: Vec::new(),
            remote_params: Vec::new(),
            server: false,
            completed: false,
            started: false,
        })
    }

    pub fn use_certificate_chain_file(&mut self, _: &str) -> Result<()> {
        Err(Error::InvalidState)
    }

    pub fn use_privkey_file(&mut self, _: &str) -> Result<()> {
        Err(Error::InvalidState)
    }

    pub fn load_verify_locations_from_file(&mut self, _: &str) -> Result<()> {
        Err(Error::InvalidState)
    }

    pub fn load_verify_locations_from_directory(
        &mut self, _: &str,
    ) -> Result<()> {
        Err(Error::InvalidState)
    }

    pub fn set_curves_list(&mut self, _: &str) -> Result<()> {
        Err(Error::InvalidState)
    }

    pub fn set_ticket_key(&mut self, _: &[u8]) -> Result<()> {
        Err(Error::InvalidState)
    }

    pub fn set_verify(&mut self, _: bool) {}

    // Cannot disable mandatory pinning.
    pub fn enable_keylog(&mut self) {}

    pub fn set_early_data_enabled(&mut self, _: bool) {}

    // Never execute 0-RTT.
    pub fn set_alpn(&mut self, v: &[&[u8]]) -> Result<()> {
        if v == [b"reproto/1".as_slice()] {
            Ok(())
        } else {
            Err(Error::InvalidState)
        }
    }
}
pub struct Handshake {
    identity: Identity,
    state: Option<snow::HandshakeState>,
    input: Vec<u8>,
    local_params: Vec<u8>,
    remote_params: Vec<u8>,
    server: bool,
    completed: bool,
    started: bool,
}
impl Handshake {
    pub fn init(&mut self, server: bool) -> Result<()> {
        self.server = server;
        let name = if self.identity.psk.is_some() {
            "Noise_IKpsk2_25519_ChaChaPoly_BLAKE3"
        } else {
            "Noise_IK_25519_ChaChaPoly_BLAKE3"
        };
        let mut prologue = b"ReProto-Noise-1/ff525001/reproto/1".to_vec();
        prologue.extend_from_slice(&self.identity.context);
        let mut b =
            snow::Builder::new(name.parse().map_err(|_| Error::CryptoFail)?)
                .local_private_key(self.identity.private.as_ref())
                .map_err(|_| Error::CryptoFail)?
                .prologue(&prologue)
                .map_err(|_| Error::CryptoFail)?;
        if !server {
            b = b
                .remote_public_key(&self.identity.peer)
                .map_err(|_| Error::CryptoFail)?;
        }
        if let Some(psk) = &self.identity.psk {
            b = b.psk(2, psk).map_err(|_| Error::CryptoFail)?;
        }
        // Only unit-test builds can supply reproducible ephemeral keys. All
        // production handshakes continue using Snow's secure RNG.
        #[cfg(test)]
        let mut ephemeral = [0; 32];
        #[cfg(test)]
        if crate::simulation::fill_bytes(&mut ephemeral) {
            b = b.fixed_ephemeral_key_for_testing_only(&ephemeral);
        }
        self.state = Some(
            if server {
                b.build_responder()
            } else {
                b.build_initiator()
            }
            .map_err(|_| Error::CryptoFail)?,
        );
        Ok(())
    }

    pub fn clear(&mut self) -> Result<()> {
        self.input.clear();
        self.remote_params.clear();
        self.completed = false;
        self.started = false;
        self.init(self.server)
    }

    pub fn use_legacy_codepoint(&mut self, _: bool) {}

    pub fn set_host_name(&mut self, _: &str) -> Result<()> {
        Ok(())
    }

    pub fn set_quic_transport_params(
        &mut self, params: &crate::TransportParams, server: bool,
    ) -> Result<()> {
        let mut raw = [0; 4096];
        self.local_params =
            crate::TransportParams::encode(params, server, &mut raw)?.to_vec();
        Ok(())
    }

    pub fn quic_transport_params(&self) -> &[u8] {
        &self.remote_params
    }

    pub fn provide_data(
        &mut self, level: crypto::Level, data: &[u8],
    ) -> Result<()> {
        if level != crypto::Level::Initial ||
            self.input.len() + data.len() > 65537
        {
            return Err(Error::CryptoFail);
        }
        self.input.extend_from_slice(data);
        Ok(())
    }

    fn emit(&mut self, ex: &mut ExData) -> Result<()> {
        let mut out = vec![0; 65535];
        let n = self
            .state
            .as_mut()
            .ok_or(Error::CryptoFail)?
            .write_message(&self.local_params, &mut out)
            .map_err(|_| Error::CryptoFail)?;
        let mut frame = (n as u16).to_be_bytes().to_vec();
        frame.extend_from_slice(&out[..n]);
        ex.crypto_ctx[packet::Epoch::Initial]
            .crypto_stream
            .send
            .write(&frame, false)?;
        Ok(())
    }

    pub fn do_handshake(&mut self, ex: &mut ExData) -> Result<()> {
        if self.completed {
            return Ok(());
        }
        if !self.server && !self.started {
            self.emit(ex)?;
            self.started = true;
        }
        if self.input.len() < 2 {
            return Err(Error::Done);
        }
        let n = u16::from_be_bytes([self.input[0], self.input[1]]) as usize;
        if self.input.len() < n + 2 {
            return Err(Error::Done);
        }
        if self.input.len() != n + 2 {
            return Err(Error::CryptoFail);
        }
        let mut payload = vec![0; 65535];
        let state = self.state.as_mut().ok_or(Error::CryptoFail)?;
        let count = state
            .read_message(&self.input[2..], &mut payload)
            .map_err(|_| Error::CryptoFail)?;
        crypto::verify_slices_are_equal(
            state.get_remote_static().ok_or(Error::CryptoFail)?,
            &self.identity.peer,
        )?;
        self.remote_params = payload[..count].to_vec();
        self.input.clear();
        if self.server {
            self.emit(ex)?;
        }
        let state = self.state.as_mut().ok_or(Error::CryptoFail)?;
        if !state.is_handshake_finished() {
            return Err(Error::CryptoFail);
        }
        let (mut c, mut s) = state.dangerously_get_raw_split();
        let (rx, tx) = if self.server { (&c, &s) } else { (&s, &c) };
        let ctx = &mut ex.crypto_ctx[packet::Epoch::Application];
        ctx.crypto_open = Some(crypto::Open::from_secret(
            crypto::Algorithm::ChaCha20_Poly1305,
            rx,
        )?);
        ctx.crypto_seal = Some(crypto::Seal::from_secret(
            crypto::Algorithm::ChaCha20_Poly1305,
            tx,
        )?);
        use zeroize::Zeroize;
        c.zeroize();
        s.zeroize();
        self.completed = true;
        self.state = None;
        Ok(())
    }

    pub fn process_post_handshake(&mut self, _: &mut ExData) -> Result<()> {
        Ok(())
    }

    pub fn is_completed(&self) -> bool {
        self.completed
    }

    pub fn is_resumed(&self) -> bool {
        false
    }

    pub fn is_in_early_data(&self) -> bool {
        false
    }

    pub fn set_session(&mut self, _: &[u8]) -> Result<()> {
        Err(Error::InvalidState)
    }

    pub fn alpn_protocol(&self) -> &[u8] {
        b"reproto/1"
    }

    pub fn server_name(&self) -> Option<&str> {
        None
    }

    pub fn peer_cert(&self) -> Option<&[u8]> {
        None
    }

    pub fn peer_cert_chain(&self) -> Option<Vec<&[u8]>> {
        None
    }

    pub fn early_data_reason(&self) -> u32 {
        0
    }

    pub fn curve(&self) -> Option<&str> {
        Some("X25519")
    }

    pub fn sigalg(&self) -> Option<&str> {
        None
    }

    pub fn cipher(&self) -> Option<crypto::Algorithm> {
        Some(crypto::Algorithm::ChaCha20_Poly1305)
    }

    pub fn write_level(&self) -> crypto::Level {
        if self.completed {
            crypto::Level::OneRTT
        } else {
            crypto::Level::Initial
        }
    }
}
pub struct ExData<'a> {
    pub application_protos: &'a Vec<Vec<u8>>,

    pub crypto_ctx: &'a mut [packet::CryptoContext; packet::Epoch::count()],

    pub session: &'a mut Option<Vec<u8>>,

    pub local_error: &'a mut Option<ConnectionError>,

    pub keylog: Option<&'a mut Box<dyn Write + Send + Sync>>,

    pub trace_id: &'a str,

    pub local_transport_params: crate::TransportParams,

    pub recovery_config: crate::recovery::RecoveryConfig,

    pub tx_cap_factor: f64,

    /// PMTUD configuration: (enable, max_probes)
    pub pmtud: Option<(bool, u8)>,

    pub is_server: bool,
}

#[cfg(test)]
mod tests {
    use super::Context;
    use snow::params::HashChoice;
    use snow::resolvers::CryptoResolver;
    use snow::resolvers::DefaultResolver;

    #[test]
    fn noise_blake3_profile() {
        let mut dh = DefaultResolver
            .resolve_dh(&snow::params::DHChoice::Curve25519)
            .unwrap();
        dh.set(&[1; 32]);
        let initiator: [u8; 32] = dh.pubkey().try_into().unwrap();
        dh.set(&[2; 32]);
        let responder: [u8; 32] = dh.pubkey().try_into().unwrap();
        for server in [false, true] {
            for psk in [None, Some([3; 32])] {
                let name = if psk.is_some() {
                    b"Noise_IKpsk2_25519_ChaChaPoly_BLAKE3".as_slice()
                } else {
                    b"Noise_IK_25519_ChaChaPoly_BLAKE3".as_slice()
                };
                // Noise Initialize(), then MixHash(prologue) and the
                // responder's pre-message static key. Check the actual
                // backend state, including the protocol name's hash binding.
                let mut hash =
                    DefaultResolver.resolve_hash(&HashChoice::Blake3).unwrap();
                let mut expected = [0; 32];
                if name.len() <= expected.len() {
                    expected[..name.len()].copy_from_slice(name);
                } else {
                    hash.input(name);
                    hash.result(&mut expected);
                }
                for input in [
                    b"ReProto-Noise-1/ff525001/reproto/1profile-test".as_slice(),
                    &responder,
                ] {
                    hash.reset();
                    hash.input(&expected);
                    hash.input(input);
                    hash.result(&mut expected);
                }
                let mut context = Context::new().unwrap();
                context
                    .configure(
                        if server { [2; 32] } else { [1; 32] },
                        if server { initiator } else { responder },
                        psk,
                        b"profile-test",
                    )
                    .unwrap();
                let mut handshake = context.new_handshake().unwrap();
                handshake.init(server).unwrap();
                assert_eq!(
                    handshake.state.unwrap().get_handshake_hash(),
                    expected
                );
            }
        }
    }
}
