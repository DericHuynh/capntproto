//! Network policy boundary; conservative defaults support custom integrations.
use super::*;

/// Protocol activity whose effects on a control policy must be declared.
/// These are scheduling certificates, never permission to bypass wire checks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Activity {
    PlainPublication,
    PlainReturn,
    PlainCall,
    Retirement,
}

/// Integration with a vat network. Every received event is delivered once.
/// Retain any unfinished bootstrap/network work in this handler and drive it in
/// `poll`, registering the supplied waker. Return true only after making progress.
/// Raw wire IDs never authenticate a handoff; use the driver's authenticated APIs.
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
    fn observes_hooks(&self, _event: &connection::Received<'id, server::Results>) -> bool {
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
    ///
    /// The default delegates to the original per-activity methods for source
    /// compatibility. New integrations should override this single contract.
    fn preserves_idle(&self, activity: Activity) -> bool {
        match activity {
            Activity::PlainPublication => self.direct_publication_preserves_idle(),
            Activity::PlainReturn => self.plain_return_preserves_idle(),
            Activity::PlainCall => self.plain_call_preserves_idle(),
            Activity::Retirement => self.retirement_preserves_idle(),
        }
    }
    /// True only when an ordinary directly published Call cannot enable this
    /// idle policy, including by releasing FIFO capacity. Independent commands
    /// must still notify the registered control waker.
    fn direct_publication_preserves_idle(&self) -> bool {
        false
    }
    fn plain_return_preserves_idle(&self) -> bool {
        false
    }
    /// Admission of an ordinary descriptor-free Call to a hosted target does
    /// not enable this idle control policy. Independent commands must notify
    /// its registered waker. Custom policies retain the general rescan.
    fn plain_call_preserves_idle(&self) -> bool {
        false
    }
    /// Finish/Release only retire existing owners; they cannot enable this
    /// idle policy. Independent commands and destructor callbacks must notify
    /// its registered waker. Custom policies retain the full rescan.
    fn retirement_preserves_idle(&self) -> bool {
        false
    }
    fn received(
        &mut self,
        event: connection::Received<'id, server::Results>,
        coordinator: &mut Coordinator<'p, 'id, 's, I, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
    ) -> capnp::Result<()>;
    fn poll(
        &mut self,
        coordinator: &mut Coordinator<'p, 'id, 's, I, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
        queue: &mut messages::Queue<'p, '_, O::Bundle>,
        scratch: &mut [u32],
        output: &O,
        cx: &mut Context<'_>,
    ) -> Poll<capnp::Result<bool>>;
    fn shutdown(
        &mut self,
        coordinator: &mut Coordinator<'p, 'id, 's, I, P, C, H>,
        messages: &mut messages::Messages<'p, '_, '_>,
    );
}
