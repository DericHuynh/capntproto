//! Typed parameter/result payload access independent of allocation and publication.
use crate::{
    field_api::{FixedField, FixedSchema, FixedStruct, Struct},
    message, MessageSize, Result,
};

/// The editor borrows its owner and cannot outlive the payload storage.
/// Fixed providers report capacity failure and retain their previous root;
/// size hints never enlarge their caller-supplied capacity.
pub trait PayloadStorage<T> {
    type Builder<'a>
    where
        Self: 'a;
    fn try_get(&mut self, hint: Option<MessageSize>) -> Result<Self::Builder<'_>>;
    fn try_init(&mut self, hint: Option<MessageSize>) -> Result<Self::Builder<'_>>;
}

impl<T, H: PayloadStorage<T> + ?Sized> PayloadStorage<T> for &mut H {
    type Builder<'a>
        = H::Builder<'a>
    where
        Self: 'a;
    fn try_get(&mut self, hint: Option<MessageSize>) -> Result<Self::Builder<'_>> {
        (**self).try_get(hint)
    }
    fn try_init(&mut self, hint: Option<MessageSize>) -> Result<Self::Builder<'_>> {
        (**self).try_init(hint)
    }
}
impl<T, H: PayloadStorage<T> + ?Sized> PayloadStorage<T> for core::cell::RefMut<'_, H> {
    type Builder<'a>
        = H::Builder<'a>
    where
        Self: 'a;
    fn try_get(&mut self, hint: Option<MessageSize>) -> Result<Self::Builder<'_>> {
        (**self).try_get(hint)
    }
    fn try_init(&mut self, hint: Option<MessageSize>) -> Result<Self::Builder<'_>> {
        (**self).try_init(hint)
    }
}

impl<T: FixedSchema> PayloadStorage<T> for message::FixedBuilder<'_> {
    type Builder<'a>
        = FixedStruct<'a, T::Schema>
    where
        Self: 'a;
    fn try_get(&mut self, _: Option<MessageSize>) -> Result<Self::Builder<'_>> {
        if self
            .get_root_as_reader::<crate::any_pointer::Reader<'_>>()?
            .is_null()
        {
            self.try_init_root::<T::Schema>()
        } else {
            self.try_upgrade_root::<T::Schema>()
        }
    }
    fn try_init(&mut self, _: Option<MessageSize>) -> Result<Self::Builder<'_>> {
        self.try_init_root::<T::Schema>()
    }
}

// An RPC payload can own the content field inside a Call or Return envelope.
// Transferring its full borrow preserves the independently initialized header.
impl<T: FixedSchema> PayloadStorage<T> for FixedField<'_, Struct<T::Schema>> {
    type Builder<'a>
        = FixedStruct<'a, T::Schema>
    where
        Self: 'a;
    fn try_get(&mut self, _: Option<MessageSize>) -> Result<Self::Builder<'_>> {
        if self.is_null()? {
            self.reborrow().init()
        } else {
            self.reborrow().upgrade()
        }
    }
    fn try_init(&mut self, _: Option<MessageSize>) -> Result<Self::Builder<'_>> {
        self.reborrow().replace()
    }
}

#[cfg(feature = "alloc")]
impl<T: crate::traits::Owned> PayloadStorage<T>
    for alloc::boxed::Box<dyn crate::private::capability::ResultsHook>
{
    type Builder<'a>
        = T::Builder<'a>
    where
        Self: 'a;
    #[inline(always)]
    fn try_get(&mut self, hint: Option<MessageSize>) -> Result<Self::Builder<'_>> {
        self.get_with_size_hint(hint)?.get_as()
    }
    #[inline(always)]
    fn try_init(&mut self, hint: Option<MessageSize>) -> Result<Self::Builder<'_>> {
        Ok(self.get_with_size_hint(hint)?.init_as())
    }
}
