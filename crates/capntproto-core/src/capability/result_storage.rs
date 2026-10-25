//! Typed output access independent of its allocation and publication owner.
use crate::{
    field_api::{FixedField, FixedSchema, FixedStruct, Struct},
    message, MessageSize, Result,
};

/// The editor borrows its owner and cannot outlive the response storage.
/// Fixed providers report capacity failure and retain their previous root;
/// size hints never enlarge their caller-supplied capacity.
pub trait ResultStorage<T> {
    type Builder<'a>
    where
        Self: 'a;
    fn try_get(&mut self, hint: Option<MessageSize>) -> Result<Self::Builder<'_>>;
    fn try_init(&mut self, hint: Option<MessageSize>) -> Result<Self::Builder<'_>>;
}

impl<T, H: ResultStorage<T> + ?Sized> ResultStorage<T> for &mut H {
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
impl<T, H: ResultStorage<T> + ?Sized> ResultStorage<T> for core::cell::RefMut<'_, H> {
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

impl<T: FixedSchema> ResultStorage<T> for message::FixedBuilder<'_> {
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

// An RPC result owns the content field inside a Return envelope. Transferring
// this field's full borrow preserves the independently initialized header.
impl<T: FixedSchema> ResultStorage<T> for FixedField<'_, Struct<T::Schema>> {
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
impl<T: crate::traits::Owned> ResultStorage<T>
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
