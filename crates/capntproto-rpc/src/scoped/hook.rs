//! Owned client-hook routing and generic forwarding.
//!
//! Exact private hook identity authorizes native operations. Opaque wrappers
//! keep their ResultsHook policy, and forwarding retains response credit even
//! when the inner payload is copied. Registry/protocol progress stays in the
//! parent module; this adapter only constructs or enqueues calls.
use super::{
    admission, response, Call, CallCompletion, CallKind, CallParams, CallPermit, Endpoint, Inbox,
    Resolution, Shutdown,
};
use capnp::{
    any_pointer,
    capability::{self, CallHints, Promise},
    private::capability::{ClientHook, DebugInfo, MetadataChanges, ParamsHook, ResultsHook},
    Error,
};
use capntproto_rpc_engine::completion::{self, shared};
use futures::FutureExt;
use std::{
    future::Future,
    pin::Pin,
    rc::{Rc, Weak},
    task::{Context, Poll},
};

#[derive(Clone)]
pub(crate) struct Hook(
    pub(super) Rc<Endpoint>,
    pub(super) Option<Rc<admission::Credit>>,
);
impl Hook {
    fn new_request(
        &self,
        interface: u64,
        method: u16,
        size: Option<capnp::MessageSize>,
        hints: CallHints,
    ) -> capnp::Result<Box<Call>> {
        let inbox = self.0.inbox.upgrade();
        let credit = inbox
            .as_ref()
            .and_then(|inbox| inbox.admission.clone())
            .map(admission::Credit::new);
        if self.0.remote.get() {
            if let Some(credit) = &credit {
                credit.ensure()?;
            }
        }
        let allocator = crate::local::result_allocator(size);
        let allocator = match inbox.as_ref().and_then(|inbox| inbox.message_pool.as_ref()) {
            Some(pool) => allocator.segment_pool(pool.clone()),
            None => allocator,
        };
        let template = self.0.remote.get().then(|| {
            inbox
                .as_ref()
                .map(|inbox| inbox.call_template(interface, method))
        });
        let call = Call::boxed(inbox.as_deref(), || {
            let request = crate::local::Params::scoped(interface, method, allocator, template);
            Call {
                kind: CallKind::Ordinary,
                credit,
                interface,
                method,
                hints,
                params: CallParams::Request(request),
                results: response::Results::Unsent,
                completion: CallCompletion::Unsent,
                shutdown: inbox
                    .as_ref()
                    .map_or_else(Shutdown::new, |inbox| inbox.shutdown.clone()),
                permit: CallPermit::new(self.0.clone()),
            }
        });
        Ok(call)
    }
    fn call_to(
        &self,
        interface: u64,
        method: u16,
        params: Box<dyn ParamsHook>,
        results: Box<dyn ResultsHook>,
        hints: CallHints,
        third_party: Option<Rc<capnp::message::Builder<capnp::message::HeapAllocator>>>,
    ) -> Promise<(), Error> {
        if let Some(error) = self.0.failure.borrow().clone() {
            return Promise::err(error);
        }
        if third_party.is_none() {
            if let Some(client) = self.introduced() {
                return client.call_with_hints(interface, method, params, results, hints);
            }
        }
        let (sender, receiver) = shared::channel();
        let sender: completion::Sender<'static, capnp::Result<()>> = sender.into();
        match self.enqueue(
            interface,
            method,
            CallParams::hook(params),
            response::Results::Hook(results),
            hints,
            third_party,
            CallCompletion::Promise(sender.track()),
        ) {
            Ok(inbox) => Promise::from_future(Reply {
                receiver: Some(receiver),
                shutdown: inbox.shutdown.clone(),
                inbox: Rc::downgrade(&inbox),
            }),
            Err(error) => Promise::err(*error),
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    pub(super) fn enqueue(
        &self,
        interface: u64,
        method: u16,
        params: CallParams,
        results: response::Results,
        hints: CallHints,
        third_party: Option<Rc<capnp::message::Builder<capnp::message::HeapAllocator>>>,
        completion: CallCompletion,
    ) -> Result<Rc<Inbox>, Box<Error>> {
        let inbox = self.0.inbox.upgrade();
        let call = Call::boxed(inbox.as_deref(), || Call {
            kind: third_party.map_or(CallKind::Ordinary, CallKind::ThirdParty),
            credit: self.1.clone(),
            interface,
            method,
            hints,
            params,
            results,
            completion,
            shutdown: inbox
                .as_ref()
                .map_or_else(Shutdown::new, |inbox| inbox.shutdown.clone()),
            permit: CallPermit::new(self.0.clone()),
        });
        call.enqueue_owned(inbox)
    }

    // A projection can bind to another unresolved promise. Resolution observers
    // must see that first step; only when_resolved follows the remaining chain.
    fn wait_for_resolution(&self, follow: bool) -> Promise<(), Error> {
        let Some(inbox) = self.0.inbox.upgrade() else {
            return Promise::err(Error::disconnected(
                "scoped ClientHook driver dropped".into(),
            ));
        };
        let endpoint = self.0.clone();
        let shutdown = inbox.shutdown.clone();
        let resolved = endpoint
            .resolution
            .borrow_mut()
            .get_or_insert_with(|| {
                let (sender, receiver) = shared::channel();
                Resolution {
                    sender,
                    receiver: receiver.shared(),
                }
            })
            .receiver
            .clone();
        Promise::from_future(async move {
            // The waiter retains its graph route. Bridge close/drop wakes it
            // through shutdown even though notification state is endpoint-owned.
            let result = match futures::future::select(shutdown, resolved).await {
                futures::future::Either::Right((result, _)) => result
                    .map_err(crate::canceled_to_error)
                    .and_then(core::convert::identity),
                futures::future::Either::Left((reason, resolved)) => {
                    if !endpoint.remote.get() && endpoint.introduced.borrow().is_some() {
                        // Teardown drops older queued calls before revealing a
                        // known local authority. That authority survives its
                        // former connection, including deferred resolution.
                        resolved.await.map_err(crate::canceled_to_error)?
                    } else {
                        Err(reason.unwrap_or_else(|_| {
                            Error::disconnected("scoped ClientHook driver dropped".into())
                        }))
                    }
                }
            };
            result?;
            let forwarded = endpoint.introduced.borrow().clone();
            if follow {
                if let Some(forwarded) = forwarded {
                    forwarded.when_resolved().await?;
                }
            }
            Ok(())
        })
    }

    pub(crate) fn direct_response_available(&self) -> bool {
        self.introduced().is_none()
    }
    #[inline(always)]
    pub(super) fn introduced(&self) -> Option<Box<dyn ClientHook>> {
        if self.0.local_deferred.get() {
            return None;
        }
        if let Some(client) = self.0.introduced.borrow().as_ref() {
            return Some(client.add_ref());
        }
        let contact = self.0.introduction.borrow().clone()?;
        // Match legacy ThirdPartyClient::ensure_accepted: install a terminal
        // reentrancy result before entering caller-owned network callbacks.
        // Reentrant calls observe it rather than starting another acceptance.
        *self.0.introduced.borrow_mut() = Some(Rc::from(crate::broken::new_cap(Error::failed(
            "reentrant third-party acceptance".into(),
        ))));
        let result = (|| {
            let inbox = self
                .0
                .inbox
                .upgrade()
                .ok_or_else(|| Error::disconnected("introduction driver closed".into()))?;
            inbox
                .joins
                .as_ref()
                .ok_or_else(|| Error::unimplemented("introduction network unavailable".into()))?
                .introduced(self.clone(), contact.get_root_as_reader()?)
        })();
        let client: Rc<dyn ClientHook> = Rc::from(result.unwrap_or_else(crate::broken::new_cap));
        let result = client.add_ref();
        *self.0.introduced.borrow_mut() = Some(client);
        Some(result)
    }
}
impl ClientHook for Hook {
    fn join_capabilities(
        &self,
        caps: Vec<Box<dyn ClientHook>>,
    ) -> Promise<capnp::private::capability::JoinedCapability, Error> {
        if let Some(client) = self.introduced() {
            return client.join_capabilities(caps);
        }
        let Some(inbox) = self.0.inbox.upgrade() else {
            return Promise::err(Error::disconnected("Join connection ended".into()));
        };
        let brand = Rc::as_ptr(&inbox) as usize;
        if !caps.iter().all(|cap| cap.get_brand() == brand) {
            return Promise::err(Error::unimplemented(
                "Join capabilities use different routes".into(),
            ));
        }
        match &inbox.joins {
            Some(joins) => joins.join_capabilities(caps),
            None => Promise::err(Error::unimplemented("bilateral Join unavailable".into())),
        }
    }
    fn metadata_changes(&self) -> Option<Rc<MetadataChanges>> {
        let introduced = self.0.introduced.borrow().clone();
        if let Some(client) = introduced {
            return client.metadata_changes();
        }
        Some(
            self.0
                .metadata_changes
                .get_or_init(|| Rc::new(MetadataChanges::default()))
                .clone(),
        )
    }

    fn get_failure(&self) -> Option<Error> {
        // Inspect an already accepted route without triggering an introduction.
        let introduced = self.0.introduced.borrow().clone();
        if let Some(client) = introduced {
            return client.get_failure();
        }
        self.0
            .failure
            .borrow()
            .clone()
            .or_else(|| match self.0.inbox.upgrade() {
                Some(inbox) => inbox.shutdown.reason(),
                None => Some(Error::disconnected(
                    "scoped ClientHook driver dropped".into(),
                )),
            })
    }
    fn forward_join(
        &self,
        part: any_pointer::Reader<'_>,
    ) -> Option<crate::multiparty::PartResponse> {
        if let Some(client) = self.introduced() {
            return client.forward_join(part);
        }
        let inbox = self.0.inbox.upgrade()?;
        inbox
            .joins
            .as_ref()
            .map(|joins| joins.join(self.add_ref(), part))
    }
    #[cfg(unix)]
    fn get_fd(&self) -> Option<Rc<std::os::fd::OwnedFd>> {
        let introduced = self.0.introduced.borrow().clone();
        if let Some(client) = introduced {
            return client.get_fd();
        }
        self.0.fd.borrow().clone()
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn debug_info(&self, info: &mut DebugInfo) {
        info.push("scopedEndpoint");
    }
    fn add_ref(&self) -> Box<dyn ClientHook> {
        Box::new(Self(self.0.clone(), self.1.clone()))
    }
    fn new_call(
        &self,
        interface: u64,
        method: u16,
        size: Option<capnp::MessageSize>,
    ) -> capability::Request<any_pointer::Owned, any_pointer::Owned> {
        self.new_call_with_hints(interface, method, size, CallHints::default())
    }
    fn new_call_with_hints(
        &self,
        interface: u64,
        method: u16,
        size: Option<capnp::MessageSize>,
        hints: CallHints,
    ) -> capability::Request<any_pointer::Owned, any_pointer::Owned> {
        if let Some(client) = self.introduced() {
            return client.new_call_with_hints(interface, method, size, hints);
        }
        match self.new_request(interface, method, size, hints) {
            Ok(call) => capability::Request::new(call),
            Err(error) => {
                capability::Request::new(Box::new(crate::broken::Request::new(error, size)))
            }
        }
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
        mut results: Box<dyn ResultsHook>,
        hints: CallHints,
    ) -> Promise<(), Error> {
        if self.0.inbox.upgrade().is_some_and(|inbox| {
            inbox.stream_factory.is_some() || (self.0.remote.get() && inbox.native_tail)
        }) {
            // Match legacy Client::call_with_hints even before the route settles
            // or when optimized tail transfer is disabled. ResultsHook chooses
            // streaming credit, native/third-party transfer or ordinary forwarding.
            let request = params.get().and_then(|params| {
                let size = Some(params.target_size()?);
                let mut request = if let Some(client) = self.introduced() {
                    client.new_call_with_hints(interface, method, size, hints)
                } else {
                    let call = self.new_request(interface, method, size, hints)?;
                    // Forwarding may copy the inner reply through opaque result
                    // hooks. Preserve the same reservation in the outer reply,
                    // independently of that copy and the inner response owner.
                    if let Some(credit) = &call.credit {
                        results.retain_response_context(credit.clone());
                    }
                    capability::Request::new(call)
                };
                request.get().set_as(params)?;
                Ok(request)
            });
            return match request {
                Ok(request) => results.tail_call(request.hook),
                Err(error) => Promise::err(error),
            };
        }
        self.call_to(interface, method, params, results, hints, None)
    }
    // Opaque local brand: the legacy network must not treat this as one of its
    // wire clients or infer Join/tail-call/FD authority from graph identity.
    fn get_brand(&self) -> usize {
        self.0
            .inbox
            .upgrade()
            .filter(|inbox| inbox.joins.is_some())
            .map_or(0, |inbox| Rc::as_ptr(&inbox) as usize)
    }
    fn get_ptr(&self) -> usize {
        Rc::as_ptr(&self.0) as usize
    }
    fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
        if self.0.local_deferred.get() {
            return None;
        }
        // A metadata query must not start an introduction. Legacy exposes an
        // already accepted hook here and starts acceptance from actual use.
        self.0
            .introduced
            .borrow()
            .as_ref()
            .map(|client| client.add_ref())
    }
    fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, Error>> {
        if let Some(client) = self.introduced() {
            // Match legacy PromiseClient: expose this accepted resolution
            // step immediately. Only when_resolved follows the next promise;
            // waiting on it here hides progress from late subscribers.
            return Some(Promise::ok(client));
        }
        if let Some(error) = self.0.failure.borrow().clone() {
            return Some(Promise::err(error));
        }
        if self.0.settled.get() {
            None
        } else {
            let endpoint = self.0.clone();
            let ready = self.wait_for_resolution(false);
            Some(Promise::from_future(async move {
                ready.await?;
                // A generic collect can publish graph readiness before the
                // hosted refresh supplies a local hook. That hook must still
                // wait for older calls before this waiter exposes it directly.
                if endpoint.local_deferred.get() {
                    Hook(endpoint.clone(), None)
                        .wait_for_resolution(false)
                        .await?;
                }
                // A reflected local resolution already owns its next hook.
                // Return it directly rather than boxing another scoped proxy.
                let resolved = endpoint.introduced.borrow().clone();
                Ok(match resolved {
                    Some(client) => client.add_ref(),
                    None => Box::new(Hook(endpoint, None)) as Box<dyn ClientHook>,
                })
            }))
        }
    }
    fn when_resolved(&self) -> Promise<(), Error> {
        if let Some(client) = self.introduced() {
            return client.when_resolved();
        }
        if let Some(error) = self.0.failure.borrow().clone() {
            return Promise::err(error);
        }
        if self.0.settled.get() {
            return Promise::ok(());
        }
        self.wait_for_resolution(true)
    }
}

pub(super) struct Reply {
    pub(super) receiver: Option<shared::Receiver<capnp::Result<()>>>,
    pub(super) shutdown: Shutdown,
    pub(super) inbox: Weak<Inbox>,
}
impl Future for Reply {
    type Output = capnp::Result<()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Poll::Ready(reason) = Pin::new(&mut self.shutdown).poll(cx) {
            drop(self.receiver.take());
            return Poll::Ready(Err(reason.unwrap_or_else(|_| {
                Error::disconnected("scoped ClientHook driver dropped".into())
            })));
        }
        let result = Pin::new(
            self.receiver
                .as_mut()
                .expect("reply polled after completion"),
        )
        .poll(cx);
        if result.is_ready() {
            // Completion already notified its tracked observer. A later drop
            // cannot cancel this call or enable another coordinator scan.
            drop(self.receiver.take());
        }
        result.map(|result| {
            result
                .map_err(crate::canceled_to_error)
                .and_then(|result| result)
        })
    }
}
impl Drop for Reply {
    fn drop(&mut self) {
        // First close the receiver, then wake the dispatcher to prune queued
        // cancellation even when no IO or application operation is ready.
        if let Some(receiver) = self.receiver.take() {
            drop(receiver);
            if let Some(inbox) = self.inbox.upgrade() {
                inbox.notify();
            }
        }
    }
}
// Exact private hook types carry authority; public brands and wrappers do not.
pub(crate) fn tail_target(client: &dyn ClientHook) -> Option<Box<dyn std::any::Any>> {
    let hook = client.as_any()?.downcast_ref::<Hook>()?;
    // A settled remote endpoint cannot resolve into a different connection
    // after its authenticated destination metadata has been committed.
    if !hook.0.settled.get() || !hook.0.remote.get() || hook.0.introduction.borrow().is_some() {
        return None;
    }
    let handle = hook.0.inbox.upgrade()?.joins.clone()?;
    handle.tail_target()
}
pub(crate) fn call_third_party(
    client: &dyn ClientHook,
    interface: u64,
    method: u16,
    params: Box<dyn ParamsHook>,
    results: Box<dyn ResultsHook>,
    hints: CallHints,
    contact: Rc<capnp::message::Builder<capnp::message::HeapAllocator>>,
) -> Promise<(), Error> {
    let Some(hook) = client.as_any().and_then(|any| any.downcast_ref::<Hook>()) else {
        return Promise::err(Error::failed("foreign tail transfer hook".into()));
    };
    hook.call_to(interface, method, params, results, hints, Some(contact))
}
