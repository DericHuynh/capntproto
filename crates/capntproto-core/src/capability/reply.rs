//! Affine server reply contexts used by the structured-replies generator mode.
use super::{Promise, Request};
use crate::{private::capability::ResultsHook, traits::Owned, Error, MessageSize};
use alloc::boxed::Box;
use core::marker::PhantomData;

/// A fresh server reply: choose writing or tail forwarding once.
///
/// Generated with `CodeGenerationCommand::structured_replies(true)`. There is
/// no public constructor or raw-hook accessor. `build()` consumes this context
/// and returns an editor without a tail-call operation. Network, decoding and
/// application failures remain ordinary `Result` errors.
#[must_use = "complete, build, or forward the reply"]
pub struct Reply<T> {
    hook: Box<dyn ResultsHook>,
    marker: PhantomData<T>,
}

impl<T> Reply<T> {
    pub(crate) fn new(hook: Box<dyn ResultsHook>) -> Self {
        Self {
            hook,
            marker: PhantomData,
        }
    }
}

impl<T: Owned> Reply<T> {
    /// Consume the fresh context before editing. No result allocation occurs
    /// until the first edit, initialization or copy.
    pub fn build(self) -> ReplyBuilder<T> {
        ReplyBuilder {
            hook: self.hook,
            marker: PhantomData,
        }
    }

    /// Fill a reply in one synchronous step. Editors cannot escape the closure.
    /// Return this result from the server method to propagate construction errors.
    pub fn complete(
        self,
        fill: impl for<'a> FnOnce(T::Builder<'a>) -> crate::Result<()>,
    ) -> crate::Result<()> {
        let mut builder = self.build();
        fill(builder.edit()?)?;
        builder.finish()
    }

    /// Forward without initializing the reply. The request must return exactly
    /// T; streaming requests and different result schemas do not type-check.
    /// Retains the backend's optimized tail transfer and three-party adoption.
    pub fn tail_call<P>(self, request: Request<P, T>) -> Promise<(), Error> {
        self.hook.tail_call(request.hook)
    }
}

/// An exclusively owned result editor. It cannot tail-call. Borrowed builders
/// and orphanage tokens must end before publishing or finishing this owner.
#[must_use = "finish or publish the reply builder"]
pub struct ReplyBuilder<T> {
    hook: Box<dyn ResultsHook>,
    marker: PhantomData<T>,
}

impl<T: Owned> ReplyBuilder<T> {
    pub fn edit(&mut self) -> crate::Result<T::Builder<'_>> {
        self.edit_with_size_hint(None)
    }

    /// The hint affects only the first allocation; it is not a size limit.
    pub fn edit_with_size_hint(
        &mut self,
        hint: Option<MessageSize>,
    ) -> crate::Result<T::Builder<'_>> {
        self.hook.get_with_size_hint(hint)?.get_as()
    }

    pub fn init(&mut self) -> crate::Result<T::Builder<'_>> {
        Ok(self.hook.get()?.init_as())
    }

    pub fn copy_from(&mut self, value: T::Reader<'_>) -> crate::Result<()> {
        self.hook.get()?.set_as(value)
    }

    pub fn get_orphanage(
        &mut self,
        hint: Option<MessageSize>,
    ) -> crate::Result<(
        crate::dynamic_orphan::Root<'_>,
        crate::dynamic_orphan::Orphanage<'_>,
    )> {
        Ok(
            crate::dynamic_orphan::Root::new(self.hook.get_with_size_hint(hint)?, T::introspect())?
                .with_orphanage(),
        )
    }

    /// Publish a snapshot of these result capabilities and freeze the entire
    /// payload. No editor or root replacement is available afterward, so final
    /// capability identities cannot diverge from the published snapshot.
    ///
    /// Keep the returned owner across asynchronous work, then `finish()` it.
    /// Publication does not complete the method or undo children on later error.
    pub fn publish(mut self) -> crate::Result<PublishedReply<T>> {
        self.hook.set_pipeline()?;
        Ok(PublishedReply {
            _hook: self.hook,
            marker: PhantomData,
        })
    }

    /// Release result ownership. The method's returned future still determines
    /// success or failure; return this value as its final successful expression.
    pub fn finish(self) -> crate::Result<()> {
        Ok(())
    }
}

/// Published, immutable result context. It can be retained until method
/// completion, but cannot be edited, republished or converted back to a draft.
#[must_use = "retain the published reply while the method is pending, then finish it"]
pub struct PublishedReply<T> {
    _hook: Box<dyn ResultsHook>,
    marker: PhantomData<T>,
}
impl<T> PublishedReply<T> {
    pub fn finish(self) -> crate::Result<()> {
        Ok(())
    }
}
