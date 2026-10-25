// Copyright (c) 2013-2015 Sandstorm Development Group, Inc. and contributors
// Licensed under the MIT License:
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

//! Hooks for the RPC system.
//!
//! Roughly corresponds to capability.h in the C++ implementation.
//! Parameters and responses can retain concrete frame owners in every profile.
//! Allocating request, client and dispatch hooks are enabled by `alloc`.

#[cfg(feature = "alloc")]
use core::future::Future;
use core::marker::PhantomData;
#[cfg(feature = "alloc")]
use core::pin::Pin;
#[cfg(feature = "alloc")]
use core::task::Poll;

use crate::any_pointer;
#[cfg(feature = "alloc")]
use crate::private::capability::{ClientHook, RequestHook, ResultsHook};
pub use crate::private::capability::{ParamsHook, ResponseHook};
use crate::traits::Owned;
#[cfg(feature = "alloc")]
use crate::traits::Pipelined;
#[cfg(feature = "alloc")]
use crate::{Error, MessageSize};

#[cfg(test)]
mod owner_views;

#[cfg(feature = "alloc")]
mod reply;
#[cfg(feature = "alloc")]
pub use reply::{PublishedReply, Reply, ReplyBuilder};

#[cfg(feature = "alloc")]
mod call;
#[cfg(feature = "alloc")]
pub use call::{CallContext, ReplyContext};

/// Type alias for `dyn ClientHook`. We define this here because so that generated code
/// can avoid needing to refer to `dyn` types directly; in Rust 2015 the syntax for
/// `dyn` types requires extra parentheses that trigger warnings in newer editions.
#[cfg(feature = "alloc")]
pub type DynClientHook = dyn ClientHook;

/// A computation that might eventually resolve to a value of type `T` or to an error
///  of type `E`. Dropping the promise cancels the computation.
///
/// The nightly-only `rpc_try` feature allows `?` on a `Result` inside a function
/// returning `Promise<T, Error>`, converting the error with `Error::from`.
/// A promise must be awaited before inspecting its result: use `promise.await?`
/// in async code. `Promise` itself does not implement `Try`.
#[cfg(feature = "alloc")]
#[must_use = "futures do nothing unless polled"]
pub struct Promise<T, E> {
    inner: PromiseInner<T, E>,
}

#[cfg(feature = "alloc")]
enum PromiseInner<T, E> {
    // Keep potentially large error context off every deferred promise. Success
    // stays inline; only a synchronously rejected promise allocates its error.
    Immediate(Result<T, alloc::boxed::Box<E>>),
    Deferred(Pin<alloc::boxed::Box<dyn Future<Output = core::result::Result<T, E>> + 'static>>),
    Empty,
}

// Allow Promise<T,E> to be Unpin, regardless of whether T and E are.
#[cfg(feature = "alloc")]
impl<T, E> Unpin for PromiseInner<T, E> {}

#[cfg(feature = "alloc")]
impl<T, E> Promise<T, E> {
    pub fn ok(value: T) -> Self {
        Self {
            inner: PromiseInner::Immediate(Ok(value)),
        }
    }

    pub fn err(error: E) -> Self {
        Self {
            inner: PromiseInner::Immediate(Err(alloc::boxed::Box::new(error))),
        }
    }

    pub fn from_future<F>(f: F) -> Self
    where
        F: Future<Output = core::result::Result<T, E>> + 'static,
    {
        Self {
            inner: PromiseInner::Deferred(alloc::boxed::Box::pin(f)),
        }
    }
}

#[cfg(feature = "alloc")]
impl<T, E> Future for Promise<T, E> {
    type Output = core::result::Result<T, E>;

    #[inline(always)]
    fn poll(self: Pin<&mut Self>, cx: &mut ::core::task::Context) -> Poll<Self::Output> {
        match self.get_mut().inner {
            PromiseInner::Empty => panic!("Promise polled after done."),
            ref mut imm @ PromiseInner::Immediate(_) => {
                match core::mem::replace(imm, PromiseInner::Empty) {
                    PromiseInner::Immediate(r) => Poll::Ready(r.map_err(|error| *error)),
                    _ => unreachable!(),
                }
            }
            PromiseInner::Deferred(ref mut f) => f.as_mut().poll(cx),
        }
    }
}

#[cfg(feature = "alloc")]
impl<T, E> From<Result<T, E>> for Promise<T, E> {
    fn from(value: Result<T, E>) -> Self {
        Self {
            inner: PromiseInner::Immediate(value.map_err(alloc::boxed::Box::new)),
        }
    }
}

#[cfg(feature = "alloc")]
#[cfg(feature = "rpc_try")]
/// Propagate synchronous validation errors from a promise-returning function.
/// Successful results continue synchronously; an error becomes an immediately
/// rejected promise without polling or blocking on other work.
///
/// ```
/// use capnp::capability::Promise;
/// fn validate(bytes: &[u8]) -> Promise<&str, capnp::Error> {
///     let text = core::str::from_utf8(bytes)?;
///     Promise::ok(text)
/// }
/// # drop(validate(b"valid"));
/// ```
///
/// `?` cannot wait for a promise. Use `.await?` inside async code instead.
///
/// ```compile_fail,E0277
/// use capnp::capability::Promise;
/// fn cannot_skip_await(promise: Promise<(), capnp::Error>) -> Promise<(), capnp::Error> {
///     promise?
/// }
/// ```
impl<T, E> core::ops::FromResidual<Result<core::convert::Infallible, E>>
    for Promise<T, crate::Error>
where
    crate::Error: From<E>,
{
    fn from_residual(residual: Result<core::convert::Infallible, E>) -> Self {
        match residual {
            Ok(never) => match never {},
            Err(error) => Self::err(error.into()),
        }
    }
}

/// A promise for a result from a method call.
#[cfg(feature = "alloc")]
#[must_use]
pub struct RemotePromise<Results>
where
    Results: Pipelined + Owned + 'static,
{
    pub promise: ResponsePromise<Results>,
    pub pipeline: Results::Pipeline,
}

// Only Promise is polled, and it is Unpin independently of its output. There
// is no pinned projection into the pipeline.
#[cfg(feature = "alloc")]
impl<T: Pipelined + Owned + 'static> Unpin for RemotePromise<T> {}

#[cfg(feature = "alloc")]
impl<T: Pipelined + Owned + 'static> Future for RemotePromise<T> {
    type Output = crate::Result<Response<T>>;
    fn poll(self: Pin<&mut Self>, cx: &mut core::task::Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.get_mut().promise).poll(cx)
    }
}

#[cfg(feature = "alloc")]
impl<T: Pipelined + Owned + 'static> RemotePromise<T> {
    /// Split completion and pipeline ownership explicitly. A derived pipeline
    /// capability owns its own reference and can outlive either returned part.
    pub fn into_parts(self) -> (ResponsePromise<T>, T::Pipeline) {
        (self.promise, self.pipeline)
    }
}

/// A response retaining its concrete owner until all borrowed readers end.
/// Hosted callers default to an owned boxed hook; caller-backed profiles supply
/// their frame guard directly with `from_owner`.
#[cfg(feature = "alloc")]
pub struct Response<Results, H = alloc::boxed::Box<dyn ResponseHook>> {
    pub marker: PhantomData<Results>,
    pub hook: H,
}
#[cfg(not(feature = "alloc"))]
pub struct Response<Results, H> {
    pub marker: PhantomData<Results>,
    pub hook: H,
}
#[cfg(feature = "alloc")]
impl<Results: Pipelined + Owned> Response<Results> {
    pub fn new(hook: alloc::boxed::Box<dyn ResponseHook>) -> Self {
        Self {
            marker: PhantomData,
            hook,
        }
    }
}
impl<Results, H: ResponseHook> Response<Results, H> {
    /// Transfer the caller's owner without boxing or changing its lifetime.
    pub fn from_owner(hook: H) -> Self {
        Self {
            marker: PhantomData,
            hook,
        }
    }
    #[inline(always)]
    pub fn get(&self) -> crate::Result<Results::Reader<'_>>
    where
        Results: Owned,
    {
        self.hook.get()?.get_as()
    }
    /// Retain the same owner while selecting a generated schema. Reading still
    /// validates the wire type and normal traversal/nesting limits.
    pub fn into_typed<T>(self) -> Response<T, H> {
        Response {
            marker: PhantomData,
            hook: self.hook,
        }
    }
}

/// A response future whose generated result type is selected without allocating
/// a forwarding future. Dropping it cancels the original driver completion.
#[cfg(feature = "alloc")]
#[must_use = "a response future must be polled"]
pub struct ResponsePromise<R, E = Error> {
    inner: ResponseFuture<E>,
    marker: PhantomData<R>,
}
// Keep response-only ownership out of the ubiquitous Promise<(), E>. This flat
// representation also avoids a second discriminant around a general Promise.
#[cfg(feature = "alloc")]
enum ResponseFuture<E> {
    Immediate(Result<Response<any_pointer::Owned>, alloc::boxed::Box<E>>),
    Deferred(Pin<alloc::boxed::Box<dyn Future<Output = Result<Response<any_pointer::Owned>, E>>>>),
    Direct(DirectResponseOwner<E>),
    Empty,
}
#[cfg(feature = "alloc")]
struct DirectResponseOwner<E>(
    Option<
        alloc::rc::Rc<
            dyn crate::private::capability::SharedPromise<alloc::boxed::Box<dyn ResponseHook>, E>,
        >,
    >,
);
#[cfg(feature = "alloc")]
impl<E> Drop for DirectResponseOwner<E> {
    fn drop(&mut self) {
        if let Some(state) = self.0.take() {
            state.cancel();
        }
    }
}
#[cfg(feature = "alloc")]
impl<R, E> Unpin for ResponsePromise<R, E> {}
#[cfg(feature = "alloc")]
impl<R: Pipelined + Owned, E> ResponsePromise<R, E> {
    #[doc(hidden)]
    pub fn new(inner: Promise<Response<any_pointer::Owned>, E>) -> Self {
        let inner = match inner.inner {
            PromiseInner::Immediate(result) => ResponseFuture::Immediate(result),
            PromiseInner::Deferred(future) => ResponseFuture::Deferred(future),
            PromiseInner::Empty => ResponseFuture::Empty,
        };
        Self {
            inner,
            marker: PhantomData,
        }
    }
    /// Own the single response interest in an existing producer allocation.
    /// Completion or abandonment calls cancel once, as specified by SharedPromise.
    #[doc(hidden)]
    pub fn from_shared(
        state: alloc::rc::Rc<
            dyn crate::private::capability::SharedPromise<alloc::boxed::Box<dyn ResponseHook>, E>,
        >,
    ) -> Self {
        Self {
            inner: ResponseFuture::Direct(DirectResponseOwner(Some(state))),
            marker: PhantomData,
        }
    }
    fn into_typed_response<T: Pipelined + Owned>(self) -> ResponsePromise<T, E> {
        ResponsePromise {
            inner: self.inner,
            marker: PhantomData,
        }
    }
}
#[cfg(feature = "alloc")]
impl<R: Pipelined + Owned, E> Future for ResponsePromise<R, E> {
    type Output = Result<Response<R>, E>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut core::task::Context<'_>) -> Poll<Self::Output> {
        let result = match &mut self.inner {
            ResponseFuture::Empty => panic!("ResponsePromise polled after completion"),
            immediate @ ResponseFuture::Immediate(_) => {
                let ResponseFuture::Immediate(result) =
                    core::mem::replace(immediate, ResponseFuture::Empty)
                else {
                    unreachable!()
                };
                Poll::Ready(result.map(|response| response.hook).map_err(|error| *error))
            }
            ResponseFuture::Deferred(future) => future
                .as_mut()
                .poll(cx)
                .map(|result| result.map(|response| response.hook)),
            ResponseFuture::Direct(owner) => {
                owner.0.as_ref().expect("live response interest").poll(cx)
            }
        };
        let Poll::Ready(result) = result else {
            return Poll::Pending;
        };
        // Release completed request ownership even if this future is retained.
        let completed = core::mem::replace(&mut self.inner, ResponseFuture::Empty);
        drop(completed);
        Poll::Ready(result.map(Response::new))
    }
}

/// A method call that has not been sent yet.
#[cfg(feature = "alloc")]
#[must_use = "an unsent request does nothing; send or forward it"]
pub struct Request<Params, Results> {
    pub marker: PhantomData<(Params, Results)>,
    pub hook: alloc::boxed::Box<dyn RequestHook>,
}

#[cfg(feature = "alloc")]
impl<Params, Results> Request<Params, Results> {
    /// Send the call and wait for its completion, discarding the response data.
    ///
    /// Errors are propagated normally. This drops the result pipeline immediately
    /// and does not decode the response. Keep or await the returned promise while
    /// the call is needed: dropping it follows the method's normal cancellation
    /// policy. This is not a detached or streaming call.
    pub fn send_ignoring_result(self) -> Promise<(), Error> {
        let promise = self.hook.send().promise;
        Promise::from_future(async move { promise.await.map(|_| ()) })
    }
}

#[cfg(feature = "alloc")]
impl<Params, Results> Request<Params, Results>
where
    Params: Owned,
{
    pub fn new(hook: alloc::boxed::Box<dyn RequestHook>) -> Self {
        Self {
            hook,
            marker: PhantomData,
        }
    }

    #[inline(always)]
    pub fn get(&mut self) -> Params::Builder<'_> {
        self.hook.get().get_as().unwrap()
    }

    pub fn set(&mut self, from: Params::Reader<'_>) -> crate::Result<()> {
        self.hook.get().set_as(from)
    }

    /// Fill parameters before sending. On error this consumes and drops the
    /// request, so the partially filled request cannot accidentally be sent.
    pub fn with_params(
        mut self,
        fill: impl for<'a> FnOnce(Params::Builder<'a>) -> crate::Result<()>,
    ) -> crate::Result<Self> {
        fill(self.hook.get().get_as()?)?;
        Ok(self)
    }
}

#[cfg(feature = "alloc")]
impl<Params, Results> Request<Params, Results>
where
    Results: Pipelined + Owned + 'static + Unpin,
    <Results as Pipelined>::Pipeline: FromTypelessPipeline,
{
    /// Send a call whose response data will never be observed. Retain the
    /// pipeline or its derived capabilities for as long as the call is needed.
    pub fn send_for_pipeline(self) -> Results::Pipeline {
        FromTypelessPipeline::new(self.hook.send_for_pipeline())
    }

    pub fn send(self) -> RemotePromise<Results> {
        let RemotePromise {
            promise, pipeline, ..
        } = self.hook.send();
        let typed_promise = promise.into_typed_response();
        RemotePromise {
            promise: typed_promise,
            pipeline: FromTypelessPipeline::new(pipeline),
        }
    }
}

/// A method call that has not been sent yet.
#[cfg(feature = "alloc")]
#[must_use = "an unsent streaming request does nothing; send it"]
pub struct StreamingRequest<Params> {
    pub marker: PhantomData<Params>,
    pub hook: alloc::boxed::Box<dyn RequestHook>,
}

#[cfg(feature = "alloc")]
impl<Params> StreamingRequest<Params>
where
    Params: Owned,
{
    /// Fill parameters before sending; failure drops the unsent request.
    pub fn with_params(
        mut self,
        fill: impl for<'a> FnOnce(Params::Builder<'a>) -> crate::Result<()>,
    ) -> crate::Result<Self> {
        fill(self.hook.get().get_as()?)?;
        Ok(self)
    }

    #[inline(always)]
    pub fn get(&mut self) -> Params::Builder<'_> {
        self.hook.get().get_as().unwrap()
    }

    pub fn send(self) -> Promise<(), Error> {
        self.hook.send_streaming()
    }
}

/// The parameters of a method call, retaining the caller's concrete owner.
#[cfg(feature = "alloc")]
pub struct Params<T, H = alloc::boxed::Box<dyn ParamsHook>> {
    pub marker: PhantomData<T>,
    pub hook: H,
}
#[cfg(not(feature = "alloc"))]
pub struct Params<T, H> {
    pub marker: PhantomData<T>,
    pub hook: H,
}
#[cfg(feature = "alloc")]
impl<T> Params<T> {
    pub fn new(hook: alloc::boxed::Box<dyn ParamsHook>) -> Self {
        Self {
            marker: PhantomData,
            hook,
        }
    }
}
impl<T, H: ParamsHook> Params<T, H> {
    /// Retain a borrowed frame guard or another concrete owner without boxing.
    ///
    /// ```compile_fail,E0597
    /// use capnp::{capability::Params, message, text};
    /// let params;
    /// {
    ///     let words = [capnp::word(0,0,0,0,0,0,0,0)];
    ///     let segments = [capnp::Word::words_to_bytes(&words)];
    ///     let reader = message::Reader::new(&segments[..], Default::default());
    ///     params = Params::<text::Owned, _>::from_owner(reader);
    /// }
    /// let _ = params.get();
    /// ```
    pub fn from_owner(hook: H) -> Self {
        Self {
            marker: PhantomData,
            hook,
        }
    }
    #[inline(always)]
    pub fn get(&self) -> crate::Result<T::Reader<'_>>
    where
        T: Owned,
    {
        self.hook.get()?.get_as()
    }
    pub fn into_typed<U>(self) -> Params<U, H> {
        Params {
            marker: PhantomData,
            hook: self.hook,
        }
    }
}

/// The return values of a method, written in-place by the method body.
#[cfg(feature = "alloc")]
pub struct Results<T> {
    pub marker: PhantomData<T>,
    pub hook: alloc::boxed::Box<dyn ResultsHook>,
}

#[cfg(feature = "alloc")]
impl<T> Results<T>
where
    T: Owned,
{
    /// Forward a request with exactly this result schema. Legacy Results still
    /// checks initialization at runtime; generated [`Reply`] contexts prevent
    /// mixing writing and forwarding at compile time.
    pub fn tail_call<P>(self, request: Request<P, T>) -> Promise<(), Error> {
        self.hook.tail_call(request.hook)
    }

    pub fn new(hook: alloc::boxed::Box<dyn ResultsHook>) -> Self {
        Self {
            marker: PhantomData,
            hook,
        }
    }

    #[inline(always)]
    pub fn get(&mut self) -> T::Builder<'_> {
        self.get_with_size_hint(None)
    }

    /// Hint the payload size for the first allocation. Later hints are ignored;
    /// the message can still grow. Exclude the root pointer and RPC envelope.
    pub fn get_with_size_hint(&mut self, size_hint: Option<MessageSize>) -> T::Builder<'_> {
        self.hook
            .get_with_size_hint(size_hint)
            .unwrap()
            .get_as()
            .unwrap()
    }

    /// Replace the result root with a freshly initialized value.
    pub fn init(&mut self) -> T::Builder<'_> {
        self.init_with_size_hint(None)
    }

    pub fn init_with_size_hint(&mut self, size_hint: Option<MessageSize>) -> T::Builder<'_> {
        self.hook.get_with_size_hint(size_hint).unwrap().init_as()
    }

    /// Borrow a result root editor and its orphanage together. Allocate or copy
    /// detached values through `token.in_root(&mut root)`, then call `root.adopt()`.
    /// Both halves borrow this Results, so neither an orphan nor a view can
    /// outlive the response arena. End this borrow before publishing a pipeline.
    ///
    /// ```
    /// use capnp::{capability::Results, introspect::Introspect, schema_capnp::node};
    /// fn build(results: &mut Results<node::Owned>) -> capnp::Result<()> {
    ///     let (mut root, token) = results.get_orphanage(None)?;
    ///     let mut access = token.in_root(&mut root)?;
    ///     let orphan = access.new_struct(node::Owned::introspect().as_struct_schema()?)?;
    ///     let mut orphan = orphan.release_as::<node::Owned>().map_err(|e| e.error)?;
    ///     access.edit_typed(&mut orphan, |mut node| { node.set_id(123); Ok(()) })?;
    ///     root.adopt(orphan.into_dynamic()).map_err(|e| e.error)
    /// }
    /// ```
    ///
    /// ```compile_fail
    /// use capnp::{capability::Results, dynamic_orphan::Orphan,
    ///             introspect::Introspect, schema_capnp::node};
    /// fn escape(results: &mut Results<node::Owned>) -> Orphan<'static> {
    ///     let (mut root, token) = results.get_orphanage(None).unwrap();
    ///     let mut access = token.in_root(&mut root).unwrap();
    ///     access.new_struct(node::Owned::introspect().as_struct_schema().unwrap()).unwrap()
    /// }
    /// ```
    pub fn get_orphanage(
        &mut self,
        size_hint: Option<MessageSize>,
    ) -> crate::Result<(
        crate::dynamic_orphan::Root<'_>,
        crate::dynamic_orphan::Orphanage<'_>,
    )> {
        Ok(crate::dynamic_orphan::Root::new(
            self.hook.get_with_size_hint(size_hint)?,
            T::introspect(),
        )?
        .with_orphanage())
    }

    pub fn set(&mut self, other: T::Reader<'_>) -> crate::Result<()> {
        self.hook.get().unwrap().set_as(other)
    }

    /// Call this method to signal that all of the capabilities have been filled in for this
    /// `Results` and that pipelined calls should be allowed to start using those capabilities.
    /// (Usually pipelined calls are enqueued until the initial call completes.)
    pub fn set_pipeline(&mut self) -> crate::Result<()> {
        self.hook.set_pipeline()
    }

    /// Publish an independently built or forwarded result pipeline, without
    /// initializing the response payload. This may be called only once, including
    /// calls to `set_pipeline()`. The eventual result capabilities must have the
    /// same hook identities as this pipeline, or resolve to those identities.
    /// Publishing does not complete the call or suppress a later error.
    pub fn set_pipeline_from(&mut self, pipeline: T::Pipeline) -> crate::Result<()>
    where
        T: Pipelined,
        T::Pipeline: IntoTypelessPipeline,
    {
        self.hook
            .set_pipeline_from(pipeline.into_typeless_pipeline().into_hook())
    }
}

pub trait FromTypelessPipeline {
    fn new(typeless: any_pointer::Pipeline) -> Self;
}

/// Consume a typed pipeline while preserving its complete pointer path.
pub trait IntoTypelessPipeline {
    fn into_typeless_pipeline(self) -> any_pointer::Pipeline;
}

#[cfg(feature = "alloc")]
impl<T: FromClientHook> FromTypelessPipeline for T {
    fn new(typeless: any_pointer::Pipeline) -> Self {
        Self::new(typeless.as_cap())
    }
}

/// Trait implemented (via codegen) by all user-defined capability client types.
#[cfg(feature = "alloc")]
pub trait FromClientHook {
    /// Describe this capability's current wrappers for debug logging only.
    /// The text is implementation-specific and must not be parsed or used for
    /// equality/authorization. This does not wait for or drive resolution.
    fn debug_info(&self) -> alloc::string::String {
        let mut chain = crate::private::capability::DebugInfo::default();
        chain.follow(self.as_client_hook());
        chain.finish()
    }

    fn interface_schema() -> Option<crate::schema::InterfaceSchema> {
        None
    }

    /// Wraps a client hook to create a new client.
    fn new(hook: alloc::boxed::Box<dyn ClientHook>) -> Self;

    /// Unwraps client to get the underlying client hook.
    fn into_client_hook(self) -> alloc::boxed::Box<dyn ClientHook>;

    /// Gets a reference to the underlying client hook.
    fn as_client_hook(&self) -> &dyn ClientHook;

    /// Casts `self` to another instance of `FromClientHook`. This always succeeds,
    /// but if the underlying capability does not actually implement `T`'s interface,
    /// then method calls will fail with "unimplemented" errors.
    fn cast_to<T: FromClientHook + Sized>(self) -> T
    where
        Self: Sized,
    {
        FromClientHook::new(self.into_client_hook())
    }
}

#[cfg(feature = "alloc")]
impl FromClientHook for alloc::boxed::Box<dyn ClientHook> {
    fn new(hook: alloc::boxed::Box<dyn ClientHook>) -> Self {
        hook
    }

    fn into_client_hook(self) -> alloc::boxed::Box<dyn ClientHook> {
        self
    }

    fn as_client_hook(&self) -> &dyn ClientHook {
        self.as_ref()
    }
}

/// Optional optimization promises made by a caller. A peer may ignore them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CallHints {
    /// The caller will not invoke capabilities through the returned pipeline.
    pub no_promise_pipelining: bool,
    /// Advisory forwarding hint. Use Request::send_for_pipeline to request this
    /// behavior; setting this field is not a substitute for that send method.
    pub only_promise_pipeline: bool,
}

/// An untyped client.
#[cfg(feature = "alloc")]
pub struct Client {
    pub hook: alloc::boxed::Box<dyn ClientHook>,
}

#[cfg(feature = "alloc")]
impl Client {
    /// Describe the current wrapper chain without resolving the capability.
    /// This is diagnostic text only; its format and type names are not stable.
    pub fn debug_info(&self) -> alloc::string::String {
        FromClientHook::debug_info(self)
    }

    pub fn new(hook: alloc::boxed::Box<dyn ClientHook>) -> Self {
        Self { hook }
    }

    pub fn new_call<Params, Results>(
        &self,
        interface_id: u64,
        method_id: u16,
        size_hint: Option<MessageSize>,
    ) -> Request<Params, Results> {
        let typeless = self.hook.new_call(interface_id, method_id, size_hint);
        Request {
            hook: typeless.hook,
            marker: PhantomData,
        }
    }

    /// Construct a call with explicit optimization hints. Use send_for_pipeline
    /// when only the result capabilities, rather than response data, are needed.
    pub fn new_call_with_hints<Params, Results>(
        &self,
        interface_id: u64,
        method_id: u16,
        size_hint: Option<MessageSize>,
        hints: CallHints,
    ) -> Request<Params, Results> {
        let typeless = self
            .hook
            .new_call_with_hints(interface_id, method_id, size_hint, hints);
        Request {
            hook: typeless.hook,
            marker: PhantomData,
        }
    }

    pub fn new_streaming_call<Params>(
        &self,
        interface_id: u64,
        method_id: u16,
        size_hint: Option<MessageSize>,
    ) -> StreamingRequest<Params> {
        let typeless = self.hook.new_call(interface_id, method_id, size_hint);
        StreamingRequest {
            hook: typeless.hook,
            marker: PhantomData,
        }
    }

    /// Construct a streaming call with explicit optimization hints.
    pub fn new_streaming_call_with_hints<Params>(
        &self,
        interface_id: u64,
        method_id: u16,
        size_hint: Option<MessageSize>,
        hints: CallHints,
    ) -> StreamingRequest<Params> {
        let typeless = self
            .hook
            .new_call_with_hints(interface_id, method_id, size_hint, hints);
        StreamingRequest {
            hook: typeless.hook,
            marker: PhantomData,
        }
    }

    /// Obtain this capability's file descriptor, following promise resolution
    /// only when no descriptor is available yet. The returned owner keeps the
    /// descriptor valid independently of the capability's lifetime.
    #[cfg(all(feature = "std", unix))]
    pub fn get_fd(&self) -> Promise<Option<alloc::rc::Rc<std::os::fd::OwnedFd>>, Error> {
        let mut hook = self.hook.add_ref();
        Promise::from_future(async move {
            loop {
                if let Some(fd) = hook.get_fd() {
                    return Ok(Some(fd));
                }
                match hook.when_more_resolved() {
                    Some(promise) => hook = promise.await?,
                    None => return Ok(None),
                }
            }
        })
    }

    /// If the capability is actually only a promise, the returned promise resolves once the
    /// capability itself has resolved to its final destination (or propagates the exception if
    /// the capability promise is rejected).  This is mainly useful for error-checking in the case
    /// where no calls are being made.  There is no reason to wait for this before making calls; if
    /// the capability does not resolve, the call results will propagate the error.
    pub fn when_resolved(&self) -> Promise<(), Error> {
        self.hook.when_resolved()
    }
}

#[cfg(feature = "alloc")]
impl FromClientHook for Client {
    fn new(hook: alloc::boxed::Box<dyn ClientHook>) -> Self {
        Client::new(hook)
    }

    fn into_client_hook(self) -> alloc::boxed::Box<dyn ClientHook> {
        self.hook
    }

    fn as_client_hook(&self) -> &dyn ClientHook {
        self.hook.as_ref()
    }
}

#[cfg(feature = "alloc")]
impl Clone for Client {
    fn clone(&self) -> Self {
        Self {
            hook: self.hook.add_ref(),
        }
    }
}

/// The return value of Server::dispatch_call().
#[cfg(feature = "alloc")]
pub struct DispatchCallResult {
    /// Promise for completion of the call.
    pub promise: Promise<(), Error>,

    /// If true, this method was declared as `-> stream;`. If this call throws
    /// an exception, then all future calls on the capability will throw the
    /// same exception.
    pub is_streaming: bool,

    /// Whether the application method can be canceled after dispatch. Generated
    /// bindings set this from the method/interface/file allowCancellation annotation.
    pub allow_cancellation: bool,
}

#[cfg(feature = "alloc")]
impl DispatchCallResult {
    /// Preserve the cancellable policy of older generated bindings and existing
    /// hand-written dispatchers. New bindings use `with_cancellation_policy`.
    pub fn new(promise: Promise<(), Error>, is_streaming: bool) -> Self {
        Self {
            promise,
            is_streaming,
            allow_cancellation: true,
        }
    }

    /// Select the static cancellation policy of the declaring method.
    pub fn with_cancellation_policy(
        promise: Promise<(), Error>,
        is_streaming: bool,
        allow_cancellation: bool,
    ) -> Self {
        Self {
            promise,
            is_streaming,
            allow_cancellation,
        }
    }
}

/// An executor which owns application calls that must outlive their callers.
/// Implementations must retain and poll accepted tasks until completion or
/// executor shutdown, even if every caller drops its response future.
#[cfg(feature = "alloc")]
pub trait CallExecutor {
    fn spawn(&self, task: Promise<(), Error>) -> crate::Result<()>;
}

/// Type alias that allows us to avoid using `alloc` directly in generated code,
/// which would require an `extern crate alloc` in the crate root.
#[cfg(feature = "alloc")]
pub type Rc<T> = alloc::rc::Rc<T>;

/// A server's weak reference to its own capability. Embed this in the server
/// and expose it through `ServerHooks`; `get()` is available after construction
/// of the local client and becomes unavailable on revocation or last-client drop.
/// Storing this slot in the server does not keep the server alive.
#[cfg(feature = "alloc")]
#[derive(Default)]
pub struct SelfCapability {
    upgrade: core::cell::RefCell<Option<Rc<dyn Fn() -> Option<alloc::boxed::Box<dyn ClientHook>>>>>,
}

#[cfg(feature = "alloc")]
impl SelfCapability {
    pub fn get<C: FromClientHook>(&self) -> Option<C> {
        let upgrade = self.upgrade.borrow().clone()?;
        upgrade().map(C::new)
    }

    #[doc(hidden)]
    pub fn bind(&self, upgrade: Rc<dyn Fn() -> Option<alloc::boxed::Box<dyn ClientHook>>>) {
        *self.upgrade.borrow_mut() = Some(upgrade);
    }
}

/// Optional lifecycle hooks for generated capability servers. Override the
/// generated Server trait's `_capnp_server_hooks()` to expose an implementation.
#[cfg(feature = "alloc")]
pub trait ServerHooks {
    fn self_cap(&self) -> Option<&SelfCapability> {
        None
    }

    /// Calls go to this server until this promise resolves, then to its target.
    /// Previously queued streaming calls retain their position before the target.
    /// Rejecting resolution leaves direct local calls usable but rejects clients
    /// waiting for resolution (including remote promise imports). The RPC runtime
    /// drives this with an explicitly supplied CallExecutor, or while calls and
    /// resolution waiters are polled; there is no implicit ambient executor.
    fn shorten_path(&self) -> Option<Promise<Client, crate::Error>> {
        None
    }
}

/// An untyped server.
#[cfg(feature = "alloc")]
pub trait Server {
    fn dispatch_call(
        self,
        interface_id: u64,
        method_id: u16,
        context: CallContext<'_, any_pointer::Owned, any_pointer::Owned>,
    ) -> DispatchCallResult;

    fn as_ptr(&self) -> usize;

    fn get_hooks(&self) -> Option<&dyn ServerHooks> {
        None
    }
}

/// Trait to track the relationship between generated Server traits and Client structs.
#[cfg(feature = "alloc")]
pub trait FromServer<S>: FromClientHook {
    // Implemented by the generated ServerDispatch struct.
    type Dispatch: Server + 'static + core::ops::Deref<Target = S> + Clone;

    fn from_server(s: Rc<S>) -> Self::Dispatch;
}

/// Gets the "resolved" version of a capability. One place this is useful is for pre-resolving
/// the argument to `capnp_rpc::CapabilityServerSet::get_local_server_of_resolved()`.
#[cfg(feature = "alloc")]
pub async fn get_resolved_cap<C: FromClientHook>(cap: C) -> C {
    let mut hook = cap.into_client_hook();
    let _ = hook.when_resolved().await;
    while let Some(resolved) = hook.get_resolved() {
        hook = resolved;
    }
    FromClientHook::new(hook)
}

#[cfg(all(test, feature = "alloc"))]
mod promise_tests {
    use super::*;
    use alloc::rc::Rc;
    use core::{cell::Cell, marker::PhantomPinned, task::Waker};
    struct OwnedError {
        drops: Rc<Cell<usize>>,
        _marker: PhantomPinned,
    }
    impl Drop for OwnedError {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }
    #[test]
    fn direct_response_preserves_unique_receiver_release_and_compact_storage() {
        assert!(core::mem::size_of::<Promise<(), Error>>() <= 3 * core::mem::size_of::<usize>());
        assert!(
            core::mem::size_of::<ResponsePromise<any_pointer::Owned>>()
                <= 4 * core::mem::size_of::<usize>()
        );
        struct Hook(Rc<Cell<usize>>);
        impl ResponseHook for Hook {
            fn get(&self) -> crate::Result<any_pointer::Reader<'_>> {
                Err(Error::failed(alloc::string::String::from("direct hook")))
            }
        }
        impl Drop for Hook {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        struct State {
            ready: Cell<bool>,
            observed: Cell<bool>,
            cancels: Cell<usize>,
            value: core::cell::RefCell<Option<Result<alloc::boxed::Box<dyn ResponseHook>, Error>>>,
        }
        impl crate::private::capability::SharedPromise<alloc::boxed::Box<dyn ResponseHook>, Error>
            for State
        {
            fn poll(
                &self,
                _: &mut core::task::Context<'_>,
            ) -> Poll<Result<alloc::boxed::Box<dyn ResponseHook>, Error>> {
                assert!(!self.observed.get());
                if self.ready.get() {
                    self.observed.set(true);
                    Poll::Ready(self.value.borrow_mut().take().unwrap())
                } else {
                    Poll::Pending
                }
            }
            fn cancel(self: Rc<Self>) {
                self.cancels.set(self.cancels.get() + 1);
                let value = self.value.borrow_mut().take();
                drop(value);
            }
        }
        for completion in 0..3 {
            let drops = Rc::new(Cell::new(0));
            let value: Result<alloc::boxed::Box<dyn ResponseHook>, Error> = if completion == 2 {
                Err(Error::failed(alloc::string::String::from("direct error")))
            } else {
                Ok(alloc::boxed::Box::new(Hook(drops.clone())))
            };
            let state = Rc::new(State {
                ready: Cell::new(false),
                observed: Cell::new(false),
                cancels: Cell::new(0),
                value: core::cell::RefCell::new(Some(value)),
            });
            let mut response = ResponsePromise::<any_pointer::Owned>::from_shared(state.clone())
                .into_typed_response::<any_pointer::Owned>();
            assert_eq!(Rc::strong_count(&state), 2);
            let mut cx = core::task::Context::from_waker(Waker::noop());
            assert!(Pin::new(&mut response).poll(&mut cx).is_pending());
            if completion != 0 {
                state.ready.set(true);
                let Poll::Ready(result) = Pin::new(&mut response).poll(&mut cx) else {
                    panic!("ready response")
                };
                if completion == 1 {
                    let result = result.unwrap();
                    assert_eq!(result.hook.get().err().unwrap().extra, "direct hook");
                    assert_eq!(drops.get(), 0);
                    drop(result);
                    assert_eq!(drops.get(), 1);
                } else {
                    assert_eq!(result.err().unwrap().extra, "direct error");
                }
                assert_eq!(state.cancels.get(), 1);
                assert_eq!(Rc::strong_count(&state), 1);
            }
            drop(response);
            assert_eq!(state.cancels.get(), 1);
            assert_eq!(Rc::strong_count(&state), 1);
            assert!(state.value.borrow().is_none());
            assert_eq!(drops.get(), usize::from(completion != 2));
        }
    }

    #[test]
    fn typed_response_releases_deferred_owner_at_completion() {
        struct F(Rc<Cell<usize>>);
        impl Future for F {
            type Output = Result<Response<any_pointer::Owned>, Error>;
            fn poll(self: Pin<&mut Self>, _: &mut core::task::Context<'_>) -> Poll<Self::Output> {
                Poll::Ready(Err(Error::failed(alloc::string::String::from("done"))))
            }
        }
        impl Drop for F {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        let drops = Rc::new(Cell::new(0));
        let mut response =
            ResponsePromise::<any_pointer::Owned>::new(Promise::from_future(F(drops.clone())));
        assert!(Pin::new(&mut response)
            .poll(&mut core::task::Context::from_waker(Waker::noop()))
            .is_ready());
        assert_eq!(drops.get(), 1);
        drop(response);
        assert_eq!(drops.get(), 1);
    }

    #[test]
    fn typed_shared_response_errors_release_the_owner_once() {
        struct Failed(Rc<Cell<usize>>);
        impl crate::private::capability::SharedPromise<alloc::boxed::Box<dyn ResponseHook>, Error>
            for Failed
        {
            fn poll(
                &self,
                _: &mut core::task::Context<'_>,
            ) -> Poll<Result<alloc::boxed::Box<dyn ResponseHook>, Error>> {
                Poll::Ready(Err(Error::failed(alloc::string::String::from(
                    "injected completion failure",
                ))))
            }
            fn cancel(self: Rc<Self>) {
                self.0.set(self.0.get() + 1);
            }
        }
        let canceled = Rc::new(Cell::new(0));
        for expected in 1..=2 {
            let mut typed = ResponsePromise::<any_pointer::Owned, _>::from_shared(Rc::new(Failed(
                canceled.clone(),
            )))
            .into_typed_response::<any_pointer::Owned>();
            let Poll::Ready(Err(error)) =
                Pin::new(&mut typed).poll(&mut core::task::Context::from_waker(Waker::noop()))
            else {
                panic!("expected completion error")
            };
            assert_eq!(error.extra, "injected completion failure");
            drop(typed);
            assert_eq!(canceled.get(), expected);
        }
    }
    #[test]
    fn typed_shared_response_preserves_one_receiver_and_hook_ownership() {
        // Response mapping must not enlarge the ubiquitous server completion.
        assert!(core::mem::size_of::<Promise<(), Error>>() <= 3 * core::mem::size_of::<usize>());
        struct Hook;
        impl ResponseHook for Hook {
            fn get(&self) -> crate::Result<any_pointer::Reader<'_>> {
                Err(Error::failed(alloc::string::String::from("test hook")))
            }
        }
        struct State {
            ready: Cell<bool>,
            cancels: Cell<usize>,
        }
        impl crate::private::capability::SharedPromise<alloc::boxed::Box<dyn ResponseHook>, Error>
            for State
        {
            fn poll(
                &self,
                _: &mut core::task::Context<'_>,
            ) -> Poll<Result<alloc::boxed::Box<dyn ResponseHook>, Error>> {
                if self.ready.get() {
                    Poll::Ready(Ok(alloc::boxed::Box::new(Hook)))
                } else {
                    Poll::Pending
                }
            }
            fn cancel(self: Rc<Self>) {
                self.cancels.set(self.cancels.get() + 1);
            }
        }
        for completed in [false, true] {
            let state = Rc::new(State {
                ready: Cell::new(false),
                cancels: Cell::new(0),
            });
            let mut typed = ResponsePromise::<any_pointer::Owned, _>::from_shared(state.clone())
                .into_typed_response::<any_pointer::Owned>();
            assert!(matches!(&typed.inner, ResponseFuture::Direct(_)));
            assert_eq!(Rc::strong_count(&state), 2);
            let mut cx = core::task::Context::from_waker(Waker::noop());
            assert!(Pin::new(&mut typed).poll(&mut cx).is_pending());
            if completed {
                state.ready.set(true);
                let Poll::Ready(Ok(response)) = Pin::new(&mut typed).poll(&mut cx) else {
                    panic!("completed response")
                };
                assert_eq!(response.hook.get().err().unwrap().extra, "test hook");
            }
            drop(typed);
            assert_eq!(state.cancels.get(), 1);
            assert_eq!(Rc::strong_count(&state), 1);
            let weak = Rc::downgrade(&state);
            drop(state);
            assert!(weak.upgrade().is_none());
        }
    }
    #[test]
    fn rejected_promises_transfer_or_drop_the_exact_error_once() {
        for poll in [false, true] {
            for from_result in [false, true] {
                let drops = Rc::new(Cell::new(0));
                let error = OwnedError {
                    drops: drops.clone(),
                    _marker: PhantomPinned,
                };
                let mut promise: Promise<(), OwnedError> = if from_result {
                    Err(error).into()
                } else {
                    Promise::err(error)
                };
                if poll {
                    let Poll::Ready(Err(error)) = Pin::new(&mut promise)
                        .poll(&mut core::task::Context::from_waker(Waker::noop()))
                    else {
                        panic!()
                    };
                    assert_eq!(drops.get(), 0);
                    drop(promise);
                    assert_eq!(drops.get(), 0);
                    drop(error);
                } else {
                    drop(promise);
                }
                assert_eq!(drops.get(), 1);
            }
        }
    }
    #[test]
    fn immediate_and_deferred_success_transfer_their_owned_values() {
        for deferred in [false, true] {
            let value = Rc::new(7);
            let mut promise: Promise<_, ()> = if deferred {
                let value = value.clone();
                Promise::from_future(async move { Ok(value) })
            } else {
                Promise::ok(value.clone())
            };
            let Poll::Ready(Ok(result)) =
                Pin::new(&mut promise).poll(&mut core::task::Context::from_waker(Waker::noop()))
            else {
                panic!()
            };
            assert!(Rc::ptr_eq(&value, &result));
            drop(promise);
            assert_eq!(Rc::strong_count(&value), 2);
            drop(result);
            assert_eq!(Rc::strong_count(&value), 1);
        }
    }
}
