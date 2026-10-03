//! Shared runtime for the opt-in field-operation generator.
//!
//! Readers decode lazily. Editors borrow storage exclusively; obtaining a field
//! handle does not mutate it. `edit` never allocates, `ensure` may upgrade, and
//! `init` rejects occupied pointers. Recursive copies stage in the destination
//! arena before publication. Allocation uses the underlying `message::Allocator`
//! contract (which does not represent recoverable allocation failure).
use crate::private::layout::{
    self, ElementSize, ListBuilder, ListReader, PointerBuilder, PointerReader, StructBuilder,
    StructReader, StructSize,
};
use crate::{Error, ErrorKind, Result, Word};
use core::marker::PhantomData;

pub mod diagnostics;
pub mod native;
mod owners;
use diagnostics::Location;
pub use owners::{CapabilityTable, FrozenMessage, Message, MessageReader, MessageView};

mod sealed {
    pub trait Mode {}
    pub trait State {}
    pub trait Native {}
    pub trait Bytes {}
}
/// Sealed storage representations; write views cannot be cloned.
pub mod mode {
    #[derive(Clone, Copy)]
    pub struct Schema;
    #[derive(Clone, Copy)]
    pub struct Read<'a>(pub(crate) crate::private::layout::StructReader<'a>);
    pub struct Write<'a>(pub(crate) crate::private::layout::StructBuilder<'a>);
    impl super::sealed::Mode for Schema {}
    impl super::sealed::Mode for Read<'_> {}
    impl super::sealed::Mode for Write<'_> {}
    impl Read<'_> {
        #[doc(hidden)]
        pub fn new(raw: crate::private::layout::StructReader<'_>) -> Read<'_> {
            Read(raw)
        }
    }
    impl<'a> Read<'a> {
        #[doc(hidden)]
        pub fn raw(&self) -> crate::private::layout::StructReader<'a> {
            self.0
        }
    }
    impl<'a> Write<'a> {
        #[doc(hidden)]
        pub fn new(raw: crate::private::layout::StructBuilder<'a>) -> Self {
            Self(raw)
        }
        #[doc(hidden)]
        pub fn raw(&mut self) -> crate::private::layout::StructBuilder<'_> {
            self.0.reborrow()
        }
        #[doc(hidden)]
        pub fn into_raw(self) -> crate::private::layout::StructBuilder<'a> {
            self.0
        }
        #[doc(hidden)]
        pub fn read(&self) -> crate::private::layout::StructReader<'_> {
            self.0.as_reader()
        }
    }
}
pub trait Mode: sealed::Mode {}
impl<T: sealed::Mode> Mode for T {}
/// Generated schema marker. Child editors must have at least `SIZE` storage.
pub trait Schema: Sized {
    /// Schema display name used in generated diagnostics.
    const NAME: &'static str = "<unnamed>";
    type Ref<'a>: From<StructReader<'a>> + crate::traits::IntoInternalStructReader<'a>;
    type Mut<'a>: From<StructBuilder<'a>>;
    const SIZE: StructSize;
}
/// Schema-known storage belonging to a group, excluding its parent siblings.
/// Generated masks include nested groups and every known nested union arm.
#[doc(hidden)]
pub trait GroupSchema: Schema {
    const DATA_MASKS: &'static [(usize, u64)];
    const POINTERS: &'static [usize];
}
#[doc(hidden)]
pub fn check_layout(raw: &StructBuilder<'_>, size: StructSize) {
    assert!(raw.as_reader().get_data_section_size() >= u32::from(size.data) * 64);
    assert!(raw.as_reader().get_pointer_section_size() >= size.pointers);
}
#[derive(Clone, Copy)]
pub struct Mutable;
#[derive(Clone, Copy)]
pub struct Frozen;
impl sealed::State for Mutable {}
impl sealed::State for Frozen {}
pub trait MessageState: sealed::State {}
impl<T: sealed::State> MessageState for T {}
/// A generated union selection. Acquiring an arm does not select it.
#[derive(Clone, Copy)]
#[doc(hidden)]
pub struct Selection {
    pub offset: usize,
    pub tag: u16,
    pub clear: fn(&mut StructBuilder<'_>, Option<usize>),
}
impl Selection {
    fn check(self, b: &StructBuilder<'_>) {
        assert!(self
            .offset
            .checked_add(1)
            .and_then(|n| n.checked_mul(16))
            .is_some_and(|bits| bits <= b.as_reader().get_data_section_size() as usize));
    }
    fn selected(self, b: &StructBuilder<'_>) -> bool {
        self.check(b);
        b.get_data_field::<u16>(self.offset) == self.tag
    }
    fn commit(self, b: &mut StructBuilder<'_>, keep: Option<usize>) {
        if !self.selected(b) {
            (self.clear)(b, keep);
            b.set_data_field(self.offset, self.tag);
        }
    }
}
/// A group occupying a union arm. Acquisition is lazy; `edit` requires the
/// selected arm. `ensure` selects a default group; `replace` resets its storage.
pub struct GroupField<'a, S: Schema> {
    location: Location,
    raw: StructBuilder<'a>,
    selection: Selection,
    marker: PhantomData<S>,
}
impl<'a, S: Schema> GroupField<'a, S> {
    #[doc(hidden)]
    pub fn new(raw: StructBuilder<'a>, selection: Selection) -> Self {
        check_layout(&raw, S::SIZE);
        selection.check(&raw);
        Self {
            location: Location::Unknown,
            raw,
            selection,
            marker: PhantomData,
        }
    }
    #[doc(hidden)]
    pub fn with_location(mut self, location: Location) -> Self {
        self.location = location;
        self
    }
    pub fn edit(self) -> Result<S::Mut<'a>> {
        if !self.selection.selected(&self.raw) {
            return Err(self.location.error(Error::from_kind(ErrorKind::NotPresent)));
        }
        Ok(self.raw.into())
    }
    pub fn ensure(mut self) -> S::Mut<'a> {
        self.selection.commit(&mut self.raw, None);
        self.raw.into()
    }
    pub fn replace(mut self) -> S::Mut<'a> {
        (self.selection.clear)(&mut self.raw, None);
        self.raw
            .set_data_field(self.selection.offset, self.selection.tag);
        self.raw.into()
    }
}
impl<S: GroupSchema> GroupField<'_, S> {
    /// Build a default-valued group in detached storage, then select and replace
    /// the whole arm on success. Errors and callback unwinding preserve the old
    /// arm and parent siblings, and release the candidate's capabilities.
    /// Descendant payloads move on success; they are not copied again.
    /// Callback side effects and arena allocation history are not rolled back.
    pub fn replace_with(
        mut self,
        fill: impl for<'b> FnOnce(S::Mut<'b>) -> Result<()>,
    ) -> Result<()> {
        let selection = self.selection;
        self.raw
            .stage_group(
                S::SIZE,
                S::DATA_MASKS,
                S::POINTERS,
                |raw| fill(raw.into()),
                |raw| {
                    (selection.clear)(raw, None);
                    raw.set_data_field(selection.offset, selection.tag);
                },
            )
            .map_err(|e| self.location.error(e))
    }
}
/// Scalar encoding shared by generated fields and open enums.
pub trait Scalar: Copy {
    type Mask: Copy;
    const BITS: usize;
    fn read(raw: StructReader<'_>, offset: usize, mask: Self::Mask) -> Self;
    fn write(raw: &StructBuilder<'_>, offset: usize, mask: Self::Mask, value: Self);
}
macro_rules! scalar {
    ($($t:ty => $mask:ty),*) => { $(impl Scalar for $t {
        type Mask=$mask; const BITS:usize=core::mem::size_of::<Self>()*8;
        #[inline] fn read(raw:StructReader<'_>,offset:usize,mask:Self::Mask)->Self { raw.get_data_field_mask::<Self>(offset,mask) }
        #[inline] fn write(raw:&StructBuilder<'_>,offset:usize,mask:Self::Mask,value:Self) { raw.set_data_field_mask(offset,value,mask); }
    })* }
}
scalar!(u8=>u8,u16=>u16,u32=>u32,u64=>u64,i8=>i8,i16=>i16,i32=>i32,i64=>i64,f32=>u32,f64=>u64);
impl Scalar for bool {
    type Mask = bool;
    const BITS: usize = 1;
    fn read(raw: StructReader<'_>, offset: usize, mask: bool) -> Self {
        raw.get_bool_field_mask(offset, mask)
    }
    fn write(raw: &StructBuilder<'_>, offset: usize, mask: bool, value: Self) {
        raw.set_bool_field_mask(offset, value, mask);
    }
}
pub struct ScalarField<'a, T: Scalar> {
    raw: StructBuilder<'a>,
    offset: usize,
    mask: T::Mask,
    selection: Option<Selection>,
}
impl<'a, T: Scalar> ScalarField<'a, T> {
    #[doc(hidden)]
    pub fn new(
        raw: StructBuilder<'a>,
        offset: usize,
        mask: T::Mask,
        selection: Option<Selection>,
    ) -> Self {
        assert!(offset
            .checked_add(1)
            .and_then(|n| n.checked_mul(T::BITS))
            .is_some_and(|n| n <= raw.as_reader().get_data_section_size() as usize));
        Self {
            raw,
            offset,
            mask,
            selection,
        }
    }
    #[inline]
    pub fn set(mut self, value: T) {
        if let Some(s) = self.selection {
            s.commit(&mut self.raw, None);
        }
        T::write(&self.raw, self.offset, self.mask, value);
    }
}
pub struct VoidField<'a> {
    raw: StructBuilder<'a>,
    selection: Option<Selection>,
}
impl<'a> VoidField<'a> {
    #[doc(hidden)]
    pub fn new(raw: StructBuilder<'a>, selection: Option<Selection>) -> Self {
        Self { raw, selection }
    }
    pub fn set(mut self) {
        if let Some(s) = self.selection {
            s.commit(&mut self.raw, None);
        }
    }
}
/// Pointer-kind operations. Implementations must check counts before allocation.
pub trait PointerType {
    type Ref<'a>;
    type Mut<'a>;
    fn read<'a>(p: PointerReader<'a>, default: Option<&'a [Word]>) -> Result<Self::Ref<'a>>;
    fn init<'a>(p: PointerBuilder<'a>, count: usize) -> Result<Self::Mut<'a>>;
    fn edit<'a>(p: PointerBuilder<'a>) -> Result<Self::Mut<'a>>;
    fn ensure<'a>(p: PointerBuilder<'a>, default: Option<&'a [Word]>) -> Result<Self::Mut<'a>>;
    /// Acquire an occupied entry and retain its editor or explicit upgrade state.
    /// Custom kinds implement this contract directly; inspection must not defer
    /// another acquisition until edit/ensure or silently allocate an upgrade.
    fn acquire<'a>(p: PointerBuilder<'a>) -> Result<OccupiedField<'a, Self>>
    where
        Self: Sized;
}
pub struct Text;
pub struct Data;
pub struct Struct<S>(PhantomData<S>);
pub struct List<E>(PhantomData<E>);
fn present(p: &PointerBuilder<'_>) -> Result<()> {
    if p.is_null() {
        Err(Error::from_kind(ErrorKind::NotPresent))
    } else {
        Ok(())
    }
}
fn count(n: usize, words: usize) -> Result<u32> {
    if n >= (1 << 29) || n.checked_mul(words).is_none_or(|v| v >= (1 << 29)) {
        return Err(Error::from_kind(ErrorKind::LengthOverflow));
    }
    u32::try_from(n).map_err(|_| Error::from_kind(ErrorKind::LengthOverflow))
}
impl PointerType for Text {
    type Ref<'a> = &'a str;
    type Mut<'a> = crate::text::Builder<'a>;
    fn acquire<'a>(p: PointerBuilder<'a>) -> Result<OccupiedField<'a, Self>> {
        present(&p)?;
        // Occupied text inspection checks UTF-8, as read() does. The editor
        // itself permits byte edits; none can occur while this entry is held.
        Self::read(p.as_reader(), None)?;
        Ok(OccupiedField::new(Acquired::Ready(p.get_text(None))))
    }
    fn read<'a>(p: PointerReader<'a>, d: Option<&'a [Word]>) -> Result<&'a str> {
        Ok(p.get_text(d)?.to_str()?)
    }
    fn init<'a>(p: PointerBuilder<'a>, n: usize) -> Result<Self::Mut<'a>> {
        let n = count(
            n.checked_add(1)
                .ok_or_else(|| Error::from_kind(ErrorKind::LengthOverflow))?,
            0,
        )?;
        Ok(p.init_text(n - 1))
    }
    fn edit<'a>(p: PointerBuilder<'a>) -> Result<Self::Mut<'a>> {
        if p.is_null() {
            return Err(Error::from_kind(ErrorKind::NotPresent));
        }
        p.as_reader().get_text(None)?;
        p.get_text(None)
    }
    fn ensure<'a>(p: PointerBuilder<'a>, d: Option<&'a [Word]>) -> Result<Self::Mut<'a>> {
        p.as_reader().get_text(d)?;
        p.get_text(d)
    }
}
impl PointerType for Data {
    type Ref<'a> = &'a [u8];
    type Mut<'a> = &'a mut [u8];
    fn acquire<'a>(p: PointerBuilder<'a>) -> Result<OccupiedField<'a, Self>> {
        present(&p)?;
        Self::read(p.as_reader(), None)?;
        Ok(OccupiedField::new(Acquired::Ready(p.get_data(None))))
    }
    fn read<'a>(p: PointerReader<'a>, d: Option<&'a [Word]>) -> Result<Self::Ref<'a>> {
        p.get_data(d)
    }
    fn init<'a>(p: PointerBuilder<'a>, n: usize) -> Result<Self::Mut<'a>> {
        Ok(p.init_data(count(n, 0)?))
    }
    fn edit<'a>(p: PointerBuilder<'a>) -> Result<Self::Mut<'a>> {
        if p.is_null() {
            return Err(Error::from_kind(ErrorKind::NotPresent));
        }
        p.as_reader().get_data(None)?;
        p.get_data(None)
    }
    fn ensure<'a>(p: PointerBuilder<'a>, d: Option<&'a [Word]>) -> Result<Self::Mut<'a>> {
        p.as_reader().get_data(d)?;
        p.get_data(d)
    }
}
impl<S: Schema> PointerType for Struct<S> {
    type Ref<'a> = S::Ref<'a>;
    type Mut<'a> = S::Mut<'a>;
    fn acquire<'a>(p: PointerBuilder<'a>) -> Result<OccupiedField<'a, Self>> {
        present(&p)?;
        let r = p.as_reader().get_struct(None)?;
        if r.get_data_section_size() < u32::from(S::SIZE.data) * 64
            || r.get_pointer_section_size() < S::SIZE.pointers
        {
            return Ok(OccupiedField::new(Acquired::Upgrade(
                p,
                Error::from_kind(ErrorKind::NeedsUpgrade),
            )));
        }
        Ok(OccupiedField::new(Acquired::Ready(
            p.get_struct(S::SIZE, None).map(Into::into),
        )))
    }
    fn read<'a>(p: PointerReader<'a>, d: Option<&'a [Word]>) -> Result<Self::Ref<'a>> {
        Ok(p.get_struct(d)?.into())
    }
    fn init<'a>(p: PointerBuilder<'a>, _: usize) -> Result<Self::Mut<'a>> {
        Ok(p.init_struct(S::SIZE).into())
    }
    fn edit<'a>(p: PointerBuilder<'a>) -> Result<Self::Mut<'a>> {
        if p.is_null() {
            return Err(Error::from_kind(ErrorKind::NotPresent));
        }
        let r = p.as_reader().get_struct(None)?;
        if r.get_data_section_size() < u32::from(S::SIZE.data) * 64
            || r.get_pointer_section_size() < S::SIZE.pointers
        {
            return Err(Error::from_kind(ErrorKind::NeedsUpgrade));
        }
        Ok(p.get_struct(S::SIZE, None)?.into())
    }
    fn ensure<'a>(p: PointerBuilder<'a>, d: Option<&'a [Word]>) -> Result<Self::Mut<'a>> {
        Ok(p.get_struct(S::SIZE, d)?.into())
    }
}
/// Pointer handles may be rooted in a struct field or in a pointer-list element.
pub struct PointerField<'a, K: PointerType> {
    location: Location,
    slot: Slot<'a>,
    default: Option<&'static [Word]>,
    marker: PhantomData<K>,
}
enum Slot<'a> {
    Struct(StructBuilder<'a>, usize, Option<Selection>),
    Element(PointerBuilder<'a>),
}
impl Slot<'_> {
    fn pointer(&mut self) -> PointerBuilder<'_> {
        match self {
            Self::Struct(b, i, _) => b.reborrow().get_pointer_field(*i),
            Self::Element(p) => p.reborrow(),
        }
    }
    fn check_arm(&self) -> Result<()> {
        if let Self::Struct(b, _, Some(s)) = self {
            if !s.selected(b) {
                return Err(Error::from_kind(ErrorKind::NotPresent));
            }
        }
        Ok(())
    }
    fn commit(&mut self) {
        if let Self::Struct(b, i, Some(s)) = self {
            s.commit(b, Some(*i));
        }
    }
}
impl<'a> Slot<'a> {
    fn into_pointer(self) -> PointerBuilder<'a> {
        match self {
            Self::Struct(b, i, _) => b.get_pointer_field(i),
            Self::Element(p) => p,
        }
    }
}
pub type TextField<'a> = PointerField<'a, Text>;
pub type DataField<'a> = PointerField<'a, Data>;
pub type StructField<'a, S> = PointerField<'a, Struct<S>>;
pub type ListField<'a, E> = PointerField<'a, List<E>>;
impl<'a, K: PointerType> PointerField<'a, K> {
    #[doc(hidden)]
    pub fn new(
        raw: StructBuilder<'a>,
        offset: usize,
        default: Option<&'static [Word]>,
        selection: Option<Selection>,
    ) -> Self {
        assert!(offset < usize::from(raw.as_reader().get_pointer_section_size()));
        Self {
            location: Location::Unknown,
            slot: Slot::Struct(raw, offset, selection),
            default,
            marker: PhantomData,
        }
    }
    fn element(pointer: PointerBuilder<'a>, index: u32) -> Self {
        Self {
            location: Location::Index(index as usize),
            slot: Slot::Element(pointer),
            default: None,
            marker: PhantomData,
        }
    }
    #[doc(hidden)]
    pub fn with_location(mut self, location: Location) -> Self {
        self.location = location;
        self
    }
    pub fn edit(self) -> Result<K::Mut<'a>> {
        let location = self.location;
        location.run(|| {
            self.slot.check_arm()?;
            K::edit(self.slot.into_pointer())
        })
    }
    pub fn ensure(self) -> Result<K::Mut<'a>> {
        let location = self.location;
        location.run(|| {
            self.slot.check_arm()?;
            K::ensure(self.slot.into_pointer(), self.default)
        })
    }
    pub fn clear(mut self) {
        self.slot.pointer().clear();
        self.slot.commit();
    }
    fn initialize(mut self, n: usize, vacant: bool) -> Result<K::Mut<'a>> {
        let location = self.location;
        location.run(|| {
            if vacant && !self.slot.pointer().is_null() {
                return Err(Error::from_kind(ErrorKind::AlreadyPresent));
            }
            self.slot.pointer().stage(|p| {
                K::init(p, n)?;
                Ok(())
            })?;
            self.slot.commit();
            K::edit(self.slot.into_pointer())
        })
    }
    /// Checks presence and acquires the occupied pointer without substituting a default.
    pub fn entry(mut self) -> Result<Entry<'a, K>> {
        let location = self.location;
        location.run(|| {
            self.slot.check_arm()?;
            if self.slot.pointer().is_null() {
                Ok(Entry::Vacant(VacantField(self)))
            } else {
                let mut entry = K::acquire(self.slot.into_pointer())?;
                entry.location = self.location;
                Ok(Entry::Occupied(entry))
            }
        })
    }
}
pub enum Entry<'a, K: PointerType> {
    Vacant(VacantField<'a, K>),
    Occupied(OccupiedField<'a, K>),
}
pub struct VacantField<'a, K: PointerType>(PointerField<'a, K>);
/// Retains the exclusive field borrow and an acquired editor. Undersized
/// layouts retain the slot for an explicit upgrade instead of allocating here.
pub struct OccupiedField<'a, K: PointerType> {
    acquired: Acquired<'a, K>,
    location: Location,
}
enum Acquired<'a, K: PointerType> {
    Ready(Result<K::Mut<'a>>),
    Upgrade(PointerBuilder<'a>, Error),
}
impl<'a, K: PointerType> OccupiedField<'a, K> {
    /// Retain an already-acquired editor or a strict-edit error (for example an
    /// immutable segment). The pointer kind must validate the readable value
    /// before constructing an occupied entry with an error.
    pub fn from_acquired(editor: Result<K::Mut<'a>>) -> Self {
        Self::new(Acquired::Ready(editor))
    }
    /// Retain a validated occupied slot requiring an explicit allocating upgrade.
    pub fn requiring_upgrade(slot: PointerBuilder<'a>, error: Error) -> Self {
        Self::new(Acquired::Upgrade(slot, error))
    }
    fn new(acquired: Acquired<'a, K>) -> Self {
        Self {
            acquired,
            location: Location::Unknown,
        }
    }
    pub fn edit(self) -> Result<K::Mut<'a>> {
        match self.acquired {
            Acquired::Ready(editor) => editor,
            Acquired::Upgrade(_, error) => Err(error),
        }
        .map_err(|e| self.location.error(e))
    }
    pub fn ensure(self) -> Result<K::Mut<'a>> {
        match self.acquired {
            Acquired::Ready(editor) => editor,
            Acquired::Upgrade(p, _) => K::ensure(p, None),
        }
        .map_err(|e| self.location.error(e))
    }
}
impl<'a, S: Schema> PointerField<'a, Struct<S>> {
    pub fn init(self) -> Result<S::Mut<'a>> {
        self.initialize(0, true)
    }
    pub fn replace(self) -> Result<S::Mut<'a>> {
        self.initialize(0, false)
    }
    pub fn copy_from(mut self, value: S::Ref<'_>) -> Result<()> {
        let location = self.location;
        location.run(|| {
            use crate::traits::IntoInternalStructReader;
            let raw = value.into_internal_struct_reader();
            self.slot
                .pointer()
                .stage(|mut p| p.set_struct(&raw, false))?;
            self.slot.commit();
            Ok(())
        })
    }
}
impl<'a, S: Schema> VacantField<'a, Struct<S>> {
    pub fn init(self) -> Result<S::Mut<'a>> {
        self.0.initialize(0, false)
    }
}
impl<'a> PointerField<'a, Text> {
    pub fn copy_from(mut self, value: &str) -> Result<()> {
        let location = self.location;
        location.run(|| {
            count(
                value
                    .len()
                    .checked_add(1)
                    .ok_or_else(|| Error::from_kind(ErrorKind::LengthOverflow))?,
                0,
            )?;
            // Native input cannot fail after this preflight; same-size storage is reused.
            let compatible = self.slot.check_arm().is_ok() && {
                let p = self.slot.pointer();
                !p.is_null()
                    && p.as_reader()
                        .get_text(None)
                        .is_ok_and(|v| v.len() == value.len())
            };
            if compatible {
                self.slot
                    .pointer()
                    .get_text(None)?
                    .as_bytes_mut()
                    .copy_from_slice(value.as_bytes());
            } else {
                self.slot.pointer().stage(|mut p| {
                    p.set_text(value.into());
                    Ok(())
                })?;
            }
            self.slot.commit();
            Ok(())
        })
    }
}
impl<'a> PointerField<'a, Data> {
    pub fn init(self, n: usize) -> Result<&'a mut [u8]> {
        self.initialize(n, true)
    }
    pub fn replace(self, n: usize) -> Result<&'a mut [u8]> {
        self.initialize(n, false)
    }
    pub fn copy_from(mut self, value: &[u8]) -> Result<()> {
        let location = self.location;
        location.run(|| {
            count(value.len(), 0)?;
            let compatible = self.slot.check_arm().is_ok() && {
                let p = self.slot.pointer();
                !p.is_null()
                    && p.as_reader()
                        .get_data(None)
                        .is_ok_and(|v| v.len() == value.len())
            };
            if compatible {
                self.slot.pointer().get_data(None)?.copy_from_slice(value);
            } else {
                self.slot.pointer().stage(|mut p| {
                    p.set_data(value);
                    Ok(())
                })?;
            }
            self.slot.commit();
            Ok(())
        })
    }
}
impl<'a> VacantField<'a, Data> {
    pub fn init(self, n: usize) -> Result<&'a mut [u8]> {
        self.0.initialize(n, false)
    }
}
/// Message-scoped identity token; it grants no access to the arena by itself.
pub struct Orphanage<'message> {
    identity: layout::ArenaIdentity,
    lifetime: PhantomData<&'message mut ()>,
}
/// Unique ownership of an unreachable encoded object in a borrowed message.
/// Drop releases capabilities. Arena bytes remain allocated until compaction
/// or message destruction, just like other replaced objects.
pub struct Orphan<'message, K: PointerType> {
    object: layout::DetachedObject,
    lifetime: PhantomData<&'message mut ()>,
    kind: PhantomData<K>,
}
/// Failed adoption returns ownership to the caller and leaves the field intact.
pub struct AdoptError<T> {
    pub error: Error,
    pub orphan: T,
}
impl<T> core::fmt::Debug for AdoptError<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AdoptError")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}
impl<K: PointerType> PointerField<'_, K> {
    pub fn take<'message>(
        mut self,
        orphanage: &Orphanage<'message>,
    ) -> Result<Option<Orphan<'message, K>>> {
        let location = self.location;
        location.run(|| {
            self.slot.check_arm()?;
            let mut pointer = self.slot.pointer();
            if pointer.identity() != orphanage.identity {
                return Err(Error::from_kind(ErrorKind::WrongArena));
            }
            if pointer.is_null() {
                return Ok(None);
            }
            K::read(pointer.as_reader(), None)?;
            Ok(Some(Orphan {
                object: pointer.disown()?,
                lifetime: PhantomData,
                kind: PhantomData,
            }))
        })
    }
    pub fn adopt<'message>(
        mut self,
        mut orphan: Orphan<'message, K>,
    ) -> core::result::Result<(), AdoptError<Orphan<'message, K>>> {
        if let Err(error) = self.slot.pointer().adopt(&mut orphan.object) {
            return Err(AdoptError {
                error: self.location.error(error),
                orphan,
            });
        }
        self.slot.commit();
        Ok(())
    }
}

type InvariantBrand<'brand> = PhantomData<fn(&'brand ()) -> &'brand ()>;
/// Controlled constructor: fields and brands cannot be constructed externally.
pub struct EditSession<'message, 'brand, S: Schema> {
    raw: StructBuilder<'message>,
    schema: PhantomData<S>,
    brand: InvariantBrand<'brand>,
}
pub struct ScopedRoot<'message, 'brand, S: Schema>(EditSession<'message, 'brand, S>);
pub struct ScopedOrphanage<'message, 'brand> {
    inner: Orphanage<'message>,
    brand: InvariantBrand<'brand>,
}
pub struct ScopedOrphan<'message, 'brand, K: PointerType> {
    inner: Orphan<'message, K>,
    brand: InvariantBrand<'brand>,
}
pub struct ScopedField<'a, 'brand, K: PointerType> {
    inner: PointerField<'a, K>,
    brand: InvariantBrand<'brand>,
}
impl<'message, 'brand, S: Schema> EditSession<'message, 'brand, S> {
    pub fn into_parts(
        self,
    ) -> (
        ScopedRoot<'message, 'brand, S>,
        ScopedOrphanage<'message, 'brand>,
    ) {
        let orphanage = ScopedOrphanage {
            inner: Orphanage {
                identity: self.raw.identity(),
                lifetime: PhantomData,
            },
            brand: PhantomData,
        };
        (ScopedRoot(self), orphanage)
    }
}
impl<'brand, S: Schema> ScopedRoot<'_, 'brand, S> {
    pub fn read(&self) -> S::Ref<'_> {
        self.0.raw.as_reader().into()
    }
    pub fn edit(&mut self) -> S::Mut<'_> {
        self.0.raw.reborrow().into()
    }
    /// Pointer descriptors retain union selection metadata as well as defaults.
    pub fn field<K: PointerType>(&mut self, descriptor: Field<S, K>) -> ScopedField<'_, 'brand, K> {
        ScopedField {
            inner: PointerField::new(
                self.0.raw.reborrow(),
                descriptor.offset,
                descriptor.default,
                descriptor.selection,
            )
            .with_location(descriptor.location()),
            brand: PhantomData,
        }
    }
}
impl<'a, 'brand, K: PointerType> ScopedField<'a, 'brand, K> {
    pub fn take<'message>(
        self,
        orphanage: &ScopedOrphanage<'message, 'brand>,
    ) -> Result<Option<ScopedOrphan<'message, 'brand, K>>> {
        Ok(self
            .inner
            .take(&orphanage.inner)?
            .map(|inner| ScopedOrphan {
                inner,
                brand: PhantomData,
            }))
    }
    pub fn adopt<'message>(
        self,
        orphan: ScopedOrphan<'message, 'brand, K>,
    ) -> core::result::Result<(), AdoptError<ScopedOrphan<'message, 'brand, K>>> {
        self.inner.adopt(orphan.inner).map_err(|e| AdoptError {
            error: e.error,
            orphan: ScopedOrphan {
                inner: e.orphan,
                brand: PhantomData,
            },
        })
    }
    pub fn edit(self) -> Result<K::Mut<'a>> {
        self.inner.edit()
    }
    pub fn ensure(self) -> Result<K::Mut<'a>> {
        self.inner.ensure()
    }
    pub fn clear(self) {
        self.inner.clear();
    }
}

/// A staged replacement that cannot be published before its construction step
/// succeeds. Forgetting it leaves the old field intact and all bytes initialized.
pub struct Draft;
pub struct Ready;
impl sealed::State for Draft {}
impl sealed::State for Ready {}
pub struct Pending<'a, K: PointerType, State: MessageState = Draft> {
    field: Option<PointerField<'a, K>>,
    candidate: Option<layout::DetachedSlot>,
    length: usize,
    filled: usize,
    state: PhantomData<State>,
}
impl<K: PointerType, State: MessageState> Drop for Pending<'_, K, State> {
    fn drop(&mut self) {
        if let (Some(field), Some(candidate)) = (&mut self.field, &self.candidate) {
            field.slot.pointer().detached(candidate).clear();
        }
    }
}
impl<K: PointerType, State: MessageState> Pending<'_, K, State> {
    /// Inspect the still-published value while holding the replacement borrow.
    pub fn previous(&mut self) -> Result<K::Ref<'_>> {
        let location = self.field.as_ref().unwrap().location;
        location.run(|| {
            let field = self.field.as_mut().unwrap();
            field.slot.check_arm()?;
            K::read(field.slot.pointer().into_reader(), field.default)
        })
    }
}
impl<'a, K: PointerType> Pending<'a, K, Draft> {
    fn new(mut field: PointerField<'a, K>, n: usize, vacant: bool) -> Result<Self> {
        let location = field.location;
        location.run(|| {
            if vacant && !field.slot.pointer().is_null() {
                return Err(Error::from_kind(ErrorKind::AlreadyPresent));
            }
            let candidate = field.slot.pointer().allocate_detached();
            let mut result = Self {
                field: Some(field),
                candidate: Some(candidate),
                length: n,
                filled: 0,
                state: PhantomData,
            };
            K::init(
                result
                    .field
                    .as_mut()
                    .unwrap()
                    .slot
                    .pointer()
                    .detached(result.candidate.as_ref().unwrap()),
                n,
            )?;
            Ok(result)
        })
    }
    fn into_ready(mut self) -> Result<Pending<'a, K, Ready>> {
        let location = self.field.as_ref().unwrap().location;
        location.run(|| {
            let mut pointer = self.field.as_mut().unwrap().slot.pointer();
            K::read(
                pointer
                    .detached(self.candidate.as_ref().unwrap())
                    .as_reader(),
                None,
            )?;
            Ok(Pending {
                field: self.field.take(),
                candidate: self.candidate.take(),
                length: self.length,
                filled: self.length,
                state: PhantomData,
            })
        })
    }
    /// The callback cannot retain an editor. For text this also checks UTF-8.
    /// Struct construction checks the root shape; descendant reads remain lazy.
    pub fn fill_with(
        mut self,
        fill: impl for<'b> FnOnce(K::Mut<'b>) -> Result<()>,
    ) -> Result<Pending<'a, K, Ready>> {
        let location = self.field.as_ref().unwrap().location;
        let mut pointer = self.field.as_mut().unwrap().slot.pointer();
        location.run(|| fill(K::edit(pointer.detached(self.candidate.as_ref().unwrap()))?))?;
        self.into_ready()
    }
}
impl<K: PointerType> Pending<'_, K, Ready> {
    pub fn commit(mut self) -> Result<()> {
        let location = self.field.as_ref().unwrap().location;
        location.run(|| {
            let field = self.field.as_mut().unwrap();
            field
                .slot
                .pointer()
                .publish_detached(self.candidate.as_ref().unwrap())?;
            field.slot.commit();
            Ok(())
        })
    }
}
/// The sealed byte-oriented staging kinds: Data and Text.
pub trait ByteKind: PointerType + sealed::Bytes {
    #[doc(hidden)]
    fn bytes<'a>(value: Self::Mut<'a>) -> &'a mut [u8];
}
impl sealed::Bytes for Data {}
impl sealed::Bytes for Text {}
impl ByteKind for Data {
    fn bytes<'a>(value: Self::Mut<'a>) -> &'a mut [u8] {
        value
    }
}
impl ByteKind for Text {
    fn bytes<'a>(value: Self::Mut<'a>) -> &'a mut [u8] {
        value.as_bytes_mut()
    }
}
impl<'a, K: ByteKind> Pending<'a, K, Draft> {
    pub fn remaining(&self) -> usize {
        self.length - self.filled
    }
    /// Append one complete chunk into final arena storage. Overlong chunks fail
    /// before changing the candidate, and never change the published field.
    pub fn write_chunk(&mut self, bytes: &[u8]) -> Result<()> {
        let location = self.field.as_ref().unwrap().location;
        location.run(|| {
            if bytes.len() > self.remaining() {
                return Err(Error::from_kind(ErrorKind::LengthOverflow));
            }
            let mut pointer = self.field.as_mut().unwrap().slot.pointer();
            let target = K::bytes(K::edit(pointer.detached(self.candidate.as_ref().unwrap()))?);
            target[self.filled..self.filled + bytes.len()].copy_from_slice(bytes);
            self.filled += bytes.len();
            Ok(())
        })
    }
    pub fn finish(self) -> Result<Pending<'a, K, Ready>> {
        if self.remaining() != 0 {
            return Err(self
                .field
                .as_ref()
                .unwrap()
                .location
                .error(Error::from_kind(ErrorKind::IncompleteFill)));
        }
        self.into_ready()
    }
    /// Fill only the remaining suffix after any already-written chunks.
    pub fn read_exact_from(
        mut self,
        source: &mut impl crate::io::Read,
    ) -> Result<Pending<'a, K, Ready>> {
        let location = self.field.as_ref().unwrap().location;
        location.run(|| {
            let mut pointer = self.field.as_mut().unwrap().slot.pointer();
            let target = K::bytes(K::edit(pointer.detached(self.candidate.as_ref().unwrap()))?);
            source.read_exact(&mut target[self.filled..])
        })?;
        self.filled = self.length;
        self.finish()
    }
}
impl<'a> PointerField<'a, Data> {
    pub fn stage_replace(self, n: usize) -> Result<Pending<'a, Data>> {
        Pending::new(self, n, false)
    }
    pub fn init_with(self, n: usize, fill: impl FnOnce(&mut [u8]) -> Result<()>) -> Result<()> {
        Pending::new(self, n, true)?.fill_with(fill)?.commit()
    }
    pub fn replace_with(self, n: usize, fill: impl FnOnce(&mut [u8]) -> Result<()>) -> Result<()> {
        self.stage_replace(n)?.fill_with(fill)?.commit()
    }
}
impl<'a> PointerField<'a, Text> {
    pub fn stage_replace(self, n: usize) -> Result<Pending<'a, Text>> {
        Pending::new(self, n, false)
    }
    pub fn init_with(
        self,
        n: usize,
        fill: impl for<'b> FnOnce(crate::text::Builder<'b>) -> Result<()>,
    ) -> Result<()> {
        Pending::new(self, n, true)?.fill_with(fill)?.commit()
    }
    pub fn replace_with(
        self,
        n: usize,
        fill: impl for<'b> FnOnce(crate::text::Builder<'b>) -> Result<()>,
    ) -> Result<()> {
        self.stage_replace(n)?.fill_with(fill)?.commit()
    }
}
impl<'a, S: Schema> PointerField<'a, Struct<S>> {
    pub fn stage_replace(self) -> Result<Pending<'a, Struct<S>>> {
        Pending::new(self, 0, false)
    }
    pub fn init_with(self, fill: impl for<'b> FnOnce(S::Mut<'b>) -> Result<()>) -> Result<()> {
        Pending::new(self, 0, true)?.fill_with(fill)?.commit()
    }
    pub fn replace_with(self, fill: impl for<'b> FnOnce(S::Mut<'b>) -> Result<()>) -> Result<()> {
        self.stage_replace()?.fill_with(fill)?.commit()
    }
}
impl<'a, E: Element> PointerField<'a, List<E>> {
    pub fn stage_replace(self, n: usize) -> Result<Pending<'a, List<E>>> {
        Pending::new(self, n, false)
    }
}

/// Schema-specific descriptor. Its owner parameter prevents applying it to another schema.
pub struct Field<S, K: PointerType> {
    schema: &'static str,
    offset: usize,
    default: Option<&'static [Word]>,
    selection: Option<Selection>,
    pub name: &'static str,
    pub ordinal: u16,
    marker: PhantomData<(S, K)>,
}
impl<S, K: PointerType> Copy for Field<S, K> {}
impl<S, K: PointerType> Clone for Field<S, K> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<S, K: PointerType> Field<S, K> {
    #[doc(hidden)]
    pub const fn new(
        offset: usize,
        default: Option<&'static [Word]>,
        name: &'static str,
        ordinal: u16,
        selection: Option<Selection>,
    ) -> Self {
        Self {
            schema: "<unnamed>",
            offset,
            default,
            name,
            ordinal,
            selection,
            marker: PhantomData,
        }
    }
    #[doc(hidden)]
    pub const fn with_schema(mut self, schema: &'static str) -> Self {
        self.schema = schema;
        self
    }
    fn location(&self) -> Location {
        Location::Field {
            schema: self.schema,
            name: self.name,
            ordinal: Some(self.ordinal),
        }
    }
    #[doc(hidden)]
    pub fn inspect(self, raw: StructReader<'_>) -> Inspection<'_, K> {
        Inspection {
            location: self.location(),
            pointer: raw.get_pointer_field(self.offset),
            default: self.default,
            marker: PhantomData,
        }
    }
}
pub struct Inspection<'a, K: PointerType> {
    location: Location,
    pointer: PointerReader<'a>,
    default: Option<&'static [Word]>,
    marker: PhantomData<K>,
}
impl<'a, K: PointerType> Inspection<'a, K> {
    pub fn is_null(&self) -> bool {
        self.pointer.is_null()
    }
    pub fn present(&self) -> Result<Option<K::Ref<'a>>> {
        if self.is_null() {
            Ok(None)
        } else {
            K::read(self.pointer, None)
                .map(Some)
                .map_err(|e| self.location.error(e))
        }
    }
}
pub type WireText<'a> = crate::text::Reader<'a>;
impl<'a> Inspection<'a, Text> {
    pub fn wire_text(&self) -> Result<WireText<'a>> {
        self.pointer
            .get_text(self.default)
            .map_err(|e| self.location.error(e))
    }
}

/// An element's encoded access rules. Pointer elements preserve decoding errors.
pub trait Element {
    type Ref<'a>;
    type Mut<'a>;
    const SIZE: ElementSize;
    const STRUCT: Option<StructSize> = None;
    fn get<'a>(list: ListReader<'a>, index: u32) -> Self::Ref<'a>;
    fn get_mut<'a>(list: ListBuilder<'a>, index: u32) -> Self::Mut<'a>;
}
pub struct ListRef<'a, E: Element> {
    raw: ListReader<'a>,
    marker: PhantomData<E>,
}
impl<E: Element> Copy for ListRef<'_, E> {}
impl<E: Element> Clone for ListRef<'_, E> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<'a, E: Element> ListRef<'a, E> {
    pub fn len(&self) -> usize {
        self.raw.len() as usize
    }
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }
    pub fn get(&self, index: usize) -> Option<E::Ref<'a>> {
        (index < self.len()).then(|| {
            E::get(
                self.raw,
                u32::try_from(index).expect("index is below encoded length"),
            )
        })
    }
    pub fn iter(&self) -> ListIter<'a, E> {
        ListIter {
            list: *self,
            index: 0,
        }
    }
}
pub struct ListIter<'a, E: Element> {
    list: ListRef<'a, E>,
    index: usize,
}
impl<'a, E: Element> Iterator for ListIter<'a, E> {
    type Item = E::Ref<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        let v = self.list.get(self.index);
        if v.is_some() {
            self.index += 1;
        }
        v
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.list.len() - self.index;
        (n, Some(n))
    }
}
impl<E: Element> ExactSizeIterator for ListIter<'_, E> {}
impl<E: Element> core::iter::FusedIterator for ListIter<'_, E> {}
impl<'a, E: Element> IntoIterator for ListRef<'a, E> {
    type Item = E::Ref<'a>;
    type IntoIter = ListIter<'a, E>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
pub struct ListMut<'a, E: Element> {
    raw: ListBuilder<'a>,
    marker: PhantomData<E>,
}
impl<E: Element> ListMut<'_, E> {
    pub fn len(&self) -> usize {
        self.raw.len() as usize
    }
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }
    pub fn get_mut(&mut self, index: usize) -> Option<E::Mut<'_>> {
        (index < self.len()).then(|| {
            E::get_mut(
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
    pub fn try_for_each_mut(
        &mut self,
        mut f: impl for<'b> FnMut(usize, E::Mut<'b>) -> Result<()>,
    ) -> Result<()> {
        for i in 0..self.len() {
            Location::Index(i).run(|| f(i, self.get_mut(i).unwrap()))?;
        }
        Ok(())
    }
}
impl<E: Element> PointerType for List<E> {
    type Ref<'a> = ListRef<'a, E>;
    type Mut<'a> = ListMut<'a, E>;
    fn acquire<'a>(p: PointerBuilder<'a>) -> Result<OccupiedField<'a, Self>> {
        present(&p)?;
        let r = p.as_reader().get_list(E::SIZE, None)?;
        if E::STRUCT.is_some_and(|s| !r.struct_layout_fits(s)) {
            return Ok(OccupiedField::new(Acquired::Upgrade(
                p,
                Error::from_kind(ErrorKind::NeedsUpgrade),
            )));
        }
        Ok(OccupiedField::new(Acquired::Ready(Self::ensure(p, None))))
    }
    fn read<'a>(p: PointerReader<'a>, d: Option<&'a [Word]>) -> Result<Self::Ref<'a>> {
        Ok(ListRef {
            raw: p.get_list(E::SIZE, d)?,
            marker: PhantomData,
        })
    }
    fn init<'a>(p: PointerBuilder<'a>, n: usize) -> Result<Self::Mut<'a>> {
        let n = count(n, E::STRUCT.map_or(0, |s| s.total() as usize))?;
        let raw = match E::STRUCT {
            Some(s) => p.init_struct_list(n, s),
            None => p.init_list(E::SIZE, n),
        };
        Ok(ListMut {
            raw,
            marker: PhantomData,
        })
    }
    fn edit<'a>(p: PointerBuilder<'a>) -> Result<Self::Mut<'a>> {
        if p.is_null() {
            return Err(Error::from_kind(ErrorKind::NotPresent));
        }
        let r = p.as_reader().get_list(E::SIZE, None)?;
        if let Some(s) = E::STRUCT {
            if !r.struct_layout_fits(s) {
                return Err(Error::from_kind(ErrorKind::NeedsUpgrade));
            }
        }
        Self::ensure(p, None)
    }
    fn ensure<'a>(p: PointerBuilder<'a>, d: Option<&'a [Word]>) -> Result<Self::Mut<'a>> {
        let raw = match E::STRUCT {
            Some(s) => p.get_struct_list(s, d)?,
            None => p.get_list(E::SIZE, d)?,
        };
        Ok(ListMut {
            raw,
            marker: PhantomData,
        })
    }
}
impl<'a, E: Element> PointerField<'a, List<E>> {
    pub fn init(self, n: usize) -> Result<ListMut<'a, E>> {
        self.initialize(n, true)
    }
    pub fn replace(self, n: usize) -> Result<ListMut<'a, E>> {
        self.initialize(n, false)
    }
    pub fn copy_from(mut self, value: ListRef<'_, E>) -> Result<()> {
        let location = self.location;
        location.run(|| {
            self.slot
                .pointer()
                .stage(|mut p| p.set_list(&value.raw, false))?;
            self.slot.commit();
            Ok(())
        })
    }
    pub fn init_with(
        self,
        n: usize,
        f: impl for<'b> FnMut(usize, E::Mut<'b>) -> Result<()>,
    ) -> Result<()> {
        self.fill(n, true, f)
    }
    pub fn replace_with(
        self,
        n: usize,
        f: impl for<'b> FnMut(usize, E::Mut<'b>) -> Result<()>,
    ) -> Result<()> {
        self.fill(n, false, f)
    }
    fn fill(
        self,
        n: usize,
        vacant: bool,
        mut f: impl for<'b> FnMut(usize, E::Mut<'b>) -> Result<()>,
    ) -> Result<()> {
        Pending::new(self, n, vacant)?
            .fill_with(|mut list| list.try_for_each_mut(&mut f))?
            .commit()
    }
}
impl<'a, E: Element> VacantField<'a, List<E>> {
    pub fn init(self, n: usize) -> Result<ListMut<'a, E>> {
        self.0.initialize(n, false)
    }
}
pub struct ListScalar<'a, T: crate::private::layout::PrimitiveElement> {
    raw: ListBuilder<'a>,
    index: u32,
    marker: PhantomData<T>,
}
impl<T: crate::private::layout::PrimitiveElement> ListScalar<'_, T> {
    pub fn set(self, value: T) {
        T::set(&self.raw, self.index, value);
    }
}
macro_rules! elements {($($t:ty => $size:ident),*)=>{$(impl Element for $t {
    type Ref<'a>=Self;type Mut<'a>=ListScalar<'a,Self>;
    const SIZE:ElementSize=ElementSize::$size;
    fn get<'a>(raw:ListReader<'a>,i:u32)->Self{<Self as layout::PrimitiveElement>::get(&raw,i)}
    fn get_mut<'a>(raw:ListBuilder<'a>,index:u32)->Self::Mut<'a>{ListScalar{raw,index,marker:PhantomData}}
})*}}
elements!(() => Void,bool => Bit,u8 => Byte,u16 => TwoBytes,u32 => FourBytes,u64 => EightBytes,i8 => Byte,i16 => TwoBytes,i32 => FourBytes,i64 => EightBytes,f32 => FourBytes,f64 => EightBytes);
macro_rules! pointer_element {
    ($t:ty) => {
        impl Element for $t {
            type Ref<'a> = Result<<Self as PointerType>::Ref<'a>>;
            type Mut<'a> = PointerField<'a, Self>;
            const SIZE: ElementSize = ElementSize::Pointer;
            fn get<'a>(raw: ListReader<'a>, i: u32) -> Self::Ref<'a> {
                Self::read(raw.get_pointer_element(i), None)
                    .map_err(|e| Location::Index(i as usize).error(e))
            }
            fn get_mut<'a>(raw: ListBuilder<'a>, i: u32) -> Self::Mut<'a> {
                PointerField::element(raw.get_pointer_element(i), i)
            }
        }
    };
}
pointer_element!(Text);
pointer_element!(Data);
impl<E: Element> Element for List<E> {
    type Ref<'a> = Result<ListRef<'a, E>>;
    type Mut<'a> = ListField<'a, E>;
    const SIZE: ElementSize = ElementSize::Pointer;
    fn get<'a>(raw: ListReader<'a>, i: u32) -> Self::Ref<'a> {
        Self::read(raw.get_pointer_element(i), None)
            .map_err(|e| Location::Index(i as usize).error(e))
    }
    fn get_mut<'a>(raw: ListBuilder<'a>, i: u32) -> Self::Mut<'a> {
        PointerField::element(raw.get_pointer_element(i), i)
    }
}
/// Escape hatch for generic pointer parameters and AnyPointer. This preserves
/// existing capability contexts and uses strict staged copies.
pub struct Generic<T: crate::traits::Owned>(PhantomData<T>);
impl<T: crate::traits::Owned> PointerType for Generic<T> {
    type Ref<'a> = T::Reader<'a>;
    type Mut<'a> = T::Builder<'a>;
    fn acquire<'a>(p: PointerBuilder<'a>) -> Result<OccupiedField<'a, Self>> {
        present(&p)?;
        Self::read(p.as_reader(), None)?;
        match Self::check_edit(&p) {
            Ok(()) => Ok(OccupiedField::new(Acquired::Ready(Self::ensure(p, None)))),
            Err(error) => Ok(OccupiedField::new(Acquired::Upgrade(p, error))),
        }
    }
    fn read<'a>(p: PointerReader<'a>, d: Option<&'a [Word]>) -> Result<Self::Ref<'a>> {
        crate::traits::FromPointerReader::get_from_pointer(&p, d)
    }
    fn init<'a>(p: PointerBuilder<'a>, n: usize) -> Result<Self::Mut<'a>> {
        Ok(crate::traits::FromPointerBuilder::init_pointer(
            p,
            count(n, 0)?,
        ))
    }
    fn edit<'a>(p: PointerBuilder<'a>) -> Result<Self::Mut<'a>> {
        Self::check_edit(&p)?;
        crate::traits::FromPointerBuilder::get_from_pointer(p, None)
    }
    fn ensure<'a>(p: PointerBuilder<'a>, d: Option<&'a [Word]>) -> Result<Self::Mut<'a>> {
        crate::traits::FromPointerBuilder::get_from_pointer(p, d)
    }
}
impl<T: crate::traits::Owned> Generic<T> {
    fn check_edit(p: &PointerBuilder<'_>) -> Result<()> {
        use crate::introspect::TypeVariant;
        if p.is_null() {
            return Err(Error::from_kind(ErrorKind::NotPresent));
        }
        fn size(raw: crate::introspect::RawBrandedStructSchema) -> Result<StructSize> {
            let crate::schema_capnp::node::Struct(s) =
                crate::schema::StructSchema::new(raw).get_proto().which()?
            else {
                return Err(Error::failed("struct metadata has another kind".into()));
            };
            Ok(StructSize {
                data: s.get_data_word_count(),
                pointers: s.get_pointer_count(),
            })
        }
        let reader = p.as_reader();
        match T::introspect().which() {
            TypeVariant::Struct(raw) => {
                let want = size(raw)?;
                let have = reader.get_struct(None)?;
                if have.get_data_section_size() < u32::from(want.data) * 64
                    || have.get_pointer_section_size() < want.pointers
                {
                    return Err(Error::from_kind(ErrorKind::NeedsUpgrade));
                }
            }
            TypeVariant::List(element) => {
                let have = reader.get_list(element.expected_element_size(), None)?;
                if let TypeVariant::Struct(raw) = element.which() {
                    if !have.struct_layout_fits(size(raw)?) {
                        return Err(Error::from_kind(ErrorKind::NeedsUpgrade));
                    }
                }
            }
            TypeVariant::Text => {
                reader.get_text(None)?;
            }
            TypeVariant::Data => {
                reader.get_data(None)?;
            }
            TypeVariant::Capability | TypeVariant::Interface(_) => {
                reader.get_capability()?;
            }
            TypeVariant::AnyPointer => {
                reader.get_pointer_type()?;
            }
            _ => {
                return Err(Error::failed(
                    "generic pointer metadata is not a pointer type".into(),
                ))
            }
        }
        Ok(())
    }
}
impl<T: crate::traits::Owned> PointerField<'_, Generic<T>> {
    pub fn copy_from(mut self, value: T::Reader<'_>) -> Result<()> {
        let location = self.location;
        location.run(|| {
            self.slot
                .pointer()
                .stage(|p| crate::traits::SetterInput::<T>::set_pointer_builder(p, value, false))?;
            self.slot.commit();
            Ok(())
        })
    }
}
pub struct Capability<C: crate::capability::FromClientHook>(PhantomData<C>);
impl<C: crate::capability::FromClientHook> PointerType for Capability<C> {
    type Ref<'a> = C;
    type Mut<'a> = C;
    fn acquire<'a>(p: PointerBuilder<'a>) -> Result<OccupiedField<'a, Self>> {
        present(&p)?;
        // Keep the one acquired hook reference through entry consumption/drop.
        Ok(OccupiedField::new(Acquired::Ready(Ok(Self::read(
            p.as_reader(),
            None,
        )?))))
    }
    fn read<'a>(p: PointerReader<'a>, _: Option<&'a [Word]>) -> Result<C> {
        Ok(C::new(p.get_capability()?))
    }
    fn init<'a>(_: PointerBuilder<'a>, _: usize) -> Result<C> {
        Err(Error::from_kind(ErrorKind::Unimplemented))
    }
    fn edit<'a>(p: PointerBuilder<'a>) -> Result<C> {
        Ok(C::new(p.get_capability()?))
    }
    fn ensure<'a>(p: PointerBuilder<'a>, _: Option<&'a [Word]>) -> Result<C> {
        Self::edit(p)
    }
}
impl<C: crate::capability::FromClientHook> PointerField<'_, Capability<C>> {
    pub fn copy_from(mut self, value: C) -> Result<()> {
        let location = self.location;
        location.run(|| {
            self.slot.pointer().stage(|mut p| {
                p.set_capability(value.into_client_hook());
                Ok(())
            })?;
            self.slot.commit();
            Ok(())
        })
    }
}
impl<T: crate::traits::Owned> Element for Generic<T> {
    type Ref<'a> = Result<T::Reader<'a>>;
    type Mut<'a> = PointerField<'a, Self>;
    const SIZE: ElementSize = ElementSize::Pointer;
    fn get<'a>(raw: ListReader<'a>, i: u32) -> Self::Ref<'a> {
        Self::read(raw.get_pointer_element(i), None)
            .map_err(|e| Location::Index(i as usize).error(e))
    }
    fn get_mut<'a>(raw: ListBuilder<'a>, i: u32) -> Self::Mut<'a> {
        PointerField::element(raw.get_pointer_element(i), i)
    }
}
impl<C: crate::capability::FromClientHook> Element for Capability<C> {
    type Ref<'a> = Result<C>;
    type Mut<'a> = PointerField<'a, Self>;
    const SIZE: ElementSize = ElementSize::Pointer;
    fn get<'a>(raw: ListReader<'a>, i: u32) -> Self::Ref<'a> {
        Self::read(raw.get_pointer_element(i), None)
            .map_err(|e| Location::Index(i as usize).error(e))
    }
    fn get_mut<'a>(raw: ListBuilder<'a>, i: u32) -> Self::Mut<'a> {
        PointerField::element(raw.get_pointer_element(i), i)
    }
}
pub struct EnumElement<'a, E: From<u16> + Into<u16>> {
    raw: ListBuilder<'a>,
    index: u32,
    marker: PhantomData<E>,
}
impl<'a, E: From<u16> + Into<u16>> EnumElement<'a, E> {
    #[doc(hidden)]
    pub fn new(raw: ListBuilder<'a>, index: u32) -> Self {
        assert!(index < raw.len());
        Self {
            raw,
            index,
            marker: PhantomData,
        }
    }
    pub fn set(self, value: E) {
        <u16 as layout::PrimitiveElement>::set(&self.raw, self.index, value.into());
    }
}

/// Native slice access is restricted to primitive representations with no
/// invalid bit patterns. Bool and struct/enum lists never manufacture slices.
pub trait NativeElement: Element + layout::PrimitiveElement + sealed::Native {}
macro_rules! native { ($($t:ty),*) => { $(impl sealed::Native for $t {} impl NativeElement for $t {})* } }
native!(u8, u16, u32, u64, i8, i16, i32, i64, f32, f64);
impl<'a, E: NativeElement> ListRef<'a, E> {
    pub fn as_slice(&self) -> Option<&'a [E]> {
        if self.raw.get_element_size() != E::SIZE
            || (core::mem::size_of::<E>() > 1 && !cfg!(target_endian = "little"))
        {
            return None;
        }
        let bytes = self.raw.into_raw_bytes();
        if bytes.is_empty() {
            return Some(&[]);
        }
        if bytes.as_ptr().align_offset(core::mem::align_of::<E>()) != 0 {
            return None;
        }
        // SAFETY: sealed native types accept all bits; actual physical stride,
        // alignment and endianness match E, and the reader already checked bounds.
        Some(unsafe { core::slice::from_raw_parts(bytes.as_ptr().cast::<E>(), self.len()) })
    }
}

/// Unsent RPC request with the generated field-operation parameter editor.
#[must_use = "an unsent request does nothing; send or forward it"]
pub struct Request<P: Schema, R: Schema + crate::traits::Pipelined> {
    hook: alloc::boxed::Box<dyn crate::private::capability::RequestHook>,
    marker: PhantomData<(P, R)>,
}
impl<P: Schema, R: Schema + crate::traits::Pipelined> Request<P, R> {
    /// Fill parameters before sending; failure drops the unsent request.
    pub fn with_params(
        mut self,
        fill: impl for<'a> FnOnce(P::Mut<'a>) -> Result<()>,
    ) -> Result<Self> {
        fill(self.edit())?;
        Ok(self)
    }
    #[doc(hidden)]
    pub fn new(hook: alloc::boxed::Box<dyn crate::private::capability::RequestHook>) -> Self {
        Self {
            hook,
            marker: PhantomData,
        }
    }
    pub fn edit(&mut self) -> P::Mut<'_> {
        self.hook
            .get()
            .builder
            .get_struct(P::SIZE, None)
            .expect("typed request root")
            .into()
    }
    pub fn copy_from(&mut self, params: P::Ref<'_>) -> Result<()> {
        use crate::traits::IntoInternalStructReader;
        self.hook
            .get()
            .builder
            .stage(|mut pointer| pointer.set_struct(&params.into_internal_struct_reader(), false))
    }
    /// Send and await completion, discarding response data while preserving errors
    /// and the method's cancellation policy. The result pipeline is dropped at send.
    pub fn send_ignoring_result(self) -> crate::capability::Promise<(), Error> {
        crate::capability::Request::<crate::any_pointer::Owned, crate::any_pointer::Owned>::new(
            self.hook,
        )
        .send_ignoring_result()
    }
}
impl<P: Schema, R: Schema + crate::traits::Pipelined + 'static> Request<P, R>
where
    R::Pipeline: crate::capability::FromTypelessPipeline,
{
    pub fn send(self) -> PendingCall<R> {
        let sent = self.hook.send();
        PendingCall {
            response: crate::capability::Promise::from_future(async move {
                Ok(Response {
                    hook: sent.promise.await?.hook,
                    marker: PhantomData,
                })
            }),
            pipeline: crate::capability::FromTypelessPipeline::new(sent.pipeline),
        }
    }
    pub fn send_for_pipeline(self) -> R::Pipeline {
        crate::capability::FromTypelessPipeline::new(self.hook.send_for_pipeline())
    }
}
/// Owns the response hook and lends results without allocating a native graph.
pub struct Response<R: Schema> {
    hook: alloc::boxed::Box<dyn crate::private::capability::ResponseHook>,
    marker: PhantomData<R>,
}
impl<R: Schema> Response<R> {
    pub fn read(&self) -> Result<R::Ref<'_>> {
        Ok(self.hook.get()?.reader.get_struct(None)?.into())
    }
}
/// Awaitable response plus a pipeline available before awaiting. Derived
/// capability clients retain their own hooks. This does not make local RPC Send.
#[must_use]
pub struct PendingCall<R: Schema + crate::traits::Pipelined> {
    response: crate::capability::Promise<Response<R>, Error>,
    pub pipeline: R::Pipeline,
}
// No field is exposed through a pinned projection; the only polled field is
// Promise, which is Unpin independently of its output type.
impl<R: Schema + crate::traits::Pipelined> Unpin for PendingCall<R> {}
impl<R: Schema + crate::traits::Pipelined> core::future::Future for PendingCall<R> {
    type Output = Result<Response<R>>;
    fn poll(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        core::pin::Pin::new(&mut self.get_mut().response).poll(cx)
    }
}
impl<R: Schema + crate::traits::Pipelined> PendingCall<R> {
    pub fn into_parts(self) -> (crate::capability::Promise<Response<R>, Error>, R::Pipeline) {
        (self.response, self.pipeline)
    }
}
#[must_use = "an unsent streaming request does nothing; send it"]
pub struct StreamingRequest<P: Schema> {
    hook: alloc::boxed::Box<dyn crate::private::capability::RequestHook>,
    marker: PhantomData<P>,
}
impl<P: Schema> StreamingRequest<P> {
    /// Fill parameters before sending; failure drops the unsent request.
    pub fn with_params(
        mut self,
        fill: impl for<'a> FnOnce(P::Mut<'a>) -> Result<()>,
    ) -> Result<Self> {
        fill(self.edit())?;
        Ok(self)
    }
    #[doc(hidden)]
    pub fn new(hook: alloc::boxed::Box<dyn crate::private::capability::RequestHook>) -> Self {
        Self {
            hook,
            marker: PhantomData,
        }
    }
    pub fn edit(&mut self) -> P::Mut<'_> {
        self.hook
            .get()
            .builder
            .get_struct(P::SIZE, None)
            .expect("typed request root")
            .into()
    }
    pub fn send(self) -> crate::capability::Promise<(), Error> {
        self.hook.send_streaming()
    }
}
