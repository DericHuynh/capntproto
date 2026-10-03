// Copyright (c) 2026 ReProto contributors
// Licensed under the MIT license; see LICENSE.

//! Bidirectional capability membranes. Every capability embedded in parameters,
//! results, or pipelines crosses the same policy; reverse crossings unwrap it.
use capnp::any_pointer;
use capnp::capability::CallHints;
use capnp::capability::{Client, FromClientHook, Promise, Request};
use capnp::private::capability::{
    ClientHook, JoinDelegation, JoinedCapability, ParamsHook, PipelineHook, RequestHook,
    ResponseHook, ResultsHook,
};
use capnp::traits::{Imbue, ImbueMut};
use capnp::Error;
use futures::{FutureExt, TryFutureExt};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Inbound,
    Outbound,
}

/// A policy can reject a call, permit it, or redirect it to another capability.
/// A redirect is outside the membrane's transformation boundary, as in C++.
pub trait Policy {
    /// A diagnostic type label, captured when constructing the membrane.
    /// This text is for debug logs only, not a stable identity or authority.
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Permit equality checks on distinct wrappers from this exact boundary and
    /// direction. Default denial is an unsupported operation, not inequality.
    /// Targets are already held by this policy; permission does not assert their
    /// equality. The runtime performs Join and transforms its result through the
    /// same membrane. Equal wrapper aliases need no delegation.
    fn allow_join(&self, _direction: Direction, _targets: &[Client]) -> capnp::Result<bool> {
        Ok(false)
    }

    /// File descriptors bypass RPC interception and cannot be revoked after
    /// delivery. Opt in only when this is consistent with the boundary policy.
    fn allow_fd_passthrough(&self) -> bool {
        false
    }

    fn call(
        &self,
        direction: Direction,
        interface: u64,
        method: u16,
        target: &Client,
    ) -> capnp::Result<Option<Client>>;

    /// Substitute for an external capability crossing inward. `None` selects
    /// the usual reversible wrapper. Substitutions control their own boundary
    /// behavior, including revocation, and are not automatically wrapped again.
    fn import_external(&self, _cap: &Client) -> capnp::Result<Option<Client>> {
        Ok(None)
    }

    /// Substitute for an internal capability crossing outward. See
    /// `import_external()` for substitution and default semantics.
    fn export_internal(&self, _cap: &Client) -> capnp::Result<Option<Client>> {
        Ok(None)
    }

    /// Optional policy-owned revocation signal. It must reject with the
    /// revocation error. Successful completion is treated as a policy error.
    /// The boundary requests this once and shares it across all wrappers.
    fn on_revoked(&self) -> Option<Promise<(), Error>> {
        None
    }

    /// Delay a redirect while its target is a promise. Resolution may reflect
    /// the call back across the boundary, in which case no interception applies.
    fn should_resolve_before_redirecting(&self) -> bool {
        false
    }

    /// Related policies may share a root for reversible crossings. The root
    /// identity must remain stable for the policy's lifetime.
    fn root_policy(&self) -> Option<Rc<dyn Policy>> {
        None
    }

    /// Re-import an internal capability previously exported by a related policy.
    fn import_internal(&self, cap: Client, _export: &dyn Policy, _import: &dyn Policy) -> Client {
        cap
    }

    /// Re-export an external capability previously imported by a related policy.
    fn export_external(&self, cap: Client, _import: &dyn Policy, _export: &dyn Policy) -> Client {
        cap
    }
}

thread_local! {
    // Like C++ MembranePolicy's wrapper maps, boundary identity belongs to the
    // policy object, not to each construction of the Membrane handle.
    static CORES: RefCell<HashMap<usize, Weak<Core>>> = RefCell::new(HashMap::new());
}

struct Core {
    policy: Rc<dyn Policy>,
    debug_name: &'static str,
    root: Rc<dyn Policy>,
    by_inner: RefCell<HashMap<(usize, bool), Weak<Wrapper>>>,
    by_wrapper: RefCell<HashMap<usize, Weak<Wrapper>>>,
    revoked: RefCell<Option<Error>>,
    revocation: Option<
        futures::future::Shared<futures::future::LocalBoxFuture<'static, Result<(), Error>>>,
    >,
    waiters: RefCell<crate::sender_queue::SenderQueue<(), Error>>,
}

/// One shared membrane boundary and its revocation authority.
#[derive(Clone)]
pub struct Membrane(Rc<Core>);
impl Membrane {
    pub fn new(policy: Rc<dyn Policy>) -> Self {
        let key = Rc::as_ptr(&policy) as *const () as usize;
        if let Some(core) = CORES.with(|cores| cores.borrow().get(&key).and_then(Weak::upgrade)) {
            return Self(core);
        }
        let root = policy.root_policy().unwrap_or_else(|| policy.clone());
        let signal = policy.on_revoked();
        let debug_name = policy.debug_name();
        let core = Rc::new_cyclic(|weak: &Weak<Core>| {
            let weak = weak.clone();
            let revocation = signal.map(|signal| {
                async move {
                    let error = signal.await.err().unwrap_or_else(|| {
                        Error::failed(
                            "on_revoked() completed successfully; expected an error".into(),
                        )
                    });
                    if let Some(core) = weak.upgrade() {
                        Membrane(core).revoke(error.clone());
                    }
                    Err(error)
                }
                .boxed_local()
                .shared()
            });
            Core {
                policy,
                debug_name,
                root,
                by_inner: RefCell::new(HashMap::new()),
                by_wrapper: RefCell::new(HashMap::new()),
                revoked: RefCell::new(None),
                revocation,
                waiters: RefCell::new(crate::sender_queue::SenderQueue::new()),
            }
        });
        CORES.with(|cores| {
            let mut cores = cores.borrow_mut();
            cores.retain(|_, core| core.strong_count() != 0);
            cores.insert(key, Rc::downgrade(&core));
            Self(core)
        })
    }

    pub fn export<T: FromClientHook>(&self, cap: T) -> T {
        T::new(self.wrap(cap.into_client_hook(), false))
    }
    pub fn import<T: FromClientHook>(&self, cap: T) -> T {
        T::new(self.wrap(cap.into_client_hook(), true))
    }

    /// Copy an outside value into an inside orphanage, importing every embedded
    /// capability through this boundary. Generated readers, compiled dynamic
    /// values, lists and AnyPointers are accepted. Independent aggregates retain
    /// unknown fields. Inline groups follow Rust orphanage rules: copy known
    /// fields and the active union arm, excluding parent siblings.
    /// Reverse crossings unwrap existing wrappers, just as [`Self::import`].
    ///
    /// The returned owner borrows the destination message, not the source. It
    /// can be adopted or accessed through the supplied orphanage's token. The
    /// destination must be imbued with a capability table if the value contains
    /// capabilities. Copying does not invoke RPC methods or await promises.
    /// Structural errors leave existing source/destination values unchanged;
    /// policy callbacks run only after the structural copy succeeds.
    ///
    /// Revocation does not prevent copying data: ordinary copied capabilities
    /// become broken. Policy substitutions retain their own revocation rules,
    /// and substitution errors become broken capabilities, as for `import()`.
    pub fn copy_into<'source, 'message>(
        &self,
        value: impl Into<capnp::dynamic_value::Reader<'source>>,
        to: &mut capnp::dynamic_orphan::Access<'_, 'message>,
    ) -> capnp::Result<capnp::dynamic_orphan::Orphan<'message>> {
        to.copy_with_capability_transform(value.into(), |cap| Ok(self.import(cap)))
    }

    /// Copy an inside value into an outside orphanage, exporting all embedded
    /// capabilities. This is C++ `copyOutOfMembrane()`; [`Self::copy_into`] is
    /// `copyIntoMembrane()`. Ownership, failure and revocation rules are the
    /// same as `copy_into()`, with the crossing direction reversed.
    pub fn copy_out<'source, 'message>(
        &self,
        value: impl Into<capnp::dynamic_value::Reader<'source>>,
        to: &mut capnp::dynamic_orphan::Access<'_, 'message>,
    ) -> capnp::Result<capnp::dynamic_orphan::Orphan<'message>> {
        to.copy_with_capability_transform(value.into(), |cap| Ok(self.export(cap)))
    }
    /// Rejects future and pending calls through either side of this boundary.
    /// Already-executed application side effects cannot be rolled back.
    pub fn revoke(&self, error: Error) {
        if self.0.revoked.borrow().is_some() {
            return;
        }
        *self.0.revoked.borrow_mut() = Some(error.clone());
        for (_, sender) in self.0.waiters.borrow_mut().drain() {
            let _ = sender.send(error.clone());
        }
        // Original targets can be destroyed by revocation; their addresses may
        // subsequently be reused. Do not keep identity cache entries for them.
        self.0.by_inner.borrow_mut().clear();
        let wrappers = self
            .0
            .by_wrapper
            .borrow()
            .values()
            .filter_map(Weak::upgrade)
            .collect::<Vec<_>>();
        for wrapper in wrappers {
            if wrapper.custom {
                let source = wrapper.source.borrow_mut().take();
                drop(source);
                continue;
            }
            let old = wrapper.inner.replace(crate::broken::new_cap(error.clone()));
            drop(old);
        }
    }
    fn check(&self) -> capnp::Result<()> {
        self.refresh_revocation();
        match &*self.0.revoked.borrow() {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }
    fn refresh_revocation(&self) {
        if let Some(signal) = &self.0.revocation {
            let _ = signal.clone().now_or_never();
        }
    }

    /// Wait for manual or policy-owned revocation, rejecting with its error.
    /// Poll this future on an executor for eager cleanup even when there are
    /// no calls. Active calls/resolution waiters also drive the policy signal.
    pub fn when_revoked(&self) -> Promise<(), Error> {
        if let Err(error) = self.check() {
            return Promise::err(error);
        }
        let manual = Promise::from_future(
            self.0
                .waiters
                .borrow_mut()
                .push(())
                .and_then(|error| async { Err(error) }),
        );
        match self.0.revocation.clone() {
            None => manual,
            Some(signal) => Promise::from_future(async move {
                match futures::future::select(signal, manual).await {
                    futures::future::Either::Left((result, _)) => result,
                    futures::future::Either::Right((result, _)) => result,
                }
            }),
        }
    }
    fn wrap(&self, cap: Box<dyn ClientHook>, reverse: bool) -> Box<dyn ClientHook> {
        self.refresh_revocation();
        let related = CORES.with(|cores| {
            cores
                .borrow()
                .values()
                .filter_map(Weak::upgrade)
                .filter(|core| Rc::ptr_eq(&core.root, &self.0.root))
                .collect::<Vec<_>>()
        });
        for core in related {
            let existing = core
                .by_wrapper
                .borrow()
                .get(&cap.get_ptr())
                .and_then(Weak::upgrade);
            if let Some(wrapper) = existing {
                if !wrapper.custom && wrapper.reverse != reverse {
                    let original = Client::new(wrapper.inner());
                    let source = wrapper.membrane.0.policy.as_ref();
                    let destination = self.0.policy.as_ref();
                    return if reverse {
                        self.0.root.import_internal(original, source, destination)
                    } else {
                        self.0.root.export_external(original, source, destination)
                    }
                    .hook;
                }
            }
        }
        let key = (cap.get_ptr(), reverse);
        let revoked = self.0.revoked.borrow().clone();
        if revoked.is_none() {
            if let Some(existing) = self.0.by_inner.borrow().get(&key).and_then(Weak::upgrade) {
                return Box::new(Hook(existing));
            }
        }
        let target = Client::new(cap);
        let substitution = if reverse {
            self.0.policy.import_external(&target)
        } else {
            self.0.policy.export_internal(&target)
        };
        let substitution = match substitution {
            Ok(value) => value,
            Err(error) => return crate::broken::new_cap(error),
        };
        let custom = substitution.is_some();
        let (inner, source) = match substitution {
            Some(replacement) => (replacement.hook, revoked.is_none().then_some(target.hook)),
            None => (
                match revoked {
                    Some(ref error) => crate::broken::new_cap(error.clone()),
                    None => target.hook,
                },
                None,
            ),
        };
        let wrapper = Rc::new(Wrapper {
            identity: key.0,
            inner: RefCell::new(inner),
            source: RefCell::new(source),
            custom,
            resolved: RefCell::new(None),
            membrane: self.clone(),
            reverse,
        });
        if revoked.is_none() {
            self.0
                .by_inner
                .borrow_mut()
                .insert(key, Rc::downgrade(&wrapper));
        }
        self.0
            .by_wrapper
            .borrow_mut()
            .insert(Rc::as_ptr(&wrapper) as usize, Rc::downgrade(&wrapper));
        Box::new(Hook(wrapper))
    }
}
struct Wrapper {
    identity: usize,
    inner: RefCell<Box<dyn ClientHook>>,
    // Keep the original cache key alive while a custom substitution is held.
    source: RefCell<Option<Box<dyn ClientHook>>>,
    custom: bool,
    resolved: RefCell<Option<Box<dyn ClientHook>>>,
    membrane: Membrane,
    reverse: bool,
}
impl Wrapper {
    fn inner(&self) -> Box<dyn ClientHook> {
        self.inner.borrow().clone()
    }
}
impl Drop for Wrapper {
    fn drop(&mut self) {
        let key = (self.identity, self.reverse);
        let mut cache = self.membrane.0.by_inner.borrow_mut();
        if cache
            .get(&key)
            .is_some_and(|weak| std::ptr::eq(weak.as_ptr(), self))
        {
            cache.remove(&key);
        }
        drop(cache);
        self.membrane
            .0
            .by_wrapper
            .borrow_mut()
            .remove(&(self as *const _ as usize));
    }
}
struct Hook(Rc<Wrapper>);
impl ClientHook for Hook {
    fn debug_info(&self, chain: &mut capnp::private::capability::DebugInfo) {
        chain.push(self.0.membrane.0.debug_name);
        let inner = {
            let Ok(inner) = self.0.inner.try_borrow() else {
                chain.push("busy");
                return;
            };
            inner.clone()
        };
        chain.follow(&*inner);
    }

    fn delegate_join(&self, caps: &[Box<dyn ClientHook>]) -> Option<capnp::Result<JoinDelegation>> {
        Some((|| {
            let membrane = self.0.membrane.clone();
            membrane.check()?;
            let mut targets = Vec::with_capacity(caps.len());
            for cap in caps {
                let wrapper = if cap.get_brand() == self.get_brand() {
                    membrane
                        .0
                        .by_wrapper
                        .borrow()
                        .get(&cap.get_ptr())
                        .and_then(Weak::upgrade)
                } else {
                    None
                };
                let wrapper = wrapper.ok_or_else(|| {
                    Error::unimplemented("Join crosses distinct membrane boundaries".into())
                })?;
                if wrapper.custom || wrapper.reverse != self.0.reverse {
                    return Err(Error::unimplemented(
                        "Join crosses distinct membrane directions or substitutions".into(),
                    ));
                }
                targets.push(Client::new(wrapper.inner()));
            }
            let reverse = self.0.reverse;
            let direction = if reverse {
                Direction::Outbound
            } else {
                Direction::Inbound
            };
            if !membrane.0.policy.allow_join(direction, &targets)? {
                return Err(Error::unimplemented(
                    "membrane policy does not permit Join".into(),
                ));
            }
            // Policy callbacks may revoke reentrantly. Do not start a network
            // operation after such a callback has withdrawn authority.
            membrane.check()?;
            Ok(JoinDelegation {
                caps: targets.into_iter().map(|c| c.hook).collect(),
                complete: Box::new(move |operation| {
                    Promise::from_future(async move {
                        membrane.check()?;
                        let outcome =
                            match futures::future::select(operation, membrane.when_revoked()).await
                            {
                                futures::future::Either::Left((result, canceled)) => {
                                    drop(canceled);
                                    result?
                                }
                                futures::future::Either::Right((result, operation)) => {
                                    drop(operation);
                                    result?;
                                    unreachable!()
                                }
                            };
                        membrane.check()?;
                        let cap = outcome.cap.map(|cap| membrane.wrap(cap, reverse));
                        // Result transformations can also revoke reentrantly.
                        membrane.check()?;
                        Ok(JoinedCapability {
                            cap,
                            guard: outcome.guard,
                        })
                    })
                }),
            })
        })())
    }

    #[cfg(unix)]
    fn get_fd(&self) -> Option<Rc<std::os::fd::OwnedFd>> {
        self.0.membrane.refresh_revocation();
        let fd = self.0.inner().get_fd()?;
        if self.0.custom || self.0.membrane.0.policy.allow_fd_passthrough() {
            Some(fd)
        } else {
            None
        }
    }

    fn add_ref(&self) -> Box<dyn ClientHook> {
        Box::new(Self(self.0.clone()))
    }
    fn get_ptr(&self) -> usize {
        Rc::as_ptr(&self.0) as usize
    }
    fn get_brand(&self) -> usize {
        Rc::as_ptr(&self.0.membrane.0) as usize
    }
    fn new_call(
        &self,
        interface: u64,
        method: u16,
        size: Option<capnp::MessageSize>,
    ) -> Request<any_pointer::Owned, any_pointer::Owned> {
        if self.0.custom {
            return self.0.inner().new_call(interface, method, size);
        }
        Request::new(Box::new(crate::local::Request::new(
            interface,
            method,
            size,
            self.add_ref(),
        )))
    }
    fn call(
        &self,
        interface: u64,
        method: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
    ) -> Promise<(), Error> {
        self.call_with_hints(interface, method, params, results, CallHints::default())
    }
    fn call_with_hints(
        &self,
        interface: u64,
        method: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
        hints: CallHints,
    ) -> Promise<(), Error> {
        let wrapper = self.0.clone();
        if wrapper.custom {
            return wrapper
                .inner()
                .call_with_hints(interface, method, params, results, hints);
        }
        Promise::from_future(async move {
            wrapper.membrane.refresh_revocation();
            let hook = Hook(wrapper.clone());
            if let Some(resolved) = hook.get_resolved() {
                return resolved
                    .call_with_hints(interface, method, params, results, hints)
                    .await;
            }
            let target = Client::new(wrapper.inner());
            let direction = if wrapper.reverse {
                Direction::Outbound
            } else {
                Direction::Inbound
            };
            if let Some(redirect) = wrapper
                .membrane
                .0
                .policy
                .call(direction, interface, method, &target)?
            {
                if wrapper
                    .membrane
                    .0
                    .policy
                    .should_resolve_before_redirecting()
                {
                    if let Some(resolving) = hook.when_more_resolved() {
                        let resolved = resolving.await?;
                        return resolved
                            .call_with_hints(interface, method, params, results, hints)
                            .await;
                    }
                }
                return redirect
                    .hook
                    .call_with_hints(interface, method, params, results, hints)
                    .await;
            }
            let params = Payload::copy(params.get()?, &wrapper.membrane, !wrapper.reverse)?;
            let failure = Rc::new(RefCell::new(None));
            let results = MembraneResults {
                outer: Some(results),
                payload: Payload::new(),
                membrane: wrapper.membrane.clone(),
                reverse: wrapper.reverse,
                failure: failure.clone(),
                allocation: None,
            };
            let call = wrapper.inner().call_with_hints(
                interface,
                method,
                Box::new(params),
                Box::new(results),
                hints,
            );
            let canceled = wrapper.membrane.when_revoked();
            let result = futures::future::select(call, canceled).await;
            // Drop the losing future before checking errors from Results::drop.
            let outcome = match result {
                futures::future::Either::Left((result, pending)) => {
                    drop(pending);
                    result
                }
                futures::future::Either::Right((result, pending)) => {
                    drop(pending);
                    result
                }
            };
            outcome?;
            let result = failure.borrow_mut().take().map_or(Ok(()), Err);
            result
        })
    }
    fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
        if self.0.custom {
            return Some(self.0.inner());
        }
        if let Some(cap) = self.0.resolved.borrow().as_ref() {
            return Some(cap.clone());
        }
        let cap = self.0.inner().get_resolved()?;
        let cap = self.0.membrane.wrap(cap, self.0.reverse);
        *self.0.resolved.borrow_mut() = Some(cap.clone());
        Some(cap)
    }
    fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, Error>> {
        if self.0.custom {
            return Some(Promise::ok(self.0.inner()));
        }
        if let Some(cap) = self.0.resolved.borrow().as_ref() {
            return Some(Promise::ok(cap.clone()));
        }
        if let Err(error) = self.0.membrane.check() {
            return Some(Promise::err(error));
        }
        let resolving = self.0.inner().when_more_resolved()?;
        let membrane = self.0.membrane.clone();
        let reverse = self.0.reverse;
        let wrapper = self.0.clone();
        Some(Promise::from_future(async move {
            let cap = match futures::future::select(resolving, membrane.when_revoked()).await {
                futures::future::Either::Left((result, _)) => result?,
                futures::future::Either::Right((result, _)) => {
                    result?;
                    unreachable!()
                }
            };
            let cap = membrane.wrap(cap, reverse);
            let result = wrapper.resolved.borrow_mut().get_or_insert(cap).clone();
            Ok(result)
        }))
    }
    fn when_resolved(&self) -> Promise<(), Error> {
        crate::rpc::default_when_resolved_impl(self)
    }
}

struct Payload {
    message: capnp::message::Builder<capnp::message::HeapAllocator>,
    caps: Vec<Option<Box<dyn ClientHook>>>,
}
impl Payload {
    fn new() -> Self {
        Self {
            message: capnp::message::Builder::new_default(),
            caps: vec![],
        }
    }
    fn copy(
        reader: any_pointer::Reader<'_>,
        membrane: &Membrane,
        reverse: bool,
    ) -> capnp::Result<Self> {
        membrane.check()?;
        let mut payload = Self::new();
        payload.builder().set_as(reader)?;
        for cap in &mut payload.caps {
            if let Some(value) = cap.take() {
                *cap = Some(membrane.wrap(value, reverse));
            }
        }
        Ok(payload)
    }
    fn builder(&mut self) -> any_pointer::Builder<'_> {
        let mut root: any_pointer::Builder = self.message.get_root().unwrap();
        root.imbue_mut(&mut self.caps);
        root
    }
}
impl ParamsHook for Payload {
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        let mut root: any_pointer::Reader = self.message.get_root_as_reader()?;
        root.imbue(&self.caps);
        Ok(root)
    }
}
struct MembraneResults {
    outer: Option<Box<dyn ResultsHook>>,
    payload: Payload,
    membrane: Membrane,
    reverse: bool,
    failure: Rc<RefCell<Option<Error>>>,
    allocation: Option<Option<capnp::MessageSize>>,
}
impl MembraneResults {
    fn flush(&mut self) -> capnp::Result<()> {
        let payload = Payload::copy(self.payload.get()?, &self.membrane, self.reverse)?;
        self.outer
            .as_mut()
            .unwrap()
            .get_with_size_hint(self.allocation.flatten())?
            .set_as(payload.get()?)
    }
}
impl Drop for MembraneResults {
    fn drop(&mut self) {
        if self.outer.is_some() {
            if let Err(error) = self.flush() {
                *self.failure.borrow_mut() = Some(error);
            }
        }
    }
}
impl ResultsHook for MembraneResults {
    fn cancellation_guard(&self) -> Option<Box<dyn std::any::Any>> {
        self.outer.as_ref()?.cancellation_guard()
    }
    fn cancellation_executor(&self) -> Option<Rc<dyn capnp::capability::CallExecutor>> {
        self.outer.as_ref()?.cancellation_executor()
    }

    fn get_with_size_hint(
        &mut self,
        size_hint: Option<capnp::MessageSize>,
    ) -> capnp::Result<any_pointer::Builder<'_>> {
        self.membrane.check()?;
        if self.allocation.is_none() {
            self.payload.message =
                capnp::message::Builder::new(crate::local::result_allocator(size_hint));
            self.allocation = Some(size_hint);
        }
        Ok(self.payload.builder())
    }
    fn set_pipeline(&mut self) -> capnp::Result<()> {
        self.flush()?;
        self.outer.as_mut().unwrap().set_pipeline()
    }
    fn set_pipeline_from(&mut self, pipeline: Box<dyn PipelineHook>) -> capnp::Result<()> {
        self.membrane.check()?;
        self.outer
            .as_mut()
            .unwrap()
            .set_pipeline_from(Box::new(MembranePipeline {
                inner: pipeline,
                membrane: self.membrane.clone(),
                reverse: self.reverse,
            }))
    }
    fn allow_cancellation(&self) {
        self.outer.as_ref().unwrap().allow_cancellation();
    }
    fn tail_call(self: Box<Self>, request: Box<dyn RequestHook>) -> Promise<(), Error> {
        self.direct_tail_call(request).0
    }
    fn direct_tail_call(
        mut self: Box<Self>,
        request: Box<dyn RequestHook>,
    ) -> (Promise<(), Error>, Box<dyn PipelineHook>) {
        let request = Box::new(MembraneRequest {
            inner: request,
            membrane: self.membrane.clone(),
            reverse: self.reverse,
        });
        self.outer.take().unwrap().direct_tail_call(request)
    }
}
struct MembranePipeline {
    inner: Box<dyn PipelineHook>,
    membrane: Membrane,
    reverse: bool,
}
impl PipelineHook for MembranePipeline {
    fn add_ref(&self) -> Box<dyn PipelineHook> {
        Box::new(Self {
            inner: self.inner.clone(),
            membrane: self.membrane.clone(),
            reverse: self.reverse,
        })
    }
    fn get_pipelined_cap(
        &self,
        ops: &[capnp::private::capability::PipelineOp],
    ) -> Box<dyn ClientHook> {
        self.membrane
            .wrap(self.inner.get_pipelined_cap(ops), self.reverse)
    }
}

struct MembraneRequest {
    inner: Box<dyn RequestHook>,
    membrane: Membrane,
    reverse: bool,
}
struct MembraneResponse(Payload);
impl ResponseHook for MembraneResponse {
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        self.0.get()
    }
}
impl RequestHook for MembraneRequest {
    fn set_hints(&mut self, hints: CallHints) {
        self.inner.set_hints(hints);
    }
    fn send_for_pipeline(self: Box<Self>) -> any_pointer::Pipeline {
        any_pointer::Pipeline::new(Box::new(MembranePipeline {
            inner: self.inner.send_for_pipeline().hook,
            membrane: self.membrane,
            reverse: self.reverse,
        }))
    }

    fn get(&mut self) -> any_pointer::Builder<'_> {
        self.inner.get()
    }
    fn get_brand(&self) -> usize {
        0
    }
    fn send(self: Box<Self>) -> capnp::capability::RemotePromise<any_pointer::Owned> {
        let Self {
            inner,
            membrane,
            reverse,
        } = *self;
        let remote = inner.send();
        let pipeline = Box::new(MembranePipeline {
            inner: remote.pipeline.hook,
            membrane: membrane.clone(),
            reverse,
        });
        let promise = Promise::from_future(async move {
            let response = remote.promise.await?;
            let payload = Payload::copy(response.get()?, &membrane, reverse)?;
            Ok(capnp::capability::Response::new(Box::new(
                MembraneResponse(payload),
            )))
        });
        capnp::capability::RemotePromise {
            promise,
            pipeline: any_pointer::Pipeline::new(pipeline),
        }
    }
    fn send_streaming(self: Box<Self>) -> Promise<(), Error> {
        let promise = self.inner.send_streaming();
        let revoked = self.membrane.when_revoked();
        Promise::from_future(async move {
            match futures::future::select(promise, revoked).await {
                futures::future::Either::Left((result, _)) => result,
                futures::future::Either::Right((result, _)) => result,
            }
        })
    }
    fn tail_send(self: Box<Self>) -> Option<(u32, Promise<(), Error>, Box<dyn PipelineHook>)> {
        None
    }
}
