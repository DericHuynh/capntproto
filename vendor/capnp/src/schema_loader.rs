//! Owned runtime schema loading. Handles borrow the loader: replacing a schema
//! requires an exclusive borrow, so a live dynamic message view cannot be invalidated.
//! Compiled reflection remains allocation-free and independent of this API.
//!
//! A schema handle cannot escape its arena owner:
//! ```compile_fail
//! use capnp::schema_loader::SchemaLoader;
//! let schema = {
//!     let loader = SchemaLoader::default();
//!     loader.get(1).unwrap()
//! };
//! let _ = schema.id();
//! ```
//!
//! Replacing schemas cannot invalidate a live view:
//! ```compile_fail
//! use capnp::{message, schema_capnp::node, schema_loader::SchemaLoader};
//! let mut loader = SchemaLoader::default();
//! let mut message = message::Builder::new_default();
//! let mut node = message.init_root::<node::Builder>();
//! node.set_id(1);
//! node.init_struct();
//! loader.load(message.get_root_as_reader().unwrap()).unwrap();
//! let schema = loader.get(1).unwrap();
//! loader.load(message.get_root_as_reader().unwrap()).unwrap();
//! let _ = schema.id();
//! ```
use crate::{
    message,
    schema_capnp::{brand, field, node, type_},
    Error, Result,
};
use alloc::{
    boxed::Box,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    vec::Vec,
};
mod brands;
mod compatible;
pub use brands::BrandArguments;
pub mod dynamic;
mod metadata;
mod native;
pub use metadata::{Annotation, AnnotationList, Enumerant};
mod validate;

fn invalid(s: &str) -> Error {
    Error::failed(s.into())
}
fn require(ok: bool, s: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(invalid(s))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub nodes: usize,
    pub words: usize,
    pub node_words: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            nodes: 4096,
            words: 2 * 1024 * 1024,
            node_words: 128 * 1024,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Struct,
    Enum,
    Interface,
    Const,
    Annotation,
}
fn kind(n: node::Reader<'_>) -> Result<Kind> {
    Ok(match n.which()? {
        node::File(()) => Kind::File,
        node::Struct(_) => Kind::Struct,
        node::Enum(_) => Kind::Enum,
        node::Interface(_) => Kind::Interface,
        node::Const(_) => Kind::Const,
        node::Annotation(_) => Kind::Annotation,
    })
}
#[derive(Clone)]
struct Entry {
    message: Rc<message::Builder<message::HeapAllocator>>,
    stub: bool,
}
impl Entry {
    fn proto(&self) -> node::Reader<'_> {
        self.message
            .get_root_as_reader()
            .expect("validated owned schema")
    }
}

#[derive(Clone, Default)]
pub struct SchemaLoader {
    entries: BTreeMap<u64, Entry>,
    limits: Limits,
    native_ids: BTreeSet<u64>,
}
impl SchemaLoader {
    pub fn new(limits: Limits) -> Self {
        Self {
            entries: BTreeMap::new(),
            limits,
            native_ids: BTreeSet::new(),
        }
    }
    pub fn get(&self, id: u64) -> Result<Schema<'_>> {
        require(self.entries.contains_key(&id), "schema ID is not loaded")?;
        Ok(Schema {
            loader: self,
            id,
            bindings: Rc::new(BTreeMap::new()),
            unbound: false,
        })
    }
    pub fn try_get(&self, id: u64) -> Option<Schema<'_>> {
        self.get(id).ok()
    }
    pub fn get_unbound(&self, id: u64) -> Result<Schema<'_>> {
        let mut s = self.get(id)?;
        s.unbound = s.get_proto().get_is_generic();
        Ok(s)
    }
    pub fn get_all_loaded(&self) -> impl Iterator<Item = Schema<'_>> {
        self.entries
            .iter()
            .filter(|(_, e)| !e.stub)
            .map(|(&id, _)| self.get(id).unwrap())
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Copy and validate before committing. Dependencies missing from this batch
    /// become typed stubs. A failure leaves every previous definition unchanged.
    pub fn load(&mut self, proto: node::Reader<'_>) -> Result<Schema<'_>> {
        let id = proto.get_id();
        self.load_batch([proto])?;
        self.get(id)
    }
    pub fn load_once(&mut self, proto: node::Reader<'_>) -> Result<Schema<'_>> {
        let id = proto.get_id();
        if !self.entries.get(&id).is_some_and(|e| !e.stub) {
            self.load_batch([proto])?;
        }
        self.get(id)
    }
    /// Explicit lazy loading requires an exclusive borrow, preserving all live
    /// schema/view lifetimes. The callback can load a whole dependency closure.
    pub fn get_or_load(
        &mut self,
        id: u64,
        load: impl FnOnce(&mut Self, u64) -> Result<()>,
    ) -> Result<Schema<'_>> {
        if self.entries.get(&id).is_none_or(|e| e.stub) {
            load(self, id)?;
        }
        self.get(id)
    }
    /// Register native schemas before using downcast_native(). Definitions still
    /// pass the same compatibility checks as schemas received over the network.
    pub fn load_compiled_type_and_dependencies<T: crate::introspect::Introspect>(
        &mut self,
    ) -> Result<()> {
        use crate::introspect::TypeVariant as V;
        let mut pending = alloc::vec![T::introspect()];
        let mut seen = BTreeSet::new();
        let mut nodes = Vec::new();
        while let Some(ty) = pending.pop() {
            match ty.which() {
                V::Struct(_) => {
                    let s = ty.as_struct_schema()?;
                    let n = s.get_proto();
                    if seen.insert(n.get_id()) {
                        nodes.push(n);
                        for f in s.get_fields()? {
                            pending.push(f.get_type());
                        }
                    }
                }
                V::Interface(raw) => {
                    let s = crate::schema::InterfaceSchema::new(raw);
                    let n = s.get_proto();
                    if seen.insert(n.get_id()) {
                        nodes.push(n);
                        for parent in s.get_superclasses()?.iter() {
                            pending
                                .push(crate::introspect::TypeVariant::Interface(parent.raw).into());
                        }
                        for method in s.get_methods()?.iter() {
                            pending.push(method.get_param_type().as_type());
                            if let Some(result) = method.get_result_type() {
                                pending.push(result.as_type());
                            }
                        }
                    }
                }
                V::Enum(raw) => {
                    let s: crate::schema::EnumSchema = raw.into();
                    let n = s.get_proto();
                    if seen.insert(n.get_id()) {
                        nodes.push(n);
                    }
                }
                V::List(element) => pending.push(element),
                _ => (),
            }
            require(
                seen.len() <= self.limits.nodes && pending.len() <= self.limits.node_words,
                "compiled schema dependency limit",
            )?;
        }
        self.load_batch(nodes)?;
        self.native_ids.extend(seen);
        Ok(())
    }
    pub fn load_request(
        &mut self,
        request: crate::schema_capnp::code_generator_request::Reader<'_>,
    ) -> Result<()> {
        self.load_batch(request.get_nodes()?.iter())
    }
    pub fn load_batch<'a>(
        &mut self,
        nodes: impl IntoIterator<Item = node::Reader<'a>>,
    ) -> Result<()> {
        let mut staged = self.clone();
        let mut requirements = BTreeMap::new();
        let mut expectations = Vec::new();
        let mut count = 0usize;
        let mut copied_words = 0u64;
        for n in nodes {
            count += 1;
            require(count <= self.limits.nodes, "schema batch node limit")?;
            let size = n.total_size()?;
            copied_words = copied_words
                .checked_add(size.word_count)
                .ok_or_else(|| invalid("schema size overflow"))?;
            require(
                size.cap_count == 0
                    && size.word_count <= self.limits.node_words as u64
                    && copied_words <= self.limits.words as u64,
                "schema size/capability limit",
            )?;
            let mut message = message::Builder::new_default();
            message.set_root(n)?;
            if let Some(old) = staged.entries.get(&n.get_id()).filter(|e| e.stub) {
                if let (node::Struct(minimum), node::Struct(mut actual)) = (
                    old.proto().which()?,
                    message.get_root::<node::Builder>()?.which()?,
                ) {
                    let data = actual
                        .reborrow()
                        .get_data_word_count()
                        .max(minimum.get_data_word_count());
                    let pointers = actual
                        .reborrow()
                        .get_pointer_count()
                        .max(minimum.get_pointer_count());
                    actual.set_data_word_count(data);
                    actual.set_pointer_count(pointers);
                }
            }
            let entry = Entry {
                message: Rc::new(message),
                stub: false,
            };
            let n = entry.proto();
            validate::node(n, &mut requirements)?;
            let id = n.get_id();
            let replace = match staged.entries.get(&id) {
                None => true,
                Some(old) if old.stub => {
                    compatible::replace(old.proto(), n, &mut expectations)?
                        || !compatible::replace(n, old.proto(), &mut expectations)?
                }
                Some(old) => compatible::replace(old.proto(), n, &mut expectations)?,
            };
            if replace {
                staged.entries.insert(id, entry);
            }
            require(
                staged.entries.len() <= self.limits.nodes,
                "schema node limit",
            )?;
        }
        let mut constraints = 0;
        while let Some(expected) = expectations.pop() {
            constraints += 1;
            require(
                constraints <= self.limits.nodes,
                "schema upgrade constraint limit",
            )?;
            let id = expected.proto().get_id();
            validate::node(expected.proto(), &mut requirements)?;
            let replace = if let Some(old) = staged.entries.get(&id) {
                compatible::replace(old.proto(), expected.proto(), &mut expectations)?
            } else {
                true
            };
            if replace {
                staged.entries.insert(id, expected);
            }
            require(
                staged.entries.len() <= self.limits.nodes,
                "schema node limit",
            )?;
        }
        for entry in staged.entries.values() {
            validate::node(entry.proto(), &mut requirements)?;
        }
        for (id, req) in requirements {
            if let Some(entry) = staged.entries.get(&id) {
                require(kind(entry.proto())? == req.kind, "dependency kind mismatch")?;
                if req.data != 0 || req.pointers != 0 {
                    let node::Struct(s) = entry.proto().which()? else {
                        return Err(invalid("group parent is not a struct"));
                    };
                    if s.get_data_word_count() < req.data || s.get_pointer_count() < req.pointers {
                        // Parent sizes must include groups loaded independently, as in C++.
                        let mut message = message::Builder::new_default();
                        message.set_root(entry.proto())?;
                        let node::Struct(mut s) = message.get_root::<node::Builder>()?.which()?
                        else {
                            unreachable!()
                        };
                        let data = s.reborrow().get_data_word_count().max(req.data);
                        s.set_data_word_count(data);
                        let pointers = s.reborrow().get_pointer_count().max(req.pointers);
                        s.set_pointer_count(pointers);
                        staged.entries.insert(
                            id,
                            Entry {
                                message: Rc::new(message),
                                stub: entry.stub,
                            },
                        );
                    }
                }
            } else {
                require(
                    staged.entries.len() < self.limits.nodes,
                    "schema dependency limit",
                )?;
                let mut message = message::Builder::new_default();
                let mut n = message.init_root::<node::Builder>();
                n.set_id(id);
                n.set_display_name("(unloaded schema)");
                match req.kind {
                    Kind::Struct => {
                        let mut s = n.init_struct();
                        s.set_data_word_count(req.data);
                        s.set_pointer_count(req.pointers);
                    }
                    Kind::Enum => {
                        n.init_enum();
                    }
                    Kind::Interface => {
                        n.init_interface();
                    }
                    _ => return Err(invalid("invalid stub kind")),
                }
                staged.entries.insert(
                    id,
                    Entry {
                        message: Rc::new(message),
                        stub: true,
                    },
                );
            }
        }
        validate::group_sizes(&mut staged)?;
        validate::graph(&staged)?;
        let words: usize = staged
            .entries
            .values()
            .map(|e| e.message.size_in_words())
            .sum();
        require(words <= self.limits.words, "schema loader word limit")?;
        *self = staged;
        Ok(())
    }
}

type Bindings<'a> = BTreeMap<u64, BrandArguments<'a>>;
#[derive(Clone)]
pub struct Schema<'a> {
    loader: &'a SchemaLoader,
    id: u64,
    bindings: Rc<Bindings<'a>>,
    unbound: bool,
}
impl<'a> Schema<'a> {
    /// Snapshot loader ownership and the full applied brand into an immutable
    /// `Eq + Hash` key. Construction permits at most 128 metadata visits.
    ///
    /// Keys cannot outlive the loader:
    /// ```compile_fail
    /// use capnp::schema_loader::SchemaLoader;
    /// let key = {
    ///     let loader = SchemaLoader::default();
    ///     loader.get(1).unwrap().identity().unwrap()
    /// };
    /// drop(key);
    /// ```
    /// Nor can a live key be invalidated by replacement:
    /// ```compile_fail
    /// use capnp::{message, schema_capnp::node, schema_loader::SchemaLoader};
    /// let mut loader = SchemaLoader::default();
    /// let key = loader.get(1).unwrap().identity().unwrap();
    /// let mut data = message::Builder::new_default();
    /// data.init_root::<node::Builder>().set_id(1);
    /// loader.load(data.get_root_as_reader().unwrap()).unwrap();
    /// drop(key);
    /// ```
    pub fn identity(&self) -> Result<crate::schema::SchemaIdentity<'a>> {
        let mut key = crate::schema::identity::Builder::new();
        self.write_identity(&mut key)?;
        Ok(key.finish(self.id))
    }
    fn write_identity(&self, key: &mut crate::schema::identity::Builder) -> Result<()> {
        use crate::schema::identity::Part;
        key.spend()?;
        key.parts.push(Part::Loaded {
            address: self.loader as *const _ as usize,
            id: self.id,
            unbound: self.unbound,
        });
        key.parts.push(Part::Length(self.bindings.len()));
        for arguments in self.bindings.values() {
            arguments.write_identity(key)?;
        }
        Ok(())
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn get_proto(&self) -> node::Reader<'a> {
        self.loader.entries[&self.id].proto()
    }
    /// The display name after its schema-supplied byte prefix.
    pub fn short_display_name(&self) -> Result<crate::text::Reader<'a>> {
        crate::schema::short_display_name(self.get_proto())
    }
    /// Alias for `short_display_name()`, matching the C++ schema API.
    pub fn unqualified_name(&self) -> Result<crate::text::Reader<'a>> {
        self.short_display_name()
    }
    pub fn kind(&self) -> Kind {
        kind(self.get_proto()).unwrap()
    }
    pub fn is_stub(&self) -> bool {
        self.loader.entries[&self.id].stub
    }
    pub fn equals(&self, other: &Self) -> bool {
        core::ptr::eq(self.loader, other.loader)
            && self.id == other.id
            && self.unbound == other.unbound
            && self.bindings == other.bindings
    }
    pub fn bind(&self, brand: brand::Reader<'_>, scope: Option<&Schema<'a>>) -> Result<Self> {
        self.bind_inner(brand, scope, 0)
    }
    fn bind_inner(
        &self,
        brand: brand::Reader<'_>,
        scope: Option<&Schema<'a>>,
        depth: usize,
    ) -> Result<Self> {
        require(depth < 64, "schema brand nesting limit")?;
        let mut bindings = BTreeMap::new();
        let default_scope = self.generic();
        for s in brand.get_scopes()? {
            let id = s.get_scope_id();
            let target = self.loader.get(id)?;
            let arguments = match s.which()? {
                brand::scope::Bind(values) => {
                    let values = values?;
                    require(
                        values.len() <= target.get_proto().get_parameters()?.len(),
                        "too many brand arguments",
                    )?;
                    let mut arguments = Vec::with_capacity(values.len() as usize);
                    for binding in values {
                        let t = if let brand::binding::Type(t) = binding.which()? {
                            let t = scope.unwrap_or(&default_scope).type_inner(t?, depth + 1)?;
                            require(t.is_pointer(), "generic argument must be a pointer")?;
                            t
                        } else {
                            Type::AnyPointer(PointerKind::Any)
                        };
                        arguments.push(t);
                    }
                    BrandArguments::bound(id, arguments)
                }
                brand::scope::Inherit(()) => scope.unwrap_or(&default_scope).arguments_at_scope(id),
            };
            require(
                bindings.insert(id, arguments).is_none(),
                "duplicate brand scope",
            )?;
        }
        let mut result = self.generic();
        result.bindings = Rc::new(bindings);
        Ok(result)
    }
    pub fn get_type(&self, t: type_::Reader<'_>) -> Result<Type<'a>> {
        self.type_inner(t, 0)
    }
    fn type_inner(&self, t: type_::Reader<'_>, depth: usize) -> Result<Type<'a>> {
        require(depth < 64, "schema type nesting limit")?;
        let which = match t.which() {
            Ok(t) => t,
            Err(tag) => return Ok(Type::Unknown(tag.0)),
        };
        Ok(match which {
            type_::Void(()) => Type::Void,
            type_::Bool(()) => Type::Bool,
            type_::Int8(()) => Type::Int8,
            type_::Int16(()) => Type::Int16,
            type_::Int32(()) => Type::Int32,
            type_::Int64(()) => Type::Int64,
            type_::Uint8(()) => Type::UInt8,
            type_::Uint16(()) => Type::UInt16,
            type_::Uint32(()) => Type::UInt32,
            type_::Uint64(()) => Type::UInt64,
            type_::Float32(()) => Type::Float32,
            type_::Float64(()) => Type::Float64,
            type_::Text(()) => Type::Text,
            type_::Data(()) => Type::Data,
            type_::Enum(t) => Type::Enum(self.loader.get(t.get_type_id())?.bind_inner(
                t.get_brand()?,
                Some(self),
                depth + 1,
            )?),
            type_::Struct(t) => Type::Struct(self.loader.get(t.get_type_id())?.bind_inner(
                t.get_brand()?,
                Some(self),
                depth + 1,
            )?),
            type_::Interface(t) => Type::Interface(self.loader.get(t.get_type_id())?.bind_inner(
                t.get_brand()?,
                Some(self),
                depth + 1,
            )?),
            type_::List(l) => {
                Type::List(Box::new(self.type_inner(l.get_element_type()?, depth + 1)?))
            }
            type_::AnyPointer(p) => match p.which()? {
                type_::any_pointer::Unconstrained(p) => Type::AnyPointer(match p.which()? {
                    type_::any_pointer::unconstrained::AnyKind(()) => PointerKind::Any,
                    type_::any_pointer::unconstrained::Struct(()) => PointerKind::Struct,
                    type_::any_pointer::unconstrained::List(()) => PointerKind::List,
                    type_::any_pointer::unconstrained::Capability(()) => PointerKind::Capability,
                }),
                type_::any_pointer::Parameter(p) => self
                    .arguments_at_scope(p.get_scope_id())
                    .get(p.get_parameter_index()),
                type_::any_pointer::ImplicitMethodParameter(p) => {
                    self.arguments_at_scope(0).get(p.get_parameter_index())
                }
            },
        })
    }
    pub fn fields(&self) -> Result<Vec<Field<'a>>> {
        let node::Struct(s) = self.get_proto().which()? else {
            return Err(invalid("not a struct schema"));
        };
        Ok((0..s.get_fields()?.len())
            .map(|index| Field {
                parent: self.clone(),
                index: u16::try_from(index).expect("validated member count"),
            })
            .collect())
    }
    pub fn field(&self, name: &str) -> Result<Field<'a>> {
        self.find_field(name)?
            .ok_or_else(|| invalid("schema field not found"))
    }
    pub fn find_field(&self, name: &str) -> Result<Option<Field<'a>>> {
        for f in self.fields()? {
            if f.get_proto().get_name()?.to_str()? == name {
                return Ok(Some(f));
            }
        }
        Ok(None)
    }
    /// Members of this struct's unnamed union, in ordinal order. Groups are
    /// returned as fields; their children belong to the group's schema.
    pub fn union_fields(&self) -> Result<Vec<Field<'a>>> {
        Ok(self
            .fields()?
            .into_iter()
            .filter(|f| f.get_proto().get_discriminant_value() != field::NO_DISCRIMINANT)
            .collect())
    }
    pub fn non_union_fields(&self) -> Result<Vec<Field<'a>>> {
        Ok(self
            .fields()?
            .into_iter()
            .filter(|f| f.get_proto().get_discriminant_value() == field::NO_DISCRIMINANT)
            .collect())
    }
    /// Unknown tags, including the non-union marker, have no matching arm.
    pub fn field_by_discriminant(&self, discriminant: u16) -> Result<Option<Field<'a>>> {
        let node::Struct(s) = self.get_proto().which()? else {
            return Err(invalid("not a struct schema"));
        };
        if discriminant >= s.get_discriminant_count() {
            return Ok(None);
        }
        Ok(self
            .fields()?
            .into_iter()
            .find(|f| f.get_proto().get_discriminant_value() == discriminant))
    }
    pub fn methods(&self) -> Result<Vec<Method<'a>>> {
        let node::Interface(s) = self.get_proto().which()? else {
            return Err(invalid("not an interface schema"));
        };
        Ok((0..s.get_methods()?.len())
            .map(|index| Method {
                parent: self.clone(),
                index: u16::try_from(index).expect("validated member count"),
                implicit: Rc::new(Vec::new()),
            })
            .collect())
    }
    pub fn superclasses(&self) -> Result<Vec<Self>> {
        let node::Interface(s) = self.get_proto().which()? else {
            return Err(invalid("not an interface schema"));
        };
        s.get_superclasses()?
            .iter()
            .map(|s| {
                self.loader
                    .get(s.get_id())?
                    .bind(s.get_brand()?, Some(self))
            })
            .collect()
    }
    pub fn method(&self, name: &str) -> Result<Method<'a>> {
        self.find_method(name)?
            .ok_or_else(|| invalid("schema method not found"))
    }
    /// Search this interface, then superclasses in declaration order, depth first.
    /// Missing members return None; malformed or excessive inheritance is an error.
    pub fn find_method(&self, name: &str) -> Result<Option<Method<'a>>> {
        self.find_in_hierarchy(&mut 64, &mut |schema| {
            for m in schema.methods()? {
                if m.get_proto().get_name()?.as_bytes() == name.as_bytes() {
                    return Ok(Some(m));
                }
            }
            Ok(None)
        })
    }
    /// Find this interface or its first transitive superclass with the given ID,
    /// preserving the generic arguments applied along that inheritance path.
    pub fn find_superclass(&self, id: u64) -> Result<Option<Self>> {
        self.find_in_hierarchy(&mut 64, &mut |schema| {
            Ok((schema.id == id).then(|| schema.clone()))
        })
    }
    pub fn extends(&self, target: &Self) -> Result<bool> {
        Ok(self
            .find_in_hierarchy(&mut 64, &mut |schema| {
                Ok(schema.equals(target).then_some(()))
            })?
            .is_some())
    }
    fn find_in_hierarchy<T>(
        &self,
        budget: &mut usize,
        visit: &mut impl FnMut(&Self) -> Result<Option<T>>,
    ) -> Result<Option<T>> {
        require(*budget > 0, "interface inheritance traversal limit")?;
        *budget -= 1;
        let node::Interface(interface) = self.get_proto().which()? else {
            return Err(invalid("not an interface schema"));
        };
        if let Some(found) = visit(self)? {
            return Ok(Some(found));
        }
        // Resolve dependencies only when visited. An error in an unused later
        // branch must not hide an earlier match, and a visited error must not be
        // mistaken for absence. One budget covers the entire search, including
        // repeated visits through diamonds rather than just recursive depth.
        for parent in interface.get_superclasses()? {
            let schema = self
                .loader
                .get(parent.get_id())?
                .bind(parent.get_brand()?, Some(self))?;
            if let Some(found) = schema.find_in_hierarchy(budget, visit)? {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }
    /// Conservative capability hint. Unknown placeholders and generic pointer
    /// fields may contain authority; cycles without such fields do not.
    pub fn may_contain_capabilities(&self) -> Result<bool> {
        fn ty(
            loader: &SchemaLoader,
            t: type_::Reader<'_>,
            seen: &mut BTreeSet<u64>,
            depth: usize,
        ) -> Result<bool> {
            if depth >= 64 {
                return Ok(true);
            }
            let which = match t.which() {
                Ok(t) => t,
                Err(_) => return Ok(true),
            };
            Ok(match which {
                type_::Interface(_) | type_::AnyPointer(_) => true,
                type_::Struct(s) => node(loader, s.get_type_id(), seen, depth + 1)?,
                type_::List(l) => ty(loader, l.get_element_type()?, seen, depth + 1)?,
                _ => false,
            })
        }
        fn node(
            loader: &SchemaLoader,
            id: u64,
            seen: &mut BTreeSet<u64>,
            depth: usize,
        ) -> Result<bool> {
            if depth >= 64 {
                return Ok(true);
            }
            let entry = &loader.entries[&id];
            if entry.stub {
                return Ok(true);
            }
            if !seen.insert(id) {
                return Ok(false);
            }
            let node::Struct(s) = entry.proto().which()? else {
                return Ok(true);
            };
            for f in s.get_fields()? {
                let yes = match f.which()? {
                    field::Slot(s) => ty(loader, s.get_type()?, seen, depth + 1)?,
                    field::Group(g) => node(loader, g.get_type_id(), seen, depth + 1)?,
                };
                if yes {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        node(self.loader, self.id, &mut BTreeSet::new(), 0)
    }
    pub(crate) fn check_native<T: crate::traits::OwnedStruct>(&self) -> Result<()> {
        Type::Struct(self.clone()).require_usable_as::<T>()
    }
    pub(crate) fn struct_size(&self) -> Result<crate::private::layout::StructSize> {
        let node::Struct(s) = self.get_proto().which()? else {
            return Err(invalid("not a struct schema"));
        };
        Ok(crate::private::layout::StructSize {
            data: s.get_data_word_count(),
            pointers: s.get_pointer_count(),
        })
    }
}
#[derive(Clone)]
pub struct Field<'a> {
    parent: Schema<'a>,
    index: u16,
}
impl<'a> Field<'a> {
    /// Snapshot the owning branded schema and field index into an `Eq + Hash` key.
    pub fn identity(&self) -> Result<crate::schema::MemberIdentity<'a>> {
        Ok(self
            .parent
            .identity()?
            .member(crate::schema::MemberKind::Field, self.index))
    }
    pub fn parent(&self) -> Schema<'a> {
        self.parent.clone()
    }
    pub fn index(&self) -> u16 {
        self.index
    }
    pub fn get_proto(&self) -> field::Reader<'a> {
        let node::Struct(s) = self.parent.get_proto().which().unwrap() else {
            unreachable!()
        };
        s.get_fields().unwrap().get(self.index as u32)
    }
    pub fn get_type(&self) -> Result<Type<'a>> {
        match self.get_proto().which()? {
            field::Slot(s) => self.parent.get_type(s.get_type()?),
            field::Group(g) => {
                let mut s = self.parent.loader.get(g.get_type_id())?;
                s.bindings = self.parent.bindings.clone();
                s.unbound = self.parent.unbound;
                Ok(Type::Struct(s))
            }
        }
    }
}
#[derive(Clone)]
pub struct Method<'a> {
    parent: Schema<'a>,
    index: u16,
    implicit: Rc<Vec<Type<'a>>>,
}
impl<'a> Method<'a> {
    /// Identify the declaration in its branded parent. Implicit method argument
    /// bindings specialize a use of the declaration and are not part of this key.
    pub fn identity(&self) -> Result<crate::schema::MemberIdentity<'a>> {
        Ok(self
            .parent
            .identity()?
            .member(crate::schema::MemberKind::Method, self.index))
    }
    pub fn parent(&self) -> Schema<'a> {
        self.parent.clone()
    }
    pub fn index(&self) -> u16 {
        self.index
    }
    pub fn get_proto(&self) -> crate::schema_capnp::method::Reader<'a> {
        let node::Interface(s) = self.parent.get_proto().which().unwrap() else {
            unreachable!()
        };
        s.get_methods().unwrap().get(self.index as u32)
    }
    pub fn params(&self) -> Result<Schema<'a>> {
        let m = self.get_proto();
        self.method_struct(m.get_param_struct_type(), m.get_param_brand()?)
    }
    pub fn results(&self) -> Result<Schema<'a>> {
        let m = self.get_proto();
        self.method_struct(m.get_result_struct_type(), m.get_result_brand()?)
    }
    fn scope(&self) -> Result<Schema<'a>> {
        let mut scope = self.parent.clone();
        let mut bindings = (*scope.bindings).clone();
        if !self.implicit.is_empty() {
            bindings.insert(0, BrandArguments::bound(0, (*self.implicit).clone()));
        }
        scope.bindings = Rc::new(bindings);
        Ok(scope)
    }
    fn method_struct(&self, id: u64, brand: brand::Reader<'_>) -> Result<Schema<'a>> {
        let scope = self.scope()?;
        let mut result = self.parent.loader.get(id)?.bind(brand, Some(&scope))?;
        // Anonymous method structs declare implicit method parameters as their
        // own ordinary parameters. Explicit result structs use resultBrand.
        if result.get_proto().get_scope_id() == 0 && !self.implicit.is_empty() {
            let mut bindings = (*result.bindings).clone();
            require(
                self.implicit.len() <= result.get_proto().get_parameters()?.len() as usize,
                "anonymous method parameter count",
            )?;
            bindings.insert(id, BrandArguments::bound(id, (*self.implicit).clone()));
            result.bindings = Rc::new(bindings);
        }
        Ok(result)
    }
    pub fn bind_implicit(&self, arguments: &[Type<'a>]) -> Result<Self> {
        require(
            arguments.len() == self.get_proto().get_implicit_parameters()?.len() as usize,
            "method generic argument count",
        )?;
        for t in arguments {
            require(t.is_pointer(), "generic argument must be a pointer")?;
        }
        let mut result = self.clone();
        result.implicit = Rc::new(arguments.to_vec());
        Ok(result)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerKind {
    Any,
    Struct,
    List,
    Capability,
}
#[derive(Clone)]
pub enum Type<'a> {
    Unknown(u16),
    Void,
    Bool,
    Int8,
    Int16,
    Int32,
    Int64,
    UInt8,
    UInt16,
    UInt32,
    UInt64,
    Float32,
    Float64,
    Text,
    Data,
    Enum(Schema<'a>),
    Struct(Schema<'a>),
    Interface(Schema<'a>),
    List(Box<Type<'a>>),
    AnyPointer(PointerKind),
    Parameter(u64, u16),
}
impl PartialEq for Type<'_> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Enum(a), Self::Enum(b))
            | (Self::Struct(a), Self::Struct(b))
            | (Self::Interface(a), Self::Interface(b)) => a.equals(b),
            (Self::List(a), Self::List(b)) => a == b,
            (Self::Unknown(a), Self::Unknown(b)) => a == b,
            (Self::AnyPointer(a), Self::AnyPointer(b)) => a == b,
            (Self::Parameter(a, b), Self::Parameter(c, d)) => a == c && b == d,
            _ => core::mem::discriminant(self) == core::mem::discriminant(other),
        }
    }
}
impl Type<'_> {
    fn write_identity(&self, key: &mut crate::schema::identity::Builder) -> Result<()> {
        use crate::schema::identity::Part;
        let mut ty = self;
        let mut lists = 0;
        while let Self::List(inner) = ty {
            key.spend()?;
            lists += 1;
            ty = inner;
        }
        key.spend()?;
        key.parts.push(Part::Lists(lists));
        let tag = match ty {
            Self::Void => 0,
            Self::Bool => 1,
            Self::Int8 => 2,
            Self::Int16 => 3,
            Self::Int32 => 4,
            Self::Int64 => 5,
            Self::UInt8 => 6,
            Self::UInt16 => 7,
            Self::UInt32 => 8,
            Self::UInt64 => 9,
            Self::Float32 => 10,
            Self::Float64 => 11,
            Self::Text => 12,
            Self::Data => 13,
            Self::Enum(schema) | Self::Struct(schema) | Self::Interface(schema) => {
                key.parts.push(Part::Type(match ty {
                    Self::Enum(_) => 16,
                    Self::Struct(_) => 17,
                    _ => 18,
                }));
                return schema.write_identity(key);
            }
            Self::AnyPointer(kind) => match kind {
                PointerKind::Any => 20,
                PointerKind::Struct => 21,
                PointerKind::List => 22,
                PointerKind::Capability => 23,
            },
            Self::Parameter(scope, index) => {
                key.parts.push(Part::Parameter(*scope, *index));
                return Ok(());
            }
            Self::Unknown(tag) => {
                key.parts.push(Part::Unknown(*tag));
                return Ok(());
            }
            Self::List(_) => unreachable!("list layers removed"),
        };
        key.parts.push(Part::Type(tag));
        Ok(())
    }
    pub fn is_pointer(&self) -> bool {
        matches!(
            self,
            Self::Text
                | Self::Data
                | Self::Struct(_)
                | Self::Interface(_)
                | Self::List(_)
                | Self::AnyPointer(_)
                | Self::Parameter(..)
        )
    }
    pub(crate) fn element_size(&self) -> crate::private::layout::ElementSize {
        use crate::private::layout::ElementSize as E;
        match self {
            Self::Unknown(_) | Self::Void => E::Void,
            Self::Bool => E::Bit,
            Self::Int8 | Self::UInt8 => E::Byte,
            Self::Int16 | Self::UInt16 | Self::Enum(_) => E::TwoBytes,
            Self::Int32 | Self::UInt32 | Self::Float32 => E::FourBytes,
            Self::Int64 | Self::UInt64 | Self::Float64 => E::EightBytes,
            Self::Struct(_) => E::InlineComposite,
            _ => E::Pointer,
        }
    }
}
