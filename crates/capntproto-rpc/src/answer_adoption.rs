// Copyright (c) 2026 ReProto contributors
// Licensed under the MIT license; see LICENSE.

//! Authenticated level-3 tail return adoption using the network rendezvous hooks.
use super::*;

const FIRST: u32 = 1 << 30;
const END: u32 = 1 << 31;
// Finished questions must reserve their ID until Return, even when setup timed
// out. Bound those tombstones together with live adoptions so a peer cannot
// recycle the rendezvous waiter quota into unbounded question-table growth.
const MAX_ADOPTED_QUESTIONS: usize = 4096;

pub(super) fn ordinary_id(id: AnswerId) -> capnp::Result<()> {
    if (FIRST..END).contains(&id.to_wire()) {
        Err(Error::failed(
            "caller used a callee-allocated question ID".into(),
        ))
    } else {
        Ok(())
    }
}
pub(super) fn supported<VatId>(state: &ConnectionState<VatId>) -> bool {
    state
        .connection
        .borrow()
        .as_ref()
        .is_ok_and(|c| c.supports_third_party_answers())
}

fn deadline<VatId>(state: &ConnectionState<VatId>) -> Promise<(), Error> {
    match state.connection.borrow().as_ref() {
        Ok(connection) => connection.third_party_answer_timeout(),
        Err(error) => Promise::err(error.clone()),
    }
}

// Poll the authenticated completion first if both are ready in one executor
// turn. Timeout releases only this exchange/question, never a healthy peer.
fn setup<T: 'static>(ready: Promise<T, Error>, timeout: Promise<(), Error>) -> Promise<T, Error> {
    Promise::from_future(async move {
        match futures::future::select(ready, timeout).await {
            futures::future::Either::Left((result, _)) => result,
            futures::future::Either::Right((result, _)) => Err(Error::disconnected(match result {
                Ok(()) => "third-party answer setup timed out".into(),
                Err(error) => format!("third-party answer deadline failed: {error}"),
            })),
        }
    })
}

/// Freeze new calls behind a queued client, then send independent Join shares
/// through both paths. The old share follows previously sent calls; equality
/// authenticates that it reached the adopted object, not merely a relay. Any
/// unsupported boundary, inequality, timeout or failure keeps the old route.
pub(super) fn fence<VatId>(
    state: &Rc<ConnectionState<VatId>>,
    old: Box<dyn ClientHook>,
    adopted: Box<dyn ClientHook>,
) -> Option<Box<dyn ClientHook>> {
    if !state
        .connection
        .borrow()
        .as_ref()
        .is_ok_and(|c| c.supports_pipeline_join_fence())
    {
        return None;
    }
    let registry = state.registry.upgrade()?;
    let (network, context) = {
        let registry = registry.borrow();
        if registry.closing {
            return None;
        }
        (
            registry.join_network.clone()?,
            registry.join_context.upgrade()?,
        )
    };
    let registry = Rc::downgrade(&registry);
    let caps = vec![old.clone(), adopted];
    let joining = Promise::from_future(async move {
        let mut prepared = Vec::new();
        for mut cap in caps {
            // Unwrap synchronous resolutions, but do not wait for a native
            // promised-answer target's Return. Join can address that pipeline
            // on the wire and settle it at its owner after set_pipeline().
            let mut owners = Vec::new();
            while let Some(next) = cap.get_resolved() {
                if owners.len() >= 64 {
                    return Err(Error::failed("cyclic pipeline resolution".into()));
                }
                owners.push(cap);
                cap = next;
            }
            // Opaque ClientHook methods may reenter the RPC system. Query them
            // before borrowing its connection registry.
            let brand = cap.get_brand();
            let ptr = cap.get_ptr();
            let native = registry.upgrade().is_some_and(|registry| {
                registry.borrow().states.values().any(|state| {
                    brand == state.get_brand() && Client::from_ptr(ptr, state).is_some()
                })
            });
            prepared.push(if native {
                cap
            } else {
                join::settle(cap).await?
            });
        }
        multiparty_join::start(
            registry,
            context.bootstrap.clone(),
            context.tasks.clone(),
            network,
            prepared,
        )
        .await
    });
    let joining = setup(joining, deadline(state));
    let mut queued = queued::Client::new(None);
    let weak = Rc::downgrade(&queued.inner);
    queued.drive(async move {
        let replacement = match joining.await {
            Ok(mut outcome) => outcome.cap.take().unwrap_or(old),
            _ => old,
        };
        if let Some(queued) = weak.upgrade() {
            queued::ClientInner::resolve(&queued, Ok(replacement));
        }
        Ok(())
    });
    Some(Box::new(queued))
}

pub(super) fn await_answer<VatId>(
    state: &Rc<ConnectionState<VatId>>,
    token: any_pointer::Reader<'_>,
    pipeline: Option<Weak<RefCell<PipelineState<VatId>>>>,
) -> Promise<Response<VatId>, Error> {
    let (exchange, receiver) = crate::third_party::ThirdPartyExchange::new_answer();
    let registration = match state.connection.borrow_mut().as_mut() {
        Ok(connection) => connection.await_third_party(token, exchange),
        Err(error) => Err(error.clone()),
    };
    let registration = crate::pry!(registration);
    let answer = setup(
        Promise::from_future(receiver.map_err(crate::canceled_to_error)),
        deadline(state),
    );
    Promise::from_future(async move {
        let answer = answer.await?;
        // The exchange is only delivered after both sides of the authenticated
        // rendezvous match. Its pipeline can be used before its response exists.
        if let Some(pipeline) = pipeline.and_then(|p| p.upgrade()) {
            PipelineState::adopt(&pipeline, answer.pipeline);
        }
        let response = answer.response.await?;
        Ok(Response {
            variant: Rc::new(ResponseVariant::Adopted(response, registration)),
        })
    })
}

pub(super) fn receive<VatId>(
    state: &Rc<ConnectionState<VatId>>,
    answer: crate::rpc_capnp::third_party_answer::Reader<'_>,
) -> capnp::Result<()> {
    let id = QuestionId::from_wire(answer.get_answer_id());
    if !id.is_adopted() || state.questions.borrow_mut().find(id).is_some() {
        return Err(Error::failed(
            "invalid or in-use ThirdPartyAnswer ID".into(),
        ));
    }
    if state.questions.borrow().adopted_len() >= MAX_ADOPTED_QUESTIONS {
        return Err(Error::overloaded(
            "third-party answer question limit".into(),
        ));
    }
    let completing = state
        .connection
        .borrow_mut()
        .as_mut()
        .map_err(|e| e.clone())?
        .complete_third_party(answer.get_completion());
    let completing = setup(completing, deadline(state));
    // Reserve the slot before constructing its cleanup owner. A failed insert
    // must not create a QuestionRef whose Drop could Finish somebody else's ID.
    state
        .questions
        .borrow_mut()
        .insert_adopted(id, Question::new())
        .map_err(|_| Error::failed("ThirdPartyAnswer ID became occupied".into()))?;
    let (sender, receiver) = oneshot::channel();
    let reference = Rc::new(RefCell::new(QuestionRef::new(state.clone(), id, sender)));
    state.questions.borrow_mut().find(id).unwrap().self_ref = Some(Rc::downgrade(&reference));
    // Install the sparse question synchronously: Return may precede the redirect
    // on the introducer connection, but is not exposed until authentication matches.
    let response = Promise::from_future(
        receiver
            .map_err(crate::canceled_to_error)
            .and_then(|r| r)
            .attach(reference.clone()),
    )
    .shared();
    let pipeline = Pipeline::new(
        state,
        reference.clone(),
        Some(Promise::from_future(response.clone())),
    );
    let answer = crate::third_party::Answer {
        // Pipeline resolution is eager, including errors. Keep the adopted
        // question alive until authorization consumes or cancels the exchange.
        response: Promise::from_future(
            response
                .map_ok(|r| Box::new(r) as Box<dyn ResponseHook>)
                .attach(reference),
        ),
        pipeline: Box::new(pipeline),
    };
    let weak = Rc::downgrade(state);
    state.system_tasks.clone().add(async move {
        let result = match completing.await {
            Ok(exchange) => exchange.adopt(answer),
            // A retired/canceled rendezvous only closes this question. A wrong
            // exchange kind or repeated claim is a protocol violation below.
            Err(error) => {
                drop(answer);
                if error.kind == capnp::ErrorKind::Disconnected {
                    Ok(())
                } else {
                    Err(error)
                }
            }
        };
        if let Err(error) = result {
            if let Some(state) = weak.upgrade() {
                state.disconnect(error);
            }
        }
        Ok(())
    });
    Ok(())
}

pub(super) fn prepare_tail<VatId>(
    inner: &ResultsInner<VatId>,
    request: &mut dyn RequestHook,
) -> capnp::Result<Option<Box<dyn crate::OutgoingMessage>>> {
    if !supported(&inner.connection_state) {
        return Ok(None);
    }
    let Some(context) = request.third_party_tail_target() else {
        return Ok(None);
    };
    let Some(destination) = context.downcast_ref::<Rc<ConnectionState<VatId>>>() else {
        return Ok(None);
    };
    if !Weak::ptr_eq(&inner.connection_state.registry, &destination.registry)
        || Rc::ptr_eq(&inner.connection_state, destination)
    {
        return Ok(None);
    }
    let peer = destination
        .connection
        .borrow()
        .as_ref()
        .map_err(|e| e.clone())?
        .get_peer_vat_id();
    let mut contact = capnp::message::Builder::new_default();
    let mut redirect = inner.connection_state.new_outgoing_message(32)?;
    let mut ret = redirect
        .get_body()?
        .init_as::<message::Builder>()
        .init_return();
    ret.set_answer_id(inner.answer_id.to_wire());
    ret.set_release_param_caps(false);
    let introduced = inner
        .connection_state
        .connection
        .borrow_mut()
        .as_mut()
        .map_err(|e| e.clone())?
        .introduce_to(peer, contact.init_root(), ret.init_await_from_third_party())?;
    if !introduced {
        return Ok(None);
    }
    request.set_third_party_tail_target(contact.get_root_as_reader()?)?;
    Ok(Some(redirect))
}

pub(super) fn receive_call<VatId>(
    origin: &Rc<ConnectionState<VatId>>,
    message: Box<dyn crate::IncomingMessage>,
    fds: &mut IncomingFds,
) -> capnp::Result<()> {
    if !supported(origin) {
        return Err(Error::unimplemented(
            "third-party tail calls unavailable".into(),
        ));
    }
    let message::Call(call) = message.get_body()?.get_as::<message::Reader>()?.which()? else {
        unreachable!()
    };
    let call = call?;
    let original_id = AnswerId::from_wire(call.get_question_id());
    if origin.answers.borrow().slots.contains_key(&original_id) {
        return Err(Error::failed("questionId is already in use".into()));
    }
    let capability = origin.get_message_target(call.get_target()?)?;
    let call::send_results_to::ThirdParty(contact) = call.get_send_results_to().which()? else {
        unreachable!()
    };
    let mut completion = capnp::message::Builder::new_default();
    let connection = origin
        .connection
        .borrow_mut()
        .as_mut()
        .map_err(|e| e.clone())?
        .connect_to_introduced(contact, completion.init_root())?;
    let Some(connection) = connection else {
        let completing = origin
            .connection
            .borrow_mut()
            .as_mut()
            .map_err(|e| e.clone())?
            .complete_third_party_local(completion.get_root_as_reader()?);
        return receive_self_call(origin, message, completing, fds);
    };
    let registry = origin
        .registry
        .upgrade()
        .ok_or_else(|| Error::disconnected("RPC system ended".into()))?;
    if registry.borrow().closing {
        return Err(Error::disconnected("RPC system is closing".into()));
    }
    let destination = crate::RpcSystem::get_connection_state(
        &registry,
        origin.bootstrap.clone(),
        connection,
        origin.system_tasks.clone(),
    );
    if !supported(&destination) {
        return Err(Error::unimplemented(
            "destination cannot adopt answers".into(),
        ));
    }
    let id = loop {
        let id = FIRST | (destination.next_adopted_answer.get() & (FIRST - 1));
        destination.next_adopted_answer.set(id.wrapping_add(1));
        let id = AnswerId::from_wire(id);
        if !destination.answers.borrow().slots.contains_key(&id) {
            break id;
        }
        if destination.answers.borrow().slots.len() >= FIRST as usize {
            return Err(Error::overloaded("adopted answer IDs exhausted".into()));
        }
    };
    // Allocate/validate all wire content before publishing an answer or invoking code.
    let mut notice = destination.new_outgoing_message(32)?;
    let mut adopted = notice
        .get_body()?
        .init_as::<message::Builder>()
        .init_third_party_answer();
    adopted.set_answer_id(id.to_wire());
    adopted
        .get_completion()
        .set_as(completion.get_root_as_reader::<any_pointer::Reader>()?)?;
    let (interface, method) = (call.get_interface_id(), call.get_method_id());
    let hints = CallHints {
        no_promise_pipelining: call.get_no_promise_pipelining(),
        only_promise_pipeline: false,
    };
    let caps = ConnectionState::receive_caps(origin, call.get_params()?.get_cap_table()?, fds)?;
    let words = message.size_in_words();
    let total = origin
        .call_words
        .get()
        .checked_add(words)
        .ok_or_else(|| Error::overloaded("incoming call word count overflow".into()))?;
    let mut parent = Answer::new();
    parent.request_words = words;
    let mut direct = Answer::new();
    direct.callee_allocated = true;
    let ack = Rc::new(ReturnGuard {
        state: Rc::downgrade(origin),
        id: original_id,
        status: parent.status.clone(),
        redirect: true,
        only_pipeline: false,
    });
    let (sender, receiver) = oneshot::channel();
    let (pipeline_sender, mut pipeline) = queued::Pipeline::new();
    let results = Results::new(
        &destination,
        id,
        false,
        sender,
        direct.status.clone(),
        Some(pipeline_sender.weak_clone()),
    );
    origin.call_words.set(total);
    origin
        .answers
        .borrow_mut()
        .slots
        .insert(original_id, parent);
    destination.answers.borrow_mut().slots.insert(id, direct);
    let _ = notice.send_detached();
    let invocation = capability.call_with_hints(
        interface,
        method,
        Box::new(Params::new(message, caps)),
        Box::new(results),
        hints,
    );
    let task = async move {
        let status = invocation.await;
        let _ = ResultsDone::from_results_inner(
            receiver.await.map_err(crate::canceled_to_error),
            status,
            pipeline_sender,
        );
        drop(ack);
        Ok(())
    }
    .boxed_local()
    .shared();
    pipeline.drive(task.clone());
    let original_running = origin.eagerly_evaluate(task.clone());
    let direct_running = destination.eagerly_evaluate(task);
    {
        let mut answers = origin.answers.borrow_mut();
        if let Some(parent) = answers.slots.get_mut(&original_id) {
            parent.pipeline = Some(Box::new(pipeline.clone()));
            parent.call_completion_promise = Some(original_running);
        }
    }
    {
        let mut answers = destination.answers.borrow_mut();
        if let Some(direct) = answers.slots.get_mut(&id) {
            direct.pipeline = Some(Box::new(pipeline));
            direct.call_completion_promise = Some(direct_running);
        }
    }
    Ok(())
}

fn receive_self_call<VatId>(
    origin: &Rc<ConnectionState<VatId>>,
    message: Box<dyn crate::IncomingMessage>,
    completing: Promise<Rc<crate::third_party::ThirdPartyExchange>, Error>,
    fds: &mut IncomingFds,
) -> capnp::Result<()> {
    let completing = setup(completing, deadline(origin));
    let message::Call(call) = message.get_body()?.get_as::<message::Reader>()?.which()? else {
        unreachable!()
    };
    let call = call?;
    let id = AnswerId::from_wire(call.get_question_id());
    let capability = origin.get_message_target(call.get_target()?)?;
    let (interface, method) = (call.get_interface_id(), call.get_method_id());
    let hints = CallHints {
        no_promise_pipelining: call.get_no_promise_pipelining(),
        only_promise_pipeline: false,
    };
    let caps = ConnectionState::receive_caps(origin, call.get_params()?.get_cap_table()?, fds)?;
    let words = message.size_in_words();
    let total = origin
        .call_words
        .get()
        .checked_add(words)
        .ok_or_else(|| Error::overloaded("incoming call word count overflow".into()))?;
    let mut answer = Answer::new();
    answer.request_words = words;
    let (sender, receiver) = oneshot::channel();
    let (pipeline_sender, mut pipeline) = queued::Pipeline::new();
    let results = Results::new(
        origin,
        id,
        true,
        sender,
        answer.status.clone(),
        Some(pipeline_sender.weak_clone()),
    );
    origin.call_words.set(total);
    origin.answers.borrow_mut().slots.insert(id, answer);
    let invocation = capability.call_with_hints(
        interface,
        method,
        Box::new(Params::new(message, caps)),
        Box::new(results),
        hints,
    );
    let (send_result, result) = oneshot::channel();
    let task = async move {
        let status = invocation.await;
        let result = ResultsDone::from_results_inner(
            receiver.await.map_err(crate::canceled_to_error),
            status,
            pipeline_sender,
        );
        let _ = send_result.send(result);
        Ok(())
    }
    .boxed_local()
    .shared();
    pipeline.drive(task.clone());
    let running = origin.eagerly_evaluate(task.clone());
    let result_owner = origin.eagerly_evaluate(task);
    let adopted_pipeline = pipeline.clone();
    {
        let mut answers = origin.answers.borrow_mut();
        if let Some(answer) = answers.slots.get_mut(&id) {
            answer.pipeline = Some(Box::new(pipeline));
            answer.call_completion_promise = Some(running);
        }
    }
    let response = Promise::from_future(
        async move {
            let result = result
                .await
                .map_err(crate::canceled_to_error)??
                .into_retained()?;
            Ok(Box::new(Response::<VatId>::redirected(result)) as Box<dyn ResponseHook>)
        }
        .boxed_local()
        .attach(result_owner),
    );
    let answer = crate::third_party::Answer {
        response,
        pipeline: Box::new(adopted_pipeline),
    };
    let weak = Rc::downgrade(origin);
    origin.system_tasks.clone().add(async move {
        match completing.await {
            Ok(exchange) => {
                if let Err(error) = exchange.adopt(answer) {
                    if let Some(origin) = weak.upgrade() {
                        origin.disconnect(error);
                    }
                }
            }
            Err(error) => {
                drop(answer);
                if error.kind != capnp::ErrorKind::Disconnected {
                    if let Some(origin) = weak.upgrade() {
                        origin.disconnect(error);
                    }
                }
            }
        }
        Ok(())
    });
    Ok(())
}
