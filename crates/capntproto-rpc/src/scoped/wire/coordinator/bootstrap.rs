//! Bootstrap policy for a two-party stream. Factories run once per
//! admitted request; output backpressure only retries publication.
use super::*;

/// Two-party control handler with a caller-supplied bootstrap factory. Capture
/// the transport's authenticated peer identity in the factory when applying
/// peer-specific access policy (e.g. `BootstrapFactory::create_for`). Never take
/// that identity from a wire question ID. Factory failures become Return
/// exceptions and leave this connection usable.
///
/// This handler rejects multi-party network controls. Vat networks which support
/// introductions/Join must supply their own authenticated `Control` integration.
pub struct TwoParty<'id, F> {
    factory: F,
    pending: VecDeque<connection::Bootstrap<'id>>,
    maximum: NonZeroUsize,
    idle: Option<Box<dyn FnMut(bool) + 'id>>,
}
impl<'id, F: FnMut() -> capnp::Result<capability::Client>> TwoParty<'id, F> {
    pub fn new(factory: F, maximum: NonZeroUsize) -> Self {
        Self {
            factory,
            pending: VecDeque::new(),
            maximum,
            idle: None,
        }
    }
    pub fn with_idle_notifier(mut self, notify: impl FnMut(bool) + 'id) -> Self {
        self.idle = Some(Box::new(notify));
        self
    }
}
impl<'p, 'id, 's, I, P: Copy + Eq, const C: usize, const H: usize, F, O: Output<I>>
    Control<'p, 'id, 's, I, P, C, H, O> for TwoParty<'id, F>
where
    F: FnMut() -> capnp::Result<capability::Client>,
{
    fn observes_hooks(&self, event: &connection::Received<'id, server::Results>) -> bool {
        !matches!(
            event,
            connection::Received::Queued(_)
                | connection::Received::DeferredCall(_)
                | connection::Received::Returned(_)
        )
    }
    fn accepts_direct_plain_return(&self) -> bool {
        true
    }
    fn direct_publication_preserves_idle(&self) -> bool {
        self.pending.is_empty()
    }
    fn plain_return_preserves_idle(&self) -> bool {
        self.pending.is_empty()
    }
    fn set_idle(&mut self, idle: bool) {
        if let Some(notify) = &mut self.idle {
            notify(idle);
        }
    }
    fn received(
        &mut self,
        event: connection::Received<'id, server::Results>,
        coordinator: &mut Coordinator<'p, 'id, 's, I, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        _: &mut messages::Queue<'p, '_, O::Bundle>,
    ) -> capnp::Result<()> {
        let bootstrap = match event {
            connection::Received::Bootstrap(bootstrap) => bootstrap,
            connection::Received::Network | connection::Received::QueuedNetwork => {
                return Err(Error::unimplemented(
                    "two-party connection has no handoff network".into(),
                ));
            }
            _ => return Ok(()),
        };
        if self.pending.len() == self.maximum.get() {
            return Err(Error::overloaded(
                "pending bootstrap capacity exhausted".into(),
            ));
        }
        self.pending.try_reserve(1).map_err(wire_error)?;
        // No protocol-table borrow is held while invoking application policy.
        let value = (self.factory)();
        let driver = &mut coordinator.driver;
        let options = coordinator.options;
        match value {
            Ok(client) => {
                let client = match coordinator
                    .hooks
                    .try_retain(&mut driver.runtime, &*client.hook)?
                {
                    Some(client) => client,
                    None => coordinator
                        .foreign
                        .import(&mut driver.runtime, &*client.hook)?,
                };
                let result = driver.complete_bootstrap(
                    &bootstrap,
                    Ok(&client),
                    messages,
                    options.output_bytes.get(),
                    options.reader,
                );
                driver.runtime.release(client);
                result.map_err(wire_error)?;
            }
            Err(error) => {
                driver
                    .complete_bootstrap(
                        &bootstrap,
                        Err(error.kind),
                        messages,
                        options.output_bytes.get(),
                        options.reader,
                    )
                    .map_err(wire_error)?;
                // The producer is retained by the answer through Finish and all
                // pipelines, preserving the complete exception for publication.
                driver
                    .ledgers
                    .answers
                    .get(bootstrap.answer())
                    .map_err(wire_error)?
                    .owner
                    .producer
                    .fail(error);
            }
        }
        self.pending.push_back(bootstrap);
        Ok(())
    }

    fn poll(
        &mut self,
        coordinator: &mut Coordinator<'p, 'id, 's, I, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        scratch: &mut [u32],
        output: &O,
        _: &mut Context<'_>,
    ) -> Poll<capnp::Result<bool>> {
        let Some(bootstrap) = self.pending.front() else {
            return Poll::Pending;
        };
        let key = bootstrap.answer();
        let queued = !queue.is_empty();
        match queue.reserve() {
            Ok(slot) => drop(slot),
            Err(messages::Error::NoSendSlot) if queued => return Poll::Pending,
            Err(error) => return Poll::Ready(Err(wire_error(error))),
        }
        let outcome = (|| {
            let driver = &mut coordinator.driver;
            let producer = &driver
                .ledgers
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
                    |results, output| results.write_exception(output),
                )
                .map_err(wire_error)?;
            self.pending.pop_front();
            Ok(true)
        })();
        Poll::Ready(outcome)
    }

    fn shutdown(
        &mut self,
        _: &mut Coordinator<'p, 'id, 's, I, P, C, H>,
        _: &mut messages::Messages<'p, '_, '_>,
    ) {
        // All result frames and capability references remain in the driver's
        // ledger. Its terminal drain reclaims them after these tokens are gone.
        self.pending.clear();
    }
}
