// Copyright (c) 2013-2017 Sandstorm Development Group, Inc. and contributors
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
use capnp::capability::{self, Promise};
use capnp::private::capability::{
    ClientHook, ParamsHook, PipelineHook, PipelineOp, RequestHook, ResponseHook, ResultsHook,
};
use capnp::traits::{Imbue, ImbueMut};
use capnp::Error;
use capnp::{any_pointer, message};

use capntproto_rpc_engine::completion::shared as oneshot;
use futures::FutureExt;
use futures::TryFutureExt;

use std::any::{Any, TypeId};
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll};

#[cfg(test)]
mod owned_results_tests;

pub trait ResultsDoneHook {
    fn add_ref(&self) -> Box<dyn ResultsDoneHook>;
    fn get(&self) -> ::capnp::Result<any_pointer::Reader<'_>>;
}

impl Clone for Box<dyn ResultsDoneHook> {
    fn clone(&self) -> Self {
        self.add_ref()
    }
}

pub(crate) struct Response {
    results: Box<dyn ResultsDoneHook>,
}

impl Response {
    pub(crate) fn new(results: Box<dyn ResultsDoneHook>) -> Self {
        Self { results }
    }
}

impl ResponseHook for Response {
    fn get(&self) -> ::capnp::Result<any_pointer::Reader<'_>> {
        self.results.get()
    }
}

pub(crate) struct Params {
    request: RequestMessage,
    cap_table: Vec<Option<Box<dyn ClientHook>>>,
    envelope: bool,
}

impl Params {
    pub(crate) fn freeze_large_params(&mut self) {
        if self.cap_table.is_empty() {
            self.request.freeze_large();
        }
    }

    pub(crate) fn batch_content_bytes(&self) -> Option<usize> {
        if !self.cap_table.is_empty() {
            return None;
        }
        let size = self.get().ok()?.target_size().ok()?;
        if size.cap_count != 0 {
            return None;
        }
        usize::try_from(size.word_count).ok()?.checked_mul(8)
    }
    pub(crate) fn single_capability_table(&self) -> Option<&[Option<Box<dyn ClientHook>>]> {
        matches!(self.cap_table.as_slice(), [Some(_)]).then_some(&self.cap_table)
    }

    #[inline(always)]
    pub(crate) fn scoped(
        interface_id: u64,
        method_id: u16,
        allocator: message::HeapAllocator,
        // None is a local request. Some(None) needs a fresh remote envelope.
        envelope: Option<Option<&capntproto_rpc_wire::owned::connection::PreparedCallTemplate>>,
    ) -> Self {
        use capntproto_rpc_wire::owned::connection::PreparedCall;
        let is_envelope = envelope.is_some();
        let message = match envelope {
            None => RequestMessage::Root(message::Builder::new(allocator)),
            Some(template) => RequestMessage::Envelope(
                match template {
                    Some(template) => {
                        PreparedCall::from_template(template, allocator, interface_id, method_id)
                    }
                    None => {
                        PreparedCall::new(message::Builder::new(allocator), interface_id, method_id)
                    }
                }
                .expect("private request envelope"),
            ),
        };
        Self {
            request: message,
            cap_table: Vec::new(),
            envelope: is_envelope,
        }
    }
    pub(crate) fn builder(&mut self) -> any_pointer::Builder<'_> {
        let mut result = match &mut self.request {
            RequestMessage::Root(message) => message.get_root::<any_pointer::Builder>().unwrap(),
            RequestMessage::Envelope(message) => message.params(),
            RequestMessage::Frozen(_) => unreachable!("sent request is immutable"),
        };
        result.imbue_mut(&mut self.cap_table);
        result
    }
    pub(crate) fn into_request(
        self,
        interface_id: u64,
        method_id: u16,
        hints: CallHints,
        client: crate::scoped::Hook,
        context: Option<Rc<dyn Any>>,
        third_party: Option<Rc<message::Builder<message::HeapAllocator>>>,
    ) -> Request {
        Request {
            third_party,
            context,
            message: self.request,
            cap_table: self.cap_table,
            interface_id,
            method_id,
            client: Some(RequestClient::Scoped(client)),
            hints,
            is_streaming: false,
        }
    }

    pub(crate) fn prepared_call(
        &self,
    ) -> Option<&capntproto_rpc_wire::owned::connection::PreparedCall<message::HeapAllocator>> {
        if !self.cap_table.is_empty() {
            return None;
        }
        match &self.request {
            RequestMessage::Envelope(message) => Some(message),
            RequestMessage::Frozen(message) => Some(message),
            RequestMessage::Root(_) => None,
        }
    }
}

impl ParamsHook for Params {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn get(&self) -> ::capnp::Result<any_pointer::Reader<'_>> {
        let request = match &self.request {
            RequestMessage::Root(message) => message,
            RequestMessage::Envelope(message) => message.message(),
            RequestMessage::Frozen(message) => message.message(),
        };
        let mut result: any_pointer::Reader = if self.envelope {
            let root = request.get_root_as_reader::<crate::rpc_capnp::message::Reader>()?;
            let crate::rpc_capnp::message::Call(call) = root.which()? else {
                return Err(Error::failed("request envelope changed".into()));
            };
            call?.get_params()?.get_content()
        } else {
            request.get_root_as_reader()?
        };
        result.imbue(&self.cap_table);
        Ok(result)
    }
}

pub(crate) struct Results {
    context: Option<Rc<dyn std::any::Any>>,
    retained_results: OnceCell<Rc<RefCell<Option<Box<dyn ResultsDoneHook>>>>>,
    only_pipeline: bool,
    is_streaming: bool,
    message: Option<message::Builder<message::HeapAllocator>>,
    response: Option<Rc<dyn ResponseHook>>,
    cap_table: Vec<Option<Box<dyn ClientHook>>>,
    results_done_fulfiller: Option<ResultsFulfiller>,
    pipeline_sender: Option<crate::queued::PipelineInnerSender>,
}
enum ResultsFulfiller {
    Joined(oneshot::Sender<Box<dyn ResultsDoneHook>>),
    Direct(crate::scoped::response::ResultsSender),
}

impl Results {
    fn new(
        fulfiller: oneshot::Sender<Box<dyn ResultsDoneHook>>,
        pipeline_sender: crate::queued::PipelineInnerSender,
    ) -> Self {
        Self {
            retained_results: OnceCell::new(),
            context: None,
            only_pipeline: false,
            is_streaming: false,
            message: None,
            response: None,
            cap_table: Vec::new(),
            results_done_fulfiller: Some(ResultsFulfiller::Joined(fulfiller)),
            pipeline_sender: Some(pipeline_sender),
        }
    }
    pub(crate) fn direct(
        fulfiller: crate::scoped::response::ResultsSender,
        context: Option<Rc<dyn Any>>,
    ) -> Self {
        Self {
            context,
            retained_results: OnceCell::new(),
            only_pipeline: false,
            is_streaming: false,
            message: None,
            response: None,
            cap_table: Vec::new(),
            results_done_fulfiller: Some(ResultsFulfiller::Direct(fulfiller)),
            pipeline_sender: Some(crate::queued::PipelineInnerSender::disabled()),
        }
    }
    pub(crate) fn direct_pipelined(
        fulfiller: crate::scoped::response::ResultsSender,
        pipeline: crate::queued::PipelineInnerSender,
    ) -> Self {
        let mut results = Self::direct(fulfiller, None);
        results.pipeline_sender = Some(pipeline);
        results
    }

    pub(crate) fn take_wire_pipeline(
        &mut self,
    ) -> Option<Option<Rc<crate::scoped::wire::PipelineState>>> {
        let pipeline = self.pipeline_sender.as_mut()?.take_wire()?;
        self.pipeline_sender.take();
        Some(pipeline)
    }

    pub(crate) fn adopt_pipeline_clients(
        &mut self,
        pipeline: Box<dyn PipelineHook>,
        adopt: impl FnMut(&Rc<RefCell<crate::queued::ClientInner>>, Vec<PipelineOp>),
    ) -> capnp::Result<()> {
        let Some(sender) = self.pipeline_sender.take() else {
            return Err(Error::failed("set_pipeline() called twice".into()));
        };
        sender.complete_with_clients(pipeline, adopt);
        Ok(())
    }
    /// The portable payload is constructed only by validated admission or a
    /// successful owning copy. Preserve that proof instead of traversing the
    /// same graph again at the exact local sink. Arbitrary hooks still use the
    /// validating ResultsHook::set_response entry point below.
    pub(crate) fn set_admitted_response(&mut self, response: Rc<crate::scoped::wire::Payload>) {
        self.response = Some(response);
        self.message = None;
        self.cap_table.clear();
    }

    /// A final private completion can transfer its owner directly to the
    /// response future. Retained cancellation contexts and pipeline observers
    /// require the ordinary shareable ResultsDone path instead.
    // Rejection preserves the existing owner without another heap allocation.
    #[allow(clippy::result_large_err)]
    pub(crate) fn finish_admitted_response(
        &mut self,
        payload: crate::scoped::wire::Payload,
    ) -> Result<(), crate::scoped::wire::Payload> {
        if !matches!(
            self.results_done_fulfiller,
            Some(ResultsFulfiller::Direct(_))
        ) || self
            .retained_results
            .get()
            .is_some_and(|retained| Rc::strong_count(retained) > 1)
        {
            return Err(payload);
        }
        let Some(ResultsFulfiller::Direct(sender)) = self.results_done_fulfiller.take() else {
            unreachable!()
        };
        sender.send_payload(payload, self.context.take());
        Ok(())
    }
}

struct OwnedResponse {
    payload: crate::scoped::wire::Payload,
    _context: Option<Rc<dyn Any>>,
}
impl crate::scoped::response::ResultsSender {
    pub(crate) fn send_payload(
        self,
        mut payload: crate::scoped::wire::Payload,
        context: Option<Rc<dyn Any>>,
    ) {
        let context = context.and_then(|context| payload.retain_plain_context(context).err());
        if context.is_none() {
            self.send_response(payload.into_response());
            return;
        }
        self.send_response(Box::new(OwnedResponse {
            payload,
            _context: context,
        }));
    }
}
impl ResponseHook for OwnedResponse {
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        ResponseHook::get(&self.payload)
    }
}

impl Drop for Results {
    fn drop(&mut self) {
        let Some(fulfiller) = self.results_done_fulfiller.take() else {
            return;
        };
        let cap_table = ::std::mem::take(&mut self.cap_table);
        let result: Box<dyn ResultsDoneHook> = match self.response.take() {
            Some(response) => Box::new(SharedResponse {
                response,
                context: self.context.take(),
            }),
            None => Box::new(
                ResultsDone::new(self.message.take().unwrap_or_default(), cap_table)
                    .with_context(self.context.take()),
            ),
        };
        if let Some(retained) = self
            .retained_results
            .get()
            .filter(|r| Rc::strong_count(r) > 1)
        {
            *retained.borrow_mut() = Some(result.add_ref());
        }
        match fulfiller {
            ResultsFulfiller::Joined(fulfiller) => {
                let _ = fulfiller.send(result);
            }
            ResultsFulfiller::Direct(fulfiller) => fulfiller.send(result),
        }
    }
}

impl ResultsHook for Results {
    fn as_any_mut(&mut self) -> Option<&mut dyn Any> {
        Some(self)
    }
    fn retain_response_context(&mut self, context: Rc<dyn Any>) {
        crate::admission::retain_context(&mut self.context, context);
    }
    fn cancellation_guard(&self) -> Option<Box<dyn std::any::Any>> {
        Some(Box::new(
            self.retained_results
                .get_or_init(|| Rc::new(RefCell::new(None)))
                .clone(),
        ))
    }

    fn get_with_size_hint(
        &mut self,
        size_hint: Option<capnp::MessageSize>,
    ) -> ::capnp::Result<any_pointer::Builder<'_>> {
        if let Some(response) = &self.response {
            let reader = response.get()?;
            let mut message = message::Builder::new(result_allocator(Some(reader.target_size()?)));
            let mut caps = Vec::new();
            let mut root: any_pointer::Builder = message.init_root();
            root.imbue_mut(&mut caps);
            root.set_as(reader)?;
            self.message = Some(message);
            self.cap_table = caps;
            self.response = None;
        }
        let message = self
            .message
            .get_or_insert_with(|| message::Builder::new(result_allocator(size_hint)));
        let mut result: any_pointer::Builder = message.get_root()?;
        result.imbue_mut(&mut self.cap_table);
        Ok(result)
    }

    fn set_response(&mut self, response: Rc<dyn ResponseHook>) -> capnp::Result<()> {
        // Validate before replacing the previous payload. The immutable owner
        // remains independent of any subsequent mutable results builder.
        response.get()?.target_size()?;
        self.response = Some(response);
        self.message = None;
        self.cap_table.clear();
        Ok(())
    }

    fn set_pipeline(&mut self) -> capnp::Result<()> {
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
            Box::new(ResultsDone::new(message2, cap_table2).with_context(self.context.clone()))
                as Box<dyn ResultsDoneHook>;
        self.set_pipeline_from(Box::new(Pipeline::new(hook)))
    }

    fn set_pipeline_from(&mut self, pipeline: Box<dyn PipelineHook>) -> capnp::Result<()> {
        let Some(sender) = self.pipeline_sender.take() else {
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
        request: Box<dyn RequestHook>,
    ) -> (Promise<(), Error>, Box<dyn PipelineHook>) {
        if self.only_pipeline {
            let pipeline = request.send_for_pipeline();
            if let Some(sender) = self.pipeline_sender.take() {
                sender.complete(pipeline.hook.clone());
            }
            return (
                Promise::from_future(async move {
                    let _context = self;
                    futures::future::pending::<capnp::Result<()>>().await
                }),
                pipeline.hook,
            );
        }
        if self.is_streaming {
            let promise = request.send_streaming();
            let pipeline = Box::new(crate::broken::Pipeline::new(Error::failed(
                "streaming calls have no result pipeline".into(),
            )));
            if let Some(sender) = self.pipeline_sender.take() {
                sender.complete(pipeline.add_ref());
            }
            return (
                Promise::from_future(async move {
                    let result = promise.await;
                    drop(self);
                    result
                }),
                pipeline,
            );
        }
        let capnp::capability::RemotePromise { promise, pipeline } = request.send();
        if let Some(sender) = self.pipeline_sender.take() {
            sender.complete(pipeline.hook.clone());
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

// Large hints are advisory. Bound preallocation as the C++ RPC implementation
// does; payloads can still grow beyond this bound through ordinary allocation.
pub(crate) fn result_allocator(size_hint: Option<capnp::MessageSize>) -> message::HeapAllocator {
    let allocator = message::HeapAllocator::new();
    match size_hint {
        // Hints describe the payload, excluding its root pointer. Without the
        // extra word an exact-size copy spills its largest object into a second
        // segment and retains two nearly payload-sized allocations.
        Some(size) => {
            allocator.first_segment_words(size.word_count.saturating_add(1).min(1 << 20) as u32)
        }
        None => allocator.first_segment_words(256),
    }
}

struct ResultsDoneInner {
    _context: Option<Rc<dyn std::any::Any>>,
    message: ::capnp::message::Builder<::capnp::message::HeapAllocator>,
    cap_table: Vec<Option<Box<dyn ClientHook>>>,
}

struct SharedResponse {
    response: Rc<dyn ResponseHook>,
    context: Option<Rc<dyn Any>>,
}
impl ResultsDoneHook for SharedResponse {
    fn add_ref(&self) -> Box<dyn ResultsDoneHook> {
        Box::new(Self {
            response: self.response.clone(),
            context: self.context.clone(),
        })
    }
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        self.response.get()
    }
}

pub(crate) struct ResultsDone {
    inner: Rc<ResultsDoneInner>,
}

impl ResultsDone {
    pub(crate) fn new(
        message: message::Builder<message::HeapAllocator>,
        cap_table: Vec<Option<Box<dyn ClientHook>>>,
    ) -> Self {
        Self {
            inner: Rc::new(ResultsDoneInner {
                message,
                cap_table,
                _context: None,
            }),
        }
    }
    pub(crate) fn with_context(mut self, context: Option<Rc<dyn std::any::Any>>) -> Self {
        Rc::get_mut(&mut self.inner)
            .expect("unpublished results")
            ._context = context;
        self
    }
}

impl ResultsDoneHook for ResultsDone {
    fn add_ref(&self) -> Box<dyn ResultsDoneHook> {
        Box::new(Self {
            inner: self.inner.clone(),
        })
    }
    fn get(&self) -> ::capnp::Result<any_pointer::Reader<'_>> {
        let mut result: any_pointer::Reader = self.inner.message.get_root_as_reader()?;
        result.imbue(&self.inner.cap_table);
        Ok(result)
    }
}
impl ResponseHook for ResultsDone {
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        ResultsDoneHook::get(self)
    }
}

enum RequestMessage {
    Root(message::Builder<message::HeapAllocator>),
    Envelope(capntproto_rpc_wire::owned::connection::PreparedCall<message::HeapAllocator>),
    // Constructed only after ownership has left the public RequestHook editor.
    Frozen(Rc<capntproto_rpc_wire::owned::connection::PreparedCall<message::HeapAllocator>>),
}

impl RequestMessage {
    fn freeze_large(&mut self) {
        use capnp::message::ReaderSegments;
        let RequestMessage::Envelope(message) = &*self else {
            return;
        };
        let builder = message.message();
        if builder.len() != 2
            || builder.get_segment(0).unwrap().len() > 4096
            || builder.get_segment(1).unwrap().len() < 4096
        {
            return;
        }
        let RequestMessage::Envelope(message) =
            std::mem::replace(self, RequestMessage::Root(message::Builder::new_default()))
        else {
            unreachable!()
        };
        *self = RequestMessage::Frozen(message.into_shared());
    }
}

// Keep the private hosted endpoint inline until an opaque API needs a box.
// Ordinary remote requests already own a box for their parameter storage.
enum RequestClient {
    Hook(Box<dyn ClientHook>),
    Scoped(crate::scoped::Hook),
}
impl std::ops::Deref for RequestClient {
    type Target = dyn ClientHook;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Hook(hook) => &**hook,
            Self::Scoped(hook) => hook,
        }
    }
}

pub(crate) struct Request {
    third_party: Option<Rc<message::Builder<message::HeapAllocator>>>,
    context: Option<Rc<dyn std::any::Any>>,
    message: RequestMessage,
    cap_table: Vec<Option<Box<dyn ClientHook>>>,
    interface_id: u64,
    method_id: u16,
    client: Option<RequestClient>,
    hints: CallHints,
    is_streaming: bool,
}

impl ParamsHook for Request {
    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        let mut content = match &self.message {
            RequestMessage::Root(message) => message.get_root_as_reader()?,
            RequestMessage::Envelope(message) => {
                let root = message
                    .message()
                    .get_root_as_reader::<crate::rpc_capnp::message::Reader>()?;
                let crate::rpc_capnp::message::Call(call) = root.which()? else {
                    unreachable!("private Call envelope")
                };
                call?.get_params()?.get_content()
            }
            RequestMessage::Frozen(message) => {
                let root = message
                    .message()
                    .get_root_as_reader::<crate::rpc_capnp::message::Reader>()?;
                let crate::rpc_capnp::message::Call(call) = root.which()? else {
                    unreachable!("private request envelope")
                };
                call?.get_params()?.get_content()
            }
        };
        content.imbue(&self.cap_table);
        Ok(content)
    }
}

impl Request {
    pub(crate) fn freeze_large_params(&mut self) {
        if self.cap_table.is_empty() {
            self.message.freeze_large();
        }
    }
    pub(crate) fn send_to(
        mut self: Box<Self>,
        client: &dyn ClientHook,
    ) -> capability::RemotePromise<any_pointer::Owned> {
        debug_assert!(self.client.is_none());
        if self.hints.no_promise_pipelining
            && !self.hints.only_promise_pipeline
            && !self.is_streaming
            && self.third_party.is_none()
            && crate::scoped::response::available(client)
        {
            // The resolved queue already owns this hook. Admission retains its
            // exact endpoint and projected parent interest before returning;
            // cloning temporary boxed hooks adds no ownership protection.
            let context = self.context.take();
            return crate::scoped::response::request(
                client,
                self.interface_id,
                self.method_id,
                self.hints,
                self,
                context,
            );
        }
        self.client = Some(RequestClient::Hook(client.add_ref()));
        self.send()
    }
    pub(crate) fn batch_content_bytes(&self) -> Option<usize> {
        if !self.cap_table.is_empty() {
            return None;
        }
        let size = self.get().ok()?.target_size().ok()?;
        if size.cap_count != 0 {
            return None;
        }
        usize::try_from(size.word_count).ok()?.checked_mul(8)
    }
    pub(crate) fn single_capability_table(&self) -> Option<&[Option<Box<dyn ClientHook>>]> {
        matches!(self.cap_table.as_slice(), [Some(_)]).then_some(&self.cap_table)
    }
    pub(crate) fn prepared_call(
        &self,
    ) -> Option<&capntproto_rpc_wire::owned::connection::PreparedCall<message::HeapAllocator>> {
        if !self.cap_table.is_empty() {
            return None;
        }
        match &self.message {
            RequestMessage::Envelope(message) => Some(message),
            RequestMessage::Frozen(message) => Some(message),
            _ => None,
        }
    }
    pub(crate) fn new(
        interface_id: u64,
        method_id: u16,
        size_hint: Option<::capnp::MessageSize>,
        client: Box<dyn ClientHook>,
    ) -> Self {
        Self {
            message: RequestMessage::Root(message::Builder::new(result_allocator(size_hint))),
            third_party: None,
            context: None,
            cap_table: Vec::new(),
            interface_id,
            method_id,
            client: Some(RequestClient::Hook(client)),
            hints: CallHints::default(),
            is_streaming: false,
        }
    }
}

impl RequestHook for Request {
    fn third_party_tail_target(&self) -> Option<Box<dyn std::any::Any>> {
        crate::scoped::tail_target(self.client.as_deref().expect("unsent request"))
    }
    fn set_third_party_tail_target(
        &mut self,
        contact: any_pointer::Reader<'_>,
    ) -> capnp::Result<()> {
        if self.third_party.is_some() || self.third_party_tail_target().is_none() {
            return Err(Error::failed("ineligible tail transfer".into()));
        }
        contact.validate_capability_indices(0)?;
        let mut body = message::Builder::new_default();
        body.set_root(contact)?;
        self.third_party = Some(Rc::new(body));
        Ok(())
    }
    fn set_hints(&mut self, hints: CallHints) {
        self.hints = hints;
    }
    fn send_for_pipeline(mut self: Box<Self>) -> any_pointer::Pipeline {
        self.hints.only_promise_pipeline = true;
        self.hints.no_promise_pipelining = false;
        self.send().pipeline
    }

    fn get(&mut self) -> any_pointer::Builder<'_> {
        let mut result: any_pointer::Builder = match &mut self.message {
            RequestMessage::Root(message) => message.get_root().unwrap(),
            RequestMessage::Envelope(message) => message.params(),
            RequestMessage::Frozen(_) => unreachable!("sent request is immutable"),
        };
        result.imbue_mut(&mut self.cap_table);
        result
    }
    fn get_brand(&self) -> usize {
        0
    }
    fn send(mut self: Box<Self>) -> capability::RemotePromise<any_pointer::Owned> {
        if self.hints.no_promise_pipelining
            && !self.hints.only_promise_pipeline
            && !self.is_streaming
            && self.third_party.is_none()
        {
            let queued = self
                .client
                .as_deref()
                .expect("unsent request")
                .as_any()
                .and_then(|client| client.downcast_ref::<crate::queued::Client>())
                .and_then(crate::queued::Client::pending_wire);
            if let Some(queued) = queued {
                // Keep the request intact until its wire projection resolves.
                // The queue's waiter owns the client; storing it in the request
                // too would form a cycle and delay cancellation.
                self.client.take();
                return queued.send_request(self);
            }
        }
        if self.hints.no_promise_pipelining
            && !self.hints.only_promise_pipeline
            && !self.is_streaming
            && self.third_party.is_none()
            && match self.client.as_ref().expect("unsent request") {
                RequestClient::Scoped(hook) => hook.direct_response_available(),
                RequestClient::Hook(hook) => crate::scoped::response::available(&**hook),
            }
        {
            // The request already owns exactly the parameter storage required
            // by this call. Transfer its box instead of allocating another
            // ParamsHook and moving the builder and capability table into it.
            let client = self.client.take().unwrap();
            let context = self.context.take();
            return match client {
                RequestClient::Scoped(hook) => crate::scoped::response::request_scoped(
                    &hook,
                    self.interface_id,
                    self.method_id,
                    self.hints,
                    self,
                    context,
                ),
                RequestClient::Hook(hook) => crate::scoped::response::request(
                    &*hook,
                    self.interface_id,
                    self.method_id,
                    self.hints,
                    self,
                    context,
                ),
            };
        }
        let tmp = *self;
        let Self {
            third_party,
            message,
            context,
            cap_table,
            interface_id,
            method_id,
            client,
            hints,
            is_streaming,
        } = tmp;
        let client = client.expect("unsent request");
        let (request, envelope) = match message {
            RequestMessage::Frozen(message) => (RequestMessage::Frozen(message), true),
            RequestMessage::Root(message) => (RequestMessage::Root(message), false),
            RequestMessage::Envelope(message)
                if hints.no_promise_pipelining && !hints.only_promise_pipeline =>
            {
                (RequestMessage::Envelope(message), true)
            }
            RequestMessage::Envelope(message) => {
                let mut message = message.into_inner();
                let root = message
                    .get_root::<crate::rpc_capnp::message::Builder>()
                    .unwrap();
                let crate::rpc_capnp::message::Call(call) = root.which().unwrap() else {
                    unreachable!("private request envelope")
                };
                let mut call = call.unwrap();
                call.set_no_promise_pipelining(hints.no_promise_pipelining);
                call.set_only_promise_pipeline(hints.only_promise_pipeline);
                (RequestMessage::Root(message), true)
            }
        };
        let params = Params {
            request,
            cap_table,
            envelope,
        };
        // Hints are final at send (send_for_pipeline may change them). Allocate
        // pipeline state only for callers that can observe a pipeline.
        let (pipeline_sender, pipeline) = if hints.no_promise_pipelining {
            (crate::queued::PipelineInnerSender::disabled(), None)
        } else {
            let (sender, pipeline) = crate::queued::Pipeline::new();
            (sender, Some(pipeline))
        };

        let (results_done_fulfiller, results_done_promise) =
            oneshot::channel::<Box<dyn ResultsDoneHook>>();
        let results_done_promise = results_done_promise.map_err(crate::canceled_to_error);
        let mut results = Results::new(results_done_fulfiller, pipeline_sender.weak_clone());
        results.context = context;
        results.only_pipeline = hints.only_promise_pipeline;
        results.is_streaming = is_streaming;
        let promise = if let Some(contact) = third_party {
            crate::scoped::call_third_party(
                &*client,
                interface_id,
                method_id,
                Box::new(params),
                Box::new(results),
                hints,
                contact,
            )
        } else {
            client.call_with_hints(
                interface_id,
                method_id,
                Box::new(params),
                Box::new(results),
                hints,
            )
        };

        if hints.no_promise_pipelining {
            return capability::RemotePromise {
                promise: Promise::from_future(async move {
                    let ((), results) =
                        futures::future::try_join(promise, results_done_promise).await?;
                    Ok(capability::Response::new(Box::new(Response::new(results))))
                }),
                pipeline: any_pointer::Pipeline::new(Box::new(crate::broken::DisabledPipeline)),
            };
        }
        let mut pipeline = pipeline.expect("enabled pipeline");
        let p = futures::future::try_join(promise, results_done_promise).and_then(
            move |((), results_done_hook)| {
                pipeline_sender
                    .complete(Box::new(Pipeline::new(results_done_hook.add_ref()))
                        as Box<dyn PipelineHook>);
                Promise::ok((
                    capability::Response::new(Box::new(Response::new(results_done_hook))),
                    (),
                ))
            },
        );

        let (left, right) = crate::split::split(p);

        pipeline.drive(right);
        let pipeline = any_pointer::Pipeline::new(Box::new(pipeline));

        capability::RemotePromise {
            promise: Promise::from_future(left),
            pipeline,
        }
    }
    fn send_streaming(mut self: Box<Self>) -> Promise<(), Error> {
        // Local servers complete normally. If this capability resolves remotely,
        // direct_tail_call must preserve streaming credit-based readiness.
        self.is_streaming = true;
        // Admit at send(), just like ordinary requests. Delaying self.send()
        // until first poll lets a subsequent ordinary call overtake the stream.
        let promise = self.send().promise;
        Promise::from_future(async move {
            let _ = promise.await?;
            Ok(())
        })
    }
    fn tail_send(self: Box<Self>) -> Option<(u32, Promise<(), Error>, Box<dyn PipelineHook>)> {
        self.third_party.as_ref()?;
        let capability::RemotePromise { promise, pipeline } = self.send();
        // Portable transfer never uses numeric IDs to identify another driver.
        Some((
            0,
            Promise::from_future(async move {
                promise.await?;
                Ok(())
            }),
            pipeline.hook,
        ))
    }
}

struct PipelineInner {
    results: Box<dyn ResultsDoneHook>,
}

pub(crate) struct Pipeline {
    inner: Rc<RefCell<PipelineInner>>,
}

impl Pipeline {
    pub(crate) fn share_response(
        response: capability::Response<any_pointer::Owned>,
    ) -> (
        capability::Response<any_pointer::Owned>,
        Box<dyn PipelineHook>,
    ) {
        let results = SharedResponse {
            response: Rc::from(response.hook),
            context: None,
        };
        let pipeline = Box::new(Self::new(results.add_ref()));
        let response = capability::Response::new(Box::new(Response::new(Box::new(results))));
        (response, pipeline)
    }

    pub(crate) fn new(results: Box<dyn ResultsDoneHook>) -> Self {
        Self {
            inner: Rc::new(RefCell::new(PipelineInner { results })),
        }
    }
}

impl Clone for Pipeline {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl PipelineHook for Pipeline {
    fn add_ref(&self) -> Box<dyn PipelineHook> {
        Box::new(self.clone())
    }
    fn get_pipelined_cap(&self, ops: &[PipelineOp]) -> Box<dyn ClientHook> {
        match self
            .inner
            .borrow_mut()
            .results
            .get()
            .unwrap()
            .get_pipelined_cap(ops)
        {
            Ok(v) => v,
            Err(e) => Box::new(crate::broken::Client::new(e, true, 0)) as Box<dyn ClientHook>,
        }
    }
}

pub(crate) struct Client<S>
where
    S: capability::Server + Clone,
{
    state: Rc<RefCell<ClientState<S>>>,
}

type LocalKey = (TypeId, usize);
struct LocalRegistration {
    state: Weak<dyn Any>,
    live: Weak<Cell<bool>>,
    revocable: bool,
}
impl LocalRegistration {
    fn is_live(&self) -> bool {
        self.live.upgrade().is_some_and(|live| live.get())
    }
}
thread_local! {
    // Weak entries do not keep application servers alive. Different generated
    // interface views of a Rust server have distinct dispatch implementations.
    static LOCAL_CLIENTS: RefCell<HashMap<usize, HashMap<TypeId, LocalRegistration>>> = RefCell::default();
}

struct ClientState<S>
where
    S: capability::Server + Clone,
{
    inner: Option<S>,
    identity: usize,
    registration: Option<LocalKey>,
    live: Rc<Cell<bool>>,
    resolved: Option<Box<dyn ClientHook>>,
    resolve_task: Option<futures::future::Shared<Promise<(), Error>>>,
    resolve_canceler: Rc<crate::revocable::Canceler>,
    revocation: Option<Rc<crate::revocable::Canceler>>,
    executor: Option<Rc<dyn capability::CallExecutor>>,
    #[cfg(unix)]
    fd: Option<std::rc::Rc<std::os::fd::OwnedFd>>,

    /// If a streaming call on this capability has returned an error,
    /// this contains a copy of that error.
    broken_error: Option<Error>,

    /// True while a streaming call is in flight. Later calls must wait here so
    /// non-streaming calls, like EOF methods, cannot overtake streaming writes.
    blocked: bool,
    blocked_calls: VecDeque<BlockedCall>,
}

impl<S: capability::Server + Clone> Drop for ClientState<S> {
    fn drop(&mut self) {
        if let Some(key) = self.registration {
            let _ = LOCAL_CLIENTS.try_with(|clients| {
                if let Ok(mut clients) = clients.try_borrow_mut() {
                    if let Some(views) = clients.get_mut(&key.1) {
                        if views
                            .get(&key.0)
                            .is_some_and(|entry| entry.state.strong_count() == 0)
                        {
                            views.remove(&key.0);
                        }
                        if views.is_empty() {
                            clients.remove(&key.1);
                        }
                    }
                }
            });
        }
        self.resolve_canceler
            .cancel(Error::failed("local client dropped".into()));
    }
}

pub(crate) struct Revoker {
    revoke: Box<dyn Fn(Error)>,
    in_use: Box<dyn Fn() -> bool>,
}
impl Revoker {
    pub(crate) fn revoke(&self, error: Error) {
        (self.revoke)(error);
    }
    pub(crate) fn is_in_use(&self) -> bool {
        (self.in_use)()
    }
}

enum BlockedCall {
    Call {
        interface_id: u64,
        method_id: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
        fulfiller: oneshot::Sender<Promise<(), Error>>,
    },
    Barrier(oneshot::Sender<()>),
    Redirect(ShorteningBarrier),
}

struct ShorteningBarrier {
    client: Weak<RefCell<crate::queued::ClientInner>>,
    target: Option<Box<dyn ClientHook>>,
}
impl ShorteningBarrier {
    fn complete(mut self) {
        let target = self.target.take().unwrap();
        if let Some(client) = self.client.upgrade() {
            crate::queued::ClientInner::resolve(&client, Ok(target));
        }
    }
}
impl Drop for ShorteningBarrier {
    fn drop(&mut self) {
        if self.target.is_some() {
            if let Some(client) = self.client.upgrade() {
                crate::queued::ClientInner::resolve(
                    &client,
                    Err(Error::failed("shortening barrier dropped".into())),
                );
            }
        }
    }
}

struct StreamingCall<S>
where
    S: capability::Server + Clone + 'static,
{
    state: Rc<RefCell<ClientState<S>>>,
    promise: Promise<(), Error>,
    completed: bool,
}

impl<S> Unpin for StreamingCall<S> where S: capability::Server + Clone + 'static {}

impl<S> Future for StreamingCall<S>
where
    S: capability::Server + Clone + 'static,
{
    type Output = Result<(), Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match Pin::new(&mut this.promise).poll(cx) {
            Poll::Ready(result) => {
                if let Err(e) = &result {
                    this.state.borrow_mut().broken_error = Some(e.clone());
                }
                this.completed = true;
                ClientState::unblock(this.state.clone());
                Poll::Ready(result)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<S> Drop for StreamingCall<S>
where
    S: capability::Server + Clone + 'static,
{
    fn drop(&mut self) {
        if !self.completed {
            ClientState::unblock(self.state.clone());
        }
    }
}

impl<S> ClientState<S>
where
    S: capability::Server + Clone + 'static,
{
    fn revoke(state: &Rc<RefCell<Self>>, error: Error) {
        let (server, revocation, resolution, queued) = {
            let mut state = state.borrow_mut();
            let Some(server) = state.inner.take() else {
                return;
            };
            state.live.set(false);
            state.broken_error = Some(error.clone());
            state.blocked = false;
            #[cfg(unix)]
            {
                state.fd = None;
            }
            (
                server,
                state.revocation.clone(),
                state.resolve_canceler.clone(),
                std::mem::take(&mut state.blocked_calls),
            )
        };
        if let Some(revocation) = revocation {
            revocation.cancel(error.clone());
        }
        resolution.cancel(error.clone());
        for call in queued {
            match call {
                BlockedCall::Call { fulfiller, .. } => {
                    let _ = fulfiller.send(Promise::err(error.clone()));
                }
                BlockedCall::Barrier(fulfiller) => {
                    let _ = fulfiller.send(());
                }
                BlockedCall::Redirect(barrier) => barrier.complete(),
            }
        }
        drop(server);
    }

    fn dispatch_or_queue(
        state: Rc<RefCell<Self>>,
        interface_id: u64,
        method_id: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
    ) -> Promise<(), Error> {
        {
            let mut state_ref = state.borrow_mut();
            if let Some(e) = &state_ref.broken_error {
                return Promise::err(e.clone());
            }

            if state_ref.blocked {
                let (fulfiller, promise) = oneshot::channel();
                state_ref.blocked_calls.push_back(BlockedCall::Call {
                    interface_id,
                    method_id,
                    params,
                    results,
                    fulfiller,
                });

                // The queued call has not started yet. Resolve this promise
                // with the call's real promise once it reaches the front.
                return Promise::from_future(async move {
                    match promise.await {
                        Ok(p) => p.await,
                        Err(e) => Err(crate::canceled_to_error(e)),
                    }
                });
            }
        }

        Self::dispatch_call(state, interface_id, method_id, params, results)
    }

    fn dispatch_call(
        state: Rc<RefCell<Self>>,
        interface_id: u64,
        method_id: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
    ) -> Promise<(), Error> {
        let mut context = capnp::private::capability::OwnedCall::new(params, results);
        Self::dispatch_context(state, interface_id, method_id, &mut context)
    }

    fn dispatch_context(
        state: Rc<RefCell<Self>>,
        interface_id: u64,
        method_id: u16,
        context: &mut dyn capnp::private::capability::CallContextHook,
    ) -> Promise<(), Error> {
        let inner = {
            let state_ref = state.borrow();
            if let Some(e) = &state_ref.broken_error {
                return Promise::err(e.clone());
            }
            state_ref.inner.as_ref().expect("live server").clone()
        };

        let results = context.results();
        let permits_immediate_poll = results.permits_immediate_poll();
        let guard = results.cancellation_context();
        let executor = state
            .borrow()
            .executor
            .clone()
            .or_else(|| results.cancellation_executor());
        let mut f = inner.dispatch_call(
            interface_id,
            method_id,
            ::capnp::capability::CallContext::new(context),
        );

        let revocation = state.borrow().revocation.clone();
        if let Some(revocation) = revocation {
            f.promise = revocation.wrap(f.promise);
        }

        if !f.allow_cancellation {
            let Some(executor) = executor else {
                return Promise::err(Error::failed(
                    "non-cancellable local call requires new_client_with_executor()".into(),
                ));
            };
            // The inbound RPC dispatcher already performs a first poll with no
            // protocol tables borrowed. Finish synchronous non-streaming calls
            // before allocating a shared promise and an independent task. A
            // pending call still receives the full protected lifetime below.
            // Streaming calls must establish their ordering barrier first.
            if permits_immediate_poll && !f.is_streaming {
                if let Some(result) = (&mut f.promise).now_or_never() {
                    drop(guard);
                    return Promise::from(result);
                }
            }
            let retained_server = state.clone();
            let task = Promise::from_future(async move {
                let result = f.promise.await;
                drop(guard);
                drop(retained_server);
                result
            })
            .shared();
            if let Err(error) = executor.spawn(Promise::from_future(task.clone())) {
                return Promise::err(error);
            }
            f.promise = Promise::from_future(task);
        }

        if f.is_streaming {
            // A streaming call serializes later calls on the same local
            // capability until its server-side promise completes.
            state.borrow_mut().blocked = true;
            Promise::from_future(StreamingCall {
                state,
                promise: f.promise,
                completed: false,
            })
        } else {
            f.promise
        }
    }

    fn unblock(state: Rc<RefCell<Self>>) {
        loop {
            let blocked_call = {
                let mut state_ref = state.borrow_mut();
                state_ref.blocked = false;
                state_ref.blocked_calls.pop_front()
            };

            let Some(blocked_call) = blocked_call else {
                return;
            };

            let BlockedCall::Call {
                interface_id,
                method_id,
                params,
                results,
                fulfiller,
            } = blocked_call
            else {
                if let BlockedCall::Barrier(fulfiller) = blocked_call {
                    let _ = fulfiller.send(());
                } else if let BlockedCall::Redirect(barrier) = blocked_call {
                    barrier.complete();
                }
                continue;
            };
            if fulfiller.is_closed() {
                continue;
            }
            let promise =
                Self::dispatch_call(state.clone(), interface_id, method_id, params, results);
            let _ = fulfiller.send(promise);

            // If the unblocked call was itself streaming, dispatch_call() set
            // blocked again; leave remaining calls queued behind it.
            if state.borrow().blocked {
                return;
            }
        }
    }
}

impl<S> Client<S>
where
    S: capability::Server + Clone,
{
    pub(crate) fn is_registered(server: usize, identity: usize) -> bool
    where
        S: 'static,
    {
        let key = (TypeId::of::<S>(), server);
        let existing = LOCAL_CLIENTS.with(|clients| {
            clients
                .borrow()
                .get(&key.1)
                .and_then(|views| views.get(&key.0))
                .and_then(|entry| entry.state.upgrade())
        });
        existing
            .and_then(|state| state.downcast::<RefCell<ClientState<S>>>().ok())
            .is_some_and(|state| {
                let state = state.borrow();
                state.identity == identity && state.inner.is_some()
            })
    }

    pub(crate) fn into_hook(self) -> Box<dyn ClientHook>
    where
        S: 'static,
    {
        let key = (
            TypeId::of::<S>(),
            self.state.borrow().inner.as_ref().unwrap().as_ptr(),
        );
        let revocable = self.state.borrow().revocation.is_some();
        let (existing, conflict) = LOCAL_CLIENTS.with(|clients| {
            let clients = clients.borrow();
            match clients.get(&key.1) {
                Some(views) => (
                    views.get(&key.0).and_then(|entry| entry.state.upgrade()),
                    views
                        .values()
                        .any(|entry| entry.is_live() && (revocable || entry.revocable)),
                ),
                None => (None, false),
            }
        });
        if conflict {
            return Box::new(crate::broken::Client::new(
                Error::failed(
                    "cannot create another client for a server with a revocable client".into(),
                ),
                true,
                0,
            ));
        }
        if let Some(existing) = existing {
            let state = existing.downcast::<RefCell<ClientState<S>>>().ok().unwrap();
            if state.borrow().inner.is_some() {
                return Box::new(Self { state });
            }
        }
        self.state.borrow_mut().registration = Some(key);
        let erased: Rc<dyn Any> = self.state.clone();
        let live = Rc::downgrade(&self.state.borrow().live);
        LOCAL_CLIENTS.with(|clients| {
            clients.borrow_mut().entry(key.1).or_default().insert(
                key.0,
                LocalRegistration {
                    state: Rc::downgrade(&erased),
                    live,
                    revocable,
                },
            )
        });
        drop(erased);

        // No internal borrow spans a user hook. In particular shorten_path() may
        // call this server's self capability or construct another client alias.
        let server = self.state.borrow().inner.as_ref().unwrap().clone();
        if let Some(hooks) = server.get_hooks() {
            if let Some(slot) = hooks.self_cap() {
                let weak = Rc::downgrade(&self.state);
                slot.bind(Rc::new(move || {
                    let state = weak.upgrade()?;
                    state.borrow().inner.as_ref()?;
                    Some(Box::new(Self { state }))
                }));
            }
            if let Some(promise) = hooks.shorten_path() {
                self.start_shortening(promise);
            }
        }
        Box::new(self)
    }

    fn start_shortening(&self, promise: Promise<capability::Client, Error>)
    where
        S: 'static,
    {
        let weak = Rc::downgrade(&self.state);
        let task = Promise::from_future(async move {
            let target = promise.await?;
            let Some(state) = weak.upgrade() else {
                return Ok(());
            };
            // A direct self-resolution would otherwise form a reference cycle
            // and make when_resolved() loop forever.
            if target.hook.get_ptr() == state.borrow().identity && target.hook.get_brand() == 0 {
                return Err(Error::failed("shorten_path resolved to itself".into()));
            }
            let mut state = state.borrow_mut();
            if state.inner.is_none() {
                return Err(state.broken_error.clone().unwrap());
            }
            let target = if state.blocked {
                let queued = crate::queued::Client::new(None);
                state
                    .blocked_calls
                    .push_back(BlockedCall::Redirect(ShorteningBarrier {
                        client: Rc::downgrade(&queued.inner),
                        target: Some(target.hook),
                    }));
                // Resolve as soon as this barrier leaves the old queue, even if
                // no observer happens to be polling the replacement capability.
                Box::new(queued) as Box<dyn ClientHook>
            } else {
                target.hook
            };
            state.resolved = Some(target);
            Ok(())
        });
        let (canceler, executor) = {
            let state = self.state.borrow();
            (state.resolve_canceler.clone(), state.executor.clone())
        };
        let task = canceler.wrap(task).shared();
        self.state.borrow_mut().resolve_task = Some(task.clone());
        if let Some(executor) = executor {
            // Failure to drive eagerly does not break old-route calls. Resolution
            // waiters and live calls can also drive this shared future.
            let _ = executor.spawn(Promise::from_future(task));
        }
    }

    pub(crate) fn into_revocable(self) -> (Self, Revoker)
    where
        S: 'static,
    {
        {
            let mut state = self.state.borrow_mut();
            state.revocation = Some(Rc::new(crate::revocable::Canceler::default()));
        }
        let weak = Rc::downgrade(&self.state);
        let usage = weak.clone();
        let revoker = Revoker {
            revoke: Box::new(move |error| {
                if let Some(state) = weak.upgrade() {
                    ClientState::revoke(&state, error);
                }
            }),
            in_use: Box::new(move || usage.strong_count() > 1),
        };
        (self, revoker)
    }

    pub(crate) fn new_with_executor(server: S, executor: Rc<dyn capability::CallExecutor>) -> Self {
        Self::new(server).with_executor(executor)
    }

    pub(crate) fn with_executor(self, executor: Rc<dyn capability::CallExecutor>) -> Self {
        self.state.borrow_mut().executor = Some(executor);
        self
    }

    #[cfg(unix)]
    pub(crate) fn new_with_fd(server: S, fd: std::rc::Rc<std::os::fd::OwnedFd>) -> Self {
        let client = Self::new(server);
        client.state.borrow_mut().fd = Some(fd);
        client
    }

    pub(crate) fn new(server: S) -> Self {
        let client = Self {
            state: Rc::new(RefCell::new(ClientState {
                identity: 0,
                registration: None,
                live: Rc::new(Cell::new(true)),
                resolved: None,
                resolve_task: None,
                resolve_canceler: Rc::new(crate::revocable::Canceler::default()),
                inner: Some(server),
                revocation: None,
                executor: None,
                #[cfg(unix)]
                fd: None,
                broken_error: None,
                blocked: false,
                blocked_calls: VecDeque::new(),
            })),
        };
        client.state.borrow_mut().identity = Rc::as_ptr(&client.state) as usize;
        client
    }
}

impl<S> Clone for Client<S>
where
    S: capability::Server + Clone,
{
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl<S> ClientHook for Client<S>
where
    S: capability::Server + Clone + 'static,
{
    fn get_failure(&self) -> Option<Error> {
        let state = self.state.borrow();
        // shorten_path explicitly released replacement authority. As with an
        // already-resolved RPC promise, revoking its former owner cannot recall
        // that replacement; Join will check it on the next resolution step.
        if state.resolved.is_some() {
            None
        } else {
            state.broken_error.clone()
        }
    }
    fn debug_info(&self, chain: &mut capnp::private::capability::DebugInfo) {
        let Ok(state) = self.state.try_borrow() else {
            chain.push("local(busy)");
            return;
        };
        if let Some(error) = &state.broken_error {
            chain.push("broken");
            chain.push(error.to_string());
        } else if let Some(resolved) = state.resolved.clone() {
            drop(state);
            chain.push("shortened");
            chain.follow(&*resolved);
        } else if state.inner.is_some() {
            chain.push("local");
            chain.push(std::any::type_name::<S>());
        } else {
            chain.push("revoked");
        }
    }

    fn is_local_server_ready(&self) -> bool {
        let state = self.state.borrow();
        !state.blocked && state.inner.is_some()
    }
    fn when_local_server_ready(&self) -> Promise<(), Error> {
        let promise = {
            let mut state = self.state.borrow_mut();
            if !state.blocked {
                return Promise::ok(());
            }
            let (fulfiller, promise) = oneshot::channel();
            state
                .blocked_calls
                .push_back(BlockedCall::Barrier(fulfiller));
            promise
        };
        let state = self.state.clone();
        Promise::from_future(async move {
            let result = promise.await.map_err(crate::canceled_to_error);
            drop(state);
            result
        })
    }
    #[cfg(unix)]
    fn get_fd(&self) -> Option<std::rc::Rc<std::os::fd::OwnedFd>> {
        self.state.borrow().fd.clone()
    }
    fn add_ref(&self) -> Box<dyn ClientHook> {
        Box::new(self.clone())
    }
    fn new_call(
        &self,
        interface_id: u64,
        method_id: u16,
        size_hint: Option<::capnp::MessageSize>,
    ) -> capability::Request<any_pointer::Owned, any_pointer::Owned> {
        if let Some(target) = self.get_resolved() {
            return target.new_call(interface_id, method_id, size_hint);
        }
        capability::Request::new(Box::new(Request::new(
            interface_id,
            method_id,
            size_hint,
            self.add_ref(),
        )))
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
        if let Some(target) = self.get_resolved() {
            return target.call_with_hints(interface_id, method_id, params, results, hints);
        }
        // We don't want to actually dispatch the call synchronously, because we don't want the callee
        // to have any side effects before the promise is returned to the caller.  This helps avoid
        // race conditions.
        let state = self.state.clone();
        let task = state.borrow().resolve_task.clone();
        Promise::from_future(async move {
            let call =
                ClientState::dispatch_or_queue(state, interface_id, method_id, params, results);
            if let Some(task) = task {
                match futures::future::select(call, task).await {
                    futures::future::Either::Left((result, _)) => result,
                    futures::future::Either::Right((_, call)) => call.await,
                }
            } else {
                call.await
            }
        })
    }

    fn call_during_dispatch(
        &self,
        interface_id: u64,
        method_id: u16,
        context: &mut dyn capnp::private::capability::CallContextHook,
        hints: CallHints,
    ) -> Promise<(), Error> {
        if let Some(target) = self.get_resolved() {
            return target.call_during_dispatch(interface_id, method_id, context, hints);
        }
        let state = self.state.clone();
        let task = state.borrow().resolve_task.clone();
        let blocked = state.borrow().blocked;
        let call = if blocked {
            let (params, results) = context.take();
            ClientState::dispatch_or_queue(state, interface_id, method_id, params, results)
        } else {
            ClientState::dispatch_context(state, interface_id, method_id, context)
        };
        match task {
            None => call,
            Some(task) => Promise::from_future(async move {
                match futures::future::select(call, task).await {
                    futures::future::Either::Left((result, _)) => result,
                    futures::future::Either::Right((_, call)) => call.await,
                }
            }),
        }
    }

    fn get_ptr(&self) -> usize {
        self.state.borrow().identity
    }

    fn get_brand(&self) -> usize {
        0
    }

    fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
        self.state.borrow().resolved.clone()
    }

    fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, Error>> {
        if let Some(target) = self.get_resolved() {
            return Some(Promise::ok(target));
        }
        let task = self.state.borrow().resolve_task.clone()?;
        let state = self.state.clone();
        Some(Promise::from_future(async move {
            task.await?;
            Ok(state.borrow().resolved.as_ref().unwrap().add_ref())
        }))
    }

    fn when_resolved(&self) -> Promise<(), Error> {
        crate::rpc::default_when_resolved_impl(self)
    }
}
