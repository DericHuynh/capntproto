//! Revision-pinned schema transmission over an ordinary RPC capability.
//! A Bundle owns schema node bytes and their complete dependency closure. It is
//! metadata. Bundle::load() constructs a validated, owned runtime schema loader.
use crate::schema_exchange_capnp as wire;
use capnp::{
    schema_capnp::{annotation, brand, field, node, type_},
    Error,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

fn invalid(message: &str) -> Error {
    Error::failed(message.into())
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Key {
    pub id: u64,
    pub revision: u64,
}
impl Key {
    fn check(self) -> capnp::Result<Self> {
        if self.id == 0 || self.revision == 0 {
            Err(invalid("schema ID and revision must be nonzero"))
        } else {
            Ok(self)
        }
    }
    fn read(key: wire::key::Reader<'_>) -> capnp::Result<Self> {
        Self {
            id: key.get_id(),
            revision: key.get_revision(),
        }
        .check()
    }
    fn write(self, mut key: wire::key::Builder<'_>) {
        key.set_id(self.id);
        key.set_revision(self.revision);
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub nodes: usize,
    pub bytes: usize,
    pub node_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            nodes: 4096,
            bytes: 16 * 1024 * 1024,
            node_bytes: 1024 * 1024,
        }
    }
}
impl Limits {
    fn options(self) -> capnp::message::ReaderOptions {
        let mut options = capnp::message::ReaderOptions::new();
        options.traversal_limit_in_words(Some(self.node_bytes));
        options.nesting_limit(64);
        options
    }
}
#[derive(Clone)]
pub struct Definition {
    key: Key,
    bytes: Rc<[u8]>,
    dependencies: BTreeSet<Key>,
    limits: Limits,
}
impl Definition {
    /// Decode exactly one unpacked schema.Node and derive dependencies from its
    /// contents, never from an untrusted separately supplied dependency list.
    pub fn decode(key: Key, bytes: &[u8], limits: Limits) -> capnp::Result<Self> {
        key.check()?;
        if bytes.len() > limits.node_bytes {
            return Err(invalid("schema node byte limit"));
        }
        let message = read(bytes, limits)?;
        let proto = message.get_root::<node::Reader>()?;
        if proto.get_id() != key.id {
            return Err(invalid("schema node ID mismatch"));
        }
        if proto.total_size()?.cap_count != 0 {
            return Err(invalid("schema nodes cannot contain live capabilities"));
        }
        let mut scan = Dependencies {
            ids: BTreeSet::new(),
            budget: limits.node_bytes,
            max: limits.nodes,
        };
        scan.node(proto)?;
        Ok(Self {
            key,
            bytes: bytes.into(),
            dependencies: scan
                .ids
                .into_iter()
                .map(|id| Key {
                    id,
                    revision: key.revision,
                })
                .collect(),
            limits,
        })
    }
    pub fn from_node(
        revision: u64,
        proto: node::Reader<'_>,
        limits: Limits,
    ) -> capnp::Result<Self> {
        let size = proto.total_size()?;
        if size.cap_count != 0 || size.word_count > (limits.node_bytes / 8) as u64 {
            return Err(invalid("schema node size/capability limit"));
        }
        let mut message = capnp::message::Builder::new_default();
        message.set_root(proto)?;
        Self::decode(
            Key {
                id: proto.get_id(),
                revision,
            },
            &capnp::serialize::write_message_to_words(&message),
            limits,
        )
    }
    pub fn key(&self) -> Key {
        self.key
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn dependencies(&self) -> &BTreeSet<Key> {
        &self.dependencies
    }
    /// The returned reader owns its arena. Node readers borrow that arena; no
    /// schema data is leaked or promoted to a fabricated 'static lifetime.
    pub fn read(&self) -> capnp::Result<capnp::message::Reader<capnp::serialize::OwnedSegments>> {
        read(&self.bytes, self.limits)
    }
}
fn read(
    bytes: &[u8],
    limits: Limits,
) -> capnp::Result<capnp::message::Reader<capnp::serialize::OwnedSegments>> {
    let mut tail = bytes;
    let message = capnp::serialize::read_message(&mut tail, limits.options())?;
    if !tail.is_empty() {
        return Err(invalid("trailing bytes after schema node"));
    }
    Ok(message)
}
struct Dependencies {
    ids: BTreeSet<u64>,
    budget: usize,
    max: usize,
}
impl Dependencies {
    fn spend(&mut self) -> capnp::Result<()> {
        self.budget = self
            .budget
            .checked_sub(1)
            .ok_or_else(|| invalid("schema traversal limit"))?;
        Ok(())
    }
    fn id(&mut self, id: u64) -> capnp::Result<()> {
        if id != 0 {
            self.ids.insert(id);
        }
        if self.ids.len() > self.max {
            return Err(invalid("schema dependency limit"));
        }
        Ok(())
    }
    fn required(&mut self, id: u64) -> capnp::Result<()> {
        if id == 0 {
            return Err(invalid("zero schema dependency ID"));
        }
        self.id(id)
    }
    fn annotations(
        &mut self,
        list: capnp::struct_list::Reader<'_, annotation::Owned>,
    ) -> capnp::Result<()> {
        for a in list {
            self.spend()?;
            self.required(a.get_id())?;
            self.brand(a.get_brand()?, 0)?;
            a.get_value()?.which()?;
        }
        Ok(())
    }
    fn brand(&mut self, value: brand::Reader<'_>, depth: usize) -> capnp::Result<()> {
        self.spend()?;
        if depth >= 64 {
            return Err(invalid("schema type nesting limit"));
        }
        for scope in value.get_scopes()? {
            self.spend()?;
            self.required(scope.get_scope_id())?;
            if let brand::scope::Bind(bindings) = scope.which()? {
                for binding in bindings? {
                    self.spend()?;
                    if let brand::binding::Type(ty) = binding.which()? {
                        self.ty(ty?, depth + 1)?;
                    }
                }
            }
        }
        Ok(())
    }
    fn ty(&mut self, ty: type_::Reader<'_>, depth: usize) -> capnp::Result<()> {
        self.spend()?;
        if depth >= 64 {
            return Err(invalid("schema type nesting limit"));
        }
        match ty.which()? {
            type_::List(list) => self.ty(list.get_element_type()?, depth + 1)?,
            type_::Struct(t) => {
                self.required(t.get_type_id())?;
                self.brand(t.get_brand()?, depth + 1)?;
            }
            type_::Enum(t) => {
                self.required(t.get_type_id())?;
                self.brand(t.get_brand()?, depth + 1)?;
            }
            type_::Interface(t) => {
                self.required(t.get_type_id())?;
                self.brand(t.get_brand()?, depth + 1)?;
            }
            type_::AnyPointer(t) => match t.which()? {
                type_::any_pointer::Parameter(p) => self.required(p.get_scope_id())?,
                type_::any_pointer::Unconstrained(p) => {
                    p.which()?;
                }
                type_::any_pointer::ImplicitMethodParameter(_) => (),
            },
            _ => (),
        }
        Ok(())
    }
    fn node(&mut self, proto: node::Reader<'_>) -> capnp::Result<()> {
        proto.get_display_name()?.to_str()?;
        self.id(proto.get_scope_id())?;
        for nested in proto.get_nested_nodes()? {
            self.spend()?;
            nested.get_name()?.to_str()?;
            // Declaration indexes can name unused nodes absent from a compiler
            // request. They are navigation metadata, not type dependencies.
            if nested.get_id() == 0 {
                return Err(invalid("zero nested schema ID"));
            }
        }
        for parameter in proto.get_parameters()? {
            self.spend()?;
            parameter.get_name()?.to_str()?;
        }
        self.annotations(proto.get_annotations()?)?;
        match proto.which()? {
            node::File(()) => (),
            node::Struct(s) => {
                for f in s.get_fields()? {
                    self.spend()?;
                    f.get_name()?.to_str()?;
                    self.annotations(f.get_annotations()?)?;
                    match f.which()? {
                        field::Slot(slot) => {
                            self.ty(slot.get_type()?, 0)?;
                            slot.get_default_value()?.which()?;
                        }
                        field::Group(group) => self.required(group.get_type_id())?,
                    }
                }
            }
            node::Enum(e) => {
                for e in e.get_enumerants()? {
                    self.spend()?;
                    e.get_name()?.to_str()?;
                    self.annotations(e.get_annotations()?)?;
                }
            }
            node::Interface(i) => {
                for s in i.get_superclasses()? {
                    self.spend()?;
                    self.required(s.get_id())?;
                    self.brand(s.get_brand()?, 0)?;
                }
                for m in i.get_methods()? {
                    self.spend()?;
                    m.get_name()?.to_str()?;
                    self.annotations(m.get_annotations()?)?;
                    self.required(m.get_param_struct_type())?;
                    self.required(m.get_result_struct_type())?;
                    self.brand(m.get_param_brand()?, 0)?;
                    self.brand(m.get_result_brand()?, 0)?;
                }
            }
            node::Const(c) => {
                self.ty(c.get_type()?, 0)?;
                c.get_value()?.which()?;
            }
            node::Annotation(a) => self.ty(a.get_type()?, 0)?,
        }
        Ok(())
    }
}
/// Owner-controlled catalog. A service capability exposes read access only.
/// A revision can be published incrementally; clients reject missing closure.
pub struct Catalog {
    entries: BTreeMap<Key, Definition>,
    bytes: usize,
    limits: Limits,
}
impl Catalog {
    pub fn new(limits: Limits) -> Self {
        Self {
            entries: BTreeMap::new(),
            bytes: 0,
            limits,
        }
    }
    /// Publish a batch atomically. An existing key may only repeat identical
    /// bytes; newer revisions leave all earlier revisions available.
    pub fn publish(
        &mut self,
        definitions: impl IntoIterator<Item = Definition>,
    ) -> capnp::Result<()> {
        let mut staged: BTreeMap<Key, Definition> = BTreeMap::new();
        let mut bytes = self.bytes;
        for d in definitions {
            if d.bytes.len() > self.limits.node_bytes {
                return Err(invalid("schema node byte limit"));
            }
            if let Some(old) = self.entries.get(&d.key).or_else(|| staged.get(&d.key)) {
                if old.bytes != d.bytes {
                    return Err(invalid("immutable schema revision conflict"));
                }
            } else {
                bytes = bytes
                    .checked_add(d.bytes.len())
                    .ok_or_else(|| invalid("schema byte overflow"))?;
                if bytes > self.limits.bytes
                    || self.entries.len() + staged.len() >= self.limits.nodes
                {
                    return Err(invalid("schema catalog limit"));
                }
                staged.insert(d.key, d);
            }
        }
        self.entries.extend(staged);
        self.bytes = bytes;
        Ok(())
    }
    /// Import a compiler request without exposing a partially validated batch.
    pub fn publish_request(
        &mut self,
        revision: u64,
        request: capnp::schema_capnp::code_generator_request::Reader<'_>,
    ) -> capnp::Result<()> {
        let nodes = request.get_nodes()?;
        if nodes.len() as usize > self.limits.nodes {
            return Err(invalid("schema catalog node limit"));
        }
        let mut staged = Self::new(self.limits);
        for node in nodes {
            staged.publish([Definition::from_node(revision, node, self.limits)?])?;
        }
        self.publish(staged.entries.into_values())
    }
    pub fn get(&self, key: Key) -> Option<&Definition> {
        self.entries.get(&key)
    }
    pub fn service(owner: Rc<RefCell<Self>>) -> wire::catalog::Client {
        capnp_rpc::new_client(Service(owner))
    }
}
struct Service(Rc<RefCell<Catalog>>);
impl wire::catalog::Server for Service {
    async fn get(
        self: Rc<Self>,
        p: wire::catalog::GetParams,
        mut r: wire::catalog::GetResults,
    ) -> capnp::Result<()> {
        let key = Key::read(p.get()?.get_key()?)?;
        let catalog = self.0.borrow();
        let mut result = r.get().init_result();
        match catalog.get(key) {
            None => result.set_missing(()),
            Some(d) => {
                let mut out = result.init_found();
                d.key.write(out.reborrow().init_key());
                out.set_node(&d.bytes);
            }
        }
        Ok(())
    }
}
/// No partially retrieved graph is exposed as an active Bundle.
pub struct Retrieval {
    root: Key,
    limits: Limits,
    needed: BTreeSet<Key>,
    requested: BTreeSet<Key>,
    entries: BTreeMap<Key, Definition>,
    bytes: usize,
    failed: bool,
}
impl Retrieval {
    pub fn new(root: Key, limits: Limits) -> capnp::Result<Self> {
        root.check()?;
        if limits.nodes == 0 {
            return Err(invalid("schema node limit"));
        }
        Ok(Self {
            root,
            limits,
            needed: BTreeSet::from([root]),
            requested: BTreeSet::new(),
            entries: BTreeMap::new(),
            bytes: 0,
            failed: false,
        })
    }
    pub fn request(&mut self) -> capnp::Result<Option<Key>> {
        if self.failed {
            return Err(invalid("schema retrieval failed"));
        }
        let next = self.needed.difference(&self.requested).next().copied();
        if let Some(key) = next {
            self.requested.insert(key);
        }
        Ok(next)
    }
    pub fn receive(&mut self, expected: Key, definition: Option<Definition>) -> capnp::Result<()> {
        let result = self.accept(expected, definition);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn accept(&mut self, expected: Key, definition: Option<Definition>) -> capnp::Result<()> {
        if self.failed
            || !self.requested.contains(&expected)
            || self.entries.contains_key(&expected)
        {
            return Err(invalid("unexpected schema response"));
        }
        let definition = definition.ok_or_else(|| invalid("schema revision missing"))?;
        if definition.key != expected {
            return Err(invalid("schema response key mismatch"));
        }
        if definition.bytes.len() > self.limits.node_bytes {
            return Err(invalid("schema node byte limit"));
        }
        let bytes = self
            .bytes
            .checked_add(definition.bytes.len())
            .ok_or_else(|| invalid("schema byte overflow"))?;
        let needed: BTreeSet<_> = self
            .needed
            .union(&definition.dependencies)
            .copied()
            .collect();
        if bytes > self.limits.bytes || needed.len() > self.limits.nodes {
            return Err(invalid("schema retrieval limit"));
        }
        self.needed = needed;
        self.bytes = bytes;
        self.entries.insert(expected, definition);
        Ok(())
    }
    pub fn ready(&self) -> bool {
        !self.failed && self.needed.iter().all(|k| self.entries.contains_key(k))
    }
    pub fn finish(self) -> capnp::Result<Bundle> {
        if !self.ready() {
            return Err(invalid("schema closure is incomplete or rejected"));
        }
        Ok(Bundle {
            root: self.root,
            entries: self.entries,
        })
    }
    pub fn counts(&self) -> (usize, usize, usize, bool) {
        (
            self.needed.len(),
            self.requested.len(),
            self.entries.len(),
            self.failed,
        )
    }
}
pub struct Bundle {
    root: Key,
    entries: BTreeMap<Key, Definition>,
}
impl Bundle {
    /// Activate the pinned closure in a fresh loader. Catalog revisions never
    /// compete with definitions from another catalog or revision.
    pub fn load(
        &self,
        limits: capnp::schema_loader::Limits,
    ) -> capnp::Result<capnp::schema_loader::SchemaLoader> {
        let messages: Vec<_> = self
            .entries
            .values()
            .map(Definition::read)
            .collect::<capnp::Result<_>>()?;
        let nodes: Vec<_> = messages
            .iter()
            .map(|m| m.get_root::<node::Reader>())
            .collect::<capnp::Result<_>>()?;
        let mut loader = capnp::schema_loader::SchemaLoader::new(limits);
        loader.load_batch(nodes)?;
        Ok(loader)
    }
    pub fn root(&self) -> Key {
        self.root
    }
    pub fn get(&self, key: Key) -> Option<&Definition> {
        self.entries.get(&key)
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
/// Fetch a pinned closure with up to `concurrency` outstanding ordinary RPC
/// calls. Cancellation drops all outstanding calls and the unpublished graph.
pub async fn fetch(
    client: &wire::catalog::Client,
    root: Key,
    limits: Limits,
    concurrency: usize,
) -> capnp::Result<Bundle> {
    use futures::{stream::FuturesUnordered, StreamExt};
    if concurrency == 0 || concurrency > 64 {
        return Err(invalid("schema concurrency must be 1..64"));
    }
    let mut retrieval = Retrieval::new(root, limits)?;
    let mut pending = FuturesUnordered::new();
    loop {
        while pending.len() < concurrency {
            let Some(key) = retrieval.request()? else {
                break;
            };
            let mut request = client.get_request();
            key.write(request.get().init_key());
            let promise = request.send().promise;
            pending.push(async move {
                let response = promise.await?;
                let result = match response.get()?.get_result()?.which()? {
                    wire::catalog::result::Missing(()) => None,
                    wire::catalog::result::Found(d) => {
                        let d = d?;
                        let actual = Key::read(d.get_key()?)?;
                        if actual != key {
                            return Err(invalid("schema response key mismatch"));
                        }
                        Some(Definition::decode(actual, d.get_node()?, limits)?)
                    }
                };
                Ok::<_, Error>((key, result))
            });
        }
        if retrieval.ready() {
            return retrieval.finish();
        }
        let (key, definition) = pending
            .next()
            .await
            .ok_or_else(|| invalid("schema retrieval made no progress"))??;
        retrieval.receive(key, definition)?;
    }
}
