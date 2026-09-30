//! Traits and types to support run-time type introspection, i.e. reflection.

use crate::private::layout::ElementSize;
use crate::schema::{EnumSchema, StructSchema};

/// A type that supports reflection. All types that can appear in a Cap'n Proto message
/// implement this trait.
pub trait Introspect {
    /// Retrieves a description of the type.
    fn introspect() -> Type;
}

/// A description of a Cap'n Proto type. The representation is
/// optimized to avoid heap allocation.
///
/// To examine a `Type`, you should call the `which()` method.
#[derive(Copy, Clone, Debug)]
pub struct Type {
    /// The type, minus any outer `List( )`.
    base: BaseType,

    /// How many times `base` is wrapped in `List( )`.
    list_count: usize,

    // Explicit generic arguments from regenerated bindings; old generic metadata
    // has no complete brand and is rejected by strict comparisons.
    brand: Option<Brand>,
}

impl Type {
    #[cfg(feature = "alloc")]
    pub(crate) fn identity_base(mut self) -> (usize, Self) {
        let lists = self.list_count;
        self.list_count = 0;
        (lists, self)
    }
    /// Constructs a new `Type` that is not a list.
    fn new_base(base: BaseType) -> Self {
        Self {
            base,
            list_count: 0,
            brand: None,
        }
    }

    /// Constructs a new `Type` that is a list wrapping some other `Type`.
    pub fn list_of(mut element_type: Type) -> Self {
        element_type.list_count += 1;
        element_type
    }

    /// Unfolds a single layer of the `Type`, to allow for pattern matching.
    pub fn which(&self) -> TypeVariant {
        if self.list_count > 0 {
            TypeVariant::List(Type {
                base: self.base,
                list_count: self.list_count - 1,
                brand: self.brand,
            })
        } else {
            match self.base {
                BaseType::Void => TypeVariant::Void,
                BaseType::Bool => TypeVariant::Bool,
                BaseType::Int8 => TypeVariant::Int8,
                BaseType::Int16 => TypeVariant::Int16,
                BaseType::Int32 => TypeVariant::Int32,
                BaseType::Int64 => TypeVariant::Int64,
                BaseType::UInt8 => TypeVariant::UInt8,
                BaseType::UInt16 => TypeVariant::UInt16,
                BaseType::UInt32 => TypeVariant::UInt32,
                BaseType::UInt64 => TypeVariant::UInt64,
                BaseType::Float32 => TypeVariant::Float32,
                BaseType::Float64 => TypeVariant::Float64,
                BaseType::Text => TypeVariant::Text,
                BaseType::Data => TypeVariant::Data,
                BaseType::Enum(re) => TypeVariant::Enum(re),
                BaseType::Struct(rs) => TypeVariant::Struct(rs),
                BaseType::AnyPointer => TypeVariant::AnyPointer,
                BaseType::Capability => TypeVariant::Capability,
                BaseType::Interface(schema) => TypeVariant::Interface(schema),
            }
        }
    }

    /// Attach compiled generic arguments without comparing function addresses.
    pub fn with_brand(mut self, brand: Brand) -> Self {
        self.brand = Some(brand);
        self
    }

    /// Preserve generic arguments while exposing a reflected struct schema.
    pub fn as_struct_schema(self) -> crate::Result<StructSchema> {
        let TypeVariant::Struct(raw) = self.which() else {
            return Err(crate::Error::from_kind(crate::ErrorKind::TypeMismatch));
        };
        let mut schema = StructSchema::new(raw);
        schema.brand = self.brand;
        Ok(schema)
    }

    /// Nominal type equality, including nested generic arguments. Unknown brands
    /// from older generated generic bindings fail closed. The traversal bound
    /// also prevents malformed custom metadata from recursing indefinitely.
    pub fn equals(self, other: Self) -> crate::Result<bool> {
        self.equals_with_budget(other, &mut 128)
    }
    pub(crate) fn equals_with_budget(self, other: Self, budget: &mut usize) -> crate::Result<bool> {
        spend_budget(budget)?;
        if self.list_count != other.list_count {
            return Ok(false);
        }
        // Compare the base directly: deeply nested List layers do not recurse.
        match (self.base, other.base) {
            (BaseType::Struct(a), BaseType::Struct(b)) => {
                let a = StructSchema::new(a);
                let b = StructSchema::new(b);
                if a.get_proto().get_id() != b.get_proto().get_id() {
                    return Ok(false);
                }
                let a = self
                    .brand
                    .or_else(|| (!a.get_proto().get_is_generic()).then_some(Brand::EMPTY));
                let b = other
                    .brand
                    .or_else(|| (!b.get_proto().get_is_generic()).then_some(Brand::EMPTY));
                match (a, b) {
                    (Some(a), Some(b)) => a.equals(b, budget),
                    _ => Ok(false),
                }
            }
            (BaseType::Interface(a), BaseType::Interface(b)) => {
                let left = crate::schema::InterfaceSchema::new(a);
                let right = crate::schema::InterfaceSchema::new(b);
                if left.get_proto().get_id() != right.get_proto().get_id() {
                    return Ok(false);
                }
                a.brand.equals(b.brand, budget)
            }
            (BaseType::Interface(_), BaseType::Capability)
            | (BaseType::Capability, BaseType::Interface(_)) => Ok(false),
            (BaseType::Enum(a), BaseType::Enum(b)) => {
                let a: EnumSchema = a.into();
                let b: EnumSchema = b.into();
                Ok(a.get_proto().get_id() == b.get_proto().get_id())
            }
            _ => {
                let mut a = self;
                let mut b = other;
                a.list_count = 0;
                b.list_count = 0;
                Ok(a.loose_equals(b))
            }
        }
    }

    /// If this type T appears as List(T), then what is the expected
    /// element size of the list?
    pub(crate) fn expected_element_size(&self) -> ElementSize {
        if self.list_count > 0 {
            ElementSize::Pointer
        } else {
            match self.base {
                BaseType::Void => ElementSize::Void,
                BaseType::Bool => ElementSize::Bit,
                BaseType::Int8 | BaseType::UInt8 => ElementSize::Byte,
                BaseType::Int16 | BaseType::UInt16 | BaseType::Enum(_) => ElementSize::TwoBytes,
                BaseType::Int32 | BaseType::UInt32 | BaseType::Float32 => ElementSize::FourBytes,
                BaseType::Int64 | BaseType::UInt64 | BaseType::Float64 => ElementSize::EightBytes,
                BaseType::Text
                | BaseType::Data
                | BaseType::AnyPointer
                | BaseType::Capability
                | BaseType::Interface(_) => ElementSize::Pointer,
                BaseType::Struct(_) => ElementSize::InlineComposite,
            }
        }
    }

    /// Is the `Type` a pointer type?
    pub fn is_pointer_type(&self) -> bool {
        if self.list_count > 0 {
            true
        } else {
            matches!(
                self.base,
                BaseType::Text
                    | BaseType::Data
                    | BaseType::AnyPointer
                    | BaseType::Struct(_)
                    | BaseType::Capability
                    | BaseType::Interface(_)
            )
        }
    }

    /// Returns true if `self` is equal to `other` modulo
    /// type parameters. Named interfaces must still have the same ID;
    /// inheritance and typeless capabilities are not native type aliases.
    pub fn loose_equals(&self, other: Self) -> bool {
        if self.list_count != other.list_count {
            return false;
        }
        let mut left = *self;
        let mut right = other;
        left.list_count = 0;
        right.list_count = 0;
        match (left.which(), right.which()) {
            (TypeVariant::Void, TypeVariant::Void) => true,
            (TypeVariant::Bool, TypeVariant::Bool) => true,
            (TypeVariant::UInt8, TypeVariant::UInt8) => true,
            (TypeVariant::UInt16, TypeVariant::UInt16) => true,
            (TypeVariant::UInt32, TypeVariant::UInt32) => true,
            (TypeVariant::UInt64, TypeVariant::UInt64) => true,
            (TypeVariant::Int8, TypeVariant::Int8) => true,
            (TypeVariant::Int16, TypeVariant::Int16) => true,
            (TypeVariant::Int32, TypeVariant::Int32) => true,
            (TypeVariant::Int64, TypeVariant::Int64) => true,
            (TypeVariant::Float32, TypeVariant::Float32) => true,
            (TypeVariant::Float64, TypeVariant::Float64) => true,
            (TypeVariant::Text, TypeVariant::Text) => true,
            (TypeVariant::Data, TypeVariant::Data) => true,
            (TypeVariant::Enum(es1), TypeVariant::Enum(es2)) => es1 == es2,
            (TypeVariant::Struct(rbs1), TypeVariant::Struct(rbs2)) => {
                // Ignore any type parameters. The original intent was that
                // we would additionally check that the `field_types` fields
                // were equal function pointers here. However, according to
                // Miri's behavior at least, that check returns `false`
                // more than we would like it to. So we settle for being
                // a bit more accepting.
                core::ptr::eq(rbs1.generic, rbs2.generic)
            }
            (TypeVariant::AnyPointer, TypeVariant::AnyPointer) => true,
            (TypeVariant::Capability, TypeVariant::Capability) => true,
            (TypeVariant::Interface(a), TypeVariant::Interface(b)) => {
                crate::schema::InterfaceSchema::new(a).get_proto().get_id()
                    == crate::schema::InterfaceSchema::new(b).get_proto().get_id()
            }
            _ => false,
        }
    }
}

#[derive(Copy, Clone)]
/// A `Type` unfolded one level. Suitable for pattern matching. Can be trivially
/// converted to `Type` via the `From`/`Into` traits.
pub enum TypeVariant {
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
    Struct(RawBrandedStructSchema),
    AnyPointer,
    Capability,
    Interface(RawBrandedInterfaceSchema),
    Enum(RawEnumSchema),
    List(Type),
}

impl TypeVariant {
    pub fn into_type_with_brand(self, brand: Brand) -> Type {
        Type::from(self).with_brand(brand)
    }
}

impl From<TypeVariant> for Type {
    fn from(tv: TypeVariant) -> Type {
        match tv {
            TypeVariant::Void => Type::new_base(BaseType::Void),
            TypeVariant::Bool => Type::new_base(BaseType::Bool),
            TypeVariant::Int8 => Type::new_base(BaseType::Int8),
            TypeVariant::Int16 => Type::new_base(BaseType::Int16),
            TypeVariant::Int32 => Type::new_base(BaseType::Int32),
            TypeVariant::Int64 => Type::new_base(BaseType::Int64),
            TypeVariant::UInt8 => Type::new_base(BaseType::UInt8),
            TypeVariant::UInt16 => Type::new_base(BaseType::UInt16),
            TypeVariant::UInt32 => Type::new_base(BaseType::UInt32),
            TypeVariant::UInt64 => Type::new_base(BaseType::UInt64),
            TypeVariant::Float32 => Type::new_base(BaseType::Float32),
            TypeVariant::Float64 => Type::new_base(BaseType::Float64),
            TypeVariant::Text => Type::new_base(BaseType::Text),
            TypeVariant::Data => Type::new_base(BaseType::Data),
            TypeVariant::Struct(rbs) => Type::new_base(BaseType::Struct(rbs)),
            TypeVariant::AnyPointer => Type::new_base(BaseType::AnyPointer),
            TypeVariant::Capability => Type::new_base(BaseType::Capability),
            TypeVariant::Interface(schema) => Type::new_base(BaseType::Interface(schema)),
            TypeVariant::Enum(es) => Type::new_base(BaseType::Enum(es)),
            TypeVariant::List(list) => Type::list_of(list),
        }
    }
}

/// A Cap'n Proto type, excluding `List`.
#[derive(Copy, Clone, Debug)]
enum BaseType {
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
    Struct(RawBrandedStructSchema),
    AnyPointer,
    Capability,
    Interface(RawBrandedInterfaceSchema),
    Enum(RawEnumSchema),
}

macro_rules! primitive_introspect(
    ($t:ty, $v:ident) => (
        impl Introspect for $t {
            fn introspect() -> Type { Type::new_base(BaseType::$v) }
        }
    )
);

primitive_introspect!((), Void);
primitive_introspect!(bool, Bool);
primitive_introspect!(i8, Int8);
primitive_introspect!(i16, Int16);
primitive_introspect!(i32, Int32);
primitive_introspect!(i64, Int64);
primitive_introspect!(u8, UInt8);
primitive_introspect!(u16, UInt16);
primitive_introspect!(u32, UInt32);
primitive_introspect!(u64, UInt64);
primitive_introspect!(f32, Float32);
primitive_introspect!(f64, Float64);

/// Type information that gets included in the generated code for every
/// user-defined Cap'n Proto struct.
#[derive(Copy, Clone)]
pub struct RawStructSchema {
    /// The Node (as defined in schema.capnp), as a single segment message.
    pub(crate) arena: &'static crate::private::arena::GeneratedCodeArena,

    /// Indices (not ordinals) of fields that don't have a discriminant value.
    pub(crate) nonunion_members: &'static [u16],

    /// Map from discriminant value to field index.
    pub(crate) members_by_discriminant: &'static [u16],

    /// Indices of fields, sorted by their respective names.
    pub(crate) members_by_name: &'static [u16],
}

impl RawStructSchema {
    /// Constructs a new `RawStructSchema`.
    pub const fn new(
        arena: &'static crate::private::arena::GeneratedCodeArena,
        nonunion_members: &'static [u16],
        members_by_discriminant: &'static [u16],
        members_by_name: &'static [u16],
    ) -> Self {
        Self {
            arena,
            nonunion_members,
            members_by_discriminant,
            members_by_name,
        }
    }
}

/// A RawStructSchema with branding information, i.e. resolution of type parameters.
/// To use one of this, you will usually want to convert it to a `schema::StructSchema`,
/// which can be done via `into()`.
#[derive(Copy, Clone)]
pub struct RawBrandedStructSchema {
    /// The unbranded base schema.
    pub generic: &'static RawStructSchema,

    /// Map from field index (not ordinal) to Type.
    pub field_types: fn(u16) -> Type,

    /// Map from (maybe field index, annotation index) to the Type
    /// of the value held by that annotation.
    pub annotation_types: fn(Option<u16>, u32) -> Type,
}

impl core::fmt::Debug for RawBrandedStructSchema {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::result::Result<(), core::fmt::Error> {
        write!(
            f,
            "RawBrandedStructSchema({:?}, {:?})",
            self.generic as *const _, self.field_types as *const fn(u16) -> Type
        )
    }
}

impl From<StructSchema> for RawBrandedStructSchema {
    fn from(value: StructSchema) -> Self {
        value.raw
    }
}

/// Type information that gets included in the generated code for every
/// user-defined Cap'n Proto enum.
///
/// Compiled enum types erase enclosing generic parameters, as in C++. A node
/// can be marked generic while its native enum still uses the default brand.
/// Runtime-loaded enum schemas separately retain their wire brands.
///
/// To use one of these, you will usually want to convert it to a `schema::EnumSchema`,
/// which can be done via `into()`.
#[derive(Clone, Copy)]
pub struct RawEnumSchema {
    /// The Node (as defined in schema.capnp), as a single segment message.
    pub encoded_node: &'static [crate::Word],

    /// Map from (maybe enumerant index, annotation index) to the Type
    /// of the value held by that annotation.
    pub annotation_types: fn(Option<u16>, u32) -> Type,
}

impl core::cmp::PartialEq for RawEnumSchema {
    fn eq(&self, other: &Self) -> bool {
        ::core::ptr::eq(self.encoded_node, other.encoded_node)
    }
}

impl core::cmp::Eq for RawEnumSchema {}

impl core::fmt::Debug for RawEnumSchema {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::result::Result<(), core::fmt::Error> {
        write!(f, "RawEnumSchema({:?})", self.encoded_node as *const _)
    }
}

impl From<EnumSchema> for RawEnumSchema {
    fn from(value: EnumSchema) -> Self {
        value.raw
    }
}

/**
Function intended to be called by generated `get_field_types()` methods.
Defined here so that we can use inline format args syntax, which did
not exist before Rust edition 2021. Not intended to be called directly by
end users.
 */
pub fn panic_invalid_field_index(index: u16) -> ! {
    panic!("invalid field index {index}")
}

/**
Function intended to be called by generated `get_annotation_types()` methods.
Defined here so that we can use inline format args syntax, which did
not exist before Rust edition 2021. Not intended to be called directly by
end users.
 */
pub fn panic_invalid_annotation_indices(child_index: Option<u16>, index: u32) -> ! {
    panic!("invalid annotation indices ({child_index:?}, {index})")
}

/// Compiled interface metadata, with generic method and superclass types applied.
#[derive(Clone, Copy)]
pub struct RawBrandedInterfaceSchema {
    pub arena: &'static crate::private::arena::GeneratedCodeArena,
    pub method_types: fn(u16) -> MethodTypes,
    pub superclass: fn(u16) -> Type,
    pub brand: Brand,
}

#[derive(Clone, Copy, Debug)]
pub struct MethodTypes {
    pub params: Type,
    /// None denotes the built-in StreamResult.
    pub results: Option<Type>,
    pub no_promise_pipelining: bool,
}

impl core::fmt::Debug for RawBrandedInterfaceSchema {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("RawBrandedInterfaceSchema")
            .field(&(self.arena as *const _))
            .finish()
    }
}

/// Generic parameters in declaration order, including enclosing scopes. Metadata
/// is evaluated lazily so recursive message fields do not create a recursive walk.
#[derive(Clone, Copy)]
pub struct Brand {
    len: u16,
    parameter: fn(u16) -> Type,
}
impl Brand {
    pub const EMPTY: Self = Self::new(0, |index| panic_invalid_field_index(index));
    pub const fn new(len: u16, parameter: fn(u16) -> Type) -> Self {
        Self { len, parameter }
    }
    pub fn len(self) -> u16 {
        self.len
    }
    pub fn is_empty(self) -> bool {
        self.len == 0
    }
    pub fn get(self, index: u16) -> Type {
        assert!(index < self.len);
        (self.parameter)(index)
    }
    fn equals(self, other: Self, budget: &mut usize) -> crate::Result<bool> {
        if self.len != other.len {
            return Ok(false);
        }
        for index in 0..self.len {
            if !self
                .get(index)
                .equals_with_budget(other.get(index), budget)?
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
impl core::fmt::Debug for Brand {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Brand")
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}
pub(crate) fn spend_budget(budget: &mut usize) -> crate::Result<()> {
    *budget = budget
        .checked_sub(1)
        .ok_or_else(|| crate::Error::from_kind(crate::ErrorKind::TypeMismatch))?;
    Ok(())
}
