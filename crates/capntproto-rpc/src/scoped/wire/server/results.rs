//! Owned application results; protocol ownership stays in the portable driver.
use super::*;
use capnp::private::capability::RequestHook;
use coordinator::multiparty::{tail, Handle};
#[cfg(test)]
mod tests;

#[derive(Default)]
struct State {
    message_pool: Option<capnp::message::SegmentPool>,
    tail_origin: RefCell<Option<Handle>>,
    network: RefCell<Option<Box<NetworkState>>>,
    transferred: Cell<bool>,
    context: RefCell<Option<Rc<dyn std::any::Any>>>,
    method: Cell<Option<DynamicCall>>,
    payload: RefCell<Option<Rc<Payload>>>,
    pipeline: RefCell<Option<Rc<dyn PipelineHook>>>,
    completion: RefCell<Option<capnp::Result<()>>>,
    wake: RefCell<Vec<Waker>>,
    charge: RefCell<Option<coordinator::policy::Charge>>,
    trace_prepared: Cell<bool>,
    trace: RefCell<Option<String>>,
}
// Ordinary answers do not own network transfers. Keep their larger, optional
// state separate without changing the lifetime of any transfer or guard.
#[derive(Default)]
struct NetworkState {
    redirect: Option<Rc<tail::Redirect>>,
    destination: Option<tail::Destination>,
    guard: Option<(
        Option<Rc<crate::third_party::ThirdPartyExchange>>,
        Box<dyn std::any::Any>,
    )>,
}
impl State {
    fn notify(&self) {
        let wakes = std::mem::take(&mut *self.wake.borrow_mut());
        for wake in wakes {
            wake.wake();
        }
    }
    fn complete(&self, result: capnp::Result<()>) {
        let previous = self.completion.borrow_mut().replace(result);
        drop(previous);
        self.notify();
    }
}

/// Application state suitable for an answer's producer. Contains no question
/// IDs, ledger entries, graph references or transport ownership. Keep it until
/// the answer retires so early pipelines survive final completion.
#[derive(Clone, Default)]
pub struct Results(Rc<State>);
impl Results {
    pub(crate) fn tail_origin(&self, origin: Option<Handle>) {
        *self.0.tail_origin.borrow_mut() = origin;
    }
    pub(crate) fn redirect(&self) -> Option<Rc<tail::Redirect>> {
        self.0
            .network
            .borrow()
            .as_ref()
            .and_then(|state| state.redirect.clone())
    }
    pub(crate) fn transfer_ready(&self) -> bool {
        self.0.transferred.get()
    }
    pub(crate) fn prepare_transfer(
        &self,
        destination: tail::Destination,
        permit: capntproto_rpc_engine::flow_control::outgoing::OwnedPermit,
    ) {
        crate::admission::retain_context(&mut self.0.context.borrow_mut(), Rc::new(permit));
        let previous = self
            .0
            .network
            .borrow_mut()
            .get_or_insert_with(Default::default)
            .destination
            .replace(destination);
        drop(previous);
        self.0.transferred.set(true);
    }
    pub(crate) fn start_transfer(&self, execution: Execution) -> capnp::Result<Work> {
        let work = execution.boxed_local().shared();
        let destination = self
            .0
            .network
            .borrow_mut()
            .as_mut()
            .and_then(|state| state.destination.take());
        if let Some(destination) = destination {
            destination.start(Transfer {
                results: self.clone(),
                work: work.clone(),
            })?;
        }
        Ok(work)
    }
    pub(crate) fn retain_network(
        &self,
        exchange: Option<Rc<crate::third_party::ThirdPartyExchange>>,
        guard: Box<dyn std::any::Any>,
    ) {
        let previous = self
            .0
            .network
            .borrow_mut()
            .get_or_insert_with(Default::default)
            .guard
            .replace((exchange, guard));
        drop(previous);
    }
    pub(crate) fn release_embargo(&self, id: &[u8]) -> capnp::Result<()> {
        let exchange = self
            .0
            .network
            .borrow()
            .as_ref()
            .and_then(|state| state.guard.as_ref())
            .and_then(|(exchange, _)| exchange.clone())
            .ok_or_else(|| Error::failed("missing authenticated provision".into()))?;
        exchange.disembargo(id)
    }
    pub fn new() -> Self {
        Self::default()
    }
    pub(in crate::scoped::wire) fn with_charge(
        charge: Option<coordinator::policy::Charge>,
        message_pool: Option<capnp::message::SegmentPool>,
    ) -> Self {
        Self(Rc::new(State {
            message_pool,
            charge: RefCell::new(charge),
            tail_origin: RefCell::new(None),
            transferred: Cell::new(false),
            network: RefCell::new(None),
            context: RefCell::new(None),
            method: Cell::new(None),
            payload: RefCell::new(None),
            pipeline: RefCell::new(None),
            completion: RefCell::new(None),
            wake: RefCell::new(Vec::new()),
            trace_prepared: Cell::new(false),
            trace: RefCell::new(None),
        }))
    }
    pub(in crate::scoped::wire) fn returned(&self) {
        let charge = self.0.charge.borrow_mut().take();
        drop(charge);
    }
    pub(in crate::scoped::wire) fn prepare_trace(&self, policy: &coordinator::policy::Policy) {
        let Some(error) = self.failure() else {
            return;
        };
        if !self.0.trace_prepared.replace(true) {
            let trace = policy.trace(&error);
            *self.0.trace.borrow_mut() = trace;
        }
    }
    /// Success needs both service completion and returned ResultsHook ownership.
    /// Exceptions are ready even if application code retains an unused builder.
    pub fn poll_completed(&self, cx: &mut Context<'_>) -> Poll<capnp::Result<()>> {
        // This local state cannot change between inspection and registration
        // without running application code. Avoid allocating a waiter for an
        // already-completed synchronous method.
        match self.0.completion.borrow().as_ref() {
            Some(Err(error)) => return Poll::Ready(Err(error.clone())),
            Some(Ok(())) if self.0.payload.borrow().is_some() => return Poll::Ready(Ok(())),
            _ => (),
        }
        let mut wakes = self.0.wake.borrow_mut();
        if !wakes.iter().any(|wake| wake.will_wake(cx.waker())) {
            wakes.push(cx.waker().clone());
        }
        drop(wakes);
        match self.0.completion.borrow().as_ref() {
            Some(Err(error)) => Poll::Ready(Err(error.clone())),
            Some(Ok(())) if self.0.payload.borrow().is_some() => Poll::Ready(Ok(())),
            _ => Poll::Pending,
        }
    }
    /// Poll execution before subscribing to completion, avoiding a waiter
    /// allocation and a self-wake for a ready method. A method can publish an
    /// early pipeline during this poll and still return Pending without waking
    /// us. Preserve that notification explicitly; later publications use the
    /// registered waiter. No Results borrow spans application code.
    pub(super) fn poll_work<F: Future + Unpin>(
        &self,
        work: &mut F,
        cx: &mut Context<'_>,
    ) -> Poll<F::Output> {
        let had_pipeline = self.0.pipeline.borrow().is_some();
        let outcome = Pin::new(work).poll(cx);
        if outcome.is_pending() {
            let _ = self.poll_completed(cx);
            if !had_pipeline && self.0.pipeline.borrow().is_some() {
                cx.waker().wake_by_ref();
            }
        }
        outcome
    }
    pub fn failure(&self) -> Option<Error> {
        self.0
            .completion
            .borrow()
            .as_ref()
            .and_then(|r| r.as_ref().err())
            .cloned()
    }
    pub(super) fn plain_payload(&self) -> Option<Rc<Payload>> {
        if self.transfer_ready()
            || self.redirect().is_some()
            || self.0.pipeline.borrow().is_some()
            || !matches!(&*self.0.completion.borrow(), Some(Ok(())))
        {
            return None;
        }
        let payload = self.0.payload.borrow();
        let payload = payload.as_ref()?;
        (payload.caps.is_empty() && matches!(payload.body, PayloadBody::Return(_)))
            .then(|| payload.clone())
    }
    pub(in crate::scoped::wire) fn early_pipeline(&self) -> Option<Box<dyn PipelineHook>> {
        self.0
            .pipeline
            .borrow()
            .clone()
            .map(|pipeline| Box::new(EarlyPipeline(pipeline)) as _)
    }
    /// Conservative contiguous Return storage, including descriptors and error
    /// details. Checked before reserving the portable output buffer. This is a
    /// size hint, not an allocation or a publication of the application result.
    pub fn publication_bytes(&self) -> capnp::Result<usize> {
        use capnp::field_api::Schema;
        use capntproto_rpc_wire::rpc_capnp::api::{
            exception::Detail, CapDescriptor, Exception, Message, Payload, Return,
        };
        fn words<T: Schema>() -> u64 {
            u64::from(T::SIZE.data) + u64::from(T::SIZE.pointers)
        }
        let overflow = || Error::from_kind(capnp::ErrorKind::MessageSizeOverflow);
        // Header, root pointer, inline-composite tag, plus the transactional
        // payload-copy staging word (or the provisional empty exception reason).
        let mut size = 4 + words::<Message>() + words::<Return>();
        if let Some(error) = self.failure() {
            let reason = exception_reason(&error);
            size += words::<Exception>();
            size = size
                .checked_add((reason.len() as u64 + 8) / 8)
                .ok_or_else(overflow)?;
            if let Some(trace) = self.0.trace.borrow().as_ref() {
                size = size
                    .checked_add((trace.len() as u64 + 8) / 8)
                    .ok_or_else(overflow)?;
            }
            for (_, data) in error.details() {
                size = size
                    .checked_add(words::<Detail>())
                    .and_then(|n| n.checked_add((data.len() as u64).div_ceil(8)))
                    .ok_or_else(overflow)?;
            }
        } else {
            let payload = self
                .0
                .payload
                .borrow()
                .clone()
                .ok_or_else(|| Error::overloaded("server still owns its results".into()))?;
            if payload.caps.is_empty() {
                if let PayloadBody::Return(message) = &payload.body {
                    if let Some(bytes) = message.framed_len() {
                        // Whole-envelope copies need no payload traversal for
                        // sizing. Retain the general codec's two staging/tag
                        // words as well: transferred results can use that path.
                        // The driver still validates before publication.
                        return bytes.checked_add(16).ok_or_else(overflow);
                    }
                }
            }
            let content = payload.reader()?.target_size()?.word_count;
            let descriptors = (payload.caps.len() as u64)
                .checked_mul(words::<CapDescriptor>())
                .ok_or_else(overflow)?;
            size = size
                .checked_add(words::<Payload>())
                .and_then(|n| n.checked_add(content))
                .and_then(|n| n.checked_add(descriptors))
                .ok_or_else(overflow)?;
        }
        size.checked_mul(8)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(overflow)
    }

    pub(in crate::scoped::wire) fn cancel(&self) {
        self.0
            .complete(Err(Error::disconnected("server execution canceled".into())));
    }
    pub(crate) fn fail(&self, error: Error) {
        self.0.complete(Err(error));
    }
    /// Explicit pipelines win even if the final method fails. User hooks run
    /// without state borrows. None leaves the portable observer pending.
    pub fn pipeline(
        &self,
        path: capntproto_rpc_wire::pipeline::Path<'_>,
    ) -> Option<Box<dyn ClientHook>> {
        let pipeline = self.0.pipeline.borrow().clone();
        let ops: Vec<_> = path
            .fields()
            .iter()
            .map(|&field| PipelineOp::GetPointerField(field))
            .collect();
        if let Some(pipeline) = pipeline {
            return Some(pipeline.get_pipelined_cap(&ops));
        }
        if let Some(error) = self.failure() {
            return Some(crate::broken::new_cap(error));
        }
        if self.0.completion.borrow().is_none() {
            return None;
        }
        let payload = self.0.payload.borrow().clone()?;
        Some(
            payload
                .reader()
                .and_then(|reader| reader.get_pipelined_cap(&ops))
                .unwrap_or_else(crate::broken::new_cap),
        )
    }
    /// Preserve structured error details in a portable Return. Remote traces
    /// are not forwarded as a local trace; outgoing traces remain opt-in.
    pub fn write_exception(
        &self,
        mut output: capnp::field_api::FixedStruct<
            '_,
            capntproto_rpc_wire::rpc_capnp::api::Exception,
        >,
    ) -> capnp::Result<()> {
        use capntproto_rpc_wire::rpc_capnp::api::{
            exception::{Detail, Type},
            Exception,
        };
        let error = self
            .failure()
            .ok_or_else(|| Error::failed("service has no exception".into()))?;
        output.scalar(Exception::TYPE).set(match error.kind {
            capnp::ErrorKind::Disconnected => Type::Disconnected,
            capnp::ErrorKind::Overloaded => Type::Overloaded,
            capnp::ErrorKind::Unimplemented
            | capnp::ErrorKind::SettingDynamicCapabilitiesIsUnsupported => Type::Unimplemented,
            _ => Type::Failed,
        });
        let reason = exception_reason(&error);
        output.field(Exception::REASON).copy_from(reason.as_str())?;
        if let Some(trace) = self.0.trace.borrow().as_ref() {
            output.field(Exception::TRACE).copy_from(trace.as_str())?;
        }
        let mut details = output
            .field(Exception::DETAILS)
            .init(error.details().count())?;
        for (index, (id, data)) in error.details().enumerate() {
            let mut detail = details.get_mut(index).expect("allocated details");
            detail.scalar(Detail::DETAIL_ID).set(id);
            detail.field(Detail::DATA).copy_from(data)?;
        }
        Ok(())
    }
}

struct EarlyPipeline(Rc<dyn PipelineHook>);
impl PipelineHook for EarlyPipeline {
    fn add_ref(&self) -> Box<dyn PipelineHook> {
        Box::new(Self(self.0.clone()))
    }
    fn get_pipelined_cap(&self, ops: &[PipelineOp]) -> Box<dyn ClientHook> {
        self.0.get_pipelined_cap(ops)
    }
}

fn exception_reason(error: &Error) -> String {
    match error.kind {
        capnp::ErrorKind::Failed
        | capnp::ErrorKind::Disconnected
        | capnp::ErrorKind::Overloaded
        | capnp::ErrorKind::Unimplemented => error.extra.clone(),
        _ => error.to_string(),
    }
}

/// Poll with the connection's executor. Dropping interest cancels cancellable
/// methods; generated non-cancellable methods retain their independent executor
/// task and result context until actual completion.
#[must_use = "poll execution or explicitly cancel its interest"]
pub struct Execution {
    operation: Option<Promise<(), Error>>,
    results: Results,
}
impl Future for Execution {
    type Output = capnp::Result<()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Some(operation) = self.operation.as_mut() else {
            return Poll::Ready(
                self.results
                    .0
                    .completion
                    .borrow()
                    .as_ref()
                    .expect("completed execution")
                    .clone(),
            );
        };
        let result = match Pin::new(operation).poll(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(result) => result,
        };
        drop(self.operation.take());
        self.results.0.complete(result.clone());
        Poll::Ready(result)
    }
}
impl Drop for Execution {
    fn drop(&mut self) {
        if let Some(operation) = self.operation.take() {
            drop(operation);
            self.results
                .0
                .complete(Err(Error::disconnected("server execution canceled".into())));
        }
    }
}

struct Hook {
    results: Results,
    message: Option<Box<connection::PreparedReturn<capnp::message::HeapAllocator>>>,
    caps: Vec<Option<Box<dyn ClientHook>>>,
    executor: Option<Rc<dyn capability::CallExecutor>>,
}
impl Drop for Hook {
    fn drop(&mut self) {
        let caps = std::mem::take(&mut self.caps);
        let payload = Rc::new(match self.message.take() {
            Some(message) => Payload {
                body: PayloadBody::Return(message),
                caps,
            },
            None => Payload::owned(Default::default(), caps),
        });
        let previous = self.results.0.payload.borrow_mut().replace(payload);
        drop(previous);
        self.results.0.notify();
    }
}
impl ResultsHook for Hook {
    fn permits_immediate_poll(&self) -> bool {
        // Prepared::execute owns its parameters and holds no protocol-table
        // borrow. Pending protected methods still move to the supplied executor;
        // local dispatch establishes streaming barriers before polling those.
        true
    }
    fn retain_response_context(&mut self, context: Rc<dyn std::any::Any>) {
        crate::admission::retain_context(&mut self.results.0.context.borrow_mut(), context);
    }
    fn cancellation_guard(&self) -> Option<Box<dyn std::any::Any>> {
        Some(Box::new(self.results.clone()))
    }
    fn cancellation_executor(&self) -> Option<Rc<dyn capability::CallExecutor>> {
        self.executor.clone()
    }
    fn get_with_size_hint(
        &mut self,
        hint: Option<capnp::MessageSize>,
    ) -> capnp::Result<any_pointer::Builder<'_>> {
        let mut root: any_pointer::Builder = self
            .message
            .get_or_insert_with(|| {
                let hint = hint.map(|mut size| {
                    size.word_count = size
                        .word_count
                        .saturating_add(connection::PreparedReturnTemplate::ENVELOPE_WORDS);
                    size
                });
                let allocator = crate::local::result_allocator(hint);
                let allocator = match &self.results.0.message_pool {
                    Some(pool) => allocator.segment_pool(pool.clone()),
                    None => allocator,
                };
                thread_local! {
                    static TEMPLATE: connection::PreparedReturnTemplate = Default::default();
                }
                Box::new(TEMPLATE.with(|template| {
                    connection::PreparedReturn::from_template(template, allocator)
                }))
            })
            .results();
        root.imbue_mut(&mut self.caps);
        Ok(root)
    }
    fn set_pipeline(&mut self) -> capnp::Result<()> {
        let payload = Payload::copy(self.get()?.into_reader())?;
        self.set_pipeline_from(payload.into_pipeline())
    }
    fn set_pipeline_from(&mut self, pipeline: Box<dyn PipelineHook>) -> capnp::Result<()> {
        if self.results.0.pipeline.borrow().is_some() {
            return Err(Error::failed("set_pipeline() called twice".into()));
        }
        *self.results.0.pipeline.borrow_mut() = Some(Rc::from(pipeline));
        self.results.0.notify();
        Ok(())
    }
    fn allow_cancellation(&self) {
        // Generated dispatch supplies the service's cancellation policy.
    }
    fn tail_call(self: Box<Self>, request: Box<dyn RequestHook>) -> Promise<(), Error> {
        self.direct_tail_call(request).0
    }
    fn direct_tail_call(
        mut self: Box<Self>,
        mut request: Box<dyn RequestHook>,
    ) -> (Promise<(), Error>, Box<dyn PipelineHook>) {
        let origin = self.results.0.tail_origin.borrow().clone();
        if self.results.0.pipeline.borrow().is_none() {
            if let Some(origin) = origin {
                match origin.redirect(&mut *request) {
                    Ok(Some(redirect)) => {
                        let Some((_, completion, pipeline)) = request.tail_send() else {
                            let error =
                                Error::failed("tail request refused prepared transfer".into());
                            return (
                                Promise::err(error.clone()),
                                Box::new(crate::broken::Pipeline::new(error)),
                            );
                        };
                        let previous = self
                            .results
                            .0
                            .network
                            .borrow_mut()
                            .get_or_insert_with(Default::default)
                            .redirect
                            .replace(redirect);
                        drop(previous);
                        self.set_pipeline_from(pipeline.add_ref())
                            .expect("unpublished tail pipeline");
                        return (
                            Promise::from_future(async move {
                                let result = completion.await;
                                drop(self);
                                result
                            }),
                            pipeline,
                        );
                    }
                    Err(error) => {
                        return (
                            Promise::err(error.clone()),
                            Box::new(crate::broken::Pipeline::new(error)),
                        )
                    }
                    Ok(None) => (),
                }
            }
        }
        // Generic forwarding preserves the request's membrane and pipeline.
        // Same-connection wire tail transfer remains a separate optimization.
        let capability::RemotePromise { promise, pipeline } = request.send();
        if self.results.0.pipeline.borrow().is_none() {
            self.set_pipeline_from(pipeline.hook.add_ref())
                .expect("unpublished pipeline");
        }
        let completion = Promise::from_future(async move {
            let response = promise.await?;
            self.get()?.set_as(response.get()?)?;
            Ok(())
        });
        (completion, pipeline.hook)
    }
}

pub(crate) type Work =
    futures::future::Shared<futures::future::LocalBoxFuture<'static, capnp::Result<()>>>;
pub(crate) struct Transfer {
    pub results: Results,
    pub work: Work,
}
impl Transfer {
    pub(crate) fn answer(self) -> crate::third_party::Answer {
        let pipeline = Box::new(TransferPipeline {
            results: self.results.clone(),
            work: self.work.clone(),
        });
        crate::third_party::Answer {
            pipeline,
            response: Promise::from_future(async move {
                self.work.await?;
                futures::future::poll_fn(|cx| self.results.poll_completed(cx)).await?;
                let payload = self
                    .results
                    .0
                    .payload
                    .borrow()
                    .clone()
                    .expect("completed transfer payload");
                Ok(Box::new(TransferResponse {
                    payload,
                    _results: self.results,
                })
                    as Box<dyn capnp::private::capability::ResponseHook>)
            }),
        }
    }
}
struct TransferResponse {
    payload: Rc<Payload>,
    _results: Results,
}
impl capnp::private::capability::ResponseHook for TransferResponse {
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        self.payload.reader()
    }
}
struct TransferPipeline {
    results: Results,
    work: Work,
}
impl PipelineHook for TransferPipeline {
    fn add_ref(&self) -> Box<dyn PipelineHook> {
        Box::new(Self {
            results: self.results.clone(),
            work: self.work.clone(),
        })
    }
    fn get_pipelined_cap(&self, ops: &[PipelineOp]) -> Box<dyn ClientHook> {
        let fields: Vec<_> = ops
            .iter()
            .filter_map(|op| match op {
                PipelineOp::Noop => None,
                PipelineOp::GetPointerField(field) => Some(*field),
            })
            .collect();
        let results = self.results.clone();
        // Result ownership may arrive after execution. A Shared handle cannot
        // be polled again once it has returned Ready.
        let mut work = self.work.clone().fuse();
        let client: capability::Client =
            crate::new_future_client(futures::future::poll_fn(move |cx| {
                let _ = results.poll_work(&mut work, cx);
                match results.pipeline(pipeline::Path::new(&fields)) {
                    Some(client) => Poll::Ready(Ok(capability::Client::new(client))),
                    None => {
                        // Execution can finish while the application still
                        // owns the ResultsHook. Its eventual return, rather
                        // than the already-ready work, must wake this observer.
                        let _ = results.poll_completed(cx);
                        Poll::Pending
                    }
                }
            }));
        client.hook
    }
}
impl Prepared {
    /// Start once, without borrowing driver tables. The same Results handle can
    /// live in the answer producer and supplies its executor cancellation guard.
    pub fn execute(
        self,
        destination: &dyn ClientHook,
        results: &Results,
        executor: Option<Rc<dyn capability::CallExecutor>>,
    ) -> capnp::Result<Execution> {
        if results.0.method.get().is_some() {
            return Err(Error::failed("server results already started".into()));
        }
        results.0.method.set(Some(self.method));
        let hook = Hook {
            results: results.clone(),
            message: None,
            caps: Vec::new(),
            executor,
        };
        Ok(Execution {
            operation: Some(self.dispatch(destination, Box::new(hook))),
            results: results.clone(),
        })
    }
}

impl<'id> ClientHooks<'id> {
    /// Capacity failure leaves hosted results intact for retry. Foreign imports
    /// must retain the original hook with its policy membranes. Local failures
    /// remain on Results for full wire exception encoding.
    pub fn collect_server_results<
        'pool,
        I,
        A,
        P: Copy + Eq,
        S,
        const CAPS: usize,
        const PATH: usize,
    >(
        &mut self,
        results: &Results,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        options: capnp::message::ReaderOptions,
        import: impl FnMut(
            &mut connection::Runtime<'pool, 'id, '_, I, P, S, CAPS, PATH>,
            &dyn ClientHook,
        ) -> capnp::Result<Client<'id>>,
    ) -> capnp::Result<Result<connection::LocalResult<'pool, 'id, CAPS>, capnp::ErrorKind>> {
        self.collect_server_output(results, driver, messages, options, None, import)
    }

    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    pub(super) fn collect_server_output<
        'pool,
        I,
        A,
        P: Copy + Eq,
        S,
        const CAPS: usize,
        const PATH: usize,
    >(
        &mut self,
        results: &Results,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        options: capnp::message::ReaderOptions,
        answer: Option<capntproto_rpc_engine::answers::Key<'id>>,
        mut import: impl FnMut(
            &mut connection::Runtime<'pool, 'id, '_, I, P, S, CAPS, PATH>,
            &dyn ClientHook,
        ) -> capnp::Result<Client<'id>>,
    ) -> capnp::Result<Result<connection::LocalResult<'pool, 'id, CAPS>, capnp::ErrorKind>> {
        match results.0.completion.borrow().as_ref() {
            None => return Err(Error::overloaded("server results are pending".into())),
            Some(Err(error)) => return Ok(Err(error.kind)),
            Some(Ok(())) => (),
        }
        let method = results.0.method.get().expect("started results");
        let payload = results
            .0
            .payload
            .borrow()
            .clone()
            .ok_or_else(|| Error::overloaded("server still owns its results".into()))?;
        let mut retained = Vec::with_capacity(payload.caps.len());
        let result = (|| {
            if payload.caps.is_empty() {
                if let (Some(answer), PayloadBody::Return(source)) = (answer, &payload.body) {
                    if let Some(result) = driver
                        .local_answer_prepared(
                            answer,
                            method.interface,
                            method.method,
                            source,
                            messages,
                            options,
                        )
                        .map_err(wire_error)?
                    {
                        return Ok(result);
                    }
                }
            }
            if payload.caps.len() > driver.capability_capacity() {
                return Err(Error::overloaded(
                    "wire result capability capacity exhausted".into(),
                ));
            }
            for cap in &payload.caps {
                let client = match cap {
                    None => None,
                    Some(hook) => Some(match self.try_retain(&mut driver.runtime, &**hook)? {
                        Some(client) => client,
                        None => import(&mut driver.runtime, &**hook)?,
                    }),
                };
                retained.push(client);
            }
            let content = payload.reader()?;
            let minimum = content
                .target_size()?
                .word_count
                .checked_add(16)
                .and_then(|words| words.checked_mul(8))
                .and_then(|bytes| usize::try_from(bytes).ok())
                .ok_or_else(|| Error::failed("server result size overflow".into()))?;
            let references = retained.iter().map(Option::as_ref).collect::<Vec<_>>();
            let fill = |field: capnp::field_api::FixedField<
                '_,
                capnp::field_api::Generic<any_pointer::Owned>,
            >| {
                field.copy_with_capabilities(
                    content,
                    capnp::wire_capability::IndexMap::identity(payload.caps.len() as u32),
                )
            };
            let result = if let Some(answer) = answer {
                driver.local_answer_dynamic(
                    answer,
                    method.interface,
                    method.method,
                    &references,
                    messages,
                    minimum,
                    options,
                    fill,
                )
            } else {
                driver.local_result_dynamic(
                    method.interface,
                    method.method,
                    &references,
                    messages,
                    minimum,
                    options,
                    fill,
                )
            };
            result.map_err(wire_error)
        })();
        for client in retained.into_iter().flatten() {
            driver.runtime.release(client);
        }
        result.map(Ok)
    }
}
