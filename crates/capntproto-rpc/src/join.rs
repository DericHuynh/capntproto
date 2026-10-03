//! Standard Join at a bilateral network boundary (rpc-twoparty.capnp).
//! Multiparty share authentication is deliberately a different network contract.
use super::*;
use crate::rpc_twoparty_capnp::{join_key_part, join_result};
use capnp::private::capability::JoinedCapability as Outcome;
use std::collections::{BTreeMap, HashSet};

/// Capability equality through local resolution and bilateral RPC boundaries.
///
/// `Some(cap)` proves equality and retains the joined capability's authority.
/// `None` means distinct settled local objects. Unsupported routes, broken
/// promises, malformed results and disconnects are errors, never inequality.
/// Multiparty networks may supply an authenticated independent-share profile.
pub struct Joiner<VatId: 'static> {
    registry: Weak<RefCell<ConnectionRegistry<VatId>>>,
}
impl<VatId> Clone for Joiner<VatId> {
    fn clone(&self) -> Self {
        Self {
            registry: self.registry.clone(),
        }
    }
}
impl<VatId> Joiner<VatId> {
    pub(crate) fn new(registry: Weak<RefCell<ConnectionRegistry<VatId>>>) -> Self {
        Self { registry }
    }
    /// Resolve 1..=65535 capabilities and join them. Dropping this promise
    /// cancels outstanding questions. The handle does not keep its system alive.
    pub fn join<T: capnp::capability::FromClientHook + 'static>(
        &self,
        caps: Vec<T>,
    ) -> Promise<Option<T>, Error> {
        let this = self.clone();
        Promise::from_future(async move {
            let result = this
                .resolve(caps.into_iter().map(T::into_client_hook).collect(), None, 0)
                .await?;
            Ok(result.cap.as_ref().map(|c| T::new(c.clone())))
        })
    }
    fn resolve(
        &self,
        caps: Vec<Box<dyn ClientHook>>,
        incoming: Option<usize>,
        depth: u8,
    ) -> Promise<Outcome, Error> {
        let registry = self.registry.clone();
        let this = self.clone();
        Promise::from_future(async move {
            if depth > 64 {
                return Err(Error::failed(
                    "cyclic or excessively deep Join delegation".into(),
                ));
            }
            if caps.is_empty() || caps.len() > u16::MAX as usize {
                return Err(Error::failed("Join needs 1..=65535 capabilities".into()));
            }
            let active = registry
                .upgrade()
                .ok_or_else(|| Error::disconnected("RPC system was dropped".into()))?;
            if active.borrow().closing {
                return Err(Error::disconnected("RPC system is closing".into()));
            }
            drop(active);
            let caps = future::try_join_all(caps.into_iter().map(settle)).await?;
            let active = registry
                .upgrade()
                .ok_or_else(|| Error::disconnected("RPC system was dropped".into()))?;
            if active.borrow().closing {
                return Err(Error::disconnected("RPC system is closing".into()));
            }
            drop(active);
            let first = &caps[0];
            if caps.iter().all(|c| identity(&**c) == identity(&**first)) {
                return Ok(Outcome {
                    cap: Some(first.clone()),
                    guard: Box::new(()),
                });
            }
            // Give opaque boundaries the complete batch before network routing.
            // Inspect every input so reversing mixed inputs cannot bypass a policy.
            for cap in &caps {
                if let Some(delegation) = cap.delegate_join(&caps) {
                    let delegation = delegation?;
                    let operation = this.resolve(delegation.caps, incoming, depth + 1);
                    return (delegation.complete)(operation).await;
                }
            }
            if caps.iter().all(|c| c.get_brand() == 0) {
                return Ok(Outcome {
                    cap: None,
                    guard: Box::new(()),
                });
            }
            let network = registry
                .upgrade()
                .and_then(|r| r.borrow().join_network.clone());
            if let Some(network) = network {
                let context = registry
                    .upgrade()
                    .and_then(|r| r.borrow().join_context.upgrade())
                    .ok_or_else(|| Error::disconnected("RPC system ended".into()))?;
                let bootstrap = context.bootstrap.clone();
                let tasks = context.tasks.clone();
                drop(context);
                return multiparty_join::start(registry, bootstrap, tasks, network, caps).await;
            }
            if !caps.iter().all(|c| c.get_brand() == first.get_brand())
                || incoming == Some(first.get_brand())
            {
                return Err(unsupported());
            }
            let route = first.clone();
            route.join_capabilities(caps).await
        })
    }
}
fn unsupported() -> Error {
    Error::unimplemented(
        "Join requires a common supported bilateral route or settled local identities".into(),
    )
}
fn identity(c: &dyn ClientHook) -> (usize, usize) {
    (c.get_brand(), c.get_ptr())
}
pub(super) async fn settle(mut cap: Box<dyn ClientHook>) -> capnp::Result<Box<dyn ClientHook>> {
    // Keep visited owners alive so allocator address reuse cannot look cyclic.
    let mut seen = HashSet::new();
    let mut owners = Vec::new();
    loop {
        if !seen.insert(identity(&*cap)) || owners.len() >= 1024 {
            return Err(Error::failed(
                "cyclic or excessively deep Join resolution".into(),
            ));
        }
        let Some(next) = cap.when_more_resolved() else {
            return Ok(cap);
        };
        owners.push(cap);
        cap = next.await?;
    }
}
pub(super) fn supported<VatId>(state: &ConnectionState<VatId>) -> bool {
    state
        .connection
        .borrow()
        .as_ref()
        .is_ok_and(|c| c.supports_two_party_join())
}

// Relay responses send Finish before the ID is freed, including across systems.
struct Retention<VatId: 'static> {
    responses: Vec<Response<VatId>>,
    lease: Option<IdLease<VatId>>,
}
impl<VatId> Drop for Retention<VatId> {
    fn drop(&mut self) {
        self.responses.clear();
        self.lease.take();
    }
}
pub(super) struct Table<VatId: 'static> {
    incoming: HashMap<u32, Weak<Incoming<VatId>>>,
    outgoing: HashSet<u32>,
    next: u32,
}
impl<VatId> Default for Table<VatId> {
    fn default() -> Self {
        Self {
            incoming: HashMap::new(),
            outgoing: HashSet::new(),
            next: 0,
        }
    }
}
struct IdLease<VatId: 'static> {
    state: Weak<ConnectionState<VatId>>,
    id: u32,
}
impl<VatId> Drop for IdLease<VatId> {
    fn drop(&mut self) {
        if let Some(s) = self.state.upgrade() {
            s.joins.borrow_mut().outgoing.remove(&self.id);
        }
    }
}
fn reserve<VatId>(state: &Rc<ConnectionState<VatId>>) -> capnp::Result<IdLease<VatId>> {
    let mut table = state.joins.borrow_mut();
    if table.outgoing.len() >= u32::MAX as usize {
        return Err(Error::overloaded("Join ID space exhausted".into()));
    }
    loop {
        let id = table.next;
        table.next = table.next.wrapping_add(1);
        if table.outgoing.insert(id) {
            return Ok(IdLease {
                state: Rc::downgrade(state),
                id,
            });
        }
    }
}
pub(super) fn send<VatId>(
    state: &Rc<ConnectionState<VatId>>,
    caps: Vec<Box<dyn ClientHook>>,
) -> Promise<Outcome, Error> {
    let state = state.clone();
    Promise::from_future(async move {
        if caps.is_empty()
            || caps.len() > u16::MAX as usize
            || !supported(&state)
            || !caps.iter().all(|c| c.get_brand() == state.get_brand())
        {
            return Err(unsupported());
        }
        state.set_not_idle();
        // Allocate all bodies/targets before publishing any question.
        let lease = reserve(&state)?;
        let mut messages = Vec::new();
        for (part, cap) in caps.iter().enumerate() {
            let mut message = state.new_outgoing_message(16)?;
            let mut request = message
                .get_body()?
                .init_as::<message::Builder>()
                .init_join();
            if state
                .write_target(&**cap, request.reborrow().init_target())
                .is_some()
            {
                return Err(unsupported());
            }
            let mut key = request.get_key_part().init_as::<join_key_part::Builder>();
            key.set_join_id(lease.id);
            key.set_part_count(caps.len() as u16);
            key.set_part_num(part as u16);
            messages.push(message);
        }
        let mut replies = Vec::new();
        for mut message in messages {
            let message::Join(request) =
                message.get_body()?.get_as::<message::Builder>()?.which()?
            else {
                unreachable!()
            };
            let mut request = request?;
            let mut question = Question::new();
            question.join_id = Some(lease.id);
            let id = state.questions.borrow_mut().push(question);
            request.set_question_id(id.to_wire());
            let (tx, rx) = oneshot::channel();
            let reference = Rc::new(RefCell::new(QuestionRef::new(state.clone(), id, tx)));
            state.questions.borrow_mut().find(id).unwrap().self_ref =
                Some(Rc::downgrade(&reference));
            let promise = rx
                .map_err(crate::canceled_to_error)
                .and_then(|response| response)
                .attach(reference);
            replies.push(Promise::from_future(promise));
            let _ = message.send();
        }
        let responses = future::try_join_all(replies).await?;
        let mut succeeded = None;
        let mut cap = None;
        for response in &responses {
            let result = response.get()?.get_as::<join_result::Reader>()?;
            if result.get_join_id() != lease.id
                || succeeded.is_some_and(|s| s != result.get_succeeded())
            {
                return Err(Error::failed("inconsistent Join results".into()));
            }
            succeeded = Some(result.get_succeeded());
            if result.has_cap() {
                if !result.get_succeeded() || cap.is_some() {
                    return Err(Error::failed(
                        "Join returned unexpected capabilities".into(),
                    ));
                }
                cap = Some(result.get_cap().get_as_capability()?);
            }
        }
        if succeeded == Some(true) && cap.is_none() {
            return Err(Error::failed("successful Join omitted capability".into()));
        }
        drop(caps);
        Ok(Outcome {
            cap,
            guard: Box::new(Retention {
                responses,
                lease: Some(lease),
            }),
        })
    })
}

// Typed senders avoid exposing transport-specific response guards through ClientHook.
type Sender = oneshot::Sender<capnp::Result<Rc<Outcome>>>;
struct State {
    seen: HashSet<u16>,
    pending: BTreeMap<u16, (Box<dyn ClientHook>, Sender)>,
    waiting: Vec<Sender>,
    outcome: Option<capnp::Result<Rc<Outcome>>>,
    running: Option<Promise<(), Error>>,
    live: usize,
}
struct Incoming<VatId: 'static> {
    owner: Weak<ConnectionState<VatId>>,
    id: u32,
    count: u16,
    state: RefCell<State>,
}
pub(super) struct PartGuard<VatId: 'static> {
    group: Rc<Incoming<VatId>>,
}
impl<VatId> Drop for PartGuard<VatId> {
    fn drop(&mut self) {
        let last = {
            let mut s = self.group.state.borrow_mut();
            s.live -= 1;
            s.live == 0
        };
        self.group
            .publish(Err(Error::failed("Join canceled before completion".into())));
        if last {
            if let Some(owner) = self.group.owner.upgrade() {
                owner.joins.borrow_mut().incoming.remove(&self.group.id);
            }
        }
    }
}
impl<VatId> Incoming<VatId> {
    fn publish(&self, result: capnp::Result<Rc<Outcome>>) {
        let (pending, waiting, running) = {
            let mut s = self.state.borrow_mut();
            if s.outcome.is_some() {
                return;
            }
            s.outcome = Some(result.clone());
            (
                mem::take(&mut s.pending),
                mem::take(&mut s.waiting),
                s.running.take(),
            )
        };
        drop(running);
        for (_, (_, sender)) in pending {
            let _ = sender.send(result.clone());
        }
        for sender in waiting {
            let _ = sender.send(result.clone());
        }
    }
}
pub(super) fn receive<VatId>(
    owner: &Rc<ConnectionState<VatId>>,
    request: crate::rpc_capnp::join::Reader<'_>,
) -> capnp::Result<()> {
    let id = AnswerId::from_wire(request.get_question_id());
    if id.to_wire() >= 1 << 30 || owner.answers.borrow().slots.contains_key(&id) {
        return Err(Error::failed("invalid or in-use Join question ID".into()));
    }
    let key = request.get_key_part().get_as::<join_key_part::Reader>()?;
    let (join_id, count, part) = (key.get_join_id(), key.get_part_count(), key.get_part_num());
    if count == 0 || part >= count {
        return Err(Error::failed("invalid Join part number/count".into()));
    }
    let target = owner.get_message_target(request.get_target()?)?;
    let existing = owner
        .joins
        .borrow()
        .incoming
        .get(&join_id)
        .and_then(Weak::upgrade);
    let group = existing.unwrap_or_else(|| {
        let g = Rc::new(Incoming {
            owner: Rc::downgrade(owner),
            id: join_id,
            count,
            state: RefCell::new(State {
                seen: HashSet::new(),
                pending: BTreeMap::new(),
                waiting: Vec::new(),
                outcome: None,
                running: None,
                live: 0,
            }),
        });
        owner
            .joins
            .borrow_mut()
            .incoming
            .insert(join_id, Rc::downgrade(&g));
        g
    });
    let (tx, rx) = oneshot::channel();
    let ready = {
        let mut s = group.state.borrow_mut();
        if count != group.count || !s.seen.insert(part) {
            return Err(Error::failed("duplicate or inconsistent Join part".into()));
        }
        s.live += 1;
        if let Some(result) = &s.outcome {
            let _ = tx.send(result.clone());
            false
        } else {
            s.pending.insert(part, (target, tx));
            s.seen.len() == count as usize
        }
    };
    let mut answer = Answer::new();
    answer.join = Some(PartGuard {
        group: group.clone(),
    });
    let (sender, receiver) = oneshot::channel();
    let (pipeline_sender, mut pipeline) = queued::Pipeline::new();
    let mut results = Results::new(
        owner,
        id,
        false,
        sender,
        answer.received_finish.clone(),
        answer.return_has_been_sent.clone(),
        Some(pipeline_sender.weak_clone()),
    );
    owner.answers.borrow_mut().slots.insert(id, answer);
    let task = async move {
        let status = async {
            let outcome = rx.await.map_err(crate::canceled_to_error)??;
            let mut result = results.get()?.init_as::<join_result::Builder>();
            result.set_join_id(join_id);
            result.set_succeeded(outcome.cap.is_some());
            if part == 0 {
                if let Some(cap) = &outcome.cap {
                    result.get_cap().set_as_capability(cap.clone());
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
    {
        let mut answers = owner.answers.borrow_mut();
        let a = answers.slots.get_mut(&id).unwrap();
        a.pipeline = Some(Box::new(pipeline));
        a.call_completion_promise = Some(running);
    }
    if ready {
        let pending = mem::take(&mut group.state.borrow_mut().pending);
        let (targets, waiting): (Vec<_>, Vec<_>) = pending.into_values().unzip();
        group.state.borrow_mut().waiting = waiting;
        let operation =
            Joiner::new(owner.registry.clone()).resolve(targets, Some(owner.get_brand()), 0);
        let weak = Rc::downgrade(&group);
        let running = owner.eagerly_evaluate(
            async move {
                let result = operation.await.map(Rc::new);
                if let Some(group) = weak.upgrade() {
                    group.publish(result);
                }
                Ok(())
            }
            .boxed_local(),
        );
        group.state.borrow_mut().running = Some(running);
    }
    Ok(())
}
pub(super) fn unimplemented<VatId>(
    state: &Rc<ConnectionState<VatId>>,
    request: crate::rpc_capnp::join::Reader<'_>,
) -> capnp::Result<()> {
    let id = QuestionId::from_wire(request.get_question_id());
    if state
        .questions
        .borrow_mut()
        .find(id)
        .is_some_and(|q| q.multiparty_join)
    {
        return multiparty_join::unimplemented(state, id);
    }
    let join_id = request
        .get_key_part()
        .get_as::<join_key_part::Reader>()?
        .get_join_id();
    let reference = {
        let mut questions = state.questions.borrow_mut();
        let q = questions
            .find(id)
            .ok_or_else(|| Error::failed("Unimplemented Join has unknown question ID".into()))?;
        if q.join_id != Some(join_id) || !q.is_awaiting_return {
            return Err(Error::failed("unexpected Unimplemented Join".into()));
        }
        q.is_awaiting_return = false;
        q.skip_finish = true;
        let reference = q.self_ref.as_ref().and_then(Weak::upgrade);
        if reference.is_none() {
            questions.erase(id);
        }
        reference
    };
    if let Some(reference) = reference {
        reference
            .borrow_mut()
            .reject(Error::unimplemented("peer does not implement Join".into()));
    }
    Ok(())
}
