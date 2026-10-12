//! Hosted capability hooks for generated field-operation calls.
use super::*;

/// Unsent RPC request with the generated field-operation parameter editor.
#[must_use = "an unsent request does nothing; send or forward it"]
pub struct Request<P: Schema, R: Schema + crate::traits::Pipelined> {
    hook: alloc::boxed::Box<dyn crate::private::capability::RequestHook>,
    marker: PhantomData<(P, R)>,
}
impl<P: Schema, R: Schema + crate::traits::Pipelined> Request<P, R> {
    /// Fill parameters before sending; failure drops the unsent request.
    pub fn with_params(
        mut self,
        fill: impl for<'a> FnOnce(P::Mut<'a>) -> Result<()>,
    ) -> Result<Self> {
        fill(self.edit())?;
        Ok(self)
    }
    #[doc(hidden)]
    pub fn new(hook: alloc::boxed::Box<dyn crate::private::capability::RequestHook>) -> Self {
        Self {
            hook,
            marker: PhantomData,
        }
    }
    pub fn edit(&mut self) -> P::Mut<'_> {
        self.hook
            .get()
            .builder
            .get_struct(P::SIZE, None)
            .expect("typed request root")
            .into()
    }
    pub fn copy_from(&mut self, params: P::Ref<'_>) -> Result<()> {
        use crate::traits::IntoInternalStructReader;
        self.hook
            .get()
            .builder
            .stage(|mut pointer| pointer.set_struct(&params.into_internal_struct_reader(), false))
    }
    /// Send and await completion, discarding response data while preserving errors
    /// and the method's cancellation policy. The result pipeline is dropped at send.
    pub fn send_ignoring_result(self) -> crate::capability::Promise<(), Error> {
        crate::capability::Request::<crate::any_pointer::Owned, crate::any_pointer::Owned>::new(
            self.hook,
        )
        .send_ignoring_result()
    }
}
impl<P: Schema, R: Schema + crate::traits::Pipelined + 'static> Request<P, R>
where
    R::Pipeline: crate::capability::FromTypelessPipeline,
{
    pub fn send(self) -> PendingCall<R> {
        let sent = self.hook.send();
        PendingCall {
            response: crate::capability::Promise::from_future(async move {
                Ok(Response {
                    hook: sent.promise.await?.hook,
                    marker: PhantomData,
                })
            }),
            pipeline: crate::capability::FromTypelessPipeline::new(sent.pipeline),
        }
    }
    pub fn send_for_pipeline(self) -> R::Pipeline {
        crate::capability::FromTypelessPipeline::new(self.hook.send_for_pipeline())
    }
}
/// Owns the response hook and lends results without allocating a native graph.
pub struct Response<R: Schema> {
    hook: alloc::boxed::Box<dyn crate::private::capability::ResponseHook>,
    marker: PhantomData<R>,
}
impl<R: Schema> Response<R> {
    pub fn read(&self) -> Result<R::Ref<'_>> {
        Ok(self.hook.get()?.reader.get_struct(None)?.into())
    }
}
/// Awaitable response plus a pipeline available before awaiting. Derived
/// capability clients retain their own hooks. This does not make local RPC Send.
#[must_use]
pub struct PendingCall<R: Schema + crate::traits::Pipelined> {
    response: crate::capability::Promise<Response<R>, Error>,
    pub pipeline: R::Pipeline,
}
// No field is exposed through a pinned projection; the only polled field is
// Promise, which is Unpin independently of its output type.
impl<R: Schema + crate::traits::Pipelined> Unpin for PendingCall<R> {}
impl<R: Schema + crate::traits::Pipelined> core::future::Future for PendingCall<R> {
    type Output = Result<Response<R>>;
    fn poll(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        core::pin::Pin::new(&mut self.get_mut().response).poll(cx)
    }
}
impl<R: Schema + crate::traits::Pipelined> PendingCall<R> {
    pub fn into_parts(self) -> (crate::capability::Promise<Response<R>, Error>, R::Pipeline) {
        (self.response, self.pipeline)
    }
}
#[must_use = "an unsent streaming request does nothing; send it"]
pub struct StreamingRequest<P: Schema> {
    hook: alloc::boxed::Box<dyn crate::private::capability::RequestHook>,
    marker: PhantomData<P>,
}
impl<P: Schema> StreamingRequest<P> {
    /// Fill parameters before sending; failure drops the unsent request.
    pub fn with_params(
        mut self,
        fill: impl for<'a> FnOnce(P::Mut<'a>) -> Result<()>,
    ) -> Result<Self> {
        fill(self.edit())?;
        Ok(self)
    }
    #[doc(hidden)]
    pub fn new(hook: alloc::boxed::Box<dyn crate::private::capability::RequestHook>) -> Self {
        Self {
            hook,
            marker: PhantomData,
        }
    }
    pub fn edit(&mut self) -> P::Mut<'_> {
        self.hook
            .get()
            .builder
            .get_struct(P::SIZE, None)
            .expect("typed request root")
            .into()
    }
    pub fn send(self) -> crate::capability::Promise<(), Error> {
        self.hook.send_streaming()
    }
}
