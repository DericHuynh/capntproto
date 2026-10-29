//! Owned hook calls admitted to the portable wire driver. The caller retains
//! this adapter in the same scope as the driver and polls it alongside IO,
//! protocol maintenance and application execution.
use super::*;
use capnp::private::capability::{PipelineHook, PipelineOp};
use capnp::traits::{Imbue, ImbueMut};
use capntproto_rpc_engine::messages;
use capntproto_rpc_wire::{
    calls::DynamicCall,
    owned::{
        self,
        connection::{self, CallOutput, Driver},
    },
    pipeline,
};

mod calls;
pub mod coordinator;
pub mod foreign;
mod projections;
pub(crate) mod ready;
pub mod resources;
pub mod server;
pub use calls::{Calls, Progress};

/// Rejected admission preserves the original call and its completion owner.
#[must_use]
pub struct Rejected {
    pub error: Error,
    pub call: Call,
}
pub(super) struct RejectedOwner<T> {
    error: Error,
    call: T,
    output_blocked: bool,
}

/// An owned message whose cap table is independent of scoped wire receipts.
pub(crate) struct Payload {
    body: PayloadBody,
    caps: Vec<Option<Box<dyn ClientHook>>>,
}
enum PayloadBody {
    Owned(Box<capnp::message::Builder<capnp::message::HeapAllocator>>),
    Return(Rc<connection::FrozenReturn<capnp::message::HeapAllocator>>),
    Produced(server::Results),
    Admitted(Box<owned::PlainResults>),
    Shared { message: SharedPayload },
}
struct SharedPayload {
    message: capnp::message::CachedPointerReader<
        capnp::serialize::NoAllocBufferSegments<messages::SharedBytes>,
    >,
}
impl capnp::private::capability::ResponseHook for SharedPayload {
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        Ok(self.message.root())
    }
}
impl Payload {
    fn prepared_return(&self) -> Option<&connection::FrozenReturn<capnp::message::HeapAllocator>> {
        match &self.body {
            PayloadBody::Return(message) => Some(message),
            PayloadBody::Produced(results) => results.prepared_return(),
            _ => None,
        }
    }
    pub(crate) fn retain_plain_context(
        &mut self,
        context: Rc<dyn std::any::Any>,
    ) -> Result<(), Rc<dyn std::any::Any>> {
        if let PayloadBody::Admitted(message) = &mut self.body {
            debug_assert!(self.caps.is_empty());
            message.retain_owner(context);
            Ok(())
        } else {
            Err(context)
        }
    }
    pub(crate) fn into_response(self) -> Box<dyn capnp::private::capability::ResponseHook> {
        match self.body {
            PayloadBody::Admitted(message) => {
                debug_assert!(self.caps.is_empty());
                message
            }
            PayloadBody::Shared { message } if self.caps.is_empty() => {
                debug_assert!(self.caps.is_empty());
                Box::new(message)
            }
            _ => Box::new(self),
        }
    }
    fn owned(
        message: capnp::message::Builder<capnp::message::HeapAllocator>,
        caps: Vec<Option<Box<dyn ClientHook>>>,
    ) -> Self {
        Self {
            body: PayloadBody::Owned(Box::new(message)),
            caps,
        }
    }
    pub(super) fn copy(reader: any_pointer::Reader<'_>) -> capnp::Result<Self> {
        let mut message = capnp::message::Builder::new(crate::local::result_allocator(Some(
            reader.target_size()?,
        )));
        let mut caps = Vec::new();
        let mut root: any_pointer::Builder<'_> = message.init_root();
        root.imbue_mut(&mut caps);
        root.set_as(reader)?;
        Ok(Self::owned(message, caps))
    }
    fn retain_or_copy(
        reader: any_pointer::Reader<'_>,
        retained: Option<
            capnp::message::CachedPointerReader<
                capnp::serialize::NoAllocBufferSegments<messages::SharedBytes>,
            >,
        >,
        caps: Vec<Option<Box<dyn ClientHook>>>,
    ) -> capnp::Result<Self> {
        // The wire owner selects content from its original arena. Bound hooks
        // keep the exact admitted descriptor order; reads remain lazy and share
        // the allowance already consumed by protocol decoding.
        if let Some(message) = retained {
            return Ok(Self {
                body: PayloadBody::Shared {
                    message: SharedPayload { message },
                },
                caps,
            });
        }
        let mut reader = reader;
        reader.imbue(&caps);
        Self::copy(reader)
    }
    fn reader(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        let mut root: any_pointer::Reader<'_> = match &self.body {
            PayloadBody::Owned(message) => message.get_root_as_reader()?,
            PayloadBody::Return(message) => message.reader()?,
            PayloadBody::Produced(results) => results.payload_reader()?,
            PayloadBody::Admitted(message) => {
                capnp::private::capability::ResponseHook::get(&**message)?
            }
            PayloadBody::Shared { message } => {
                capnp::private::capability::ResponseHook::get(message)?
            }
        };
        root.imbue(&self.caps);
        Ok(root)
    }
    fn into_pipeline(self) -> Box<dyn PipelineHook> {
        Box::new(PayloadPipeline(Rc::new(self)))
    }
}
impl capnp::private::capability::ResponseHook for Payload {
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        self.reader()
    }
}
// A completed wire payload is immutable. Its pipeline can retain it directly;
// there is no mutable local result state to synchronize with another owner.
pub(super) struct PayloadPipeline(pub(super) Rc<Payload>);
impl PipelineHook for PayloadPipeline {
    fn add_ref(&self) -> Box<dyn PipelineHook> {
        Box::new(Self(self.0.clone()))
    }
    fn get_pipelined_cap(&self, ops: &[PipelineOp]) -> Box<dyn ClientHook> {
        match self
            .0
            .reader()
            .and_then(|reader| reader.get_pipelined_cap(ops))
        {
            Ok(client) => client,
            Err(error) => Box::new(crate::broken::Client::new(error, true, 0)),
        }
    }
}

#[cold]
#[inline(never)]
fn wire_error(error: impl std::fmt::Display) -> Error {
    Error::failed(error.to_string())
}

impl<'id> ClientHooks<'id> {
    /// Admit an owned call to the wire driver's ordered invocation queue.
    /// Foreign capabilities are handed to the caller's policy-preserving import
    /// factory. No hook pointer or public brand is interpreted as authority.
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub fn enqueue_wire<
        'pool,
        'storage,
        I,
        A,
        P: Copy + Eq,
        S,
        const CAPS: usize,
        const PATH: usize,
    >(
        &mut self,
        call: Call,
        driver: &mut Driver<'pool, 'id, 'storage, I, A, P, S, CAPS, PATH>,
        invocations: &mut connection::Invocations<'_, '_, 'pool, 'id, I, CAPS>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        options: capnp::message::ReaderOptions,
        mut import: impl FnMut(
            &mut capntproto_rpc_wire::owned::connection::Scope<
                '_,
                'pool,
                'id,
                'storage,
                I,
                P,
                S,
                CAPS,
                PATH,
            >,
            &dyn ClientHook,
        ) -> capnp::Result<Client<'id>>,
    ) -> Result<WireCall<'pool, 'id, I, CAPS>, Rejected> {
        self.enqueue_wire_using(
            call,
            driver,
            invocations,
            messages,
            options,
            false,
            &mut import,
        )
        .map_err(|rejected| Rejected {
            error: rejected.error,
            call: rejected.call,
        })
    }
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    #[inline(never)]
    pub(super) fn enqueue_wire_using<
        'pool,
        'storage,
        B: core::borrow::BorrowMut<Call> + Into<Box<Call>>,
        I,
        A,
        P: Copy + Eq,
        S,
        const CAPS: usize,
        const PATH: usize,
    >(
        &mut self,
        mut owner: B,
        driver: &mut Driver<'pool, 'id, 'storage, I, A, P, S, CAPS, PATH>,
        invocations: &mut connection::Invocations<'_, '_, 'pool, 'id, I, CAPS>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        options: capnp::message::ReaderOptions,
        direct: bool,
        mut import: impl FnMut(
            &mut capntproto_rpc_wire::owned::connection::Scope<
                '_,
                'pool,
                'id,
                'storage,
                I,
                P,
                S,
                CAPS,
                PATH,
            >,
            &dyn ClientHook,
        ) -> capnp::Result<Client<'id>>,
    ) -> Result<WireCall<'pool, 'id, I, CAPS>, RejectedOwner<B>> {
        // The hosted inbox already owns a Box<Call>. Keep that same allocation
        // through admission, retries and completion; the public by-value API
        // boxes only after successful admission.
        let call = owner.borrow_mut();
        let mut receiver = None;
        let mut direct_route = false;
        let mut output_blocked = false;
        let outcome = (|| {
            let target = self.target(call)?;
            // Remote completion needs no second channel or polling ledger.
            // Keep unresolved calls direct too: Calls binds a completion owner
            // before accepting one locally. Both paths retain the same queued
            // task, cancellation and pipeline identities.
            let direct = direct && call.third_party.is_none();
            let params = call.params()?;
            let size = params.target_size()?;
            // Like legacy write_descriptors, use a private builder's existing
            // table and preserve its indices, including null and repeated slots.
            // Every original hook still passes through ordinary import/membrane
            // handling. Opaque parameters need collection; a table exceeding the
            // wire budget also needs collection to compact unreachable slots.
            let direct_caps = (size.cap_count != 0)
                .then(|| call.params.capability_table())
                .flatten()
                .filter(|caps| caps.len() <= driver.capability_capacity());
            // Data-only parameters already have the identity wire cap mapping.
            // Keep their caller-owned reader through the transactional copy
            // instead of allocating and copying a second hosted message first.
            let payload = if size.cap_count == 0 || direct_caps.is_some() {
                None
            } else {
                Some(Payload::copy(params)?)
            };
            let caps = direct_caps
                .unwrap_or_else(|| payload.as_ref().map_or(&[][..], |payload| &payload.caps));
            if caps.len() > driver.capability_capacity() {
                return Err(Error::overloaded(
                    "wire capability capacity exhausted".into(),
                ));
            }
            let mut retained: Vec<Option<Client<'id>>> = Vec::new();
            retained.try_reserve_exact(caps.len()).map_err(wire_error)?;
            retained.resize_with(caps.len(), || None);
            let mut refs = Vec::new();
            refs.try_reserve_exact(caps.len()).map_err(wire_error)?;
            let result = (|| {
                let mut refs = refs;
                for (slot, cap) in retained.iter_mut().zip(caps) {
                    if let Some(cap) = cap {
                        *slot =
                            Some(match self.try_retain(&mut driver.capabilities(), &**cap)? {
                                Some(client) => client,
                                None => import(&mut driver.capabilities(), &**cap)?,
                            });
                    }
                }
                let count = caps.len();
                refs.extend(retained.iter().map(Option::as_ref));
                let mapping = capnp::wire_capability::IndexMap::identity(
                    u32::try_from(count).map_err(wire_error)?,
                );
                let content = match &payload {
                    Some(payload) => payload.reader()?,
                    None => params,
                };
                let minimum = size
                    .word_count
                    .checked_add(16)
                    .and_then(|words| words.checked_mul(8))
                    .and_then(|bytes| usize::try_from(bytes).ok())
                    .ok_or_else(|| Error::from_kind(capnp::ErrorKind::MessageSizeOverflow))?;
                let third_party = call
                    .third_party
                    .as_ref()
                    .map(|body| body.get_root_as_reader::<any_pointer::Reader>())
                    .transpose()?;
                let extra = third_party
                    // Metadata copy uses a transactional staging pointer in
                    // addition to the reachable destination words.
                    .map(|reader| reader.target_size().map(|size| size.word_count + 1))
                    .transpose()?
                    .unwrap_or(0);
                let destination = if call.native_tail.is_some() {
                    capntproto_rpc_wire::calls::CallDestination::Yourself
                } else {
                    third_party.map_or(
                        capntproto_rpc_wire::calls::CallDestination::Caller,
                        capntproto_rpc_wire::calls::CallDestination::ThirdParty,
                    )
                };
                let minimum = minimum
                    .checked_add(
                        usize::try_from(extra)
                            .map_err(wire_error)?
                            .checked_mul(8)
                            .ok_or_else(|| {
                                Error::from_kind(capnp::ErrorKind::MessageSizeOverflow)
                            })?,
                    )
                    .ok_or_else(|| Error::from_kind(capnp::ErrorKind::MessageSizeOverflow))?;
                let method = DynamicCall {
                    interface: call.interface,
                    method: call.method,
                    no_promise_pipelining: call.hints.no_promise_pipelining,
                    only_promise_pipeline: call.hints.only_promise_pipeline,
                };
                if direct {
                    let task = driver
                        .enqueue_dynamic_to(
                            target,
                            method,
                            &refs[..count],
                            messages,
                            minimum,
                            options,
                            destination,
                            |field| field.copy_with_capabilities(content, mapping),
                        )
                        .map_err(|error| {
                            output_blocked = calls::output_capacity_error(&error);
                            wire_error(error)
                        })?;
                    direct_route = true;
                    return Ok(task);
                }
                let (sender, receiving) = shared::channel();
                receiver = Some(receiving);
                invocations
                    .enqueue_dynamic_to(
                        driver,
                        sender.into(),
                        target,
                        method,
                        &refs[..count],
                        messages,
                        minimum,
                        options,
                        destination,
                        |field| field.copy_with_capabilities(content, mapping),
                    )
                    .map_err(|rejected| {
                        output_blocked = calls::output_capacity_error(&rejected.error);
                        wire_error(format!("queued invocation: {:?}", rejected.error))
                    })
            })();
            for client in retained.into_iter().flatten() {
                driver.capabilities().release(client);
            }
            result
        })();
        let task = match outcome {
            Ok(task) => task,
            Err(error) => {
                return Err(RejectedOwner {
                    error,
                    call: owner,
                    output_blocked,
                })
            }
        };
        let pipeline = match self.attach_pipeline(call) {
            Ok(pipeline) => pipeline,
            Err(error) => {
                // Direct owners have no invocation binding whose canceled
                // receiver would reclaim this unpublished task.
                if direct_route {
                    driver.cancel_invocation(task, messages);
                }
                return Err(RejectedOwner {
                    error,
                    call: owner,
                    output_blocked: false,
                });
            }
        };
        Ok(WireCall {
            call: Some(owner.into()),
            route: if direct_route {
                CallRoute::Queued(task)
            } else {
                CallRoute::Tracked {
                    task,
                    receiver: receiver.expect("tracked invocation receiver"),
                }
            },
            pipeline,
            projections: Default::default(),
            local_results: None,
        })
    }
    fn attach_pipeline(&self, call: &mut Call) -> capnp::Result<Option<Rc<PipelineState>>> {
        if call.hints.no_promise_pipelining {
            return Ok(None);
        }
        if let Some(pipeline) = call.results.take_wire_pipeline() {
            if let Some(pipeline) = &pipeline {
                pipeline._credit.set(call.credit.clone());
            }
            return Ok(pipeline);
        }
        let pipeline = (!call.hints.no_promise_pipelining).then(|| {
            Rc::new(PipelineState {
                used: Cell::new(false),
                owner_wake: RefCell::new(None),
                operation: Cell::new(None),
                materialize: Cell::new(false),
                variant: RefCell::new(PipelineVariant::Waiting),
                cached: RefCell::default(),
                _credit: Cell::new(call.credit.clone()),
                waiting: RefCell::new(VecDeque::new()),
                inbox: Rc::downgrade(&self.inbox),
                maximum: self.maximum_clients,
            })
        });
        if let Some(pipeline) = &pipeline {
            let published = Box::new(WirePipeline(PipelineRef(Some(pipeline.clone()), None)));
            let results = call.results.hook();
            if let Some(local) = results
                .as_any_mut()
                .and_then(|hook| hook.downcast_mut::<crate::local::Results>())
            {
                local.adopt_pipeline_clients(published, |client, path| {
                    pipeline.used.set(true);
                    let mut waiting = pipeline.waiting.borrow_mut();
                    if waiting.len() >= pipeline.maximum || waiting.try_reserve(1).is_err() {
                        drop(waiting);
                        crate::queued::ClientInner::resolve(
                            client,
                            Err(Error::overloaded("wire pipeline capacity exhausted".into())),
                        );
                    } else {
                        waiting.push_back(Projection {
                            path,
                            client: ProjectionClient::Queued(Rc::downgrade(client)),
                            retain_pipeline: true,
                        });
                    }
                })?;
            } else {
                results.set_pipeline_from(published)?;
            }
        }
        Ok(pipeline)
    }
}

/// One admitted request. Poll projections and completion alongside Invocations.
/// Dropping it cancels its tracked reply; Invocations retains wire cleanup.
#[must_use = "drive the call to completion or cancel its tracked reply"]
pub struct WireCall<'pool, 'id, I, const CAPS: usize> {
    call: Option<Box<Call>>,
    route: CallRoute<'pool, 'id, I, CAPS>,
    pipeline: Option<Rc<PipelineState>>,
    projections: projections::DirectProjections<'id>,
    // Local result publication can precede final completion and survives the
    // transition out of Tracked. It is an application context, not a wire route.
    local_results: Option<server::Results>,
}
enum CallRoute<'pool, 'id, I, const CAPS: usize> {
    Queued(connection::InvocationKey<'id>),
    Tracked {
        task: connection::InvocationKey<'id>,
        receiver: shared::Receiver<connection::CallResult<'pool, 'id, I, CAPS>>,
    },
    Remote(capntproto_rpc_engine::questions::Key<'id>),
    Tail(Box<connection::Response<'pool, 'id, I, CAPS>>),
    Finished,
}
impl<'pool, 'id, I, const CAPS: usize> WireCall<'pool, 'id, I, CAPS> {
    /// The pre-publication or tracked-local identity, if that route still owns it.
    pub fn task(&self) -> Option<connection::InvocationKey<'id>> {
        match self.route {
            CallRoute::Queued(task) | CallRoute::Tracked { task, .. } => Some(task),
            CallRoute::Remote(_) | CallRoute::Tail(_) | CallRoute::Finished => None,
        }
    }
    fn question(&self) -> Option<capntproto_rpc_engine::questions::Key<'id>> {
        match self.route {
            CallRoute::Remote(question) => Some(question),
            _ => None,
        }
    }
    fn direct(&self) -> bool {
        matches!(self.route, CallRoute::Queued(_) | CallRoute::Remote(_))
    }
    fn close_receiver(&mut self) {
        if matches!(self.route, CallRoute::Tracked { .. }) {
            self.route = CallRoute::Finished;
        }
    }
    /// Queue pending projections while the parent is still awaiting Return.
    /// Projection requests use private owned identities; only this method can
    /// attach them to a branded invocation/question in the driver.
    pub fn poll_pipelines<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        hooks: &mut ClientHooks<'id>,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        invocations: &mut connection::Invocations<'_, '_, 'pool, 'id, I, CAPS>,
        messages: &messages::Messages<'pool, '_, '_>,
    ) -> capnp::Result<()> {
        self.poll_pipelines_with(hooks, driver, invocations, messages, |_, _, _, _| Ok(None))
    }
    /// Caller-owned local producers may supply early pipelines before their
    /// final result. Use the same resolver when polling invocation bindings.
    pub fn poll_pipelines_with<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        hooks: &mut ClientHooks<'id>,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        invocations: &mut connection::Invocations<'_, '_, 'pool, 'id, I, CAPS>,
        messages: &messages::Messages<'pool, '_, '_>,
        resolve: impl FnMut(
            &mut ClientHooks<'id>,
            connection::InvocationKey<'id>,
            pipeline::Path<'_>,
            &mut capntproto_rpc_wire::owned::connection::Scope<
                '_,
                'pool,
                'id,
                '_,
                I,
                P,
                S,
                CAPS,
                PATH,
            >,
        ) -> Result<Option<Client<'id>>, connection::Error>,
    ) -> capnp::Result<()> {
        self.poll_pipelines_using(hooks, driver, invocations, messages, resolve, |_, _| {})
    }
    pub(crate) fn poll_pipelines_using<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        hooks: &mut ClientHooks<'id>,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        invocations: &mut connection::Invocations<'_, '_, 'pool, 'id, I, CAPS>,
        messages: &messages::Messages<'pool, '_, '_>,
        mut resolve: impl FnMut(
            &mut ClientHooks<'id>,
            connection::InvocationKey<'id>,
            pipeline::Path<'_>,
            &mut capntproto_rpc_wire::owned::connection::Scope<
                '_,
                'pool,
                'id,
                '_,
                I,
                P,
                S,
                CAPS,
                PATH,
            >,
        ) -> Result<Option<Client<'id>>, connection::Error>,
        mut refresh: impl FnMut(
            &mut ClientHooks<'id>,
            &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        ),
    ) -> capnp::Result<()> {
        let Some(pipeline) = &self.pipeline else {
            return Ok(());
        };
        if pipeline.materialize.replace(false) {
            self.projections
                .materialize(hooks, &mut driver.capabilities())?;
            refresh(hooks, driver);
        }
        let budget = pipeline.waiting.borrow().len();
        for _ in 0..budget {
            let Some(projection) = pipeline.waiting.borrow_mut().pop_front() else {
                break;
            };
            let Some(client) = projection.client.upgrade() else {
                continue;
            };
            let field_count = pipeline_fields(&projection.path).count();
            if field_count > driver.path_capacity() {
                client.resolve(Err(Error::overloaded(
                    "wire pipeline path capacity exhausted".into(),
                )));
                continue;
            }
            // Keep short paths on the stack. The original operation vector
            // moves into the cache after routing instead of allocating a second
            // owned path for every projected capability.
            let mut inline_fields = [0; 8];
            let mut long_fields = Vec::new();
            let fields = if field_count <= inline_fields.len() {
                for (slot, field) in inline_fields
                    .iter_mut()
                    .zip(pipeline_fields(&projection.path))
                {
                    *slot = field;
                }
                &inline_fields[..field_count]
            } else {
                long_fields.extend(pipeline_fields(&projection.path));
                &long_fields
            };
            let path = pipeline::Path::new(fields);
            let routed = match &self.route {
                CallRoute::Tail(response) => driver.tail_pipeline(response, path, messages),
                CallRoute::Queued(task) => driver.invocation_pipeline(*task, path, messages),
                CallRoute::Remote(question) => driver.pipeline(*question, path),
                CallRoute::Tracked { task, .. } => invocations.pipeline_with(
                    driver,
                    *task,
                    path,
                    messages,
                    |task, path, runtime| resolve(hooks, task, path, runtime),
                ),
                CallRoute::Finished => {
                    return Err(Error::failed(
                        "finished operation retained a pending projection".into(),
                    ))
                }
            };
            let cap = match routed {
                Ok(cap) => cap,
                Err(error) => {
                    pipeline.waiting.borrow_mut().push_front(projection);
                    return if matches!(error, connection::Error::PendingResponse) {
                        Ok(())
                    } else {
                        Err(wire_error(error))
                    };
                }
            };
            match client {
                ProjectionTarget::Direct(endpoint) => {
                    // The deferred direct-route adapter expects Return to
                    // publish resolution. A no-Return question instead binds
                    // its already-settled route immediately, as legacy does.
                    if matches!(self.route, CallRoute::Remote(question) if !question.id().is_pipeline_only())
                        && pipeline.operation.get().is_some()
                    {
                        match self.projections.insert(endpoint.clone(), cap, hooks) {
                            Ok(index) => {
                                *endpoint.direct_parent.borrow_mut() = Rc::downgrade(pipeline);
                                endpoint.direct.set(Some(index));
                                pipeline
                                    .remember(projection.path, &Hook(endpoint.clone(), None))?;
                                endpoint.notify_binding();
                            }
                            Err(cap) => {
                                driver.capabilities().release(cap);
                                endpoint.forward_projection(Err(Error::overloaded(
                                    "direct pipeline storage exhausted".into(),
                                )));
                            }
                        }
                        continue;
                    }
                    let result =
                        hooks.bind_projection(&mut driver.capabilities(), cap, endpoint.clone());
                    refresh(hooks, driver);
                    match result {
                        Ok(hook) => pipeline.remember(projection.path, &*hook)?,
                        Err(error) => endpoint.forward_projection(Err(error)),
                    }
                }
                ProjectionTarget::Queued(client) => {
                    let result = hooks.import_client(&mut driver.capabilities(), cap);
                    refresh(hooks, driver);
                    let result = result.map(|client| client.hook);
                    if let Ok(hook) = &result {
                        pipeline.remember(projection.path, &**hook)?;
                    }
                    let result = result.map(|inner| {
                        if projection.retain_pipeline {
                            Rc::new(Projected {
                                inner,
                                pipeline: PipelineRef(Some(pipeline.clone()), None),
                            }) as Rc<dyn ClientHook>
                        } else {
                            Rc::from(inner)
                        }
                    });
                    crate::queued::ClientInner::resolve_shared(&client, result);
                }
            }
        }
        Ok(())
    }
    pub fn poll_canceled(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        let Some(call) = self.call.as_mut() else {
            return Poll::Ready(());
        };
        if call.shutdown.is_closed() {
            return Poll::Ready(());
        }
        let canceled = call.poll_canceled(cx);
        if canceled.is_ready()
            && self
                .pipeline
                .as_ref()
                .is_none_or(|pipeline| Rc::strong_count(pipeline) == 1)
        {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
    /// Cancellation closes the tracked receiver. Continue polling Invocations
    /// so its original request and any late Return are cleaned up exactly once.
    pub fn cancel(mut self) {
        self.close_receiver();
        self.fail(Error::disconnected("wire call canceled".into()));
    }
    pub(crate) fn cancel_with_error(mut self, error: Error) {
        self.close_receiver();
        self.fail(error);
    }
    fn fail(&mut self, error: Error) {
        if let Some(call) = self.call.take() {
            let error = self
                .local_results
                .as_ref()
                .and_then(server::Results::failure)
                .unwrap_or(error);
            if let Some(pipeline) = &self.pipeline {
                match self
                    .local_results
                    .as_ref()
                    .and_then(server::Results::early_pipeline)
                {
                    Some(early) => pipeline.finish(early),
                    None => pipeline.resolve(PipelineVariant::Broken(error.clone())),
                }
            }
            call.complete_boxed(Err(error));
        }
    }
    /// Publish a completed local/remote output to owned generated clients, then
    /// release its wire receipts. Returned capability hooks retain graph clients
    /// independently, including when the response itself is immediately dropped.
    pub fn poll<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        hooks: &mut ClientHooks<'id>,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        cx: &mut Context<'_>,
    ) -> Poll<capnp::Result<()>> {
        self.poll_using(hooks, driver, messages, cx, false, |_, _| {}, |_| None)
    }
    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    pub(crate) fn poll_using<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        hooks: &mut ClientHooks<'id>,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        cx: &mut Context<'_>,
        inline_completion: bool,
        refresh: impl FnMut(&mut ClientHooks<'id>, &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>),
        local_hook: impl FnMut(&S) -> Option<Box<dyn ClientHook>>,
    ) -> Poll<capnp::Result<()>> {
        if self.call.is_none() {
            if matches!(self.route, CallRoute::Tail(_)) {
                if self
                    .pipeline
                    .as_ref()
                    .is_some_and(|pipeline| Rc::strong_count(pipeline) > 1)
                {
                    return Poll::Pending;
                }
                self.release_tail(driver, messages);
            }
            return Poll::Ready(Ok(()));
        }
        let canceled = if inline_completion {
            // The hosted coordinator owns the inbox wake and polls connection
            // shutdown before every IO/work turn. Keep per-call cancellation
            // observation, without also registering one broadcast waiter per
            // active call. Standalone WireCall polling keeps its own waiter.
            let call = self.call.as_mut().expect("active call");
            call.is_canceled() || call.completion.poll_observed(cx).is_ready()
        } else {
            self.poll_canceled(cx).is_ready()
        };
        if canceled {
            self.cancel_direct(driver, messages)?;
            self.close_receiver();
            self.fail(Error::disconnected("wire call canceled".into()));
            return Poll::Ready(Ok(()));
        }
        if let CallRoute::Remote(question) = self.route {
            let result = match driver.take_output(question, messages) {
                Ok(result) => result,
                Err(
                    connection::Error::PendingResponse | connection::Error::PendingAuthorization,
                ) => return Poll::Pending,
                Err(error) => return Poll::Ready(Err(wire_error(error))),
            };
            self.route = CallRoute::Finished;
            let result = self.complete(result, hooks, driver, messages, refresh, local_hook);
            if result.is_ok() && matches!(self.route, CallRoute::Tail(_)) {
                // An acknowledgement retains the native question until its
                // last pipeline interest drops, just like tracked transfers.
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            return Poll::Ready(result);
        }
        if matches!(self.route, CallRoute::Queued(_)) {
            return Poll::Pending;
        }
        let CallRoute::Tracked { receiver, .. } = &mut self.route else {
            unreachable!("an active call owns a queued, remote, or tracked route")
        };
        let output = match Pin::new(receiver).poll(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(output)) => output,
            Poll::Ready(Err(error)) => {
                self.fail(crate::canceled_to_error(error));
                return Poll::Ready(Ok(()));
            }
        };
        self.close_receiver();
        let result = self.complete(output, hooks, driver, messages, refresh, local_hook);
        if result.is_ok() && matches!(self.route, CallRoute::Tail(_)) {
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(result)
        }
    }
    fn cancel_direct<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> capnp::Result<()> {
        if !self.projections.is_empty() {
            self.projections.release(
                &mut driver.capabilities(),
                &Error::disconnected("wire call canceled".into()),
            );
        }
        match self.route {
            CallRoute::Queued(task) => {
                if !driver.cancel_invocation(task, messages) {
                    return Err(wire_error(connection::Error::UnknownInvocation));
                }
                self.route = CallRoute::Finished;
            }
            CallRoute::Remote(question) => {
                driver.cancel(question).map_err(wire_error)?;
                self.route = CallRoute::Finished;
            }
            _ => (),
        }
        Ok(())
    }
    pub(crate) fn release_tail<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) {
        if matches!(self.route, CallRoute::Tail(_)) {
            let CallRoute::Tail(response) = std::mem::replace(&mut self.route, CallRoute::Finished)
            else {
                unreachable!()
            };
            driver.release_output(CallOutput::Remote(*response), messages);
        }
    }
    pub(super) fn accepts_plain(&self, key: capntproto_rpc_engine::questions::Key<'id>) -> bool {
        self.task().is_none()
            && self.pipeline.is_none()
            && self.call.as_ref().is_some_and(|call| {
                call.third_party.is_none()
                    && call.native_tail.is_none()
                    && self.question() == Some(key)
                    && !call.is_canceled()
            })
    }
    pub(super) fn deliver_plain(&mut self, message: Box<capntproto_rpc_wire::owned::PlainResults>) {
        self.route = CallRoute::Finished;
        let mut call = self.call.take().expect("wire completion once");
        if let Some(credit) = &call.credit {
            credit.received();
        }
        if let Err(message) = call.results.complete_plain(&call.completion, message) {
            let result = call.results.finish_plain(Payload {
                body: PayloadBody::Admitted(message),
                caps: Vec::new(),
            });
            call.complete_boxed(result);
        }
    }
    #[inline(always)]
    pub(super) fn complete_plain<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        key: capntproto_rpc_engine::questions::Key<'id>,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) -> capnp::Result<bool> {
        let Some(call) = &self.call else {
            return Ok(false);
        };
        if self.pipeline.is_some()
            || call.third_party.is_some()
            || call.native_tail.is_some()
            || self.question() != Some(key)
            || call.is_canceled()
        {
            return Ok(false);
        }
        let message = match driver.take_plain_output(key, messages) {
            Ok(Some(message)) => message,
            Ok(None)
            | Err(connection::Error::PendingResponse | connection::Error::PendingAuthorization) => {
                return Ok(false)
            }
            Err(error) => return Err(wire_error(error)),
        };
        self.deliver_plain(message);
        Ok(true)
    }

    #[inline(always)]
    fn complete<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        mut output: connection::CallResult<'pool, 'id, I, CAPS>,
        hooks: &mut ClientHooks<'id>,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        mut refresh: impl FnMut(
            &mut ClientHooks<'id>,
            &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        ),
        local_hook: impl FnMut(&S) -> Option<Box<dyn ClientHook>>,
    ) -> capnp::Result<()> {
        self.projections
            .materialize(hooks, &mut driver.capabilities())?;
        // The admitted reader already owns the immutable, capability-free
        // payload. Transfer it straight to the result sink; preserve the
        // same credit, metadata refresh and wire cleanup as general completion.
        if self.pipeline.is_none()
            && self
                .call
                .as_ref()
                .is_some_and(|call| call.third_party.is_none())
        {
            if let Ok(CallOutput::Remote(remote)) = &mut output {
                if let Some(message) = remote.take_plain_results() {
                    let call = self.call.as_ref().expect("active call");
                    if let Some(credit) = &call.credit {
                        credit.received();
                    }
                    refresh(hooks, driver);
                    driver.release_output(output.expect("remote output"), messages);
                    let mut call = self.call.take().expect("wire completion once");
                    let result = call.results.finish_plain(Payload {
                        body: PayloadBody::Admitted(message),
                        caps: Vec::new(),
                    });
                    call.complete_boxed(result);
                    return Ok(());
                }
            }
        }
        self.complete_general(output, hooks, driver, messages, refresh, local_hook)
    }

    #[inline(never)]
    fn complete_general<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        output: connection::CallResult<'pool, 'id, I, CAPS>,
        hooks: &mut ClientHooks<'id>,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        mut refresh: impl FnMut(
            &mut ClientHooks<'id>,
            &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        ),
        mut local_hook: impl FnMut(&S) -> Option<Box<dyn ClientHook>>,
    ) -> capnp::Result<()> {
        let mut output = match output {
            Err(kind) => {
                self.fail(Error::from_kind(kind));
                return Ok(());
            }
            Ok(output) => output,
        };
        if matches!(&output, CallOutput::Remote(_)) {
            if let Some(credit) = self.call.as_ref().and_then(|call| call.credit.as_ref()) {
                credit.received();
            }
        }
        if self
            .call
            .as_ref()
            .is_some_and(|call| call.third_party.is_some() || call.native_tail.is_some())
        {
            if let CallOutput::Remote(response) = &output {
                if response
                    .with(messages, |reply| {
                        matches!(reply, owned::Reply::ResultsSentElsewhere)
                    })
                    .map_err(wire_error)?
                {
                    let CallOutput::Remote(response) = output else {
                        unreachable!()
                    };
                    self.route = CallRoute::Tail(Box::new(response));
                    let call = self.call.take().expect("active tail call");
                    if let CallCompletion::Response(completion) = &call.completion {
                        completion.tail_acknowledged();
                    }
                    call.complete(Ok(()));
                    return Ok(());
                }
            }
        }
        let result = match &mut output {
            CallOutput::Local(local) => local
                .with(messages, |payload| {
                    let caps = payload
                        .capabilities()
                        .iter()
                        .map(|cap| {
                            cap.as_ref()
                                .map(|cap| {
                                    // An authenticated cross-connection result
                                    // already owns its hosted proxy. Preserve
                                    // that owner instead of making its lifetime
                                    // depend on the introducer's driver scope.
                                    if driver.runtime().is_settled(cap).unwrap_or(false) {
                                        if let capntproto_rpc_engine::clients::Destination::Endpoint {
                                            owner: capntproto_rpc_wire::clients::Route::Capability(capntproto_rpc_wire::clients::Endpoint::Local(hook)),
                                            ..
                                        } = driver.runtime().inspect(cap).map_err(wire_error)? {
                                            if let Some(hook) = local_hook(hook) { return Ok(hook); }
                                        }
                                    }
                                    hooks
                                        .export_before_wire_refresh(&mut driver.capabilities(), cap)
                                        .map(|c| c.hook)
                                })
                                .transpose()
                        })
                        .collect::<capnp::Result<Vec<_>>>()?;
                    let mut content = payload.content();
                    content.imbue(&caps);
                    Payload::copy(content)
                })
                .map_err(wire_error)
                .and_then(core::convert::identity),
            CallOutput::Remote(remote) => {
                if let Some(message) = remote.take_plain_results() {
                    Ok(Payload { body: PayloadBody::Admitted(message), caps: Vec::new() })
                } else { remote
                .with(messages, |reply| match reply {
                    owned::Reply::Results(payload) => {
                        hooks.collect(&mut driver.capabilities());
                        let mut caps = Vec::with_capacity(payload.capability_count());
                        for index in 0..payload.capability_count() {
                            let cap = payload.capability_at(index as u32).map_err(wire_error)?;
                            caps.push(match cap {
                                None => None,
                                Some(cap) => {
                                    let cap = driver.bind_capability(cap).map_err(wire_error)?.into_client();
                                    Some(hooks.import_wire_capability(driver, cap, &mut local_hook)?)
                                }
                            });
                        }
                        Payload::retain_or_copy(payload.content(), remote.retain_results(messages, payload.content()).map_err(wire_error)?, caps)
                    }
                    owned::Reply::Exception(error) => {
                        Err(crate::rpc::remote_exception_to_error(error))
                    }
                    _ => Err(Error::failed(
                        "wire output is not a completed response".into(),
                    )),
                })
                .map_err(wire_error)
                .and_then(core::convert::identity) }
            },
        };
        refresh(hooks, driver);
        driver.release_output(output, messages);
        match result {
            Ok(payload) => {
                let call = self.call.as_mut().expect("wire completion once");
                let payload = match call.results.complete_payload(
                    &call.completion,
                    payload,
                    self.pipeline.as_ref(),
                    || {
                        self.local_results
                            .as_ref()
                            .and_then(server::Results::early_pipeline)
                    },
                ) {
                    Ok(()) => {
                        self.call.take();
                        return Ok(());
                    }
                    Err(payload) => payload,
                };
                let payload = if self.pipeline.is_none() {
                    let results = &mut self.call.as_mut().expect("wire completion once").results;
                    match results.finish_admitted(payload) {
                        Ok(()) => {
                            self.call.take().unwrap().complete(Ok(()));
                            return Ok(());
                        }
                        Err(payload) => payload,
                    }
                } else {
                    payload
                };
                let payload = Rc::new(payload);
                let pipeline = self
                    .pipeline
                    .as_ref()
                    .map(|_| Box::new(PayloadPipeline(payload.clone())) as Box<dyn PipelineHook>);
                let results = &mut self
                    .call
                    .as_mut()
                    .expect("wire completion once")
                    .results
                    .hook();
                let result = if let Some(local) = results
                    .as_any_mut()
                    .and_then(|sink| sink.downcast_mut::<crate::local::Results>())
                {
                    local.set_admitted_response(payload);
                    Ok(())
                } else {
                    results.set_response(payload)
                };
                match result {
                    Ok(()) => {
                        if let Some(state) = &self.pipeline {
                            state.finish(
                                self.local_results
                                    .as_ref()
                                    .and_then(server::Results::early_pipeline)
                                    .unwrap_or_else(|| pipeline.expect("enabled pipeline")),
                            );
                        }
                        self.call.take().unwrap().complete(Ok(()));
                    }
                    Err(error) => self.fail(error),
                }
            }
            Err(error) => self.fail(error),
        }
        Ok(())
    }
}
impl<I, const CAPS: usize> Drop for WireCall<'_, '_, I, CAPS> {
    fn drop(&mut self) {
        if self.call.is_some() {
            self.fail(Error::disconnected("wire call driver dropped".into()));
        }
    }
}

struct Projection {
    path: Vec<PipelineOp>,
    client: ProjectionClient,
    // Adopted outer queued clients have no Projected wrapper yet. Install one
    // at resolution to retain the same parent cancellation interest.
    retain_pipeline: bool,
}
enum ProjectionClient {
    Direct(Weak<Endpoint>),
    Queued(Weak<RefCell<crate::queued::ClientInner>>),
}
enum ProjectionTarget {
    Direct(Rc<Endpoint>),
    Queued(Rc<RefCell<crate::queued::ClientInner>>),
}
impl ProjectionClient {
    fn upgrade(&self) -> Option<ProjectionTarget> {
        match self {
            Self::Direct(value) => value.upgrade().map(ProjectionTarget::Direct),
            Self::Queued(value) => value.upgrade().map(ProjectionTarget::Queued),
        }
    }
    fn strong_count(&self) -> usize {
        match self {
            Self::Direct(value) => value.strong_count(),
            Self::Queued(value) => value.strong_count(),
        }
    }
}
impl ProjectionTarget {
    fn resolve(self, result: capnp::Result<Box<dyn ClientHook>>) {
        match self {
            Self::Direct(endpoint) => endpoint.forward_projection(result),
            Self::Queued(client) => crate::queued::ClientInner::resolve(&client, result),
        }
    }
}
fn pipeline_fields(path: &[PipelineOp]) -> impl Iterator<Item = u16> + '_ {
    path.iter().filter_map(|op| match op {
        PipelineOp::Noop => None,
        PipelineOp::GetPointerField(field) => Some(*field),
    })
}
type CachedProjection = (Vec<PipelineOp>, Box<dyn ClientHook>);
#[derive(Default)]
struct ProjectionCache {
    first: Option<CachedProjection>,
    rest: Vec<CachedProjection>,
}
impl ProjectionCache {
    fn len(&self) -> usize {
        usize::from(self.first.is_some()) + self.rest.len()
    }
    fn get(&self, path: &[PipelineOp]) -> Option<&dyn ClientHook> {
        self.first
            .iter()
            .chain(self.rest.iter())
            .find(|(key, _)| pipeline_fields(key).eq(pipeline_fields(path)))
            .map(|(_, hook)| &**hook)
    }
    fn remember(
        &mut self,
        path: Vec<PipelineOp>,
        hook: &dyn ClientHook,
        maximum: usize,
    ) -> capnp::Result<()> {
        if self.get(&path).is_some() {
            return Ok(());
        }
        if self.len() >= maximum {
            return Err(Error::overloaded("wire pipeline cache exhausted".into()));
        }
        if self.first.is_none() {
            self.first = Some((path, hook.add_ref()));
        } else {
            self.rest.try_reserve(1).map_err(wire_error)?;
            self.rest.push((path, hook.add_ref()));
        }
        Ok(())
    }
}
enum CompletedPipeline {
    Hook(Box<dyn PipelineHook>),
    Payload(Rc<Payload>),
}
impl CompletedPipeline {
    fn add_ref(&self) -> Self {
        match self {
            Self::Hook(hook) => Self::Hook(hook.add_ref()),
            Self::Payload(payload) => Self::Payload(payload.clone()),
        }
    }
    fn get_pipelined_cap(&self, path: &[PipelineOp]) -> Box<dyn ClientHook> {
        match self {
            Self::Hook(hook) => hook.get_pipelined_cap(path),
            Self::Payload(payload) => payload
                .reader()
                .and_then(|reader| reader.get_pipelined_cap(path))
                .unwrap_or_else(crate::broken::new_cap),
        }
    }
    fn get_pipelined_cap_move(&self, path: Vec<PipelineOp>) -> Box<dyn ClientHook> {
        match self {
            Self::Hook(hook) => hook.get_pipelined_cap_move(path),
            Self::Payload(_) => self.get_pipelined_cap(&path),
        }
    }
}

// Match legacy rpc::PipelineVariant: exactly one waiting, redirected,
// completed or failed state owns the pipeline at a time.
enum PipelineVariant {
    Waiting,
    Adopted(Box<dyn PipelineHook>),
    Resolved(CompletedPipeline),
    Broken(Error),
}

pub(crate) struct PipelineState {
    used: Cell<bool>,
    owner_wake: RefCell<Option<Waker>>,
    operation: Cell<Option<usize>>,
    materialize: Cell<bool>,
    variant: RefCell<PipelineVariant>,
    cached: RefCell<ProjectionCache>,
    _credit: Cell<Option<Rc<admission::Credit>>>,
    waiting: RefCell<VecDeque<Projection>>,
    inbox: Weak<Inbox>,
    maximum: usize,
}
impl PipelineState {
    fn bind_owner(&self, wake: Waker) {
        let previous = self.owner_wake.borrow_mut().replace(wake);
        assert!(
            previous.is_none(),
            "pipeline already has an operation owner"
        );
    }
    pub(super) fn request_materialization(&self) {
        self.materialize.set(true);
        self.notify_owner();
    }
    fn notify_owner(&self) {
        let wake = self.owner_wake.borrow().clone();
        if let Some(wake) = wake {
            wake.wake();
        }
    }
    pub(crate) fn prepare_params(
        &self,
        interface: u64,
        method: u16,
        size: Option<capnp::MessageSize>,
    ) -> crate::local::Params {
        let inbox = self.inbox.upgrade();
        let allocator = crate::local::result_allocator(size);
        let allocator = match inbox.as_ref().and_then(|inbox| inbox.message_pool.as_ref()) {
            Some(pool) => allocator.segment_pool(pool.clone()),
            None => allocator,
        };
        crate::local::Params::scoped(
            interface,
            method,
            allocator,
            Some(
                inbox
                    .as_ref()
                    .map(|inbox| inbox.call_template(interface, method)),
            ),
        )
    }
    pub(super) fn prepared(inbox: Option<&Rc<Inbox>>) -> Rc<Self> {
        Rc::new(Self {
            used: Cell::new(false),
            owner_wake: RefCell::new(None),
            operation: Cell::new(None),
            materialize: Cell::new(false),
            variant: RefCell::new(PipelineVariant::Waiting),
            cached: RefCell::default(),
            // Admission owns the credit until the wire call takes this state.
            // Rejected requests must release it even if their pipeline lives.
            _credit: Cell::new(None),
            waiting: RefCell::default(),
            inbox: inbox.map_or_else(Weak::new, Rc::downgrade),
            maximum: inbox.map_or(0, |inbox| inbox.maximum_clients),
        })
    }
    pub(super) fn owned(
        self: &Rc<Self>,
        interest: response::PipelineInterest,
    ) -> Box<dyn PipelineHook> {
        Box::new(WirePipeline(PipelineRef(
            Some(self.clone()),
            Some(interest),
        )))
    }
    pub(crate) fn unresolved(&self) -> bool {
        matches!(*self.variant.borrow(), PipelineVariant::Waiting)
    }
    // Local forwarding may publish a pipeline before its operation completes.
    // That publication remains valid even if the later operation fails.
    pub(crate) fn publish(&self, pipeline: Box<dyn PipelineHook>) {
        if !self.unresolved() {
            return;
        }
        let published = pipeline.add_ref();
        if !self.unresolved() {
            return;
        }
        let old = self.variant.replace(PipelineVariant::Adopted(published));
        drop(old);
        let waiting = std::mem::take(&mut *self.waiting.borrow_mut());
        for projection in waiting {
            if let Some(client) = projection.client.upgrade() {
                client.resolve(Ok(pipeline.get_pipelined_cap(&projection.path)));
            }
        }
    }
    pub(super) fn finish_pending(&self, pipeline: impl FnOnce() -> Box<dyn PipelineHook>) {
        if self.unresolved() {
            self.finish(pipeline());
        }
    }
    fn remember(&self, path: Vec<PipelineOp>, hook: &dyn ClientHook) -> capnp::Result<()> {
        self.cached.borrow_mut().remember(path, hook, self.maximum)
    }
    pub(super) fn finish(&self, pipeline: Box<dyn PipelineHook>) {
        self.finish_completed(CompletedPipeline::Hook(pipeline));
    }
    pub(super) fn finish_payload(&self, payload: Rc<Payload>) {
        self.finish_completed(CompletedPipeline::Payload(payload));
    }
    pub(super) fn break_pending(&self, error: Error) {
        if self.unresolved() {
            self.resolve(PipelineVariant::Broken(error));
        }
    }
    fn finish_completed(&self, pipeline: CompletedPipeline) {
        self.resolve(PipelineVariant::Resolved(pipeline));
    }
    fn resolve(&self, resolved: PipelineVariant) {
        let pipeline_ref = match &resolved {
            PipelineVariant::Resolved(pipeline) => Ok(pipeline.add_ref()),
            PipelineVariant::Broken(error) => Err(error.clone()),
            _ => unreachable!("terminal pipeline state"),
        };
        let old = self.variant.replace(resolved);
        drop(old);
        let cached = std::mem::take(&mut *self.cached.borrow_mut());
        drop(cached);
        let waiting = std::mem::take(&mut *self.waiting.borrow_mut());
        for projection in waiting {
            if let Some(client) = projection.client.upgrade() {
                client.resolve(match &pipeline_ref {
                    Ok(pipeline) => Ok(pipeline.get_pipelined_cap(&projection.path)),
                    Err(error) => Ok(crate::broken::new_cap(error.clone())),
                });
            }
        }
    }
}
#[derive(Clone)]
pub(crate) struct PipelineRef(
    Option<Rc<PipelineState>>,
    // Only public interests retain response interest. Completion is pushed by
    // its producer; no shared future or recursive pipeline polling is needed.
    Option<response::PipelineInterest>,
);
impl std::ops::Deref for PipelineRef {
    type Target = Rc<PipelineState>;
    fn deref(&self) -> &Self::Target {
        self.0.as_ref().expect("live pipeline interest")
    }
}
impl Drop for PipelineRef {
    fn drop(&mut self) {
        let inbox = self
            .0
            .as_ref()
            // The driver holds at most one reference. Dropping an additional
            // public clone cannot remove the last cancellation interest while
            // two other owners remain. Endpoint drops notify independently.
            .filter(|pipeline| {
                Rc::strong_count(pipeline) <= 2
                    // A completed pipeline cannot cancel its call. Releasing
                    // its payload still notifies through the final endpoints.
                    // A reentrant hook may hold the cell: retain the ordinary
                    // notification in that case rather than borrowing again.
                    && !pipeline.variant.try_borrow().is_ok_and(|state| {
                        matches!(*state, PipelineVariant::Resolved(_) | PipelineVariant::Broken(_))
                    })
            })
            .and_then(|pipeline| pipeline.inbox.upgrade());
        let wake = inbox
            .as_ref()
            .and(self.0.as_ref())
            .and_then(|pipeline| pipeline.owner_wake.borrow().clone());
        // A synchronous wake may poll cancellation immediately. Release the
        // interest first so that poll cannot observe the soon-to-die owner and
        // sleep forever after the final pipeline has gone away.
        drop(self.0.take());
        drop(self.1.take());
        if let Some(wake) = wake {
            wake.wake();
        }
        if let Some(inbox) = inbox {
            inbox.notify();
        }
    }
}
struct WirePipeline(PipelineRef);
impl PipelineHook for WirePipeline {
    fn add_ref(&self) -> Box<dyn PipelineHook> {
        Box::new(Self(self.0.clone()))
    }
    fn get_pipelined_cap(&self, path: &[PipelineOp]) -> Box<dyn ClientHook> {
        self.get_pipelined_cap_move(path.into())
    }
    fn get_pipelined_cap_move(&self, mut path: Vec<PipelineOp>) -> Box<dyn ClientHook> {
        let cached = self.0.cached.borrow().get(&path).map(|hook| hook.add_ref());
        if let Some(inner) = cached {
            return Box::new(Projected {
                inner,
                pipeline: self.0.clone(),
            });
        }
        let variant = match &*self.0.variant.borrow() {
            PipelineVariant::Waiting => PipelineVariant::Waiting,
            PipelineVariant::Adopted(pipeline) => PipelineVariant::Adopted(pipeline.add_ref()),
            PipelineVariant::Resolved(pipeline) => PipelineVariant::Resolved(pipeline.add_ref()),
            PipelineVariant::Broken(error) => PipelineVariant::Broken(error.clone()),
        };
        // As in legacy, select one variant before entering any client hook.
        // Actual projection callbacks run after the state borrow has ended.
        match variant {
            PipelineVariant::Adopted(pipeline) => return pipeline.get_pipelined_cap_move(path),
            PipelineVariant::Resolved(pipeline) => {
                self.0.used.set(true);
                return pipeline.get_pipelined_cap_move(path);
            }
            PipelineVariant::Broken(error) => {
                self.0.used.set(true);
                return crate::broken::new_cap(error);
            }
            PipelineVariant::Waiting => self.0.used.set(true),
        }
        let mut waiting = self.0.waiting.borrow_mut();
        waiting.retain(|entry| entry.client.strong_count() != 0);
        if let Some(endpoint) = waiting.iter().find_map(|entry| {
            if !pipeline_fields(&entry.path).eq(pipeline_fields(&path)) {
                return None;
            }
            match &entry.client {
                ProjectionClient::Direct(endpoint) => endpoint.upgrade(),
                _ => None,
            }
        }) {
            return Box::new(Projected {
                inner: Box::new(Hook(endpoint, None)),
                pipeline: self.0.clone(),
            });
        }
        if waiting.len().saturating_add(self.0.cached.borrow().len()) >= self.0.maximum
            || waiting.try_reserve(1).is_err()
        {
            return crate::broken::new_cap(Error::overloaded(
                "wire pipeline capacity exhausted".into(),
            ));
        }
        let endpoint = Endpoint::projection(self.0.inbox.clone());
        // Consume the caller's transform buffer instead of allocating a second
        // path. Noop is semantically empty and can be removed in place.
        path.retain(|op| !matches!(op, PipelineOp::Noop));
        waiting.push_back(Projection {
            path,
            client: ProjectionClient::Direct(Rc::downgrade(&endpoint)),
            retain_pipeline: false,
        });
        drop(waiting);
        self.0.notify_owner();
        if let Some(inbox) = self.0.inbox.upgrade() {
            inbox.notify();
        }
        Box::new(Projected {
            inner: Box::new(Hook(endpoint, None)),
            pipeline: self.0.clone(),
        })
    }
}
struct Projected {
    inner: Box<dyn ClientHook>,
    pipeline: PipelineRef,
}
impl Projected {
    fn drive<T: 'static>(
        &self,
        mut promise: impl Future<Output = Result<T, Error>> + Unpin + 'static,
    ) -> Promise<T, Error> {
        use crate::attach::Attach;
        let endpoint = direct_response_hook(self).map(|hook| hook.0.clone());
        Promise::from_future(
            futures::future::poll_fn(move |cx| {
                if let Poll::Ready(result) = Pin::new(&mut promise).poll(cx) {
                    return Poll::Ready(result);
                }
                // Subscribe to the projection itself, not the parent's response
                // future. Local forwarding and rejection can settle it before a
                // protocol driver admits this child.
                if let Some(endpoint) = &endpoint {
                    endpoint.binding_pending(cx);
                    if endpoint.index.get().is_none() {
                        if let Some(error) = Hook(endpoint.clone(), None).get_failure() {
                            return Poll::Ready(Err(error));
                        }
                    }
                }
                Poll::Pending
            })
            .attach(self.pipeline.clone()),
        )
    }
}
impl<'id> ClientHooks<'id> {
    fn import_wire_capability<I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>(
        &mut self,
        driver: &mut Driver<'_, 'id, '_, I, A, P, S, CAPS, PATH>,
        client: Client<'id>,
        local_hook: &mut impl FnMut(&S) -> Option<Box<dyn ClientHook>>,
    ) -> capnp::Result<Box<dyn ClientHook>> {
        // A reflected route returns the original local authority, including an
        // opaque promise or membrane. Ordering barriers remain Waiting and keep
        // their ordinary adapter until the graph allows the route to settle.
        let local = match driver.runtime().inspect(&client) {
            Ok(capntproto_rpc_engine::clients::Destination::Endpoint {
                owner:
                    capntproto_rpc_wire::clients::Route::Capability(
                        capntproto_rpc_wire::clients::Endpoint::Local(owner)
                        | capntproto_rpc_wire::clients::Endpoint::Failed { owner, .. },
                    )
                    | capntproto_rpc_wire::clients::Route::Vine(owner),
                ..
            }) => local_hook(owner),
            _ => None,
        };
        match local {
            Some(hook) => {
                driver.capabilities().release(client);
                Ok(hook)
            }
            None => self
                .import_after_collect(&mut driver.capabilities(), client)
                .map(|client| client.hook),
        }
    }
}

pub(crate) fn native_join_target(client: &dyn ClientHook) -> Option<Box<dyn ClientHook>> {
    let hook = direct_response_hook(client).or_else(|| client.as_any()?.downcast_ref::<Hook>())?;
    let inbox = hook.0.inbox.upgrade()?;
    if inbox.closed.get()
        || inbox.joins.is_none()
        || hook.0.local_deferred.get()
        || hook.0.introduction.borrow().is_some()
        || (!hook.0.remote.get() && hook.0.direct.get().is_none())
    {
        return None;
    }
    Some(hook.add_ref())
}

pub(super) fn direct_response_hook(client: &dyn ClientHook) -> Option<&Hook> {
    client
        .as_any()?
        .downcast_ref::<Projected>()?
        .inner
        .as_any()?
        .downcast_ref::<Hook>()
}

pub(super) fn request_projected(
    client: &dyn ClientHook,
    interface: u64,
    method: u16,
    hints: CallHints,
    mut params: Box<crate::local::Request>,
    context: Option<Rc<dyn std::any::Any>>,
) -> capability::RemotePromise<any_pointer::Owned> {
    let projected = client
        .as_any()
        .and_then(|client| client.downcast_ref::<Projected>())
        .expect("private projected request");
    params.retain_pipeline(projected.pipeline.clone());
    let mut response = response::request_scoped(
        direct_response_hook(client).expect("private projected endpoint"),
        interface,
        method,
        hints,
        params,
        context,
    );
    response.promise = capability::ResponsePromise::new(projected.drive(response.promise));
    response
}
impl ClientHook for Projected {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn get_failure(&self) -> Option<Error> {
        self.inner.get_failure()
    }
    fn metadata_changes(&self) -> Option<Rc<capnp::private::capability::MetadataChanges>> {
        self.inner.metadata_changes()
    }
    #[cfg(unix)]
    fn get_fd(&self) -> Option<Rc<std::os::fd::OwnedFd>> {
        self.inner.get_fd()
    }
    fn add_ref(&self) -> Box<dyn ClientHook> {
        Box::new(Self {
            inner: self.inner.add_ref(),
            pipeline: self.pipeline.clone(),
        })
    }
    fn debug_info(&self, info: &mut DebugInfo) {
        info.push("wirePipeline");
        info.follow(&*self.inner);
    }
    fn new_call(
        &self,
        interface: u64,
        method: u16,
        size: Option<capnp::MessageSize>,
    ) -> capability::Request<any_pointer::Owned, any_pointer::Owned> {
        capability::Request::new(Box::new(crate::local::Request::prepared(
            interface,
            method,
            size,
            self.add_ref(),
            &self.pipeline,
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
        if direct_response_hook(self).is_some_and(|hook| {
            hook.0.remote.get() && (hook.0.index.get().is_some() || hook.0.direct.get().is_some())
        }) {
            // A bound native destination already retains its wire question.
            // Publication transfers that receipt; the parent response need not
            // stay alive until this child's unrelated acknowledgement.
            return self
                .inner
                .call_with_hints(interface, method, params, results, hints);
        }
        self.drive(
            self.inner
                .call_with_hints(interface, method, params, results, hints),
        )
    }
    fn get_brand(&self) -> usize {
        0
    }
    fn get_ptr(&self) -> usize {
        self.inner.get_ptr()
    }
    fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
        self.inner.get_resolved().map(|inner| {
            Box::new(Self {
                inner,
                pipeline: self.pipeline.clone(),
            }) as Box<dyn ClientHook>
        })
    }
    fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, Error>> {
        self.inner.when_more_resolved().map(|promise| {
            let pipeline = self.pipeline.clone();
            self.drive(Promise::from_future(async move {
                Ok(Box::new(Self {
                    inner: promise.await?,
                    pipeline,
                }) as Box<dyn ClientHook>)
            }))
        })
    }
    fn when_resolved(&self) -> Promise<(), Error> {
        self.drive(self.inner.when_resolved())
    }
}

#[cfg(test)]
mod tests;
