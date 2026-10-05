use crate::fd::{AttachedFd, IncomingFds, OutgoingFds};
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

use capnp::capability::CallHints;
use std::pin::Pin;
use std::task::{Context, Poll};

use capnp::any_pointer;
use capnp::capability::Promise;
use capnp::private::capability::{
    ClientHook, ParamsHook, PipelineHook, PipelineOp, RequestHook, ResponseHook, ResultsHook,
};
use capnp::Error;

use futures::channel::oneshot;
use futures::{future, Future, FutureExt, TryFutureExt};

use std::cell::{Cell, RefCell};
use std::collections::hash_map::{self, HashMap};
use std::mem;
use std::rc::{Rc, Weak};

use crate::attach::Attach;
use crate::local::ResultsDoneHook;
use crate::rpc_capnp::{
    bootstrap, call, cap_descriptor, disembargo, exception, finish, message, message_target,
    payload, promised_answer, resolve, return_,
};
use crate::task_set::TaskSet;
use crate::{broken, local, queued};

#[path = "answer_adoption.rs"]
mod answer_adoption;
mod capabilities;
mod dispatch;
#[path = "join.rs"]
pub(crate) mod join;
#[path = "multiparty_join.rs"]
mod multiparty_join;

#[path = "rpc_ids.rs"]
mod rpc_ids;
#[path = "rpc_tables.rs"]
mod rpc_tables;
use rpc_ids::{AnswerId, EmbargoId, ExportId, ImportId, QuestionId, WireId};
use rpc_tables::{LocalTable, PeerTable};

struct Question<VatId>
where
    VatId: 'static,
{
    _permit: Option<Rc<crate::admission::Permit>>,
    is_awaiting_return: bool,

    #[allow(dead_code)]
    param_exports: Vec<ExportId>,

    #[allow(dead_code)]
    is_tail_call: bool,

    /// The local QuestionRef, set to None when it is destroyed.
    self_ref: Option<Weak<RefCell<QuestionRef<VatId>>>>,

    /// If true, don't send a Finish message.
    skip_finish: bool,
    join_id: Option<u32>,
    allow_third_party: bool,
    multiparty_join: bool,
}

impl<VatId> Question<VatId> {
    fn new() -> Self {
        Self {
            _permit: None,
            is_awaiting_return: true,
            param_exports: Vec::new(),
            is_tail_call: false,
            self_ref: None,
            skip_finish: false,
            join_id: None,
            allow_third_party: false,
            multiparty_join: false,
        }
    }
}

/// A reference to an entry on the question table.  Used to detect when the `Finish` message
/// can be sent.
struct QuestionRef<VatId>
where
    VatId: 'static,
{
    connection_state: Option<Rc<ConnectionState<VatId>>>,
    id: QuestionId,
    fulfiller: Option<oneshot::Sender<Promise<Response<VatId>, Error>>>,
    pipeline: Option<Weak<RefCell<PipelineState<VatId>>>>,
}

impl<VatId> QuestionRef<VatId> {
    fn new(
        state: Rc<ConnectionState<VatId>>,
        id: QuestionId,
        fulfiller: oneshot::Sender<Promise<Response<VatId>, Error>>,
    ) -> Self {
        Self {
            connection_state: Some(state),
            id,
            fulfiller: Some(fulfiller),
            pipeline: None,
        }
    }
    fn fulfill(&mut self, response: Promise<Response<VatId>, Error>) {
        if let Some(fulfiller) = self.fulfiller.take() {
            let _ = fulfiller.send(response);
        }
    }

    fn reject(&mut self, err: Error) {
        if let Some(fulfiller) = self.fulfiller.take() {
            let _ = fulfiller.send(Promise::err(err));
        }
    }
}

impl<VatId> Drop for QuestionRef<VatId> {
    fn drop(&mut self) {
        let Some(connection_state) = self.connection_state.take() else {
            return;
        };
        let mut questions = connection_state.questions.borrow_mut();
        let Some(q) = questions.find(self.id) else {
            unreachable!()
        };
        if let Ok(ref mut c) = *connection_state.connection.borrow_mut() {
            if !q.skip_finish {
                let mut message = c.new_outgoing_message(5);
                {
                    let root: message::Builder = message.get_body().unwrap().init_as();
                    let mut builder = root.init_finish();
                    builder.set_question_id(self.id.to_wire());
                    builder.set_require_early_cancellation_workaround(false);

                    // If we're still awaiting a return, then this request is being
                    // canceled, and we're going to ignore any capabilities in the return
                    // message, so set releaseResultCaps true. If we already received the
                    // return, then we've already built local proxies for the caps and will
                    // send Release messages when those are destroyed.
                    builder.set_release_result_caps(q.is_awaiting_return);
                }
                let _ = message.send_detached();
            }
        }

        if q.is_awaiting_return {
            // Still waiting for return, so just remove the QuestionRef pointer from the table.
            q.self_ref = None;
        } else {
            // Call has already returned, so we can now remove it from the table.
            questions.erase(self.id)
        }
        drop(questions);
        connection_state.schedule_idle_check();
    }
}

#[derive(Default)]
struct AnswerStatus {
    return_has_been_sent: Cell<bool>,
    received_finish: Cell<bool>,
}

struct Answer<VatId>
where
    VatId: 'static,
{
    status: Rc<AnswerStatus>,
    request_words: usize,
    pipeline_only_guard: Option<Rc<ReturnGuard<VatId>>>,

    // Send pipelined calls here.  Becomes null as soon as a `Finish` is received.
    pipeline: Option<Box<dyn PipelineHook>>,

    // For locally-redirected calls (Call.sendResultsTo.yourself), this is a promise for the call
    // result, to be picked up by a subsequent `Return`.
    redirected_results: Option<Promise<Response<VatId>, Error>>,

    call_completion_promise: Option<Promise<(), Error>>,

    // List of exports that were sent in the results.  If the finish has `releaseResultCaps` these
    // will need to be released.
    result_exports: Vec<ExportId>,
    provision: Option<(
        Rc<crate::third_party::ThirdPartyExchange>,
        Box<dyn std::any::Any>,
    )>,
    join: Option<join::PartGuard<VatId>>,
    callee_allocated: bool,
    join_response: Option<Box<dyn std::any::Any>>,
}

impl<VatId> Answer<VatId> {
    fn new() -> Self {
        Self {
            status: Rc::default(),
            request_words: 0,
            pipeline_only_guard: None,
            pipeline: None,
            redirected_results: None,
            call_completion_promise: None,
            result_exports: Vec::new(),
            provision: None,
            join: None,
            callee_allocated: false,
            join_response: None,
        }
    }
}

pub(crate) struct Export {
    refcount: u32,

    /// If true, this is the canonical export entry for this clientHook, that is,
    /// `exports_by_cap[clientHook]` points to this entry.
    canonical: bool,

    client_hook: Box<dyn ClientHook>,

    // If this export is a promise (not a settled capability), the `resolve_op` represents the
    // ongoing operation to wait for that promise to resolve and then send a `Resolve` message.
    resolve_op: Promise<(), Error>,
    vine: Option<Rc<dyn Fn(&[u8]) -> capnp::Result<()>>>,
    reflected_vine: Option<Rc<dyn Fn() -> Box<dyn ClientHook>>>,
}

impl Export {
    fn new(client_hook: Box<dyn ClientHook>) -> Self {
        Self {
            refcount: 1,
            canonical: false,
            client_hook,
            resolve_op: Promise::err(Error::failed("no resolve op".to_string())),
            vine: None,
            reflected_vine: None,
        }
    }
}

pub(crate) struct Import<VatId>
where
    VatId: 'static,
{
    import_client: Weak<RefCell<ImportClient<VatId>>>,

    // Either a copy of importClient, or, in the case of promises, the wrapping PromiseClient.
    // Becomes null when it is discarded *or* when the import is destroyed (e.g. the promise is
    // resolved and the import is no longer needed).
    app_client: Option<WeakClient<VatId>>,

    // If non-null, the import is a promise.
    promise_client_to_resolve: Option<Weak<RefCell<PromiseClient<VatId>>>>,
}

impl<VatId> Import<VatId> {
    fn new(import_client: &Rc<RefCell<ImportClient<VatId>>>) -> Self {
        Self {
            import_client: Rc::downgrade(import_client),
            app_client: None,
            promise_client_to_resolve: None,
        }
    }
}

struct Embargo {
    fulfiller: Option<oneshot::Sender<Result<(), Error>>>,
}

impl Embargo {
    fn new(fulfiller: oneshot::Sender<Result<(), Error>>) -> Self {
        Self {
            fulfiller: Some(fulfiller),
        }
    }
}

fn to_pipeline_ops(
    ops: ::capnp::struct_list::Reader<promised_answer::op::Owned>,
) -> ::capnp::Result<Vec<PipelineOp>> {
    let mut result = Vec::new();
    for op in ops {
        match op.which()? {
            promised_answer::op::Noop(()) => {
                result.push(PipelineOp::Noop);
            }
            promised_answer::op::GetPointerField(idx) => {
                result.push(PipelineOp::GetPointerField(idx));
            }
        }
    }
    Ok(result)
}

fn from_error(error: &Error, mut builder: exception::Builder, trace: Option<&str>) {
    if let Some(trace) = trace {
        builder.set_trace(trace);
    }
    let details: Vec<_> = error.details().collect();
    if !details.is_empty() {
        let mut output = builder.reborrow().init_details(details.len() as u32);
        for (index, (id, data)) in details.into_iter().enumerate() {
            let mut detail = output.reborrow().get(index as u32);
            detail.set_detail_id(id);
            detail.set_data(data);
        }
    }
    let typ = match error.kind {
        ::capnp::ErrorKind::Failed => exception::Type::Failed,
        ::capnp::ErrorKind::Overloaded => exception::Type::Overloaded,
        ::capnp::ErrorKind::Disconnected => exception::Type::Disconnected,
        ::capnp::ErrorKind::Unimplemented => exception::Type::Unimplemented,
        ::capnp::ErrorKind::SettingDynamicCapabilitiesIsUnsupported => {
            exception::Type::Unimplemented
        }
        _ => exception::Type::Failed,
    };
    builder.set_type(typ);
    match error.kind {
        ::capnp::ErrorKind::Failed
        | ::capnp::ErrorKind::Overloaded
        | ::capnp::ErrorKind::Disconnected
        | ::capnp::ErrorKind::Unimplemented => {
            builder.set_reason(&error.extra);
        }
        _ => {
            // There is extra information in `error.kind` that is not
            // captured by `typ`. We call `error.to_string()` to allow that
            // information to be recorded in the `reason` field.
            builder.set_reason(error.to_string());
        }
    }
}

fn remote_exception_to_error(exception: exception::Reader) -> Error {
    // Unknown enum values still carry a useful reason; clamp only their kind.
    let kind = match exception.get_type() {
        Ok(exception::Type::Overloaded) => capnp::ErrorKind::Overloaded,
        Ok(exception::Type::Disconnected) => capnp::ErrorKind::Disconnected,
        Ok(exception::Type::Unimplemented) => capnp::ErrorKind::Unimplemented,
        _ => capnp::ErrorKind::Failed,
    };
    let mut error = Error::from_kind(kind);
    error.extra = match exception.get_reason() {
        Ok(reason) => {
            let reason = reason
                .to_str()
                .unwrap_or("<malformed utf-8 in error reason>");
            if reason.starts_with("remote exception: ") {
                reason.to_string()
            } else {
                format!("remote exception: {reason}")
            }
        }
        Err(_) => "remote exception: (malformed error)".into(),
    };
    // Malformed diagnostic pointers must not hide the original RPC failure.
    if exception.has_trace() {
        if let Ok(trace) = exception
            .get_trace()
            .and_then(|trace| trace.to_str().map_err(Into::into))
        {
            error.set_remote_trace(trace.to_string());
        }
    }
    if let Ok(details) = exception.get_details() {
        for detail in details {
            if detail.has_data() {
                if let Ok(data) = detail.get_data() {
                    error.set_detail(detail.get_detail_id(), data.to_vec());
                }
            }
        }
    }
    error
}

pub(crate) struct ConnectionErrorHandler<VatId>
where
    VatId: 'static,
{
    weak_state: Weak<ConnectionState<VatId>>,
}

impl<VatId> ConnectionErrorHandler<VatId> {
    fn new(weak_state: Weak<ConnectionState<VatId>>) -> Self {
        Self { weak_state }
    }
}

impl<VatId> crate::task_set::TaskReaper<capnp::Error> for ConnectionErrorHandler<VatId> {
    fn task_failed(&mut self, error: ::capnp::Error) {
        if let Some(state) = self.weak_state.upgrade() {
            state.disconnect(error)
        }
    }
}

pub(crate) struct ConnectionState<VatId>
where
    VatId: 'static,
{
    bootstrap: crate::Bootstrap<VatId>,
    weak_self: Weak<Self>,
    connection_id: usize,
    idle: Cell<bool>,
    idle_check_queued: Cell<bool>,
    read_canceler: RefCell<Option<future::AbortHandle>>,
    exports: RefCell<LocalTable<ExportId, Export>>,
    questions: RefCell<LocalTable<QuestionId, Question<VatId>>>,
    got_return_for_high_id: Cell<bool>,
    answers: RefCell<PeerTable<AnswerId, Answer<VatId>>>,
    joins: RefCell<join::Table<VatId>>,
    next_adopted_answer: Cell<u32>,
    imports: RefCell<PeerTable<ImportId, Import<VatId>>>,

    /// Exports keyed by ClientHook::get_ptr().
    exports_by_cap: RefCell<HashMap<usize, ExportId>>,

    embargoes: RefCell<LocalTable<EmbargoId, Embargo>>,

    tasks: RefCell<Option<crate::task_set::TaskSetHandle<capnp::Error>>>,
    connection: RefCell<::std::result::Result<Box<dyn crate::Connection<VatId>>, ::capnp::Error>>,
    disconnect_fulfiller: RefCell<Option<oneshot::Sender<Promise<(), Error>>>>,

    client_downcast_map: RefCell<HashMap<usize, WeakClient<VatId>>>,
    registry: Weak<RefCell<ConnectionRegistry<VatId>>>,
    system_tasks: crate::task_set::TaskSetHandle<Error>,
    call_executor: Rc<dyn capnp::capability::CallExecutor>,
    flow_limit: Cell<usize>,
    admission: Rc<crate::admission::Admission>,
    held_responses: Cell<usize>,
    call_words: Cell<usize>,
    flow_waiter: RefCell<Option<oneshot::Sender<()>>>,
}

impl<VatId> ConnectionState<VatId> {
    pub(crate) fn new(
        bootstrap: crate::Bootstrap<VatId>,
        connection: Box<dyn crate::Connection<VatId>>,
        disconnect_fulfiller: oneshot::Sender<Promise<(), Error>>,
        registry: Weak<RefCell<ConnectionRegistry<VatId>>>,
        system_tasks: crate::task_set::TaskSetHandle<Error>,
        flow_limit: usize,
        outgoing_call_limit: usize,
    ) -> (TaskSet<Error>, Rc<Self>) {
        let connection_id = connection.connection_id();
        let write_finished = connection.when_write_finished();
        let state = Rc::new_cyclic(|weak| Self {
            bootstrap,
            weak_self: weak.clone(),
            connection_id,
            idle: Cell::new(true),
            idle_check_queued: Cell::new(false),
            read_canceler: RefCell::new(None),
            exports: RefCell::new(LocalTable::new()),
            questions: RefCell::new(LocalTable::new()),
            got_return_for_high_id: Cell::new(false),
            answers: RefCell::new(PeerTable::new()),
            joins: RefCell::new(join::Table::default()),
            next_adopted_answer: Cell::new(0),
            imports: RefCell::new(PeerTable::new()),
            exports_by_cap: RefCell::new(HashMap::new()),
            embargoes: RefCell::new(LocalTable::new()),
            tasks: RefCell::new(None),
            connection: RefCell::new(Ok(connection)),
            disconnect_fulfiller: RefCell::new(Some(disconnect_fulfiller)),
            client_downcast_map: RefCell::new(HashMap::new()),
            registry,
            call_executor: Rc::new(crate::CallTaskExecutor(system_tasks.clone())),
            system_tasks,
            flow_limit: Cell::new(flow_limit),
            admission: crate::admission::Admission::new(outgoing_call_limit),
            held_responses: Cell::new(0),
            call_words: Cell::new(0),
            flow_waiter: RefCell::new(None),
        });
        let (mut handle, tasks) =
            TaskSet::new(Box::new(ConnectionErrorHandler::new(Rc::downgrade(&state))));

        state.set_not_idle();
        handle.add(Self::message_loop(Rc::downgrade(&state)));
        if let Some(write_finished) = write_finished {
            handle.add(write_finished);
        }
        *state.tasks.borrow_mut() = Some(handle);
        (tasks, state)
    }

    pub(crate) fn set_outgoing_call_limit(&self, calls: usize) {
        self.admission.limit.set(calls);
    }

    pub(crate) fn snapshot(&self) -> crate::ConnectionSnapshot {
        crate::ConnectionSnapshot {
            connection_id: self.connection_id,
            outgoing_call_limit: self.admission.limit.get(),
            outgoing_calls: self.admission.used.get(),
            questions: self.questions.borrow().len(),
            answers: self.answers.borrow().slots.len(),
            imports: self.imports.borrow().slots.len(),
            exports: self.exports.borrow().len(),
            embargoes: self.embargoes.borrow().len(),
            incoming_call_words: self.call_words.get(),
            held_responses: self.held_responses.get(),
        }
    }

    fn send_call(
        &self,
        mut message: Box<dyn crate::OutgoingMessage>,
        permit: Rc<crate::admission::Permit>,
    ) {
        // The transport owns the message even if all application owners vanish.
        // In particular, pipeline-only questions can finish before this flush.
        match message.retain_until_sent(permit) {
            Ok(()) => drop(message.send_detached()),
            Err(permit) => {
                let (completion, _) = message.send();
                self.add_task(Promise::from_future(completion.attach(permit)));
            }
        }
    }

    pub(crate) fn set_not_idle(&self) {
        if self.idle.replace(false) {
            if let Ok(connection) = self.connection.borrow_mut().as_mut() {
                connection.set_idle(false);
            }
        }
    }

    fn all_tables_empty(&self) -> bool {
        self.questions.borrow().is_empty()
            && self.answers.borrow().slots.is_empty()
            && self.imports.borrow().slots.is_empty()
            && self.exports.borrow().is_empty()
            && self.embargoes.borrow().is_empty()
    }

    fn schedule_idle_check(&self) {
        if self.idle.get() || self.idle_check_queued.get() {
            return;
        }
        // Like C++'s checkIfBecameIdle(), reject a known-active connection
        // before scheduling work. Releasing the last import/export requests a
        // fresh check. Use fallible borrows because capability destructors can
        // reach here during table mutation. If neither table proves activity,
        // keep the deferred path instead of invoking transport callbacks here.
        if self
            .imports
            .try_borrow()
            .is_ok_and(|imports| !imports.slots.is_empty())
            || self
                .exports
                .try_borrow()
                .is_ok_and(|exports| !exports.is_empty())
        {
            return;
        }
        self.idle_check_queued.set(true);
        let weak = self.weak_self.clone();
        self.add_task(async move {
            if let Some(state) = weak.upgrade() {
                state.idle_check_queued.set(false);
                if !state.idle.get() && state.all_tables_empty() {
                    if let Ok(connection) = state.connection.borrow_mut().as_mut() {
                        state.idle.set(true);
                        connection.set_idle(true);
                    }
                }
            }
            Ok(())
        });
    }

    fn detach_from_registry(&self) {
        if let Some(registry) = self.registry.upgrade() {
            let removed = {
                let mut registry = registry.borrow_mut();
                let removed = registry.states.remove(&self.connection_id);
                if removed.is_some() {
                    registry.closing_connections += 1;
                }
                removed
            };
            drop(removed);
        }
    }

    fn cancel_read(&self) {
        let canceler = self.read_canceler.borrow_mut().take();
        if let Some(canceler) = canceler {
            canceler.abort();
        }
    }

    fn idle_eof(&self) {
        debug_assert!(self.all_tables_empty());
        let old = mem::replace(
            &mut *self.connection.borrow_mut(),
            Err(Error::disconnected(
                "Peer disconnected idle session.".into(),
            )),
        );
        let Ok(mut connection) = old else {
            return;
        };
        self.detach_from_registry();
        self.cancel_read();
        let shutdown = connection.shutdown(Ok(()));
        if let Some(fulfiller) = self.disconnect_fulfiller.borrow_mut().take() {
            let _ = fulfiller.send(Promise::from_future(shutdown.attach(connection)));
        }
    }

    fn encode_trace(&self, error: &Error) -> Option<String> {
        // Snapshot the callback without retaining the registry borrow while
        // application code runs. Existing and introduced connections share it.
        let encoder = self
            .registry
            .upgrade()
            .and_then(|registry| registry.borrow().trace_encoder.clone());
        encoder.map(|encode| encode(error))
    }

    pub(crate) fn set_flow_limit(&self, words: usize) {
        self.flow_limit.set(words);
        self.maybe_unblock_flow();
    }

    fn maybe_unblock_flow(&self) {
        if self.call_words.get() < self.flow_limit.get() {
            let waiter = self.flow_waiter.borrow_mut().take();
            if let Some(waiter) = waiter {
                let _ = waiter.send(());
            }
        }
    }

    fn new_outgoing_message(
        &self,
        first_segment_words: u32,
    ) -> capnp::Result<Box<dyn crate::OutgoingMessage>> {
        self.set_not_idle();
        match self.connection.borrow_mut().as_mut() {
            Err(e) => Err(e.clone()),
            Ok(c) => Ok(c.new_outgoing_message(first_segment_words)),
        }
    }

    pub(crate) fn disconnect(&self, error: ::capnp::Error) {
        if self.connection.borrow().is_err() {
            // Already disconnected.
            return;
        }

        // Application calls observe a lost connection even when its original
        // failure was a protocol error or an Abort with another error kind.
        // Clone the complete error so remote diagnostics and details survive.
        let mut network_error = error.clone();
        network_error.kind = capnp::ErrorKind::Disconnected;

        // Publish the failure before any capability destructor, promise resolution,
        // trace encoder, or network callback can re-enter RPC. C++ does this before
        // touching its tables as well. Only this stack frame owns the dying transport.
        let connection = mem::replace(
            &mut *self.connection.borrow_mut(),
            Err(network_error.clone()),
        );
        let Ok(mut c) = connection else {
            unreachable!()
        };
        self.detach_from_registry();
        self.cancel_read();

        // Detach tables before invoking user code. In particular, rejecting a
        // promise or dropping an export can release imports and QuestionRefs.
        let questions = mem::replace(&mut *self.questions.borrow_mut(), LocalTable::new());
        for question in questions.iter() {
            if let Some(reference) = question.self_ref.as_ref().and_then(Weak::upgrade) {
                let mut reference = reference.borrow_mut();
                reference.connection_state.take();
                reference.reject(network_error.clone());
            }
        }
        let answers_to_release = mem::take(&mut self.answers.borrow_mut().slots);
        for answer in answers_to_release.values() {
            answer.status.received_finish.set(true);
        }
        let exports_to_release = mem::replace(&mut *self.exports.borrow_mut(), LocalTable::new());
        self.exports_by_cap.borrow_mut().clear();
        let imports = self
            .imports
            .borrow_mut()
            .slots
            .values_mut()
            .filter_map(|import| import.promise_client_to_resolve.take())
            .collect::<Vec<_>>();
        for import in imports {
            if let Some(promise) = import.upgrade() {
                promise.borrow_mut().resolve(Err(network_error.clone()));
            }
        }
        let mut embargoes = mem::replace(&mut *self.embargoes.borrow_mut(), LocalTable::new());
        for embargo in embargoes.iter_mut() {
            if let Some(fulfiller) = embargo.fulfiller.take() {
                let _ = fulfiller.send(Err(network_error.clone()));
            }
        }
        drop(exports_to_release);
        // Includes redirected results and tail-call completion ownership.
        drop(answers_to_release);

        // Idle is a promise to the transport that no further messages will be
        // sent without new traffic/network activity. Explicit disconnect and
        // receive errors must honor it as well as orderly EOF.
        if !self.idle.get() {
            let trace = self.encode_trace(&error);
            // Abort is best effort; a transport that cannot allocate its body
            // must not prevent shutdown and release of the remaining owners.
            let mut message = c.new_outgoing_message(100);
            if let Ok(body) = message.get_body() {
                from_error(
                    &error,
                    body.init_as::<message::Builder>().init_abort(),
                    trace.as_deref(),
                );
                let _ = message.send_detached();
            }
        }

        self.call_words.set(0);
        let flow_waiter = self.flow_waiter.borrow_mut().take();
        drop(flow_waiter);

        let promise = c.shutdown(Err(network_error)).then(|r| match r {
            Ok(()) => Promise::ok(()),
            Err(e) => {
                if e.kind != ::capnp::ErrorKind::Disconnected {
                    // Don't report disconnects as an error.
                    Promise::err(e)
                } else {
                    Promise::ok(())
                }
            }
        });
        let Some(fulfiller) = self.disconnect_fulfiller.borrow_mut().take() else {
            unreachable!()
        };
        let _ = fulfiller.send(Promise::from_future(promise.attach(c)));
    }

    // Transform a future into a promise that gets executed even if it is never polled.
    // Dropping the returned promise cancels the computation.
    fn eagerly_evaluate<T, F>(&self, task: F) -> Promise<T, Error>
    where
        F: Future<Output = Result<T, Error>> + 'static + Unpin,
        T: 'static,
    {
        let (tx, rx) = oneshot::channel::<Result<T, Error>>();
        let (tx2, rx2) = oneshot::channel::<()>();
        let f1 = Box::pin(task.map(move |r| {
            let _ = tx.send(r);
        })) as Pin<Box<dyn Future<Output = ()> + Unpin>>;
        let f2 = Box::pin(rx2.map(drop)) as Pin<Box<dyn Future<Output = ()> + Unpin>>;

        self.add_task(future::select(f1, f2).map(|_| Ok(())));
        Promise::from_future(rx.map_err(crate::canceled_to_error).map(|r| {
            drop(tx2);
            r?
        }))
    }

    fn add_task<F>(&self, task: F)
    where
        F: Future<Output = Result<(), Error>> + 'static,
    {
        if let Some(ref mut tasks) = *self.tasks.borrow_mut() {
            tasks.add(task);
        }
    }

    pub(crate) fn bootstrap(state: &Rc<Self>) -> Box<dyn ClientHook> {
        let question_id = state.questions.borrow_mut().push(Question::new());

        let (fulfiller, promise) = oneshot::channel();
        let promise = promise.map_err(crate::canceled_to_error);
        let promise = promise.and_then(|response_promise| response_promise);
        let question_ref = Rc::new(RefCell::new(QuestionRef::new(
            state.clone(),
            question_id,
            fulfiller,
        )));
        let promise = promise.attach(question_ref.clone());
        match state.questions.borrow_mut().find(question_id) {
            Some(ref mut q) => {
                q.self_ref = Some(Rc::downgrade(&question_ref));
            }
            None => unreachable!(),
        }
        match *state.connection.borrow_mut() {
            Ok(ref mut c) => {
                let mut message = c.new_outgoing_message(5);
                {
                    let mut builder = message
                        .get_body()
                        .unwrap()
                        .init_as::<message::Builder>()
                        .init_bootstrap();
                    builder.set_question_id(question_id.to_wire());
                }
                let _ = message.send_detached();
            }
            Err(_) => panic!(),
        }

        let pipeline = Pipeline::new(state, question_ref, Some(Promise::from_future(promise)));
        pipeline.get_pipelined_cap_move(Vec::new())
    }

    fn message_loop(weak_state: Weak<Self>) -> Promise<(), capnp::Error> {
        let Some(state) = weak_state.upgrade() else {
            return Promise::err(Error::disconnected(
                "message loop cannot continue without a connection".into(),
            ));
        };
        // One cancellation registration covers transport input for this
        // connection. It never cancels protected calls or delivered responses.
        let (canceler, registration) = future::AbortHandle::new_pair();
        *state.read_canceler.borrow_mut() = Some(canceler);
        drop(state);
        let read = async move {
            let mut budget = 0;
            loop {
                let Some(state) = weak_state.upgrade() else {
                    return Ok(());
                };
                if state.connection.borrow().is_ok()
                    && state.call_words.get() > state.flow_limit.get()
                {
                    let (sender, receiver) = oneshot::channel();
                    *state.flow_waiter.borrow_mut() = Some(sender);
                    drop(state);
                    let _ = receiver.await;
                    continue;
                }
                let promise = match *state.connection.borrow_mut() {
                    Err(_) => return Ok(()),
                    Ok(ref mut connection) => connection.receive_incoming_message(),
                };
                // Waiting for input must not retain the connection owner.
                drop(state);
                match promise.await? {
                    Some(message) => Self::handle_message(&weak_state, message)?,
                    None => {
                        if let Some(state) = weak_state.upgrade() {
                            if state.idle.get() && state.connection.borrow().is_ok() {
                                state.idle_eof();
                            } else {
                                state.disconnect(Error::disconnected("Peer disconnected.".into()));
                            }
                        }
                        return Ok(());
                    }
                }
                budget += 1;
                if budget == 32 {
                    budget = 0;
                    // A continuously readable peer cannot monopolize the RPC
                    // task set. Resume through its normal ready queue.
                    let mut yielded = false;
                    future::poll_fn(move |cx| {
                        if yielded {
                            Poll::Ready(())
                        } else {
                            yielded = true;
                            cx.waker().wake_by_ref();
                            Poll::Pending
                        }
                    })
                    .await;
                }
            }
        };
        Promise::from_future(async move {
            match future::Abortable::new(read, registration).await {
                Ok(result) => result,
                Err(_) => Ok(()),
            }
        })
    }

    fn acknowledge_redirected_answer(
        &self,
        id: AnswerId,
        responded: &Cell<bool>,
    ) -> capnp::Result<()> {
        if !responded.get() {
            let mut acknowledgement = self.new_outgoing_message(8)?;
            let mut ret = acknowledgement
                .get_body()?
                .init_as::<message::Builder>()
                .init_return();
            ret.set_answer_id(id.to_wire());
            ret.set_release_param_caps(false);
            ret.set_results_sent_elsewhere(());
            let _ = acknowledgement.send_detached();
            self.answer_has_sent_return(id, vec![]);
        }
        Ok(())
    }

    fn answer_has_sent_return(&self, id: AnswerId, result_exports: Vec<ExportId>) {
        let (removed, words) = {
            let mut answers = self.answers.borrow_mut();
            let hash_map::Entry::Occupied(mut entry) = answers.slots.entry(id) else {
                // Disconnect already removed every answer. A retained call
                // context can outlive the connection but cannot send more wire data.
                debug_assert!(self.connection.borrow().is_err());
                return;
            };
            let answer = entry.get_mut();
            answer.status.return_has_been_sent.set(true);
            let words = mem::take(&mut answer.request_words);
            if answer.status.received_finish.get() {
                (Some(entry.remove()), words)
            } else {
                answer.result_exports = result_exports;
                (None, words)
            }
        };
        self.call_words.set(self.call_words.get() - words);
        self.maybe_unblock_flow();
        drop(removed);
        self.schedule_idle_check();
    }

    fn release_export(&self, id: ExportId, refcount: u32) -> ::capnp::Result<()> {
        let mut exports = self.exports.borrow_mut();
        let Some(e) = exports.find(id) else {
            return Err(Error::failed(
                "Tried to release invalid export ID.".to_string(),
            ));
        };
        if refcount > e.refcount {
            return Err(Error::failed(
                "Tried to drop export's refcount below zero.".to_string(),
            ));
        }
        e.refcount -= refcount;
        if e.refcount == 0 {
            let client_ptr = e.client_hook.get_ptr();
            if e.canonical {
                self.exports_by_cap.borrow_mut().remove(&client_ptr);
            }
            let released = exports.remove(id);
            drop(exports);
            drop(released);
            self.schedule_idle_check();
        }
        Ok(())
    }

    fn release_exports(&self, exports: &[ExportId]) -> ::capnp::Result<()> {
        for &export_id in exports {
            self.release_export(export_id, 1)?;
        }
        Ok(())
    }

    fn get_brand(&self) -> usize {
        self as *const _ as usize
    }
}

pub(crate) struct JoinContext<VatId> {
    pub(crate) bootstrap: crate::Bootstrap<VatId>,
    pub(crate) tasks: crate::task_set::TaskSetHandle<Error>,
}

pub(crate) struct ConnectionRegistry<VatId: 'static> {
    pub(crate) join_context: Weak<JoinContext<VatId>>,
    pub(crate) join_network: Option<Rc<dyn crate::multiparty::JoinNetwork<VatId>>>,
    pub(crate) flow_limit: usize,
    pub(crate) outgoing_call_limit: usize,
    pub(crate) trace_encoder: Option<Rc<dyn Fn(&Error) -> String>>,
    pub(crate) states: HashMap<usize, Rc<ConnectionState<VatId>>>,
    pub(crate) closing: bool,
    pub(crate) closing_connections: usize,
    pub(crate) waker: Option<std::task::Waker>,
}
impl<VatId> ConnectionRegistry<VatId> {
    pub(crate) fn new() -> Self {
        Self {
            join_context: Weak::new(),
            join_network: None,
            flow_limit: usize::MAX,
            outgoing_call_limit: usize::MAX,
            trace_encoder: None,
            states: HashMap::new(),
            closing: false,
            closing_connections: 0,
            waker: None,
        }
    }
}

/// Closes every connection in an RPC system and waits for outgoing shutdowns.
pub struct Disconnector<VatId: 'static> {
    connections: Rc<RefCell<ConnectionRegistry<VatId>>>,
}
impl<VatId> Disconnector<VatId> {
    pub(crate) fn new(connections: Rc<RefCell<ConnectionRegistry<VatId>>>) -> Self {
        Self { connections }
    }
}
impl<VatId: 'static> Future for Disconnector<VatId> {
    type Output = Result<(), capnp::Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let states = {
            let mut registry = self.connections.borrow_mut();
            if registry.states.is_empty() && registry.closing_connections == 0 {
                registry.closing = true;
                return Poll::Ready(Ok(()));
            }
            registry.waker = Some(cx.waker().clone());
            if registry.closing {
                return Poll::Pending;
            }
            registry.closing = true;
            registry.states.values().cloned().collect::<Vec<_>>()
        };
        // Destructors may re-enter the system; release the registry borrow first.
        for state in states {
            state.disconnect(Error::disconnected("client requested disconnect".into()));
        }
        Poll::Pending
    }
}

struct ResponseState<VatId>
where
    VatId: 'static,
{
    _connection_state: Rc<ConnectionState<VatId>>,
    message: Box<dyn crate::IncomingMessage>,
    cap_table: Vec<Option<Box<dyn ClientHook>>>,
    _question_ref: Rc<RefCell<QuestionRef<VatId>>>,
}

impl<VatId> Drop for ResponseState<VatId> {
    fn drop(&mut self) {
        let count = &self._connection_state.held_responses;
        count.set(count.get() - 1);
    }
}

enum ResponseVariant<VatId>
where
    VatId: 'static,
{
    Rpc(ResponseState<VatId>),
    LocallyRedirected(Box<dyn ResultsDoneHook>),
    Adopted(Box<dyn ResponseHook>, Box<dyn std::any::Any>),
}

struct Response<VatId>
where
    VatId: 'static,
{
    variant: Rc<ResponseVariant<VatId>>,
}

impl<VatId> Response<VatId> {
    fn new(
        connection_state: Rc<ConnectionState<VatId>>,
        question_ref: Rc<RefCell<QuestionRef<VatId>>>,
        message: Box<dyn crate::IncomingMessage>,
        cap_table_array: Vec<Option<Box<dyn ClientHook>>>,
    ) -> Self {
        connection_state
            .held_responses
            .set(connection_state.held_responses.get() + 1);
        Self {
            variant: Rc::new(ResponseVariant::Rpc(ResponseState {
                _connection_state: connection_state,
                message,
                cap_table: cap_table_array,
                _question_ref: question_ref,
            })),
        }
    }
    fn redirected(results_done: Box<dyn ResultsDoneHook>) -> Self {
        Self {
            variant: Rc::new(ResponseVariant::LocallyRedirected(results_done)),
        }
    }
}

impl<VatId> Clone for Response<VatId> {
    fn clone(&self) -> Self {
        Self {
            variant: self.variant.clone(),
        }
    }
}

impl<VatId> ResponseHook for Response<VatId> {
    fn get(&self) -> ::capnp::Result<any_pointer::Reader<'_>> {
        match *self.variant {
            ResponseVariant::Rpc(ref state) => {
                match state
                    .message
                    .get_body()?
                    .get_as::<message::Reader>()?
                    .which()?
                {
                    message::Return(Ok(ret)) => match ret.which()? {
                        return_::Results(Ok(mut payload)) => {
                            use ::capnp::traits::Imbue;
                            payload.imbue(&state.cap_table);
                            Ok(payload.get_content())
                        }
                        _ => unreachable!(),
                    },
                    _ => unreachable!(),
                }
            }
            ResponseVariant::LocallyRedirected(ref results_done) => results_done.get(),
            ResponseVariant::Adopted(ref response, ref _guard) => response.get(),
        }
    }
}

// A streaming controller owns the send operation. Retain admission through the
// write receipt even when that controller discards its copy of the receipt.
struct StreamingMessage<VatId: 'static> {
    message: Box<dyn crate::OutgoingMessage>,
    state: Rc<ConnectionState<VatId>>,
    permit: Rc<crate::admission::Permit>,
}
impl<VatId> crate::OutgoingMessage for StreamingMessage<VatId> {
    #[cfg(unix)]
    fn set_fds(&mut self, fds: Vec<Rc<std::os::fd::OwnedFd>>) {
        self.message.set_fds(fds);
    }
    fn get_body(&mut self) -> capnp::Result<any_pointer::Builder<'_>> {
        self.message.get_body()
    }
    fn get_body_as_reader(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        self.message.get_body_as_reader()
    }
    fn size_in_words(&self) -> usize {
        self.message.size_in_words()
    }
    fn take(self: Box<Self>) -> capnp::message::Builder<capnp::message::HeapAllocator> {
        self.message.take()
    }
    fn send(
        self: Box<Self>,
    ) -> (
        Promise<(), Error>,
        Rc<capnp::message::Builder<capnp::message::HeapAllocator>>,
    ) {
        let (completion, message) = self.message.send();
        let completion = completion.shared();
        self.state
            .add_task(Promise::from_future(completion.clone().attach(self.permit)));
        (Promise::from_future(completion), message)
    }
}

struct Request<VatId>
where
    VatId: 'static,
{
    connection_state: Rc<ConnectionState<VatId>>,
    permit: Rc<crate::admission::Permit>,
    target: Client<VatId>,
    message: Box<dyn crate::OutgoingMessage>,
    cap_table: Vec<Option<Box<dyn ClientHook>>>,
}

fn get_call(message: &mut Box<dyn crate::OutgoingMessage>) -> ::capnp::Result<call::Builder<'_>> {
    let message_root: message::Builder = message.get_body()?.get_as()?;
    match message_root.which()? {
        message::Call(call) => call,
        _ => Err(Error::failed("request does not contain Call".into())),
    }
}

impl<VatId> Request<VatId>
where
    VatId: 'static,
{
    fn new(
        connection_state: Rc<ConnectionState<VatId>>,
        size_hint: Option<::capnp::MessageSize>,
        target: Client<VatId>,
    ) -> ::capnp::Result<Self> {
        let permit = connection_state.admission.reserve()?;
        use capnp::traits::HasStructSize;
        let envelope = 1
            + message::Builder::STRUCT_SIZE.total()
            + call::Builder::STRUCT_SIZE.total()
            + message_target::Builder::STRUCT_SIZE.total()
            + promised_answer::Builder::STRUCT_SIZE.total()
            + payload::Builder::STRUCT_SIZE.total()
            + 1; // inline-composite tag for a promised-answer transform list
        let words = size_hint.map_or(256, |size| payload_size_hint(size, envelope));
        let message = connection_state.new_outgoing_message(words)?;
        Ok(Self {
            connection_state,
            permit,
            target,
            message,
            cap_table: Vec::new(),
        })
    }

    fn init_call(&mut self) -> call::Builder<'_> {
        let message_root: message::Builder = self.message.get_body().unwrap().get_as().unwrap();
        let mut call = message_root.init_call();
        call.set_allow_third_party_tail_call(answer_adoption::supported(&self.connection_state));
        call
    }

    fn send_internal(
        connection_state: &Rc<ConnectionState<VatId>>,
        mut message: Box<dyn crate::OutgoingMessage>,
        cap_table: &[Option<Box<dyn ClientHook>>],
        is_tail_call: bool,
        permit: Rc<crate::admission::Permit>,
    ) -> (
        Rc<RefCell<QuestionRef<VatId>>>,
        Promise<Response<VatId>, Error>,
    ) {
        // Build the cap table.
        let mut fds = OutgoingFds::default();
        let exports = ConnectionState::write_descriptors(
            connection_state,
            cap_table,
            get_call(&mut message).unwrap().get_params().unwrap(),
            &mut fds,
        );
        fds.attach(&mut *message);

        // Init the question table.  Do this after writing descriptors to avoid interference.
        let mut question = Question::<VatId>::new();
        question._permit = Some(permit.clone());
        question.is_awaiting_return = true;
        question.param_exports = exports;
        question.is_tail_call = is_tail_call;
        question.allow_third_party = get_call(&mut message)
            .unwrap()
            .get_allow_third_party_tail_call();

        let question_id = connection_state.questions.borrow_mut().push(question);
        {
            let mut call_builder: call::Builder = get_call(&mut message).unwrap();
            // Finish and send.
            call_builder
                .reborrow()
                .set_question_id(question_id.to_wire());
            if is_tail_call
                && !matches!(
                    call_builder.reborrow().get_send_results_to().which(),
                    Ok(call::send_results_to::ThirdParty(_))
                )
            {
                call_builder.get_send_results_to().set_yourself(());
            }
        }
        connection_state.send_call(message, permit);
        // Make the result promise.
        let (fulfiller, promise) = oneshot::channel::<Promise<Response<VatId>, Error>>();
        let promise = promise.map_err(crate::canceled_to_error).and_then(|x| x);
        let question_ref = Rc::new(RefCell::new(QuestionRef::new(
            connection_state.clone(),
            question_id,
            fulfiller,
        )));

        match connection_state.questions.borrow_mut().find(question_id) {
            Some(ref mut q) => {
                q.self_ref = Some(Rc::downgrade(&question_ref));
            }
            None => unreachable!(),
        }

        let promise = promise.attach(question_ref.clone());
        let promise2 = Promise::from_future(promise);

        (question_ref, promise2)
    }

    fn send_streaming_internal(
        connection_state: &Rc<ConnectionState<VatId>>,
        mut message: Box<dyn crate::OutgoingMessage>,
        cap_table: &[Option<Box<dyn ClientHook>>],
        flow: Rc<RefCell<Option<Box<dyn crate::FlowController>>>>,
        permit: Rc<crate::admission::Permit>,
    ) -> Promise<(), Error> {
        // Build the cap table.
        let mut fds = OutgoingFds::default();
        let exports = ConnectionState::write_descriptors(
            connection_state,
            cap_table,
            get_call(&mut message).unwrap().get_params().unwrap(),
            &mut fds,
        );
        fds.attach(&mut *message);

        // Init the question table.  Do this after writing descriptors to avoid interference.
        let mut question = Question::<VatId>::new();
        question._permit = Some(permit.clone());
        question.is_awaiting_return = true;
        question.param_exports = exports;
        question.is_tail_call = false;
        question.allow_third_party = get_call(&mut message)
            .unwrap()
            .get_allow_third_party_tail_call();

        let question_id = connection_state.questions.borrow_mut().push(question);
        {
            let mut call_builder: call::Builder = get_call(&mut message).unwrap();
            call_builder
                .reborrow()
                .set_question_id(question_id.to_wire());
        }

        // Make the result promise.
        let (fulfiller, promise) = oneshot::channel::<Promise<Response<VatId>, Error>>();
        let promise = promise.map_err(crate::canceled_to_error).and_then(|x| x);
        let question_ref = Rc::new(RefCell::new(QuestionRef::new(
            connection_state.clone(),
            question_id,
            fulfiller,
        )));

        match connection_state.questions.borrow_mut().find(question_id) {
            Some(ref mut q) => {
                q.self_ref = Some(Rc::downgrade(&question_ref));
            }
            None => unreachable!(),
        }
        let promise = promise.attach(question_ref.clone());

        let mut flow = flow.borrow_mut();
        if flow.is_none() {
            match connection_state.connection.borrow_mut().as_mut() {
                Err(_) => return Promise::err(Error::failed("no connection".into())),
                Ok(connection) => {
                    let (s, p) = connection.new_stream();
                    connection_state.add_task(p);
                    *flow = Some(s);
                }
            };
        }
        let Some(ref mut flow) = *flow else {
            unreachable!()
        };
        flow.send(
            Box::new(StreamingMessage {
                message,
                state: connection_state.clone(),
                permit,
            }),
            Promise::from_future(async move {
                let _ = promise.await?;
                Ok(())
            }),
        )
    }
}

impl<VatId> RequestHook for Request<VatId> {
    fn third_party_tail_target(&self) -> Option<Box<dyn std::any::Any>> {
        if self.get_brand() != 0 && answer_adoption::supported(&self.connection_state) {
            Some(Box::new(self.connection_state.clone()))
        } else {
            None
        }
    }
    fn set_third_party_tail_target(
        &mut self,
        contact: any_pointer::Reader<'_>,
    ) -> capnp::Result<()> {
        get_call(&mut self.message)?
            .get_send_results_to()
            .init_third_party()
            .set_as(contact)
    }

    fn set_hints(&mut self, hints: CallHints) {
        let mut call = get_call(&mut self.message).unwrap();
        call.set_no_promise_pipelining(hints.no_promise_pipelining);
        // Normal send() always requests its response; only send_for_pipeline()
        // enables onlyPromisePipeline on the wire.
    }
    fn send_for_pipeline(mut self: Box<Self>) -> any_pointer::Pipeline {
        if let Err(error) = &*self.connection_state.connection.borrow() {
            return any_pointer::Pipeline::new(Box::new(broken::Pipeline::new(error.clone())));
        }
        let redirect = self
            .target
            .write_target(get_call(&mut self.message).unwrap().get_target().unwrap());
        if let Some(redirect) = redirect {
            drop(self.permit);
            let mut call = get_call(&mut self.message).unwrap();
            let mut replacement = redirect.new_call_with_hints(
                call.reborrow().get_interface_id(),
                call.reborrow().get_method_id(),
                None,
                CallHints {
                    no_promise_pipelining: false,
                    only_promise_pipeline: true,
                },
            );
            use capnp::traits::Imbue;
            let mut params = call.get_params().unwrap().get_content().into_reader();
            params.imbue(&self.cap_table);
            if let Err(error) = replacement.set(params) {
                return any_pointer::Pipeline::new(Box::new(broken::Pipeline::new(error)));
            }
            return replacement.hook.send_for_pipeline();
        }
        get_call(&mut self.message)
            .unwrap()
            .set_no_promise_pipelining(false);
        if self.connection_state.got_return_for_high_id.get() {
            return self.send().pipeline;
        }
        let mut fds = OutgoingFds::default();
        let exports = ConnectionState::write_descriptors(
            &self.connection_state,
            &self.cap_table,
            get_call(&mut self.message).unwrap().get_params().unwrap(),
            &mut fds,
        );
        fds.attach(&mut *self.message);
        let mut question = Question::new();
        question._permit = Some(self.permit.clone());
        question.is_awaiting_return = false;
        question.param_exports = exports;
        let id = self
            .connection_state
            .questions
            .borrow_mut()
            .push_high(question);
        let reference = Rc::new(RefCell::new(QuestionRef {
            connection_state: Some(self.connection_state.clone()),
            id,
            fulfiller: None,
            pipeline: None,
        }));
        self.connection_state
            .questions
            .borrow_mut()
            .find(id)
            .unwrap()
            .self_ref = Some(Rc::downgrade(&reference));
        let mut call = get_call(&mut self.message).unwrap();
        call.set_question_id(id.to_wire());
        call.set_no_promise_pipelining(false);
        call.set_only_promise_pipeline(true);
        self.connection_state.send_call(self.message, self.permit);
        any_pointer::Pipeline::new(Box::new(Pipeline::never_done(
            self.connection_state.clone(),
            reference,
        )))
    }

    fn get(&mut self) -> any_pointer::Builder<'_> {
        use ::capnp::traits::ImbueMut;
        let mut builder = get_call(&mut self.message)
            .unwrap()
            .get_params()
            .unwrap()
            .get_content();
        builder.imbue_mut(&mut self.cap_table);
        builder
    }
    fn get_brand<'a>(&self) -> usize {
        // tail_send consumes its request even when it returns None. Only expose
        // the connection brand while the target can still be sent on it, so a
        // ResultsHook can choose ordinary forwarding before consuming a request
        // whose promise resolved elsewhere. No executor turn occurs before send.
        if self.connection_state.connection.borrow().is_ok() && self.target.can_tail_send() {
            self.connection_state.get_brand()
        } else {
            0
        }
    }
    fn send(self: Box<Self>) -> ::capnp::capability::RemotePromise<any_pointer::Owned> {
        if let Err(error) = &*self.connection_state.connection.borrow() {
            return capnp::capability::RemotePromise {
                promise: Promise::err(error.clone()),
                pipeline: any_pointer::Pipeline::new(Box::new(broken::Pipeline::new(
                    error.clone(),
                ))),
            };
        }
        let tmp = *self;
        let Self {
            connection_state,
            permit,
            target,
            mut message,
            cap_table,
        } = tmp;
        let write_target_result = {
            let call_builder: call::Builder = get_call(&mut message).unwrap();
            target.write_target(call_builder.get_target().unwrap())
        };
        if let Some(redirect) = write_target_result {
            drop(permit);
            // Whoops, this capability has been redirected while we were building the request!
            // We'll have to make a new request and do a copy.  Ick.
            let mut call_builder: call::Builder = get_call(&mut message).unwrap();
            let mut replacement = redirect.new_call_with_hints(
                call_builder.reborrow().get_interface_id(),
                call_builder.reborrow().get_method_id(),
                None,
                CallHints {
                    no_promise_pipelining: call_builder.reborrow().get_no_promise_pipelining(),
                    only_promise_pipeline: false,
                },
            );

            use capnp::traits::Imbue;
            let mut params = call_builder
                .get_params()
                .unwrap()
                .get_content()
                .into_reader();
            params.imbue(&cap_table);
            replacement.set(params).unwrap();
            return replacement.send();
        }
        let disabled = get_call(&mut message).unwrap().get_no_promise_pipelining();
        let (question_ref, promise) =
            Self::send_internal(&connection_state, message, &cap_table, false, permit);
        if disabled {
            return capnp::capability::RemotePromise {
                promise: Promise::from_future(
                    promise.map_ok(|response| capnp::capability::Response::new(Box::new(response))),
                ),
                pipeline: any_pointer::Pipeline::new(Box::new(broken::DisabledPipeline)),
            };
        }

        let forked_promise1 = promise.shared();
        let forked_promise2 = forked_promise1.clone();

        // The pipeline must get notified of resolution before the app does to maintain ordering.
        let pipeline = Pipeline::new(
            &connection_state,
            question_ref,
            Some(Promise::from_future(forked_promise1)),
        );

        let resolved = pipeline.when_resolved();

        let forked_promise2 = resolved.map(|_| Ok(())).and_then(|()| forked_promise2);

        let app_promise = Promise::from_future(
            forked_promise2
                .map_ok(|response| ::capnp::capability::Response::new(Box::new(response))),
        );

        ::capnp::capability::RemotePromise {
            promise: app_promise,
            pipeline: any_pointer::Pipeline::new(Box::new(pipeline)),
        }
    }
    fn send_streaming(self: Box<Self>) -> Promise<(), Error> {
        if let Err(error) = &*self.connection_state.connection.borrow() {
            return Promise::err(error.clone());
        }
        let tmp = *self;
        let Self {
            connection_state,
            permit,
            target,
            mut message,
            cap_table,
        } = tmp;
        let write_target_result = {
            let call_builder: call::Builder = get_call(&mut message).unwrap();
            target.write_target(call_builder.get_target().unwrap())
        };
        if let Some(redirect) = write_target_result {
            drop(permit);
            // Whoops, this capability has been redirected while we were building the request!
            // We'll have to make a new request and do a copy.  Ick.
            let mut call_builder: call::Builder = get_call(&mut message).unwrap();
            let mut replacement = redirect.new_call_with_hints(
                call_builder.reborrow().get_interface_id(),
                call_builder.reborrow().get_method_id(),
                None,
                CallHints {
                    no_promise_pipelining: call_builder.reborrow().get_no_promise_pipelining(),
                    only_promise_pipeline: false,
                },
            );

            use capnp::traits::Imbue;
            let mut params = call_builder
                .get_params()
                .unwrap()
                .get_content()
                .into_reader();
            params.imbue(&cap_table);
            replacement.set(params).unwrap();
            return replacement.hook.send_streaming();
        }
        Self::send_streaming_internal(
            &connection_state,
            message,
            &cap_table,
            target.flow_controller,
            permit,
        )
    }
    fn tail_send(self: Box<Self>) -> Option<(u32, Promise<(), Error>, Box<dyn PipelineHook>)> {
        let tmp = *self;
        let Self {
            connection_state,
            permit,
            target,
            mut message,
            cap_table,
        } = tmp;

        if connection_state.connection.borrow().is_err() {
            // Disconnected; fall back to a regular send() which will fail appropriately.
            return None;
        }

        let write_target_result = {
            let call_builder: crate::rpc_capnp::call::Builder = get_call(&mut message).unwrap();
            target.write_target(call_builder.get_target().unwrap())
        };

        let disabled = get_call(&mut message).unwrap().get_no_promise_pipelining();
        let (question_ref, promise) = match write_target_result {
            Some(_redirect) => {
                return None;
            }
            None => Self::send_internal(&connection_state, message, &cap_table, true, permit),
        };

        let promise = promise.map_ok(|_response| ());

        let question_id = question_ref.borrow().id;
        let pipeline: Box<dyn PipelineHook> = if disabled {
            Box::new(broken::DisabledPipeline)
        } else {
            Box::new(Pipeline::never_done(connection_state, question_ref))
        };

        // RequestHook is shared with non-RPC backends and exposes a wire ID;
        // only this trait boundary projects the typed local question.
        Some((
            question_id.to_wire(),
            Promise::from_future(promise),
            pipeline,
        ))
    }
}

enum PipelineVariant<VatId>
where
    VatId: 'static,
{
    Waiting(Rc<RefCell<QuestionRef<VatId>>>),
    Adopted(Box<dyn PipelineHook>),
    Resolved(Response<VatId>),
    Broken(Error),
}

struct PipelineState<VatId>
where
    VatId: 'static,
{
    variant: PipelineVariant<VatId>,
    redirect_later: Option<RefCell<futures::future::Shared<Promise<Response<VatId>, Error>>>>,
    connection_state: Rc<ConnectionState<VatId>>,

    #[allow(dead_code)]
    resolve_self_promise: Promise<(), Error>,

    promise_clients_to_resolve: RefCell<
        crate::sender_queue::SenderQueue<
            (Weak<RefCell<PromiseClient<VatId>>>, Vec<PipelineOp>),
            (),
        >,
    >,
    resolution_waiters: crate::sender_queue::SenderQueue<(), ()>,
}

impl<VatId> PipelineState<VatId>
where
    VatId: 'static,
{
    fn adopt(state: &Rc<RefCell<Self>>, pipeline: Box<dyn PipelineHook>) {
        let clients = state
            .borrow()
            .promise_clients_to_resolve
            .borrow_mut()
            .drain();
        for ((client, ops), _) in clients {
            if let Some(client) = client.upgrade() {
                let (used, old, connection) = {
                    let client = client.borrow();
                    (
                        client.received_call,
                        client.cap.clone(),
                        client.connection_state.clone(),
                    )
                };
                let direct = pipeline.get_pipelined_cap(&ops);
                if used {
                    // A bounded, authenticated Join freezes future calls until
                    // the old path has reached the adopted capability. Standard
                    // senderLoopback is not a fence for a third-party target.
                    if let Some(fenced) = answer_adoption::fence(&connection, old.clone(), direct) {
                        client.borrow_mut().replace_resolved(fenced);
                    } else {
                        client.borrow_mut().resolve(Ok(old));
                    }
                } else {
                    client.borrow_mut().resolve(Ok(direct));
                }
            }
        }
        let _old = mem::replace(
            &mut state.borrow_mut().variant,
            PipelineVariant::Adopted(pipeline),
        );
    }

    fn resolve(state: &Rc<RefCell<Self>>, response: Result<Response<VatId>, Error>) {
        let to_resolve = {
            let tmp = state.borrow();
            let r = tmp.promise_clients_to_resolve.borrow_mut().drain();
            r
        };
        for ((c, ops), _) in to_resolve {
            let resolved = match response.clone() {
                Ok(v) => match v.get() {
                    Ok(x) => x.get_pipelined_cap(&ops),
                    Err(e) => Err(e),
                },
                Err(e) => Err(e),
            };
            if let Some(c) = c.upgrade() {
                c.borrow_mut().resolve(resolved);
            }
        }

        let new_variant = match response {
            Ok(r) => PipelineVariant::Resolved(r),
            Err(e) => PipelineVariant::Broken(e),
        };
        let _old_variant = mem::replace(&mut state.borrow_mut().variant, new_variant);

        let waiters = state.borrow_mut().resolution_waiters.drain();
        for (_, waiter) in waiters {
            let _ = waiter.send(());
        }
    }
}

struct Pipeline<VatId>
where
    VatId: 'static,
{
    state: Rc<RefCell<PipelineState<VatId>>>,
}

impl<VatId> Pipeline<VatId> {
    fn new(
        connection_state: &Rc<ConnectionState<VatId>>,
        question_ref: Rc<RefCell<QuestionRef<VatId>>>,
        redirect_later: Option<Promise<Response<VatId>, ::capnp::Error>>,
    ) -> Self {
        let reference = question_ref.clone();
        let state = Rc::new(RefCell::new(PipelineState {
            variant: PipelineVariant::Waiting(question_ref),
            connection_state: connection_state.clone(),
            redirect_later: None,
            resolve_self_promise: Promise::from_future(future::pending()),
            promise_clients_to_resolve: RefCell::new(crate::sender_queue::SenderQueue::new()),
            resolution_waiters: crate::sender_queue::SenderQueue::new(),
        }));
        reference.borrow_mut().pipeline = Some(Rc::downgrade(&state));
        if let Some(redirect_later_promise) = redirect_later {
            let fork = redirect_later_promise.shared();
            let this = Rc::downgrade(&state);
            let resolve_self_promise =
                connection_state.eagerly_evaluate(fork.clone().then(move |response| {
                    let Some(state) = this.upgrade() else {
                        return Promise::err(Error::failed("dangling reference to this".into()));
                    };
                    PipelineState::resolve(&state, response);
                    Promise::ok(())
                }));

            state.borrow_mut().resolve_self_promise = resolve_self_promise;
            state.borrow_mut().redirect_later = Some(RefCell::new(fork));
        }
        Self { state }
    }

    fn when_resolved(&self) -> Promise<(), Error> {
        let mut state = self.state.borrow_mut();
        match &state.variant {
            PipelineVariant::Waiting(_) | PipelineVariant::Adopted(_) => {
                state.resolution_waiters.push(())
            }
            PipelineVariant::Resolved(_) => Promise::ok(()),
            PipelineVariant::Broken(error) => Promise::err(error.clone()),
        }
    }

    fn never_done(
        connection_state: Rc<ConnectionState<VatId>>,
        question_ref: Rc<RefCell<QuestionRef<VatId>>>,
    ) -> Self {
        let state = Rc::new(RefCell::new(PipelineState {
            variant: PipelineVariant::Waiting(question_ref),
            connection_state,
            redirect_later: None,
            resolve_self_promise: Promise::from_future(future::pending()),
            promise_clients_to_resolve: RefCell::new(crate::sender_queue::SenderQueue::new()),
            resolution_waiters: crate::sender_queue::SenderQueue::new(),
        }));

        Self { state }
    }
}

impl<VatId> PipelineHook for Pipeline<VatId> {
    fn add_ref(&self) -> Box<dyn PipelineHook> {
        Box::new(Self {
            state: self.state.clone(),
        })
    }
    fn get_pipelined_cap(&self, ops: &[PipelineOp]) -> Box<dyn ClientHook> {
        self.get_pipelined_cap_move(ops.into())
    }
    fn get_pipelined_cap_move(&self, ops: Vec<PipelineOp>) -> Box<dyn ClientHook> {
        match *self.state.borrow() {
            PipelineState {
                variant: PipelineVariant::Waiting(ref question_ref),
                ref connection_state,
                ref redirect_later,
                ref promise_clients_to_resolve,
                ..
            } => {
                // Wrap a PipelineClient in a PromiseClient.
                let pipeline_client =
                    PipelineClient::new(connection_state, question_ref.clone(), ops.clone());

                match redirect_later {
                    Some(_r) => {
                        let client: Client<VatId> = pipeline_client.into();
                        let promise_client =
                            PromiseClient::new(connection_state, Box::new(client), None);
                        promise_client.borrow_mut().pipeline_owner = Some(self.state.clone());
                        promise_clients_to_resolve
                            .borrow_mut()
                            .push_detach((Rc::downgrade(&promise_client), ops));
                        let result: Client<VatId> = promise_client.into();
                        Box::new(result)
                    }
                    None => {
                        // Oh, this pipeline will never get redirected, so just return the PipelineClient.
                        let client: Client<VatId> = pipeline_client.into();
                        Box::new(client)
                    }
                }
            }
            PipelineState {
                variant: PipelineVariant::Adopted(ref pipeline),
                ..
            } => pipeline.get_pipelined_cap_move(ops),
            PipelineState {
                variant: PipelineVariant::Resolved(ref response),
                ..
            } => response
                .get()
                .and_then(|r| r.get_pipelined_cap(&ops))
                .unwrap_or_else(broken::new_cap),
            PipelineState {
                variant: PipelineVariant::Broken(ref e),
                ..
            } => broken::new_cap(e.clone()),
        }
    }
}

pub(crate) struct Params {
    request: Box<dyn crate::IncomingMessage>,
    cap_table: Vec<Option<Box<dyn ClientHook>>>,
}

impl Params {
    fn new(
        request: Box<dyn crate::IncomingMessage>,
        cap_table: Vec<Option<Box<dyn ClientHook>>>,
    ) -> Self {
        Self { request, cap_table }
    }
}

impl ParamsHook for Params {
    fn get(&self) -> ::capnp::Result<any_pointer::Reader<'_>> {
        let root: message::Reader = self.request.get_body()?.get_as()?;
        let message::Call(call) = root.which()? else {
            unreachable!()
        };
        use ::capnp::traits::Imbue;
        let mut content = call?.get_params()?.get_content();
        content.imbue(&self.cap_table);
        Ok(content)
    }
}

enum ResultsVariant {
    Transferred(Box<dyn PipelineHook>),
    Rpc(
        Box<dyn crate::OutgoingMessage>,
        Vec<Option<Box<dyn ClientHook>>>,
    ),
    LocallyRedirected(
        ::capnp::message::Builder<::capnp::message::HeapAllocator>,
        Vec<Option<Box<dyn ClientHook>>>,
    ),
}

async fn yield_once() {
    let mut yielded = false;
    future::poll_fn(move |cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await
}

// Equivalent to the C++ RpcCallContext destructor's first-responder rule.
// A Finish releases the answer's task, but pipelines or application-owned
// Results may still hold the call context. Only its final drop sends cancellation.
struct ReturnGuard<VatId: 'static> {
    state: Weak<ConnectionState<VatId>>,
    id: AnswerId,
    status: Rc<AnswerStatus>,
    redirect: bool,
    only_pipeline: bool,
}
impl<VatId> Drop for ReturnGuard<VatId> {
    fn drop(&mut self) {
        if self.status.return_has_been_sent.replace(true) {
            return;
        }
        let Some(state) = self.state.upgrade() else {
            return;
        };
        if state.connection.borrow().is_err() {
            return;
        }
        if self.only_pipeline {
            state.answer_has_sent_return(self.id, Vec::new());
            return;
        }
        let result = (|| -> capnp::Result<()> {
            let mut message = state.new_outgoing_message(8)?;
            let mut ret = message
                .get_body()?
                .init_as::<message::Builder>()
                .init_return();
            ret.set_answer_id(self.id.to_wire());
            ret.set_release_param_caps(false);
            if self.redirect {
                ret.set_results_sent_elsewhere(());
            } else {
                ret.set_canceled(());
            }
            let _ = message.send_detached();
            Ok(())
        })();
        state.answer_has_sent_return(self.id, Vec::new());
        if let Err(error) = result {
            state.add_task(async move { Err(error) });
        }
    }
}

struct ResultsInner<VatId>
where
    VatId: 'static,
{
    connection_state: Rc<ConnectionState<VatId>>,
    variant: Option<ResultsVariant>,
    redirect_results: bool,
    only_promise_pipeline: bool,
    allow_third_party: bool,
    answer_id: AnswerId,
    pipeline_sender: Option<queued::PipelineInnerSender>,
    return_guard: Rc<ReturnGuard<VatId>>,
    retained_results: Rc<RefCell<Option<ResultsVariant>>>,
}

impl<VatId> Drop for ResultsInner<VatId> {
    fn drop(&mut self) {
        // A detached application method still owns its result capabilities even
        // if it released Results early and its caller has since canceled.
        if Rc::strong_count(&self.retained_results) > 1 {
            *self.retained_results.borrow_mut() = self.variant.take();
        }
    }
}

impl<VatId> ResultsInner<VatId>
where
    VatId: 'static,
{
    fn ensure_initialized(&mut self, size_hint: Option<capnp::MessageSize>) {
        let answer_id = self.answer_id;
        if self.variant.is_none() {
            match (
                self.redirect_results || self.only_promise_pipeline,
                self.connection_state.connection.borrow_mut().as_mut(),
            ) {
                (false, Ok(c)) => {
                    let mut message = c.new_outgoing_message(result_size_hint(size_hint));

                    {
                        let root: message::Builder = message.get_body().unwrap().init_as();
                        let mut ret = root.init_return();
                        ret.set_answer_id(answer_id.to_wire());
                        ret.set_release_param_caps(false);
                    }
                    self.variant = Some(ResultsVariant::Rpc(message, Vec::new()));
                }
                _ => {
                    self.variant = Some(ResultsVariant::LocallyRedirected(
                        ::capnp::message::Builder::new(local::result_allocator(size_hint)),
                        Vec::new(),
                    ));
                }
            }
        }
    }
}

fn result_size_hint(size: Option<capnp::MessageSize>) -> u32 {
    use capnp::traits::HasStructSize;
    size.map_or(0, |s| {
        let envelope = 1
            + message::Builder::STRUCT_SIZE.total()
            + return_::Builder::STRUCT_SIZE.total()
            + payload::Builder::STRUCT_SIZE.total();
        payload_size_hint(s, envelope)
    })
}

fn payload_size_hint(size: capnp::MessageSize, envelope: u32) -> u32 {
    use capnp::traits::HasStructSize;
    let descriptor = cap_descriptor::Builder::STRUCT_SIZE.total()
        + promised_answer::Builder::STRUCT_SIZE.total();
    size.word_count
        .saturating_add(u64::from(size.cap_count) * u64::from(descriptor))
        .saturating_add(u64::from(size.cap_count != 0)) // nonempty composite-list tag
        .min(1 << 20) as u32
        + envelope
}

// This takes the place of both RpcCallContext and RpcServerResponse in capnproto-c++.
pub(crate) struct Results<VatId>
where
    VatId: 'static,
{
    inner: Option<ResultsInner<VatId>>,
    results_done_fulfiller: Option<oneshot::Sender<ResultsInner<VatId>>>,
    permits_immediate_poll: bool,
}

impl<VatId> Results<VatId>
where
    VatId: 'static,
{
    fn new(
        connection_state: &Rc<ConnectionState<VatId>>,
        answer_id: AnswerId,
        redirect_results: bool,
        fulfiller: oneshot::Sender<ResultsInner<VatId>>,
        status: Rc<AnswerStatus>,
        pipeline_sender: Option<queued::PipelineInnerSender>,
    ) -> Self {
        Self {
            inner: Some(ResultsInner {
                variant: None,
                connection_state: connection_state.clone(),
                redirect_results,
                only_promise_pipeline: false,
                allow_third_party: false,
                answer_id,
                pipeline_sender,
                retained_results: Rc::new(RefCell::new(None)),
                return_guard: Rc::new(ReturnGuard {
                    state: Rc::downgrade(connection_state),
                    id: answer_id,
                    status,
                    redirect: redirect_results,
                    only_pipeline: false,
                }),
            }),
            results_done_fulfiller: Some(fulfiller),
            permits_immediate_poll: false,
        }
    }
}

impl<VatId> Drop for Results<VatId> {
    fn drop(&mut self) {
        match (self.inner.take(), self.results_done_fulfiller.take()) {
            (Some(inner), Some(fulfiller)) => {
                let _ = fulfiller.send(inner);
            }
            (None, None) => (),
            _ => unreachable!(),
        }
    }
}

impl<VatId> ResultsHook for Results<VatId> {
    fn permits_immediate_poll(&self) -> bool {
        self.permits_immediate_poll
    }

    fn cancellation_guard(&self) -> Option<Box<dyn std::any::Any>> {
        let inner = self.inner.as_ref()?;
        Some(Box::new((
            inner.return_guard.clone(),
            inner.retained_results.clone(),
        )))
    }
    fn cancellation_executor(&self) -> Option<Rc<dyn capnp::capability::CallExecutor>> {
        Some(self.inner.as_ref()?.connection_state.call_executor.clone())
    }

    fn get_with_size_hint(
        &mut self,
        size_hint: Option<capnp::MessageSize>,
    ) -> ::capnp::Result<any_pointer::Builder<'_>> {
        use ::capnp::traits::ImbueMut;
        let Some(ref mut inner) = self.inner else {
            unreachable!();
        };
        inner.ensure_initialized(size_hint);
        match inner.variant {
            Some(ResultsVariant::Transferred(_)) => {
                Err(Error::failed("tail results transferred".into()))
            }
            None => unreachable!(),
            Some(ResultsVariant::Rpc(ref mut message, ref mut cap_table)) => {
                let root: message::Builder = message.get_body()?.get_as()?;
                let message::Return(ret) = root.which()? else {
                    unreachable!();
                };
                let return_::Results(payload) = ret?.which()? else {
                    unreachable!()
                };
                let mut content = payload?.get_content();
                content.imbue_mut(cap_table);
                Ok(content)
            }
            Some(ResultsVariant::LocallyRedirected(ref mut message, ref mut cap_table)) => {
                let mut result: any_pointer::Builder = message.get_root()?;
                result.imbue_mut(cap_table);
                Ok(result)
            }
        }
    }

    fn set_pipeline(&mut self) -> ::capnp::Result<()> {
        use ::capnp::traits::ImbueMut;
        let root = self.get()?;
        let size = root.target_size()?;
        let mut message2 = capnp::message::Builder::new(
            capnp::message::HeapAllocator::new().first_segment_words(size.word_count as u32 + 1),
        );
        let mut root2: capnp::any_pointer::Builder = message2.init_root();
        let mut cap_table2 = vec![];
        root2.imbue_mut(&mut cap_table2);
        root2.set_as(root.into_reader())?;
        let hook =
            Box::new(local::ResultsDone::new(message2, cap_table2)) as Box<dyn ResultsDoneHook>;
        self.set_pipeline_from(Box::new(local::Pipeline::new(hook)))
    }

    fn set_pipeline_from(&mut self, pipeline: Box<dyn PipelineHook>) -> capnp::Result<()> {
        let Some(ref mut inner) = self.inner else {
            unreachable!();
        };
        let Some(sender) = inner.pipeline_sender.take() else {
            return Err(Error::failed("set_pipeline() called twice".into()));
        };
        sender.complete(pipeline);
        Ok(())
    }

    fn tail_call(self: Box<Self>, request: Box<dyn RequestHook>) -> Promise<(), Error> {
        self.direct_tail_call(request).0
    }

    fn direct_tail_call(
        mut self: Box<Self>,
        mut request: Box<dyn RequestHook>,
    ) -> (Promise<(), Error>, Box<dyn PipelineHook>) {
        let inner = self.inner.as_mut().expect("missing results context");
        if inner.variant.is_some() {
            let error = Error::failed("tail_call after initializing results".into());
            return (
                Promise::err(error.clone()),
                Box::new(broken::Pipeline::new(error)),
            );
        }
        if inner.only_promise_pipeline {
            let pipeline = request.send_for_pipeline();
            if let Some(sender) = inner.pipeline_sender.take() {
                sender.complete(pipeline.hook.clone());
            }
            let completion = Promise::from_future(async move {
                let _context = self;
                futures::future::pending::<capnp::Result<()>>().await
            });
            return (completion, pipeline.hook);
        }
        if !inner.redirect_results
            && !inner.only_promise_pipeline
            && request.get_brand() == inner.connection_state.get_brand()
        {
            let Some((question, completion, pipeline)) = request.tail_send() else {
                let error =
                    Error::failed("tail-send target changed during synchronous send".into());
                return (
                    Promise::err(error.clone()),
                    Box::new(broken::Pipeline::new(error)),
                );
            };
            inner.variant = Some(ResultsVariant::Transferred(pipeline.clone()));
            if let Some(sender) = inner.pipeline_sender.take() {
                sender.complete(pipeline.clone());
            }
            let result = (|| -> capnp::Result<()> {
                let mut message = inner.connection_state.new_outgoing_message(8)?;
                let mut ret = message
                    .get_body()?
                    .init_as::<message::Builder>()
                    .init_return();
                ret.set_answer_id(inner.answer_id.to_wire());
                ret.set_release_param_caps(false);
                ret.set_take_from_other_question(question);
                let _ = message.send_detached();
                inner
                    .connection_state
                    .answer_has_sent_return(inner.answer_id, vec![]);
                Ok(())
            })();
            if let Err(error) = result {
                return (Promise::err(error), pipeline);
            }
            return (Promise::from_future(completion.attach(self)), pipeline);
        }
        if inner.allow_third_party && !inner.redirect_results {
            match answer_adoption::prepare_tail(inner, &mut *request) {
                Ok(Some(redirect)) => {
                    let Some((_, completion, pipeline)) = request.tail_send() else {
                        let error = Error::failed("third-party tail target changed".into());
                        return (
                            Promise::err(error.clone()),
                            Box::new(broken::Pipeline::new(error)),
                        );
                    };
                    inner.variant = Some(ResultsVariant::Transferred(pipeline.clone()));
                    if let Some(sender) = inner.pipeline_sender.take() {
                        sender.complete(pipeline.clone());
                    }
                    let _ = redirect.send_detached();
                    inner
                        .connection_state
                        .answer_has_sent_return(inner.answer_id, vec![]);
                    return (Promise::from_future(completion.attach(self)), pipeline);
                }
                Ok(None) => (),
                Err(error) => {
                    return (
                        Promise::err(error.clone()),
                        Box::new(broken::Pipeline::new(error)),
                    )
                }
            }
        }
        let capnp::capability::RemotePromise { promise, pipeline } = request.send();
        if let Some(inner) = self.inner.as_mut() {
            if let Some(sender) = inner.pipeline_sender.take() {
                sender.complete(pipeline.hook.clone());
            }
        }
        let completion = Promise::from_future(async move {
            let response = promise.await?;
            self.get()?.set_as(response.get()?)?;
            Ok(())
        });
        (completion, pipeline.hook)
    }

    fn allow_cancellation(&self) {
        // This runtime already permits cancellation when the response and all
        // dependent pipelines are dropped. No additional opt-in is required.
    }
}

enum ResultsDoneVariant {
    Rpc(
        Rc<::capnp::message::Builder<::capnp::message::HeapAllocator>>,
        Vec<Option<Box<dyn ClientHook>>>,
    ),
    LocallyRedirected(
        ::capnp::message::Builder<::capnp::message::HeapAllocator>,
        Vec<Option<Box<dyn ClientHook>>>,
    ),
}

struct ResultsDone {
    inner: Rc<ResultsDoneVariant>,
}

// Ordinary replies are already owned by the transport. Only local redirection
// needs to return a results hook to the caller of from_results_inner().
enum ResultsCompletion {
    Sent,
    Retained(Box<dyn ResultsDoneHook>),
}
impl ResultsCompletion {
    fn into_retained(self) -> Result<Box<dyn ResultsDoneHook>, Error> {
        match self {
            Self::Retained(results) => Ok(results),
            Self::Sent => Err(Error::failed(
                "redirected call completed without local results".into(),
            )),
        }
    }
}

impl ResultsDone {
    fn from_results_inner<VatId>(
        results_inner: Result<ResultsInner<VatId>, Error>,
        call_status: Result<(), Error>,
        pipeline_sender: queued::PipelineInnerSender,
    ) -> Result<ResultsCompletion, Error>
    where
        VatId: 'static,
    {
        match results_inner {
            Err(e) => {
                pipeline_sender.complete_with(|| Box::new(crate::broken::Pipeline::new(e.clone())));
                Err(e)
            }
            Ok(mut results_inner) => {
                results_inner.ensure_initialized(None);
                let connection_state = results_inner.connection_state.clone();
                let variant = results_inner.variant.take();
                let answer_id = results_inner.answer_id;
                let status = results_inner.return_guard.status.clone();
                // Kept until after response serialization or exception handling.
                let _return_guard = results_inner.return_guard.clone();
                match variant {
                    Some(ResultsVariant::Transferred(pipeline)) => {
                        // The transfer already sent the one Return and released flow
                        // credit. Completion acknowledges ownership, not result data.
                        pipeline_sender.complete(pipeline);
                        call_status?;
                        Ok(ResultsCompletion::Retained(Box::new(Self::redirected(
                            capnp::message::Builder::new_default(),
                            vec![],
                        ))))
                    }
                    None => unreachable!(),
                    Some(ResultsVariant::Rpc(mut message, cap_table)) => {
                        match (status.received_finish.get(), call_status) {
                            (true, _) => {
                                let hook = Box::new(Self::rpc(Rc::new(message.take()), cap_table))
                                    as Box<dyn ResultsDoneHook>;
                                pipeline_sender
                                    .complete_with(|| Box::new(local::Pipeline::new(hook.clone())));

                                // Send a Canceled return.
                                if let Ok(connection) =
                                    connection_state.connection.borrow_mut().as_mut()
                                {
                                    let mut message = connection.new_outgoing_message(10);
                                    {
                                        let root: message::Builder =
                                            message.get_body()?.get_as()?;
                                        let mut ret = root.init_return();
                                        ret.set_answer_id(answer_id.to_wire());
                                        ret.set_release_param_caps(false);
                                        ret.set_canceled(());
                                    }
                                    let _ = message.send_detached();
                                }

                                connection_state.answer_has_sent_return(answer_id, Vec::new());
                                Ok(ResultsCompletion::Retained(hook))
                            }
                            (false, Ok(())) => {
                                let mut fds = OutgoingFds::default();
                                let exports = {
                                    let root: message::Builder = message.get_body()?.get_as()?;
                                    let message::Return(Ok(mut ret)) = root.which()? else {
                                        unreachable!()
                                    };
                                    // Join retains downstream responses; callee-allocated
                                    // answer IDs also require explicit Finish before reuse.
                                    let requires_finish = connection_state
                                        .answers
                                        .borrow()
                                        .slots
                                        .get(&answer_id)
                                        .is_some_and(|answer| {
                                            answer.join.is_some()
                                                || answer.join_response.is_some()
                                                || answer.callee_allocated
                                        });
                                    if cap_table.is_empty() && !requires_finish {
                                        ret.set_no_finish_needed(true);
                                        status.received_finish.set(true);
                                    }
                                    let crate::rpc_capnp::return_::Results(Ok(payload)) =
                                        ret.which()?
                                    else {
                                        unreachable!()
                                    };
                                    ConnectionState::write_descriptors(
                                        &connection_state,
                                        &cap_table,
                                        payload,
                                        &mut fds,
                                    )
                                };

                                fds.attach(&mut *message);
                                let m = message.send_detached();
                                connection_state.answer_has_sent_return(answer_id, exports);
                                // If no pipeline can observe the results, leave
                                // ownership in the queued send. Creating and
                                // immediately dropping a Box + Rc results hook
                                // here used to cost two allocations per reply.
                                pipeline_sender.complete_with(|| {
                                    Box::new(local::Pipeline::new(Box::new(Self::rpc(
                                        m, cap_table,
                                    ))))
                                });
                                Ok(ResultsCompletion::Sent)
                            }
                            (false, Err(e)) => {
                                // Send an error return.
                                let trace = connection_state.encode_trace(&e);
                                if let Ok(connection) =
                                    connection_state.connection.borrow_mut().as_mut()
                                {
                                    // Error reasons, traces and detail attachments have variable size.
                                    let mut message = connection.new_outgoing_message(0);
                                    {
                                        let root: message::Builder =
                                            message.get_body()?.get_as()?;
                                        let mut ret = root.init_return();
                                        ret.set_answer_id(answer_id.to_wire());
                                        ret.set_release_param_caps(false);
                                        let mut exc = ret.init_exception();
                                        from_error(&e, exc.reborrow(), trace.as_deref());
                                    }
                                    let _ = message.send_detached();
                                }
                                connection_state.answer_has_sent_return(answer_id, Vec::new());

                                pipeline_sender.complete_with(|| {
                                    Box::new(crate::broken::Pipeline::new(e.clone()))
                                });

                                Err(e)
                            }
                        }
                    }
                    Some(ResultsVariant::LocallyRedirected(results_done, cap_table)) => {
                        if let Err(error) = call_status {
                            pipeline_sender
                                .complete_with(|| Box::new(broken::Pipeline::new(error.clone())));
                            return Err(error);
                        }
                        let hook = Box::new(Self::redirected(results_done, cap_table))
                            as Box<dyn ResultsDoneHook>;
                        pipeline_sender
                            .complete_with(|| Box::new(crate::local::Pipeline::new(hook.clone())));
                        Ok(ResultsCompletion::Retained(hook))
                    }
                }
            }
        }
    }

    fn rpc(
        message: Rc<::capnp::message::Builder<::capnp::message::HeapAllocator>>,
        cap_table: Vec<Option<Box<dyn ClientHook>>>,
    ) -> Self {
        Self {
            inner: Rc::new(ResultsDoneVariant::Rpc(message, cap_table)),
        }
    }

    fn redirected(
        message: ::capnp::message::Builder<::capnp::message::HeapAllocator>,
        cap_table: Vec<Option<Box<dyn ClientHook>>>,
    ) -> Self {
        Self {
            inner: Rc::new(ResultsDoneVariant::LocallyRedirected(message, cap_table)),
        }
    }
}

impl ResultsDoneHook for ResultsDone {
    fn add_ref(&self) -> Box<dyn ResultsDoneHook> {
        Box::new(Self {
            inner: self.inner.clone(),
        })
    }
    fn get(&self) -> ::capnp::Result<any_pointer::Reader<'_>> {
        use ::capnp::traits::Imbue;
        match *self.inner {
            ResultsDoneVariant::Rpc(ref message, ref cap_table) => {
                let root: message::Reader = message.get_root_as_reader()?;
                let message::Return(ret) = root.which()? else {
                    unreachable!();
                };
                let crate::rpc_capnp::return_::Results(payload) = ret?.which()? else {
                    unreachable!();
                };
                let mut content = payload?.get_content();
                content.imbue(cap_table);
                Ok(content)
            }
            ResultsDoneVariant::LocallyRedirected(ref message, ref cap_table) => {
                let mut result: any_pointer::Reader = message.get_root_as_reader()?;
                result.imbue(cap_table);
                Ok(result)
            }
        }
    }
}

// This capability's old route is ordered by its Accept embargo. Retaining
// the introducing connection's brand avoids a second senderLoopback embargo.
struct ThirdPartyClient<VatId: 'static> {
    state: Rc<ConnectionState<VatId>>,
    cap: RefCell<ThirdPartyState>,
}

enum ThirdPartyState {
    Deferred {
        contact: Rc<capnp::message::Builder<capnp::message::HeapAllocator>>,
        vine: Box<dyn ClientHook>,
    },
    Accepted(Box<dyn ClientHook>),
}

impl<VatId> ThirdPartyClient<VatId> {
    fn deferred(
        state: &Rc<ConnectionState<VatId>>,
        contact: Rc<capnp::message::Builder<capnp::message::HeapAllocator>>,
        vine: Box<dyn ClientHook>,
    ) -> Box<dyn ClientHook> {
        Box::new(Client::new(
            state,
            ClientVariant::ThirdParty(Rc::new(Self {
                state: state.clone(),
                cap: RefCell::new(ThirdPartyState::Deferred { contact, vine }),
            })),
        ))
    }

    fn ensure_accepted(&self) -> Box<dyn ClientHook> {
        // Cache both success and failure. Release the state borrow before any
        // transport or capability callback; reentrant use sees a broken cap.
        let old = {
            let mut state = self.cap.borrow_mut();
            if let ThirdPartyState::Accepted(cap) = &*state {
                return cap.clone();
            }
            std::mem::replace(
                &mut *state,
                ThirdPartyState::Accepted(broken::new_cap(Error::failed(
                    "reentrant third-party acceptance".into(),
                ))),
            )
        };
        let ThirdPartyState::Deferred { contact, vine } = old else {
            unreachable!()
        };
        let cap = contact
            .get_root_as_reader()
            .and_then(|contact| ConnectionState::accept_third_party(&self.state, contact, vine))
            .unwrap_or_else(broken::new_cap);
        *self.cap.borrow_mut() = ThirdPartyState::Accepted(cap.clone());
        cap
    }

    fn descriptor_cap(&self) -> Box<dyn ClientHook> {
        match &*self.cap.borrow() {
            ThirdPartyState::Deferred { vine, .. } => vine.clone(),
            ThirdPartyState::Accepted(cap) => cap.clone(),
        }
    }

    fn resolved(&self) -> Option<Box<dyn ClientHook>> {
        match &*self.cap.borrow() {
            ThirdPartyState::Deferred { .. } => None,
            ThirdPartyState::Accepted(cap) => Some(cap.clone()),
        }
    }

    fn forward_to(
        &self,
        destination: &Rc<ConnectionState<VatId>>,
        mut descriptor: cap_descriptor::Builder<'_>,
    ) -> capnp::Result<Option<ExportId>> {
        let (contact, vine) = match &*self.cap.borrow() {
            ThirdPartyState::Deferred { contact, vine } => (contact.clone(), vine.clone()),
            ThirdPartyState::Accepted(_) => return Ok(None),
        };
        let peer = destination
            .connection
            .borrow()
            .as_ref()
            .map_err(|e| e.clone())?
            .get_peer_vat_id();
        let mut forwarded = capnp::message::Builder::new_default();
        if !self
            .state
            .connection
            .borrow_mut()
            .as_mut()
            .map_err(|e| e.clone())?
            .forward_third_party_to_contact(
                contact.get_root_as_reader()?,
                peer,
                forwarded.init_root(),
            )?
        {
            return Ok(None);
        }
        let mut export = Export::new(vine.clone());
        let origin = self.state.clone();
        let reflected_origin = origin.clone();
        let reflected_vine = vine.clone();
        export.reflected_vine = Some(Rc::new(move || {
            Self::deferred(&reflected_origin, contact.clone(), reflected_vine.clone())
        }));
        export.vine = Some(Rc::new(move |embargo| {
            let Ok(mut message) = origin.new_outgoing_message(32) else {
                // A forwarded vine can outlive its upstream connection.
                // C++ ignores this Disembargo and lets provision loss reject
                // the acceptance, preserving the downstream connection.
                return Ok(());
            };
            let mut d = message
                .get_body()?
                .init_as::<message::Builder>()
                .init_disembargo();
            if origin
                .write_target(&*vine, d.reborrow().init_target())
                .is_some()
            {
                return Err(Error::failed("forwarded vine changed connection".into()));
            }
            d.init_context().set_accept(embargo);
            let _ = message.send_detached();
            Ok(())
        }));
        let export_id = destination.exports.borrow_mut().push(export);
        let mut third = descriptor.reborrow().init_third_party_hosted();
        third.set_vine_id(export_id.to_wire());
        third
            .get_id()
            .set_as(forwarded.get_root_as_reader::<any_pointer::Reader>()?)?;
        Ok(Some(export_id))
    }
}
impl<VatId> Drop for ThirdPartyClient<VatId> {
    fn drop(&mut self) {
        self.state
            .client_downcast_map
            .borrow_mut()
            .remove(&(self as *const _ as usize));
    }
}

enum ClientVariant<VatId>
where
    VatId: 'static,
{
    Import(Rc<RefCell<ImportClient<VatId>>>),
    Pipeline(Rc<RefCell<PipelineClient<VatId>>>),
    Promise(Rc<RefCell<PromiseClient<VatId>>>),
    ThirdParty(Rc<ThirdPartyClient<VatId>>),
}

struct Client<VatId>
where
    VatId: 'static,
{
    connection_state: Rc<ConnectionState<VatId>>,
    variant: ClientVariant<VatId>,
    flow_controller: Rc<RefCell<Option<Box<dyn crate::FlowController>>>>,
}

enum WeakClientVariant<VatId>
where
    VatId: 'static,
{
    Import(Weak<RefCell<ImportClient<VatId>>>),
    Pipeline(Weak<RefCell<PipelineClient<VatId>>>),
    Promise(Weak<RefCell<PromiseClient<VatId>>>),
    ThirdParty(Weak<ThirdPartyClient<VatId>>),
}

struct WeakClient<VatId>
where
    VatId: 'static,
{
    connection_state: Weak<ConnectionState<VatId>>,
    variant: WeakClientVariant<VatId>,
    flow_controller: Weak<RefCell<Option<Box<dyn crate::FlowController>>>>,
}

impl<VatId> WeakClient<VatId>
where
    VatId: 'static,
{
    fn upgrade(&self) -> Option<Client<VatId>> {
        let variant = match &self.variant {
            WeakClientVariant::ThirdParty(c) => ClientVariant::ThirdParty(c.upgrade()?),
            WeakClientVariant::Import(ic) => ClientVariant::Import(ic.upgrade()?),
            WeakClientVariant::Pipeline(pc) => ClientVariant::Pipeline(pc.upgrade()?),
            WeakClientVariant::Promise(pc) => ClientVariant::Promise(pc.upgrade()?),
        };
        let connection_state = self.connection_state.upgrade()?;
        let flow_controller = self.flow_controller.upgrade()?;
        Some(Client {
            connection_state,
            variant,
            flow_controller,
        })
    }
}

struct ImportClient<VatId>
where
    VatId: 'static,
{
    connection_state: Rc<ConnectionState<VatId>>,
    import_id: ImportId,

    /// Number of times we've received this import from the peer.
    remote_ref_count: u32,
    fd: Option<AttachedFd>,
}

impl<VatId> Drop for ImportClient<VatId> {
    fn drop(&mut self) {
        let connection_state = self.connection_state.clone();

        assert!(connection_state
            .client_downcast_map
            .borrow_mut()
            .remove(&((self) as *const _ as usize))
            .is_some());

        // Remove the corresponding entry of the imports table.
        // Note: the C++ implementation checks here pointer equality between self and
        // the entry in the imports table, but as far as I can tell the check should
        // always pass because of how we construct ImportClient in import().
        connection_state
            .imports
            .borrow_mut()
            .slots
            .remove(&self.import_id);

        // Send a message releasing our remote references.
        let mut tmp = connection_state.connection.borrow_mut();
        if let (true, Ok(c)) = (self.remote_ref_count > 0, tmp.as_mut()) {
            let mut message = c.new_outgoing_message(10);
            {
                let root: message::Builder = message.get_body().unwrap().init_as();
                let mut release = root.init_release();
                release.set_id(self.import_id.to_wire());
                release.set_reference_count(self.remote_ref_count);
            }
            let _ = message.send_detached();
        }
        drop(tmp);
        connection_state.schedule_idle_check();
    }
}

impl<VatId> ImportClient<VatId>
where
    VatId: 'static,
{
    fn new(
        connection_state: &Rc<ConnectionState<VatId>>,
        import_id: ImportId,
    ) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            connection_state: connection_state.clone(),
            import_id,
            remote_ref_count: 0,
            fd: None,
        }))
    }

    fn add_remote_ref(&mut self) {
        self.remote_ref_count += 1;
    }
}

impl<VatId> From<Rc<RefCell<ImportClient<VatId>>>> for Client<VatId> {
    fn from(client: Rc<RefCell<ImportClient<VatId>>>) -> Self {
        let connection_state = client.borrow().connection_state.clone();
        Self::new(&connection_state, ClientVariant::Import(client))
    }
}

/// A `ClientHook` representing a pipelined promise.  Always wrapped in `PromiseClient`.
struct PipelineClient<VatId>
where
    VatId: 'static,
{
    connection_state: Rc<ConnectionState<VatId>>,
    question_ref: Rc<RefCell<QuestionRef<VatId>>>,
    ops: Vec<PipelineOp>,
}

impl<VatId> PipelineClient<VatId>
where
    VatId: 'static,
{
    fn new(
        connection_state: &Rc<ConnectionState<VatId>>,
        question_ref: Rc<RefCell<QuestionRef<VatId>>>,
        ops: Vec<PipelineOp>,
    ) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            connection_state: connection_state.clone(),
            question_ref,
            ops,
        }))
    }
}

impl<VatId> From<Rc<RefCell<PipelineClient<VatId>>>> for Client<VatId> {
    fn from(client: Rc<RefCell<PipelineClient<VatId>>>) -> Self {
        let connection_state = client.borrow().connection_state.clone();
        Self::new(&connection_state, ClientVariant::Pipeline(client))
    }
}

impl<VatId> Drop for PipelineClient<VatId> {
    fn drop(&mut self) {
        assert!(self
            .connection_state
            .client_downcast_map
            .borrow_mut()
            .remove(&((self) as *const _ as usize))
            .is_some());
    }
}

/// A `ClientHook` that initially wraps one client and then, later on, redirects
/// to some other client.
struct PromiseClient<VatId>
where
    VatId: 'static,
{
    connection_state: Rc<ConnectionState<VatId>>,
    is_resolved: bool,
    cap: Box<dyn ClientHook>,
    import_id: Option<ImportId>,
    received_call: bool,
    pipeline_owner: Option<Rc<RefCell<PipelineState<VatId>>>>,
    resolution_waiters: crate::sender_queue::SenderQueue<(), Box<dyn ClientHook>>,
}

impl<VatId> PromiseClient<VatId> {
    fn new(
        connection_state: &Rc<ConnectionState<VatId>>,
        initial: Box<dyn ClientHook>,
        import_id: Option<ImportId>,
    ) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            connection_state: connection_state.clone(),
            is_resolved: false,
            cap: initial,
            import_id,
            received_call: false,
            pipeline_owner: None,
            resolution_waiters: crate::sender_queue::SenderQueue::new(),
        }))
    }

    fn resolve(&mut self, replacement: Result<Box<dyn ClientHook>, Error>) {
        let (mut replacement, is_error) = match replacement {
            Ok(v) => (v, false),
            Err(e) => (broken::new_cap(e), true),
        };
        let connection_state = self.connection_state.clone();
        let is_connected = connection_state.connection.borrow().is_ok();
        let replacement_brand = replacement.get_brand();
        if replacement_brand != connection_state.get_brand()
            && self.received_call
            && !is_error
            && is_connected
        {
            // The new capability is hosted locally, not on the remote machine.  And, we had made calls
            // to the promise.  We need to make sure those calls echo back to us before we allow new
            // calls to go directly to the local capability, so we need to set a local embargo and send
            // a `Disembargo` to echo through the peer.
            let (fulfiller, promise) = oneshot::channel::<Result<(), Error>>();
            let promise = promise
                .map_err(crate::canceled_to_error)
                .and_then(future::ready);
            let embargo = Embargo::new(fulfiller);
            let embargo_id = connection_state.embargoes.borrow_mut().push(embargo);

            let mut message = connection_state
                .new_outgoing_message({
                    use capnp::traits::HasStructSize;
                    1 + message::Builder::STRUCT_SIZE.total()
                        + disembargo::Builder::STRUCT_SIZE.total()
                        + message_target::Builder::STRUCT_SIZE.total()
                })
                .expect("connection checked above; no await or user callback before allocation");
            {
                let root: message::Builder = message.get_body().unwrap().init_as();
                let mut disembargo = root.init_disembargo();
                disembargo
                    .reborrow()
                    .init_context()
                    .set_sender_loopback(embargo_id.to_wire());
                let target = disembargo.init_target();

                let redirect = connection_state.write_target(&*self.cap, target);
                if redirect.is_some() {
                    panic!("Original promise target should always be from this RPC connection.")
                }
            }

            // Make a promise which resolves to `replacement` as soon as the `Disembargo` comes back.
            let embargo_promise = promise.map_ok(move |()| replacement);

            let mut queued_client = queued::Client::new(None);
            let weak_queued = Rc::downgrade(&queued_client.inner);

            queued_client.drive(embargo_promise.then(move |r| {
                if let Some(q) = weak_queued.upgrade() {
                    queued::ClientInner::resolve(&q, r);
                }
                Promise::ok(())
            }));

            // We need to queue up calls in the meantime, so we'll resolve ourselves to a local promise
            // client instead.
            replacement = Box::new(queued_client);

            let _ = message.send_detached();
        }

        self.replace_resolved(replacement);
    }

    // Used only after the ordinary embargo above or an authenticated adoption
    // fence has installed its own queue. Never infer ordering from a new brand.
    fn replace_resolved(&mut self, replacement: Box<dyn ClientHook>) {
        let connection_state = self.connection_state.clone();
        for ((), waiter) in self.resolution_waiters.drain() {
            let _ = waiter.send(replacement.clone());
        }

        let old_cap = mem::replace(&mut self.cap, replacement);
        connection_state.add_task(async move {
            drop(old_cap);
            Ok(())
        });

        self.is_resolved = true;
        self.pipeline_owner.take();
    }
}

impl<VatId> Drop for PromiseClient<VatId> {
    fn drop(&mut self) {
        let self_ptr = (self) as *const _ as usize;

        if let Some(id) = self.import_id {
            // This object is representing an import promise.  That means the import table may still
            // contain a pointer back to it.  Remove that pointer.  Note that we have to verify that
            // the import still exists and the pointer still points back to this object because this
            // object may actually outlive the import.
            let slots = &mut self.connection_state.imports.borrow_mut().slots;
            if let Some(import) = slots.get_mut(&id) {
                if let Some(c) = &import.app_client {
                    if let Some(cs) = c.upgrade() {
                        if cs.get_ptr() == self_ptr {
                            import.app_client = None;
                        }
                    }
                }
            }
        }

        assert!(self
            .connection_state
            .client_downcast_map
            .borrow_mut()
            .remove(&self_ptr)
            .is_some());
    }
}

impl<VatId> From<Rc<RefCell<PromiseClient<VatId>>>> for Client<VatId> {
    fn from(client: Rc<RefCell<PromiseClient<VatId>>>) -> Self {
        let connection_state = client.borrow().connection_state.clone();
        Self::new(&connection_state, ClientVariant::Promise(client))
    }
}

impl<VatId> Client<VatId> {
    fn new(connection_state: &Rc<ConnectionState<VatId>>, variant: ClientVariant<VatId>) -> Self {
        let mut client = Self {
            connection_state: connection_state.clone(),
            variant,
            flow_controller: Rc::new(RefCell::new(None)),
        };
        let ptr = client.get_ptr();
        let mut clients = connection_state.client_downcast_map.borrow_mut();
        // Re-imports share the variant identity. They must also share stream
        // state, otherwise a temporary alias can overwrite the weak downcast
        // entry and invalidate an older live client's lookup when it is dropped.
        if let Some(flow) = clients
            .get(&ptr)
            .and_then(|old| old.flow_controller.upgrade())
        {
            client.flow_controller = flow;
        }
        clients.insert(ptr, client.downgrade());
        client
    }
    fn downgrade(&self) -> WeakClient<VatId> {
        let variant = match &self.variant {
            ClientVariant::ThirdParty(c) => WeakClientVariant::ThirdParty(Rc::downgrade(c)),
            ClientVariant::Import(import_client) => {
                WeakClientVariant::Import(Rc::downgrade(import_client))
            }
            ClientVariant::Pipeline(pipeline_client) => {
                WeakClientVariant::Pipeline(Rc::downgrade(pipeline_client))
            }
            ClientVariant::Promise(promise_client) => {
                WeakClientVariant::Promise(Rc::downgrade(promise_client))
            }
        };
        WeakClient {
            connection_state: Rc::downgrade(&self.connection_state),
            variant,
            flow_controller: Rc::downgrade(&self.flow_controller),
        }
    }

    fn from_ptr(ptr: usize, connection_state: &ConnectionState<VatId>) -> Option<Self> {
        match connection_state.client_downcast_map.borrow().get(&ptr) {
            Some(c) => c.upgrade(),
            None => None,
        }
    }

    fn can_tail_send(&self) -> bool {
        match &self.variant {
            ClientVariant::Import(_) | ClientVariant::Pipeline(_) => true,
            ClientVariant::ThirdParty(_) => false,
            ClientVariant::Promise(promise) => {
                let cap = promise.borrow().cap.clone();
                cap.get_brand() == self.connection_state.get_brand()
                    && Client::from_ptr(cap.get_ptr(), &self.connection_state)
                        .is_some_and(|client| client.can_tail_send())
            }
        }
    }

    fn write_target(
        &self,
        mut target: crate::rpc_capnp::message_target::Builder,
    ) -> Option<Box<dyn ClientHook>> {
        match &self.variant {
            ClientVariant::ThirdParty(c) => Some(c.ensure_accepted()),
            ClientVariant::Import(import_client) => {
                target.set_imported_cap(import_client.borrow().import_id.to_wire());
                None
            }
            ClientVariant::Pipeline(pipeline_client) => {
                let mut builder = target.init_promised_answer();
                let question_ref = &pipeline_client.borrow().question_ref;
                builder.set_question_id(question_ref.borrow().id.to_wire());
                let mut transform =
                    builder.init_transform(pipeline_client.borrow().ops.len() as u32);
                for idx in 0..pipeline_client.borrow().ops.len() {
                    if let ::capnp::private::capability::PipelineOp::GetPointerField(ordinal) =
                        pipeline_client.borrow().ops[idx]
                    {
                        transform
                            .reborrow()
                            .get(idx as u32)
                            .set_get_pointer_field(ordinal);
                    }
                }
                None
            }
            ClientVariant::Promise(promise_client) => {
                promise_client.borrow_mut().received_call = true;
                self.connection_state
                    .write_target(&*promise_client.borrow().cap, target)
            }
        }
    }

    fn write_descriptor(
        &self,
        mut descriptor: cap_descriptor::Builder,
        fds: &mut OutgoingFds,
    ) -> Option<ExportId> {
        match &self.variant {
            ClientVariant::ThirdParty(c) => ConnectionState::write_descriptor(
                &self.connection_state,
                c.descriptor_cap(),
                descriptor,
                fds,
            )
            .unwrap(),
            ClientVariant::Import(import_client) => {
                descriptor.set_receiver_hosted(import_client.borrow().import_id.to_wire());
                None
            }
            ClientVariant::Pipeline(pipeline_client) => {
                let mut promised_answer = descriptor.init_receiver_answer();
                let question_ref = &pipeline_client.borrow().question_ref;
                promised_answer.set_question_id(question_ref.borrow().id.to_wire());
                let mut transform =
                    promised_answer.init_transform(pipeline_client.borrow().ops.len() as u32);
                for idx in 0..pipeline_client.borrow().ops.len() {
                    if let ::capnp::private::capability::PipelineOp::GetPointerField(ordinal) =
                        pipeline_client.borrow().ops[idx]
                    {
                        transform
                            .reborrow()
                            .get(idx as u32)
                            .set_get_pointer_field(ordinal);
                    }
                }

                None
            }
            ClientVariant::Promise(promise_client) => {
                promise_client.borrow_mut().received_call = true;

                ConnectionState::write_descriptor(
                    &self.connection_state.clone(),
                    promise_client.borrow().cap.clone(),
                    descriptor,
                    fds,
                )
                .unwrap()
            }
        }
    }
}

impl<VatId> Clone for Client<VatId> {
    fn clone(&self) -> Self {
        let variant = match &self.variant {
            ClientVariant::ThirdParty(c) => ClientVariant::ThirdParty(c.clone()),
            ClientVariant::Import(import_client) => ClientVariant::Import(import_client.clone()),
            ClientVariant::Pipeline(pipeline_client) => {
                ClientVariant::Pipeline(pipeline_client.clone())
            }
            ClientVariant::Promise(promise_client) => {
                ClientVariant::Promise(promise_client.clone())
            }
        };
        Self {
            connection_state: self.connection_state.clone(),
            variant,
            flow_controller: self.flow_controller.clone(),
        }
    }
}

impl<VatId> ClientHook for Client<VatId> {
    fn debug_info(&self, chain: &mut capnp::private::capability::DebugInfo) {
        match &self.variant {
            ClientVariant::Import(_) => chain.push("rpcImport"),
            ClientVariant::Pipeline(_) => chain.push("rpcPipeline"),
            ClientVariant::Promise(client) => {
                chain.push("rpcPromise");
                let Ok(state) = client.try_borrow() else {
                    chain.push("busy");
                    return;
                };
                let cap = state.cap.clone();
                drop(state);
                chain.follow(&*cap);
            }
            ClientVariant::ThirdParty(client) => {
                // Inspection must never call ensure_accepted(): even a pending
                // handoff is observable without allocating a route or sending Accept.
                let Ok(state) = client.cap.try_borrow() else {
                    chain.push("thirdParty(busy)");
                    return;
                };
                let cap = match &*state {
                    ThirdPartyState::Deferred { vine, .. } => {
                        chain.push("thirdPartyPending");
                        vine.clone()
                    }
                    ThirdPartyState::Accepted(cap) => {
                        chain.push("thirdPartyAccepted");
                        cap.clone()
                    }
                };
                drop(state);
                chain.follow(&*cap);
            }
        }
    }

    fn forward_join(
        &self,
        part: any_pointer::Reader<'_>,
    ) -> Option<crate::multiparty::PartResponse> {
        Some(multiparty_join::send_part(
            &self.connection_state,
            self.add_ref(),
            part,
        ))
    }
    fn join_capabilities(
        &self,
        caps: Vec<Box<dyn ClientHook>>,
    ) -> Promise<capnp::private::capability::JoinedCapability, Error> {
        join::send(&self.connection_state, caps)
    }
    #[cfg(unix)]
    fn get_fd(&self) -> Option<AttachedFd> {
        match &self.variant {
            ClientVariant::Import(c) => c.borrow().fd.clone(),
            ClientVariant::Promise(c) => c.borrow().cap.get_fd(),
            ClientVariant::Pipeline(_) => None,
            ClientVariant::ThirdParty(c) => c.resolved().and_then(|cap| cap.get_fd()),
        }
    }

    fn add_ref(&self) -> Box<dyn ClientHook> {
        Box::new(self.clone())
    }
    fn new_call(
        &self,
        interface_id: u64,
        method_id: u16,
        size_hint: Option<::capnp::MessageSize>,
    ) -> ::capnp::capability::Request<any_pointer::Owned, any_pointer::Owned> {
        // A resolved promise can outlive its original connection. Construct new
        // requests on the replacement route instead of allocating on that link.
        if let Some(resolved) = self.get_resolved() {
            return resolved.new_call(interface_id, method_id, size_hint);
        }
        if let ClientVariant::ThirdParty(c) = &self.variant {
            return c
                .ensure_accepted()
                .new_call(interface_id, method_id, size_hint);
        }
        let request: Box<dyn RequestHook> =
            match Request::new(self.connection_state.clone(), size_hint, self.clone()) {
                Ok(mut request) => {
                    {
                        let mut call_builder = request.init_call();
                        call_builder.set_interface_id(interface_id);
                        call_builder.set_method_id(method_id);
                    }
                    Box::new(request)
                }
                Err(e) => Box::new(broken::Request::new(e, None)),
            };

        ::capnp::capability::Request::new(request)
    }

    fn call(
        &self,
        interface_id: u64,
        method_id: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
    ) -> Promise<(), Error> {
        self.call_with_hints(
            interface_id,
            method_id,
            params,
            results,
            CallHints::default(),
        )
    }
    fn call_with_hints(
        &self,
        interface_id: u64,
        method_id: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
        hints: CallHints,
    ) -> Promise<(), Error> {
        // Copy parameters into the destination connection, preserving capabilities.

        let maybe_request = params.get().and_then(|p| {
            let mut request = p
                .target_size()
                .map(|s| self.new_call_with_hints(interface_id, method_id, Some(s), hints))?;
            request.get().set_as(p)?;
            Ok(request)
        });

        // ResultsHook selects same-connection transfer or ordinary forwarding,
        // and publishes the outgoing pipeline before completion in both cases.
        match maybe_request {
            Err(e) => Promise::err(e),
            Ok(request) => results.tail_call(request.hook),
        }
    }

    fn get_ptr(&self) -> usize {
        match &self.variant {
            ClientVariant::ThirdParty(c) => Rc::as_ptr(c) as usize,
            ClientVariant::Import(import_client) => (&*import_client.borrow()) as *const _ as usize,
            ClientVariant::Pipeline(pipeline_client) => {
                (&*pipeline_client.borrow()) as *const _ as usize
            }
            ClientVariant::Promise(promise_client) => {
                (&*promise_client.borrow()) as *const _ as usize
            }
        }
    }

    fn get_brand(&self) -> usize {
        self.connection_state.get_brand()
    }

    fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
        match &self.variant {
            ClientVariant::ThirdParty(c) => c.resolved(),
            ClientVariant::Import(_import_client) => None,
            ClientVariant::Pipeline(_pipeline_client) => None,
            ClientVariant::Promise(promise_client) => {
                if promise_client.borrow().is_resolved {
                    Some(promise_client.borrow().cap.clone())
                } else {
                    None
                }
            }
        }
    }

    fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, Error>> {
        match &self.variant {
            ClientVariant::ThirdParty(c) => Some(Promise::ok(c.ensure_accepted())),
            ClientVariant::Import(_import_client) => None,
            ClientVariant::Pipeline(_pipeline_client) => None,
            ClientVariant::Promise(promise_client) => {
                let mut promise = promise_client.borrow_mut();
                if promise.is_resolved {
                    Some(Promise::ok(promise.cap.clone()))
                } else {
                    // The observer owns its pending resolution even if the
                    // application drops the temporary capability projection.
                    Some(Promise::from_future(
                        promise
                            .resolution_waiters
                            .push(())
                            .attach(promise_client.clone()),
                    ))
                }
            }
        }
    }

    fn when_resolved(&self) -> Promise<(), Error> {
        default_when_resolved_impl(self)
    }
}

pub(crate) fn default_when_resolved_impl<C>(client: &C) -> Promise<(), Error>
where
    C: ClientHook,
{
    match client.when_more_resolved() {
        Some(promise) => {
            Promise::from_future(promise.and_then(|resolution| resolution.when_resolved()))
        }
        None => Promise::ok(()),
    }
}

// ===================================

struct SingleCapPipeline {
    cap: Box<dyn ClientHook>,
}

impl SingleCapPipeline {
    fn new(cap: Box<dyn ClientHook>) -> Self {
        Self { cap }
    }
}

impl PipelineHook for SingleCapPipeline {
    fn add_ref(&self) -> Box<dyn PipelineHook> {
        Box::new(Self {
            cap: self.cap.clone(),
        })
    }
    fn get_pipelined_cap(&self, ops: &[PipelineOp]) -> Box<dyn ClientHook> {
        if ops.is_empty() {
            self.cap.add_ref()
        } else {
            broken::new_cap(Error::failed("Invalid pipeline transform.".to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_promise_cannot_be_upgraded_on_live_connection() {
        use crate::rpc_twoparty_capnp::Side;
        let network = crate::twoparty::VatNetwork::new(
            futures::io::Cursor::new(Vec::<u8>::new()),
            futures::io::Cursor::new(Vec::<u8>::new()),
            Side::Client,
            Default::default(),
        );
        let mut system = crate::RpcSystem::new(Box::new(network), None);
        let _bootstrap: capnp::capability::Client = system.bootstrap(Side::Server);
        let state = system
            .connections
            .borrow()
            .states
            .values()
            .next()
            .unwrap()
            .clone();
        let client = Client::from(PromiseClient::new(
            &state,
            broken::new_cap(Error::failed("test promise".into())),
            None,
        ));
        let weak = client.downgrade();
        let ptr = client.get_ptr();
        assert!(weak.upgrade().is_some());

        // Keep both surrounding owners alive so only the capability expires.
        // This exercises the cleanup ordering without relying on network timing.
        let flow = client.flow_controller.clone();
        drop(client);
        assert!(state.connection.borrow().is_ok());
        assert_eq!(Rc::strong_count(&flow), 1);
        assert!(weak.upgrade().is_none());
        assert!(!state.client_downcast_map.borrow().contains_key(&ptr));
    }
}
