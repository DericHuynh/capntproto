//! Last local interest: retain the wire ID until Finish publication is accepted.
use super::{Finish, Release};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Publication {
    Accepted,
    /// The provider retains the Finish and any eventual reclamation obligation
    /// in existing question/control storage. Dropping it is not acceptance.
    Deferred,
}

/// Storage and transport effects of releasing the last local question interest.
/// Keys must identify the current generation. Every callback runs after the
/// preceding callback returns, so providers can release table borrows before
/// invoking a transport, waking a task or destroying an owner.
pub trait Owner {
    type Key;
    /// Detach last local interest. Providers that can receive stale or duplicate
    /// keys return None for them; a hosted owner may enforce validity internally.
    fn release(&self, key: &Self::Key) -> Option<Release>;
    /// Accept publication, or retain the complete effect and reclamation flag.
    /// The question must remain occupied while publication is deferred.
    fn finish(&self, key: &Self::Key, finish: Finish, reclaim: bool) -> Publication;
    /// Recheck the current generation: transport callbacks may have disconnected
    /// or retired it. Never reclaim a replacement or an unpublished Finish.
    fn reclaim(&self, key: &Self::Key);
    fn idle(&self);
}

/// The same ordering for hosted connection state and caller-backed contexts.
/// Deferred providers resume their retained effect from their ordinary output
/// driver; this function never polls a transport or spins for capacity.
#[inline(always)]
pub fn run<O: Owner + ?Sized>(owner: &O, key: O::Key) {
    let Some(release) = owner.release(&key) else {
        return;
    };
    publish(owner, &key, release);
}

/// Resume a retained Finish after output capacity becomes available. The
/// provider validates the same generation and consumes its pending effect only
/// on acceptance. Internal storage retains keys/effects, never its own public
/// borrowing Last finalizer.
#[inline(always)]
pub fn resume<O: Owner + ?Sized>(owner: &O, key: &O::Key, finish: Finish, reclaim: bool) {
    publish(
        owner,
        key,
        Release {
            finish: Some(finish),
            reclaim,
        },
    );
}
#[inline(always)]
fn publish<O: Owner + ?Sized>(owner: &O, key: &O::Key, release: Release) {
    let accepted = release
        .finish
        .is_none_or(|finish| owner.finish(key, finish, release.reclaim) == Publication::Accepted);
    if release.reclaim && accepted {
        owner.reclaim(key);
    }
    owner.idle();
}

/// A last-interest finalizer borrowing stable caller-owned context. The owner
/// validates generation and once-only release; this does not create a reference
/// count, authorize a second producer or manufacture a key. A key need not copy.
///
/// ```compile_fail
/// use capntproto_rpc_engine::question::release::{Last, Owner};
/// fn cannot_escape<O: Owner + 'static>(owner: &O, key: O::Key) -> Last<'static, O> {
///     Last::new(owner, key)
/// }
/// ```
pub struct Last<'a, O: Owner + ?Sized> {
    owner: &'a O,
    key: Option<O::Key>,
}
impl<'a, O: Owner + ?Sized> Last<'a, O> {
    pub fn new(owner: &'a O, key: O::Key) -> Self {
        Self {
            owner,
            key: Some(key),
        }
    }
    /// Connection teardown can take back authority without publishing Finish.
    pub fn disarm(mut self) -> O::Key {
        self.key.take().expect("armed last interest")
    }
}
impl<O: Owner + ?Sized> Drop for Last<'_, O> {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            run(self.owner, key);
        }
    }
}

#[cfg(test)]
mod tests;
