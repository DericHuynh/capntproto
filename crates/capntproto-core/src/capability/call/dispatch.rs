//! Transfer legacy invocation owners into existing generated wire dispatch.
use super::{CallContext, PreparedCall};
use crate::{
    any_pointer,
    field_api::{FixedField, Generic},
    private::capability::ParamsHook,
    wire_capability::{ContextService, Service},
    Error, ErrorKind, Result,
};

/// The result owner can retain a pooled buffer guard, rather than requiring an
/// already borrowed field outside the dispatch future. Its fallible view cannot
/// escape the owner. Preparing this view neither sends Return nor changes call
/// cancellation/protection policy.
pub trait DispatchResults {
    fn content(&mut self) -> Result<FixedField<'_, Generic<any_pointer::Owned>>>;
}
impl DispatchResults for FixedField<'_, Generic<any_pointer::Owned>> {
    #[inline(always)]
    fn content(&mut self) -> Result<FixedField<'_, Generic<any_pointer::Owned>>> {
        Ok(self.reborrow())
    }
}
impl<R: DispatchResults + ?Sized> DispatchResults for &mut R {
    fn content(&mut self) -> Result<FixedField<'_, Generic<any_pointer::Owned>>> {
        (**self).content()
    }
}
impl<R: DispatchResults + ?Sized> DispatchResults for core::cell::RefMut<'_, R> {
    fn content(&mut self) -> Result<FixedField<'_, Generic<any_pointer::Owned>>> {
        (**self).content()
    }
}

type Invocation<'call, PH, RH> =
    CallContext<'call, any_pointer::Owned, any_pointer::Owned, PreparedCall<PH, RH>>;

/// The returned future owns the original invocation and its generated service
/// adapter. S may borrow a dispatcher or own a small per-call dispatcher over
/// shared application state, allowing concurrent calls without a shared mutable
/// dispatcher borrow. It may be placed in caller-pinned task storage without a box or
/// static lifetime. Parameters remain alive across every method suspension.
///
/// The connection must admit/pin this future and apply the method's cancellation
/// and streaming policy before polling it. This adapter neither publishes Return
/// nor detaches protected calls; dropping it destroys its owned invocation.
/// Service metadata and application entry occur only when it is polled. Unknown
/// methods reject before parameters are accessed or result storage is edited.
pub async fn dispatch_call<
    'call,
    PH: ParamsHook + 'call,
    RH: DispatchResults + 'call,
    S: Service + 'call,
>(
    invocation: Invocation<'call, PH, RH>,
    mut service: S,
    interface_id: u64,
    method_id: u16,
) -> Result<()> {
    if service.method_info(interface_id, method_id).is_none() {
        return Err(Error::from_kind(ErrorKind::Unimplemented));
    }
    let mut storage = invocation.into_owner();
    let (params, results) = storage.borrow_owners();
    service
        .dispatch(interface_id, method_id, params.get()?, results.content()?)
        .await
}

/// The same ownership transfer with an independently retained application
/// context, for nested calls/capability lookup. Context and service/task storage
/// must not borrow a connection record that contains that same future.
pub async fn dispatch_call_with_context<
    'call,
    PH: ParamsHook + 'call,
    RH: DispatchResults + 'call,
    S: ContextService<C> + 'call,
    C,
>(
    invocation: Invocation<'call, PH, RH>,
    mut service: S,
    interface_id: u64,
    method_id: u16,
    context: &'call mut C,
) -> Result<()> {
    if service.method_info(interface_id, method_id).is_none() {
        return Err(Error::from_kind(ErrorKind::Unimplemented));
    }
    let mut storage = invocation.into_owner();
    let (params, results) = storage.borrow_owners();
    service
        .dispatch_with_context(
            interface_id,
            method_id,
            params.get()?,
            results.content()?,
            context,
        )
        .await
}
