//! Third-party tail transfer uses private connection handles and owned service
//! work. No branded graph handle or numeric authority crosses driver scopes.
use super::*;
use capnp::private::capability::RequestHook;
use capntproto_rpc_engine::answers;

pub(crate) struct Target(Handle);
pub(crate) struct Redirect {
    pub recipient: Body,
    pub sent: Cell<bool>,
}
pub(crate) enum Destination {
    Remote(Reservation),
    Local(Box<dyn FnOnce(server::Transfer) -> capnp::Result<()>>),
}
pub(crate) struct Reservation {
    commands: Rc<Commands>,
    completion: Body,
    permit: capntproto_rpc_engine::flow_control::outgoing::OwnedPermit,
}
pub(super) struct Adoption {
    completion: Body,
    task: server::Transfer,
    _permit: capntproto_rpc_engine::flow_control::outgoing::OwnedPermit,
}
pub(super) struct Produced<'id> {
    key: answers::Key<'id>,
    task: server::Transfer,
    work_done: bool,
    result_revision: u64,
    collected: bool,
    _permit: capntproto_rpc_engine::flow_control::outgoing::OwnedPermit,
}
impl Handle {
    pub(crate) fn tail_target(&self) -> Option<Box<dyn std::any::Any>> {
        let commands = self.0.upgrade()?;
        if commands.closed.borrow().is_some()
            || !commands.network.borrow().as_ref()?.supports_answers()
        {
            return None;
        }
        Some(Box::new(Target(self.clone())))
    }
    pub(crate) fn prepare_adoption(
        &self,
        completion: any_pointer::Reader<'_>,
    ) -> capnp::Result<Destination> {
        let commands = self.0.upgrade().ok_or_else(closed)?;
        if let Some(error) = commands.closed.borrow().clone() {
            return Err(error);
        }
        if !commands
            .network
            .borrow()
            .as_ref()
            .is_some_and(|network| network.supports_answers())
        {
            return Err(Error::unimplemented(
                "destination cannot adopt answers".into(),
            ));
        }
        let completion = metadata(completion, commands.bytes)?;
        let permit = commands
            .capacity
            .reserve_owned()
            .map_err(|_| Error::overloaded("tail transfer capacity exhausted".into()))?;
        commands
            .adoptions
            .borrow_mut()
            .try_reserve(1)
            .map_err(wire_error)?;
        Ok(Destination::Remote(Reservation {
            commands,
            completion,
            permit,
        }))
    }
    pub(crate) fn redirect(
        &self,
        request: &mut dyn RequestHook,
    ) -> capnp::Result<Option<Rc<Redirect>>> {
        let Some(target) = request
            .third_party_tail_target()
            .and_then(|target| target.downcast::<Target>().ok())
        else {
            return Ok(None);
        };
        let origin = self.0.upgrade().ok_or_else(closed)?;
        let destination = target.0 .0.upgrade().ok_or_else(closed)?;
        if Rc::ptr_eq(&origin, &destination) {
            return Ok(None);
        }
        let Some(caller) = origin.network.borrow().clone() else {
            return Ok(None);
        };
        let Some(callee) = destination.network.borrow().clone() else {
            return Ok(None);
        };
        if caller.domain() != callee.domain()
            || !caller.supports_answers()
            || !callee.supports_answers()
        {
            return Ok(None);
        }
        // Introduce the original caller to the tail callee, through the exact
        // authenticated connections that admitted the call and request.
        let Some((contact, recipient)) = caller.introduce(callee.peer())? else {
            return Ok(None);
        };
        metadata(contact.get_root_as_reader()?, destination.bytes)?;
        metadata(recipient.get_root_as_reader()?, origin.bytes)?;
        request.set_third_party_tail_target(contact.get_root_as_reader()?)?;
        Ok(Some(Rc::new(Redirect {
            recipient,
            sent: Cell::new(false),
        })))
    }
}
fn metadata(reader: any_pointer::Reader<'_>, bytes: usize) -> capnp::Result<Body> {
    reader.validate_capability_indices(0)?;
    if minimum(reader)? > bytes {
        return Err(Error::overloaded(
            "tail metadata exceeds frame budget".into(),
        ));
    }
    let mut body = Body::new_default();
    body.set_root(reader)?;
    Ok(body)
}
impl Destination {
    pub(crate) fn start(self, task: server::Transfer) -> capnp::Result<()> {
        match self {
            Self::Remote(reservation) => {
                if let Some(error) = reservation.commands.closed.borrow().clone() {
                    return Err(error);
                }
                reservation
                    .commands
                    .adoptions
                    .borrow_mut()
                    .push_back(Adoption {
                        completion: reservation.completion,
                        task,
                        _permit: reservation.permit,
                    });
                reservation.commands.notify();
                Ok(())
            }
            Self::Local(start) => start(task),
        }
    }
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
    pub(super) fn prepare_transfer(
        &mut self,
        coordinator: &mut Coordinator<'p, 'id, '_, resources::Import, (), C, H>,
        messages: &messages::Messages<'p, '_, '_>,
    ) -> capnp::Result<bool> {
        let (mut dispatch, answers) = coordinator.driver.execution();
        let Some(ready) = dispatch.ready().map_err(wire_error)? else {
            return Ok(false);
        };
        let Some(call) = ready.owner().as_call() else {
            return Ok(false);
        };
        if !answers.is_third_party(call.answer()).map_err(wire_error)? {
            return Ok(false);
        }
        let results = &answers
            .get(call.answer())
            .map_err(wire_error)?
            .owner
            .producer;
        if results.transfer_ready() {
            return Ok(false);
        }
        let permit = self
            .commands
            .capacity
            .reserve_owned()
            .map_err(|_| Error::overloaded("incoming tail capacity exhausted".into()))?;
        let network = self.commands.network.borrow().clone().ok_or_else(closed)?;
        if !network.supports_answers() {
            return Err(Error::unimplemented(
                "network does not support third-party answers".into(),
            ));
        }
        let destination = call
            .with(messages, |call| {
                let contact = call
                    .incoming()
                    .third_party_destination()?
                    .ok_or_else(|| Error::failed("missing third-party destination".into()))?;
                metadata(contact, self.commands.bytes)?;
                network.tail_destination(contact)
            })
            .map_err(wire_error)??;
        results.prepare_transfer(destination, permit);
        Ok(true)
    }
    pub(super) fn poll_transfers(
        &mut self,
        coordinator: &mut Coordinator<'p, 'id, '_, resources::Import, (), C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, vat::Bundle>,
        scratch: &mut [u32],
        output: &vat::Resources,
        cx: &mut Context<'_>,
    ) -> Poll<capnp::Result<bool>> {
        // Publishing new transfers must not monopolize the output queue while
        // adopted work is waiting to run or return its result.
        for _ in 0..3 {
            let lane = self.tail_lane;
            self.tail_lane = (lane + 1) % 3;
            let progress = match lane {
                0 => self.poll_redirect(coordinator, messages, queue),
                1 => self.poll_adoption(coordinator, messages, queue),
                _ => self.poll_produced(coordinator, messages, queue, scratch, output, cx),
            };
            if progress.is_ready() {
                return progress;
            }
        }
        Poll::Pending
    }
    fn poll_redirect(
        &mut self,
        coordinator: &mut Coordinator<'p, 'id, '_, resources::Import, (), C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, vat::Bundle>,
    ) -> Poll<capnp::Result<bool>> {
        if let Some((key, results, redirect)) = coordinator.worker.pending_redirect() {
            let recipient = redirect
                .recipient
                .get_root_as_reader::<any_pointer::Reader>()?;
            coordinator
                .driver
                .send_network_answer(
                    key,
                    messages,
                    queue,
                    minimum(recipient)?,
                    coordinator.options.reader,
                    |builder, id| {
                        capntproto_rpc_wire::network::await_from_third_party(builder, id, |field| {
                            field.copy_from(recipient)
                        })
                    },
                )
                .map_err(wire_error)?;
            coordinator.worker.redirect_published(key);
            redirect.sent.set(true);
            results.protocol_changed();
            results.returned();
            return Poll::Ready(Ok(true));
        }
        Poll::Pending
    }
    fn poll_adoption(
        &mut self,
        coordinator: &mut Coordinator<'p, 'id, '_, resources::Import, (), C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, vat::Bundle>,
    ) -> Poll<capnp::Result<bool>> {
        if self.produced.len() < self.maximum {
            let job = self.commands.adoptions.borrow_mut().pop_front();
            if let Some(job) = job {
                self.produced.try_reserve(1).map_err(wire_error)?;
                let completion = job.completion.get_root_as_reader::<any_pointer::Reader>()?;
                match coordinator.driver.send_adopted_answer(
                    messages,
                    queue,
                    minimum(completion)?,
                    coordinator.options.reader,
                    job.task.results.clone().into_producer(),
                    |field| field.copy_from(completion),
                ) {
                    Ok(key) => self.produced.push_back(Produced {
                        key,
                        task: job.task,
                        work_done: false,
                        result_revision: 0,
                        collected: false,
                        _permit: job._permit,
                    }),
                    Err(rejected) => {
                        self.commands.adoptions.borrow_mut().push_front(job);
                        return Poll::Ready(Err(wire_error(rejected.error)));
                    }
                }
                return Poll::Ready(Ok(true));
            }
        }
        Poll::Pending
    }
    fn poll_produced(
        &mut self,
        coordinator: &mut Coordinator<'p, 'id, '_, resources::Import, (), C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, vat::Bundle>,
        scratch: &mut [u32],
        output: &vat::Resources,
        cx: &mut Context<'_>,
    ) -> Poll<capnp::Result<bool>> {
        for _ in 0..self.produced.len() {
            let mut job = self.produced.pop_front().expect("retained adoption");
            if !job.collected {
                let canceled = coordinator
                    .driver
                    .ledgers()
                    .answers
                    .cancellation_requested(job.key)
                    .map_err(wire_error)?;
                let result = if canceled {
                    Err(capnp::ErrorKind::Disconnected)
                } else {
                    let _ = job.task.results.poll_completed(cx);
                    if !job.work_done {
                        job.work_done = Pin::new(&mut job.task.work).poll(cx).is_ready();
                    }
                    let revision = job.task.results.revision();
                    let changed = job.result_revision != revision;
                    job.result_revision = revision;
                    if !job.work_done || job.task.results.poll_completed(cx).is_pending() {
                        // The original worker observes a different answer on
                        // another connection. Publish early pipeline changes to
                        // this adopted answer too, even while work is pending.
                        if changed {
                            coordinator
                                .driver
                                .answers()
                                .notify_pipeline(job.key)
                                .map_err(wire_error)?;
                        }
                        self.produced.push_back(job);
                        if changed {
                            return Poll::Ready(Ok(true));
                        }
                        continue;
                    }
                    coordinator.hooks.collect_server_results(
                        &job.task.results,
                        &mut coordinator.driver,
                        messages,
                        coordinator.options.reader,
                        |runtime, hook| coordinator.foreign.import(runtime, hook),
                    )?
                };
                if let Err(rejected) = coordinator
                    .driver
                    .complete_adopted_answer(job.key, result, messages)
                {
                    if let Ok(result) = rejected.result {
                        result.release(messages, &mut coordinator.driver.capabilities());
                    }
                    return Poll::Ready(Err(wire_error(rejected.error)));
                }
                job.collected = true;
            }
            job.task.results.prepare_trace(&coordinator.policy);
            let bytes = if coordinator
                .driver
                .prepare_answer_return(job.key)
                .map_err(wire_error)?
                .is_cancelled()
            {
                128
            } else {
                job.task.results.publication_bytes()?
            };
            coordinator
                .driver
                .send_answer_using(
                    job.key,
                    messages,
                    queue,
                    bytes.max(128),
                    coordinator.options.reader,
                    scratch,
                    output.bundle(),
                    |owner, imports, bundle| output.attach(owner, imports, bundle),
                    server::Producer::write_exception,
                )
                .map_err(wire_error)?;
            return Poll::Ready(Ok(true));
        }
        Poll::Pending
    }
}
