**A proposed Rust API for Cap’n Proto, cross-referenced against recapn**

Design specification, 19 September 2026. This is the design reference, not a current implementation checklist. The [generator guide](RUST_GENERATOR.md) documents the implemented reader/editor, ownership and generated-call contracts and their Cargo verification; the [roadmap](ROADMAP.md) tracks optional design work. Examples here use the illustrative crate name `wire` and are not all literal examples of the shipped API.

Revised to use selective typestate: named reader/editor aliases over a sealed representation family, consuming lifecycle transitions, checked field entries, and staged replacement states. The performance review retains those states, adds lazy inspection paths, and separates replacement guarantees from their implementation: compatible storage can be reused when all fallible work precedes mutation. Performance expectations below are design hypotheses, not benchmark results.

The central rule is: **readers return values; editors return operations on storage.** Both use the schema’s field names. Reads preserve decoding errors. Writes disclose allocation, copying, replacement, and movement through a small shared vocabulary.

The design targets the standard Cap’n Proto wire format. Better generated code does not, by itself, make the current arena reclaim discarded objects or make wire lists grow like Rust vectors.

The inspected source snapshots are [recapn 296226d](https://github.com/cloudflare/recapn/tree/296226d8594796f7830d7899f1683d41486b54a9) and [capnproto-rust 81bc1b8, capnp 0.27.2](https://github.com/capnproto/capnproto-rust/tree/81bc1b815d0f450c9114f9cc2e2274182d210df2). Source links below are pinned to those snapshots.

Recapn’s actual generator emits the same field name on its reader and builder, and additional consuming `into_field` accessors for pointer fields and groups. Its field module’s introductory discussion of `foo_mut` is not the naming emitted by the inspected generator. The generated source and runtime implementations, rather than that introductory comment, establish the comparison. [Generator](https://github.com/cloudflare/recapn/blob/296226d8594796f7830d7899f1683d41486b54a9/recapnc/src/generator.rs), [generated accessor templates](https://github.com/cloudflare/recapn/blob/296226d8594796f7830d7899f1683d41486b54a9/recapnc/src/quotes.rs)

| Area | Recapn implementation | Decision for this API |
| --- | --- | --- |
| Generated struct family | `Person<Representation>` with reader/builder aliases | Retain a sealed representation family, expose `Person`, `PersonRef`, and `PersonMut` aliases, and specialize methods by state |
| Mutable fields | One accessor returns a typed operation handle | Adopt this for all mutable fields, including scalars |
| Immutable pointer fields | Handles expose `get`, `get_option`, `try_get`, `try_get_option`, and raw inspection | Ordinary getters return the useful value through `Result`; advanced inspection uses a typed descriptor |
| Invalid pointers | Several default getters substitute defaults or absence | Return errors; recovery is explicit application code |
| Text | `as_str()` validates UTF-8, but uses a getter that can suppress pointer errors; checked pointer access is also available | `name()` validates the pointer, terminator, and UTF-8 in one error path |
| Text input | Supports both wire-text readers and `set_str`/`try_set_str` | Accept ordinary `&str` through `copy_from`; append the wire terminator internally |
| Mutable struct access | `get()` can materialize defaults and replace erroneous values | `edit()` requires existing sufficient storage; `ensure()` explicitly permits materialization or enlargement; neither repairs malformed data |
| Initialization | `init` initializes through the pointer; several convenience variants panic on invalid sizes | `init` requires a null field and checks sizes; `replace` explicitly overwrites |
| Lists | `at` panics, `try_at` checks bounds; default iteration uses `InfalliblePtrs`, with `Fallible` available | `get` follows slice conventions; default iteration propagates pointer and text errors |
| Copies | Some `set` convenience methods use `IgnoreErrors` | `copy_from` is strict; the destination’s logical value is unchanged on an error |
| Orphans | Already supports orphanage, disowning, and adoption; low-level `try_adopt` returns the orphan on failure | Retain the mechanism; surface fallible `take` and `adopt` with ownership-preserving errors |
| Lifetimes | Short-borrow accessors and consuming `into_field` accessors | Retain both; make short borrowing the ordinary path |

These implementation observations come from recapn’s [fields](https://github.com/cloudflare/recapn/blob/296226d8594796f7830d7899f1683d41486b54a9/recapn/src/field.rs), [lists](https://github.com/cloudflare/recapn/blob/296226d8594796f7830d7899f1683d41486b54a9/recapn/src/list.rs), [text](https://github.com/cloudflare/recapn/blob/296226d8594796f7830d7899f1683d41486b54a9/recapn/src/text.rs), [orphans](https://github.com/cloudflare/recapn/blob/296226d8594796f7830d7899f1683d41486b54a9/recapn/src/orphan.rs), and [pointer runtime](https://github.com/cloudflare/recapn/blob/296226d8594796f7830d7899f1683d41486b54a9/recapn/src/ptr.rs).

The example schema has `Person.id: UInt64`, `name: Text`, `address: Address`, `previousAddress: Address`, `phones: List(PhoneNumber)`, and `payload: Data`. `Address` has `city: Text`; `PhoneNumber` has `number: Text` and `kind: PhoneKind`. A named `employment` union contains unemployed, employer text, school text, and self-employed arms.

**The ordinary workflow has two views of one message.**

```rust
let mut message = Message::<Person>::new()?;

{
    let mut person = message.edit();
    person.id().set(42);
    person.name().copy_from("Deric")?;

    let mut address = person.address().ensure()?;
    address.city().copy_from("Edmonton")?;

    person.phones().init_with(2, |index, mut phone| {
        phone.number().copy_from(["555-0100", "555-0101"][index])?;
        phone.kind().set(PhoneKind::Mobile);
        Ok(())
    })?;

    person.employment().school().copy_from("NAIT")?;
}

let person = message.read();
println!("{}: {}", person.id(), person.name()?);

for phone in person.phones()? {
    println!("{}", phone.number()?);
}
```

The shorter read and write forms are deliberate. `PersonRef::name` returns a decoded borrowed string. `PersonMut::name` returns a text field operation handle. An editor reads through `person.read()`, which returns a short immutable view; mutable field handles do not need their own parallel family of defaulting read methods.

`Message::new()` allocates and establishes a correctly sized typed root before returning. Its `read()` and `edit()` methods can therefore return root views directly. This is a proposed runtime invariant, not a claim about existing capn-rs constructors. Nested reads remain fallible.

| Type | Meaning |
| --- | --- |
| `Person` | Schema marker, including typed field descriptors |
| `PersonRef<'a>` | Immutable borrowed view of encoded storage and its read context |
| `PersonMut<'a>` | Exclusive borrowed editor; never `Clone` |
| `Message<Person>` | Owns mutable encoded storage with an established typed root |
| `MessageView<'a, Person>` | Borrows immutable input whose framing and root have been checked |
| `FrozenMessage<Person>` | Owns immutable encoded storage |
| `PersonValue` | Optional native Rust data representation, generated separately |

The table shows default storage/capability configurations. Custom allocators, segment providers, and capability contexts remain supported through additional inferred generic parameters or associated types. The design does not require type-erasing those components or imposing `Send` on local RPC capabilities.

Public generic programming uses a schema trait with associated reader/editor families. This already has precedent in capn-rs’s GAT-based `Owned` trait; the naming change is that `Person` clearly denotes a schema rather than suggesting an owned value. [Current traits](https://github.com/capnproto/capnproto-rust/blob/81bc1b815d0f450c9114f9cc2e2274182d210df2/capnp/src/traits.rs)

**Generated signatures carry the borrowing rules.**

```rust
impl<'a> PersonRef<'a> {
    pub fn id(&self) -> u64;
    pub fn name(&self) -> Result<&'a str>;
    pub fn address(&self) -> Result<AddressRef<'a>>;
    pub fn phones(&self) -> Result<ListRef<'a, PhoneNumber>>;
    pub fn payload(&self) -> Result<&'a [u8]>;
    pub fn employment(&self) -> Result<EmploymentRef<'a>>;
    pub fn employment_tag(&self) -> EmploymentTag;
}

impl<'a> PersonMut<'a> {
    pub fn read(&self) -> PersonRef<'_>;

    pub fn id(&mut self) -> ScalarField<'_, u64>;
    pub fn name(&mut self) -> TextField<'_>;
    pub fn address(&mut self) -> StructField<'_, Address>;
    pub fn phones(&mut self) -> ListField<'_, PhoneNumber>;
    pub fn payload(&mut self) -> DataField<'_>;
    pub fn employment(&mut self) -> EmploymentMut<'_>;

    // Advanced lifetime transfer; ordinary code uses address().
    pub fn into_address(self) -> StructField<'a, Address>;
}
```

These are declaration sketches, not complete Rust items. A field handle carries a short exclusive borrow and identifies the field, using a descriptor or equivalent compile-time information. Merely obtaining the handle performs no allocation or mutation. Tiny generated accessors and operation shims should allow the compiler to propagate known field offsets; the ordinary path requires no boxing, dynamic dispatch, or runtime field-name lookup. Operations that return child editors consume the handle so their lifetime comes from the parent borrow rather than the temporary handle object:

```rust
impl<'a, T: Schema> StructField<'a, T> {
    pub fn edit(self) -> Result<T::Mut<'a>>;
    pub fn ensure(self) -> Result<T::Mut<'a>>;
    pub fn init(self) -> Result<T::Mut<'a>>;
    pub fn replace(self) -> Result<T::Mut<'a>>;
}
```

The runtime must continue to exclude parent mutation while a child editor or an immutable view derived from that editor is live. It must not fabricate independent `&mut` references to bit-packed fields or overlapping union storage. Standard Rust references to scalars are generally unsuitable because wire defaults, endianness, and bit packing may require access logic.

**A small operation vocabulary exposes costs.**

| Operation | Meaning | Storage contract |
| --- | --- | --- |
| `set(value)` | Store a scalar or enum value in an established location | No allocation; apply wire defaults and encoding |
| `copy_from(value)` | Replace a pointer field with a strict copy | May allocate; never silently truncate or suppress source errors |
| `edit()` | Access an existing writable object | No allocation or relocation; error on null, read-only storage, or insufficient layout |
| `ensure()` | Obtain an editable effective value | May materialize the schema field default or enlarge an older object while preserving contents |
| `init(...)` | Initialize a null field | Error if non-null; allocate fresh storage initialized to type defaults |
| `replace(...)` | Explicitly discard the old value and initialize fresh storage | May allocate; does not promise arena reclamation |
| `clear()` | Store a null pointer | Subsequent reads observe the schema field default |
| `take(&orphanage)` | Detach an existing object | No payload copy; return absence separately |
| `adopt(orphan)` | Attach a detached object in the same message | No payload copy; may allocate pointer metadata |

Not every operation exists on every field kind. Scalar fields have `set`; pointer fields have copy/reset/movement operations. Lists and data add a length argument to initialization. Text normally uses `copy_from(&str)`; a raw uninitialized text buffer is not part of the ordinary API.

`init` differs intentionally from the existing libraries’ overwrite-oriented initialization. Existing-code migration must choose `replace` where overwriting was intentional. A null field with a nonempty schema default illustrates the distinction: `ensure` materializes that field default, while `init` allocates a fresh value initialized according to its type.

`edit` is the explicit no-allocation path. If an older struct lacks space required by the generated editor, it returns `NeedsUpgrade`. `ensure` performs the enlargement. This preserves infallible scalar setters once an editor has been acquired.

`copy_from` specifies copy semantics and its error guarantee, not a mandatory allocation strategy. The runtime should support a fast path for compatible existing storage, initially same-length text from `&str` and data from `&[u8]`: finish all fallible checks first, then perform the non-fallible copy and, for text, maintain the terminator. Correct handling of any possible source/destination overlap is required. Other layouts or fallible sources may require detached storage. Callers requiring a strict no-allocation contract use `edit`; arbitrary `copy_from` calls do not promise reuse. Native source text and data are copied into message-owned storage; accepting `&str` does not create an arbitrary pointer into that string.

**Errors, absence, and defaults are separate.**

Ordinary pointer reads return the field default when the pointer is null, and an error when a non-null pointer is invalid. `Text` reads combine pointer, terminator, and UTF-8 checks. No error is converted to an empty string or an empty list.

For an explicitly optional pointer field, the generated getter returns `Result<Option<T>>`. Existing `$Rust.option` semantics can be retained. Otherwise, a typed descriptor gives advanced callers wire-presence access without generating several additional methods for every field:

```rust
let effective: &str = person.name()?;
let stored: Option<&str> = person.field(Person::NAME).present()?;
let is_null: bool = person.field(Person::NAME).is_null();
```

`present()` does not substitute the default; a malformed non-null value is still an error. `is_null()` describes pointer presence, not semantic validation. A scalar field generally cannot reveal whether its default was explicitly written.

The direct string getter is intentionally convenient, but producing `&str` requires UTF-8 validation. Advanced code that only needs the encoded length or bytes can defer that scan:

```rust
let text: WireText<'_> = person.field(Person::NAME).wire_text()?;
let byte_count = text.len();       // Excludes the wire terminator.
let bytes: &[u8] = text.as_bytes(); // May contain invalid UTF-8.
let name: &str = text.to_str()?;    // Establishes the UTF-8 proof.
```

`wire_text()` checks pointer shape, bounds, terminator, and applicable reader limits; it does not suppress errors. A null pointer uses the field default, matching the ordinary getter. `to_str()` validates UTF-8 and borrows the same payload. Retain the resulting `&str` when reusing it; repeated calls are not promised to cache validation. This advanced descriptor operation preserves lazy access without adding a second getter family for every generated text field.

Errors retain structured causes such as `InvalidPointer`, `InvalidUtf8`, `LimitExceeded`, `AlreadyPresent`, `NotPresent`, `NeedsUpgrade`, `LengthOverflow`, `WouldTruncate`, `ReadOnly`, and `DifferentMessage`. Generated field descriptors can attach field names and ordinals. Rich full-path diagnostics should be optional so ordinary successful reads need not allocate path strings or carry large path objects.

No `get_unchecked` is emitted as a convenience. An advanced raw layer can expose explicitly unsafe operations under separately documented invariants. Recovery remains explicit application policy, such as inspecting an error and choosing a fallback.

**Lists preserve the same read/write distinction.**

`len()` and indices use `usize` in the Rust API. Every conversion to an encoded count checks the actual wire limit; no unchecked `as u32` narrowing is used.

| Encoded list | Iterator item | `get(index)` |
| --- | --- | --- |
| `List(UInt32)` | `u32` | `Option<u32>` |
| `List(Person)` | `PersonRef<'a>` | `Option<PersonRef<'a>>` |
| `List(Text)` | `Result<&'a str>` | `Option<Result<&'a str>>` |
| `List(Data)` | `Result<&'a [u8]>` | `Option<Result<&'a [u8]>>` |
| `List(List(UInt32))` | `Result<ListRef<'a, u32>>` | `Option<Result<ListRef<'a, u32>>>` |
| `List(PhoneKind)` | `PhoneKind`, including unknown numeric values | `Option<PhoneKind>` |

For text lists, `names.get(i).transpose()?` obtains `Option<&str>` with standard Rust tools. Struct-list iteration itself yields inline struct handles; their pointer-field reads can fail later. Checked pointer acquisition and read budgets still apply to obtaining the list.

`ListMut::get_mut()` returns an optional short-lived editor: a struct editor for inline structs, a scalar field handle for primitives, and a pointer field handle for pointer elements. It never implicitly resizes the list.

```rust
let mut phones = person.phones().init(2)?;
if let Some(mut phone) = phones.get_mut(0) {
    phone.number().copy_from("555-0100")?;
}
```

`init_with(count, callback)` and `replace_with(count, callback)` are construction helpers. They allocate a detached destination list, invoke the callback once per index with an exclusive element editor, and publish the field only after the callback completes successfully. Length is known in advance, so the list does not repeatedly grow. A callback cannot retain element editors beyond their invocation. The initial API fixes the callback result to the library’s `Result<()>`, avoiding ambiguous error-type inference; domain-specific errors are mapped explicitly at the callback boundary.

For editing existing contents, `try_for_each_mut(callback)` lends one element at a time. It is an ordinary sequential edit: earlier successful iterations remain changed if a later iteration errors. Its name and documentation must not suggest whole-list rollback. A conventional `Iterator<Item = PersonMut<'a>>` is deferred until a runtime representation can justify simultaneously retained mutable element handles.

Wire lists remain fixed-length. Optional growable staging collections are native Rust containers with an explicit copy/build step. Repeated whole-list replacement is not presented as an efficient `push()` operation.

For primitive lists, `as_slice() -> Option<&[T]>` is available only when the actual physical encoding, alignment, and target endianness allow a native slice. It never copies to manufacture a successful result. Struct lists are not slices of Rust structs; bit lists are not slices of Rust `bool`.

Copying into an inline struct-list element checks its actual allocated data and pointer sizes. The ordinary API returns `WouldTruncate` rather than dropping newer fields. Replacing a whole list can allocate an adequate stride before copying. This addresses the limitation documented by capn-rs’s existing `set_with_caveats`. [Struct lists](https://github.com/capnproto/capnproto-rust/blob/81bc1b815d0f450c9114f9cc2e2274182d210df2/capnp/src/struct_list.rs)

**Unions use Rust matching for reads and variant operations for writes.**

```rust
pub enum EmploymentRef<'a> {
    Unemployed,
    Employer(&'a str),
    School(&'a str),
    SelfEmployed,
    Unknown(u16),
}

match person.employment()? {
    EmploymentRef::School(name) => println!("Studies at {name}"),
    EmploymentRef::Unknown(tag) => record_future_variant(tag),
    _ => {}
}

person_mut.employment().school().copy_from("NAIT")?;
person_mut.employment().unemployed().set();
```

Readers resolve and validate only the selected known arm. Unknown tags are not malformed pointers. An enum with no payload similarly has `Unknown(u16)`, preserving numeric values through reads and writes.

`employment_tag() -> EmploymentTag` reads just the discriminant, returning the corresponding payload-free tag or `Unknown(u16)`. It does not follow or validate the selected payload. This matters when routing or filtering by tag: `employment()` must decode a selected text payload even if the caller later ignores that string in a wildcard match. A tag-only read is not proof that the payload is valid.

The union editor returns variant operation handles. Obtaining `school()` does not change the discriminant; committing `copy_from` changes the tag and payload together. Payload-free `set()` can be infallible because it does not need allocation. Complex variant construction uses a staged replacement helper; acquiring a mutable view of an existing arm is strict and never switches arms as a side effect.

`Unknown(u16)` is informational and cannot reconstruct an unknown union payload. Forward the whole containing encoded object to preserve unknown fields and union data. A `PersonValue` conversion must state its loss policy; a tag-only conversion must never claim lossless round-trip support.

Known union writes clear the schema-known overlapping storage required for correct known-arm behavior. An older schema cannot identify every byte belonging to future union fields. The API must not claim that selecting a known arm securely erases all unknown data. Explicit sanitization/reconstruction requires a defined schema policy.

**Orphans use recapn’s separated lifetime mechanism.**

Recapn’s `Builder::into_parts()` produces a root and an orphanage, and `disown_into` ties the detached object to the orphanage lifetime. This solves the practical problem of a detached value keeping the source field mutably borrowed. Its low-level adoption also checks message identity. [Message parts](https://github.com/cloudflare/recapn/blob/296226d8594796f7830d7899f1683d41486b54a9/recapn/src/message.rs), [pointer adoption](https://github.com/cloudflare/recapn/blob/296226d8594796f7830d7899f1683d41486b54a9/recapn/src/ptr.rs)

The proposed public form is:

```rust
let (mut person, orphanage) = message.edit_with_orphans();

if let Some(address) = person.address().take(&orphanage)? {
    person.previous_address().adopt(address)?;
}
```

The orphanage and root borrow the same live message but authorize different operations. The orphan owns the detached object logically and is not clonable. It cannot be serialized as a standalone message without a copy or a suitable root ownership transfer.

`take` and `adopt` check arena identity. Merely giving two values equal lifetime parameters does not establish that they belong to the same message. `AdoptError<Orphan<T>>` returns the orphan to the caller; an unsuccessful adoption leaves the destination unchanged. Same-message movement may still need far-pointer landing-pad allocation. It is zero payload copy, not necessarily zero allocation.

Cross-message `adopt` returns an error. `copy_from` is the explicit cross-message operation. Capability-bearing objects additionally require compatible/importable capability contexts; copying bytes alone does not transfer capability semantics.

**Each fallible replacement has a bounded error guarantee.**

For `copy_from`, staged union writes, `init_with`, `replace_with`, and adoption: finish all fallible preparation before changing the destination’s reachable logical value. An `Err` leaves that value unchanged. This is a new runtime requirement; recapn’s low-level `try_adopt` clears its destination before all later fallible work, so wrapping it cannot supply the stronger guarantee.

This contract does not mandate staging for every replacement. For a valid native string or byte slice and compatible existing storage, preflight checks followed by a non-fallible in-place copy can supply the same guarantee. A recursive reader copy, construction callback, or I/O source can fail after producing part of its output, so it generally builds a detached candidate directly in final arena storage and then publishes it. Do not impose an intermediate native payload buffer or a separate full validation pass followed by another checked traversal. Union commits must also prepare the tag/payload transition and any required cleanup before publication.

This guarantee is not a transaction across several method calls. It does not undo callback side effects, restore traversal budgets, or roll arena allocation counters back. Failed staging may consume arena space until the message is dropped or compacted. Panic unwinding must maintain memory safety, but the API does not equate panics with ordinary returned errors.

Imported untrusted bytes are immutable by default. Creating an editor over imported storage requires an explicit checked copy or a dedicated validated mutable-storage API; an `&mut [u8]` alone is not proof of a well-formed, non-aliasing Cap’n Proto object tree.

**Zero-copy must remain a precise promise.**

```rust
let message = MessageView::<Person>::from_unpacked(bytes, limits)?;
let person = message.read();
let name: &str = person.name()?;
```

`from_unpacked` borrows payload bytes, checks framing, alignment policy, and root shape, and retains a read context for lazy nested validation. It does not authenticate the producer’s intended schema or deeply validate the message. An exact-message constructor rejects trailing data; a separate prefix parser returns the remainder. Packed input requires unpacking into storage before ordinary wire views are available.

No constructor named as a borrowed operation silently copies misaligned input. It returns a clear alignment error or uses an explicitly selected unaligned reader implementation. Segment metadata may allocate even when payloads are borrowed. Existing capn-rs already distinguishes borrowed flat-slice reads and alignment requirements. [Serialization runtime](https://github.com/capnproto/capnproto-rust/blob/81bc1b815d0f450c9114f9cc2e2274182d210df2/capnp/src/serialize.rs)

Reading a scalar loads its value; reading text validates UTF-8 and lends `&str`; reading a struct lends a view. Those operations avoid constructing a separate owned object graph. Arbitrary text/data inputs still need copying when inserted into message-owned storage.

Direct data construction can avoid an intermediate payload buffer:

```rust
let mut person = message.edit();
let payload: &mut [u8] = person.payload().init(byte_count)?;
source.read_exact(payload)?;
```

This initializes a zeroed final-sized data field and lends its bytes. If the I/O operation fails, the field remains partially filled: the I/O call is outside the initialization operation’s error guarantee. A staged closure helper can offer all-or-nothing publication when needed.

`freeze(self)` removes mutation access without compaction or deep validation. `compact_copy()` explicitly copies reachable data into fresh storage. `to_vec()` explicitly flattens framing and segments into a new buffer; `write_unpacked` writes existing segments with framing without first requiring such a flattened buffer. Neither wording promises that an OS or network stack makes no copies.

Immutable sharing is optional and respects storage and capability `Send`/`Sync` bounds. Retaining a tiny subview through a shared owner retains that owner’s message storage. A read-only mmap view also requires the backing bytes to remain stable; read-only mapping permissions alone do not prevent another process from changing a file.

**Optional features stay outside the minimal field vocabulary.**

- Borrowed projections such as `PersonView { id, name, phones }` assemble scalars and handles once and keep text borrowed. They are not Rust-layout casts of wire objects, and nested handles can remain fallible.
- A native `PersonValue` can support conventional derives and Serde. Conversion is explicitly fallible, allocating, and subject to an unknown-field policy. It is not generated as the default zero-copy API.
- Logical comparison should be an explicitly fallible operation on wire views, with a documented unknown-field policy. Do not derive bytewise equality and call it semantic equality.
- Generated Rustdoc describes schema defaults, ordinals, copy/allocation behavior, bounds, and presence semantics. Runtime helpers own shared documentation; field methods carry the field-specific information.
- RPC request editors use the same field API. A pending call can be awaitable while retaining a separate pipeline handle; a response owns its storage and lends result views. Do not force an await before accessing pipelined capabilities or promise that local RPC capability handles are `Send`.
- Custom arenas and scratch allocation remain first-class. Ordinary allocation-capable methods return the library’s error type for representable allocator failures; like other Rust code, they cannot promise recovery from a process-aborting global OOM policy.

Whole-message infallible validated views are deferred. They need a schema-scoped proof, stable storage, handling of `AnyPointer` and capabilities, cycle/amplification limits, and a different contract for repeated traversal. Existing capn-rs readers can charge repeated accesses against a shared traversal budget; successful previous access is not sufficient proof that a future accessor cannot fail. [Reader options](https://github.com/capnproto/capnproto-rust/blob/81bc1b815d0f450c9114f9cc2e2274182d210df2/capnp/src/message.rs)

**Implementation should proceed in three independently reviewable increments.**

1. Generate the read/editor facade over sealed representation states, direct borrowed text access, field-operation wrappers, Rust-style bounds access, open enums, flattened union reads, consuming accessors, and typed descriptors. Add mutable/frozen owner states and typed vacant/occupied entries. Implement shared behavior in a small runtime layer instead of regenerating every operation per field. Benchmark before describing wrappers as zero-overhead.
2. Implement runtime contracts: strict no-allocation editing, checked sizes, lossless struct-list copying, draft/ready staged replacements, and fallible orphan transfer with arena identity and ownership-preserving errors. These cannot all be obtained by renaming existing convenience methods. A scoped branded editing API can supplement the checked unbranded API.
3. Add optional native values, borrowed projections, diagnostics, scratch allocators, and RPC conveniences. Keep the default generated surface stable while these capabilities grow.

Acceptance checks should exercise meaningful failure modes: compile-fail cases for overlapping borrows and escaping callback editors; malformed pointers and UTF-8; old/new schema struct-list copies; repeated traversal limits; incorrect-arena orphan operations; allocation failure during adoption; unknown union forwarding; and failed staged copies preserving destination values. A successful API review is not a substitute for those runtime checks.

**Typestate records established facts at the scope where they remain true.**

The pattern is particularly useful for this API because readers, editors, field slots, and staged replacements already expose different capabilities. The generated family should encode those differences while keeping common signatures short. It should not encode every field’s presence in the type of its entire parent message.

Typestate does not establish a fact by naming a marker. A trusted constructor or successful checked transition establishes the fact; the resulting type preserves it while its lifetime and ownership rules prevent invalidation. Borrowing, generative identity brands, and typestate serve complementary roles rather than replacing one another.

| Scope | State or type | What it proves | What it does not prove |
| --- | --- | --- | --- |
| View | `PersonRef` / `PersonMut` | Permitted access and a live backing context | Deep validity of all descendants |
| Owner | `Message<T, Mutable>` / `Message<T, Frozen>` | Whether this owner exposes mutation | Complete validation, compaction, or `Send`/`Sync` |
| Field entry | `VacantField` / `OccupiedField` | Null/non-null status under an exclusive borrow; occupied pointer acquisition is checked | Allocator success or enough room for an upgraded schema |
| Replacement | `Pending<T, Draft>` / `Pending<T, Ready>` | Whether the staged value satisfies its publication preconditions | That final metadata allocation cannot fail |
| Union arm | `ArmMut<School>` | Which already-selected arm an exclusive editor accesses | Validity of every nested pointer field |
| Text | `&str` | The borrowed span is valid UTF-8 | Nonempty text, a valid email address, or application-specific rules |
| Arena identity | Fresh invariant brand | Handles belong to one scoped message session | Allocation success or universal cross-session interchangeability |

**Use recapn’s representation family behind familiar aliases.**

This revises the earlier preference for separate public wrapper implementations. Recapn’s family shape is useful; the clean interface is achieved through aliases and a small set of sealed modes:

```rust
// Representation declarations are schematic; mode implementations are sealed.
pub struct Person<M = mode::Schema>(M);

pub type PersonRef<'a> = Person<mode::Read<'a>>;
pub type PersonMut<'a> = Person<mode::Write<'a>>;

impl<'a> Person<mode::Read<'a>> {
    pub fn name(&self) -> Result<&'a str>;
}

impl<'a> Person<mode::Write<'a>> {
    pub fn name(&mut self) -> TextField<'_>;
    pub fn read(&self) -> PersonRef<'_>;
}
```

`Person` by itself remains the schema marker through its default mode. The read representation carries immutable access; the write representation carries exclusive access and is not clonable. A wrapper may implement `Clone` conditionally when its representation permits it, never unconditionally for every mode. Generic runtime helpers use sealed mode traits; there is no public unchecked constructor that lets a caller forge write access or a proof state.

The representation must carry the correct ownership and borrowing semantics, not merely a tag alongside an unbounded raw pointer. Where marker fields are appropriate, `PhantomData` has no storage cost but affects variance, drop checking, and auto traits. It is not evidence that all higher-level validation or staging work is free. [Rustonomicon on PhantomData](https://doc.rust-lang.org/nomicon/phantom-data.html)

**Owner states make freezing a consuming transition.**

```rust
pub struct Message<T, State = Mutable> { /* private storage and state */ }
pub type FrozenMessage<T> = Message<T, Frozen>;

impl<T: Schema> Message<T, Mutable> {
    pub fn edit(&mut self) -> T::Mut<'_>;
    pub fn freeze(self) -> Message<T, Frozen>;
}

impl<T: Schema, S: MessageState> Message<T, S> {
    pub fn read(&self) -> T::Ref<'_>;
}
```

After `freeze`, `edit()` is absent from the type. The storage ownership moves into the frozen wrapper without copying payloads. Existing borrows must end before a consuming transition when those borrows would otherwise be used afterward.

A frozen owner is useful for APIs that retain immutable storage. Synchronous serialization may continue to accept an immutable borrow of a mutable message; forcing a freeze for that use case adds no safety benefit. If thawing is later exposed, it consumes a uniquely owned frozen value. Shared ownership requires recovery of uniqueness or an explicitly named copy, and any validation proof must be relinquished before mutation is reintroduced.

`Frozen` is intentionally independent of `Validated`. Borrowed/mapped bytes also require a stable backing-storage contract; a mode parameter does not prevent external file mutation.

**Local field states preserve presence checks for subsequent operations.**

```rust
match person.address().entry()? {
    Entry::Vacant(slot) => {
        let mut address = slot.init()?;
        address.city().copy_from("Edmonton")?;
    }
    Entry::Occupied(slot) => {
        let mut address = slot.ensure()?;
        address.city().copy_from("Edmonton")?;
    }
}
```

`entry()` performs the dynamic inspection and returns distinct typed handles, following the useful precedent of Rust’s `HashMap::Entry`. The vacant handle has `init`; the occupied handle does not. Both retain the exclusive parent-field borrow, so their presence fact cannot become stale through ordinary safe mutation. Consuming the handle to obtain a child prevents duplicate use. [Rust Entry API](https://doc.rust-lang.org/std/collections/hash_map/enum.Entry.html)

The implementation should retain the acquired pointer/layout information so a consuming entry operation does not immediately repeat the same acquisition. This can remove redundant work in a composed algorithm; it is not inherently faster than a well-written ordinary `ensure()` that also checks once. The initial dynamic inspection remains necessary.

`slot.init()` can still fail to allocate. `slot.ensure()` can still need layout expansion. `Occupied` is deliberately not named `ReadyToEdit`: presence does not establish that all required storage is available. After successful `edit()` or `ensure()`, the child editor itself carries the stronger writable-layout invariant needed by its scalar setters.

The simple `person.address().init()?` remains as a checked convenience over this transition. `ensure()` remains the common operation when callers do not care which branch occurs. Explicit entries are useful when an algorithm needs to distinguish new and existing values; typestate must not force a match into every application.

**Draft/ready replacements prevent premature publication.**

```rust
let staged = person.payload().stage_replace(byte_count)?;
// Pending<Data, Draft>: there is no commit() method yet.

let ready = staged.read_exact_from(&mut source)?;
// Pending<Data, Ready>: the source supplied the complete payload.

ready.commit()?;
```

`stage_replace` exclusively borrows the destination slot but leaves its old pointer intact. The draft owns detached storage. A complete read establishes the application-visible fill condition and returns the ready state. Draft storage is initialized or otherwise isolated so no safe view or output operation can observe uninitialized memory. This proposal uses zeroed data storage by default.

Only a ready replacement exposes `commit(self)`. For text or a structured object, the transition must establish the corresponding UTF-8 or construction invariants; a data fill check is not a universal message validator. Runtime lengths, callback results, and malformed source pointers still require runtime checks at the transition.

`commit` can remain fallible because it may reserve pointer metadata. It must prepare everything that can fail before changing the reachable destination, as required by the earlier error guarantee. Typestate controls when commit is callable; the runtime still implements its correctness. On a returned commit error, the old destination remains intact and the unpublished replacement can be discarded.

`init_with` and fallible-source replacements use this lifecycle internally, so common call sites do not name `Draft` or `Ready`. A compatible native-source `copy_from` can instead preflight and update existing storage with no detached candidate. The explicit staged API is useful for streaming directly into final message allocations, without an intermediate payload buffer.

Do not repair an invalid published value in a guard’s destructor or automatically commit on drop. Rust allows safe code to skip destructors with `mem::forget`. Forgetting an unpublished draft may waste arena space, but it must leave the old field valid and must never expose uninitialized memory. Cancellation and errors leave the same publication boundary intact. Drop is cleanup, not a prerequisite for soundness. [Rust mem::forget safety contract](https://doc.rust-lang.org/std/mem/fn.forget.html)

Unreachable does not by itself mean unobservable: a serializer can emit allocated segment ranges that include detached or abandoned storage. An uninitialized-data optimization would need to prevent every safe output path from observing those bytes, including after a forgotten draft. `Draft`/`Ready` markers alone do not establish this. Keep zeroed storage as the safe default and measure its initialization cost separately from payload copying.

**Validation states should initially be local.**

Returning `&str` after validation already carries exactly the UTF-8 proof most callers need. A separate `Text<Validated>` wrapper is unnecessary unless it provides additional behavior. Likewise, a checked struct view proves that the struct can be addressed, not that every descendant can be accessed without errors. [Rust from_utf8](https://doc.rust-lang.org/std/str/fn.from_utf8.html)

An optional `Validated<Person>` remains possible as a later runtime feature. Its checked constructor would need to define which schema and reachable fields are covered, how unknown union arms and `AnyPointer` are treated, how cycles and amplification are bounded, and how subsequent traversal avoids fallible budget exhaustion. It may require a scan and metadata even while preserving zero-copy payloads. Structural proof, schema-specific validity, and application validation must not be conflated.

This keeps the earlier decision to defer whole-message infallible readers. A marker cannot turn the current lazy reader’s successful past accesses into a guarantee about all future accesses.

**An arena brand can statically exclude cross-message adoption.**

This is a related generative-identity technique, not merely the mutable/frozen state pattern. A scoped editing API can give its root, orphanage, and detached objects the same fresh invariant brand:

```rust
message.scoped_edit(|session| {
    let (mut person, orphanage) = session.into_parts();

    if let Some(address) = person.address().take(&orphanage)? {
        person.previous_address().adopt(address)?;
    }

    Ok(())
})?;
```

The closure boundary introduces a private generative lifetime. Branded handles from separate sessions cannot be combined, and branded handles cannot escape through the closure’s result. The brand must be invariant and must only be minted by the controlled constructor. An arbitrary ordinary lifetime, even an invariant one obtained without that uniqueness discipline, is not an arena identity proof. See the [generativity crate’s identity model](https://docs.rs/generativity/latest/generativity/) and Rust’s [higher-ranked lifetime bounds](https://doc.rust-lang.org/reference/trait-bounds.html#higher-ranked-trait-bounds).

Adoption still returns `Result` for allocation and other relevant runtime failures. The existing unbranded `edit_with_orphans()` API can remain available with checked arena identity, especially for code that needs more flexible composition. Neither form permits duplicating an orphan or bypassing capability-context compatibility.

**Keep whole-object field flags out of the default generated API.**

Do not generate a family such as `Person<HasId, HasName, HasAddress, HasPhones, ...>` for ordinary wire editing. Independent presence flags create many type combinations, complicate conditionals and loops, and must be rebuilt from dynamic input. More fundamentally, a valid Cap’n Proto object can use schema defaults without the caller explicitly assigning every field.

An application can opt into a separate construction policy, such as requiring a nonempty name before issuing a business operation. A small `PersonDraft<NameMissing>` to `PersonDraft<NameProvided>` builder can express that policy; the input value or transition must validate any semantic constraints. This is application completeness, not wire initialization. It should not obstruct ordinary schema evolution or deserialization.

New acceptance checks should verify that frozen values lack editors, occupied entries lack `init`, draft replacements lack `commit`, handles cannot be reused after consuming transitions, and scoped arena brands cannot mix or escape. Runtime tests must cover allocation failure after readiness, failed streaming fills, and forgotten staged guards. The intended result is fewer invalid operations available in autocomplete while the ordinary API remains the same size.

**Performance review: retain the states, control the work.**

There are three design iterations to compare: the initial direct-accessor API, the recapn-inspired field-handle API, and the selective-typestate revision. Allocation/error policies and eager text decoding were already choices in the field-handle proposal. Their costs should not be attributed to adding state parameters. No implementation or benchmark accompanies this specification; the following conclusions are architectural expectations that need measurement.

| Concern | Earlier direct-accessor design | Field handles plus selective typestate | Performance decision |
| --- | --- | --- | --- |
| Scalar write | `set_id(value)` | `id().set(value)` | Target identical optimized loads/stores; verify cross-crate inlining |
| Reader/editor types | Separate wrappers | Sealed representations behind familiar aliases | Keep the same underlying representation; no dynamic dispatch |
| Presence branching | Runtime test in an operation | Checked `Entry` with typed branches | Carry the acquired information forward; do not check twice |
| Text read | Borrowed fallible getter | Convenient borrowed getter, plus advanced wire text | Keep validation local and defer UTF-8 work when only bytes/length are needed |
| Union read | Payload decoding depended on getter shape | Convenient fully decoded enum, plus tag-only access | Never require payload traversal just to inspect the discriminant |
| Replacement failure | Could leave partial state unless separately specified | Unchanged destination on returned error | Preflight and reuse where possible; stage fallible construction |
| Ownership transition | Different owner wrappers | Consuming mutable/frozen state transition | Move ownership without hidden validation, compaction, or reference counting |
| Generated footprint | Per-schema methods | Small wrappers plus shared runtime operations | Measure instantiated code and compile time, not just generated source lines |

**Wrappers are a code-generation question; validation is real work.**

Recapn's scalar field implementation stores a static descriptor and representation, then its inline setter passes the descriptor's slot/default to the underlying builder. Once inlined, a generated field accessor can expose constant offsets and defaults to the optimizer. If inlining fails, an accessor chain can retain calls, descriptor loads, and handle movement. This is why the target is parity with the old setter, not an unconditional claim of zero overhead. Rust explicitly treats `#[inline]` as a hint. [Recapn field implementation](https://github.com/cloudflare/recapn/blob/296226d8594796f7830d7899f1683d41486b54a9/recapn/src/field.rs), [Rust codegen attributes](https://doc.rust-lang.org/reference/attributes/codegen.html#the-inline-attribute)

Use ordinary static dispatch and small inline accessors on the hot path. Do not box field handles, resolve field names at runtime, or add an `Arc` clone to each accessor. Keep expensive recursive copying, validation, and diagnostic formatting in shared runtime routines where possible. Indiscriminate `inline(always)` can trade call overhead for instruction-cache pressure and code size.

`PhantomData` markers require no storage, and a generative lifetime does not introduce a runtime identity object or a separate code instance for each message lifetime. Actual type and const parameter combinations can create separate monomorphizations. A sealed read/write family is comparable to the existing separate reader/builder types; a product of field-presence flags, validation modes, allocator types, capability types, and schemas is a different proposition. Share large algorithms without forcing allocator or capability operations through trait objects. Recapn already has an empty capability-table representation that is useful to preserve for data-only messages. [Rust monomorphization](https://rustc-dev-guide.rust-lang.org/backend/monomorph.html), [Recapn capability tables](https://github.com/cloudflare/recapn/blob/296226d8594796f7830d7899f1683d41486b54a9/recapn/src/rpc.rs)

**The important read costs are repeated scans and pointer traversal.**

Returning `&str` avoids a payload copy, but validating a long string still examines its bytes. Retain `let name = person.name()?` if it is reused. Use `WireText` for a byte-oriented operation, and `employment_tag()` for routing by discriminant. The ordinary API remains convenient for callers that actually need decoded strings. [Rust UTF-8 conversion](https://doc.rust-lang.org/std/str/fn.from_utf8.html)

Acquire a nested struct or list outside a loop instead of repeatedly following its parent pointer. In capn-rs, repeated getters can consume the traversal budget repeatedly, so this is not work the optimizer can always eliminate as a redundant pure expression. The proposal should preserve bounded reads while making the natural iterator retain its checked list view. Iteration should advance with the acquired bounds and stride; nested pointer elements still require their own checks. [Reader limits](https://github.com/capnproto/capnproto-rust/blob/81bc1b815d0f450c9114f9cc2e2274182d210df2/capnp/src/message.rs)

`as_slice()` permits native bulk processing only for compatible primitive-list encodings. It must not silently copy on other encodings. Mutation callbacks can be statically dispatched and inlined; they do not inherently require allocation. Whole-message validation and eager projections remain opt-in because sparse reads, especially over mapped storage, benefit from not touching unused pages. Validation may pay off for repeated traversal of a stable snapshot, but requires the separate proof and budget contract already described.

**Staging and allocation are separate costs.**

For a fallible recursive source or callback, construct the candidate in its final arena allocation and publish only after successful preparation. This does not inherently copy the candidate twice. Upstream capn-rs text setters also allocate and copy, so comparing a staged allocation against an imaginary universally in-place baseline would be misleading. Its builder arena advances allocated word counts; clearing an old object does not reduce that counter. [Text pointer setters and object clearing](https://github.com/capnproto/capnproto-rust/blob/81bc1b815d0f450c9114f9cc2e2274182d210df2/capnp/src/private/layout.rs), [Builder arena allocation](https://github.com/capnproto/capnproto-rust/blob/81bc1b815d0f450c9114f9cc2e2274182d210df2/capnp/src/private/arena.rs)

The material improvement is allowing compatible native-source copies to reuse writable storage. For a payload of size n replaced k times in a non-reclaiming arena, allocating each replacement can consume storage proportional to k times n; same-size reuse retains storage proportional to n. Both still copy proportional to k times n payload bytes. This is an asymptotic example, excluding headers, segment slack, cleanup, and metadata, not a measured result. `replace` deliberately requests fresh storage; `edit` guarantees existing storage; `copy_from` lets the runtime select a correct strategy.

Error atomicity also has costs beyond payload allocation: metadata reservation, old-value cleanup, and retained failed allocations. Do not add a complete prevalidation traversal followed by an equally checked copy traversal as the default implementation. A detached single-pass construction can combine checking and copying while preserving the old logical value. Cleanup must obey the runtime's capability and storage rules and its error contract.

An orphan move means no payload copy, not necessarily constant total time. Moving into a vacant compatible destination can avoid work that overwriting an occupied destination cannot: clearing the old object may traverse its subtree, and connecting segments can require pointer metadata. State-based elimination of an arena-identity check is useful, but is unlikely to outweigh copying a large payload or clearing a large displaced graph. Benchmark vacant and occupied adoption separately.

Zeroed data followed by a streaming fill can write the destination twice even though no intermediate payload buffer exists. Track bytes initialized separately from bytes copied. A future sound uninitialized-allocation API is a runtime feature, not a benefit automatically obtained from `Pending<Draft>`.

**Validate these hypotheses with equivalent workloads.**

Use the same schemas, input bytes, segment sizing, traversal limits, and allocator policy. Compare upstream capn-rs, recapn's strict accessors, the direct-accessor facade, and the field-handle/typestate facade. Separate unavoidable costs of stronger error guarantees from wrapper costs by running both equal-semantics comparisons and explicitly labeled policy comparisons. Do not compare recapn's error-suppressing fast paths to strict proposed accessors as if their guarantees matched.

| Workload | Question | Measurements |
| --- | --- | --- |
| Scalar get/set loops in a separate consumer crate | Do handles and state aliases disappear after optimization? | Assembly, throughput, instructions, with default release and with LTO |
| Repeated text access, byte-only inspection, and union tag filtering | Are scans and unused payload reads avoidable? | Bytes examined, traversal charges, elapsed time |
| Sparse reads versus complete traversal | Does optional validation justify its up-front scan? | Construction plus access time, cache/page effects, metadata allocation |
| Repeated same-size text/data updates | Does reuse actually prevent arena growth? | Allocations, allocated words, bytes zeroed/copied, final segment size |
| Nested copy and callback failure at early/late positions | What does unchanged-on-error cost? | Traversal, retained allocation, cleanup work, destination correctness |
| Primitive and struct-list loops | Are acquisition/bounds checks duplicated? | Throughput, instructions, vectorization where applicable |
| Vacant/occupied and same/cross-segment orphan moves | Which moves avoid copying, and what cleanup remains? | Payload copies, metadata allocation, cleanup traversal |
| Small and large generated schemas in consumer crates | Do states and callbacks inflate compilation? | Clean/incremental build time, release text size, peak compiler memory |

Use several payload sizes and warm/cold access patterns; prevent dead-code elimination and report the compiler, optimization profile, hardware, and variability. Inspect optimized code for tiny operations before attributing timing differences to syntax. No numerical performance claim is justified until these comparisons exist.

The recommended implementation keeps direct borrowed readers, field-operation editors, and a few local state transitions. It adds cheap inspection paths, implements compatible-storage reuse, stages only where the error contract requires it, and keeps substantial algorithms in the runtime. This preserves the ergonomic gains while making the dominant work explicit and testable.
