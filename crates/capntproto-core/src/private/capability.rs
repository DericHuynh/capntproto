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

#![cfg(feature = "alloc")]

use crate::any_pointer;
use crate::capability::{Params, Promise, RemotePromise, Request, Results};
use crate::MessageSize;
mod metadata;
pub use metadata::{MetadataChanges, MetadataWatch};

/// A single promise receiver over an existing shared completion allocation.
/// Implementations own interior synchronization and must register a task on
/// Pending. The promise calls cancel exactly once on drop, including after
/// successful polling; cancel must distinguish observation from abandonment.
/// Sharing producer state does not clone the promise's receiver ownership.
pub trait SharedPromise<T, E> {
    fn poll(&self, cx: &mut core::task::Context<'_>) -> core::task::Poll<Result<T, E>>;
    fn cancel(self: alloc::rc::Rc<Self>);
}

pub trait ResponseHook {
    fn get(&self) -> crate::Result<any_pointer::Reader<'_>>;
}

pub trait RequestHook {
    /// Prepare a runtime-authorized same-connection tail transfer. Opaque
    /// wrappers keep ordinary forwarding unless they explicitly preserve it.
    fn prepare_tail_call(&mut self, _context: alloc::rc::Rc<dyn core::any::Any>) -> bool {
        false
    }
    /// Private runtime connection context for third-party tail calls. Wrappers
    /// default to ordinary forwarding to preserve their interception policy.
    fn third_party_tail_target(&self) -> Option<alloc::boxed::Box<dyn core::any::Any>> {
        None
    }
    fn set_third_party_tail_target(
        &mut self,
        _contact: any_pointer::Reader<'_>,
    ) -> crate::Result<()> {
        Err(crate::Error::unimplemented(alloc::string::String::from(
            "third-party tail calls unavailable",
        )))
    }
    fn set_hints(&mut self, _hints: crate::capability::CallHints) {}
    fn send_for_pipeline(self: alloc::boxed::Box<Self>) -> any_pointer::Pipeline {
        self.send().pipeline
    }

    fn get(&mut self) -> any_pointer::Builder<'_>;
    fn get_brand(&self) -> usize;
    fn send(self: alloc::boxed::Box<Self>) -> RemotePromise<any_pointer::Owned>;
    fn send_streaming(self: alloc::boxed::Box<Self>) -> Promise<(), crate::Error>;
    fn tail_send(
        self: alloc::boxed::Box<Self>,
    ) -> Option<(
        u32,
        crate::capability::Promise<(), crate::Error>,
        alloc::boxed::Box<dyn PipelineHook>,
    )>;
}

/// A joined capability and network-owned context retained until its caller has
/// acquired the capability (or, for a relay, until all upstream Finishes).
pub struct JoinedCapability {
    pub cap: Option<alloc::boxed::Box<dyn ClientHook>>,
    pub guard: alloc::boxed::Box<dyn core::any::Any>,
}

/// An explicitly authorized opaque-boundary Join. The runtime joins `caps`
/// using its existing network context, then passes the pending operation to
/// `complete`, which must preserve boundary policy and downstream retention.
pub struct JoinDelegation {
    pub caps: alloc::vec::Vec<alloc::boxed::Box<dyn ClientHook>>,
    pub complete: alloc::boxed::Box<
        dyn FnOnce(
            Promise<JoinedCapability, crate::Error>,
        ) -> Promise<JoinedCapability, crate::Error>,
    >,
}

/// Collector for a capability's diagnostic wrapper chain. Hook implementations
/// append labels and use `follow()` for inner hooks. The depth limit also makes
/// diagnostics terminate for cyclic or unexpectedly deep wrapper graphs.
/// Labels are for humans, not a stable serialization or capability identity.
#[derive(Default)]
pub struct DebugInfo {
    chain: alloc::vec::Vec<alloc::string::String>,
    depth: usize,
}
impl DebugInfo {
    pub fn push(&mut self, label: impl Into<alloc::string::String>) {
        self.chain.push(label.into());
    }

    /// Inspect another layer without querying resolution, identity or authority.
    pub fn follow(&mut self, hook: &dyn ClientHook) {
        if self.depth == 64 {
            self.push("truncated");
            return;
        }
        self.depth += 1;
        hook.debug_info(self);
        self.depth -= 1;
    }

    pub fn finish(self) -> alloc::string::String {
        self.chain.join(":")
    }
}

pub trait ClientHook {
    /// Changes to this hook's FD or diagnostics, independently of promise
    /// resolution. None declares metadata immutable while the route is stable.
    /// Mutable hooks must notify this source after each update. Policy wrappers
    /// expose their own notifications; this grants no access to an inner hook.
    fn metadata_changes(&self) -> Option<alloc::rc::Rc<MetadataChanges>> {
        None
    }

    /// Optional concrete runtime identity for safe in-process adapter lookup.
    /// An adapter must downcast and validate its own private identity; pointer
    /// and connection-brand integers alone are not capability authority.
    /// Wrappers stay opaque by default. Forward only for an explicitly
    /// transparent wrapper, since exposing an inner hook can bypass policy.
    fn as_any(&self) -> Option<&dyn core::any::Any> {
        None
    }

    /// Append this hook's diagnostic description. Overrides must not make calls,
    /// drive promises, connect, or otherwise change capability state. Follow
    /// inner hooks through `chain.follow()` so recursion remains bounded.
    fn debug_info(&self, chain: &mut DebugInfo) {
        chain.push(core::any::type_name::<Self>());
    }

    /// A terminal failure already known locally, independently of whether this
    /// reference is settled. `None` is not a liveness guarantee. Do not send
    /// calls or start resolution/connection work to answer this query.
    ///
    /// Join uses this before proving equality: a settled broken reference must
    /// not be confused with usable authority. Transparent retention wrappers
    /// forward this query; opaque policies report their own revocation too.
    fn get_failure(&self) -> Option<crate::Error> {
        None
    }

    /// Opt in to joining a complete batch through an opaque policy boundary.
    /// None keeps the hook an opaque equality endpoint. Implementations must
    /// validate every input and protect the result; never forward individual
    /// multiparty shares merely because a batch can be delegated.
    fn delegate_join(
        &self,
        _caps: &[alloc::boxed::Box<dyn ClientHook>],
    ) -> Option<crate::Result<JoinDelegation>> {
        None
    }

    /// Forward an independently routed Join share through a transparent RPC
    /// proxy. None identifies this hook as an opaque/local equality endpoint;
    /// wrappers must not forward implicitly across their policy boundary.
    fn forward_join(
        &self,
        _part: any_pointer::Reader<'_>,
    ) -> Option<Promise<alloc::boxed::Box<dyn ResponseHook>, crate::Error>> {
        None
    }

    /// Join settled capabilities sharing this hook's connection brand. Network
    /// hooks may implement this to cross independent RPC systems without exposing
    /// their connection type. Opaque wrappers must not delegate this implicitly:
    /// doing so could bypass their capability policy.
    fn join_capabilities(
        &self,
        _caps: alloc::vec::Vec<alloc::boxed::Box<dyn ClientHook>>,
    ) -> Promise<JoinedCapability, crate::Error> {
        Promise::err(crate::Error::unimplemented(alloc::string::String::from(
            "capability does not support distributed Join",
        )))
    }

    /// A server-set lookup may not bypass streaming calls already queued on a
    /// local server. The returned barrier covers work queued before this call.
    fn when_local_server_ready(&self) -> Promise<(), crate::Error> {
        Promise::ok(())
    }
    fn is_local_server_ready(&self) -> bool {
        true
    }

    fn new_call_with_hints(
        &self,
        interface_id: u64,
        method_id: u16,
        size_hint: Option<MessageSize>,
        hints: crate::capability::CallHints,
    ) -> Request<any_pointer::Owned, any_pointer::Owned> {
        let mut request = self.new_call(interface_id, method_id, size_hint);
        request.hook.set_hints(hints);
        request
    }
    fn call_with_hints(
        &self,
        interface_id: u64,
        method_id: u16,
        params: alloc::boxed::Box<dyn ParamsHook>,
        results: alloc::boxed::Box<dyn ResultsHook>,
        _hints: crate::capability::CallHints,
    ) -> Promise<(), crate::Error> {
        self.call(interface_id, method_id, params, results)
    }

    /// Start a call during an inbound dispatch poll. The caller has already
    /// admitted it and released all protocol-table borrows before running user
    /// code. Unlike call_with_hints, local implementations may dispatch before
    /// returning. Pending operations retain ordinary cancellation, streaming
    /// ordering and revocation behavior. Opaque wrappers keep their usual path.
    fn call_during_dispatch(
        &self,
        interface_id: u64,
        method_id: u16,
        context: &mut dyn CallContextHook,
        hints: crate::capability::CallHints,
    ) -> Promise<(), crate::Error> {
        // An opaque wrapper retains its normal policy path and takes ownership.
        let (params, results) = context.take();
        self.call_with_hints(interface_id, method_id, params, results, hints)
    }

    /// An owned reference to the descriptor associated with this capability.
    /// May be available before a promise resolves. The owner remains alive for
    /// as long as a caller or outgoing message retains this reference.
    #[cfg(all(feature = "std", unix))]
    fn get_fd(&self) -> Option<alloc::rc::Rc<std::os::fd::OwnedFd>> {
        None
    }

    fn add_ref(&self) -> alloc::boxed::Box<dyn ClientHook>;
    fn new_call(
        &self,
        interface_id: u64,
        method_id: u16,
        size_hint: Option<MessageSize>,
    ) -> Request<any_pointer::Owned, any_pointer::Owned>;

    fn call(
        &self,
        interface_id: u64,
        method_id: u16,
        params: alloc::boxed::Box<dyn ParamsHook>,
        results: alloc::boxed::Box<dyn ResultsHook>,
    ) -> crate::capability::Promise<(), crate::Error>;

    /// If this capability is associated with an rpc connection, then this method
    /// returns an identifier for that connection.
    fn get_brand(&self) -> usize;

    /// Returns a (locally) unique identifier for this capability.
    fn get_ptr(&self) -> usize;

    /// If this ClientHook is a promise that has already resolved, returns the inner, resolved version
    /// of the capability.  The caller may permanently replace this client with the resolved one if
    /// desired.  Returns null if the client isn't a promise or hasn't resolved yet -- use
    /// `whenMoreResolved()` to distinguish between them.
    fn get_resolved(&self) -> Option<alloc::boxed::Box<dyn ClientHook>>;

    /// If this client is a settled reference (not a promise), return nullptr.  Otherwise, return a
    /// promise that eventually resolves to a new client that is closer to being the final, settled
    /// client (i.e. the value eventually returned by `getResolved()`).  Calling this repeatedly
    /// should eventually produce a settled client.
    fn when_more_resolved(
        &self,
    ) -> Option<crate::capability::Promise<alloc::boxed::Box<dyn ClientHook>, crate::Error>>;

    /// Repeatedly calls whenMoreResolved() until it returns nullptr.
    fn when_resolved(&self) -> Promise<(), crate::Error>;
}

impl Clone for alloc::boxed::Box<dyn ClientHook> {
    fn clone(&self) -> Self {
        self.add_ref()
    }
}

/// A context retained until protected execution completes. Shared contexts
/// retain their existing allocation; owned adapters preserve their exact guard.
pub enum CancellationGuard {
    Owned(alloc::boxed::Box<dyn core::any::Any>),
    Shared(alloc::rc::Rc<dyn core::any::Any>),
}

/// Storage behind a single dispatch invocation. Taking ownership consumes both
/// hooks exactly once; subsequent access is a programming error. Implementations
/// must preserve the original result producer and its cancellation context.
pub trait CallContextHook {
    fn borrow(&mut self) -> (&dyn ParamsHook, &mut dyn ResultsHook);
    fn results(&self) -> &dyn ResultsHook;
    fn take(
        &mut self,
    ) -> (
        alloc::boxed::Box<dyn ParamsHook>,
        alloc::boxed::Box<dyn ResultsHook>,
    );
}

/// An already-owned invocation, used by queued calls and policy adapters.
pub struct OwnedCall {
    params: Option<alloc::boxed::Box<dyn ParamsHook>>,
    results: Option<alloc::boxed::Box<dyn ResultsHook>>,
}
impl OwnedCall {
    pub fn new(
        params: alloc::boxed::Box<dyn ParamsHook>,
        results: alloc::boxed::Box<dyn ResultsHook>,
    ) -> Self {
        Self {
            params: Some(params),
            results: Some(results),
        }
    }
}
impl CallContextHook for OwnedCall {
    fn borrow(&mut self) -> (&dyn ParamsHook, &mut dyn ResultsHook) {
        (
            &**self.params.as_ref().expect("consumed call"),
            &mut **self.results.as_mut().expect("consumed call"),
        )
    }
    fn results(&self) -> &dyn ResultsHook {
        &**self.results.as_ref().expect("consumed call")
    }
    fn take(
        &mut self,
    ) -> (
        alloc::boxed::Box<dyn ParamsHook>,
        alloc::boxed::Box<dyn ResultsHook>,
    ) {
        (
            self.params.take().expect("consumed call"),
            self.results.take().expect("consumed call"),
        )
    }
}

pub trait ResultsHook {
    /// Identify this exact result sink. Transforming/policy wrappers should use
    /// the default rather than exposing an inner sink through this method.
    fn as_any_mut(&mut self) -> Option<&mut dyn core::any::Any> {
        None
    }

    /// Keep adapter-owned context alive with completed response readers and
    /// pipelines. Results implementations that retain response data should
    /// retain this owner alongside it; forwarding wrappers should delegate.
    /// Stateless/custom result sinks may use the default no-op.
    fn retain_response_context(&mut self, _context: alloc::rc::Rc<dyn core::any::Any>) {}

    /// The caller permits one immediate poll of a non-streaming implementation
    /// during dispatch. RPC sets this only after releasing its protocol-table
    /// borrows. Ordinary local calls and wrappers remain deferred by default.
    fn permits_immediate_poll(&self) -> bool {
        false
    }

    /// Retain protocol context until a non-cancellable method actually completes.
    fn cancellation_guard(&self) -> Option<alloc::boxed::Box<dyn core::any::Any>> {
        None
    }
    /// Retain the cancellation context without boxing an already shared owner.
    /// The default preserves custom hooks implementing cancellation_guard().
    fn cancellation_context(&self) -> Option<CancellationGuard> {
        self.cancellation_guard().map(CancellationGuard::Owned)
    }
    /// Wire calls supply their RPC system's executor. Pure local calls can use
    /// the executor configured on their local capability.
    fn cancellation_executor(&self) -> Option<alloc::rc::Rc<dyn crate::capability::CallExecutor>> {
        None
    }

    fn get(&mut self) -> crate::Result<any_pointer::Builder<'_>> {
        self.get_with_size_hint(None)
    }

    /// Only the first result allocation consults the hint; it excludes the root
    /// pointer and RPC envelope. Hints never limit the eventual result size.
    fn get_with_size_hint(
        &mut self,
        size_hint: Option<crate::MessageSize>,
    ) -> crate::Result<any_pointer::Builder<'_>>;

    /// Replace the result payload with an immutable, independently owned reply.
    /// The default copies through this hook's ordinary builder, preserving
    /// capability transformations in wrappers. An owning sink may retain the
    /// response instead, provided later edits copy it before mutation and all
    /// response contexts and early pipelines retain their usual lifetimes.
    fn set_response(&mut self, response: alloc::rc::Rc<dyn ResponseHook>) -> crate::Result<()> {
        let reader = response.get()?;
        self.get_with_size_hint(Some(reader.target_size()?))?
            .set_as(reader)
    }

    fn set_pipeline(&mut self) -> crate::Result<()>;

    /// Publish capability routing independently of the eventual results payload.
    fn set_pipeline_from(
        &mut self,
        pipeline: alloc::boxed::Box<dyn PipelineHook>,
    ) -> crate::Result<()>;

    fn allow_cancellation(&self);
    fn tail_call(
        self: alloc::boxed::Box<Self>,
        request: alloc::boxed::Box<dyn RequestHook>,
    ) -> Promise<(), crate::Error>;
    fn direct_tail_call(
        self: alloc::boxed::Box<Self>,
        request: alloc::boxed::Box<dyn RequestHook>,
    ) -> (
        crate::capability::Promise<(), crate::Error>,
        alloc::boxed::Box<dyn PipelineHook>,
    );
}

pub trait ParamsHook {
    fn get(&self) -> crate::Result<crate::any_pointer::Reader<'_>>;
    /// Optional private implementation identity. Wrappers retain their own
    /// identity, so a transport cannot bypass their parameter transformations.
    fn as_any(&self) -> Option<&dyn core::any::Any> {
        None
    }
}

// Where should this live?
pub fn internal_get_typed_params<T>(typeless: Params<any_pointer::Owned>) -> Params<T> {
    Params {
        hook: typeless.hook,
        marker: ::core::marker::PhantomData,
    }
}

pub fn internal_get_typed_results<T>(typeless: Results<any_pointer::Owned>) -> Results<T> {
    Results {
        hook: typeless.hook,
        marker: ::core::marker::PhantomData,
    }
}

/// Generated dispatcher boundary. Runtime implementors must supply a fresh,
/// unpublished results context, just as for internal_get_typed_results().
#[doc(hidden)]
pub fn internal_get_typed_reply<T>(
    typeless: Results<any_pointer::Owned>,
) -> crate::capability::Reply<T> {
    crate::capability::Reply::new(typeless.hook)
}

pub fn internal_get_untyped_results<T>(typeful: Results<T>) -> Results<any_pointer::Owned> {
    Results {
        hook: typeful.hook,
        marker: ::core::marker::PhantomData,
    }
}

pub trait PipelineHook {
    fn add_ref(&self) -> alloc::boxed::Box<dyn PipelineHook>;
    fn get_pipelined_cap(&self, ops: &[PipelineOp]) -> alloc::boxed::Box<dyn ClientHook>;

    /// Version of get_pipelined_cap() passing the array by move. May avoid a copy in some cases.
    /// Default implementation just calls the other version.
    fn get_pipelined_cap_move(
        &self,
        ops: alloc::vec::Vec<PipelineOp>,
    ) -> alloc::boxed::Box<dyn ClientHook> {
        self.get_pipelined_cap(&ops)
    }
}

impl Clone for alloc::boxed::Box<dyn PipelineHook> {
    fn clone(&self) -> Self {
        self.add_ref()
    }
}

#[derive(Clone, Copy)]
pub enum PipelineOp {
    Noop,
    GetPointerField(u16),
}
