# Optional Cap’n Proto compatibility

`capnp-compat` ports the pinned C++ compatibility protocols into an opt-in Rust
workspace crate. It uses the maintained `capnp` / `capnp-rpc` runtime and requires
Rust 1.97. The crate is unpublished; it is not a dependency of `reproto`.

The modules provide:

- `json::JsonCodec`: dynamic schema values, exact 64-bit integer strings,
  non-finite floats, lists/groups/unions, presence modes, unknown-field policy,
  type/field handlers, and the standard `name`, `flatten`, `discriminator`,
  `base64` and `hex` annotations. Embedded `json.Value` values are supported.
  Handler registrations retain schema-loader and generic-brand identity.
- `text::TextCodec`: bidirectional Cap’n Proto text values, compact/pretty output,
  byte-preserving strings and standalone orphan values. It uses the Rust schema
  compiler’s lexer. External constants, imports and embeds are rejected.
- `byte_stream::ByteStreamFactory`: the standard streaming capability, explicit
  EOF, TLS-upgrade callbacks, local capability unwrapping, bounded substreams,
  byte-count callbacks and capability path resolution. `OutputIo` adapts to
  futures `AsyncWrite`; close sends EOF and drop aborts. Legacy EOF on drop is
  available only with a supplied `CallExecutor`.
- `http::HttpOverCapnpFactory`: level-2 request/response streaming, fixed and
  unknown body sizes, compact common headers, repeated headers, HEAD/no-body
  constraints, WebSocket upgrades, and CONNECT acceptance/rejection and tunnels.
- `websocket::WebSocketMessageStream`: one serialized Cap’n Proto message per
  binary WebSocket message, ordered writes, close, bounded reads and a futures I/O
  facade for `capnp_rpc::twoparty::VatNetwork`.
- `json_rpc::JsonRpc`: schema-based, bidirectional JSON-RPC 2.0, named parameters,
  renamed methods, notifications, results/errors, cancellation cleanup and bounded
  pending calls. `ContentLengthTransport` supports language-server framing.

## Ownership and integration

Codecs use `capnp::schema_loader::dynamic` values. Load generated metadata with
`SchemaLoader::load_compiled_type_and_dependencies`, or use the compiler’s
`parse_schemas` / `parse_session` APIs. A `JsonCodec` that registers schemas borrows
that loader; it cannot outlive or silently observe a replaced schema.

`decode` fills an existing dynamic struct. `decode_orphan` creates a typed value
through `dynamic::orphan::Access`, suitable for checked adoption into structs,
lists or result roots. Decode errors can leave an existing destination partially
modified, as in C++; decode into a fresh message when transactional input is needed.

Adapters use futures I/O and local futures; applications supply their executor,
HTTP service, WebSocket implementation and TLS-upgrade implementation. The crate
does not implement HTTP parsing, WebSocket handshakes, TLS or socket authentication.
`JsonRpc::new` returns a handle and driver: poll the driver concurrently with calls.
Dropping it rejects pending calls. Capabilities and promise pipelines cannot be
carried by JSON-RPC. Batches and positional parameters are not supported, matching
the scope of the pinned C++ bridge. HTTP `startRequest` is intentionally unimplemented;
the pinned C++ factory supports level 2 (`request`).

Backpressure is explicit: the HTTP body pipe allows one acknowledged chunk at a
time and splits writes into 64 KiB chunks. A missing `end()` is truncation, not EOF.
The byte-stream adapter’s TLS method invokes application policy; it never silently
upgrades a connection or chooses credentials.

## Limits and wire details

JSON defaults: 4 MiB input, 16 MiB encoded output, nesting depth 64, and 100,000
schema/value visits per conversion. Configurable nesting is capped at 256 to
protect the native stack. Annotation registration validates flattened name
collisions and rejects recursive flattening through its depth/work limits.
Text uses the compiler’s 4 MiB source, token and nesting limits, plus a 16 MiB
output limit. WebSocket users supply a maximum message size; the adapter also
checks segment counts, complete framing and reader traversal limits. It rejects
trailing/partial data rather than silently discarding it. It carries no file
descriptors. Close without status uses `None`; WebSocket implementations must not
send reserved status 1005 on the wire.

JSON object order and duplicate keys are retained. `Call` and `Raw` values are
encoding facilities: the parser produces neither. Raw output is trusted text and
must come from a trusted serializer. JSON null pointer fields are ignored during
struct decode, preserving existing values and union selection, as in C++.
Unknown enum ordinals can be encoded numerically but both JSON and text decode
require names.
Opaque pointer/capability text is diagnostic output and is not round-trippable.

Text numeric literals retain their integer or floating-point kind through type
checking. Numbers cannot initialize Text or enum fields, and floating-point
literals cannot initialize integer fields. Decimal, lowercase `0x` hexadecimal
and leading-zero octal integers are range-checked before conversion. Positive
integers beyond UInt64 are rejected, matching the Rust compiler's checked policy;
the pinned C++ lexer wraps those oversized values. Negative magnitudes beyond
Int64 are rejected even for floating-point destinations.

Integer-to-Float32 conversion rounds directly, preserving C++ results near large
integer rounding boundaries. Integer `-0` is zero (including unsigned fields),
while `-0.0` retains its floating-point sign. Floating-point overflow and underflow
follow C++ infinity and signed-zero behavior. Hexadecimal byte literals accept
all six ASCII whitespace characters, including vertical tabs.

Text values support one level of implicit scalar wrapping through a struct or
group's first field in schema order. For example, `child = true` is equivalent to
`child = (flag = true)` when `flag` is the child's first field. Other fields use
their defaults. The same conversion works for list elements and typed orphan
decoding, including bound generic fields and union members. `decode()` into an
existing root still requires an explicit `(field = value, ...)` expression.

The scalar must have an unambiguous type before selecting the first field.
Wrapping does not infer enum/list/Data literals, convert quoted Text to Data,
unwrap a second struct/group, or choose a later field whose type happens to fit.
Explicit nested struct expressions remain available for those cases. Existing
integer range checks and nesting limits apply.

Text assignments run in source order, including repeated fields and successive
union selections. Later successful assignments replace earlier values. Struct
pointer and list replacements are evaluated before adoption; a failed replacement
retains the previous value and union selection. Groups reset and fill in place,
so an error leaves their reset defaults and any successfully assigned fields.
Resetting a group selects its default union alternative and clears its non-union
fields, preserving storage exclusive to inactive alternatives as C++ does.
Semantic errors stop at the first failing assignment; syntax errors leave the
destination unchanged. Failed orphan decoding leaves the destination untouched.

Text encoding preserves valid Unicode and uses C++ short escapes for controls
and quotes. Data always escapes high bytes as octal. If a Text value contains
invalid UTF-8, Rust uses reversible octal escapes to keep the output a valid
`String`; C++'s byte-string API can emit those invalid bytes directly.

## Verification

All tests are part of the root `cargo test --workspace` command. For focused work,
retain the repository’s cargo-auditable PATH setup and run:

```sh
cargo test --locked -p capnp-compat
```

Portable tests cover codecs, annotations, handlers, generic brands, limits,
backpressure, EOF/abort, substreams, HTTP, WebSocket framing, JSON-RPC errors,
concurrency, cancellation and Content-Length framing. Linux tests build the
pinned C++ reference and compare canonical wire bytes, compact/pretty JSON and
text, and floating-point formatting. Loopback tests exercise C++ ByteStream,
HTTP upload/echo, WebSocket upgrade, CONNECT and JSON-RPC peers with a 30-second
execution deadline and automatic child cleanup.

The codec oracle compares 152 basic accepted input/format combinations,
66 shared numeric/type rejections, and 72 ordered-assignment combinations
(including 30 failure states). All successful and partial-state comparisons check
canonical bytes and exact compact/pretty JSON/text. It exercises C++'s public
generated-type orphan decoder as well as mutable-root decoding, including updates
to pre-populated destinations. Portable tests additionally assert
Float32 and signed-zero bit patterns, the deliberate positive-integer overflow
boundary, wrapped-value replacement/defaults, ordered assignment results,
partial changes after errors and non-UTF-8 preservation.
Inputs, C++ diagnostics and the summary are under `target/verification/compat/`.

Platform CI compiles this crate and runs portable smoke tests on Linux, macOS
and Windows. Full LLVM reporting includes every source file under this crate.
These are bounded interoperability tests, not an exhaustive KJ replacement or
production qualification.

The bundled schemas are unmodified copies of reference revision
`0de72d8d8cec6b69edaa29de51d3bd490341f9c2`. See [NOTICE](../../crates/capnp-compat/NOTICE) for attribution.
