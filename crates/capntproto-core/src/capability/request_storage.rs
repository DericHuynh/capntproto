//! Request storage and publication have the same concrete ownership lifetime.
use super::PayloadStorage;
use crate::{MessageSize, Result};

/// Publication consumes the original owner, rather than borrowing a temporary
/// parameters view. The provider chooses its completion/pipeline representation.
/// A bounded provider must return the unsent owner when publication is rejected.
pub trait RequestStorage<R>: Sized {
    type Sent;
    fn publish(self) -> Self::Sent;
}

/// Concrete parameters storage and its one-shot publication callback. No box,
/// dynamic future or allocation fallback is introduced by this adapter.
///
/// Dropping an unsent owner releases storage before dropping the callback. Thus
/// a captured cancellation sender cannot wake a consumer while a storage borrow
/// guard is still held by this request.
pub struct PreparedRequest<H, F> {
    storage: H,
    publish: F,
}
impl<H, F> PreparedRequest<H, F> {
    pub fn new<S>(storage: H, publish: F) -> Self
    where
        F: FnOnce(H) -> S,
    {
        Self { storage, publish }
    }
    pub fn storage(&self) -> &H {
        &self.storage
    }
}
impl<P, H: PayloadStorage<P>, F> PayloadStorage<P> for PreparedRequest<H, F> {
    type Builder<'a>
        = H::Builder<'a>
    where
        Self: 'a;
    fn try_get(&mut self, hint: Option<MessageSize>) -> Result<Self::Builder<'_>> {
        self.storage.try_get(hint)
    }
    fn try_init(&mut self, hint: Option<MessageSize>) -> Result<Self::Builder<'_>> {
        self.storage.try_init(hint)
    }
}
impl<R, H, F: FnOnce(H) -> S, S> RequestStorage<R> for PreparedRequest<H, F> {
    type Sent = S;
    fn publish(self) -> Self::Sent {
        (self.publish)(self.storage)
    }
}

#[cfg(feature = "alloc")]
impl<P: crate::traits::Owned> PayloadStorage<P>
    for alloc::boxed::Box<dyn crate::private::capability::RequestHook>
{
    type Builder<'a>
        = P::Builder<'a>
    where
        Self: 'a;
    #[inline(always)]
    fn try_get(&mut self, _: Option<MessageSize>) -> Result<Self::Builder<'_>> {
        self.get().get_as()
    }
    #[inline(always)]
    fn try_init(&mut self, _: Option<MessageSize>) -> Result<Self::Builder<'_>> {
        Ok(self.get().init_as())
    }
}
#[cfg(feature = "alloc")]
impl<R> RequestStorage<R> for alloc::boxed::Box<dyn crate::private::capability::RequestHook>
where
    R: crate::traits::Pipelined + crate::traits::Owned + 'static + Unpin,
    R::Pipeline: super::FromTypelessPipeline,
{
    type Sent = super::RemotePromise<R>;
    #[inline(always)]
    fn publish(self) -> Self::Sent {
        let super::RemotePromise {
            promise, pipeline, ..
        } = self.send();
        super::RemotePromise {
            promise: promise.into_typed_response(),
            pipeline: super::FromTypelessPipeline::new(pipeline),
        }
    }
}
