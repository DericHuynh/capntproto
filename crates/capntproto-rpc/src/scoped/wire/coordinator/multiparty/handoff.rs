//! Authenticated answer rendezvous. Each job owns its exact wire generation;
//! no completion can claim another connection's question by numeric ID.
use super::*;
use crate::third_party::{Answer, ThirdPartyExchange};

pub(super) enum Job<'p, 'id> {
    Await {
        token: connection::AwaitingResult<'p, 'id>,
        result: AwaitStage,
    },
    Adopt {
        token: owned::IncomingAdoption<'p, 'id>,
        exchange: Promise<Rc<ThirdPartyExchange>, Error>,
    },
}
pub(super) enum AwaitStage {
    Authenticate(Promise<(Answer, Box<dyn std::any::Any>), Error>),
    Response(Promise<Box<dyn ResponseHook>, Error>),
}
impl<'p, 'id> Job<'p, 'id> {
    pub(super) fn release<const C: usize, const H: usize>(
        self,
        driver: &mut Peer<'p, 'id, '_, resources::Import, (), C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
    ) {
        driver.release_network(
            match self {
                Self::Await { token, .. } => connection::NetworkRequest::Await(token),
                Self::Adopt { token, .. } => connection::NetworkRequest::Adopt(token),
            },
            messages,
        );
    }
}

fn setup<T: 'static>(ready: Promise<T, Error>, deadline: Promise<(), Error>) -> Promise<T, Error> {
    Promise::from_future(async move {
        match futures::future::select(ready, deadline).await {
            futures::future::Either::Left((result, _)) => result,
            futures::future::Either::Right((result, _)) => Err(Error::disconnected(match result {
                Ok(()) => "third-party answer setup timed out".into(),
                Err(error) => format!("third-party answer deadline failed: {error}"),
            })),
        }
    })
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
    pub(super) fn receive_handoff<'s>(
        &mut self,
        request: connection::NetworkRequest<'p, 'id, H>,
        coordinator: &mut Coordinator<'p, 'id, 's, resources::Import, (), C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
    ) -> capnp::Result<()> {
        if !self.connection.borrow().supports_third_party_answers()
            || self.handoffs.len() >= self.maximum
            || self.handoffs.try_reserve(1).is_err()
        {
            coordinator.driver.release_network(request, messages);
            return Err(Error::overloaded(
                "answer rendezvous unavailable or full".into(),
            ));
        }
        match request {
            connection::NetworkRequest::Await(token) => {
                let (exchange, receiver) = ThirdPartyExchange::new_answer();
                let registration = token.with_metadata(messages, |metadata| {
                    self.connection
                        .borrow_mut()
                        .await_third_party(metadata, exchange)
                });
                let registration = match registration.map_err(wire_error).and_then(|r| r) {
                    Ok(guard) => guard,
                    Err(error) => {
                        coordinator
                            .driver
                            .release_network(connection::NetworkRequest::Await(token), messages);
                        return Err(error);
                    }
                };
                let received = setup(
                    Promise::from_future(async move { receiver.await.map_err(|_| closed()) }),
                    self.connection.borrow().third_party_answer_timeout(),
                );
                // Timeout applies to authentication only. A legitimate method
                // may continue beyond the setup deadline.
                let result = AwaitStage::Authenticate(Promise::from_future(async move {
                    let answer = received.await?;
                    Ok((answer, registration))
                }));
                self.handoffs.push(Job::Await { token, result });
            }
            connection::NetworkRequest::Adopt(token) => {
                let completing = match token.with_completion(messages, |completion| {
                    self.connection
                        .borrow_mut()
                        .complete_third_party(completion)
                }) {
                    Ok(future) => future,
                    Err(error) => {
                        coordinator
                            .driver
                            .release_network(connection::NetworkRequest::Adopt(token), messages);
                        return Err(wire_error(error));
                    }
                };
                let exchange = setup(
                    completing,
                    self.connection.borrow().third_party_answer_timeout(),
                );
                self.handoffs.push(Job::Adopt { token, exchange });
            }
            _ => unreachable!("answer rendezvous"),
        }
        Ok(())
    }

    pub(super) fn poll_handoffs<'s>(
        &mut self,
        coordinator: &mut Coordinator<'p, 'id, 's, resources::Import, (), C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        cx: &mut Context<'_>,
    ) -> Poll<capnp::Result<()>> {
        for index in (0..self.handoffs.len()).rev() {
            match &mut self.handoffs[index] {
                Job::Await { token, result } => {
                    let Some(method) = coordinator.driver.awaiting_method(token) else {
                        self.handoffs
                            .swap_remove(index)
                            .release(&mut coordinator.driver, messages);
                        return Poll::Ready(Ok(()));
                    };
                    if let AwaitStage::Authenticate(future) = result {
                        let Poll::Ready(ready) = Pin::new(future).poll(cx) else {
                            continue;
                        };
                        *result = AwaitStage::Response(match ready {
                            Ok((answer, guard)) => {
                                coordinator.calls.adopt_pipeline(
                                    &mut coordinator.hooks,
                                    &mut coordinator.driver,
                                    token.key(),
                                    answer.pipeline.add_ref(),
                                )?;
                                Promise::from_future(async move {
                                    let result = answer.response.await;
                                    drop((guard, answer.pipeline));
                                    result
                                })
                            }
                            Err(error) => Promise::err(error),
                        });
                        // Authentication can install a pipeline before its
                        // response is ready. Report the transition so the
                        // coordinator revisits previously idle protocol lanes.
                        return Poll::Ready(Ok(()));
                    }
                    let AwaitStage::Response(future) = result else {
                        unreachable!()
                    };
                    let Poll::Ready(result) = Pin::new(future).poll(cx) else {
                        continue;
                    };
                    let Job::Await { token, .. } = self.handoffs.swap_remove(index) else {
                        unreachable!()
                    };
                    let result =
                        result.and_then(|response| freeze(response, method, coordinator, messages));
                    let outcome = coordinator.driver.complete_await_retaining_routes(
                        token,
                        result.map_err(|error| error.kind),
                        messages,
                        |_| Ok::<_, Error>(()),
                    );
                    if let Err(rejected) = outcome {
                        if let Ok(result) = rejected.result {
                            result.release(messages, &mut coordinator.driver.capabilities());
                        }
                        coordinator.driver.release_network(
                            connection::NetworkRequest::Await(rejected.awaiting),
                            messages,
                        );
                        return Poll::Ready(Err(wire_error(format!(
                            "answer handoff: {:?}",
                            rejected.error
                        ))));
                    }
                    return Poll::Ready(Ok(()));
                }
                Job::Adopt { token: _, exchange } => {
                    let Poll::Ready(result) = Pin::new(exchange).poll(cx) else {
                        continue;
                    };
                    let Job::Adopt { token, .. } = self.handoffs.swap_remove(index) else {
                        unreachable!()
                    };
                    let exchange = match result {
                        Ok(exchange) => exchange,
                        Err(error) => {
                            coordinator.driver.release_network(
                                connection::NetworkRequest::Adopt(token),
                                messages,
                            );
                            return Poll::Ready(if error.kind == capnp::ErrorKind::Disconnected {
                                Ok(())
                            } else {
                                Err(error)
                            });
                        }
                    };
                    let permit = match self.commands.capacity.reserve_owned() {
                        Ok(permit) if self.sent.try_reserve(1).is_ok() => permit,
                        _ => {
                            coordinator.driver.release_network(
                                connection::NetworkRequest::Adopt(token),
                                messages,
                            );
                            return Poll::Ready(Err(Error::overloaded(
                                "adopted answer capacity exhausted".into(),
                            )));
                        }
                    };
                    let (sender, receiver) = shared::channel();
                    let pipeline = Rc::new(PipelineState {
                        used: Cell::new(false),
                        // Projections and final-interest drops belong to this
                        // network control, not the ordinary outgoing-call lane.
                        owner_wake: RefCell::new(Some(cx.waker().clone())),
                        adopted: RefCell::default(),
                        cached: RefCell::default(),
                        _credit: Cell::new(None),
                        waiting: RefCell::default(),
                        finished: RefCell::default(),
                        inbox: Rc::downgrade(&coordinator.hooks.inbox),
                        maximum: self.maximum,
                    });
                    let answer = Answer {
                        response: Promise::from_future(async move {
                            receiver.await.map_err(|_| closed())?
                        }),
                        pipeline: Box::new(WirePipeline(PipelineRef(Some(pipeline.clone()), None))),
                    };
                    match coordinator
                        .driver
                        .authorize_adoption(token, messages, |_| exchange.adopt(answer))
                    {
                        Ok(key) => self.sent.push(Sent {
                            key,
                            sender: Some(sender),
                            permit: Some(permit),
                            response: None,
                            observer: Weak::new(),
                            pipeline: Some(pipeline),
                        }),
                        Err(rejected) => {
                            coordinator.driver.release_network(
                                connection::NetworkRequest::Adopt(rejected.adoption),
                                messages,
                            );
                            return Poll::Ready(Err(wire_error(format!(
                                "answer authorization: {:?}",
                                rejected.error
                            ))));
                        }
                    }
                    return Poll::Ready(Ok(()));
                }
            }
        }
        Poll::Pending
    }
}

pub(super) fn project<'p, 'id, const C: usize, const H: usize>(
    key: questions::Key<'id>,
    pipeline: &Rc<PipelineState>,
    coordinator: &mut Coordinator<'p, 'id, '_, resources::Import, (), C, H>,
) -> capnp::Result<bool> {
    let waiting = std::mem::take(&mut *pipeline.waiting.borrow_mut());
    let changed = !waiting.is_empty();
    for projection in waiting {
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
        let result = coordinator
            .driver
            .pipeline(key, pipeline::Path::new(&fields))
            .map_err(wire_error)
            .and_then(|cap| {
                let result = coordinator
                    .hooks
                    .export_snapshot(&mut coordinator.driver.capabilities(), &cap);
                coordinator.driver.capabilities().release(cap);
                result.map(|cap| cap.hook)
            });
        if let Ok(hook) = &result {
            pipeline.remember(projection.path, &**hook)?;
        }
        client.resolve(result);
    }
    Ok(changed)
}

fn freeze<'p, 'id, const C: usize, const H: usize>(
    response: Box<dyn ResponseHook>,
    method: (u64, u16),
    coordinator: &mut Coordinator<'p, 'id, '_, resources::Import, (), C, H>,
    messages: &mut messages::Messages<'p, '_, '_>,
) -> capnp::Result<connection::LocalResult<'p, 'id, C>> {
    let response: Rc<dyn ResponseHook> = Rc::from(response);
    let mut payload = Payload::copy(response.get()?)?;
    for cap in payload.caps.iter_mut().flatten() {
        *cap = Box::new(Retained {
            inner: cap.add_ref(),
            response: response.clone(),
        });
    }
    let driver = &mut coordinator.driver;
    let mut clients = Vec::new();
    let result = (|| {
        if payload.caps.len() > driver.capability_capacity() {
            return Err(Error::overloaded(
                "adopted result capability capacity exhausted".into(),
            ));
        }
        for cap in &payload.caps {
            clients.push(match cap {
                None => None,
                Some(cap) => Some(
                    match coordinator
                        .hooks
                        .try_retain(&mut driver.capabilities(), &**cap)?
                    {
                        Some(client) => client,
                        None => coordinator
                            .foreign
                            .import(&mut driver.capabilities(), &**cap)?,
                    },
                ),
            });
        }
        let content = payload.reader()?;
        driver
            .local_result_dynamic(
                method.0,
                method.1,
                &clients.iter().map(Option::as_ref).collect::<Vec<_>>(),
                messages,
                minimum(content)?,
                coordinator.options.reader,
                |field| {
                    field.copy_with_capabilities(
                        content,
                        capnp::wire_capability::IndexMap::identity(payload.caps.len() as u32),
                    )
                },
            )
            .map_err(wire_error)
    })();
    for client in clients.into_iter().flatten() {
        driver.capabilities().release(client);
    }
    result
}

// Results copied between graph domains keep their source answer alive while
// capabilities derived from that result survive. No numeric key crosses domains.
struct Retained {
    inner: Box<dyn ClientHook>,
    response: Rc<dyn ResponseHook>,
}
impl ClientHook for Retained {
    fn get_failure(&self) -> Option<Error> {
        self.inner.get_failure()
    }
    fn add_ref(&self) -> Box<dyn ClientHook> {
        Box::new(Self {
            inner: self.inner.add_ref(),
            response: self.response.clone(),
        })
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
                .attach(self.response.clone()),
        )
    }
    fn get_brand(&self) -> usize {
        self.inner.get_brand()
    }
    fn get_ptr(&self) -> usize {
        self.inner.get_ptr()
    }
    fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
        self.inner.get_resolved().map(|inner| {
            Box::new(Self {
                inner,
                response: self.response.clone(),
            }) as _
        })
    }
    fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, Error>> {
        let future = self.inner.when_more_resolved()?;
        let response = self.response.clone();
        Some(Promise::from_future(async move {
            Ok(Box::new(Self {
                inner: future.await?,
                response,
            }) as Box<dyn ClientHook>)
        }))
    }
    fn when_resolved(&self) -> Promise<(), Error> {
        self.inner.when_resolved()
    }
    fn forward_join(&self, part: any_pointer::Reader<'_>) -> Option<PartResponse> {
        self.inner.forward_join(part)
    }
    fn metadata_changes(&self) -> Option<Rc<capnp::private::capability::MetadataChanges>> {
        self.inner.metadata_changes()
    }
    #[cfg(unix)]
    fn get_fd(&self) -> Option<Rc<std::os::fd::OwnedFd>> {
        self.inner.get_fd()
    }
}
