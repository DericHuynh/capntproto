# Schema compiler qualification

The Rust frontend implements the schema-language production families below and
is checked against C++ revision `0de72d8d8cec6b69edaa29de51d3bd490341f9c2`.
The qualification boundary is the pinned corpus and the documented resource
limits. It does **not** establish equivalence for every possible schema; the
deliberate differences and unqualified combinations below bound the parity claim.

## Language coverage

The reference sources are the pinned [lexer](../vendor/capnproto/c++/src/capnp/compiler/lexer.c++),
[parser](../vendor/capnproto/c++/src/capnp/compiler/parser.c++) and
[grammar representation](../vendor/capnproto/c++/src/capnp/compiler/grammar.capnp).
Acceptance tests also load emitted requests through the Rust runtime validator.
Differential tests compare known node fields, canonical annotation/default/constant
bytes, source information, requested files and normalized identifier references.
They exclude the compiler-version field and do not equate diagnostic wording.

- Lexical forms and expression delimiters: identifiers, contextual keywords,
  operators, comments, BOM/ASCII spacing, integer/float radices, quoted strings,
  escapes, backtick strings, binary literals, parentheses, lists and trailing
  commas. [Grammar/lexical corpus](../crates/capnp-compiler/tests/corpus/grammar.rs),
  [numeric corpus](../crates/capnp-compiler/tests/corpus/numbers.rs),
  [string tests](../crates/capnp-compiler/tests/strings.rs).
- Declarations: file IDs, structs, enums/enumerants, interfaces/methods,
  constants, annotations, groups and named/unnamed unions, including legacy union
  ordinals. [Core/layout oracle](../tests/schema_compiler.rs),
  [annotations](../tests/schema_compiler/annotations.rs),
  [interfaces](../tests/schema_compiler/interfaces.rs).
- Types and names: primitives, pointer constraints, lists, nested/absolute names,
  aliases, generic application/inheritance, lexical shadowing and implicit method
  parameters. [Generic oracle](../tests/schema_compiler/generics.rs) and
  [portable grammar tests](../crates/capnp-compiler/tests/grammar.rs).
- Values: scalar/enum/blob/list/struct constants and defaults, groups/unions,
  nested composites, generic values, annotation applications and explicit null
  method defaults. [Constants/composite oracle](../tests/schema_compiler.rs),
  [native constants](../crates/capnp-compiler/tests/constants.rs),
  [native composites](../crates/capnp-compiler/tests/composites.rs).
- Source graphs: relative/absolute imports, cyclic imports, file identity,
  re-exported aliases, binary embeds, custom providers, optional file IDs,
  lazy declaration discovery and failed-extension rollback.
  [Import tests](../crates/capnp-compiler/tests/imports.rs),
  [embed oracle](../tests/schema_compiler/embeds.rs),
  [provider oracle](../tests/schema_compiler/provider.rs),
  [ID oracle](../tests/schema_compiler/file_ids.rs),
  [session oracle](../tests/schema_compiler/session.rs),
  [import-discovery oracle](../tests/schema_compiler/discovery.rs).
- Compiler metadata and generated consumers: declaration/member ranges,
  documentation, identifier references, generated Rust declarations/constants,
  imported types, annotations, generics and executable RPC calls.
  [Source-info oracle](../tests/schema_compiler/source_info.rs),
  [identifiers](../tests/schema_compiler/identifiers.rs),
  [Rustdoc](../tests/schema_compiler/rustdoc.rs), and generated-code acceptance
  cases in the root compiler suite.

The [upstream corpus test](../tests/schema_compiler/upstream.rs) discovers every
real `.capnp` file in the pinned C++ source/sample tree, skipping mirrored Ekam
symlinks. All **22** unmodified schemas compare without metadata normalization
beyond the existing standard StreamResult location handling. Sources under `src`
use that standard include/source root; samples use their sample source root.
This includes the language's own lexer/grammar schemas, runtime schemas,
compatibility schemas, benchmarks and examples. A fixed count detects reference
corpus changes. The existing suite separately compares all 21 repository schemas.

The mixed-root `test-import2.capnp` invocation also compares unchanged requests,
including the file and annotation display names/prefix lengths that previously
differed. A shared [22-case corpus](../crates/capnp-compiler/tests/corpus/discovery.rs)
exercises the same source identity through relative and absolute import spellings.
It covers dependencies before unused aliases, child declaration order, alias name
order, slot ordinals across nested groups, member annotation source order,
generic declaration/brand dependencies, inline and explicit RPC signatures, and
scalar versus deferred pointer defaults. Every case compares all the usual
request fields and canonical values without display-name normalization.
[Portable tests](../crates/capnp-compiler/tests/discovery.rs) also check concurrent
cache initialization, lazy extension and retained snapshot names/bytes.

## Known differences and remaining work

- Integer literals larger than UInt64 are checked errors when evaluated in Rust;
  the pinned C++ lexer wraps arithmetic. Unbound generic list elements are
  consistently rejected in Rust; C++ incorrectly varies by parameter index.
  These are deliberate differences, covered by regression tests.
- Incomplete float exponents produce ordinary Rust diagnostics but can cause
  C++ exceptions. Only rejection is compared. Error wording, recovery into
  partially invalid requests and diagnostic multiplicity are not parity promises.
- Frontend graph, work, nesting and expansion limits are implementation bounds.
  Exhaustive grammar combinations and source/error equivalence beyond the corpus
  remain unqualified. Further differential generation should focus on interacting
  import aliases, annotation dependencies and nested generic expressions.

## Concurrent cache coverage

[ConcurrentSchemaParser](../crates/capnp-compiler/src/cache.rs) owns one session on
a worker. Cloneable clients share source reads, positive alias resolution and
successful compilation. A bounded queue serializes mutations; immutable snapshots
are transferable across threads and materialize independent local reflection.
The requested-file set and successful input bytes remain fixed for the cache's
lifetime. Persistent caching, automatic invalidation, parallel compilation and
in-place mutation of retained runtime handles are outside this API.

[Portable tests](../crates/capnp-compiler/tests/cache.rs) use 32 simultaneous
lookups and cover shared storage/read counts, concurrent distinct extensions,
failed batches, loader limits, disk correction/retry, optional IDs, snapshot
lifetimes, owned non-Sync providers, last-client shutdown, callback reentry, and
24 callers released after a provider panic. The existing C++ session oracle now
compares **18 snapshots from each of the exclusive and concurrent APIs**, including
aliases, generic erasure, implicit nodes, metadata and lazy dependency closures.

## Reproduction

Activate the [auditable Cargo wrapper](QUALITY.md#auditable-cargo-builds), then:

```sh
cargo test --locked -p capnp-compiler
cargo test --locked -p reproto --test schema_compiler
```

These tests are included in `cargo test --workspace`. Portable cache tests also
run in the existing Linux/macOS/Windows compiler smoke jobs. The root suite needs
the pinned C++ source and CMake/C++ toolchain and also compiles generated Rust.
Focused filters are `--test cache` or `--test discovery` for the compiler crate
and `upstream::`, `discovery::` or `session::` for the root compiler integration
test. Request dumps and summaries are retained under
`target/verification/schema-compiler/upstream/`, `discovery/` and `session/`.
