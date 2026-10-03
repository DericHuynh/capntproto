// Copyright (C) 2026, ReProto contributors.
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

//! Standard QUIC v2 vectors and authenticated version parameter checks.
//! RFC 9369 Appendix A vectors, IETF Trust; see ../LICENSE-RFC9369.
use crate::*;
fn hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
#[test]
fn rfc9369_server_initial() {
    let cid = hex("8394c8f03e515708");
    let (_, mut seal) = crypto::derive_initial_key_material(
        &cid,
        PROTOCOL_VERSION_V2,
        true,
        true,
    )
    .unwrap();
    let mut bytes = hex("d16b3343cf0008f067a5502a4262b50040750001");
    let offset = bytes.len();
    bytes.extend(hex(
        "02000000000600405a020000560303ee fce7f7b37ba1d1632e96677825ddf739
 88cfc79825df566dc5430b9a045a1200 130100002e00330024001d00209d3c94
 0d89690b84d08a60993c144eca684d10 81287c834d5311bcf32bb9da1a002b00 020304",
    ));
    let payload = bytes.len() - offset;
    bytes.resize(bytes.len() + 16, 0);
    let mut out = octets::OctetsMut::with_slice(&mut bytes);
    packet::encrypt_pkt(&mut out, 1, 2, payload, offset, None, &mut seal)
        .unwrap();
    assert_eq!(bytes,hex("dc6b3343cf0008f067a5502a4262b500 4075d92faaf16f05d8a4398c47089698
 baeea26b91eb761d9b89237bbf872630 17915358230035f7fd3945d88965cf17
 f9af6e16886c61bfc703106fbaf3cb4c fa52382dd16a393e42757507698075b2
 c984c707f0a0812d8cd5a6881eaf21ce da98f4bd23f6fe1a3e2c43edd9ce7ca8 4bed8521e2e140"));
    let hdr = Header::from_slice(&mut bytes, 0).unwrap();
    assert_eq!(hdr.ty, Type::Initial);
    assert_eq!(hdr.version, PROTOCOL_VERSION_V2);
}
#[test]
fn rfc9369_retry() {
    let mut bytes=hex("cf6b3343cf0008f067a5502a4262b5746f6b656ec8646ce8bfe33952d955543665dcc7b6");
    let len = bytes.len() - 16;
    let mut packet = octets::OctetsMut::with_slice(&mut bytes);
    packet.skip(len).unwrap();
    assert!(packet::verify_retry_integrity(
        &packet,
        &hex("8394c8f03e515708"),
        PROTOCOL_VERSION_V2
    )
    .is_ok());
    assert!(packet::verify_retry_integrity(
        &packet,
        &hex("8394c8f03e515708"),
        PROTOCOL_VERSION_V1
    )
    .is_err());
}
#[test]
fn malformed_version_information_is_rejected() {
    for value in [vec![], vec![0; 4], vec![1; 5], hex("6b3343cf00000001")] {
        let mut bytes = vec![0x11, value.len() as u8];
        bytes.extend(value);
        assert!(TransportParams::decode(&bytes, true, None).is_err());
    }
    let bytes = hex("11086b3343cf6b3343cf11086b3343cf6b3343cf");
    assert!(TransportParams::decode(&bytes, true, None).is_err());
}
#[test]
fn resumption_cache_cannot_cross_versions() {
    let mut cfg = test_utils::config_no_pq(PROTOCOL_VERSION_V2).unwrap();
    let mut conn = connect(
        None,
        &ConnectionId::from_ref(&[1; 16]),
        "127.0.0.1:1".parse().unwrap(),
        "127.0.0.1:2".parse().unwrap(),
        &mut cfg,
    )
    .unwrap();
    assert_eq!(
        conn.set_session(&PROTOCOL_VERSION_V1.to_be_bytes()),
        Err(Error::InvalidState)
    );
}
