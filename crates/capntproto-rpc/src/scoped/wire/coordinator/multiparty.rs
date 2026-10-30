//! Hosted network callbacks over the portable ledgers. Transport identities and
//! opaque tokens are authenticated by VatNetwork, never by protocol question IDs.
use super::transport::vat;
use super::*;
mod bilateral;
mod handoff;
pub(crate) mod introduction;
pub(crate) mod tail;
use crate::multiparty::{JoinNetwork, PartResponse};
use capnp::private::capability::ResponseHook;
use capntproto_rpc_engine::{
    clients::{Capabilities, Destination},
    questions,
};
use capntproto_rpc_wire::{
    clients::{Endpoint, Route},
    owned::TargetRef,
};

type Body = capnp::message::Builder<capnp::message::HeapAllocator>;
type Sender = shared::Sender<capnp::Result<Box<dyn ResponseHook>>>;
enum Operation {
    Join {
        target: Box<dyn ClientHook>,
        part: Body,
    },
    Accept(Body, Option<Vec<u8>>),
}
struct Request {
    operation: Operation,
    sender: Sender,
    permit: capntproto_rpc_engine::flow_control::outgoing::OwnedPermit,
}
pub(crate) struct Commands {
    pub(crate) bilateral: Cell<bool>,
    join_ids: RefCell<bilateral::Ids>,
    pub(crate) network: RefCell<Option<Rc<dyn introduction::Network>>>,
    provisions: RefCell<VecDeque<Rc<introduction::Provision>>>,
    forwarding: RefCell<VecDeque<Rc<introduction::Provision>>>,
    markers: RefCell<VecDeque<introduction::Marker>>,
    queue: RefCell<VecDeque<Request>>,
    adoptions: RefCell<VecDeque<tail::Adoption>>,
    wake: RefCell<Option<Waker>>,
    closed: RefCell<Option<Error>>,
    capacity: Rc<capntproto_rpc_engine::flow_control::outgoing::Outgoing>,
    bytes: usize,
}
impl Commands {
    pub(crate) fn new(maximum: usize, bytes: usize) -> Rc<Self> {
        Rc::new(Self {
            bilateral: Cell::new(false),
            join_ids: RefCell::default(),
            network: RefCell::default(),
            provisions: RefCell::default(),
            forwarding: RefCell::default(),
            markers: RefCell::default(),
            queue: RefCell::default(),
            adoptions: RefCell::default(),
            wake: RefCell::default(),
            closed: RefCell::default(),
            capacity: Rc::new(
                capntproto_rpc_engine::flow_control::outgoing::Outgoing::new(maximum),
            ),
            bytes,
        })
    }
    fn notify(&self) {
        let wake = self.wake.borrow_mut().take();
        if let Some(wake) = wake {
            wake.wake();
        }
    }
    pub(crate) fn close(&self, error: Error) {
        if self.closed.borrow().is_some() {
            return;
        }
        *self.closed.borrow_mut() = Some(error.clone());
        let queue = std::mem::take(&mut *self.queue.borrow_mut());
        let adoptions = std::mem::take(&mut *self.adoptions.borrow_mut());
        drop(adoptions);
        let provisions = std::mem::take(&mut *self.provisions.borrow_mut());
        drop(provisions);
        let forwarding = std::mem::take(&mut *self.forwarding.borrow_mut());
        drop(forwarding);
        let markers = std::mem::take(&mut *self.markers.borrow_mut());
        drop(markers);
        for request in queue {
            let _ = request.sender.send(Err(error.clone()));
        }
        self.notify();
    }
    pub(crate) fn handle(self: &Rc<Self>) -> Handle {
        Handle(Rc::downgrade(self))
    }
}
impl Drop for Commands {
    fn drop(&mut self) {
        self.close(closed());
    }
}
#[derive(Clone)]
pub(crate) struct Handle(Weak<Commands>);
struct Notify(Weak<Commands>);
impl Drop for Notify {
    fn drop(&mut self) {
        if let Some(commands) = self.0.upgrade() {
            commands.notify();
        }
    }
}
struct Lease {
    _permit: capntproto_rpc_engine::flow_control::outgoing::OwnedPermit,
    _notify: Notify,
}
struct Reply {
    payload: Rc<Payload>,
    _lease: Rc<Lease>,
}
impl ResponseHook for Reply {
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        self.payload.reader()
    }
}
fn closed() -> Error {
    Error::disconnected("portable network control ended".into())
}
impl Handle {
    pub(crate) fn unavailable() -> Self {
        Self(Weak::new())
    }
    pub(crate) fn close(&self, error: Error) {
        if let Some(commands) = self.0.upgrade() {
            commands.close(error);
        }
    }
    pub(crate) fn join(
        &self,
        target: Box<dyn ClientHook>,
        part: any_pointer::Reader<'_>,
    ) -> PartResponse {
        self.request(part, |part| Operation::Join { target, part })
    }
    pub(crate) fn accept(&self, completion: any_pointer::Reader<'_>) -> PartResponse {
        self.request(completion, |body| Operation::Accept(body, None))
    }
    pub(crate) fn accept_embargo(
        &self,
        completion: any_pointer::Reader<'_>,
        embargo: Vec<u8>,
    ) -> PartResponse {
        self.request(completion, |body| Operation::Accept(body, Some(embargo)))
    }
    fn request(
        &self,
        body: any_pointer::Reader<'_>,
        operation: impl FnOnce(Body) -> Operation,
    ) -> PartResponse {
        let prepare = || {
            let commands = self.0.upgrade().ok_or_else(closed)?;
            if let Some(error) = commands.closed.borrow().clone() {
                return Err(error);
            }
            let permit = commands
                .capacity
                .reserve_owned()
                .map_err(|_| Error::overloaded("network command capacity exhausted".into()))?;
            body.validate_capability_indices(0)?;
            if body.target_size()?.word_count > commands.bytes.saturating_sub(128) as u64 / 8 {
                return Err(Error::overloaded(
                    "network metadata exceeds frame budget".into(),
                ));
            }
            let mut owned = Body::new_default();
            owned.set_root(body)?;
            let (sender, receiver) = shared::channel();
            {
                let mut queue = commands.queue.borrow_mut();
                queue.try_reserve(1).map_err(wire_error)?;
                queue.push_back(Request {
                    operation: operation(owned),
                    sender,
                    permit,
                });
            }
            commands.notify();
            Ok(receiver)
        };
        match prepare() {
            Err(error) => Promise::err(error),
            Ok(receiver) => {
                let notify = Notify(self.0.clone());
                Promise::from_future(async move {
                    let _notify = notify;
                    receiver.await.map_err(|_| closed())?
                })
            }
        }
    }
}

enum Completion {
    Capability(Box<dyn ClientHook>),
    Join(Box<dyn ResponseHook>),
    Bilateral(Box<dyn ResponseHook>),
}
struct Incoming<'p, 'id, const H: usize> {
    request: connection::NetworkRequest<'p, 'id, H>,
    future: Option<Promise<Completion, Error>>,
    completed: Option<capnp::Result<Completion>>,
}
struct Sent<'p, 'id, const C: usize> {
    key: questions::Key<'id>,
    sender: Option<Sender>,
    permit: Option<capntproto_rpc_engine::flow_control::outgoing::OwnedPermit>,
    response: Option<connection::Response<'p, 'id, resources::Import, C>>,
    observer: Weak<Lease>,
    pipeline: Option<Rc<PipelineState>>,
}
struct Local {
    sender: Sender,
    future: PartResponse,
    permit: capntproto_rpc_engine::flow_control::outgoing::OwnedPermit,
}
struct LocalReply {
    inner: Box<dyn ResponseHook>,
    _lease: Lease,
}
impl ResponseHook for LocalReply {
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        self.inner.get()
    }
}
pub(crate) struct NetworkControl<'p, 'id, V: 'static, F, const C: usize, const H: usize> {
    joiner: crate::portable::Joiner<V>,
    bilateral: bilateral::Groups,
    bootstrap: TwoParty<'id, F>,
    connection: vat::Shared<V>,
    joins: Option<Rc<dyn JoinNetwork<V>>>,
    commands: Rc<Commands>,
    pending: VecDeque<Incoming<'p, 'id, H>>,
    sent: Vec<Sent<'p, 'id, C>>,
    local: Vec<Local>,
    handoffs: Vec<handoff::Job<'p, 'id>>,
    provided: Vec<introduction::Provided<'id>>,
    barriers: VecDeque<connection::VineBarrier<'p, 'id>>,
    produced: VecDeque<tail::Produced<'id>>,
    maximum: usize,
    lane: usize,
    tail_lane: usize,
}
impl<
        'p,
        'id,
        V: 'static,
        F: FnMut() -> capnp::Result<capability::Client>,
        const C: usize,
        const H: usize,
    > NetworkControl<'p, 'id, V, F, C, H>
{
    pub(crate) fn new(
        bootstrap: TwoParty<'id, F>,
        connection: vat::Shared<V>,
        joins: Option<Rc<dyn JoinNetwork<V>>>,
        commands: Rc<Commands>,
        maximum: usize,
        joiner: crate::portable::Joiner<V>,
    ) -> Self {
        Self {
            joiner,
            bilateral: bilateral::Groups::default(),
            bootstrap,
            connection,
            joins,
            commands,
            pending: VecDeque::new(),
            sent: Vec::new(),
            local: Vec::new(),
            handoffs: Vec::new(),
            provided: Vec::new(),
            barriers: VecDeque::new(),
            produced: VecDeque::new(),
            maximum,
            lane: 0,
            tail_lane: 0,
        }
    }
    fn target<'s>(
        coordinator: &mut Coordinator<'p, 'id, 's, resources::Import, (), C, H>,
        target: TargetRef<'_, 'id, 'id>,
    ) -> capnp::Result<Box<dyn ClientHook>> {
        if let TargetRef::Promised { answer, path } = &target {
            // The hosted early pipeline is authoritative for this admitted
            // answer generation, even while the final Return is still pending.
            let producer = coordinator
                .driver
                .ledgers()
                .answers
                .pipelined(answer)
                .owner
                .producer
                .clone();
            if let Some(hook) = producer.pipeline(*path) {
                return Ok(hook);
            }
        }
        let driver = &mut coordinator.driver;
        let client = match target {
            TargetRef::Hosted(lease) => driver.reflected(lease).map_err(wire_error)?,
            TargetRef::Promised { answer, path } => {
                driver.project_answer(answer, path).map_err(wire_error)?
            }
        };
        let result = match driver.runtime().inspect(&client) {
            Ok(Destination::Endpoint {
                owner:
                    Route::Capability(Endpoint::Local(hook) | Endpoint::Failed { owner: hook, .. })
                    | Route::Vine(hook),
                ..
            }) => Ok(hook.add_ref()),
            _ => coordinator
                .hooks
                .export_client(&mut driver.capabilities(), &client)
                .map(|c| c.hook),
        };
        driver.capabilities().release(client);
        result
    }
    fn receive_network<'s>(
        &mut self,
        request: connection::NetworkRequest<'p, 'id, H>,
        coordinator: &mut Coordinator<'p, 'id, 's, resources::Import, (), C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
    ) -> capnp::Result<()> {
        use connection::NetworkRequest as N;
        if matches!(request, N::Await(_) | N::Adopt(_)) {
            return self.receive_handoff(request, coordinator, messages);
        }
        // Reserve before callbacks, keeping admission and ownership failure-atomic.
        if matches!(request, N::Accept(_) | N::Join(_))
            && (self.pending.len() >= self.maximum || self.pending.try_reserve(1).is_err())
        {
            coordinator.driver.release_network(request, messages);
            return Err(Error::overloaded(
                "network operation capacity exhausted".into(),
            ));
        }
        match request {
            N::Unsupported(frame) => {
                coordinator
                    .driver
                    .release_network(N::Unsupported(frame), messages);
                return Err(Error::failed(
                    "unsupported message bypassed echo policy".into(),
                ));
            }
            N::Provide(provide) => {
                let target = match Self::target(coordinator, provide.target()) {
                    Ok(target) => target,
                    Err(error) => {
                        coordinator
                            .driver
                            .release_network(N::Provide(provide), messages);
                        return Err(error);
                    }
                };
                let exchange = crate::third_party::ThirdPartyExchange::new(target);
                let result = coordinator.driver.register_provide(
                    provide,
                    messages,
                    |metadata, _, producer| {
                        let guard = self
                            .connection
                            .borrow_mut()
                            .await_third_party(metadata, exchange.clone())?;
                        producer.retain_network(Some(exchange), guard);
                        Ok::<_, Error>(())
                    },
                );
                if let Err(rejected) = result {
                    coordinator
                        .driver
                        .release_network(N::Provide(rejected.provide), messages);
                    return Err(Error::failed(format!(
                        "provision registration: {:?}",
                        rejected.error
                    )));
                }
            }
            N::Disembargo(barrier) => {
                if let Err(rejected) =
                    coordinator
                        .driver
                        .release_accept_embargo(barrier, messages, |id, producer| {
                            producer.release_embargo(id)
                        })
                {
                    coordinator
                        .driver
                        .release_network(N::Disembargo(rejected.barrier), messages);
                    return Err(Error::failed(format!(
                        "provision embargo: {:?}",
                        rejected.error
                    )));
                }
            }
            N::Accept(accept) => {
                let prepared = accept.with(messages, |request| {
                    let future = self
                        .connection
                        .borrow_mut()
                        .complete_third_party(request.completion);
                    let embargo = request.embargo.map(<[u8]>::to_vec);
                    Promise::from_future(async move {
                        let exchange = future.await?;
                        let acceptance = exchange.accept(embargo);
                        // The Provide owns the rendezvous. Holding it across
                        // this await would prevent Finish from canceling the
                        // pending embargo and retain its authority indefinitely.
                        drop(exchange);
                        Ok(Completion::Capability(acceptance.await?))
                    })
                });
                match prepared {
                    Ok(future) => self.pending.push_back(Incoming {
                        request: N::Accept(accept),
                        future: Some(future),
                        completed: None,
                    }),
                    Err(error) => {
                        coordinator
                            .driver
                            .release_network(N::Accept(accept), messages);
                        return Err(wire_error(error));
                    }
                }
            }
            N::Join(join) => {
                if self.commands.bilateral.get() {
                    let joiner = self.joiner.clone();
                    let origin = Rc::as_ptr(&coordinator.hooks.inbox) as usize;
                    let prepared = Self::target(coordinator, join.target()).and_then(|target| {
                        join.with_key_part(messages, |part| {
                            self.bilateral
                                .receive(part, target, self.maximum, move |caps| {
                                    joiner.resolve_from(caps, origin)
                                })
                        })
                        .map_err(wire_error)?
                    });
                    let (guard, future) = match prepared {
                        Ok(prepared) => prepared,
                        Err(error) => {
                            coordinator.driver.release_network(N::Join(join), messages);
                            return Err(error);
                        }
                    };
                    coordinator
                        .driver
                        .ledgers()
                        .answers
                        .get(join.key())
                        .expect("admitted Join generation")
                        .owner
                        .producer
                        .retain_network(None, guard);
                    self.pending.push_back(Incoming {
                        request: N::Join(join),
                        future: Some(future),
                        completed: None,
                    });
                    return Ok(());
                }
                let prepared = (|| {
                    let network = self
                        .joins
                        .clone()
                        .filter(|_| self.connection.borrow().supports_multiparty_join())
                        .ok_or_else(|| {
                            Error::unimplemented(
                                "authenticated Join unavailable on this route".into(),
                            )
                        })?;
                    let part = join
                        .with_key_part(messages, |part| network.forward(part))
                        .map_err(wire_error)??;
                    let target = Self::target(coordinator, join.target())?;
                    Ok(Promise::from_future(async move {
                        // A private native promised-answer route can carry a
                        // Join share before Return. Waiting for its response
                        // would deadlock an ordering fence around that answer.
                        let target = match crate::scoped::wire::native_join_target(&*target) {
                            Some(native) => native,
                            None => crate::rpc::join::settle(target).await?,
                        };
                        crate::rpc::multiparty_join::part_at(
                            network,
                            target,
                            part.get_root_as_reader()?,
                        )
                        .await
                        .map(Completion::Join)
                    }))
                })();
                let (future, completed) = match prepared {
                    Ok(future) => (Some(future), None),
                    Err(error) => (None, Some(Err(error))),
                };
                self.pending.push_back(Incoming {
                    request: N::Join(join),
                    future,
                    completed,
                });
            }
            N::ForwardEmbargo(barrier) => {
                if self.barriers.len() >= self.maximum || self.barriers.try_reserve(1).is_err() {
                    coordinator
                        .driver
                        .release_network(N::ForwardEmbargo(barrier), messages);
                    return Err(Error::overloaded("vine barrier capacity exhausted".into()));
                }
                self.barriers.push_back(barrier);
            }
            N::Await(_) | N::Adopt(_) => unreachable!("handled before capability operations"),
        }
        Ok(())
    }
}

enum Target {
    Imported(capntproto_rpc_engine::ids::ImportId),
    Promised(capntproto_rpc_engine::ids::QuestionId, Vec<u16>),
}
impl Target {
    fn borrowed(&self) -> capntproto_rpc_wire::calls::Target<'_> {
        match self {
            Self::Imported(id) => capntproto_rpc_wire::calls::Target::Imported(*id),
            Self::Promised(question, path) => capntproto_rpc_wire::calls::Target::Promised {
                question: *question,
                path: pipeline::Path::new(path),
            },
        }
    }
}
fn minimum(body: any_pointer::Reader<'_>) -> capnp::Result<usize> {
    usize::try_from(body.target_size()?.word_count)
        .ok()
        .and_then(|n| n.checked_mul(8))
        .and_then(|n| n.checked_add(128))
        .ok_or_else(|| Error::from_kind(capnp::ErrorKind::MessageSizeOverflow))
}
impl<
        'p,
        'id,
        V: 'static,
        F: FnMut() -> capnp::Result<capability::Client>,
        const C: usize,
        const H: usize,
    > NetworkControl<'p, 'id, V, F, C, H>
{
    #[inline(never)]
    fn poll_active<'s>(
        &mut self,
        coordinator: &mut Coordinator<'p, 'id, 's, resources::Import, (), C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, vat::Bundle>,
        scratch: &mut [u32],
        output: &vat::Resources,
        cx: &mut Context<'_>,
    ) -> Poll<capnp::Result<bool>> {
        let available = match queue.ensure_capacity() {
            Ok(()) => true,
            Err(messages::Error::NoSendSlot) => false,
            Err(error) => return Poll::Ready(Err(wire_error(error))),
        };
        if !coordinator.worker.has_undispatched() {
            if let Some(barrier) = self.barriers.pop_front() {
                let mut scratch = vec![0; self.commands.bytes];
                if let Err(rejected) = coordinator.driver.forward_vine_embargo(
                    barrier,
                    messages,
                    &mut scratch,
                    |id, owner, _| introduction::forward(owner, id),
                ) {
                    coordinator.driver.release_network(
                        connection::NetworkRequest::ForwardEmbargo(rejected.barrier),
                        messages,
                    );
                    return Poll::Ready(Err(wire_error(format!(
                        "vine embargo: {:?}",
                        rejected.error
                    ))));
                }
                return Poll::Ready(Ok(true));
            }
        }
        if available
            && coordinator.hooks.inbox.queue.borrow().is_empty()
            && !coordinator.calls.has_unpublished()
            && !coordinator.worker.has_undispatched()
            && self.poll_provisions(coordinator, messages, queue)?
        {
            return Poll::Ready(Ok(true));
        }
        // Resume after the last productive lane. Continuous incoming Join
        // shares or tail transfers cannot starve completion, cancellation,
        // output, or bootstrap.
        for _ in 0..7 {
            let lane = self.lane;
            self.lane = (lane + 1) % 7;
            match lane {
                0 => {
                    if let Some(request) = coordinator.driver.take_network() {
                        return Poll::Ready(
                            self.receive_network(request, coordinator, messages)
                                .map(|()| true),
                        );
                    }
                }
                1 => {
                    for index in (0..self.local.len()).rev() {
                        if self.local[index].sender.is_closed() {
                            self.local.swap_remove(index);
                            return Poll::Ready(Ok(true));
                        }
                        if let Poll::Ready(result) =
                            Pin::new(&mut self.local[index].future).poll(cx)
                        {
                            let local = self.local.swap_remove(index);
                            let result = result.map(|inner| {
                                Box::new(LocalReply {
                                    inner,
                                    _lease: Lease {
                                        _permit: local.permit,
                                        _notify: Notify(Rc::downgrade(&self.commands)),
                                    },
                                }) as Box<dyn ResponseHook>
                            });
                            let _ = local.sender.send(result);
                            return Poll::Ready(Ok(true));
                        }
                    }
                }
                2 => {
                    for index in (0..self.sent.len()).rev() {
                        let sent = &mut self.sent[index];
                        if let Some(pipeline) = &sent.pipeline {
                            if handoff::project(sent.key, pipeline, coordinator)? {
                                return Poll::Ready(Ok(true));
                            }
                        }
                        if let Some(response) = sent.response.as_ref() {
                            let _ = response;
                            if sent.observer.upgrade().is_none() {
                                let sent = self.sent.swap_remove(index);
                                coordinator.driver.release_output(
                                    CallOutput::Remote(sent.response.unwrap()),
                                    messages,
                                );
                                return Poll::Ready(Ok(true));
                            }
                        } else if sent.sender.as_ref().is_some_and(Sender::is_closed)
                            && sent
                                .pipeline
                                .as_ref()
                                .is_none_or(|pipeline| Rc::strong_count(pipeline) == 1)
                        {
                            let sent = self.sent.swap_remove(index);
                            return Poll::Ready(
                                coordinator
                                    .driver
                                    .cancel(sent.key)
                                    .map(|()| true)
                                    .map_err(wire_error),
                            );
                        } else {
                            match coordinator.driver.take_response(sent.key) {
                                Err(connection::Error::PendingResponse) => (),
                                Err(error) => return Poll::Ready(Err(wire_error(error))),
                                Ok(response) => {
                                    let result =
                                        Self::retain_response(&response, coordinator, messages)
                                            .map(Rc::new);
                                    if let Some(pipeline) = sent.pipeline.take() {
                                        match &result {
                                            Ok(payload) => pipeline.finish_payload(payload.clone()),
                                            Err(error) => pipeline.finish(Box::new(
                                                crate::broken::Pipeline::new(error.clone()),
                                            )),
                                        }
                                    }
                                    let lease = Rc::new(Lease {
                                        _permit: sent.permit.take().unwrap(),
                                        _notify: Notify(Rc::downgrade(&self.commands)),
                                    });
                                    sent.observer = Rc::downgrade(&lease);
                                    sent.response = Some(response);
                                    let _ =
                                        sent.sender.take().unwrap().send(result.map(|payload| {
                                            Box::new(Reply {
                                                payload,
                                                _lease: lease,
                                            })
                                                as Box<dyn ResponseHook>
                                        }));
                                    return Poll::Ready(Ok(true));
                                }
                            }
                        }
                    }
                }
                3 => {
                    for _ in 0..self.pending.len() {
                        let mut job = self.pending.pop_front().unwrap();
                        let key = job.request.answer_key().unwrap();
                        let canceled = coordinator
                            .driver
                            .ledgers()
                            .answers
                            .cancellation_requested(key)
                            .unwrap_or(true);
                        if canceled && job.future.is_some() {
                            job.future = None;
                            job.completed = Some(Err(closed()));
                        }
                        if let Some(future) = &mut job.future {
                            if let Poll::Ready(result) = Pin::new(future).poll(cx) {
                                job.future = None;
                                job.completed = Some(result);
                            }
                        }
                        if available && job.completed.is_some() {
                            return Poll::Ready(
                                self.publish(job, coordinator, messages, queue, scratch, output)
                                    .map(|()| true),
                            );
                        }
                        self.pending.push_back(job);
                    }
                }
                4 => {
                    if available {
                        let request = self.commands.queue.borrow_mut().pop_front();
                        if let Some(request) = request {
                            if self.sent.try_reserve(1).is_err()
                                || self.local.try_reserve(1).is_err()
                            {
                                let _ = request.sender.send(Err(Error::overloaded(
                                    "network command allocation failed".into(),
                                )));
                                return Poll::Ready(Ok(true));
                            }
                            return Poll::Ready(
                                self.send(request, coordinator, messages, queue)
                                    .map(|()| true),
                            );
                        }
                    }
                }
                5 => {
                    if let Poll::Ready(result) = self.bootstrap.poll_bootstrap(
                        &mut ControlContext::new(coordinator),
                        messages,
                        queue,
                        scratch,
                        output,
                    ) {
                        return Poll::Ready(result);
                    }
                }
                _ => {
                    if available {
                        if let Poll::Ready(result) =
                            self.poll_transfers(coordinator, messages, queue, scratch, output, cx)
                        {
                            return Poll::Ready(result);
                        }
                    }
                }
            }
        }
        Poll::Pending
    }

    fn send<'s>(
        &mut self,
        request: Request,
        coordinator: &mut Coordinator<'p, 'id, 's, resources::Import, (), C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, vat::Bundle>,
    ) -> capnp::Result<()> {
        if request.sender.is_closed() {
            return Ok(());
        }
        let driver = &mut coordinator.driver;
        let result = (|| match &request.operation {
            Operation::Join { target, part } => {
                let client = coordinator
                    .hooks
                    .try_retain(&mut driver.capabilities(), &**target)?
                    .ok_or_else(|| Error::failed("foreign Join command target".into()))?;
                let route = (|| {
                    let Destination::Endpoint { owner, .. } =
                        driver.runtime().inspect(&client).map_err(wire_error)?
                    else {
                        return Err(Error::failed("Join target is not settled".into()));
                    };
                    if let Route::Capability(
                        Endpoint::Local(hook) | Endpoint::Failed { owner: hook, .. },
                    ) = owner
                    {
                        return Ok(Err(hook.add_ref()));
                    }
                    match owner
                        .target(&driver.ledgers().questions)
                        .map_err(wire_error)?
                    {
                        Some(capntproto_rpc_wire::calls::Target::Imported(id)) => {
                            Ok(Ok(Target::Imported(id)))
                        }
                        Some(capntproto_rpc_wire::calls::Target::Promised { question, path }) => {
                            Ok(Ok(Target::Promised(question, path.fields().to_vec())))
                        }
                        None => Err(Error::unimplemented(
                            "Join target cannot be forwarded".into(),
                        )),
                    }
                })();
                driver.capabilities().release(client);
                match route? {
                    Err(hook) => {
                        let network = self.joins.clone().ok_or_else(|| {
                            Error::unimplemented("Join network unavailable".into())
                        })?;
                        Ok(Err(crate::rpc::multiparty_join::part_at(
                            network,
                            hook,
                            part.get_root_as_reader()?,
                        )))
                    }
                    Ok(target) => {
                        if !self.commands.bilateral.get()
                            && !self.connection.borrow().supports_multiparty_join()
                        {
                            return Err(Error::unimplemented(
                                "Join unavailable on this route".into(),
                            ));
                        }
                        let part = part.get_root_as_reader::<any_pointer::Reader>()?;
                        let bytes = minimum(part)?
                            .checked_add(match &target {
                                Target::Promised(_, path) => {
                                    path.len().checked_mul(16).ok_or_else(|| {
                                        Error::from_kind(capnp::ErrorKind::MessageSizeOverflow)
                                    })?
                                }
                                _ => 0,
                            })
                            .ok_or_else(|| {
                                Error::from_kind(capnp::ErrorKind::MessageSizeOverflow)
                            })?;
                        driver
                            .send(
                                messages,
                                queue,
                                bytes,
                                coordinator.options.reader,
                                |builder, id| {
                                    capntproto_rpc_wire::network::join(
                                        builder,
                                        id,
                                        target.borrowed(),
                                        |field| field.copy_from(part),
                                    )
                                },
                            )
                            .map(Ok)
                            .map_err(wire_error)
                    }
                }
            }
            Operation::Accept(completion, embargo) => {
                if !self.connection.borrow().supports_third_party() {
                    return Err(Error::unimplemented(
                        "Accept unavailable on this route".into(),
                    ));
                }
                let completion = completion.get_root_as_reader::<any_pointer::Reader>()?;
                driver
                    .send(
                        messages,
                        queue,
                        minimum(completion)?.saturating_add(
                            embargo.as_ref().map_or(0, |b| b.len().saturating_add(8)),
                        ),
                        coordinator.options.reader,
                        |builder, id| {
                            capntproto_rpc_wire::network::accept(
                                builder,
                                id,
                                embargo.as_deref(),
                                |field| field.copy_from(completion),
                            )
                        },
                    )
                    .map(Ok)
                    .map_err(wire_error)
            }
        })();
        match result {
            Ok(Ok(key)) => self.sent.push(Sent {
                key,
                sender: Some(request.sender),
                permit: Some(request.permit),
                response: None,
                observer: Weak::new(),
                pipeline: None,
            }),
            Ok(Err(future)) => self.local.push(Local {
                sender: request.sender,
                future,
                permit: request.permit,
            }),
            Err(error) => {
                let _ = request.sender.send(Err(error));
            }
        }
        Ok(())
    }
    fn retain_response<'s>(
        response: &connection::Response<'p, 'id, resources::Import, C>,
        coordinator: &mut Coordinator<'p, 'id, 's, resources::Import, (), C, H>,
        messages: &messages::Messages<'p, '_, '_>,
    ) -> capnp::Result<Payload> {
        let driver = &mut coordinator.driver;
        response
            .with(messages, |reply| match reply {
                owned::Reply::Results(payload) => {
                    let mut caps = Vec::new();
                    caps.try_reserve_exact(payload.capability_count())
                        .map_err(wire_error)?;
                    for index in 0..payload.capability_count() {
                        let cap = match payload.capability_at(index as u32).map_err(wire_error)? {
                            None => None,
                            Some(cap) => {
                                let client = driver
                                    .bind_capability(cap)
                                    .map_err(wire_error)?
                                    .into_client();
                                let result = coordinator
                                    .hooks
                                    .export_client(&mut driver.capabilities(), &client);
                                driver.capabilities().release(client);
                                Some(result?.hook)
                            }
                        };
                        caps.push(cap);
                    }
                    let content = payload.content();
                    Payload::retain_or_copy(
                        content,
                        response
                            .retain_results(messages, content)
                            .map_err(wire_error)?,
                        caps,
                    )
                }
                owned::Reply::Exception(error) => Err(crate::rpc::remote_exception_to_error(error)),
                _ => Err(Error::failed("invalid network command response".into())),
            })
            .map_err(wire_error)?
    }
    fn publish<'s>(
        &mut self,
        job: Incoming<'p, 'id, H>,
        coordinator: &mut Coordinator<'p, 'id, 's, resources::Import, (), C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, vat::Bundle>,
        scratch: &mut [u32],
        output: &vat::Resources,
    ) -> capnp::Result<()> {
        let key = job.request.answer_key().expect("answer control");
        let driver = &mut coordinator.driver;
        let producer = match driver.ledgers().answers.get(key) {
            Ok(state) => state.owner.producer.clone(),
            Err(error) => {
                driver.release_network(job.request, messages);
                return Err(wire_error(error));
            }
        };
        let result = job.completed.expect("completed network operation");
        match job.request {
            connection::NetworkRequest::Join(join) => {
                let outcome = match result {
                    Ok(Completion::Bilateral(response)) => {
                        let prepared = (|| {
                            let payload = Payload::copy(response.get()?)?;
                            let mut retained = Vec::new();
                            retained
                                .try_reserve_exact(payload.caps.len())
                                .map_err(wire_error)?;
                            let prepared = (|| {
                                for cap in &payload.caps {
                                    let client = cap
                                        .as_ref()
                                        .map(|hook| {
                                            match coordinator
                                                .hooks
                                                .try_retain(&mut driver.capabilities(), &**hook)?
                                            {
                                                Some(client) => Ok(client),
                                                None => coordinator
                                                    .foreign
                                                    .import(&mut driver.capabilities(), &**hook),
                                            }
                                        })
                                        .transpose()?;
                                    retained.push(client);
                                }
                                let content = payload.reader()?;
                                let size = minimum(content)?;
                                driver
                                    .local_result_dynamic(
                                        0,
                                        0,
                                        retained.iter().map(Option::as_ref),
                                        messages,
                                        size,
                                        coordinator.options.reader,
                                        |field| {
                                            field.copy_with_capabilities(
                                                content,
                                                capnp::wire_capability::IndexMap::identity(
                                                    payload.caps.len() as u32,
                                                ),
                                            )
                                        },
                                    )
                                    .map_err(wire_error)
                            })();
                            for client in retained.into_iter().flatten() {
                                driver.capabilities().release(client);
                            }
                            prepared
                        })();
                        let prepared = match prepared {
                            Ok(result) => result,
                            Err(error) => {
                                driver.release_network(
                                    connection::NetworkRequest::Join(join),
                                    messages,
                                );
                                return Err(error);
                            }
                        };
                        if let Err(rejected) =
                            driver.complete_join_results(&join, prepared, messages)
                        {
                            if let Ok(result) = rejected.result {
                                result.release(messages, &mut driver.capabilities());
                            }
                            driver
                                .release_network(connection::NetworkRequest::Join(join), messages);
                            return Err(wire_error(rejected.error));
                        }
                        // The original PartGuard remains in the producer until
                        // Finish and retains the complete batch outcome.
                        driver.send_join_results(
                            join,
                            messages,
                            queue,
                            coordinator.options.output_bytes.get(),
                            coordinator.options.reader,
                            scratch,
                        )
                    }
                    Ok(Completion::Join(response)) => {
                        let size = match response.get().and_then(minimum) {
                            Ok(size) => size,
                            Err(error) => {
                                driver.release_network(
                                    connection::NetworkRequest::Join(join),
                                    messages,
                                );
                                return Err(error);
                            }
                        };
                        let outcome = driver.complete_join(
                            join,
                            messages,
                            queue,
                            size,
                            coordinator.options.reader,
                            |field| field.copy_from(response.get()?),
                        );
                        producer.retain_network(None, Box::new(response));
                        outcome
                    }
                    Err(error) => driver.reject_join(
                        join,
                        error.kind,
                        messages,
                        queue,
                        coordinator.options.output_bytes.get(),
                        coordinator.options.reader,
                    ),
                    _ => unreachable!("Join completion"),
                };
                if let Err(rejected) = outcome {
                    driver
                        .release_network(connection::NetworkRequest::Join(rejected.join), messages);
                    return Err(wire_error(rejected.error));
                }
            }
            connection::NetworkRequest::Accept(accept) => {
                let completion: capnp::Result<()> = (|| {
                    let result = match result {
                        Ok(Completion::Capability(hook)) => {
                            let client = match coordinator
                                .hooks
                                .try_retain(&mut driver.capabilities(), &*hook)?
                            {
                                Some(client) => client,
                                None => coordinator
                                    .foreign
                                    .import(&mut driver.capabilities(), &*hook)?,
                            };
                            let result = driver.local_result_dynamic(
                                0,
                                0,
                                [Some(&client)].into_iter(),
                                messages,
                                128,
                                coordinator.options.reader,
                                |content| content.set_capability_index(0),
                            );
                            driver.capabilities().release(client);
                            Ok(result.map_err(wire_error)?)
                        }
                        Err(error) => {
                            let kind = error.kind;
                            producer.fail(error);
                            Err(kind)
                        }
                        _ => unreachable!("Accept completion"),
                    };
                    driver
                        .complete_authenticated_accept(&accept, result, messages)
                        .map_err(|rejected| {
                            if let Ok(result) = rejected.result {
                                result.release(messages, &mut driver.capabilities());
                            }
                            wire_error(rejected.error)
                        })?;
                    producer.prepare_trace(&coordinator.policy);
                    let bytes = if producer.failure().is_some() {
                        producer.publication_bytes()?
                    } else {
                        128
                    };
                    driver
                        .send_answer_using(
                            key,
                            messages,
                            queue,
                            bytes,
                            coordinator.options.reader,
                            scratch,
                            output.bundle(),
                            |owner, imports, bundle| output.attach(owner, imports, bundle),
                            |results, builder| results.write_exception(builder),
                        )
                        .map_err(wire_error)?;
                    Ok(())
                })();
                accept.release(messages);
                completion?;
            }
            _ => unreachable!("pending network operation"),
        }
        Ok(())
    }
}

impl<
        'p,
        'id,
        's,
        V: 'static,
        F: FnMut() -> capnp::Result<capability::Client>,
        const C: usize,
        const H: usize,
    > NetworkControl<'p, 'id, V, F, C, H>
{
    fn poll_network(
        &mut self,
        coordinator: &mut Coordinator<'p, 'id, 's, resources::Import, (), C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, vat::Bundle>,
        scratch: &mut [u32],
        output: &vat::Resources,
        cx: &mut Context<'_>,
    ) -> Poll<capnp::Result<bool>> {
        let unsupported = coordinator.driver.peek_network().is_some_and(|request| {
            let connection = self.connection.borrow();
            match request {
                connection::NetworkRequest::Unsupported(_) => true,
                connection::NetworkRequest::Join(_) => {
                    !connection.supports_two_party_join() && !connection.supports_multiparty_join()
                }
                _ => connection.control_mode() == crate::ControlMode::TwoParty,
            }
        });
        if unsupported {
            return ControlContext::new(coordinator)
                .reject_network(messages, queue, scratch, output);
        }
        coordinator
            .driver
            .allow_answer_handoff(self.connection.borrow().supports_third_party_answers());
        if self.prepare_transfer(coordinator, messages)? {
            // A promised target can become ready after the worker's last
            // scan. Preparing its transfer unblocks dispatch even while the
            // service result and network rendezvous are still pending.
            return Poll::Ready(Ok(true));
        }
        if let Poll::Ready(result) = self.poll_handoffs(coordinator, messages, cx) {
            return Poll::Ready(result.map(|()| true));
        }
        if self
            .commands
            .wake
            .borrow()
            .as_ref()
            .is_none_or(|wake| !wake.will_wake(cx.waker()))
        {
            let wake = cx.waker().clone();
            let previous = self.commands.wake.borrow_mut().replace(wake);
            drop(previous);
        }
        // Ordinary two-party traffic has no rendezvous work. Checking the
        // actual owners avoids scanning all seven network lanes after every
        // Call/Return. Keep the command waker registered and prepare incoming
        // transfers above: either can introduce work without a network frame.
        if self.pending.is_empty()
            && self.sent.is_empty()
            && self.local.is_empty()
            && self.handoffs.is_empty()
            && self.provided.is_empty()
            && self.barriers.is_empty()
            && self.produced.is_empty()
            && self.commands.queue.borrow().is_empty()
            && self.commands.markers.borrow().is_empty()
            && self.commands.adoptions.borrow().is_empty()
            && self.commands.provisions.borrow().is_empty()
            && !coordinator.driver.has_network()
            && coordinator.worker.pending_redirect().is_none()
        {
            return self.bootstrap.poll_bootstrap(
                &mut ControlContext::new(coordinator),
                messages,
                queue,
                scratch,
                output,
            );
        }
        self.poll_active(coordinator, messages, queue, scratch, output, cx)
    }
}

impl<
        'p,
        'id,
        's,
        V: 'static,
        F: FnMut() -> capnp::Result<capability::Client>,
        const C: usize,
        const H: usize,
    > Control<'p, 'id, 's, resources::Import, (), C, H, vat::Resources>
    for NetworkControl<'p, 'id, V, F, C, H>
{
    fn poll(
        &mut self,
        context: &mut ControlContext<'_, 'p, 'id, 's, resources::Import, (), C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, vat::Bundle>,
        scratch: &mut [u32],
        output: &vat::Resources,
        cx: &mut Context<'_>,
    ) -> Poll<capnp::Result<bool>> {
        let bilateral = {
            let connection = self.connection.borrow();
            connection.supports_two_party_join() && !connection.supports_multiparty_join()
        };
        self.commands.bilateral.set(bilateral);
        context.allow_join({
            let connection = self.connection.borrow();
            connection.supports_two_party_join() || connection.supports_multiparty_join()
        });
        // The restricted path certifies absence of authenticated network work.
        // Active authenticated handoffs affect several protocol owners; their
        // private operation records those effects even when a future suspends.
        if context.network_idle()
            && self.pending.is_empty()
            && self.sent.is_empty()
            && self.local.is_empty()
            && self.handoffs.is_empty()
            && self.provided.is_empty()
            && self.barriers.is_empty()
            && self.produced.is_empty()
            && self.commands.queue.borrow().is_empty()
            && self.commands.markers.borrow().is_empty()
            && self.commands.adoptions.borrow().is_empty()
            && self.commands.provisions.borrow().is_empty()
            && self.commands.forwarding.borrow().is_empty()
        {
            context.allow_answer_handoff(self.connection.borrow().supports_third_party_answers());
            if self
                .commands
                .wake
                .borrow()
                .as_ref()
                .is_none_or(|wake| !wake.will_wake(cx.waker()))
            {
                let previous = self.commands.wake.borrow_mut().replace(cx.waker().clone());
                drop(previous);
            }
            self.bootstrap
                .poll_bootstrap(context, messages, queue, scratch, output)
        } else {
            self.poll_network(context.network(), messages, queue, scratch, output, cx)
        }
    }
    fn observes_hooks(&self, event: &connection::Received<'id, server::Producer>) -> bool {
        !matches!(
            event,
            connection::Received::Queued(_)
                | connection::Received::DeferredCall(_)
                | connection::Received::Returned(_, _)
        )
    }
    fn accepts_direct_plain_return(&self) -> bool {
        true
    }

    #[inline(always)]
    fn preserves_idle(&self, activity: Activity) -> bool {
        <TwoParty<'id, F> as Control<'p, 'id, 's, resources::Import, (), C, H, vat::Resources>>::preserves_idle(&self.bootstrap, activity)
            && self.pending.is_empty()
            && self.sent.is_empty()
            && self.local.is_empty()
            && self.handoffs.is_empty()
            && self.provided.is_empty()
            && self.barriers.is_empty()
            && self.produced.is_empty()
            && self.commands.adoptions.borrow().is_empty()
            && self.commands.queue.borrow().is_empty()
            && self.commands.provisions.borrow().is_empty()
            && self.commands.forwarding.borrow().is_empty()
            && self.commands.markers.borrow().is_empty()
    }
    fn set_idle(&mut self, idle: bool) {
        let idle = idle
            && self.pending.is_empty()
            && self.sent.is_empty()
            && self.local.is_empty()
            && self.handoffs.is_empty()
            && self.provided.is_empty()
            && self.barriers.is_empty()
            && self.produced.is_empty()
            && self.commands.adoptions.borrow().is_empty()
            && self.commands.queue.borrow().is_empty()
            && self.commands.provisions.borrow().is_empty()
            && self.commands.forwarding.borrow().is_empty()
            && self.commands.markers.borrow().is_empty();
        <TwoParty<'id, F> as Control<'p, 'id, 's, resources::Import, (), C, H, vat::Resources>>::set_idle(&mut self.bootstrap, idle);
    }
    fn received(
        &mut self,
        event: connection::Received<'id, server::Producer>,
        context: &mut ControlContext<'_, 'p, 'id, 's, resources::Import, (), C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, vat::Bundle>,
    ) -> capnp::Result<()> {
        if matches!(
            event,
            connection::Received::Network | connection::Received::QueuedNetwork
        ) {
            return Ok(());
        }
        <TwoParty<'id, F> as Control<'p, 'id, 's, resources::Import, (), C, H, vat::Resources>>::received(&mut self.bootstrap, event, context, messages, queue)
    }

    fn shutdown(
        &mut self,
        context: &mut ControlContext<'_, 'p, 'id, 's, resources::Import, (), C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
    ) {
        let coordinator = context.network();
        self.commands.close(closed());
        self.local.clear();
        self.produced.clear();
        for barrier in self.barriers.drain(..) {
            coordinator.driver.release_network(
                connection::NetworkRequest::ForwardEmbargo(barrier),
                messages,
            );
        }
        for provision in self.provided.drain(..) {
            coordinator.driver.release_provision(provision.wire);
        }
        for job in self.handoffs.drain(..) {
            job.release(&mut coordinator.driver, messages);
        }
        for sent in self.sent.drain(..) {
            if let Some(response) = sent.response {
                coordinator
                    .driver
                    .release_output(CallOutput::Remote(response), messages);
            } else {
                let _ = coordinator.driver.cancel(sent.key);
            }
        }
        for job in self.pending.drain(..) {
            coordinator.driver.release_network(job.request, messages);
        }
        <TwoParty<'id, F> as Control<'p, 'id, 's, resources::Import, (), C, H, vat::Resources>>::shutdown(&mut self.bootstrap, &mut ControlContext::new(coordinator), messages);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn network_reply_and_pipeline_retain_original_bytes_after_driver_and_pool_teardown() {
        use capntproto_rpc_engine::{connection as storage, ids::WireId};
        let commands = Commands::new(1, 256);
        let (reply, pipeline, pointer) = {
            let mut buffers = messages::GrowingBuffers::new(4, 2048, 4 * 2055);
            messages::with_growing_messages(
                &mut buffers,
                storage::Growth {
                    initial: 0,
                    maximum: 4,
                },
                |messages| {
                    storage::with_domain(|domain| {
                        let mut slots = connection::FixedStorage::<
                            resources::Import,
                            server::Producer,
                            (),
                            Rc<dyn ClientHook>,
                            4,
                            4,
                            4,
                            32,
                        >::new();
                        let driver = slots.bind(domain, ()).unwrap();
                        let mut coordinator = Coordinator::new(
                            driver,
                            ClientHooks::new(8, 8),
                            NonZeroUsize::new(4).unwrap(),
                            None,
                            Options {
                                input_bytes: NonZeroUsize::new(2048).unwrap(),
                                output_bytes: NonZeroUsize::new(256).unwrap(),
                                budget: NonZeroUsize::new(16).unwrap(),
                                reader: Default::default(),
                            },
                        )
                        .unwrap();
                        let mut sends = [None];
                        let mut queue = messages::Queue::<()>::new(&mut sends).unwrap();
                        let key = coordinator
                            .driver
                            .send(
                                messages,
                                &mut queue,
                                128,
                                Default::default(),
                                capntproto_rpc_wire::calls::bootstrap,
                            )
                            .unwrap();
                        while let Some(window) = queue.window(messages).unwrap() {
                            let length = window.bytes().len();
                            window.advance(length).unwrap();
                        }
                        let frame = Frame::build(messages, 1024, Default::default(), |builder| {
                            capntproto_rpc_wire::calls::dynamic_results(
                                builder,
                                capntproto_rpc_engine::ids::AnswerId::from_wire(key.id().to_wire()),
                                |field| {
                                    field.cast::<capnp::field_api::Data>().init(512)?.fill(0x5a);
                                    Ok(())
                                },
                            )
                        })
                        .unwrap();
                        coordinator
                            .driver
                            .receive(
                                frame,
                                messages,
                                &mut [0; 8],
                                |_| resources::Import::default(),
                                server::Producer::new,
                            )
                            .unwrap_or_else(|_| panic!("Return"));
                        let response = coordinator.driver.take_response(key).unwrap();
                        let pointer = response
                            .with(messages, |reply| {
                                let owned::Reply::Results(payload) = reply else {
                                    panic!("Results")
                                };
                                payload
                                    .content()
                                    .get_as::<capnp::data::Reader>()
                                    .unwrap()
                                    .as_ptr()
                            })
                            .unwrap();
                        let mut payload = None;
                        let counts = allocation_counter::measure(|| {
                            payload =
                                Some(
                                    NetworkControl::<
                                        (),
                                        fn() -> capnp::Result<capability::Client>,
                                        4,
                                        4,
                                    >::retain_response(
                                        &response, &mut coordinator, messages
                                    )
                                    .unwrap(),
                                );
                        });
                        assert_eq!(counts.count_total, 0, "retain the already admitted reader");
                        let payload = Rc::new(payload.unwrap());
                        let pipeline = PipelineState::prepared(None);
                        pipeline.finish_payload(payload.clone());
                        let reply = Reply {
                            payload,
                            _lease: Rc::new(Lease {
                                _permit: commands.capacity.reserve_owned().unwrap(),
                                _notify: Notify(Rc::downgrade(&commands)),
                            }),
                        };
                        coordinator
                            .driver
                            .release_output(CallOutput::Remote(response), messages);
                        let Coordinator {
                            mut driver,
                            hooks,
                            calls,
                            worker,
                            foreign,
                            ..
                        } = coordinator;
                        calls.terminate(&mut driver, messages);
                        worker.terminate(&mut driver, messages);
                        foreign.terminate(&mut driver.capabilities());
                        hooks.close(&mut driver.capabilities(), closed());
                        queue.fail();
                        while queue.discard_one(messages).unwrap() {}
                        let mut closing = driver.terminate();
                        let mut done = false;
                        for _ in 0..256 {
                            if matches!(
                                closing
                                    .poll(messages, NonZeroUsize::new(32).unwrap())
                                    .unwrap(),
                                connection::Termination::Done
                            ) {
                                done = true;
                                break;
                            }
                        }
                        assert!(done);
                        (reply, pipeline, pointer)
                    })
                },
            )
            .unwrap()
        };
        let data = reply
            .get()
            .unwrap()
            .get_as::<capnp::data::Reader>()
            .unwrap();
        assert_eq!(data.as_ptr(), pointer);
        assert_eq!(data, &[0x5a; 512]);
        let weak = Rc::downgrade(&reply.payload);
        drop(reply);
        assert_eq!(commands.capacity.used(), 0);
        assert!(
            weak.upgrade().is_some(),
            "pipeline keeps the result after reply release"
        );
        {
            let PipelineVariant::Resolved(CompletedPipeline::Payload(payload)) =
                &*pipeline.variant.borrow()
            else {
                panic!("completed payload")
            };
            assert_eq!(
                payload
                    .reader()
                    .unwrap()
                    .get_as::<capnp::data::Reader>()
                    .unwrap()
                    .as_ptr(),
                pointer
            );
        }
        drop(pipeline);
        assert!(
            weak.upgrade().is_none(),
            "final pipeline release retires the reader"
        );
    }

    #[test]
    fn commands_bound_queued_and_canceled_work_and_preserve_close_errors() {
        let commands = Commands::new(1, 256);
        let handle = commands.handle();
        let body = Body::new_default();
        let first = handle.accept(body.get_root_as_reader().unwrap());
        let error = futures::executor::block_on(handle.accept(body.get_root_as_reader().unwrap()))
            .err()
            .unwrap();
        assert_eq!(error.kind, capnp::ErrorKind::Overloaded);
        drop(first);
        assert_eq!(commands.capacity.used(), 1);
        drop(commands.queue.borrow_mut().pop_front());
        assert_eq!(commands.capacity.used(), 0);
        let pending = handle.accept(body.get_root_as_reader().unwrap());
        commands.close(Error::disconnected("original network failure".into()));
        assert_eq!(
            futures::executor::block_on(pending).err().unwrap().extra,
            "original network failure"
        );
        assert_eq!(
            futures::executor::block_on(handle.accept(body.get_root_as_reader().unwrap()))
                .err()
                .unwrap()
                .extra,
            "original network failure"
        );
        assert_eq!(commands.capacity.used(), 0);
        drop(commands);
        assert_eq!(
            futures::executor::block_on(handle.accept(body.get_root_as_reader().unwrap()))
                .err()
                .unwrap()
                .kind,
            capnp::ErrorKind::Disconnected
        );
    }
    #[test]
    fn oversized_metadata_rejects_without_consuming_capacity() {
        let commands = Commands::new(1, 256);
        let mut body = Body::new_default();
        body.initn_root::<capnp::data::Builder>(512).fill(1);
        let error = futures::executor::block_on(
            commands.handle().accept(body.get_root_as_reader().unwrap()),
        )
        .err()
        .unwrap();
        assert_eq!(error.kind, capnp::ErrorKind::Overloaded);
        assert_eq!(commands.capacity.used(), 0);
        assert!(commands.queue.borrow().is_empty());
    }
}
