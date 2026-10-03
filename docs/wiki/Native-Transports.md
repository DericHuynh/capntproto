# Native multiparty transport

Native RPC uses TCP/TLS or standard QUIC with TLS 1.3. Quiche is the sole QUIC
engine and supports explicitly selected QUIC v1 and v2. One authenticated-session
type supplies peer identity, ordered RPC IO and shutdown control to
`native_rpc::Network`.

## Build selection

- `tls`: conventional CA/name-validated TCP/TLS with optional mandatory mTLS.
- `native`: native multiparty sessions, TCP/TLS, the quiche driver, discovery
  and provisioning. Requires a C/C++ compiler, CMake and libclang for BoringSSL.
- `quic` or `quic-quiche`: also enables conventional two-party QUIC RPC using
  the same quiche engine and CA-based TLS/mTLS.

Defaults enable `quic-quiche`, services and storage. Plain TCP is available
without default features. Use `transport::tcp` for authenticated native TCP.

## Identity and admission

`transport::Identity` owns an immutable, zeroized Ed25519 seed and public key.
Generate an identity or import a seed / matching keypair. Old X25519 identities
must be replaced, including identity-bound references.

Native sessions require mutual TLS 1.3. Each side pins the remote Ed25519 public
key, checks its self-signed certificate and validity, and verifies TLS proof of
possession. Native ALPN is `reproto/2`. Resumption and application 0-RTT are disabled.

The optional 32-byte reservation secret and application context authenticate
admission. HMAC-SHA256 binds the sorted local/remote keys, context and secret
presence; its digest is signed into the certificate common name. Shared
reservations include their unpredictable connection ID. This secret is not an
additional TLS traffic key. Existing `psk` arguments refer to this admission secret.

## Routes and features

`DirectoryConnector::insert_with_backend` selects `Backend::Tcp`,
`Backend::Quiche` (v1), or `Backend::QuicheV2`. The directory's allowlist controls
permitted destinations, and its bind address selects the local interface.
Network attachment verifies both local and remote authenticated identities.

Direct session APIs are `transport::tcp::{connect,connect_bound,accept}` and
`transport::{connect_for_version,accept_authenticated}`. Shared UDP reservations
support v1/v2 through `native_listener::connect_for_version` and
`DirectoryConnector::insert_reserved_for_version`.

Both QUIC versions use the same native protocol and support multiparty RPC,
discovery/provisioning, capability handoff, restoration, bounded datagrams,
scheduling, shutdown receipts, validated migration and explicit CID rotation.
There is no second QUIC engine with a reduced feature set.

TCP/TLS supports the ordered native RPC and receipt protocol. TCP has no
unreliable datagrams or QUIC path/CID controls; applications needing these use
quiche. TCP does not emulate these wire semantics. A receipt acknowledges bytes
delivered into the peer's bounded RPC input, not execution of methods.
Dropping a session cancels its driver.

Both QUIC versions authenticate version information. Selection is explicit;
unauthenticated Version Negotiation cannot silently downgrade a connection.
Compatible in-handshake version switching is not implemented.

## Verification

Backend tests exercise TCP/TLS and quiche v1/v2, capability RPC, datagrams,
receipts and shared reservations. Existing discovery, authorization, handoff,
restoration, migration and scheduling tests use TLS sessions. Conventional
TLS/mTLS tests cover CA trust, hostnames, client credentials, ALPN, deadlines
and cancellation. The quiche suite includes RFC 9369 vectors; aioquic provides
an independent interoperability peer. See [secure transports](TCP-TLS-and-QUIC.md)
and [testing](Testing.md).

TLS handshakes use fresh entropy. Recorded packet schedules and bounded model
checks do not establish byte-for-byte reproducibility of encrypted sessions.
