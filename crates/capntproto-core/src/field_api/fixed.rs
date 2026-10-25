//! Restricted editors: every exposed growth operation reserves fallibly.
#![forbid(unsafe_code)]
use super::*;
mod generic;
pub use generic::{FixedEnum, FixedSchema, FixedType};

/// Schema-specific scalar metadata; a descriptor cannot edit another schema.
pub struct ScalarDescriptor<S, T: Scalar> {
    offset: usize,
    mask: T::Mask,
    selection: Option<Selection>,
    marker: PhantomData<S>,
}
impl<S, T: Scalar> Copy for ScalarDescriptor<S, T> {}
impl<S, T: Scalar> Clone for ScalarDescriptor<S, T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<S, T: Scalar> ScalarDescriptor<S, T> {
    #[doc(hidden)]
    pub const fn new(offset: usize, mask: T::Mask, selection: Option<Selection>) -> Self {
        Self {
            offset,
            mask,
            selection,
            marker: PhantomData,
        }
    }
}

/// A schema-sized fixed-buffer editor. Its raw/legacy editor is never exposed.
pub struct FixedStruct<'a, S: Schema> {
    raw: StructBuilder<'a>,
    marker: PhantomData<S>,
}
impl<'a, S: Schema> FixedStruct<'a, S> {
    /// Adapter for generated fixed list elements. Validates the inline layout.
    #[doc(hidden)]
    pub fn from_layout(raw: StructBuilder<'a>) -> Self {
        check_layout(&raw, S::SIZE);
        Self::new(raw)
    }
    pub(crate) fn new(raw: StructBuilder<'a>) -> Self {
        Self {
            raw,
            marker: PhantomData,
        }
    }
    pub fn read(&self) -> S::Ref<'_> {
        self.raw.as_reader().into()
    }
    pub fn reborrow(&mut self) -> FixedStruct<'_, S> {
        FixedStruct::new(self.raw.reborrow())
    }
    pub fn scalar<T: Scalar>(&mut self, descriptor: ScalarDescriptor<S, T>) -> ScalarField<'_, T> {
        ScalarField::new(
            self.raw.reborrow(),
            descriptor.offset,
            descriptor.mask,
            descriptor.selection,
        )
    }
    pub fn field<K: PointerType>(&mut self, descriptor: Field<S, K>) -> FixedField<'_, K> {
        self.reborrow().into_field(descriptor)
    }
    /// Consume this editor to transfer its full borrow to one pointer field.
    /// This lets an RPC result editor outlive temporary envelope editors while
    /// keeping the original fixed builder exclusively borrowed.
    pub fn into_field<K: PointerType>(self, descriptor: Field<S, K>) -> FixedField<'a, K> {
        FixedField(
            PointerField::new(
                self.raw,
                descriptor.offset,
                descriptor.default,
                descriptor.selection,
            )
            .with_location(descriptor.location()),
        )
    }
    pub fn void(&mut self, descriptor: VoidDescriptor<S>) -> VoidField<'_> {
        VoidField::new(self.raw.reborrow(), descriptor.selection)
    }
    pub fn group<G: GroupSchema>(
        &mut self,
        descriptor: GroupDescriptor<S, G>,
    ) -> FixedGroup<'_, G> {
        let raw = self.raw.reborrow();
        check_layout(&raw, G::SIZE);
        assert!(G::DATA_MASKS
            .iter()
            .all(|&(i, _)| i < usize::from(G::SIZE.data)));
        assert!(G::POINTERS
            .iter()
            .all(|&i| i < usize::from(G::SIZE.pointers)));
        if let Some(s) = descriptor.selection {
            s.check(&raw);
        }
        FixedGroup {
            raw,
            selection: descriptor.selection,
            marker: PhantomData,
        }
    }
}

/// Generated schema-specific Void field, including a union's default arm.
pub struct VoidDescriptor<S> {
    selection: Option<Selection>,
    marker: PhantomData<S>,
}
impl<S> VoidDescriptor<S> {
    #[doc(hidden)]
    pub const fn new(selection: Option<Selection>) -> Self {
        Self {
            selection,
            marker: PhantomData,
        }
    }
}
impl<S> Copy for VoidDescriptor<S> {}
impl<S> Clone for VoidDescriptor<S> {
    fn clone(&self) -> Self {
        *self
    }
}

/// Generated group metadata. Parent and group types cannot be interchanged.
pub struct GroupDescriptor<S, G: GroupSchema> {
    selection: Option<Selection>,
    marker: PhantomData<(S, G)>,
}
impl<S, G: GroupSchema> GroupDescriptor<S, G> {
    pub(crate) fn pipeline_requires_selection(self) -> bool {
        self.selection.is_some()
    }
    #[doc(hidden)]
    pub const fn new(selection: Option<Selection>) -> Self {
        Self {
            selection,
            marker: PhantomData,
        }
    }
}
impl<S, G: GroupSchema> Copy for GroupDescriptor<S, G> {}
impl<S, G: GroupSchema> Clone for GroupDescriptor<S, G> {
    fn clone(&self) -> Self {
        *self
    }
}

/// Inline group editor. Selecting/resetting a group cannot allocate. Subsequent
/// child operations commit independently; this is not a multi-field transaction.
pub struct FixedGroup<'a, G: GroupSchema> {
    raw: StructBuilder<'a>,
    selection: Option<Selection>,
    marker: PhantomData<G>,
}
impl<'a, G: GroupSchema> FixedGroup<'a, G> {
    pub fn edit(self) -> Result<FixedStruct<'a, G>> {
        if self.selection.is_some_and(|s| !s.selected(&self.raw)) {
            return Err(Error::from_kind(ErrorKind::NotPresent));
        }
        Ok(FixedStruct::new(self.raw))
    }
    pub fn select(mut self) -> FixedStruct<'a, G> {
        if let Some(s) = self.selection {
            s.commit(&mut self.raw, None);
        }
        FixedStruct::new(self.raw)
    }
    /// Reset only this group's schema-known storage, preserving parent siblings.
    pub fn reset(mut self) -> FixedStruct<'a, G> {
        if let Some(s) = self.selection {
            s.commit(&mut self.raw, None);
        }
        for &(i, mask) in G::DATA_MASKS {
            self.raw
                .set_data_field::<u64>(i, self.raw.get_data_field::<u64>(i) & !mask);
        }
        for &i in G::POINTERS {
            self.raw.reborrow().get_pointer_field(i).clear();
        }
        FixedStruct::new(self.raw)
    }
}

/// Bounded pointer operations. Errors retain compact kinds; format `location()`
/// separately into a caller-owned sink when field context is needed.
pub struct FixedField<'a, K: PointerType>(pub(super) PointerField<'a, K>);
impl<K: PointerType> FixedField<'_, K> {
    /// Inspect a selected field without allocating, selecting a union arm or
    /// exposing the underlying growing pointer builder.
    pub fn is_null(&mut self) -> Result<bool> {
        self.0.slot.check_arm()?;
        Ok(self.0.slot.pointer().is_null())
    }
    /// Lend this field to a shorter operation without losing its union selector.
    /// In particular, async service adapters need not borrow local context for
    /// the entire lifetime of the surrounding result buffer.
    pub fn reborrow(&mut self) -> FixedField<'_, K> {
        let slot = match &mut self.0.slot {
            Slot::Struct(builder, offset, selection) => {
                Slot::Struct(builder.reborrow(), *offset, *selection)
            }
            Slot::Element(pointer) => Slot::Element(pointer.reborrow()),
        };
        FixedField(PointerField {
            location: self.0.location,
            slot,
            default: self.0.default,
            marker: PhantomData,
        })
    }
    pub fn location(&self) -> Location {
        self.0.location
    }
    pub fn clear(mut self) {
        self.0.slot.pointer().clear();
        self.0.slot.commit();
    }
}

impl<'a, S: Schema> FixedField<'a, Struct<S>> {
    /// Enlarge a selected, present struct while retaining unknown fields and
    /// moving child pointers. Rejection preserves both contents and capacity.
    pub fn upgrade(self) -> Result<FixedStruct<'a, S>> {
        self.0.slot.check_arm()?;
        Ok(FixedStruct::new(
            self.0.slot.into_pointer().try_upgrade_struct(S::SIZE)?,
        ))
    }
    /// Preserve the old field/tag and reclaim unpublished storage on any error.
    pub fn copy_from(mut self, value: S::Ref<'_>) -> Result<()> {
        let raw = crate::traits::IntoInternalStructReader::into_internal_struct_reader(value);
        self.0
            .slot
            .pointer()
            .stage(|mut p| p.set_struct(&raw, false))?;
        self.0.slot.commit();
        Ok(())
    }
    pub fn init(mut self) -> Result<FixedStruct<'a, S>> {
        if !self.0.slot.pointer().is_null() {
            return Err(Error::from_kind(ErrorKind::AlreadyPresent));
        }
        self.replace()
    }
    pub fn replace(mut self) -> Result<FixedStruct<'a, S>> {
        let _ = self.0.slot.pointer().try_init_struct(S::SIZE)?;
        self.0.slot.commit();
        Ok(FixedStruct::new(
            self.0
                .slot
                .into_pointer()
                .get_struct_without_upgrade(S::SIZE)?,
        ))
    }
    pub fn edit(self) -> Result<FixedStruct<'a, S>> {
        self.0.slot.check_arm()?;
        Ok(FixedStruct::new(
            self.0
                .slot
                .into_pointer()
                .get_struct_without_upgrade(S::SIZE)?,
        ))
    }
}

impl FixedField<'_, Text> {
    pub fn copy_from(mut self, value: &str) -> Result<()> {
        let n = count(
            value
                .len()
                .checked_add(1)
                .ok_or_else(|| Error::from_kind(ErrorKind::LengthOverflow))?,
            0,
        )?;
        self.0
            .slot
            .pointer()
            .try_init_text(n - 1)?
            .as_bytes_mut()
            .copy_from_slice(value.as_bytes());
        self.0.slot.commit();
        Ok(())
    }
}
impl<'a> FixedField<'a, Data> {
    pub fn init(mut self, len: usize) -> Result<&'a mut [u8]> {
        if !self.0.slot.pointer().is_null() {
            return Err(Error::from_kind(ErrorKind::AlreadyPresent));
        }
        self.replace(len)
    }
    pub fn replace(mut self, len: usize) -> Result<&'a mut [u8]> {
        let n = count(len, 0)?;
        let _ = self.0.slot.pointer().try_init_data(n)?;
        self.0.slot.commit();
        // The just-published byte list is present, so no default copy can occur.
        self.0.slot.into_pointer().get_data(None)
    }
    pub fn copy_from(self, value: &[u8]) -> Result<()> {
        self.replace(value.len())?.copy_from_slice(value);
        Ok(())
    }
    pub fn edit(self) -> Result<&'a mut [u8]> {
        self.0.slot.check_arm()?;
        let p = self.0.slot.into_pointer();
        present(&p)?;
        p.as_reader().get_data(None)?;
        p.get_data(None)
    }
}

/// Generated bounded element access. Unlike `Element::Mut`, this associated
/// editor never exposes a legacy growing editor. Custom implementations are
/// responsible for their own allocation policy, just as custom `Schema`s are.
pub trait FixedElement: Element {
    type Fixed<'a>;
    #[doc(hidden)]
    fn get_fixed<'a>(list: ListBuilder<'a>, index: u32) -> Self::Fixed<'a>;
}

/// A bounded list editor. Every nested pointer remains a `FixedField`.
pub struct FixedList<'a, E: FixedElement> {
    raw: ListBuilder<'a>,
    marker: PhantomData<E>,
}
impl<'a, E: FixedElement> FixedList<'a, E> {
    pub(crate) fn upgrade(p: PointerBuilder<'a>) -> Result<Self> {
        match E::STRUCT {
            Some(size) => Ok(Self {
                raw: p.try_upgrade_struct_list(size)?,
                marker: PhantomData,
            }),
            None => Self::edit(p),
        }
    }
    pub(crate) fn init(p: PointerBuilder<'a>, len: usize) -> Result<Self> {
        let n = count(len, E::STRUCT.map_or(0, |s| s.total() as usize))?;
        let raw = match E::STRUCT {
            Some(s) => p.try_init_struct_list(n, s)?,
            None => p.try_init_list(E::SIZE, n)?,
        };
        Ok(Self {
            raw,
            marker: PhantomData,
        })
    }
    pub(crate) fn edit(p: PointerBuilder<'a>) -> Result<Self> {
        present(&p)?;
        let r = p.as_reader().get_list(E::SIZE, None)?;
        if E::STRUCT.is_some_and(|s| !r.struct_layout_fits(s)) {
            return Err(Error::from_kind(ErrorKind::NeedsUpgrade));
        }
        let raw = match E::STRUCT {
            Some(s) => p.get_struct_list(s, None)?,
            None => p.get_list(E::SIZE, None)?,
        };
        Ok(Self {
            raw,
            marker: PhantomData,
        })
    }
    pub fn len(&self) -> usize {
        self.raw.len() as usize
    }
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }
    pub fn reborrow(&mut self) -> FixedList<'_, E> {
        FixedList {
            raw: self.raw.reborrow(),
            marker: PhantomData,
        }
    }
    pub fn get_mut(&mut self, index: usize) -> Option<E::Fixed<'_>> {
        (index < self.len()).then(|| {
            E::get_fixed(
                self.raw.reborrow(),
                u32::try_from(index).expect("index is below encoded length"),
            )
        })
    }
    pub fn read(&self) -> ListRef<'_, E> {
        ListRef {
            raw: self.raw.as_reader(),
            marker: PhantomData,
        }
    }
}
impl<'a, E: FixedElement> FixedField<'a, List<E>> {
    /// Explicitly enlarge inline elements or promote compatible primitive/pointer
    /// encodings. The current field/tag and cursor survive allocation failure.
    pub fn upgrade(self) -> Result<FixedList<'a, E>> {
        self.0.slot.check_arm()?;
        FixedList::upgrade(self.0.slot.into_pointer())
    }
    pub fn copy_from(mut self, value: ListRef<'_, E>) -> Result<()> {
        self.0
            .slot
            .pointer()
            .stage(|mut p| p.set_list(&value.raw, false))?;
        self.0.slot.commit();
        Ok(())
    }
    pub fn init(mut self, len: usize) -> Result<FixedList<'a, E>> {
        if !self.0.slot.pointer().is_null() {
            return Err(Error::from_kind(ErrorKind::AlreadyPresent));
        }
        self.replace(len)
    }
    pub fn replace(mut self, len: usize) -> Result<FixedList<'a, E>> {
        let _ = FixedList::<E>::init(self.0.slot.pointer(), len)?;
        self.0.slot.commit();
        FixedList::edit(self.0.slot.into_pointer())
    }
    pub fn edit(self) -> Result<FixedList<'a, E>> {
        self.0.slot.check_arm()?;
        FixedList::edit(self.0.slot.into_pointer())
    }
}
macro_rules! fixed_scalars { ($($t:ty),*) => { $(
    impl FixedElement for $t {
        type Fixed<'a> = ListScalar<'a, Self>;
        fn get_fixed<'a>(raw: ListBuilder<'a>, index: u32) -> Self::Fixed<'a> {
            <Self as Element>::get_mut(raw, index)
        }
    }
)* } }
fixed_scalars!((), bool, u8, u16, u32, u64, i8, i16, i32, i64, f32, f64);
macro_rules! fixed_pointers { ($($t:ty),*) => { $(
    impl FixedElement for $t {
        type Fixed<'a> = FixedField<'a, Self>;
        fn get_fixed<'a>(raw: ListBuilder<'a>, index: u32) -> Self::Fixed<'a> {
            FixedField(PointerField::element(raw.get_pointer_element(index), index))
        }
    }
)* } }
fixed_pointers!(Text, Data);
impl<E: FixedElement> FixedElement for List<E> {
    type Fixed<'a> = FixedField<'a, Self>;
    fn get_fixed<'a>(raw: ListBuilder<'a>, index: u32) -> Self::Fixed<'a> {
        FixedField(PointerField::element(raw.get_pointer_element(index), index))
    }
}
