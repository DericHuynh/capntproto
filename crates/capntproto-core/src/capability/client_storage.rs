//! Concrete call admission through the original capability owner.
use super::CallHints;
use crate::{MessageSize, Result};

/// A capability's provider selects request storage and its borrow lifetime.
/// The request may retain a generation-checked lease, a fixed message guard,
/// or hosted owned hooks. It need not be boxed, Clone or static. Implementors
/// validate capability authority and reserve storage before exposing parameters;
/// capacity rejection must not consume protocol IDs or poison the capability.
///
/// Internal connection records use keys, not public clients borrowing their
/// containing context. Parameter access/publication are separate PayloadStorage
/// and RequestStorage contracts on the resulting owner.
///
/// ```compile_fail
/// use capnp::capability::Client;
/// fn escape<H>(owner: &H) -> Client<&'static H> {
///     Client::from_owner(owner)
/// }
/// ```
pub trait ClientStorage<P, R> {
    type Request<'a>
    where
        Self: 'a;
    fn try_new_call(
        &self,
        interface_id: u64,
        method_id: u16,
        size_hint: Option<MessageSize>,
        hints: CallHints,
    ) -> Result<Self::Request<'_>>;
}

impl<P, R, H: ClientStorage<P, R> + ?Sized> ClientStorage<P, R> for &H {
    type Request<'a>
        = H::Request<'a>
    where
        Self: 'a;
    fn try_new_call(
        &self,
        interface_id: u64,
        method_id: u16,
        size_hint: Option<MessageSize>,
        hints: CallHints,
    ) -> Result<Self::Request<'_>> {
        (**self).try_new_call(interface_id, method_id, size_hint, hints)
    }
}

#[cfg(feature = "alloc")]
impl<P, R> ClientStorage<P, R> for alloc::boxed::Box<dyn crate::private::capability::ClientHook> {
    type Request<'a> = alloc::boxed::Box<dyn crate::private::capability::RequestHook>;
    fn try_new_call(
        &self,
        interface_id: u64,
        method_id: u16,
        size_hint: Option<MessageSize>,
        hints: CallHints,
    ) -> Result<Self::Request<'_>> {
        Ok(self
            .new_call_with_hints(interface_id, method_id, size_hint, hints)
            .hook)
    }
}
