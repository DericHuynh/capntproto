//! Borrowed capability pointers for allocation-free RPC bindings.
//!
//! These are message-local indices, not callable clients. Only the RPC engine
//! can resolve an index using the descriptor table of the same message. Copying
//! an index into another message without remapping its table is not supported.
#![forbid(unsafe_code)]

use crate::{private::layout::PointerReader, traits::FromPointerReader, Result};
use core::marker::PhantomData;
pub mod path;
pub use path::{CapabilityPath, Pipeline};
mod map;
pub use map::IndexMap;

pub struct Reader<'a, I> {
    index: u32,
    origin: &'a dyn crate::private::arena::ReaderArena,
    marker: PhantomData<(&'a (), I)>,
}
impl<I> Copy for Reader<'_, I> {}
impl<I> Clone for Reader<'_, I> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<I> Reader<'_, I> {
    /// An untrusted offset into the originating message's descriptor table.
    pub fn index(self) -> u32 {
        self.index
    }
    /// Whether this pointer was read by the exact message reader containing
    /// `content`. A matching numeric index from another message is not authority.
    pub fn belongs_to(self, content: crate::any_pointer::Reader<'_>) -> bool {
        core::ptr::eq(self.origin, content.reader.arena_identity())
    }
}
impl<'a, I> FromPointerReader<'a> for Reader<'a, I> {
    fn get_from_pointer(pointer: &PointerReader<'a>, _: Option<&'a [crate::Word]>) -> Result<Self> {
        Ok(Self {
            index: pointer.get_capability_index()?,
            origin: pointer.arena_identity(),
            marker: PhantomData,
        })
    }
}
impl<I> crate::traits::SetterInput<I> for Reader<'_, I> {
    fn set_pointer_builder(
        _: crate::private::layout::PointerBuilder<'_>,
        _: Self,
        _: bool,
    ) -> Result<()> {
        // A descriptor remapper, not a bare index, must authorize a cross-message
        // copy. FixedField::set_index is the explicit wire-construction boundary.
        Err(crate::Error::from_kind(crate::ErrorKind::TypeMismatch))
    }
}

/// Generated interface identity; schema brands remain available via Introspect.
pub trait Interface: crate::introspect::Introspect {
    const TYPE_ID: u64;
}

/// A generated method's parameter/result schemas and protocol hints.
/// The constructor is for generators; callers use associated method constants.
pub struct Method<I, P: crate::field_api::Schema, R: crate::field_api::Schema> {
    ordinal: u16,
    cancellation: bool,
    no_pipelining: bool,
    marker: PhantomData<(I, P, R)>,
}
impl<I, P: crate::field_api::Schema, R: crate::field_api::Schema> Copy for Method<I, P, R> {}
impl<I, P: crate::field_api::Schema, R: crate::field_api::Schema> Clone for Method<I, P, R> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<I: Interface, P: crate::field_api::Schema, R: crate::field_api::Schema> Method<I, P, R> {
    #[doc(hidden)]
    pub const fn new(ordinal: u16, cancellation: bool, no_pipelining: bool) -> Self {
        Self {
            ordinal,
            cancellation,
            no_pipelining,
            marker: PhantomData,
        }
    }
    pub const fn interface_id(self) -> u64 {
        I::TYPE_ID
    }
    pub const fn ordinal(self) -> u16 {
        self.ordinal
    }
    pub const fn allow_cancellation(self) -> bool {
        self.cancellation
    }
    pub const fn no_promise_pipelining(self) -> bool {
        self.no_pipelining
    }
    pub fn read_params<'a>(self, content: crate::any_pointer::Reader<'a>) -> Result<P::Ref<'a>> {
        Ok(content.reader.get_struct(None)?.into())
    }
    pub fn read_results<'a>(self, content: crate::any_pointer::Reader<'a>) -> Result<R::Ref<'a>> {
        Ok(content.reader.get_struct(None)?.into())
    }
    pub fn pipeline(self, storage: &mut [u16]) -> Pipeline<'_, R, R> {
        Pipeline::new(storage)
    }
}

/// Streaming methods have parameters and flow-control acknowledgements, without
/// a result schema or promise-pipeline API.
pub struct StreamingMethod<I, P: crate::field_api::Schema> {
    ordinal: u16,
    cancellation: bool,
    marker: PhantomData<(I, P)>,
}
impl<I, P: crate::field_api::Schema> Copy for StreamingMethod<I, P> {}
impl<I, P: crate::field_api::Schema> Clone for StreamingMethod<I, P> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<I: Interface, P: crate::field_api::Schema> StreamingMethod<I, P> {
    #[doc(hidden)]
    pub const fn new(ordinal: u16, cancellation: bool) -> Self {
        Self {
            ordinal,
            cancellation,
            marker: PhantomData,
        }
    }
    pub const fn interface_id(self) -> u64 {
        I::TYPE_ID
    }
    pub const fn ordinal(self) -> u16 {
        self.ordinal
    }
    pub const fn allow_cancellation(self) -> bool {
        self.cancellation
    }
    pub fn read_params<'a>(self, content: crate::any_pointer::Reader<'a>) -> Result<P::Ref<'a>> {
        Ok(content.reader.get_struct(None)?.into())
    }
}

/// Generated scheduling hints available before a service future is polled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MethodInfo {
    pub allow_cancellation: bool,
    pub no_promise_pipelining: bool,
    pub streaming: bool,
}
/// A statically dispatched service over borrowed wire views. The caller retains
/// both messages, pins/polls the returned future and admits its eventual result.
/// Generated adapters match the declaring interface and method ordinal first.
/// No executor, heap, transport or capability ownership is implied by this trait.
pub trait Service {
    fn method_info(&self, interface_id: u64, method_id: u16) -> Option<MethodInfo>;
    fn dispatch<'call>(
        &'call mut self,
        interface_id: u64,
        method_id: u16,
        params: crate::any_pointer::Reader<'call>,
        results: crate::field_api::FixedField<
            'call,
            crate::field_api::Generic<crate::any_pointer::Owned>,
        >,
    ) -> impl core::future::Future<Output = Result<()>> + 'call;
}

/// Borrow an existing dispatcher as the concrete owner of one invocation.
impl<S: Service + ?Sized> Service for &mut S {
    fn method_info(&self, interface_id: u64, method_id: u16) -> Option<MethodInfo> {
        (**self).method_info(interface_id, method_id)
    }
    async fn dispatch<'call>(
        &'call mut self,
        interface_id: u64,
        method_id: u16,
        params: crate::any_pointer::Reader<'call>,
        results: crate::field_api::FixedField<
            'call,
            crate::field_api::Generic<crate::any_pointer::Owned>,
        >,
    ) -> Result<()> {
        (**self)
            .dispatch(interface_id, method_id, params, results)
            .await
    }
}

/// A borrowed service with an explicit application-selected call context.
/// Generated adapters retain typed parameter/result schemas and inheritance.
/// The context may provide admitted capability lookup, nested calls, time or
/// cancellation policy; the serialization core imposes no runtime dependency.
pub trait ContextService<C> {
    fn method_info(&self, interface_id: u64, method_id: u16) -> Option<MethodInfo>;
    fn dispatch_with_context<'call>(
        &'call mut self,
        interface_id: u64,
        method_id: u16,
        params: crate::any_pointer::Reader<'call>,
        results: crate::field_api::FixedField<
            'call,
            crate::field_api::Generic<crate::any_pointer::Owned>,
        >,
        context: &'call mut C,
    ) -> impl core::future::Future<Output = Result<()>> + 'call;
}

impl<C, S: ContextService<C> + ?Sized> ContextService<C> for &mut S {
    fn method_info(&self, interface_id: u64, method_id: u16) -> Option<MethodInfo> {
        (**self).method_info(interface_id, method_id)
    }
    async fn dispatch_with_context<'call>(
        &'call mut self,
        interface_id: u64,
        method_id: u16,
        params: crate::any_pointer::Reader<'call>,
        results: crate::field_api::FixedField<
            'call,
            crate::field_api::Generic<crate::any_pointer::Owned>,
        >,
        context: &'call mut C,
    ) -> Result<()> {
        (**self)
            .dispatch_with_context(interface_id, method_id, params, results, context)
            .await
    }
}
