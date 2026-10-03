//! Convenience wrappers of the datatypes defined in schema.capnp.

use crate::dynamic_value;
use crate::introspect::{self, RawBrandedStructSchema, RawEnumSchema};
use crate::private::layout;
use crate::schema_capnp::{annotation, enumerant, field, node};
use crate::struct_list;
use crate::traits::{IndexMove, ListIter, ShortListIter};
use crate::Result;

#[cfg(feature = "alloc")]
pub(crate) mod identity;
#[cfg(feature = "alloc")]
pub use identity::{MemberIdentity, MemberKind, SchemaIdentity};

// Prefix lengths are byte offsets, as in schema.capnp and the C++ API. Keep
// invalid UTF-8 inspectable and report invalid offsets instead of panicking.
pub(crate) fn short_display_name(proto: node::Reader<'_>) -> Result<crate::text::Reader<'_>> {
    let name = proto.get_display_name()?;
    let bytes = name
        .as_bytes()
        .get(proto.get_display_name_prefix_length() as usize..)
        .ok_or_else(|| {
            let mut error = crate::Error::from_kind(crate::ErrorKind::Failed);
            write!(error, "schema display-name prefix out of bounds");
            error
        })?;
    Ok(bytes.into())
}

macro_rules! display_names {
    () => {
        /// The display name after its schema-supplied byte prefix.
        pub fn get_short_display_name(self) -> Result<crate::text::Reader<'static>> {
            short_display_name(self.get_proto())
        }
        /// Alias for `get_short_display_name()`, matching the C++ schema API.
        pub fn get_unqualified_name(self) -> Result<crate::text::Reader<'static>> {
            self.get_short_display_name()
        }
    };
}

/// A struct node, with generics applied.
#[derive(Clone, Copy)]
pub struct StructSchema {
    pub(crate) raw: RawBrandedStructSchema,
    pub(crate) brand: Option<introspect::Brand>,
    pub(crate) proto: node::Reader<'static>,
}

impl StructSchema {
    display_names!();
    pub fn new(raw: RawBrandedStructSchema) -> Self {
        let proto = crate::any_pointer::Reader::new(
            layout::PointerReader::get_root_from_arena(raw.generic.arena).unwrap(),
        )
        .get_as()
        .unwrap();
        Self {
            raw,
            proto,
            brand: None,
        }
    }

    pub fn as_type(self) -> introspect::Type {
        let ty = introspect::Type::from(introspect::TypeVariant::Struct(self.raw));
        match self.brand {
            Some(brand) => ty.with_brand(brand),
            None => ty,
        }
    }
    pub fn equals(self, other: Self) -> Result<bool> {
        self.as_type().equals(other.as_type())
    }
    /// Snapshot metadata ownership and the complete applied brand for use as a key.
    #[cfg(feature = "alloc")]
    pub fn identity(self) -> Result<SchemaIdentity<'static>> {
        let mut key = identity::Builder::new();
        key.compiled_type(self.as_type())?;
        Ok(key.finish(self.proto.get_id()))
    }

    pub fn get_proto(&self) -> node::Reader<'static> {
        self.proto
    }

    pub fn get_fields(self) -> crate::Result<FieldList> {
        if let node::Struct(s) = self.proto.which()? {
            Ok(FieldList {
                fields: s.get_fields()?,
                parent: self,
            })
        } else {
            panic!()
        }
    }

    pub fn get_field_by_discriminant(self, discriminant: u16) -> Result<Option<Field>> {
        match self
            .raw
            .generic
            .members_by_discriminant
            .get(discriminant as usize)
        {
            None => Ok(None),
            Some(&idx) => Ok(Some(self.get_fields()?.get(idx))),
        }
    }

    /// Looks up a field by name using binary search. Returns `None` if no matching field is found.
    pub fn find_field_by_name(&self, name: &str) -> Result<Option<Field>> {
        let fields = self.get_fields()?;
        let mut lower: usize = 0;
        let mut upper: usize = self.raw.generic.members_by_name.len();

        while lower < upper {
            let mid: usize = (lower + upper) / 2;
            let candidate_index = self.raw.generic.members_by_name[mid];
            let candidate_name = fields.get(candidate_index).get_proto().get_name()?;

            use core::cmp::Ordering;
            match (&name).partial_cmp(&candidate_name) {
                Some(Ordering::Equal) => return Ok(Some(fields.get(candidate_index))),
                Some(Ordering::Greater) => lower = mid + 1,
                Some(Ordering::Less) => upper = mid,
                None => unreachable!(),
            }
        }
        Ok(None)
    }

    /// Like `find_field_by_name()`, but returns an error if the field is not found.
    pub fn get_field_by_name(&self, name: &str) -> Result<Field> {
        if let Some(field) = self.find_field_by_name(name)? {
            Ok(field)
        } else {
            let mut error = crate::Error::from_kind(crate::ErrorKind::FieldNotFound);
            write!(error, "{name}");
            Err(error)
        }
    }

    pub fn get_union_fields(self) -> Result<FieldSubset> {
        if let node::Struct(s) = self.proto.which()? {
            Ok(FieldSubset {
                fields: s.get_fields()?,
                indices: self.raw.generic.members_by_discriminant,
                parent: self,
            })
        } else {
            panic!()
        }
    }

    pub fn get_non_union_fields(self) -> Result<FieldSubset> {
        if let node::Struct(s) = self.proto.which()? {
            Ok(FieldSubset {
                fields: s.get_fields()?,
                indices: self.raw.generic.nonunion_members,
                parent: self,
            })
        } else {
            panic!()
        }
    }

    pub fn get_annotations(self) -> Result<AnnotationList> {
        Ok(AnnotationList {
            annotations: self.proto.get_annotations()?,
            child_index: None,
            get_annotation_type: self.raw.annotation_types,
        })
    }
}

impl From<RawBrandedStructSchema> for StructSchema {
    fn from(rs: RawBrandedStructSchema) -> StructSchema {
        StructSchema::new(rs)
    }
}

/// A field of a struct, with generics applied.
#[derive(Clone, Copy)]
pub struct Field {
    proto: field::Reader<'static>,
    index: u16,
    ty: introspect::Type,
    pub(crate) parent: StructSchema,
}

impl Field {
    pub fn get_containing_struct(self) -> StructSchema {
        self.parent
    }
    /// Snapshot the owning branded schema and field index into an `Eq + Hash` key.
    #[cfg(feature = "alloc")]
    pub fn identity(self) -> Result<MemberIdentity<'static>> {
        Ok(self
            .parent
            .identity()?
            .member(MemberKind::Field, self.index))
    }
    pub fn get_proto(self) -> field::Reader<'static> {
        self.proto
    }

    pub fn get_type(&self) -> introspect::Type {
        self.ty
    }

    pub fn get_index(&self) -> u16 {
        self.index
    }

    pub fn get_annotations(self) -> Result<AnnotationList> {
        Ok(AnnotationList {
            annotations: self.proto.get_annotations()?,
            child_index: Some(self.index),
            get_annotation_type: self.parent.raw.annotation_types,
        })
    }
}

/// A list of fields of a struct, with generics applied.
#[derive(Clone, Copy)]
pub struct FieldList {
    pub(crate) fields: crate::struct_list::Reader<'static, field::Owned>,
    pub(crate) parent: StructSchema,
}

impl FieldList {
    pub fn len(&self) -> u16 {
        self.fields.len().try_into().unwrap()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(self, index: u16) -> Field {
        Field {
            proto: self.fields.get(index as u32),
            index,
            ty: (self.parent.raw.field_types)(index),
            parent: self.parent,
        }
    }

    pub fn iter(self) -> ShortListIter<Self, Field> {
        ShortListIter::new(self, self.len())
    }
}

impl IndexMove<u16, Field> for FieldList {
    fn index_move(&self, index: u16) -> Field {
        self.get(index)
    }
}

impl ::core::iter::IntoIterator for FieldList {
    type Item = Field;
    type IntoIter = ShortListIter<FieldList, Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// A list of a subset of fields of a struct, with generics applied.
#[derive(Clone, Copy)]
pub struct FieldSubset {
    fields: struct_list::Reader<'static, field::Owned>,
    indices: &'static [u16],
    parent: StructSchema,
}

impl FieldSubset {
    pub fn len(&self) -> u16 {
        self.indices.len().try_into().unwrap()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(self, index: u16) -> Field {
        let index = self.indices[index as usize];
        Field {
            proto: self.fields.get(index as u32),
            index,
            ty: (self.parent.raw.field_types)(index),
            parent: self.parent,
        }
    }

    pub fn iter(self) -> ShortListIter<Self, Field> {
        ShortListIter::new(self, self.len())
    }
}

impl IndexMove<u16, Field> for FieldSubset {
    fn index_move(&self, index: u16) -> Field {
        self.get(index)
    }
}

impl ::core::iter::IntoIterator for FieldSubset {
    type Item = Field;
    type IntoIter = ShortListIter<FieldSubset, Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// A compiled enum with enclosing type parameters erased.
/// Individual annotations can still carry their own applied brands.
#[derive(Clone, Copy)]
pub struct EnumSchema {
    pub(crate) raw: RawEnumSchema,
    pub(crate) proto: node::Reader<'static>,
}

impl EnumSchema {
    display_names!();
    /// Snapshot metadata ownership for use as an `Eq + Hash` key.
    /// Compiled enum identity erases enclosing brands, as native enum types do.
    #[cfg(feature = "alloc")]
    pub fn identity(self) -> Result<SchemaIdentity<'static>> {
        let mut key = identity::Builder::new();
        key.compiled_type(introspect::TypeVariant::Enum(self.raw).into())?;
        Ok(key.finish(self.proto.get_id()))
    }
    pub fn new(raw: RawEnumSchema) -> Self {
        let proto = crate::any_pointer::Reader::new(unsafe {
            layout::PointerReader::get_root_unchecked(raw.encoded_node.as_ptr() as *const u8)
        })
        .get_as()
        .unwrap();
        Self { raw, proto }
    }

    pub fn get_proto(self) -> node::Reader<'static> {
        self.proto
    }

    pub fn get_enumerants(self) -> crate::Result<EnumerantList> {
        if let node::Enum(s) = self.proto.which()? {
            Ok(EnumerantList {
                enumerants: s.get_enumerants()?,
                parent: self,
            })
        } else {
            panic!()
        }
    }

    pub fn find_enumerant_by_name(self, name: &str) -> Result<Option<Enumerant>> {
        for enumerant in self.get_enumerants()? {
            if enumerant.get_proto().get_name()?.as_bytes() == name.as_bytes() {
                return Ok(Some(enumerant));
            }
        }
        Ok(None)
    }

    pub fn get_enumerant_by_name(self, name: &str) -> Result<Enumerant> {
        self.find_enumerant_by_name(name)?
            .ok_or_else(|| crate::Error::from_kind(crate::ErrorKind::FieldNotFound))
    }

    pub fn get_annotations(self) -> Result<AnnotationList> {
        Ok(AnnotationList {
            annotations: self.proto.get_annotations()?,
            child_index: None,
            get_annotation_type: self.raw.annotation_types,
        })
    }
}

impl From<RawEnumSchema> for EnumSchema {
    fn from(re: RawEnumSchema) -> EnumSchema {
        EnumSchema::new(re)
    }
}

/// An enumerant of a compiled enum. Annotation types may be branded independently.
#[derive(Clone, Copy)]
pub struct Enumerant {
    ordinal: u16,
    parent: EnumSchema,
    proto: enumerant::Reader<'static>,
}

impl Enumerant {
    #[cfg(feature = "alloc")]
    pub fn identity(self) -> Result<MemberIdentity<'static>> {
        Ok(self
            .parent
            .identity()?
            .member(MemberKind::Enumerant, self.ordinal))
    }
    pub fn get_containing_enum(self) -> EnumSchema {
        self.parent
    }

    pub fn get_ordinal(self) -> u16 {
        self.ordinal
    }

    pub fn get_proto(self) -> enumerant::Reader<'static> {
        self.proto
    }

    pub fn get_annotations(self) -> Result<AnnotationList> {
        Ok(AnnotationList {
            annotations: self.proto.get_annotations()?,
            child_index: Some(self.ordinal),
            get_annotation_type: self.parent.raw.annotation_types,
        })
    }
}

/// A list of enumerants.
#[derive(Clone, Copy)]
pub struct EnumerantList {
    enumerants: struct_list::Reader<'static, enumerant::Owned>,
    parent: EnumSchema,
}

impl EnumerantList {
    pub fn len(&self) -> u16 {
        self.enumerants.len().try_into().unwrap()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(self, ordinal: u16) -> Enumerant {
        Enumerant {
            proto: self.enumerants.get(ordinal as u32),
            ordinal,
            parent: self.parent,
        }
    }

    pub fn iter(self) -> ShortListIter<Self, Enumerant> {
        ShortListIter::new(self, self.len())
    }
}

impl IndexMove<u16, Enumerant> for EnumerantList {
    fn index_move(&self, index: u16) -> Enumerant {
        self.get(index)
    }
}

impl ::core::iter::IntoIterator for EnumerantList {
    type Item = Enumerant;
    type IntoIter = ShortListIter<Self, Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// An annotation.
#[derive(Clone, Copy)]
pub struct Annotation {
    proto: annotation::Reader<'static>,
    ty: introspect::Type,
}

impl Annotation {
    /// Gets the value held in this annotation.
    pub fn get_value(self) -> Result<dynamic_value::Reader<'static>> {
        dynamic_value::Reader::new(self.proto.get_value()?, self.ty)
    }

    /// Gets the ID of the annotation node.
    pub fn get_id(&self) -> u64 {
        self.proto.get_id()
    }

    /// Gets the type of the value held in this annotation.
    pub fn get_type(&self) -> introspect::Type {
        self.ty
    }
}

/// A list of annotations.
#[derive(Clone, Copy)]
pub struct AnnotationList {
    annotations: struct_list::Reader<'static, annotation::Owned>,
    child_index: Option<u16>,
    get_annotation_type: fn(Option<u16>, u32) -> introspect::Type,
}

impl AnnotationList {
    pub fn len(&self) -> u32 {
        self.annotations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(self, index: u32) -> Annotation {
        let proto = self.annotations.get(index);
        let ty = (self.get_annotation_type)(self.child_index, index);
        Annotation { proto, ty }
    }

    /// Returns the first annotation in the list that matches `id`.
    /// Otherwise returns `None`.
    pub fn find(self, id: u64) -> Option<Annotation> {
        self.iter().find(|&annotation| annotation.get_id() == id)
    }

    pub fn iter(self) -> ListIter<Self, Annotation> {
        ListIter::new(self, self.len())
    }
}

impl IndexMove<u32, Annotation> for AnnotationList {
    fn index_move(&self, index: u32) -> Annotation {
        self.get(index)
    }
}

impl ::core::iter::IntoIterator for AnnotationList {
    type Item = Annotation;
    type IntoIter = ListIter<Self, Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// A compiled interface with generic parameters applied.
#[derive(Clone, Copy)]
pub struct InterfaceSchema {
    pub(crate) raw: introspect::RawBrandedInterfaceSchema,
    proto: node::Reader<'static>,
}
impl InterfaceSchema {
    display_names!();
    /// Snapshot metadata ownership and the full applied brand into an `Eq + Hash` key.
    #[cfg(feature = "alloc")]
    pub fn identity(self) -> Result<SchemaIdentity<'static>> {
        let mut key = identity::Builder::new();
        key.compiled_type(introspect::TypeVariant::Interface(self.raw).into())?;
        Ok(key.finish(self.proto.get_id()))
    }
    pub fn new(raw: introspect::RawBrandedInterfaceSchema) -> Self {
        let proto = crate::any_pointer::Reader::new(
            layout::PointerReader::get_root_from_arena(raw.arena).unwrap(),
        )
        .get_as()
        .unwrap();
        Self { raw, proto }
    }
    pub fn get_proto(self) -> node::Reader<'static> {
        self.proto
    }
    pub fn get_methods(self) -> Result<MethodList> {
        match self.proto.which()? {
            node::Interface(interface) => Ok(MethodList {
                parent: self,
                methods: interface.get_methods()?,
            }),
            _ => Err(crate::Error::from_kind(crate::ErrorKind::TypeMismatch)),
        }
    }
    pub fn get_superclasses(self) -> Result<SuperclassList> {
        match self.proto.which()? {
            node::Interface(interface) => Ok(SuperclassList {
                parent: self,
                len: u16::try_from(interface.get_superclasses()?.len())
                    .map_err(|_| crate::Error::from_kind(crate::ErrorKind::LengthOverflow))?,
            }),
            _ => Err(crate::Error::from_kind(crate::ErrorKind::TypeMismatch)),
        }
    }
    pub fn equals(self, other: Self) -> Result<bool> {
        introspect::Type::from(introspect::TypeVariant::Interface(self.raw))
            .equals(introspect::TypeVariant::Interface(other.raw).into())
    }
    pub fn get_brand(self) -> introspect::Brand {
        self.raw.brand
    }
    pub fn find_superclass(self, id: u64) -> Result<Option<Self>> {
        self.find_superclass_with_budget(id, &mut 64)
    }
    fn find_superclass_with_budget(self, id: u64, budget: &mut usize) -> Result<Option<Self>> {
        introspect::spend_budget(budget)?;
        if self.proto.get_id() == id {
            return Ok(Some(self));
        }
        for parent in self.get_superclasses()?.iter() {
            if let Some(found) = parent.find_superclass_with_budget(id, budget)? {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }
    pub fn extends(self, other: Self) -> Result<bool> {
        self.extends_with_budget(other, &mut 64)
    }
    fn extends_with_budget(self, other: Self, budget: &mut usize) -> Result<bool> {
        introspect::spend_budget(budget)?;
        if introspect::Type::from(introspect::TypeVariant::Interface(self.raw))
            .equals(introspect::TypeVariant::Interface(other.raw).into())?
        {
            return Ok(true);
        }
        for parent in self.get_superclasses()?.iter() {
            if parent.extends_with_budget(other, budget)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub fn find_method_by_name(self, name: &str) -> Result<Option<Method>> {
        self.find_method_with_budget(name, &mut 64)
    }
    fn find_method_with_budget(self, name: &str, budget: &mut usize) -> Result<Option<Method>> {
        introspect::spend_budget(budget)?;
        for method in self.get_methods()?.iter() {
            if method.get_proto().get_name()?.to_str()? == name {
                return Ok(Some(method));
            }
        }
        for parent in self.get_superclasses()?.iter() {
            if let Some(method) = parent.find_method_with_budget(name, budget)? {
                return Ok(Some(method));
            }
        }
        Ok(None)
    }
    pub fn get_method_by_name(self, name: &str) -> Result<Method> {
        self.find_method_by_name(name)?
            .ok_or_else(|| crate::Error::from_kind(crate::ErrorKind::FieldNotFound))
    }
}
impl From<introspect::RawBrandedInterfaceSchema> for InterfaceSchema {
    fn from(raw: introspect::RawBrandedInterfaceSchema) -> Self {
        Self::new(raw)
    }
}
#[derive(Clone, Copy)]
pub struct Method {
    parent: InterfaceSchema,
    index: u16,
    proto: crate::schema_capnp::method::Reader<'static>,
}
impl Method {
    #[cfg(feature = "alloc")]
    pub fn identity(self) -> Result<MemberIdentity<'static>> {
        Ok(self
            .parent
            .identity()?
            .member(MemberKind::Method, self.index))
    }
    pub fn get_proto(self) -> crate::schema_capnp::method::Reader<'static> {
        self.proto
    }
    pub fn get_containing_interface(self) -> InterfaceSchema {
        self.parent
    }
    pub fn get_index(self) -> u16 {
        self.index
    }
    pub fn get_param_type(self) -> StructSchema {
        (self.parent.raw.method_types)(self.index)
            .params
            .as_struct_schema()
            .expect("method parameters must be a struct")
    }
    pub fn get_result_type(self) -> Option<StructSchema> {
        (self.parent.raw.method_types)(self.index)
            .results
            .map(|ty| {
                ty.as_struct_schema()
                    .expect("method results must be a struct")
            })
    }
    pub fn is_streaming(self) -> bool {
        self.get_result_type().is_none()
    }
    pub fn no_promise_pipelining(self) -> bool {
        (self.parent.raw.method_types)(self.index).no_promise_pipelining
    }
}
#[derive(Clone, Copy)]
pub struct MethodList {
    parent: InterfaceSchema,
    methods: crate::struct_list::Reader<'static, crate::schema_capnp::method::Owned>,
}
impl MethodList {
    pub fn len(self) -> u16 {
        self.methods.len().try_into().unwrap()
    }
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }
    pub fn get(self, index: u16) -> Method {
        Method {
            parent: self.parent,
            index,
            proto: self.methods.get(index as u32),
        }
    }
    pub fn iter(self) -> impl Iterator<Item = Method> {
        (0..self.len()).map(move |index| self.get(index))
    }
}
#[derive(Clone, Copy)]
pub struct SuperclassList {
    parent: InterfaceSchema,
    len: u16,
}
impl SuperclassList {
    pub fn len(self) -> u16 {
        self.len
    }
    pub fn is_empty(self) -> bool {
        self.len == 0
    }
    pub fn get(self, index: u16) -> InterfaceSchema {
        assert!(index < self.len);
        let introspect::TypeVariant::Interface(raw) = (self.parent.raw.superclass)(index).which()
        else {
            panic!("superclass must be an interface")
        };
        raw.into()
    }
    pub fn iter(self) -> impl Iterator<Item = InterfaceSchema> {
        (0..self.len).map(move |index| self.get(index))
    }
}
