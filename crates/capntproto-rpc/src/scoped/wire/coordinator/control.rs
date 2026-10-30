//! Effect-reporting network policy boundary. Controls never own the coordinator.
use super::*;

#[derive(Clone, Copy, Default)]
pub(super) enum Effects {
    #[default]
    None,
    Publication,
    Network,
}

/// Restricted control-poll operations. Protocol owners remain in the coordinator;
/// successful operations record their scheduling effects. No public operation
/// exposes the coordinator or allows unreported mutation of its protocol owners.
pub struct ControlContext<'a, 'p, 'id, 's, I, P: Copy + Eq, const C: usize, const H: usize> {
    coordinator: &'a mut Coordinator<'p, 'id, 's, I, P, C, H>,
    effects: Effects,
}
impl<'a, 'p, 'id, 's, I, P: Copy + Eq, const C: usize, const H: usize>
    ControlContext<'a, 'p, 'id, 's, I, P, C, H>
{
    pub(super) fn new(coordinator: &'a mut Coordinator<'p, 'id, 's, I, P, C, H>) -> Self {
        Self {
            coordinator,
            effects: Effects::None,
        }
    }
    pub(super) fn has_effects(&self) -> bool {
        !matches!(self.effects, Effects::None)
    }
    // Only the built-in authenticated vat integration can access protocol
    // owners. This records all affected lanes, including on Pending/error.
    pub(super) fn network(&mut self) -> &mut Coordinator<'p, 'id, 's, I, P, C, H> {
        self.effects = Effects::Network;
        self.coordinator
    }
    /// Reject unsupported request/response operations through the ordinary
    /// output queue. The driver owns the pending request across backpressure;
    /// ordinary two-party turns only test that one owner, not network registries.
    pub(super) fn allow_join(&mut self, enabled: bool) {
        self.coordinator.driver.allow_join(enabled);
    }

    pub(super) fn reject_network<O: Output<I>>(
        &mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        scratch: &mut [u32],
        output: &O,
    ) -> Poll<capnp::Result<bool>> {
        if !self.coordinator.driver.has_network() {
            return Poll::Pending;
        }
        match queue.ensure_capacity() {
            Ok(()) => (),
            Err(messages::Error::NoSendSlot) if !queue.is_empty() => return Poll::Pending,
            Err(error) => return Poll::Ready(Err(wire_error(error))),
        }
        self.effects = Effects::Network;
        let coordinator = &mut self.coordinator;
        let driver = &mut coordinator.driver;
        if matches!(
            driver.peek_network(),
            Some(connection::NetworkRequest::Unsupported(_))
        ) {
            return match driver.send_unimplemented(messages, queue, coordinator.options.reader) {
                Err(error)
                    if super::super::calls::output_capacity_error(&error) && !queue.is_empty() =>
                {
                    Poll::Pending
                }
                result => Poll::Ready(result.map(|()| true).map_err(wire_error)),
            };
        }
        let request = driver.take_network().expect("pending network request");
        use connection::NetworkRequest as N;
        let result = match request {
            N::Join(join) => driver
                .reject_join(
                    join,
                    capnp::ErrorKind::Unimplemented,
                    messages,
                    queue,
                    coordinator.options.output_bytes.get(),
                    coordinator.options.reader,
                )
                .map(|_| ())
                .map_err(|rejected| {
                    driver.release_network(N::Join(rejected.join), messages);
                    wire_error(rejected.error)
                }),
            N::Accept(accept) => {
                let key = accept.key();
                let outcome = (|| {
                    let producer = driver
                        .ledgers()
                        .answers
                        .get(key)
                        .map_err(wire_error)?
                        .owner
                        .producer
                        .clone();
                    producer.fail(Error::unimplemented(
                        "third-party acceptance unavailable on this route".into(),
                    ));
                    driver
                        .complete_authenticated_accept(
                            &accept,
                            Err(capnp::ErrorKind::Unimplemented),
                            messages,
                        )
                        .map_err(|rejected| {
                            if let Ok(result) = rejected.result {
                                result.release(messages, &mut driver.capabilities());
                            }
                            wire_error(rejected.error)
                        })?;
                    producer.prepare_trace(&coordinator.policy);
                    driver
                        .send_answer_using(
                            key,
                            messages,
                            queue,
                            producer.publication_bytes()?,
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
                outcome
            }
            request => {
                driver.release_network(request, messages);
                Err(Error::unimplemented(
                    "two-party connection has no handoff network".into(),
                ))
            }
        };
        Poll::Ready(result.map(|()| true))
    }

    /// Complete an admitted bootstrap without exposing the protocol tables.
    pub fn complete_bootstrap(
        &mut self,
        bootstrap: &connection::Bootstrap<'id>,
        value: capnp::Result<capability::Client>,
        messages: &mut messages::Messages<'p, '_, '_>,
    ) -> capnp::Result<()> {
        self.effects = Effects::Network;
        let coordinator = &mut self.coordinator;
        let driver = &mut coordinator.driver;
        let options = coordinator.options;
        match value {
            Ok(client) => {
                let client = match coordinator
                    .hooks
                    .try_retain(&mut driver.capabilities(), &*client.hook)?
                {
                    Some(client) => client,
                    None => coordinator
                        .foreign
                        .import(&mut driver.capabilities(), &*client.hook)?,
                };
                let result = driver.complete_bootstrap(
                    bootstrap,
                    Ok(&client),
                    messages,
                    options.output_bytes.get(),
                    options.reader,
                );
                driver.capabilities().release(client);
                result.map_err(wire_error)?;
            }
            Err(error) => {
                driver
                    .complete_bootstrap(
                        bootstrap,
                        Err(error.kind),
                        messages,
                        options.output_bytes.get(),
                        options.reader,
                    )
                    .map_err(wire_error)?;
                driver
                    .ledgers()
                    .answers
                    .get(bootstrap.answer())
                    .map_err(wire_error)?
                    .owner
                    .producer
                    .fail(error);
            }
        }
        Ok(())
    }
    // Read the producer before waking it: the callback may reenter public RPC.
    // This names an existing owner and changes no protocol or application state.
    pub(super) fn notify_finished(
        &self,
        question: capntproto_rpc_engine::answers::Key<'id>,
        retired: Option<server::Producer>,
    ) {
        let results = retired.map(|producer| producer.results()).or_else(|| {
            let answers = &self.coordinator.driver.ledgers().answers;
            Some(answers.get(question).ok()?.owner.producer.results())
        });
        if let Some(results) = results {
            results.protocol_changed();
        }
    }
    pub(super) fn network_idle(&self) -> bool {
        self.coordinator.driver.runtime().queued() == 0
            && !self.coordinator.driver.has_network()
            && self.coordinator.worker.pending_redirect().is_none()
    }
    pub(super) fn allow_answer_handoff(&mut self, allow: bool) {
        self.coordinator.driver.allow_answer_handoff(allow);
    }
    pub fn publish_bootstrap<O: Output<I>>(
        &mut self,
        bootstrap: &connection::Bootstrap<'id>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        scratch: &mut [u32],
        output: &O,
    ) -> Poll<capnp::Result<bool>> {
        let coordinator = &mut self.coordinator;
        let key = bootstrap.answer();
        let queued = !queue.is_empty();
        match queue.reserve() {
            Ok(slot) => drop(slot),
            Err(messages::Error::NoSendSlot) if queued => return Poll::Pending,
            Err(error) => return Poll::Ready(Err(wire_error(error))),
        }
        let result = (|| {
            let driver = &mut coordinator.driver;
            let producer = &driver
                .ledgers()
                .answers
                .get(key)
                .map_err(wire_error)?
                .owner
                .producer;
            let minimum = if producer.failure().is_some() {
                producer.prepare_trace(&coordinator.policy);
                producer.publication_bytes()?
            } else {
                128
            }
            .max(coordinator.options.output_bytes.get());
            driver
                .send_answer_using(
                    key,
                    messages,
                    queue,
                    minimum,
                    coordinator.options.reader,
                    scratch,
                    output.bundle(),
                    |owner, imports, bundle| output.attach(owner, imports, bundle),
                    |results, root| results.write_exception(root),
                )
                .map_err(wire_error)
        })();
        if result.is_ok() && !matches!(self.effects, Effects::Network) {
            self.effects = Effects::Publication;
        }
        Poll::Ready(result.map(|_| true))
    }
}

impl<I, P: Copy + Eq, const C: usize, const H: usize> Drop
    for ControlContext<'_, '_, '_, '_, I, P, C, H>
{
    fn drop(&mut self) {
        // Pending and error paths commit mutations too. A control cannot lose
        // scheduler invalidation by returning early after a protocol operation.
        let coordinator = &mut self.coordinator;
        match self.effects {
            Effects::Network => coordinator.changed(),
            Effects::Publication => {
                coordinator.schedule.active |= WorkSet::CALLS | WorkSet::MAINTENANCE;
                if coordinator.driver.runtime().queued() != 0
                    || coordinator.driver.has_pending_call()
                {
                    coordinator.schedule.active |= WorkSet::ACCEPT;
                }
                if !coordinator.worker.is_empty() {
                    coordinator.schedule.active |= WorkSet::WORKER;
                }
            }
            Effects::None => (),
        }
    }
}

/// Protocol activity whose effects on a control policy must be declared.
/// These are scheduling certificates, never permission to bypass wire checks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Activity {
    PlainPublication,
    /// Physical output freed storage without changing protocol ownership.
    OutputProgress,
    PlainReturn,
    PlainCall,
    Retirement,
}

/// Integration with a vat network. Every received event is delivered once.
/// Retain any unfinished bootstrap/network work in this handler and drive it in
/// `poll`, registering the supplied waker. Return true only after making progress.
/// Multi-party handoffs belong to the built-in authenticated vat integration;
/// policy callbacks have no direct access to protocol tables or graph authority.
/// `shutdown` must return/release every detached protocol owner before teardown.
/// Ordinary Call/Return/Resolve/Finish handling is performed by the coordinator.
pub trait Control<
    'p,
    'id,
    's,
    I,
    P: Copy + Eq,
    const C: usize,
    const H: usize,
    O: Output<I> = NoResources,
>
{
    /// Notify protocol quiescence after output flush. This is advisory transport
    /// policy, not a peer acknowledgement or permission to discard active refs.
    fn set_idle(&mut self, _idle: bool) {}
    /// Whether `received` observes hosted client metadata for this event.
    /// Returning false defers synchronization to application dispatch/delivery;
    /// the callback must not expose newly admitted hooks itself in that case.
    fn observes_hooks(&self, _event: &connection::Received<'id, server::Producer>) -> bool {
        true
    }
    /// True only when receiving a plain result cannot enable control work.
    /// Arbitrary policies retain the full rescan after every received event.
    /// Opt in only when received() ignores ordinary data-only Returns. Allows
    /// their original application owner to take completion during admission.
    /// Arbitrary controls keep the observable admitted-then-delivered lifecycle.
    fn accepts_direct_plain_return(&self) -> bool {
        false
    }
    /// Whether this activity preserves the policy's current idle certificate.
    /// Called at the transition, so policies may depend on pending network work.
    /// Independent commands and destructor callbacks must notify the waker from
    /// `poll`. Return false whenever the activity may enable control work.
    fn preserves_idle(&self, activity: Activity) -> bool;
    /// Opt in while owning external questions or observing authenticated handoffs.
    /// With no observers, the coordinator compiles away the completion callback.
    fn has_question_observers(&self) -> bool {
        false
    }
    /// An external question is ready, or a hosted call relinquished response
    /// interest. Validate the complete key against the local slot before waking
    /// it. This grants no authority; true invalidates policy idleness.
    fn completed(
        &mut self,
        _question: capntproto_rpc_engine::questions::Key<'id>,
        _owner: usize,
    ) -> bool {
        false
    }
    fn received(
        &mut self,
        event: connection::Received<'id, server::Producer>,
        context: &mut ControlContext<'_, 'p, 'id, 's, I, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
    ) -> capnp::Result<()>;
    fn poll(
        &mut self,
        context: &mut ControlContext<'_, 'p, 'id, 's, I, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        scratch: &mut [u32],
        output: &O,
        cx: &mut Context<'_>,
    ) -> Poll<capnp::Result<bool>>;
    fn shutdown(
        &mut self,
        context: &mut ControlContext<'_, 'p, 'id, 's, I, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
    );
}
