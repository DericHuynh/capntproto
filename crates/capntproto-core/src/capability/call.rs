//! Borrowed dispatch with explicit ownership transfer for suspended methods.
#[cfg(feature = "alloc")]
use super::{Params, Promise, Reply};
#[cfg(feature = "alloc")]
use crate::{
    private::capability::{CallContextHook, OwnedCall},
    traits::Owned,
    Error,
};
use core::marker::PhantomData;
mod storage;
pub use storage::{CallStorage, PreparedCall};
mod dispatch;
pub use dispatch::{dispatch_call, dispatch_call_with_context};

/// One admitted method invocation. Synchronous handlers borrow the original
/// parameter and result storage. A handler that suspends must take ownership
/// before constructing its completion future. Concrete owners may borrow caller
/// storage; the default hosted adapter keeps its owned static hooks.
///
/// There is one dispatch interface for both forms. Borrowing does not bypass
/// cancellation policy, streaming barriers, membranes, or early pipelines.
#[cfg(feature = "alloc")]
pub struct CallContext<'call, P, R, H = &'call mut dyn CallContextHook> {
    hook: H,
    immediate_protection: bool,
    marker: PhantomData<(&'call mut (), P, R)>,
}
#[cfg(not(feature = "alloc"))]
pub struct CallContext<'call, P, R, H> {
    hook: H,
    immediate_protection: bool,
    marker: PhantomData<(&'call mut (), P, R)>,
}
impl<'call, P, R, H> CallContext<'call, P, R, H> {
    /// Retain the original invocation owner after the driver admits the call.
    /// This constructor does not authorize cancellation or publish a Return.
    pub fn from_owner(hook: H) -> Self {
        Self {
            hook,
            immediate_protection: true,
            marker: PhantomData,
        }
    }
    /// Transfer the original invocation storage without changing its admission,
    /// cancellation or Return-publication responsibilities.
    pub fn into_owner(self) -> H {
        self.hook
    }
    #[doc(hidden)]
    pub fn into_typed<A, B>(self) -> CallContext<'call, A, B, H> {
        CallContext {
            hook: self.hook,
            immediate_protection: self.immediate_protection,
            marker: PhantomData,
        }
    }
}
impl<P, R, H: CallStorage<P, R>> CallContext<'_, P, R, H> {
    /// Borrow typed views from the retained invocation without boxed adapters.
    pub fn get(&mut self) -> crate::Result<(H::Params<'_>, H::Results<'_>)> {
        self.hook.get()
    }
    /// Move the original parameter/result owners into a suspended handler.
    pub fn into_owned(self) -> H::Taken {
        self.hook.take()
    }
    /// Complete synchronously using the provider's completion representation.
    /// The closure borrows the context and can call get() for both views.
    /// Editors cannot escape this call; construction errors reach the driver.
    pub fn try_complete(mut self, f: impl FnOnce(&mut Self) -> crate::Result<()>) -> H::Completion {
        let result = f(&mut self);
        self.hook.complete(result)
    }
}

#[cfg(feature = "alloc")]
impl<'call, P, R> CallContext<'call, P, R> {
    #[doc(hidden)]
    pub fn new(hook: &'call mut dyn CallContextHook) -> Self {
        Self {
            hook,
            immediate_protection: true,
            marker: PhantomData,
        }
    }
    #[doc(hidden)]
    pub fn into_reply_context(self) -> ReplyContext<'call, P, R> {
        ReplyContext(self)
    }
    /// An explicitly selected executor must admit protected execution before
    /// entering application code, even when the inbound driver allows a poll.
    #[doc(hidden)]
    pub fn with_deferred_protection(mut self) -> Self {
        self.immediate_protection = false;
        self
    }
    /// Enter a protected non-streaming method under the existing immediate-poll
    /// permission. The runtime retains the cancellation guard and executor;
    /// suspension still transfers to its ordinary protected execution path.
    /// Opaque wrappers and ordinary local calls keep deferred application entry.
    #[doc(hidden)]
    pub fn dispatch_protected(
        self,
        f: impl FnOnce(CallContext<'_, P, R>) -> Promise<(), Error> + 'static,
    ) -> Promise<(), Error>
    where
        P: 'static,
        R: 'static,
    {
        let results = self.hook.results();
        if self.immediate_protection
            && results.permits_immediate_poll()
            && results.cancellation_executor().is_some()
        {
            f(self)
        } else {
            self.defer(f)
        }
    }
    /// Delay application entry until the runtime has admitted protected execution.
    /// Generated non-cancellable methods use this before invoking user code.
    #[doc(hidden)]
    pub fn defer(
        self,
        f: impl FnOnce(CallContext<'_, P, R>) -> Promise<(), Error> + 'static,
    ) -> Promise<(), Error>
    where
        P: 'static,
        R: 'static,
    {
        let (params, results) = self.hook.take();
        Promise::from_future(async move {
            let mut context = OwnedCall::new(params, results);
            f(CallContext::new(&mut context)).await
        })
    }
    /// Transfer the original result sink into the affine reply API.
    pub fn into_reply(self) -> (Params<P>, Reply<R>) {
        let (params, results) = self.hook.take();
        (Params::new(params), Reply::new(results))
    }
}

/// Borrowed entry to the structured reply API. Completing consumes the context;
/// suspended handlers receive an affine `Reply`, never unrestricted `Results`.
#[cfg(feature = "alloc")]
pub struct ReplyContext<'call, P, R>(CallContext<'call, P, R>);
#[cfg(feature = "alloc")]
impl<P, R> ReplyContext<'_, P, R> {
    pub fn into_owned(self) -> (Params<P>, Reply<R>) {
        self.0.into_reply()
    }
}
#[cfg(feature = "alloc")]
impl<P: Owned, R: Owned> ReplyContext<'_, P, R> {
    pub fn complete(
        self,
        f: impl FnOnce(P::Reader<'_>, R::Builder<'_>) -> crate::Result<()>,
    ) -> Promise<(), Error> {
        self.0.complete(f)
    }
}

#[cfg(feature = "alloc")]
impl<P: Owned, R: Owned> CallContext<'_, P, R> {
    /// Complete synchronously. Borrowed readers/builders cannot escape the
    /// invocation, and construction errors become the method's completion.
    pub fn complete(
        mut self,
        f: impl FnOnce(P::Reader<'_>, R::Builder<'_>) -> crate::Result<()>,
    ) -> Promise<(), Error> {
        Promise::from(self.get().and_then(|(params, results)| f(params, results)))
    }
}
