//! Borrowed dispatch with explicit ownership transfer for suspended methods.
use super::{Params, Promise, Reply, Results};
use crate::{
    private::capability::{CallContextHook, OwnedCall},
    traits::Owned,
    Error,
};
use core::marker::PhantomData;

/// One admitted method invocation. Synchronous handlers borrow the original
/// parameter and result storage. A handler that suspends must take ownership
/// before constructing its `'static` completion future.
///
/// There is one dispatch interface for both forms. Borrowing does not bypass
/// cancellation policy, streaming barriers, membranes, or early pipelines.
pub struct CallContext<'call, P, R> {
    hook: &'call mut dyn CallContextHook,
    marker: PhantomData<(P, R)>,
}
impl<'call, P, R> CallContext<'call, P, R> {
    #[doc(hidden)]
    pub fn new(hook: &'call mut dyn CallContextHook) -> Self {
        Self {
            hook,
            marker: PhantomData,
        }
    }
    #[doc(hidden)]
    pub fn into_typed<A, B>(self) -> CallContext<'call, A, B> {
        CallContext::new(self.hook)
    }
    #[doc(hidden)]
    pub fn into_reply_context(self) -> ReplyContext<'call, P, R> {
        ReplyContext(self)
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
        if results.permits_immediate_poll() && results.cancellation_executor().is_some() {
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
    /// Transfer the original hooks into an asynchronous execution owner.
    pub fn into_owned(self) -> (Params<P>, Results<R>)
    where
        R: Owned,
    {
        let (params, results) = self.hook.take();
        (Params::new(params), Results::new(results))
    }
    /// Transfer the original result sink into the affine reply API.
    pub fn into_reply(self) -> (Params<P>, Reply<R>) {
        let (params, results) = self.hook.take();
        (Params::new(params), Reply::new(results))
    }
}

/// Borrowed entry to the structured reply API. Completing consumes the context;
/// suspended handlers receive an affine `Reply`, never unrestricted `Results`.
pub struct ReplyContext<'call, P, R>(CallContext<'call, P, R>);
impl<P, R> ReplyContext<'_, P, R> {
    pub fn into_owned(self) -> (Params<P>, Reply<R>) {
        self.0.into_reply()
    }
}
impl<P: Owned, R: Owned> ReplyContext<'_, P, R> {
    pub fn complete(
        self,
        f: impl FnOnce(P::Reader<'_>, R::Builder<'_>) -> crate::Result<()>,
    ) -> Promise<(), Error> {
        self.0.complete(f)
    }
}
impl<P: Owned, R: Owned> CallContext<'_, P, R> {
    /// Borrow both sides without allocating owned parameter/results adapters.
    pub fn get(&mut self) -> crate::Result<(P::Reader<'_>, R::Builder<'_>)> {
        let (params, results) = self.hook.borrow();
        Ok((params.get()?.get_as()?, results.get()?.get_as()?))
    }
    /// Complete synchronously. Borrowed readers/builders cannot escape the
    /// invocation, and construction errors become the method's completion.
    pub fn complete(
        mut self,
        f: impl FnOnce(P::Reader<'_>, R::Builder<'_>) -> crate::Result<()>,
    ) -> Promise<(), Error> {
        Promise::from(self.get().and_then(|(params, results)| f(params, results)))
    }
}
