# Examples

[message_basic.rs](message_basic.rs) shows serialization without networking.
The [key/value application](kv) is a separate Cargo package with a
[shared schema](kv/key_value.capnp), [server](kv/src/bin/server.rs),
[client](kv/src/bin/client.rs), and [deep concurrent client](kv/src/bin/deep-client.rs).
Its [service implementation](kv/src/service.rs) is separate from connection setup.
Schemas compile in Rust; the example needs no external Cap'n Proto compiler.
Applications use `capntproto` as their sole Cap'n Proto runtime dependency.

## Run the service

In one terminal:

```sh
cd examples/kv
cargo run --bin server -- --listen 127.0.0.1:4000
```

In another terminal, from the same directory:

```sh
cargo run --bin client -- put greeting hello
cargo run --bin client -- get greeting
cargo run --bin client -- --path users/alice put name Alice
cargo run --bin client -- --path users/alice get name
cargo run --bin client -- delete greeting
cargo run --bin deep-client -- --depth 32 --concurrent 32
```

Each client invocation is a new process and connection. The server stores values
in memory across client connections until it exits. `open` returns a capability
for a child directory; reads and writes use that directory's namespace. The deep
client pipelines `open` calls without awaiting their intermediate replies, then
verifies concurrent writes and reads. Missing keys return a nonzero client exit
status. Ctrl-C stops the server and its connections. Use `--help` on any binary
for its supported arguments.

## TCP security

The default example binds plaintext TCP to loopback. TLS secures the same TCP
transport: supply `--cert` and `--key` on the server, and `--ca` on the client.
Certificates and keys use standard PEM files. Clients validate the CA and
`--server-name` (default `localhost`); verification is never disabled.

For a local development CA and server certificate, run OpenSSL from a temporary
directory, then pass the resulting paths to the programs:

```sh
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj '/CN=Example CA' \
  -addext 'basicConstraints=critical,CA:TRUE' \
  -addext 'keyUsage=critical,keyCertSign,cRLSign' -keyout ca-key.pem -out ca.pem
openssl req -new -newkey rsa:2048 -nodes -subj '/CN=localhost' \
  -keyout server-key.pem -out server.csr
cat > server.ext <<'EXT'
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
subjectAltName=DNS:localhost,IP:127.0.0.1
EXT
openssl x509 -req -in server.csr -CA ca.pem -CAkey ca-key.pem -CAcreateserial \
  -days 1 -extfile server.ext -out server.pem
```

From `examples/kv`, in separate terminals (adjust certificate paths):

```sh
cargo run --bin server -- --cert /path/server.pem --key /path/server-key.pem
cargo run --bin client -- --ca /path/ca.pem put greeting hello
cargo run --bin client -- --ca /path/ca.pem get greeting
cargo run --bin deep-client -- --ca /path/ca.pem
```

## QUIC

QUIC includes TLS 1.3. Enable the example's `quic` feature and select the QUIC
transport; use the same PEM certificate and trust files. The native build needs
CMake, a C/C++ compiler and libclang.
Clients try the hostname's resolved addresses within one setup deadline, so an
unreachable IPv6 or IPv4 address does not prevent a reachable alternative.

```sh
cargo run --features quic --bin server -- --transport quic \
  --cert /path/server.pem --key /path/server-key.pem
cargo run --features quic --bin client -- --transport quic \
  --ca /path/ca.pem put greeting hello
cargo run --features quic --bin deep-client -- --transport quic --ca /path/ca.pem
```

The public API follows this separation: `rpc::tcp` and `rpc::quic` are transports,
`rpc::tcp::tls` wraps TCP with TLS, and `rpc::security` loads shared certificates
and trust. Keep the `rpc::Connection` owner alive while using its capabilities;
`shutdown()` performs bounded cleanup. `rpc::raw` supports integrations that
manage their own driver tasks.

[downstream](downstream) demonstrates generated field APIs and durable storage.
[native_store.rs](native_store.rs), [component_store.rs](component_store.rs), and
[async_storage.rs](async_storage.rs) cover larger applications. C++ interoperability
examples have the additional prerequisites documented in the wiki.
