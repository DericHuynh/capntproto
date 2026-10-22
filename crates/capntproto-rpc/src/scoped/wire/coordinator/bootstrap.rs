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
    // NetworkControl composes bootstrap publication without inheriting the
    // two-party handler's rejection policy for authenticated network requests.
    pub(super) fn poll_bootstrap<
        'p,
        's,
        I,
        P: Copy + Eq,
        const C: usize,
        const H: usize,
        O: Output<I>,
    >(
        &mut self,
        context: &mut ControlContext<'_, 'p, 'id, 's, I, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        scratch: &mut [u32],
        output: &O,
    ) -> Poll<capnp::Result<bool>> {
        let Some(bootstrap) = self.pending.front() else {
            return Poll::Pending;
        };
        let result = context.publish_bootstrap(bootstrap, messages, queue, scratch, output);
        if matches!(result, Poll::Ready(Ok(true))) {
            self.pending.pop_front();
        }
        result
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
    fn poll(
        &mut self,
        context: &mut ControlContext<'_, 'p, 'id, 's, I, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        scratch: &mut [u32],
        output: &O,
        _: &mut Context<'_>,
    ) -> Poll<capnp::Result<bool>> {
        if self.pending.is_empty() {
            context.reject_network(messages, queue, scratch, output)
        } else {
            self.poll_bootstrap(context, messages, queue, scratch, output)
        }
    }
    fn observes_hooks(&self, event: &connection::Received<'id, server::Results>) -> bool {
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
    fn preserves_idle(&self, _activity: Activity) -> bool {
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
        context: &mut ControlContext<'_, 'p, 'id, 's, I, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        _: &mut messages::Queue<'p, '_, O::Bundle>,
    ) -> capnp::Result<()> {
        let bootstrap = match event {
            connection::Received::Bootstrap(bootstrap) => bootstrap,
            connection::Received::Network | connection::Received::QueuedNetwork => {
                // The driver's pending request retains its authority until the
                // output lane can publish the unsupported-operation response.
                return Ok(());
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
        context.complete_bootstrap(&bootstrap, value, messages)?;
        self.pending.push_back(bootstrap);
        Ok(())
    }

    fn shutdown(
        &mut self,
        _: &mut ControlContext<'_, 'p, 'id, 's, I, P, C, H>,
        _: &mut messages::Messages<'p, '_, '_>,
    ) {
        // All result frames and capability references remain in the driver's
        // ledger. Its terminal drain reclaims them after these tokens are gone.
        self.pending.clear();
    }
}
