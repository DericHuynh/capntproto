//! Transfer legacy invocation owners into existing generated wire dispatch.
use super::{CallContext, PreparedCall};
use crate::{
    any_pointer,
    field_api::{FixedField, Generic},
    private::capability::ParamsHook,
    wire_capability::{ContextService, Service},
    Error, ErrorKind, Result,
};

type Invocation<'call, PH> = CallContext<
    'call,
    any_pointer::Owned,
    any_pointer::Owned,
    PreparedCall<PH, FixedField<'call, Generic<any_pointer::Owned>>>,
>;

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
pub async fn dispatch_call<'call, PH: ParamsHook + 'call, S: Service + 'call>(
    invocation: Invocation<'call, PH>,
    mut service: S,
    interface_id: u64,
    method_id: u16,
) -> Result<()> {
    if service.method_info(interface_id, method_id).is_none() {
        return Err(Error::from_kind(ErrorKind::Unimplemented));
    }
    let (params, mut results) = invocation.into_owner().into_owners();
    service
        .dispatch(interface_id, method_id, params.get()?, results.reborrow())
        .await
}

/// The same ownership transfer with an independently retained application
/// context, for nested calls/capability lookup. Context and service/task storage
/// must not borrow a connection record that contains that same future.
pub async fn dispatch_call_with_context<
    'call,
    PH: ParamsHook + 'call,
    S: ContextService<C> + 'call,
    C,
>(
    invocation: Invocation<'call, PH>,
    mut service: S,
    interface_id: u64,
    method_id: u16,
    context: &'call mut C,
) -> Result<()> {
    if service.method_info(interface_id, method_id).is_none() {
        return Err(Error::from_kind(ErrorKind::Unimplemented));
    }
    let (params, mut results) = invocation.into_owner().into_owners();
    service
        .dispatch_with_context(
            interface_id,
            method_id,
            params.get()?,
            results.reborrow(),
            context,
        )
        .await
}
