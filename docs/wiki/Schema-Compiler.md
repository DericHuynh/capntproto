# capnp-compiler

An experimental Cap'n Proto schema-language frontend written in Rust. It parses
source, resolves imports, aliases, generic bindings, constants and annotations,
checks typed values, assigns IDs and wire offsets, and emits a standard `CodeGeneratorRequest`. It provides the in-memory
`SchemaParser`, filesystem `FileCompiler`, callback-based `SourceCompiler`, runtime `ParsedSchemas` snapshots and
lazy `SchemaSession` loading, worker-owned `ConcurrentSchemaParser` caches,
and `capnp-compile` CLI. None invokes C++.

The repository schema corpus and all 22 unmodified pinned upstream schemas are compared with
C++ requests under their standard source roots, including documentation and
source ranges. Mixed import roots also match first-discovered display names;
see the [qualification report](Compiler-Qualification.md).
The frontend remains experimental; passing this corpus does not establish full parity.
The root and test-support build scripts now use both Rust stages. The vendored RPC
crate and downstream example retain the external `capnpc::CompilerCommand` path.
`capnpc` owns Rust generation from compiled requests; this crate owns the textual
frontend. It is a root workspace member, uses Rust 1.97 and the maintained `capnp`
dependency, and is not published.

## Command line

Activate the repository's [auditable Cargo wrapper](Quality-and-Benchmarks.md#auditable-cargo-builds)
first. From the repository root, compile both files in the cyclic import example:

```sh
cargo run --locked -p capnp-compiler --bin capnp-compile -- \
  --src-prefix crates/capnp-compiler/examples/imports \
  crates/capnp-compiler/examples/imports/main.capnp \
  crates/capnp-compiler/examples/imports/common.capnp > target/import-request.bin
cargo build --locked --manifest-path vendor/capnpc/Cargo.toml --bin capnpc-rust
mkdir -p target/schema-compiler-demo
(cd target/schema-compiler-demo && \
  ../../vendor/capnpc/target/debug/capnpc-rust < ../import-request.bin)
```

This writes `main_capnp.rs` and `common_capnp.rs` under
`target/schema-compiler-demo/`. The CLI writes binary requests to stdout and
errors to stderr. `-I DIR`, `-IDIR`, and `--import-path DIR` add import roots;
`--src-prefix DIR` sets the directory stripped from requested filenames, defaulting
to the working directory. `--` ends option parsing. Explicitly list every file
whose bindings you want generated; imported dependencies alone are not marked as
requested files.

## Library and build scripts

For source already in memory, `compile(filename, text)` handles one file.
`SchemaParser` handles multiple virtual files without filesystem access:

```rust
let mut parser = capnp_compiler::SchemaParser::new();
parser.add_source("main.capnp", r#"
    using Types = import "types.capnp";
    @0xaaaaaaaaaaaaaaaa;
    struct Message { item @0 :Types.Item; }
"#)?;
parser.add_source("types.capnp", "@0xbbbbbbbbbbbbbbbb; struct Item {}")?;
let request = parser.parse(&["main.capnp", "types.capnp"])?;
```

Virtual filenames are relative, slash-separated paths. `parser.import_path("include")?`
adds a virtual import root; `.` names the virtual root. Source registrations are
immutable: duplicate normalized names are rejected. Each parse builds fresh
state, so failed compilation does not poison later attempts.
`parser.add_file("assets/blob.bin", bytes)?` registers raw bytes for `embed`; source
and embedded files share the same virtual namespace. UTF-8 files registered this
way can also be imported as schemas.

For files on disk, a build script for supported schemas can use both Rust stages:

```rust
let compiled = capnp_compiler::FileCompiler::new()
    .src_prefix("schema")
    .import_path("schema")
    .compile_with_dependencies(&["schema/main.capnp", "schema/common.capnp"])?;
for input in &compiled.dependencies {
    println!("cargo:rerun-if-changed={}", input.display());
}
let bytes = capnp::serialize::write_message_to_words(&compiled.message);
capnpc::codegen::CodeGenerationCommand::new()
    .output_directory(std::env::var_os("OUT_DIR").unwrap())
    .run(bytes.as_slice())?;
```

The caller supplies `capnp`, `capnp-compiler` and `capnpc` build dependencies.
`compile_with_dependencies` returns a `FileCompilation` containing the request
and sorted, unique absolute paths for loaded schemas and embeds. It includes
symlink spellings and canonical targets, excludes unused lazy imports, and starts
fresh on each invocation. Watch import directories separately when adding a
higher-priority file should trigger a rebuild.

The root and test-support build scripts use this API and emit Cargo rerun
instructions for those inputs. Their standard import root is `vendor`, containing
the maintained `capnp/c++.capnp` and bundled `capnp/stream.capnp`; Rust annotations
come from `vendor/capnpc`. The root build also honors `CAPNP_INCLUDE_DIR` before
that fallback. The vendored RPC crate still uses the external compiler, so the
whole workspace still requires C++ tooling. A distributable compiler dependency
is needed before migrating that separately packaged crate.

## Custom source providers

`SourceCompiler::new(&provider)` accepts an application-defined
[`SourceProvider`](../../crates/capnp-compiler/src/source/custom.rs), such as an archive, package registry or
virtual filesystem. `resolve(from, path)` returns a `SourceFile` with an opaque
identity and logical filename; `open(identity)` returns a borrowed or owned
`std::io::Read`. Imports and binary `embed` expressions use the same provider.
The compiler reads at most 4 MiB plus one byte per opened input to detect overflow,
and retains its 16 MiB combined-input and other compilation limits.

`from` is `None` for requested names and otherwise the importing file's identity.
Import strings arrive without path normalization. The provider controls relative
paths, import-root order and visibility; there is no implicit disk lookup.
Aliases must share identity, while different files and import contexts must have
different identities. Both identity and filename are limited to 4096 bytes,
must be nonempty and cannot contain control characters. Logical filenames feed
diagnostics and generator output names, so choose them accordingly.

`compile()` returns a standard generator request. `parse_schemas()` returns an
owned validated snapshot, and `parse_session()` returns a lazy session borrowing
the provider. Both reflection methods also have `_with_limits` variants. Successful
schema/embed inputs are cached separately by identity within a session; failed
extensions discard provisional compiler state and permit retry. Callback side
effects are not rolled back. Fresh compilations read inputs again. Callbacks are
synchronous and need neither `Send` nor `Sync`; use the [owned-provider cache](#concurrent-parser-caching) for concurrent callers. Custom snapshots have an empty filesystem `dependencies()` list; providers
can record their own input dependencies.

The [provider fixtures](../../crates/capnp-compiler/examples/provider-main.capnp) and
[tests](../../crates/capnp-compiler/tests/provider.rs) exercise opaque imports, aliases, cycles, binary embeds,
limits and rollback. The root integration test compares these operations with
the pinned C++ `SchemaFile` interface.

## Runtime reflection

All three source entry points provide `parse_schemas(...)`, which compiles text and validates the
result with the runtime schema loader before returning an owned `ParsedSchemas`.
This supports dynamic messages without generating Rust bindings:

```rust
use capnp::schema_loader::dynamic::{Builder, Value};
let mut parser = capnp_compiler::SchemaParser::new();
parser.add_source("config.capnp", r#"
    @0xaaaaaaaaaaaaaaaa;
    struct Config { port @0 :UInt16 = 80; }
"#)?;
let parsed = parser.parse_schemas(&["config.capnp"])?;
let config = parsed.get_file("config.capnp")?.get_nested("Config")?;
let mut message = capnp::message::Builder::new_default();
let mut value = Builder::init(message.init_root(), config.schema())?;
value.set_named("port", Value::UInt16(8080))?;
```

`requested_files()` preserves request order and deduplicates file identities.
`get_file()` uses the exact logical filename in the request. `get(id)` and
`get_all_loaded()` expose compiled declarations, including imported dependencies,
groups and implicit method structures. `source_info(id)` retains documentation
and byte ranges, while `request()` retains the complete generator request and
identifier tables. Filesystem snapshots also retain `dependencies()` for build
input tracking.

`ParsedSchema::schema()` supplies the ordinary runtime handle for fields, brands,
constants, annotations, methods and dynamic messages. `find_nested()`,
`get_nested()` and `get_all_nested()` navigate direct declarations in schema order.
Aliases, fields, groups, methods and dotted paths are not nested declarations.
Implicit group/method schemas remain accessible by their runtime schema IDs.

These are immutable snapshots: parsing again starts fresh, and old snapshots
survive source changes, deletion or parser destruction. Handles borrow their
snapshot and cannot outlive it. `loader()` is read-only; cloning it permits
independent mutation without invalidating retained source metadata. Compilation
errors are `ParseError::Compile`; runtime validation failures are `ParseError::Load`.
`parse_schemas_with_limits()` accepts runtime `schema_loader::Limits` in addition
to the frontend's existing limits. No partially validated snapshot is returned.

Snapshot lookup does not resolve aliases or compile additional
declarations. Imported nodes keep dependency-only selection: looking up a declared
but uncompiled child returns an error, while an absent name returns `None`.
Explicitly requesting an imported file compiles its complete declaration subtree.
Concurrent caching is available through the [worker-owned cache](#concurrent-parser-caching). [Optional file IDs](#optional-file-ids)
support configuration schemas. Custom source
callbacks are available through `SourceCompiler`. See [reflection.capnp](../../crates/capnp-compiler/examples/reflection.capnp) for a tested
example with imports, generics, groups, methods and embeds. Use a session below
when lookup should compile additional source.

### Lazy sessions

`SchemaParser::parse_session()`, `FileCompiler::parse_session()` and
`SourceCompiler::parse_session()` return a
`SchemaSession`. It initially compiles the same declarations as `parse_schemas()`.
`schemas()` borrows the current validated snapshot; `load(id)` and
`get_nested(parent_id, name)` compile additional declarations and their dependency
closures. `find_nested()` returns `None` for an absent name. `get_all_nested()`
loads a scope's direct declarations together in source order, excluding aliases.
Unused children and siblings remain uncompiled, and the requested-file list stays
unchanged. Names are case-sensitive and are not dotted lookup paths.

```rust
let mut parser = capnp_compiler::SchemaParser::new();
parser.add_source("main.capnp", r#"
    @0xaaaaaaaaaaaaaaaa;
    using I = import "types.capnp";
    struct Root { value @0 :I.Used; }
"#)?;
parser.add_source("types.capnp", r#"
    @0xbbbbbbbbbbbbbbbb;
    struct Used {}
    struct Later { value @0 :Text; }
    using Alias = Later;
"#)?;
let mut session = parser.parse_session(&["main.capnp"])?;
let later = session.get_nested(0xbbbbbbbbbbbbbbbb, "Alias")?;
assert!(later.schema().field("value").is_ok());
let snapshot = session.into_schemas();
```

Alias lookup follows the declaration ID, including imported aliases, file scopes,
constants, annotations and generic types. Like C++ `ParsedSchema::findNested()`,
it discards generic bindings: `using TextBox = Box(Text)` returns the unbranded
`Box` declaration. Inspect a field's runtime type to retain its applied brand.
An alias of a generic parameter returns `None`; built-in/list aliases have no
loadable schema node and return an error. Invalid aliases report diagnostics.

Loading requires `&mut self`; a live schema handle or dynamic message view prevents
extension until its borrow ends. Each extension stages source discovery, resolution
and loader validation before committing. Errors leave the old snapshot and input
dependencies unchanged, including after an unsuccessful attempt to read new imports
or embeds. Missing names do not perform I/O. Alias resolution can discover imports.

Memory sessions borrow the parser's immutable virtual filesystem; custom sessions
borrow their provider. Disk sessions own
their captured absolute search paths and can outlive `FileCompiler`. Successfully
read schema/embed bytes stay cached across extensions; start a new session to observe
changes to them. Previously unused files are opened when needed, and inputs from a
failed extension can be corrected before retrying. `into_schemas()` detaches an owned
snapshot from any provider. Frontend graph/input and loader limits cover the whole
session; alias resolution and compilation each have bounded work per extension.
`parse_session_with_limits()` selects explicit runtime loader limits. Staging copies
the retained syntax graph and recompiles the accumulated selection; this is not an
incremental compiler performance claim.

### Concurrent parser caching

`SchemaParser::into_concurrent()` and `FileCompiler::into_concurrent()` move their
source configuration into a `ConcurrentSchemaParser`. Both have `_with_limits`
variants. `ConcurrentSchemaParser::from_provider()` owns a custom provider;
`from_provider_with_options()` additionally accepts the file-ID policy and loader
limits. The owned provider must be `Send + 'static`, but need not be `Sync`.
The borrowed `SourceCompiler` API keeps its existing non-Send provider support.

```rust
let mut parser = capnp_compiler::SchemaParser::new();
parser.add_source("main.capnp", "@0xaaaaaaaaaaaaaaaa; using D = import \"types.capnp\"; struct Root { value @0 :D.Used; }")?;
parser.add_source("types.capnp", "@0xbbbbbbbbbbbbbbbb; struct Used {} struct Later { label @0 :Text; }")?;
let cache = parser.into_concurrent(&["main.capnp"])?;
let other = cache.clone();
let handle = std::thread::spawn(move || other.get_nested(0xbbbbbbbbbbbbbbbb, "Later"))
    .join().unwrap()?;
let local = handle.schemas().materialize()?;
let schema = local.get(handle.id())?;
assert!(schema.schema().field("label").is_ok());
```

Clones share one worker and a queue of at most 16 waiting operations. Synchronous
`load`, `find_nested`, `get_nested` and `get_all_nested` serialize extensions of
one session. Concurrent duplicate lookups reuse successful reads and compilation;
successful alias resolutions are also cached. Unknown names and failed lookups
do not create cache entries. Missing imports and rejected extensions can retry.
`schemas()` returns the current immutable `CachedSchemas`; declaration lookups
return `CachedSchema` handles with an ID and the generation that compiled it.
`revision()` advances only after a successful extension. Positive alias lookup
can publish once even if its target was previously loaded.

Snapshots and handles are `Send + Sync`, clone cheaply, and outlive the cache.
`serialized_request()` exposes generator input; `requested_files()` exposes file
IDs, including generated ones, and `dependencies()` records disk inputs.
`materialize()` copies and validates a snapshot into local `ParsedSchemas` for
reflection and dynamic messages. Reuse that local snapshot for repeated access.
Runtime loaders and live dynamic views keep their existing thread-local ownership.

The worker owns bounded compiler state; retained snapshot generations consume
caller-owned storage. The requested entry-point set is fixed at construction.
There is no eviction, persistence or automatic source invalidation: create a
fresh cache to observe edits to successfully cached files or change entry points.
Unread dependencies are discovered on demand. Failed extensions preserve the
previous generation and dependencies; provider side effects are not rolled back.
Dropping the last cache clone closes the queue and joins the worker. Provider
panic stops the worker and waiting callers receive errors; old snapshots remain
usable. Reentry from a callback into the same cache is rejected. Callbacks must
not wait for other threads that are themselves waiting for that cache.

Calls and final cache destruction can block on provider I/O. Async applications
should use a blocking executor. This is shared transactional caching, not parallel
compilation or in-place mutation of retained runtime handles. The
[cache tests](../../crates/capnp-compiler/tests/cache.rs) and C++ session oracle cover these boundaries.

## Imports and aliases

- Relative imports resolve beside the importing file. Slash-prefixed imports
  search configured import roots in order; no system directories are implicit.
- Direct `import "types.capnp".Item`, `using Types = import "types.capnp"`,
  shorthand `using Types.Item`, nested aliases, re-exports, primitive/list aliases
  and aliases of the `List` constructor are supported. Constant aliases use
  lowercase names, for example `using limit = import "config.capnp".limit;`.
- Lexical lookup preserves the alias declaration's scope. `.Name` selects the
  current file's scope. Parenthesized type expressions are accepted.
- Cyclic file imports and recursive type references work. Alias cycles, missing
  imports, unknown members, duplicate declarations and schema ID collisions
  produce source diagnostics.
- Disk files use canonical paths for identity, including symlink aliases. A source
  is read/parsed once per compilation. Repeated import spellings retain separate
  request metadata entries pointing to the same file ID. Repeated requested file
  identities are deduplicated in input order.
- Relative `..` can leave a requested file's directory. An import loaded through
  an import root cannot use `..` to leave that root. Disk compilation is not a
  filesystem sandbox; use the in-memory parser to control available inputs.
- Imports appearing in types, `using` declarations or annotation names in requested
  files are loaded for request metadata. Imports used only in values are omitted from that
  table because their values are inlined. Other imports load on demand during
  type/value resolution: unused dependencies inside imported files are not opened.
  Request nodes include requested declarations, referenced types/annotations and enclosing
  nodes; imported constants used only for their values are not emitted.
  C++ also omits imports used only in an annotation declaration's type from the
  import table; those type dependencies are still loaded and emitted.

Explicit file IDs may appear anywhere at file scope. Generators use requested filenames for output paths;
choose the virtual namespace or source prefix appropriate to your generated tree.

## Optional file IDs

`SchemaParser`, `FileCompiler` and `SourceCompiler` require explicit file IDs by
default. `set_file_ids_required(false)` permits configuration schemas to omit
them, matching C++ `SchemaParser::setFileIdsRequired(false)`:

```rust
let mut parser = capnp_compiler::SchemaParser::new();
parser.set_file_ids_required(false);
parser.add_source("config.capnp", "struct Config { port @0 :UInt16 = 80; }")?;
let parsed = parser.parse_schemas(&["config.capnp"])?;
let config = parsed.get_file("config.capnp")?.get_nested("Config")?;
```

The setting applies to requested schemas and imports, including files discovered
later by a session. Each file without an ID gets an OS-random 64-bit value with
the high bit set. Child IDs use the existing deterministic Cap'n Proto algorithm;
explicit file/declaration IDs always win. Invalid or duplicate explicit IDs remain
errors. Sources and source ranges are not rewritten. Entropy failure returns a
diagnostic, and generated IDs still pass the normal collision checks.

Successful session inputs keep their generated IDs across lookups and extensions.
Aliases and cyclic imports share an ID through the existing source identity cache.
A failed extension preserves the prior snapshot; IDs assigned only to its
provisional inputs are discarded and may change on retry. Sessions capture the
setting at creation, so changing a disk/custom compiler's configuration affects
new sessions only. Starting a fresh parse generates new IDs and new derived type
identities. Use explicit IDs for persistent schemas and cross-process RPC types.

`compile(filename, text)` and the `capnp-compile` CLI remain strict; opt in through
the parser APIs above. This adds no persistent ID storage. Concurrent caches retain
generated IDs for their lifetime. The OS entropy provider is `getrandom` 0.4.3, already present
in the repository lockfiles.

## Language boundary and limits

Implemented in addition to imports/aliases:

- Structs, enums, nested declarations, explicit declaration IDs and deterministic
  child IDs matching C++ (MD5 is used only for the schema ID algorithm).
- Named/unnamed unions and nested groups, shared ordinal validation, storage
  reuse, discriminant placement and deterministic group IDs matching C++. Legacy
  named union ordinals (`choice @n! :union`) are supported. Groups are inline
  fields, not independently referenceable types.
- Forward/recursive references, all integer and floating-point primitives, Void,
  Bool, Text, Data, AnyPointer, AnyStruct, AnyList, Capability, interface pointers
  and lists of supported types.
- Scalar, enum, Text, Data, list and struct constants/defaults, including nested
  lists, inline struct lists, groups, unions and typed values assigned to
  AnyPointer. Constants work at file, struct or interface scope, with qualified references,
  forward references and aliases.
- Annotation declarations, explicit/derived IDs, all target flags, wildcard
  targets, typed applications, repeated applications in source order and
  annotations on annotation declarations. Imports and aliases work for annotation
  names and values, including the real `rust.capnp` and `c++.capnp` files.
- Generic structs/interfaces, inherited and bound brands, nested aliases,
  typed generic constants/defaults and annotations, and implicit method parameters.
  Keywords are contextual: `import` and `group` can name generic parameters.
  Bare `union` after a field colon still selects named-union syntax, as in C++.
- Interfaces, multiple inheritance, nested declarations, methods with
  inline or explicit struct signatures, defaults, annotations and streaming results.
  Method parameter/result IDs match C++ and survive method renaming.
- Checked integer ranges; decimal/hex/octal integers; decimal floating-point
  literals, `inf`, `-inf` and `nan`; conversions preserve declared precision.
  Leading-zero prefixes follow the C++ lexer: `07.85` and `01e1` are accepted,
  while `08.5` and `09e1` are rejected. Digits after the decimal point or exponent
  are unaffected. The shared text-value parser uses the same lexical rule.
- Byte-preserving quoted strings, adjacent concatenation, C-style simple escapes,
  two-digit hexadecimal escapes and one-to-three-digit octal escapes. Backtick
  lines append a normalized newline. UTF-8 BOMs and ASCII whitespace are accepted.
  Data also accepts hexadecimal byte-pair literals.
- `embed` values for Text, Data and serialized structs, including imported
  constants, annotations, branded structs, defaults and struct lists.
- Ordinal validation and wire layout in ordinal order, with original code order
  retained. Comments are accepted; adjacent operator tokens require whitespace.

Schema requests include documentation comments, node/member byte ranges and
per-file identifier references. Diagnostics retain separate byte spans and
one-based line/column positions. The C++ compiler-version field is omitted.

A [295-case grammar corpus](../../crates/capnp-compiler/tests/corpus/grammar.rs) checks contextual names,
generic parameters, declaration forms, delimiters and numeric literals. Portable
tests check acceptance and runtime loading; the pinned C++ test additionally
compares emitted metadata and values. This is bounded grammar coverage, not a
claim of complete language qualification. See the [test guide](../archive/Testing-History.md#schema-grammar-corpus).
An additional 108 lexical cases cover ASCII controls, DEL and Unicode separators
inside quoted strings, between tokens and between hexadecimal byte pairs.
A [66-expression numeric corpus](../../crates/capnp-compiler/tests/corpus/numbers.rs) checks both requested
schemas and unused declarations in loaded imports against pinned C++. Malformed
numbers fail during lexing, including incomplete exponents, unsupported radix
prefixes and multiple decimal points. The standalone text-value parser uses the
same validation. Well-formed values retain lazy type/range checking; unopened
imports remain lazy, and failed session discovery can retry corrected inputs.
Integer literals exceeding UInt64 remain checked errors when evaluated, unlike
the pinned C++ lexer's wrapping arithmetic.

Per-file limits: 4 MiB, 262,144 tokens, 64 levels of declaration/type syntax nesting
and 65,535 explicit ordinals per struct, enum or interface (a struct shares its ordinal
namespace with all its groups). Whole-graph limits: 256 schemas and 256 embedded
files, 16 MiB of combined source/embed input, 4,096
nodes (including inline method parameter/result structs), 16,384 aliases, 16,384 import edges and 8 MiB of expanded display names.
Type/alias/constant/annotation resolution and expanded generic types are
depth-limited to 64, with a shared budget of 1,000,000 expression/substitution
visits across import discovery. Shared generic bindings are checked before
expansion, so compact alias diamonds cannot produce unbounded emitted brands. Composite trees have a
separate expanded depth limit of 64 and 1,000,000 value visits, so cached constant
references cannot hide excessive expansion. Text/Data and embedded struct payloads
have a 16 MiB aggregate expansion limit. Encoding separately permits 16 MiB of value storage
and 1,000,000 steps, including list elements and group clears. Repeated references
and assignments count toward these bounds. These are implementation bounds, not
language guarantees. Layout has a separate shared
budget of 1,000,000 allocation/location-search steps. Allocation failure follows Rust's
normal allocator behavior. Expanded documentation text has a separate 16 MiB
output budget, including copies on both group nodes and their fields. The
library contains no unsafe code.

Nested union layouts rejected by the pinned C++ compiler for historical issue
#344 also return source diagnostics. This compatibility check is always enabled.
See [choices.capnp](../../crates/capnp-compiler/examples/choices.capnp) for ordinary groups, group alternatives
and nested named unions.

## Constants and values

See [constants.capnp](../../crates/capnp-compiler/examples/constants.capnp) for typed declarations and defaults:

```capnp
const limit :UInt32 = 64;
const signature :Data = 0x"00 ff 7e 80";
struct Settings {
  count @0 :UInt64 = .limit;
  signature @1 :Data = .signature;
}
```

Constants require qualified references (`.limit`, `Settings.maxCount` or
`import "config.capnp".limit`), including through aliases. They retain their
source type and precision: a Float32 constant widened to Float64 retains its
Float32 rounding. Integer conversions check the destination range; floats cannot
initialize integers. Floating-point overflow becomes infinity, matching C++.
Integer literals beyond UInt64, or negative literals below Int64, are rejected;
Rust deliberately rejects the oversized positive integers that pinned C++ wraps.

Data accepts string bytes or `0x"..."` byte pairs, with whitespace between
pairs, including vertical tabs as in C++. Use `""` for empty Data. Text and Data constant references are not implicitly
converted between types. Explicit empty defaults remain distinct from null ones.

Enum declarations and constants emit typed enum values. The pinned C++ compiler
exposes an enum constant reference as its numeric ordinal; this frontend matches
that behavior, including rejection when such a reference is used as an enum
default. Use the bare enumerant name for enum defaults.

See [composites.capnp](../../crates/capnp-compiler/examples/composites.capnp) for lists, structs, groups,
unions and pointer defaults. Unassigned fields read their schema defaults;
explicit empty lists/structs remain distinct from null pointers. Struct values
use `(field = value, ...)`; lists use `[value, ...]`. Trailing commas are accepted.
Assignments apply in source order, including repeated fields and union choices.
Group assignment resets the group's default alternative and non-union fields,
matching the reference compiler's handling of shared storage.

Struct and list references retain their declared types. C++ permits one level
of implicit struct wrapping when an unambiguous value matches its first field,
such as `const x :Box = 5;` for `struct Box { value @0 :UInt32; }`.
List and struct literals require a corresponding expected type; they cannot
infer a type from an AnyPointer destination. A typed struct/list constant can
initialize AnyPointer. References to AnyPointer constants return a diagnostic;
the pinned C++ compiler fails on those references too.

The maintained Rust generator emits AnyPointer constants and honors explicit
AnyPointer defaults. Readers borrow the generated default; builders copy it into
a null field before mutation. Existing non-null fields take precedence.

## Strings and embedded files

Quoted strings support `\a`, `\b`, `\f`, `\n`, `\r`, `\t`, `\v`, `\'`,
`\"`, `\\`, `\?`, exactly two hexadecimal digits after `\x`, and one to three
octal digits after `\`. Octal values wrap to one byte, matching the reference.
Adjacent string tokens concatenate. Backtick text is literal through the end of
its line and appends `\n`, independently of LF, CRLF or CR source line endings:

```capnp
const explanation :Text =
    `A literal line, including \ and " characters.
    `A second line.
    ;
const signature :Data = "\x00\xff\377";
const asset :Data = embed "assets/blob.bin";
const settings :Settings = embed "assets/settings.bin";
```

Text values preserve embedded NULs and arbitrary bytes, matching C++; callers
that need UTF-8 use the runtime reader's checked `to_str()`. Schema sources and
filenames must be UTF-8. Single-quoted strings and Unicode `\u`/`\U` escapes are
not Cap'n Proto string syntax.

Embeds resolve relative to the file declaring the expression, including inside
an imported constant. Slash-prefixed paths search explicit import roots. They
are loaded only when evaluated, so an unused imported constant does not require
its embedded file. Embeds do not add schema nodes or import-table entries.
Canonical/normalized file identities share cached bytes and parsed messages.

Text/Data embeds copy every byte, including empty files. A struct embed reads an
unpacked, segment-framed Cap'n Proto message; packed streams are not accepted.
The first message is used, allowing whole-word trailing data like C++. Missing
fields retain schema defaults, and unknown wire fields survive pointer copies.
An inline struct list copies only the storage that fits its schema's element
layout. Direct group embeds return a diagnostic; pinned C++ aborts on that form.
Embeds require an expected Text, Data or concrete struct type; AnyPointer and
list embeds must instead reference an appropriately typed constant.

Each embedded file is limited to 4 MiB. Struct decoding uses a 64-level nesting
limit and a traversal budget of 2,097,152 words; expanded size, encoding work and
aggregate payload limits still apply to repeated uses. Malformed messages,
cycles, capabilities and excessive expansion return diagnostics instead of
using C++'s unlimited embed reader options. Disk inputs must be regular files.

## Documentation and source ranges

[documentation.capnp](../../crates/capnp-compiler/examples/documentation.capnp) demonstrates the reference
compiler's comment attachment rules. Documentation follows the declaration it
describes: after `;`, after a block's opening `{`, or after its closing `}` when
there is no opening comment. At most one line ending may precede the comment;
a blank line breaks attachment. Consecutive comment lines strip `#` and at most
one space, preserving indentation and CR bytes. Each saved line ends in LF,
including a comment at end of file. Comments before the file ID are ordinary
comments; file documentation follows the ID statement.

`CodeGeneratorRequest.sourceInfo` associates comments and byte ranges with node
IDs. Member entries follow schema/ordinal order, independently of source order.
Group documentation appears on both the group node and its containing field.
Unnamed unions have no separate node. Parameter comments are ordinary comments;
parameter lists and individual parameters still receive byte ranges. Files and
implicit method result structs have zero ranges, matching C++.

Ranges count UTF-8 bytes in the declaring file, including imported files. They
start at the first declaration token and end after its terminator and attached
trailing documentation. A block with both opening and closing comments uses the
opening text but still includes the closing comment in its range. Node ranges
are available both on `Node` and `Node.SourceInfo`. Optional comments remain null
when absent. Source-info follows the same dependency selection as schema nodes.

The maintained runtime schema bindings expose these additive position fields.
Existing requests without them continue to read zero defaults. Both standard and
field-API Rust bindings now render source-info as Rustdoc: declarations, fields,
union/enum variants, constants, annotations and client/server methods retain their
comments after Rust renaming. Field projections and native values share the same
lookup. Missing source-info or missing member entries produce no schema comment;
duplicate node entries and invalid UTF-8 return generator errors.

File comments appear on a generated `schema_documentation` module so the output
works with `include!`; a numeric suffix avoids collisions with schema declarations.
Comments are emitted as escaped string attributes. Untagged fenced and indented
examples become `text` code blocks, while explicit Rust examples remain doctests.
CommonMark parsing handles nested lists, quotes and fence delimiters; bare HTTP(S)
URLs in prose become links. See the
[Rustdoc fixture](../../crates/capnp-compiler/examples/rustdoc.capnp) and
[generator guide](Rust-Generator.md#schema-documentation).

`RequestedFile.fileSourceInfo.identifiers` records the UTF-8 byte range and target
ID of resolved names, imports, aliases and qualified expressions in each requested
file. Entries are sorted by `(startByte, endByte, typeId)` and deduplicated; C++
reports them in compiler traversal order. Parenthesized expressions keep the inner
name's start, including for qualified names such as `(S).Nested`. Generic
applications report their constructor and arguments separately. Aliases refer to
the resolved declaration, and value-only imports can reference IDs whose nodes
are omitted from generated-code dependencies.

Built-ins use the pinned compiler's reserved IDs (for example, Text is 1026 and
List is 1028). Unbound parameters, literal enum values, assignments and synthetic
stream signatures do not create identifier entries. The pinned compiler emits
only the `typeId` variant; the bindings also expose its reserved `member` variant.
Empty requested files have an initialized empty identifier list. Import/embed
retries discard partial tables, and collection shares the existing resolution
work budget. Older requests without this metadata read as an empty table.

## Annotations

See [annotations.capnp](../../crates/capnp-compiler/examples/annotations.capnp) for declarations, targets,
scalar and composite arguments, and annotations on fields, groups and unions:

```capnp
annotation label(file, struct, field) :Text;
$label("example");
struct Message $label("record") {
  payload @0 :Data $label("wire bytes");
}
```

Annotation names resolve lexically and can use bare names, aliases, qualified
names or direct imports. Only Void annotations can omit their argument. Targets
are checked against the annotated declaration; duplicates and mixing `*` with
other targets are errors. An empty target list declares an annotation that cannot
be applied anywhere. Named group/union annotations attach to the containing
field; unnamed unions cannot carry annotations, matching C++.

Values use the same checked evaluator as constants/defaults. Repeated applications
retain their source order. Annotation dependency cycles, including cycles through
annotated constants, return source diagnostics. Applications carry an explicit
brand, including bindings inherited from enclosing generic declarations. Interface and
method applications use their respective target flags; both input and output
parameters use the `param` target, not `field`.

`$Rust.name`, `$Rust.option` and `$Rust.parentModule` work with the maintained Rust
generator. Generated reflection keeps schema names when Rust identifiers are
renamed, and uses those original names for its field lookup index.

## Generic types and brands

[generics.capnp](../../crates/capnp-compiler/examples/generics.capnp) exercises generic structs, groups, enums
in generic scopes, generic interfaces, inheritance and explicit nested signatures:

```capnp
struct Envelope(T) { value @0 :T; }
struct Settings { message @0 :Envelope(Text) = (value = "hello"); }
interface Echo(T) { echo @0 (value :T) -> (value :T); }
interface Service(T) extends(Echo(T)) {
  relay @0 [U] (value :U = null) -> (value :U);
}
```

Type arguments accept Text, Data, AnyPointer, lists, structs, interfaces and type
parameters. Scalars, enums, Capability, AnyStruct and AnyList are rejected as
generic arguments by the pinned compiler, even though the last three are pointer
types. Bare `Envelope` leaves its argument unbound; `Envelope(T)` explicitly binds
it. Nested references preserve enclosing bindings, such as `Outer(Text).Inner(Data)`.
Aliases retain their declaration's scope and can be instantiated through imports
without leaking bindings between uses. Reapplying an already bound generic fails.

Method `[U]` parameters appear in `implicitParameters`; inline parameter/result
structs declare their own corresponding parameters, with C++-matching scope IDs
and brands. The maintained generator currently exposes these implicit method
values through AnyPointer. The generated-code regression round-trips one alongside
ordinary statically typed generic RPC calls. Pointer parameters may explicitly
default to `null`; ordinary fields and scalar parameters cannot.

Lists whose direct element is an unbound parameter remain unsupported. Pinned C++
inconsistently accepts these for parameter indices two and above; Rust deliberately
rejects them regardless of position. `List(Envelope(T))` is supported. Annotation
applications retain their bindings, while request dependency selection follows
C++: a type referenced only in an annotation application's brand is not included
unless another dependency selects it. Request generation and runtime loading of
such annotation values therefore have distinct dependency requirements.

## Interfaces and methods

See [interfaces.capnp](../../crates/capnp-compiler/examples/interfaces.capnp) for inheritance, explicit
parameter/result structs, annotations, returned capabilities and streaming:

```capnp
interface Base {
  ping @0 (value :UInt32 = 42) -> (value :UInt32);
}
interface Service extends(Base) {
  echo @0 (text :Text) -> (text :Text);
  upload @1 (bytes :Data) -> stream;
}
```

Omitting `->` creates an empty result struct. `echo @0 Params -> Results;` uses
existing struct types. Inline fields receive positional ordinals and use the
interface's lexical scope. Their generated schemas have zero scope IDs; their
IDs depend on the interface ID, method ordinal and parameter/result role.
Inherited interfaces do not add their nested declarations to lexical lookup.

`-> stream` loads `/capnp/stream.capnp` through the configured import paths.
For the pinned standard schemas, add `-I vendor/capnproto/c++/src` to the CLI;
in memory, register that file and its `/capnp/c++.capnp` import under an explicit
virtual import root. The standard StreamResult ID is checked. No import directory
is implicit, and a replacement with a different ID produces a source diagnostic.

Request emission preserves duplicate bases and inheritance cycles as pinned C++
does. Cyclic requests fail runtime loader validation; the Rust generator now
returns an error for cycles, inheritance depth of 64 or greater, and expansion
beyond 65,536 inherited edges per generated interface. Branded diamond paths are
retained within those bounds. The frontend emits `List(Capability)` type metadata,
but the current Rust generator rejects that list type as `List(AnyPointer)`.

## Verification and next steps

The [qualification report](Compiler-Qualification.md) maps grammar
families to tests and records known deviations, the 22-file upstream corpus and
concurrent-cache guarantees. Full language equivalence remains unqualified.

The shared [22-case import-discovery corpus](../../crates/capnp-compiler/tests/corpus/discovery.rs) checks
declaration/dependency traversal, alias ordering, field/group ordinals, annotation
timing, inline RPC signatures, generic dependencies and deferred pointer defaults.
Its requests match pinned C++ without rewriting display names. Portable tests also
verify that concurrent-cache extensions retain the same names and old snapshots.

`cargo test --workspace` includes the crate's native tests/doctests and the root
[C++ differential and generated-code tests](../../tests/schema_compiler.rs).
Focused checks:

```sh
cargo test --locked -p capnp-compiler
cargo test --locked -p reproto --test schema_compiler
```

The second command needs the pinned C++ sources and CMake/C++ toolchain. Platform
CI runs native parser/import tests on Linux, macOS and Windows. Source archives,
LLVM reporting and the CLI's auditable dependency inventory include this crate.

The reference suite includes 212 union/group schemas (including five unmodified
C++ layout fixtures), 15 invalid schemas and 128 deterministic nested layouts:
97 requests match and 31 historical compatibility rejections agree. Generated
Rust bindings switch alternatives and serialize nested unions while preserving
fields outside the union. Imported group nodes are also compared with C++.

The constants suite adds 123 successful schemas and 63 shared rejections, plus
15 import graphs and eight rejections. It compares canonical constant bytes as
well as defaults. Generated Rust constants and imported Data defaults compile
and round-trip without needing bindings for imports used only as values.

The composites suite adds 69 accepted schemas and 22 shared rejections, plus
17 import graphs and five rejections. Native tests check decoded values, source
diagnostics and expansion bounds. A fifth generated Rust acceptance test reads,
mutates and serializes composite defaults, including AnyPointer, without
changing the original constants or another message's defaults.

The annotations suite adds 56 accepted schemas and 37 shared rejections, plus
22 import graphs and eight rejections. It compares canonical annotation bytes,
including composite arguments. A sixth generated Rust test checks renamed types,
fields, enum variants, groups/unions, optional getters, parent modules, reflection
and serialization, without generating bindings for the imported annotation file.

The interfaces suite adds 52 accepted schemas and 38 shared rejections, plus
12 import/streaming graphs and six rejections. Its seventh generated Rust
acceptance test executes inherited calls, explicit struct signatures, defaults,
pipelined capabilities and streaming uploads. Native tests check stable method
IDs, pointer/default tags, import roots, malformed syntax and resource bounds.
Separate generator regressions cover cyclic, deep and exponentially expanding
inheritance.

Generics add 56 accepted schemas and 28 shared rejections, 17 import graphs,
four unmodified standard schemas (`schema`, `rpc`, `rpc-twoparty`, `persistent`)
and the then-current repository schema corpus. The eighth generated Rust acceptance crate checks
generic defaults, group fields, serialization, inherited calls, implicit method
parameters and explicit signatures with distinct nested type arguments. The
parameter-index inconsistency for lists is tested separately as an intentional difference.

Strings add 65 accepted requests and 13 shared rejections; embeds add 30 accepted
requests and 16 shared rejections. The ninth generated acceptance crate checks
embedded constants/defaults, mutation, serialization and unknown-field preservation.
Documentation adds 52 accepted requests covering attachment, spacing, Unicode,
CRLF, EOF and parameter ranges. All earlier comparisons now check node positions
and canonical source-info too. Identifier references are compared after sorting
and deduplication; 60 expression cases and three multi-file graphs cover this
metadata specifically. The focused corpus totals 1,119 accepted requests
and 311 shared rejections, with 74 native tests, six doctests and 35 root integration
tests. Two accepted requests contain cycles rejected by both runtime loaders.

The runtime snapshot suite compares 15 parsed declarations with C++ `SchemaParser`,
including direct nested lookup, node/member documentation and canonical dynamic
wire output using imported generics, groups, unions and struct lists. Five native
regressions cover snapshot ownership, input tracking, dependency selection and
compile/loader failure isolation. A compile-fail doctest checks handle lifetimes.

Seven session regressions check lazy declarations/aliases, import and embed
discovery, generic binding erasure, cumulative loader limits, atomic batch failure,
cached source identity and retry after both compilation and loader rejection.
An additional compile-fail doctest rejects extension with a live handle. The C++
compiler-backed loader matches all selected nodes and source-info across 18 session
snapshots, including declaration aliases, methods, groups and imported types.

Pinned C++ preloads `StreamResult` from an older compiled schema with zero node
positions, while its source-info retains the parsed range. Rust emits that range
in both places; the comparison supplies the authoritative source-info range for
this known C++ node discrepancy.

The root and test-support build scripts use the frontend. Separately packaged
vendored crates still need a distributable frontend dependency before changing
their existing C++ build path.

The [language reference](https://capnproto.org/language.html) and pinned C++ define
compatibility. Passing the bounded corpus does not establish full language parity.
