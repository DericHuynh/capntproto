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

#[cfg(feature = "alloc")]
use core::future::Future;
#[cfg(feature = "alloc")]
use core::marker::PhantomData;
#[cfg(feature = "alloc")]
use core::pin::Pin;
#[cfg(feature = "alloc")]
use core::task::Poll;

use crate::any_pointer;
#[cfg(feature = "alloc")]
use crate::private::capability::{ClientHook, ParamsHook, RequestHook, ResponseHook, ResultsHook};
#[cfg(feature = "alloc")]
use crate::traits::{Owned, Pipelined};
#[cfg(feature = "alloc")]
use crate::{Error, MessageSize};

#[cfg(feature = "alloc")]
mod reply;
#[cfg(feature = "alloc")]
pub use reply::{PublishedReply, Reply, ReplyBuilder};

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
    Immediate(Result<T, E>),
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
            inner: PromiseInner::Immediate(Err(error)),
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

    fn poll(self: Pin<&mut Self>, cx: &mut ::core::task::Context) -> Poll<Self::Output> {
        match self.get_mut().inner {
            PromiseInner::Empty => panic!("Promise polled after done."),
            ref mut imm @ PromiseInner::Immediate(_) => {
                match core::mem::replace(imm, PromiseInner::Empty) {
                    PromiseInner::Immediate(r) => Poll::Ready(r),
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
            inner: PromiseInner::Immediate(value),
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
    pub promise: Promise<Response<Results>, crate::Error>,
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
    pub fn into_parts(self) -> (Promise<Response<T>, Error>, T::Pipeline) {
        (self.promise, self.pipeline)
    }
}

/// A response from a method call, as seen by the client.
#[cfg(feature = "alloc")]
pub struct Response<Results> {
    pub marker: PhantomData<Results>,
    pub hook: alloc::boxed::Box<dyn ResponseHook>,
}

#[cfg(feature = "alloc")]
impl<Results> Response<Results>
where
    Results: Pipelined + Owned,
{
    pub fn new(hook: alloc::boxed::Box<dyn ResponseHook>) -> Self {
        Self {
            marker: PhantomData,
            hook,
        }
    }
    pub fn get(&self) -> crate::Result<Results::Reader<'_>> {
        self.hook.get()?.get_as()
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
        let typed_promise = Promise::from_future(async move {
            Ok(Response {
                hook: promise.await?.hook,
                marker: PhantomData,
            })
        });
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

    pub fn get(&mut self) -> Params::Builder<'_> {
        self.hook.get().get_as().unwrap()
    }

    pub fn send(self) -> Promise<(), Error> {
        self.hook.send_streaming()
    }
}

/// The values of the parameters passed to a method call, as seen by the server.
#[cfg(feature = "alloc")]
pub struct Params<T> {
    pub marker: PhantomData<T>,
    pub hook: alloc::boxed::Box<dyn ParamsHook>,
}

#[cfg(feature = "alloc")]
impl<T> Params<T> {
    pub fn new(hook: alloc::boxed::Box<dyn ParamsHook>) -> Self {
        Self {
            marker: PhantomData,
            hook,
        }
    }
    pub fn get(&self) -> crate::Result<T::Reader<'_>>
    where
        T: Owned,
    {
        self.hook.get()?.get_as()
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
        params: Params<any_pointer::Owned>,
        results: Results<any_pointer::Owned>,
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
