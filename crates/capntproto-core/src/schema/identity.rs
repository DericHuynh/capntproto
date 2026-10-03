//! Snapshot keys for reflection caches. Addresses identify metadata owners only;
//! they are process-local and must never be used as persistent or wire IDs.
use crate::{introspect, schema_capnp::node, Result};
use alloc::vec::Vec;
use core::marker::PhantomData;

/// Identity of one schema instance and its applied generic arguments.
///
/// Keys implement `Eq` and `Hash` without evaluating metadata callbacks. Construct
/// a key with a schema's `identity()` method, then use it in a map or set. Key
/// construction permits at most 128 metadata visits and is fallible; incomplete
/// generic struct metadata without an explicit brand is rejected. Compiled
/// enums erase enclosing brands, including when their node is marked generic,
/// matching native enum types and compiled reflection in C++. Loaded enum keys
/// retain the applied brand.
/// Unlike nominal type equality, identity distinguishes metadata owners even when
/// their type IDs agree. Loaded keys borrow their loader and prevent replacement
/// of the schemas while the keys remain in use. Compiled and loaded keys belong
/// to separate identity domains, including after native type registration.
///
/// Hash values and key representations are process-local, not serialization IDs.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SchemaIdentity<'a> {
    pub(super) parts: Vec<Part>,
    pub(super) node_id: u64,
    pub(super) owner: PhantomData<&'a ()>,
}
impl SchemaIdentity<'_> {
    pub fn node_id(&self) -> u64 {
        self.node_id
    }
}
impl<'a> SchemaIdentity<'a> {
    pub(crate) fn member(self, kind: MemberKind, index: u16) -> MemberIdentity<'a> {
        MemberIdentity {
            parent: self,
            kind,
            index,
        }
    }
}

/// The kind of declaration identified by a member key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MemberKind {
    Field,
    Method,
    Enumerant,
}

/// Identity of a member declaration: its owning branded schema, kind and index.
/// Inherited methods use the declaring interface. Binding implicit method
/// arguments changes a call's types, not the identity of the method declaration.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MemberIdentity<'a> {
    parent: SchemaIdentity<'a>,
    kind: MemberKind,
    index: u16,
}
impl<'a> MemberIdentity<'a> {
    pub fn parent(&self) -> &SchemaIdentity<'a> {
        &self.parent
    }
    pub fn kind(&self) -> MemberKind {
        self.kind
    }
    pub fn index(&self) -> u16 {
        self.index
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Part {
    Compiled {
        address: usize,
        length: usize,
        id: u64,
        kind: u8,
    },
    Loaded {
        address: usize,
        id: u64,
        unbound: bool,
    },
    Scope {
        id: u64,
        unbound: bool,
    },
    Length(usize),
    Type(u16),
    Lists(usize),
    Parameter(u64, u16),
    Unknown(u16),
}
pub(crate) struct Builder {
    pub(crate) parts: Vec<Part>,
    remaining: usize,
}
impl Builder {
    pub(crate) fn new() -> Self {
        Self {
            parts: Vec::new(),
            remaining: 128,
        }
    }
    pub(crate) fn spend(&mut self) -> Result<()> {
        introspect::spend_budget(&mut self.remaining)
    }
    pub(crate) fn finish<'a>(self, node_id: u64) -> SchemaIdentity<'a> {
        SchemaIdentity {
            parts: self.parts,
            node_id,
            owner: PhantomData,
        }
    }
    fn compiled_brand(&mut self, brand: introspect::Brand) -> Result<()> {
        self.parts.push(Part::Length(usize::from(brand.len())));
        for index in 0..brand.len() {
            self.compiled_type(brand.get(index))?;
        }
        Ok(())
    }
    pub(crate) fn compiled_type(&mut self, ty: introspect::Type) -> Result<()> {
        use introspect::TypeVariant as T;
        self.spend()?;
        let (lists, ty) = ty.identity_base();
        self.parts.push(Part::Lists(lists));
        let kind = match ty.which() {
            T::Void => 0,
            T::Bool => 1,
            T::Int8 => 2,
            T::Int16 => 3,
            T::Int32 => 4,
            T::Int64 => 5,
            T::UInt8 => 6,
            T::UInt16 => 7,
            T::UInt32 => 8,
            T::UInt64 => 9,
            T::Float32 => 10,
            T::Float64 => 11,
            T::Text => 12,
            T::Data => 13,
            T::AnyPointer => 14,
            T::Capability => 15,
            T::Struct(raw) => {
                let schema = ty.as_struct_schema()?;
                let proto = schema.get_proto();
                let node::Struct(_) = proto.which()? else {
                    return Err(missing_metadata());
                };
                let brand = schema
                    .brand
                    .or_else(|| (!proto.get_is_generic()).then_some(introspect::Brand::EMPTY))
                    .ok_or_else(missing_metadata)?;
                self.parts.push(Part::Compiled {
                    address: raw.generic as *const _ as usize,
                    length: 0,
                    id: proto.get_id(),
                    kind: 0,
                });
                return self.compiled_brand(brand);
            }
            T::Interface(raw) => {
                let proto = super::InterfaceSchema::new(raw).get_proto();
                let node::Interface(_) = proto.which()? else {
                    return Err(missing_metadata());
                };
                self.parts.push(Part::Compiled {
                    address: raw.arena as *const _ as usize,
                    length: 0,
                    id: proto.get_id(),
                    kind: 1,
                });
                return self.compiled_brand(raw.brand);
            }
            T::Enum(raw) => {
                let proto = super::EnumSchema::new(raw).get_proto();
                let node::Enum(_) = proto.which()? else {
                    return Err(missing_metadata());
                };
                // C++ compiled enums (including reflected fields/list elements)
                // use the default brand even in generic scopes. The node's
                // isGeneric bit describes its scope, not a branded native enum.
                self.parts.push(Part::Compiled {
                    address: raw.encoded_node.as_ptr() as usize,
                    length: raw.encoded_node.len(),
                    id: proto.get_id(),
                    kind: 2,
                });
                return Ok(());
            }
            T::List(_) => unreachable!("list layers removed"),
        };
        self.parts.push(Part::Type(kind));
        Ok(())
    }
}
fn missing_metadata() -> crate::Error {
    let mut error = crate::Error::from_kind(crate::ErrorKind::TypeMismatch);
    write!(error, "incomplete schema identity metadata");
    error
}
