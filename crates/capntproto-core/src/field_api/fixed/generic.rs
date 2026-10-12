//! Type-directed bounded access for generated generic parameters and AnyPointer.
#![forbid(unsafe_code)]
use super::*;

/// Maps a generated/standard owned type marker to its bounded pointer kind.
/// This never constructs the marker's legacy `Builder` associated type.
/// Custom implementations are responsible for matching their schema identity.
pub trait FixedType: crate::traits::Owned {
    type Kind: PointerType;
}

/// Generated mapping from a legacy struct owner to its bounded schema marker.
pub trait FixedSchema: crate::traits::OwnedStruct {
    type Schema: Schema + FixedElement;
}

/// Maps a legacy enum to the generated open enum, preserving unknown ordinals.
pub trait FixedEnum {
    type Element: FixedElement;
}

impl FixedType for crate::text::Owned {
    type Kind = Text;
}
impl FixedType for crate::data::Owned {
    type Kind = Data;
}
impl FixedType for crate::any_pointer::Owned {
    type Kind = Generic<Self>;
}
impl FixedType for crate::text_list::Owned {
    type Kind = List<Text>;
}
impl FixedType for crate::data_list::Owned {
    type Kind = List<Data>;
}
impl<E: FixedElement> FixedType for crate::primitive_list::Owned<E>
where
    Self: crate::traits::Owned,
{
    type Kind = List<E>;
}
impl<E: FixedEnum> FixedType for crate::enum_list::Owned<E>
where
    Self: crate::traits::Owned,
{
    type Kind = List<E::Element>;
}
impl<S: FixedSchema> FixedType for crate::struct_list::Owned<S> {
    type Kind = List<S::Schema>;
}
impl<T: FixedType> FixedType for crate::list_list::Owned<T>
where
    T::Kind: FixedElement,
{
    type Kind = List<T::Kind>;
}

impl<'a, K: PointerType> FixedField<'a, K> {
    fn retype<J: PointerType>(self) -> FixedField<'a, J> {
        FixedField(PointerField {
            slot: self.0.slot,
            location: self.0.location,
            default: self.0.default,
            marker: PhantomData,
        })
    }
}
impl<'a, T: FixedType> FixedField<'a, Generic<T>> {
    /// Use the generic parameter's schema-known bounded operations. Acquiring
    /// this handle does not copy a default, select a union arm or mutate storage.
    pub fn typed(self) -> FixedField<'a, T::Kind> {
        self.retype()
    }
}

impl<'a> FixedField<'a, Generic<crate::any_pointer::Owned>> {
    /// Interpret an AnyPointer with an explicit kind. Reads/edits validate the
    /// existing encoding; init/replace deliberately construct the chosen kind.
    pub fn cast<K: PointerType>(self) -> FixedField<'a, K> {
        self.retype()
    }

    pub fn read(self) -> Result<crate::any_pointer::Reader<'a>> {
        self.0.slot.check_arm()?;
        Ok(crate::any_pointer::Reader::new(
            self.0.slot.into_pointer().into_reader(),
        ))
    }

    /// Encode an index into this RPC message's capability table. Publishing the
    /// table and accounting for its references remain the RPC engine's job.
    pub fn set_capability_index(mut self, index: u32) -> Result<()> {
        self.0.slot.pointer().set_wire_capability(index)?;
        self.0.slot.commit();
        Ok(())
    }

    /// Stage an arbitrary data tree; unsupported capabilities are rejected before
    /// hook extraction. Errors retain the old value, selection and capacity.
    pub fn copy_from(mut self, source: crate::any_pointer::Reader<'_>) -> Result<()> {
        self.0
            .slot
            .pointer()
            .stage(|mut p| p.copy_from(source.reader, false))?;
        self.0.slot.commit();
        Ok(())
    }
    /// Copy a wire subtree using an explicit destination descriptor mapping.
    /// The containing field and union tag change only after the full copy fits.
    pub fn copy_with_capabilities(
        mut self,
        source: crate::any_pointer::Reader<'_>,
        mapping: crate::wire_capability::IndexMap<'_>,
    ) -> Result<()> {
        self.0
            .slot
            .pointer()
            .stage(|mut p| p.copy_wire_from(source.reader, mapping))?;
        self.0.slot.commit();
        Ok(())
    }
}

// Pointer-list generic elements must stay bounded even when their parameter is
// itself a struct or AnyPointer. Struct<S> is a pointer element; S alone is the
// ordinary inline-composite element used by generated List(S) fields.
impl<S: Schema> Element for Struct<S> {
    type Ref<'a> = Result<S::Ref<'a>>;
    type Mut<'a> = PointerField<'a, Self>;
    const SIZE: ElementSize = ElementSize::Pointer;
    fn get<'a>(list: ListReader<'a>, index: u32) -> Self::Ref<'a> {
        Self::read(list.get_pointer_element(index), None)
    }
    fn get_mut<'a>(list: ListBuilder<'a>, index: u32) -> Self::Mut<'a> {
        PointerField::element(list.get_pointer_element(index), index)
    }
}
impl<S: Schema> FixedElement for Struct<S> {
    type Fixed<'a> = FixedField<'a, Self>;
    fn get_fixed<'a>(list: ListBuilder<'a>, index: u32) -> Self::Fixed<'a> {
        FixedField(PointerField::element(
            list.get_pointer_element(index),
            index,
        ))
    }
}
impl<T: crate::traits::Owned> FixedElement for Generic<T> {
    type Fixed<'a> = FixedField<'a, Self>;
    fn get_fixed<'a>(list: ListBuilder<'a>, index: u32) -> Self::Fixed<'a> {
        FixedField(PointerField::element(
            list.get_pointer_element(index),
            index,
        ))
    }
}
