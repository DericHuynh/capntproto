//! Retain whole legacy incoming frames without prescribing a box or allocator.
use crate::IncomingMessage;
use capnp::{
    any_pointer,
    private::capability::{ParamsHook, ResponseHook},
};
#[cfg(test)]
mod tests;

/// A whole sealed frame backed by the existing message pool. The finalizer is
/// concrete and caller supplied; it receives the original lease only after the
/// immutable buffer view has been destroyed. No copy, reference increment or
/// allocator is introduced by this owner. It can back NoAllocBufferSegments.
pub struct PooledBytes<'id, 'storage, F: FnOnce(crate::engine::messages::Lease<'id>)> {
    reading: Option<crate::engine::messages::Reading<'id, 'storage>>,
    release: Option<F>,
}
impl<'id, 'storage, F: FnOnce(crate::engine::messages::Lease<'id>)> PooledBytes<'id, 'storage, F> {
    pub fn new(reading: crate::engine::messages::Reading<'id, 'storage>, release: F) -> Self {
        Self {
            reading: Some(reading),
            release: Some(release),
        }
    }
}
impl<'id, F: FnOnce(crate::engine::messages::Lease<'id>)> core::ops::Deref
    for PooledBytes<'id, '_, F>
{
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        self.reading.as_ref().expect("live pooled frame").bytes()
    }
}
impl<'id, F: FnOnce(crate::engine::messages::Lease<'id>)> AsRef<[u8]> for PooledBytes<'id, '_, F> {
    fn as_ref(&self) -> &[u8] {
        self
    }
}
impl<'id, F: FnOnce(crate::engine::messages::Lease<'id>)> Drop for PooledBytes<'id, '_, F> {
    fn drop(&mut self) {
        let lease = self.reading.take().expect("live pooled frame").into_lease();
        (self.release.take().expect("live frame finalizer"))(lease);
    }
}

/// The capability bindings retained alongside an incoming frame. Unit leaves
/// wire capability indices unbound; a driver must admit/resolve them in their
/// originating message's descriptor context. It grants no callable authority.
/// Hosted bindings retain their existing owned hooks and imbue that table.
pub trait Capabilities {
    fn bind<'a>(&'a self, content: any_pointer::Reader<'a>) -> any_pointer::Reader<'a>;
}
impl Capabilities for () {
    #[inline(always)]
    fn bind<'a>(&'a self, content: any_pointer::Reader<'a>) -> any_pointer::Reader<'a> {
        content
    }
}
#[cfg(feature = "alloc")]
impl Capabilities
    for alloc::vec::Vec<Option<alloc::boxed::Box<dyn capnp::private::capability::ClientHook>>>
{
    #[inline(always)]
    fn bind<'a>(&'a self, mut content: any_pointer::Reader<'a>) -> any_pointer::Reader<'a> {
        use capnp::traits::Imbue;
        content.imbue(self);
        content
    }
}
impl<C: Capabilities + ?Sized> Capabilities for &C {
    #[inline(always)]
    fn bind<'a>(&'a self, content: any_pointer::Reader<'a>) -> any_pointer::Reader<'a> {
        (**self).bind(content)
    }
}

#[inline(always)]
pub fn params<'a, M: IncomingMessage + ?Sized, C: Capabilities + ?Sized>(
    message: &'a M,
    caps: &'a C,
) -> capnp::Result<any_pointer::Reader<'a>> {
    Ok(caps.bind(message.get_params()?))
}
#[inline(always)]
pub fn results<'a, M: IncomingMessage + ?Sized, C: Capabilities + ?Sized>(
    message: &'a M,
    caps: &'a C,
) -> capnp::Result<any_pointer::Reader<'a>> {
    Ok(caps.bind(message.get_results()?))
}

/// A parameter owner retains its original whole frame, capability bindings and
/// read budget. They move together across suspension; no root is reopened with
/// a fresh traversal allowance. Frame storage drops before capability bindings.
pub struct Params<M, C = ()> {
    message: M,
    caps: C,
}
impl<M, C> Params<M, C> {
    pub fn new(message: M, caps: C) -> Self {
        Self { message, caps }
    }
    pub fn into_owners(self) -> (M, C) {
        (self.message, self.caps)
    }
}
impl<M: IncomingMessage, C: Capabilities> ParamsHook for Params<M, C> {
    #[inline(always)]
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        params(&self.message, &self.caps)
    }
}
/// Retained successful Return payload, with the same whole-frame ownership.
/// This adapter does not acknowledge Return or authorize question/ID cleanup.
pub struct Response<M, C = ()> {
    message: M,
    caps: C,
}
impl<M, C> Response<M, C> {
    pub fn new(message: M, caps: C) -> Self {
        Self { message, caps }
    }
    pub fn into_owners(self) -> (M, C) {
        (self.message, self.caps)
    }
}
impl<M: IncomingMessage, C: Capabilities> ResponseHook for Response<M, C> {
    #[inline(always)]
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        results(&self.message, &self.caps)
    }
}

impl<S: capnp::message::ReaderSegments, B: core::borrow::Borrow<capnp::message::ReadBudget>>
    IncomingMessage for capnp::message::Reader<S, B>
{
    fn size_in_words(&self) -> usize {
        let segments = capnp::message::Reader::get_segments(self);
        (0..segments.len())
            .map(|id| {
                segments
                    .get_segment(id as u32)
                    .map_or(0, |bytes| bytes.len() / 8)
            })
            .sum()
    }
    fn get_body(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        self.get_root()
    }
    fn get_segments(&self) -> Option<&dyn capnp::message::ReaderSegments> {
        Some(capnp::message::Reader::get_segments(self))
    }
}

// Forward checked/cached payload access too: borrowing a transport frame must
// not silently replace its cache with the default envelope decoder.
impl<M: IncomingMessage + ?Sized> IncomingMessage for &M {
    #[inline(always)]
    fn get_body(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        (**self).get_body()
    }
    #[inline(always)]
    fn get_params(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        (**self).get_params()
    }
    #[inline(always)]
    fn get_results(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        (**self).get_results()
    }
    #[inline(always)]
    fn cache_params(&self, payload: crate::rpc_capnp::payload::Reader<'_>) -> capnp::Result<()> {
        (**self).cache_params(payload)
    }
    #[inline(always)]
    fn cache_results(&self, payload: crate::rpc_capnp::payload::Reader<'_>) -> capnp::Result<()> {
        (**self).cache_results(payload)
    }
    #[inline(always)]
    fn size_in_words(&self) -> usize {
        (**self).size_in_words()
    }
    #[inline(always)]
    fn get_segments(&self) -> Option<&dyn capnp::message::ReaderSegments> {
        (**self).get_segments()
    }
    #[inline(always)]
    fn framed_bytes(&self) -> Option<&[u8]> {
        (**self).framed_bytes()
    }
}
#[cfg(feature = "alloc")]
impl<M: IncomingMessage + ?Sized> IncomingMessage for alloc::boxed::Box<M> {
    #[inline(always)]
    fn get_body(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        (**self).get_body()
    }
    #[inline(always)]
    fn get_params(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        (**self).get_params()
    }
    #[inline(always)]
    fn get_results(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        (**self).get_results()
    }
    #[inline(always)]
    fn cache_params(&self, payload: crate::rpc_capnp::payload::Reader<'_>) -> capnp::Result<()> {
        (**self).cache_params(payload)
    }
    #[inline(always)]
    fn cache_results(&self, payload: crate::rpc_capnp::payload::Reader<'_>) -> capnp::Result<()> {
        (**self).cache_results(payload)
    }
    #[inline(always)]
    fn size_in_words(&self) -> usize {
        (**self).size_in_words()
    }
    #[inline(always)]
    fn get_segments(&self) -> Option<&dyn capnp::message::ReaderSegments> {
        (**self).get_segments()
    }
    #[inline(always)]
    fn framed_bytes(&self) -> Option<&[u8]> {
        (**self).framed_bytes()
    }
    #[cfg(all(feature = "std", unix))]
    fn take_fds(&mut self) -> alloc::vec::Vec<std::os::fd::OwnedFd> {
        (**self).take_fds()
    }
    fn into_params(
        self: alloc::boxed::Box<Self>,
        caps: alloc::vec::Vec<
            Option<alloc::boxed::Box<dyn capnp::private::capability::ClientHook>>,
        >,
    ) -> alloc::boxed::Box<dyn ParamsHook>
    where
        Self: 'static,
    {
        (*self).into_params(caps)
    }
}
