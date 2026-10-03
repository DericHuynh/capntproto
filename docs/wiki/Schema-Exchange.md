# Capability-based schema transmission

`schema_exchange` implements the revision-pinned metadata exchange contract from
`CapnpSchemaExchange.tla`. The wire interface is `schema-exchange.capnp`'s
`Catalog.get(Key)`. A key contains a schema ID and an application-assigned,
nonzero immutable revision. This is an ordinary RPC capability; it adds no
`rpc.capnp` message variants and can travel over the native network.
Possession of the catalog capability grants read access. The application decides
which catalog capability to export and authenticates the transport.

The owner builds a `Catalog`, publishes `Definition`s, and exports it with
`Catalog::service(Rc<RefCell<Catalog>>)`. `Catalog::publish_request(revision,
request)` imports the nodes from a compiler `CodeGeneratorRequest` atomically.
Individual definitions can also be created using `Definition::from_node` or
`Definition::decode`. A batch either publishes entirely or leaves the catalog
unchanged. Existing keys accept only identical bytes. Publishing another
revision preserves all earlier definitions; missing definitions are explicit
responses rather than an automatic upgrade to the current revision.

The client calls `fetch(&client, root_key, limits, concurrency).await`. Retrieval
requests each required key once, permits replies to arrive in any order, and
supports shared and cyclic dependencies. Up to 64 calls can be outstanding.
Every response must match the requested ID and revision. Only a complete,
unrejected closure becomes a `Bundle`; failures expose no active partial graph.
Dropping the fetch future drops outstanding RPC calls and the unpublished graph.
The generated service permits cancellation, so suspended server calls can stop.
A caller can place a timeout around the whole fetch; timeout does not publish a
bundle or change the catalog.

Dependencies are extracted from schema bodies: field types and groups,
interface parameters/results and superclasses, generic brands and parameter
scopes, constants, annotation types/uses, and enclosing scopes. Every dependency
uses the root's pinned revision. Nested declaration indexes are preserved as
navigation metadata, but do not force retrieval of unrelated declarations: the
compiler can omit unused declarations that those indexes name. An application
can fetch such a declaration separately by its explicit key.

Definitions contain exactly one unpacked message rooted at `schema.Node`.
Validation checks framing, ID correspondence, known traversed discriminants,
UTF-8 metadata names, required dependency IDs, traversal bounds and absence of
live capability pointers. Defaults are 4,096 nodes, 16 MiB of retained encoded
node bytes, 1 MiB per node and nesting depth 64. Limits cover the metadata graph;
the RPC connection still has its own framing and buffering limits. The catalog
retains revisions for its lifetime and rejects further publication when full.
Individual decoded message arenas are transient and separately bounded.

`Bundle::get(key).read()` returns an owning message reader whose `schema.Node`
readers borrow its arena. `Bundle::load(capnp::schema_loader::Limits::default())`
now constructs a validated runtime loader from the complete revision closure.
The loader supplies borrowed reflection, dynamic message editing and capability
calls/servers; see [Schema Loader](Schema-Loader.md). Loader failures publish
no partial schema registry. Catalog revision identities remain scoped to that
capability; the client does not merge untrusted catalogs into a global cache or
claim content-hash authentication. The retrieval protocol still rejects unknown
traversed type discriminants because it cannot infer their dependencies.

## Verification

`RpcSchemaRetrieval.tla` is a bounded implementation-oriented companion to the
original exchange contract. Eight configurations cover three schema IDs, two
revisions, normal/shared/cyclic/missing dependencies, reply corruption,
publication races and cancellation. Their graphs contain 172 states and 292
edges in total; these are separate configurations, not a composed state count.
Every edge is replayed through a shortest prefix over real Cap'n Proto RPC into
the production `Retrieval`. Two mutations reject wrong revision acceptance and
activation without dependency closure. The script also reruns all 12 original
schema configurations, including safety/progress checks, mutations and witnesses.

Rust tests additionally import a real compiler request with generics, methods,
groups and annotations; test atomic publication and limits, reject malformed or
capability-bearing schema payloads, cancel outstanding calls, and fetch cyclic
metadata over authenticated Native UDP. These are bounded protocol checks, not a
proof of full schema validation, arbitrary executor schedules or equivalence to
the C++ loader.

Run `cargo test --test protocol_models schema_exchange_model -- --exact` for focused verification or
`cargo test --locked --workspace --all-targets` for the canonical runtime checks.
