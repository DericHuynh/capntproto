//! Explicit wire indices, separate from hosted capability hooks.
#![forbid(unsafe_code)]
use super::*;

pub struct WireCapability<I>(PhantomData<I>);
impl<I> PointerType for WireCapability<I> {
    type Ref<'a> = crate::wire_capability::Reader<'a, I>;
    type Mut<'a> = crate::wire_capability::Reader<'a, I>;
    fn read<'a>(p: PointerReader<'a>, _: Option<&'a [Word]>) -> Result<Self::Ref<'a>> {
        crate::traits::FromPointerReader::get_from_pointer(&p, None)
    }
    fn acquire<'a>(p: PointerBuilder<'a>) -> Result<OccupiedField<'a, Self>> {
        Ok(OccupiedField::new(Acquired::Ready(Ok(Self::read(
            p.into_reader(),
            None,
        )?))))
    }
    fn init<'a>(_: PointerBuilder<'a>, _: usize) -> Result<Self::Mut<'a>> {
        Err(Error::from_kind(ErrorKind::Unimplemented))
    }
    fn edit<'a>(p: PointerBuilder<'a>) -> Result<Self::Mut<'a>> {
        Self::read(p.into_reader(), None)
    }
    fn ensure<'a>(p: PointerBuilder<'a>, _: Option<&'a [Word]>) -> Result<Self::Mut<'a>> {
        Self::edit(p)
    }
}
impl<I> Element for WireCapability<I> {
    type Ref<'a> = Result<crate::wire_capability::Reader<'a, I>>;
    type Mut<'a> = PointerField<'a, Self>;
    const SIZE: ElementSize = ElementSize::Pointer;
    fn get<'a>(r: ListReader<'a>, i: u32) -> Self::Ref<'a> {
        Self::read(r.get_pointer_element(i), None)
    }
    fn get_mut<'a>(r: ListBuilder<'a>, i: u32) -> Self::Mut<'a> {
        PointerField::element(r.get_pointer_element(i), i)
    }
}
impl<I> FixedElement for WireCapability<I> {
    type Fixed<'a> = FixedField<'a, Self>;
    fn get_fixed<'a>(r: ListBuilder<'a>, i: u32) -> Self::Fixed<'a> {
        FixedField(PointerField::element(r.get_pointer_element(i), i))
    }
}
impl<I> FixedField<'_, WireCapability<I>> {
    /// Encode a descriptor-table index. This publishes no references by itself.
    pub fn set_index(mut self, index: u32) -> Result<()> {
        self.0.slot.pointer().set_wire_capability(index)?;
        self.0.slot.commit();
        Ok(())
    }
}
