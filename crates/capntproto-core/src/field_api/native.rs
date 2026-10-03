//! Allocating native conversions. Unknown schema fields are discarded only by
//! explicit request. Pointer absence is preserved rather than materializing defaults.
use super::*;
use crate::traits::{Imbue, ImbueMut};
use alloc::{boxed::Box, string::String, vec::Vec};

/// A native struct contains only fields known to its generated schema.
/// Use a wire-level copy instead when unknown fields must survive forwarding.
#[derive(Clone, Copy, Debug)]
pub enum UnknownFields {
    Discard,
}

/// Separate from the wire reader's traversal limits. Counts expanded values,
/// including zero-size list elements, and copied text/data/opaque payload bytes.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub items: usize,
    pub bytes: usize,
    pub depth: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            items: 1_000_000,
            bytes: 64 * 1024 * 1024,
            depth: 64,
        }
    }
}
#[doc(hidden)]
pub struct Conversion {
    remaining: Limits,
}
impl Conversion {
    pub fn new(limits: Limits) -> Self {
        Self { remaining: limits }
    }
    pub fn items(&mut self, n: usize) -> Result<()> {
        self.remaining.items = self
            .remaining
            .items
            .checked_sub(n)
            .ok_or_else(|| Error::failed("native conversion item limit exceeded".into()))?;
        Ok(())
    }
    pub fn bytes(&mut self, n: usize) -> Result<()> {
        self.remaining.bytes = self
            .remaining
            .bytes
            .checked_sub(n)
            .ok_or_else(|| Error::failed("native conversion byte limit exceeded".into()))?;
        Ok(())
    }
    pub fn nested<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        self.remaining.depth = self
            .remaining
            .depth
            .checked_sub(1)
            .ok_or_else(|| Error::failed("native conversion depth limit exceeded".into()))?;
        let result = f(self);
        self.remaining.depth += 1;
        result
    }
}
/// Implemented by the generator when native values are enabled.
pub trait NativeSchema: Schema {
    type Value;
    #[doc(hidden)]
    fn read_value(raw: StructReader<'_>, budget: &mut Conversion) -> Result<Self::Value>;
    /// Writes into a newly initialized struct. Call `to_message` to obtain atomic
    /// publication: a failed conversion never returns the partial message.
    #[doc(hidden)]
    fn write_value(
        raw: StructBuilder<'_>,
        value: &Self::Value,
        budget: &mut Conversion,
    ) -> Result<()>;
}
/// Build an independent message, retaining capability hooks. No existing
/// destination is modified if conversion fails.
pub fn to_message<S: NativeSchema>(value: &S::Value, limits: Limits) -> Result<Message<S>> {
    (|| {
        let mut message = Message::<S>::new()?;
        let mut root: crate::any_pointer::Builder<'_> = message.storage.get_root()?;
        root.imbue_mut(&mut message.caps);
        let raw = root.builder.get_struct(S::SIZE, None)?;
        S::write_value(raw, value, &mut Conversion::new(limits))?;
        Ok(message)
    })()
    .map_err(|e| diagnostics::conversion(e, S::NAME, "encode"))
}
#[doc(hidden)]
pub trait NativePointer: PointerType {
    type Value;
    fn read_value(raw: PointerReader<'_>, budget: &mut Conversion) -> Result<Self::Value>;
    fn write_value(
        raw: PointerBuilder<'_>,
        value: &Self::Value,
        budget: &mut Conversion,
    ) -> Result<()>;
}
#[doc(hidden)]
pub fn read_optional<K: NativePointer>(
    raw: PointerReader<'_>,
    budget: &mut Conversion,
) -> Result<Option<K::Value>> {
    if raw.is_null() {
        Ok(None)
    } else {
        K::read_value(raw, budget).map(Some)
    }
}
#[doc(hidden)]
pub fn write_optional<K: NativePointer>(
    mut raw: PointerBuilder<'_>,
    value: &Option<K::Value>,
    budget: &mut Conversion,
) -> Result<()> {
    match value {
        Some(v) => K::write_value(raw, v, budget),
        None => {
            raw.clear();
            Ok(())
        }
    }
}
impl NativePointer for Text {
    type Value = String;
    fn read_value(raw: PointerReader<'_>, b: &mut Conversion) -> Result<String> {
        let text = Text::read(raw, None)?;
        b.bytes(text.len())?;
        Ok(text.into())
    }
    fn write_value(mut raw: PointerBuilder<'_>, v: &String, b: &mut Conversion) -> Result<()> {
        b.bytes(v.len())?;
        count(
            v.len()
                .checked_add(1)
                .ok_or_else(|| Error::from_kind(ErrorKind::LengthOverflow))?,
            0,
        )?;
        raw.set_text(v.as_str().into());
        Ok(())
    }
}
impl NativePointer for Data {
    type Value = Vec<u8>;
    fn read_value(raw: PointerReader<'_>, b: &mut Conversion) -> Result<Self::Value> {
        let data = Data::read(raw, None)?;
        b.bytes(data.len())?;
        Ok(data.into())
    }
    fn write_value(mut raw: PointerBuilder<'_>, v: &Self::Value, b: &mut Conversion) -> Result<()> {
        b.bytes(v.len())?;
        count(v.len(), 0)?;
        raw.set_data(v);
        Ok(())
    }
}
impl<S: NativeSchema> NativePointer for Struct<S> {
    type Value = Box<S::Value>;
    fn read_value(raw: PointerReader<'_>, b: &mut Conversion) -> Result<Self::Value> {
        S::read_value(raw.get_struct(None)?, b).map(Box::new)
    }
    fn write_value(raw: PointerBuilder<'_>, v: &Self::Value, b: &mut Conversion) -> Result<()> {
        S::write_value(raw.init_struct(S::SIZE), v, b)
    }
}
impl<C: crate::capability::FromClientHook> NativePointer for Capability<C> {
    type Value = C;
    fn read_value(raw: PointerReader<'_>, _: &mut Conversion) -> Result<C> {
        Self::read(raw, None)
    }
    fn write_value(mut raw: PointerBuilder<'_>, v: &C, _: &mut Conversion) -> Result<()> {
        raw.set_capability(v.as_client_hook().add_ref());
        Ok(())
    }
}
/// An untyped or generic payload has no concrete native schema at generation
/// time. Keep an independent wire object and its capability table in this leaf.
/// It preserves unknown data; it is not a recursively decoded native struct.
pub struct OpaqueValue<T: crate::traits::Owned> {
    storage: crate::message::Builder<crate::message::HeapAllocator>,
    caps: layout::CapTable,
    marker: PhantomData<T>,
}
impl<T: crate::traits::Owned> OpaqueValue<T> {
    pub fn read(&self) -> Result<T::Reader<'_>> {
        let mut root: crate::any_pointer::Reader<'_> = self.storage.get_root_as_reader()?;
        root.imbue(&self.caps);
        root.get_as()
    }
}
impl<T: crate::traits::Owned> NativePointer for Generic<T> {
    type Value = OpaqueValue<T>;
    fn read_value(raw: PointerReader<'_>, b: &mut Conversion) -> Result<Self::Value> {
        let size = raw.total_size()?;
        b.bytes(
            usize::try_from(size.word_count)
                .ok()
                .and_then(|n| n.checked_mul(8))
                .ok_or_else(|| Error::from_kind(ErrorKind::LengthOverflow))?,
        )?;
        b.items(
            usize::try_from(size.cap_count)
                .map_err(|_| Error::from_kind(ErrorKind::LengthOverflow))?,
        )?;
        let mut value = OpaqueValue {
            storage: crate::message::Builder::new_default(),
            caps: Vec::new(),
            marker: PhantomData,
        };
        let mut root: crate::any_pointer::Builder<'_> = value.storage.get_root()?;
        root.imbue_mut(&mut value.caps);
        root.builder.copy_from(raw, false)?;
        Ok(value)
    }
    fn write_value(mut raw: PointerBuilder<'_>, v: &Self::Value, b: &mut Conversion) -> Result<()> {
        let mut root: crate::any_pointer::Reader<'_> = v.storage.get_root_as_reader()?;
        root.imbue(&v.caps);
        let size = root.reader.total_size()?;
        b.bytes(
            usize::try_from(size.word_count)
                .ok()
                .and_then(|n| n.checked_mul(8))
                .ok_or_else(|| Error::from_kind(ErrorKind::LengthOverflow))?,
        )?;
        b.items(
            usize::try_from(size.cap_count)
                .map_err(|_| Error::from_kind(ErrorKind::LengthOverflow))?,
        )?;
        raw.copy_from(root.reader, false)
    }
}
#[doc(hidden)]
pub trait NativeElement: Element {
    type Value;
    fn read_value(raw: ListReader<'_>, i: u32, b: &mut Conversion) -> Result<Self::Value>;
    fn write_value(raw: ListBuilder<'_>, i: u32, v: &Self::Value, b: &mut Conversion)
        -> Result<()>;
}
macro_rules! primitive {
    ($($t:ty),*) => { $(impl NativeElement for $t {
        type Value = $t;
        fn read_value(raw: ListReader<'_>, i: u32, _: &mut Conversion) -> Result<Self::Value> { Ok(<$t as layout::PrimitiveElement>::get(&raw, i)) }
        fn write_value(raw: ListBuilder<'_>, i: u32, v: &Self::Value, _: &mut Conversion) -> Result<()> { <$t as layout::PrimitiveElement>::set(&raw, i, *v); Ok(()) }
    })* };
}
primitive!((), bool, u8, u16, u32, u64, i8, i16, i32, i64, f32, f64);
macro_rules! pointer_element {
    ($kind:ty $(, $param:ident : $bound:path)?) => {
        impl<$($param: $bound)?> NativeElement for $kind {
            type Value = Option<<Self as NativePointer>::Value>;
            fn read_value(raw: ListReader<'_>, i: u32, b: &mut Conversion) -> Result<Self::Value> { read_optional::<Self>(raw.get_pointer_element(i), b) }
            fn write_value(raw: ListBuilder<'_>, i: u32, v: &Self::Value, b: &mut Conversion) -> Result<()> { write_optional::<Self>(raw.get_pointer_element(i), v, b) }
        }
    };
}
pointer_element!(Text);
pointer_element!(Data);
pointer_element!(List<E>, E: NativeElement);
pointer_element!(Generic<T>, T: crate::traits::Owned);
pointer_element!(Capability<C>, C: crate::capability::FromClientHook);
impl<E: NativeElement> NativePointer for List<E> {
    type Value = Vec<E::Value>;
    fn read_value(raw: PointerReader<'_>, b: &mut Conversion) -> Result<Self::Value> {
        b.nested(|b| {
            let list = raw.get_list(E::SIZE, None)?;
            b.items(list.len() as usize)?;
            (0..list.len())
                .map(|i| Location::Index(i as usize).run(|| E::read_value(list, i, b)))
                .collect()
        })
    }
    fn write_value(raw: PointerBuilder<'_>, v: &Self::Value, b: &mut Conversion) -> Result<()> {
        b.nested(|b| {
            b.items(v.len())?;
            let mut list = List::<E>::init(raw, v.len())?;
            for (i, value) in v.iter().enumerate() {
                Location::Index(i).run(|| {
                    E::write_value(
                        list.raw.reborrow(),
                        u32::try_from(i).expect("index is below checked wire length"),
                        value,
                        b,
                    )
                })?;
            }
            Ok(())
        })
    }
}
