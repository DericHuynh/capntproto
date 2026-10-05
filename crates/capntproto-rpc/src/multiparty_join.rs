//! Independently routed standard Join messages and direct capability acquisition.
use super::*;
use crate::multiparty::{JoinDestination, JoinNetwork, PartResponse};
use capnp::private::capability::JoinedCapability;

pub(super) fn supported<VatId>(state: &ConnectionState<VatId>) -> bool {
    state
        .connection
        .borrow()
        .as_ref()
        .is_ok_and(|c| c.supports_multiparty_join())
}

struct LocalReply {
    body: capnp::message::Builder<capnp::message::HeapAllocator>,
    _guard: Box<dyn std::any::Any>,
}
impl ResponseHook for LocalReply {
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        self.body.get_root_as_reader()
    }
}

pub(super) fn send_part<VatId>(
    state: &Rc<ConnectionState<VatId>>,
    target: Box<dyn ClientHook>,
    part: any_pointer::Reader<'_>,
) -> PartResponse {
    if !supported(state) {
        return Promise::err(Error::unimplemented(
            "multiparty Join unavailable on this route".into(),
        ));
    }
    let mut message = crate::pry!(state.new_outgoing_message(32));
    let mut request = crate::pry!(message.get_body())
        .init_as::<message::Builder>()
        .init_join();
    if state
        .write_target(&*target, request.reborrow().init_target())
        .is_some()
    {
        return Promise::err(Error::failed(
            "Join target changed during forwarding".into(),
        ));
    }
    crate::pry!(request.reborrow().get_key_part().set_as(part));
    let mut question = Question::new();
    question.multiparty_join = true;
    let id = state.questions.borrow_mut().push(question);
    request.set_question_id(id.to_wire());
    state.set_not_idle();
    response_for(state, id, message)
}

fn response_for<VatId>(
    state: &Rc<ConnectionState<VatId>>,
    id: QuestionId,
    message: Box<dyn crate::OutgoingMessage>,
) -> PartResponse {
    let (tx, rx) = oneshot::channel();
    let reference = Rc::new(RefCell::new(QuestionRef::new(state.clone(), id, tx)));
    state.questions.borrow_mut().find(id).unwrap().self_ref = Some(Rc::downgrade(&reference));
    let _ = message.send_detached();
    Promise::from_future(
        rx.map_err(crate::canceled_to_error)
            .and_then(|r| r)
            .map_ok(|r| Box::new(r) as Box<dyn ResponseHook>)
            .attach(reference),
    )
}

fn part_at<VatId: 'static>(
    network: Rc<dyn JoinNetwork<VatId>>,
    target: Box<dyn ClientHook>,
    part: any_pointer::Reader<'_>,
) -> PartResponse {
    if let Some(response) = target.forward_join(part) {
        return response;
    }
    let identity = (target.get_brand(), target.get_ptr());
    let value = crate::third_party::ThirdPartyExchange::new(target);
    let (body, guard) = crate::pry!(network.contribute(part, identity, value));
    Promise::ok(Box::new(LocalReply {
        body,
        _guard: guard,
    }))
}

pub(super) fn start<VatId>(
    registry: Weak<RefCell<ConnectionRegistry<VatId>>>,
    bootstrap: crate::Bootstrap<VatId>,
    tasks: crate::task_set::TaskSetHandle<Error>,
    network: Rc<dyn JoinNetwork<VatId>>,
    caps: Vec<Box<dyn ClientHook>>,
) -> Promise<JoinedCapability, Error> {
    Promise::from_future(async move {
        let session = network.start(caps.len() as u16)?;
        let mut replies = Vec::new();
        for (i, cap) in caps.into_iter().enumerate() {
            let part = session.part(i as u16)?;
            replies.push(part_at(network.clone(), cap, part.get_root_as_reader()?));
        }
        let replies = future::try_join_all(replies).await?;
        let Some(destination) = session.finish(&replies)? else {
            return Ok(JoinedCapability {
                cap: None,
                guard: Box::new(replies),
            });
        };
        let (cap, acquired): (Box<dyn ClientHook>, Option<Box<dyn ResponseHook>>) =
            match destination {
                JoinDestination::Local(value) => (value.accept(None).await?, None),
                JoinDestination::Remote {
                    connection,
                    completion,
                } => {
                    let registry = registry
                        .upgrade()
                        .ok_or_else(|| Error::disconnected("RPC system ended".into()))?;
                    if registry.borrow().closing {
                        return Err(Error::disconnected("RPC system is closing".into()));
                    }
                    let state = crate::RpcSystem::get_connection_state(
                        &registry, bootstrap, connection, tasks,
                    );
                    let mut message = state.new_outgoing_message(32)?;
                    let mut accept = message
                        .get_body()?
                        .init_as::<message::Builder>()
                        .init_accept();
                    accept
                        .reborrow()
                        .get_provision()
                        .set_as(completion.get_root_as_reader::<any_pointer::Reader>()?)?;
                    let id = state.questions.borrow_mut().push(Question::new());
                    accept.set_question_id(id.to_wire());
                    let response = response_for(&state, id, message).await?;
                    let cap = response.get()?.get_as_capability()?;
                    (cap, Some(response))
                }
            };
        // Neither relayed parts nor the direct Accept is finished until the
        // public caller has acquired its independently retained capability.
        Ok(JoinedCapability {
            cap: Some(cap),
            guard: Box::new((replies, acquired)),
        })
    })
}

pub(super) fn receive<VatId>(
    owner: &Rc<ConnectionState<VatId>>,
    request: crate::rpc_capnp::join::Reader<'_>,
) -> capnp::Result<()> {
    let id = AnswerId::from_wire(request.get_question_id());
    if id.to_wire() >= 1 << 30 || owner.answers.borrow().slots.contains_key(&id) {
        return Err(Error::failed("invalid or in-use Join question ID".into()));
    }
    let network = owner
        .registry
        .upgrade()
        .and_then(|r| r.borrow().join_network.clone())
        .ok_or_else(|| Error::unimplemented("multiparty Join network unavailable".into()))?;
    let part = network.forward(request.get_key_part())?;
    let target = owner.get_message_target(request.get_target()?)?;
    let mut answer = Answer::new();
    answer.join_response = Some(Box::new(()));
    let (sender, receiver) = oneshot::channel();
    let (pipeline_sender, mut pipeline) = queued::Pipeline::new();
    let mut results = Results::new(
        owner,
        id,
        false,
        sender,
        answer.status.clone(),
        Some(pipeline_sender.weak_clone()),
    );
    owner.answers.borrow_mut().slots.insert(id, answer);
    let weak = Rc::downgrade(owner);
    let task = async move {
        let status = async {
            let target = join::settle(target).await?;
            let response = part_at(network, target, part.get_root_as_reader()?).await?;
            results.get()?.set_as(response.get()?)?;
            if let Some(owner) = weak.upgrade() {
                if let Some(answer) = owner.answers.borrow_mut().slots.get_mut(&id) {
                    answer.join_response = Some(Box::new(response));
                }
            }
            Ok(())
        }
        .await;
        drop(results);
        let _ = ResultsDone::from_results_inner(
            receiver.await.map_err(crate::canceled_to_error),
            status,
            pipeline_sender,
        );
        Ok(())
    }
    .boxed_local()
    .shared();
    pipeline.drive(task.clone());
    let running = owner.eagerly_evaluate(task);
    if let Some(answer) = owner.answers.borrow_mut().slots.get_mut(&id) {
        answer.pipeline = Some(Box::new(pipeline));
        answer.call_completion_promise = Some(running);
    }
    Ok(())
}

pub(super) fn unimplemented<VatId>(
    state: &Rc<ConnectionState<VatId>>,
    id: QuestionId,
) -> capnp::Result<()> {
    let reference = {
        let mut questions = state.questions.borrow_mut();
        let question = questions
            .find(id)
            .ok_or_else(|| Error::failed("unknown Join question".into()))?;
        if !question.multiparty_join || !question.is_awaiting_return {
            return Err(Error::failed("unexpected Unimplemented Join".into()));
        }
        question.is_awaiting_return = false;
        question.skip_finish = true;
        let reference = question.self_ref.as_ref().and_then(Weak::upgrade);
        if reference.is_none() {
            questions.erase(id);
        }
        reference
    };
    if let Some(reference) = reference {
        reference.borrow_mut().reject(Error::unimplemented(
            "peer does not implement multiparty Join".into(),
        ));
    }
    Ok(())
}
