# Native multiparty transport

Native RPC uses TCP/TLS or standard QUIC with TLS 1.3. Quiche is the sole QUIC
engine and uses unmodified upstream quiche 0.30.0 with QUIC v1. One authenticated-session
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
possession. Native QUIC ALPN is `capntproto/3`; Native TCP retains `reproto/2`. Resumption and application 0-RTT are disabled.

The optional 32-byte reservation secret and application context authenticate
admission. HMAC-SHA256 binds the sorted local/remote keys, context and secret
presence; its digest is signed into the certificate common name. Shared
reservations include their unpredictable connection ID. This secret is not an
additional TLS traffic key. Existing `psk` arguments refer to this admission secret.

## Routes and features

`DirectoryConnector::insert_with_backend` selects `Backend::Tcp`,
`Backend::Quiche` (v1). `Backend::QuicheV2` is retained as an explicit
unsupported selection and fails at configuration. The directory's allowlist controls
permitted destinations, and its bind address selects the local interface.
Network attachment verifies both local and remote authenticated identities.

Direct session APIs are `transport::tcp::{connect,connect_bound,accept}` and
`transport::{connect_for_version,accept_authenticated}`. Shared UDP reservations
support v1 through `native_listener::connect_for_version` and
`DirectoryConnector::insert_reserved_for_version`.

Native QUIC v1 supports multiparty RPC,
discovery/provisioning, capability handoff, restoration, bounded datagrams,
scheduling, shutdown receipts, validated migration and explicit CID rotation.
There is no second QUIC engine with a reduced feature set.

TCP/TLS supports the ordered native RPC and receipt protocol. TCP has no
unreliable datagrams or QUIC path/CID controls; applications needing these use
quiche. TCP does not emulate these wire semantics. A receipt acknowledges bytes
delivered into the peer's bounded RPC input, not execution of methods.
Dropping a session cancels its driver.

Version selection is explicit; requesting v2 fails without downgrade.

## Packet sizes and execution

Linux QUIC sockets forbid IP fragmentation and let upstream Quiche discover a
path MTU up to 16 KiB. Other platforms retain a 1,350-byte outgoing limit;
all platforms accept packets up to 16 KiB. QUIC starts at its 1,200-byte minimum,
retains a working smaller size when probes fail, and revalidates a discovered
MTU after repeated timer expirations without acknowledgement progress. Oversize
send errors are handled as lost probes. This improves large loopback transfers;
ordinary network paths still determine their own smaller MTU.

Shared listeners bound each route by both its configured packet count and
128 KiB of queued payload. The unreliable application datagram limit remains
1,024 bytes. Reliable QUIC stream staging and each application bridge direction
are bounded at 128 KiB. This leaves space for RPC framing around a 64 KiB payload
without requiring another bridge transfer. It doubles the previous buffer bounds;
larger transfers still apply backpressure and drain incrementally.

Run application futures as tasks inside the same Tokio `LocalSet` as RPC tasks
(`spawn_local`), including the main request loop. This avoids extra executor
turns between the outer `run_until` future and local RPC tasks. All Rust RPC
benchmarks use this placement. Linux Native sockets subscribe to read events and request write readiness only
when a send blocks, avoiding an extra writable wakeup after every UDP send.
Bounded local task turns give RPC replies an opportunity to share their ACK.

Checkout release builds enable full LTO and one
codegen unit; downstream applications select these options in their own Cargo
release profile because dependency-local Cargo configuration is not inherited.

Native QUIC uses bounded request streams 2/3, receipt streams 6/7, and
confirmation streams 10/11. A confirmation echoes the exact validated receipt
(nonce and byte count). In a crossed shutdown, a peer must confirm our reciprocal
receipt before success. The authenticated close reason carries the same echo
so close-packet reordering does not lose a validated receipt. A close code alone
cannot substitute for it. `capntproto/3` prevents mixing this exchange with the
former fork-dependent profile. This confirms bytes delivered to bounded input,
not application execution.

## Verification

Backend tests exercise TCP/TLS and quiche v1, capability RPC, datagrams,
receipts and shared reservations. Existing discovery, authorization, handoff,
restoration, migration and scheduling tests use TLS sessions. Conventional
TLS/mTLS tests cover CA trust, hostnames, client credentials, ALPN, deadlines
and cancellation. The version gate rejects v2; aioquic provides
an independent interoperability peer. See [secure transports](TCP-TLS-and-QUIC.md)
and [testing](Testing.md).

TLS handshakes use fresh entropy. Recorded packet schedules and bounded model
checks do not establish byte-for-byte reproducibility of encrypted sessions.
