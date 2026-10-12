//! Allocating staged replacement and orphan ownership operations.
use super::*;

/// Message-scoped identity token; it grants no access to the arena by itself.
pub struct Orphanage<'message> {
    pub(super) identity: layout::ArenaIdentity,
    pub(super) lifetime: PhantomData<&'message mut ()>,
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
    pub(super) raw: StructBuilder<'message>,
    pub(super) schema: PhantomData<S>,
    pub(super) brand: InvariantBrand<'brand>,
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
    pub(super) fn new(mut field: PointerField<'a, K>, n: usize, vacant: bool) -> Result<Self> {
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
