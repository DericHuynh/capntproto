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
        connection::{self, CallOutput, Driver, Runtime},
    },
    pipeline,
};

mod calls;
pub mod coordinator;
pub mod foreign;
pub mod resources;
pub mod server;
pub use calls::{Calls, Progress};

/// Rejected admission preserves the original call and its completion owner.
#[must_use]
pub struct Rejected {
    pub error: Error,
    pub call: Call,
}

/// An owned message whose cap table is independent of scoped wire receipts.
pub(crate) struct Payload {
    body: PayloadBody,
    caps: Vec<Option<Box<dyn ClientHook>>>,
}
enum PayloadBody {
    Owned(Box<capnp::message::Builder<capnp::message::HeapAllocator>>),
    Return(Box<connection::PreparedReturn<capnp::message::HeapAllocator>>),
    Admitted(Box<owned::PlainResults>),
    Shared { message: Box<SharedPayload> },
}
struct SharedPayload {
    message: capnp::message::CheckedStructReader<
        capnp::serialize::NoAllocBufferSegments<messages::SharedBytes>,
    >,
}
impl capnp::private::capability::ResponseHook for SharedPayload {
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        Ok(
            capntproto_rpc_wire::rpc_capnp::payload::Reader::from(self.message.root())
                .get_content(),
        )
    }
}
#[derive(Clone, Copy)]
enum PayloadKind {
    Params,
    Results,
}
impl Payload {
    pub(crate) fn into_response(self) -> Box<dyn capnp::private::capability::ResponseHook> {
        match self.body {
            PayloadBody::Admitted(message) => {
                debug_assert!(self.caps.is_empty());
                message
            }
            PayloadBody::Shared { message } => {
                debug_assert!(self.caps.is_empty());
                message
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
    fn retain_admitted_or_copy(
        reader: any_pointer::Reader<'_>,
        bytes: Option<messages::SharedBytes>,
        kind: PayloadKind,
        descriptor_count: usize,
    ) -> capnp::Result<Self> {
        // Both callers use the content and receipts of an admitted immutable
        // Call/Return. Admission already traversed this entire graph under its
        // original limits and validated every capability index. With no
        // descriptors, that proves the shared payload contains no capabilities;
        // another target_size traversal cannot establish any additional fact.
        // Nonempty tables still copy/remap through their admitted hook owners.
        if descriptor_count == 0 {
            if let Some(bytes) = bytes {
                return Self::from_admitted_bytes(bytes, kind);
            }
        }
        Self::copy(reader)
    }
    #[inline(always)]
    fn from_admitted_bytes(bytes: messages::SharedBytes, kind: PayloadKind) -> capnp::Result<Self> {
        let options = capnp::message::ReaderOptions {
            traversal_limit_in_words: None,
            // Match owned builder readers after validation with the
            // source's nesting limit; no unvalidated graph is exposed.
            nesting_limit: i32::MAX,
        };
        let segments = capnp::serialize::NoAllocBufferSegments::from_buffer(bytes, options)?;
        Ok(Self {
            body: PayloadBody::Shared {
                message: Box::new(SharedPayload {
                    message: capnp::message::CheckedStructReader::select(
                        capnp::message::Reader::new(segments, options),
                        |root| {
                            use capnp::traits::IntoInternalStructReader;
                            let root = capntproto_rpc_wire::rpc_capnp::message::Reader::from(root);
                            let payload = match (
                                kind,
                                root.which()
                                    .map_err(|_| Error::from_kind(capnp::ErrorKind::Failed))?,
                            ) {
                                (
                                    PayloadKind::Params,
                                    capntproto_rpc_wire::rpc_capnp::message::Call(call),
                                ) => call?.get_params()?,
                                (
                                    PayloadKind::Results,
                                    capntproto_rpc_wire::rpc_capnp::message::Return(result),
                                ) => {
                                    let capntproto_rpc_wire::rpc_capnp::return_::Results(payload) =
                                        result?.which().map_err(|_| {
                                            Error::from_kind(capnp::ErrorKind::Failed)
                                        })?
                                    else {
                                        return Err(Error::failed(
                                            "shared response is not results".into(),
                                        ));
                                    };
                                    payload?
                                }
                                _ => {
                                    return Err(Error::failed(
                                        "shared payload message kind mismatch".into(),
                                    ))
                                }
                            };
                            Ok(payload.into_internal_struct_reader())
                        },
                    )?,
                }),
            },
            caps: Vec::new(),
        })
    }
    fn reader(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        let mut root: any_pointer::Reader<'_> = match &self.body {
            PayloadBody::Owned(message) => message.get_root_as_reader()?,
            PayloadBody::Return(message) => message.reader()?,
            PayloadBody::Admitted(message) => {
                capnp::private::capability::ResponseHook::get(&**message)?
            }
            PayloadBody::Shared { message } => {
                capnp::private::capability::ResponseHook::get(&**message)?
            }
        };
        root.imbue(&self.caps);
        Ok(root)
    }
    fn into_pipeline(self) -> Box<dyn PipelineHook> {
        Box::new(crate::local::Pipeline::new(Box::new(PayloadDone(Rc::new(
            self,
        )))))
    }
}
impl capnp::private::capability::ResponseHook for Payload {
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        self.reader()
    }
}
struct PayloadDone(Rc<Payload>);
impl crate::local::ResultsDoneHook for PayloadDone {
    fn add_ref(&self) -> Box<dyn crate::local::ResultsDoneHook> {
        Box::new(Self(self.0.clone()))
    }
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        self.0.reader()
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
            &mut Runtime<'pool, 'id, 'storage, I, P, S, CAPS, PATH>,
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
    }
    #[allow(clippy::too_many_arguments, clippy::result_large_err)]
    pub(super) fn enqueue_wire_using<
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
        mut call: Call,
        driver: &mut Driver<'pool, 'id, 'storage, I, A, P, S, CAPS, PATH>,
        invocations: &mut connection::Invocations<'_, '_, 'pool, 'id, I, CAPS>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        options: capnp::message::ReaderOptions,
        direct: bool,
        mut import: impl FnMut(
            &mut Runtime<'pool, 'id, 'storage, I, P, S, CAPS, PATH>,
            &dyn ClientHook,
        ) -> capnp::Result<Client<'id>>,
    ) -> Result<WireCall<'pool, 'id, I, CAPS>, Rejected> {
        let mut receiver = None;
        let mut direct_stage = None;
        let outcome = (|| {
            let target = self.target(&call)?;
            // A settled imported endpoint cannot become a local producer or
            // acquire a new promise pipeline. Its hosted owner can consume the
            // driver's result directly, without another completion channel and
            // polling ledger. Dispatch ordering and question ownership remain
            // in the same portable driver as every other invocation.
            let direct = direct
                && call.hints.no_promise_pipelining
                && call.third_party.is_none()
                && driver.runtime.is_settled(target).map_err(wire_error)?
                && matches!(
                    driver.runtime.inspect(target).map_err(wire_error)?,
                    capntproto_rpc_engine::clients::Destination::Endpoint {
                        owner: capntproto_rpc_wire::clients::Route::Capability(
                            capntproto_rpc_wire::clients::Endpoint::Imported(_)
                        ),
                        location: capntproto_rpc_engine::clients::Location::Remote(_),
                        ..
                    }
                );
            let params = call.params()?;
            let size = params.target_size()?;
            // Data-only parameters already have the identity wire cap mapping.
            // Keep their caller-owned reader through the transactional copy
            // instead of allocating and copying a second hosted message first.
            let payload = if size.cap_count == 0 {
                None
            } else {
                Some(Payload::copy(params)?)
            };
            let caps = payload.as_ref().map_or(&[][..], |payload| &payload.caps);
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
                        *slot = Some(match self.try_retain(&mut driver.runtime, &**cap)? {
                            Some(client) => client,
                            None => import(&mut driver.runtime, &**cap)?,
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
                let destination = call
                    .third_party
                    .as_ref()
                    .map(|body| body.get_root_as_reader::<any_pointer::Reader>())
                    .transpose()?;
                let extra = destination
                    // Metadata copy uses a transactional staging pointer in
                    // addition to the reachable destination words.
                    .map(|reader| reader.target_size().map(|size| size.word_count + 1))
                    .transpose()?
                    .unwrap_or(0);
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
                        .map_err(wire_error)?;
                    direct_stage = Some(DirectStage::Queued);
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
                        wire_error(format!("queued invocation: {:?}", rejected.error))
                    })
            })();
            for client in retained.into_iter().flatten() {
                driver.runtime.release(client);
            }
            result
        })();
        let task = match outcome {
            Ok(task) => task,
            Err(error) => return Err(Rejected { error, call }),
        };
        let pipeline = (!call.hints.no_promise_pipelining).then(|| {
            Rc::new(PipelineState {
                used: Cell::new(false),
                adopted: RefCell::default(),
                cached: RefCell::default(),
                _credit: call.credit.clone(),
                waiting: RefCell::new(VecDeque::new()),
                finished: RefCell::new(None),
                inbox: Rc::downgrade(&self.inbox),
                maximum: self.maximum_clients,
            })
        });
        if let Some(pipeline) = &pipeline {
            if let Err(error) = call
                .results
                .hook()
                .set_pipeline_from(Box::new(WirePipeline(PipelineRef(Some(pipeline.clone())))))
            {
                return Err(Rejected { error, call });
            }
        }
        Ok(WireCall {
            call: Some(Box::new(call)),
            task: Some(task),
            receiver,
            direct: direct_stage,
            pipeline,
            local_results: None,
            tail_response: None,
        })
    }
}

/// One admitted request. Poll projections and completion alongside Invocations.
/// Dropping it cancels its tracked reply; Invocations retains wire cleanup.
#[must_use = "drive the call to completion or cancel its tracked reply"]
pub struct WireCall<'pool, 'id, I, const CAPS: usize> {
    call: Option<Box<Call>>,
    // Public enqueue_wire always supplies a task. The private hosted immediate
    // path has already published its question and never exposes this WireCall.
    task: Option<dispatch::TaskKey<'id>>,
    receiver: Option<shared::Receiver<connection::CallResult<'pool, 'id, I, CAPS>>>,
    direct: Option<DirectStage<'id>>,
    pipeline: Option<Rc<PipelineState>>,
    local_results: Option<server::Results>,
    tail_response: Option<Box<connection::Response<'pool, 'id, I, CAPS>>>,
}
#[derive(Clone, Copy)]
enum DirectStage<'id> {
    Queued,
    Sent(capntproto_rpc_engine::questions::Key<'id>),
}
impl<'pool, 'id, I, const CAPS: usize> WireCall<'pool, 'id, I, CAPS> {
    pub fn task(&self) -> dispatch::TaskKey<'id> {
        self.task.expect("public wire call has a queued task")
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
            dispatch::TaskKey<'id>,
            pipeline::Path<'_>,
            &mut Runtime<'pool, 'id, '_, I, P, S, CAPS, PATH>,
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
            dispatch::TaskKey<'id>,
            pipeline::Path<'_>,
            &mut Runtime<'pool, 'id, '_, I, P, S, CAPS, PATH>,
        ) -> Result<Option<Client<'id>>, connection::Error>,
        mut refresh: impl FnMut(
            &mut ClientHooks<'id>,
            &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        ),
    ) -> capnp::Result<()> {
        let Some(pipeline) = &self.pipeline else {
            return Ok(());
        };
        let budget = pipeline.waiting.borrow().len();
        for _ in 0..budget {
            let Some(projection) = pipeline.waiting.borrow_mut().pop_front() else {
                break;
            };
            let Some(client) = projection.client.upgrade() else {
                continue;
            };
            let fields = projection
                .path
                .iter()
                .filter_map(|op| match op {
                    PipelineOp::Noop => None,
                    PipelineOp::GetPointerField(field) => Some(*field),
                })
                .collect::<Vec<_>>();
            if fields.len() > driver.path_capacity() {
                crate::queued::ClientInner::resolve(
                    &client,
                    Err(Error::overloaded(
                        "wire pipeline path capacity exhausted".into(),
                    )),
                );
                continue;
            }
            let path = pipeline::Path::new(&fields);
            let routed = if let Some(response) = &self.tail_response {
                driver.tail_pipeline(response, path, messages)
            } else {
                invocations.pipeline_with(
                    driver,
                    self.task.expect("pipelines use queued tasks"),
                    path,
                    messages,
                    |task, path, runtime| resolve(hooks, task, path, runtime),
                )
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
            let result = {
                let result = hooks.export_client(&mut driver.runtime, &cap);
                driver.runtime.release(cap);
                result.map(|client| client.hook)
            };
            refresh(hooks, driver);
            if let Ok(hook) = &result {
                pipeline.remember(fields, &**hook)?;
            }
            crate::queued::ClientInner::resolve(&client, result);
        }
        Ok(())
    }
    pub fn poll_canceled(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        let Some(call) = self.call.as_mut() else {
            return Poll::Ready(());
        };
        if call
            .endpoint
            .inbox
            .upgrade()
            .is_none_or(|inbox| inbox.closed.get())
        {
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
        drop(self.receiver.take());
        self.fail(Error::disconnected("wire call canceled".into()));
    }
    pub(crate) fn cancel_with_error(mut self, error: Error) {
        drop(self.receiver.take());
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
                pipeline.finish(
                    self.local_results
                        .as_ref()
                        .and_then(server::Results::early_pipeline)
                        .unwrap_or_else(|| Box::new(crate::broken::Pipeline::new(error.clone()))),
                );
            }
            call.complete(Err(error));
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
            if self.tail_response.is_some() {
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
            drop(self.receiver.take());
            self.fail(Error::disconnected("wire call canceled".into()));
            return Poll::Ready(Ok(()));
        }
        // Calls owns both ends of this internal completion and polls the
        // producer immediately before the receiver. Its bounded scan already
        // schedules continuation after progress. Do not notify the external
        // application lanes merely for transferring an internal result.
        if let Some(stage) = self.direct {
            let DirectStage::Sent(question) = stage else {
                return Poll::Pending;
            };
            let result = match driver.take_output(question, messages) {
                Ok(result) => result,
                Err(
                    connection::Error::PendingResponse | connection::Error::PendingAuthorization,
                ) => return Poll::Pending,
                Err(error) => return Poll::Ready(Err(wire_error(error))),
            };
            self.direct = None;
            return Poll::Ready(
                self.complete(result, hooks, driver, messages, refresh, local_hook),
            );
        }
        let mut ready_cx = Context::from_waker(Waker::noop());
        let completion_cx = if inline_completion {
            &mut ready_cx
        } else {
            &mut *cx
        };
        let output = match Pin::new(self.receiver.as_mut().expect("active wire receiver"))
            .poll(completion_cx)
        {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(output)) => output,
            Poll::Ready(Err(error)) => {
                self.fail(crate::canceled_to_error(error));
                return Poll::Ready(Ok(()));
            }
        };
        drop(self.receiver.take());
        let result = self.complete(output, hooks, driver, messages, refresh, local_hook);
        if result.is_ok() && self.tail_response.is_some() {
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
        match self.direct.take() {
            Some(DirectStage::Queued) => {
                if !driver.cancel_invocation(self.task.expect("queued task"), messages) {
                    return Err(wire_error(connection::Error::UnknownInvocation));
                }
            }
            Some(DirectStage::Sent(question)) => driver.cancel(question).map_err(wire_error)?,
            None => (),
        }
        Ok(())
    }
    pub(crate) fn release_tail<A, P: Copy + Eq, S, const PATH: usize>(
        &mut self,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) {
        if let Some(response) = self.tail_response.take() {
            driver.release_output(CallOutput::Remote(*response), messages);
        }
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
        // The admitted reader already owns the immutable, capability-free
        // payload. Transfer it straight to a direct result sink; preserve the
        // same credit, metadata refresh and wire cleanup as general completion.
        if self.pipeline.is_none()
            && self.call.as_ref().is_some_and(|call| {
                call.third_party.is_none() && matches!(call.results, response::Results::Direct(_))
            })
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
                    let result = call.results.finish_admitted(Payload {
                        body: PayloadBody::Admitted(message),
                        caps: Vec::new(),
                    });
                    debug_assert!(result.is_ok());
                    call.complete(Ok(()));
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
            .is_some_and(|call| call.third_party.is_some())
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
                    self.tail_response = Some(Box::new(response));
                    self.call.take().expect("active tail call").complete(Ok(()));
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
                                    if driver.runtime.is_settled(cap).unwrap_or(false) {
                                        if let capntproto_rpc_engine::clients::Destination::Endpoint {
                                            owner: capntproto_rpc_wire::clients::Route::Capability(capntproto_rpc_wire::clients::Endpoint::Local(hook)),
                                            ..
                                        } = driver.runtime.inspect(cap).map_err(wire_error)? {
                                            if let Some(hook) = local_hook(hook) { return Ok(hook); }
                                        }
                                    }
                                    hooks
                                        .export_client(&mut driver.runtime, cap)
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
                } else if let Some(bytes) = remote.share_plain_results(messages) {
                    Payload::from_admitted_bytes(bytes, PayloadKind::Results)
                } else { remote
                .with(messages, |reply| match reply {
                    owned::Reply::Results(payload) => {
                        let mut caps = Vec::with_capacity(payload.capability_count());
                        for index in 0..payload.capability_count() {
                            let cap = payload.capability_at(index as u32).map_err(wire_error)?;
                            let cap = match cap {
                                None => None,
                                Some(cap) => {
                                    let cap = driver
                                        .imports
                                        .bind_capability(
                                            cap,
                                            &mut driver.runtime,
                                            &mut driver.ledgers.imports,
                                            &driver.ledgers.exports,
                                            &mut driver.ledgers.answers,
                                            &mut driver.pipelines,
                                        )
                                        .map_err(wire_error)?
                                        .into_client();
                                    let result = hooks.export_client(&mut driver.runtime, &cap);
                                    driver.runtime.release(cap);
                                    Some(result?.hook)
                                }
                            };
                            caps.push(cap);
                        }
                        let mut content = payload.content();
                        content.imbue(&caps);
                        Payload::retain_admitted_or_copy(content, remote.share_results(messages), PayloadKind::Results, payload.capability_count())
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
                let pipeline = self.pipeline.as_ref().map(|_| {
                    Box::new(crate::local::Pipeline::new(Box::new(PayloadDone(
                        payload.clone(),
                    )))) as Box<dyn PipelineHook>
                });
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
    client: Weak<RefCell<crate::queued::ClientInner>>,
}
struct PipelineState {
    used: Cell<bool>,
    adopted: RefCell<Option<Box<dyn PipelineHook>>>,
    cached: RefCell<Vec<(Vec<u16>, Rc<dyn ClientHook>)>>,
    _credit: Option<Rc<admission::Credit>>,
    waiting: RefCell<VecDeque<Projection>>,
    finished: RefCell<Option<Box<dyn PipelineHook>>>,
    inbox: Weak<Inbox>,
    maximum: usize,
}
impl PipelineState {
    fn remember(&self, path: Vec<u16>, hook: &dyn ClientHook) -> capnp::Result<()> {
        let mut cache = self.cached.borrow_mut();
        if cache.iter().any(|(fields, _)| *fields == path) {
            return Ok(());
        }
        if cache.len() >= self.maximum {
            return Err(Error::overloaded("wire pipeline cache exhausted".into()));
        }
        cache.try_reserve(1).map_err(wire_error)?;
        cache.push((path, Rc::from(hook.add_ref())));
        Ok(())
    }
    fn finish(&self, pipeline: Box<dyn PipelineHook>) {
        let pipeline_ref = pipeline.add_ref();
        let old = self.finished.replace(Some(pipeline));
        drop(old);
        let cached = std::mem::take(&mut *self.cached.borrow_mut());
        let adopted = self.adopted.borrow_mut().take();
        drop((cached, adopted));
        let waiting = std::mem::take(&mut *self.waiting.borrow_mut());
        for projection in waiting {
            if let Some(client) = projection.client.upgrade() {
                crate::queued::ClientInner::resolve(
                    &client,
                    Ok(pipeline_ref.get_pipelined_cap(&projection.path)),
                );
            }
        }
    }
}
#[derive(Clone)]
struct PipelineRef(Option<Rc<PipelineState>>);
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
            .and_then(|pipeline| pipeline.inbox.upgrade());
        // A synchronous wake may poll cancellation immediately. Release the
        // interest first so that poll cannot observe the soon-to-die owner and
        // sleep forever after the final pipeline has gone away.
        drop(self.0.take());
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
        let fields: Vec<_> = path
            .iter()
            .filter_map(|op| match op {
                PipelineOp::Noop => None,
                PipelineOp::GetPointerField(field) => Some(*field),
            })
            .collect();
        let cached = self
            .0
            .cached
            .borrow()
            .iter()
            .find(|(key, _)| *key == fields)
            .map(|(_, hook)| hook.add_ref());
        if let Some(inner) = cached {
            return Box::new(Projected {
                inner,
                pipeline: self.0.clone(),
            });
        }
        let adopted = self.0.adopted.borrow().as_ref().map(|p| p.add_ref());
        if let Some(pipeline) = adopted {
            return pipeline.get_pipelined_cap(path);
        }
        self.0.used.set(true);
        let finished = self.0.finished.borrow().as_ref().map(|p| p.add_ref());
        if let Some(pipeline) = finished {
            return pipeline.get_pipelined_cap(path);
        }
        let client = crate::queued::Client::new(None);
        let mut waiting = self.0.waiting.borrow_mut();
        waiting.retain(|entry| entry.client.strong_count() != 0);
        if waiting.len().saturating_add(self.0.cached.borrow().len()) >= self.0.maximum
            || waiting.try_reserve(1).is_err()
        {
            return crate::broken::new_cap(Error::overloaded(
                "wire pipeline capacity exhausted".into(),
            ));
        }
        waiting.push_back(Projection {
            path: path.to_vec(),
            client: Rc::downgrade(&client.inner),
        });
        drop(waiting);
        if let Some(inbox) = self.0.inbox.upgrade() {
            inbox.notify();
        }
        Box::new(Projected {
            inner: Box::new(client),
            pipeline: self.0.clone(),
        })
    }
}
struct Projected {
    inner: Box<dyn ClientHook>,
    pipeline: PipelineRef,
}
impl ClientHook for Projected {
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
        capability::Request::new(Box::new(crate::local::Request::new(
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
        use crate::attach::Attach;
        Promise::from_future(
            self.inner
                .call_with_hints(interface, method, params, results, hints)
                .attach(self.pipeline.clone()),
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
            Promise::from_future(async move {
                Ok(Box::new(Self {
                    inner: promise.await?,
                    pipeline,
                }) as Box<dyn ClientHook>)
            })
        })
    }
    fn when_resolved(&self) -> Promise<(), Error> {
        use crate::attach::Attach;
        Promise::from_future(self.inner.when_resolved().attach(self.pipeline.clone()))
    }
}

#[cfg(test)]
mod tests;
