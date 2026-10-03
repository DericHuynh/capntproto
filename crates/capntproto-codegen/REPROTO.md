Derived from crates.io capnpc 0.25.3. Original license and metadata retained.

Capntproto generator changes:

* Sort reflection field lookup tables by original schema names. `$Rust.name`
  changes generated identifiers without changing names in the encoded schema;
  using renamed identifiers to sort this table broke binary-search lookup.
* Emit AnyPointer constants. Explicit AnyPointer defaults are borrowed by
  readers and copied into null fields by builders, preserving infallible getter
  signatures and existing non-null values. Compiler integration tests verify
  default materialization, mutation, clearing and independent message storage.
* Use Error::from_kind instead of Error struct literals for structured metadata.
* Emit the method/interface/file allowCancellation policy in server dispatch;
  inherited methods retain their declaring interface policy.
* Infer noPromisePipelining from result schema capability reachability, including
  groups, lists and recursive structs. AnyPointer and generic fields conservatively
  permit capabilities. Streaming requests disable result pipelining.

Original source hashes remain in vendor/provenance/capnpc-revision.json. Generated bindings
require the workspace Capntproto capnp core API.

* Emit interface/method/superclass reflection and generic brand parameter
  metadata, retaining enclosing-scope and unused parameters. Generated struct
  reflection preserves its brand through dynamic conversions.
* Generated server hooks support self-capabilities and ordered shortenPath.

* Opt-in `field_api(true)` / plugin `--field-api` emits the API-design `api`
  module and legacy `into_api()` bridges. See docs/wiki/Rust-Generator.md for supported
  contracts, acceptance tests, and deferred runtime features.

* Field API descriptors retain union selection for scoped edits. Non-group
  editors expose lossless fixed-layout copy. Generated clients expose typed
  `method_call()` editors with awaitable results and pre-await pipelines.

* Explicit RPC parameter/result aliases order used generic parameters by lexical
  declaration, including outer scopes, instead of randomized HashSet traversal.
  This fixes standard Persistent aliases with distinct owner/reference types.

* Union Reader/Builder aliases retain every enclosing parameter used by generated
  group types, even when the group's fields do not mention those parameters.

* Field API groups implement GroupSchema with merged data-bit masks and unique
  pointer slots, including nested union arms. GroupField::replace_with uses this
  metadata for staged construction without overwriting parent siblings.

* Field facade errors include original schema names/ordinals, independent of
  Rust renaming. Generated native conversions accumulate fields, union context
  and list indices while preserving ErrorKind and RPC exception metadata.

* Bound superclass expansion during server generation: return errors for cycles,
  depth of 64 or greater, and more than 65,536
  inherited edges per interface. Preserve distinct branded diamond paths within
  those limits. Rust frontend integration tests cover cycles, deep chains,
  exponential diamonds and a generated inherited RPC round trip.

* Render schema source-info as Rustdoc in standard bindings and the field API,
  including union variants, RPC methods, projections and native values. Comments
  use escaped attributes and one indexed lookup; partial metadata is tolerated,
  while duplicate source-info IDs and invalid UTF-8 return errors. File comments
  use a collision-free `schema_documentation` module compatible with `include!`.
  CommonMark parsing/serialization marks untagged examples as text and preserves
  explicit Rust doctests. This fixes accidental doctests from schema pseudocode
  and indented URLs. The Markdown dependencies are build-time generator dependencies.

* Opt-in `structured_replies(true)` / `--structured-replies` generates server
  methods using consuming Reply stages. It composes with field_api; default
  legacy bindings remain supported. See the [RPC contract](../../docs/wiki/RPC-Applications.md#structured-server-replies).
