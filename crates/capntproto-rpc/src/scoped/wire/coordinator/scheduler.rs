//! Scheduling mechanics, independent of protocol tables and transport policy.
//!
//! A ready bit is an obligation to poll a lane, not proof that it can progress.
//! Only the lane may certify itself idle. Yield retains the bit and uses the
//! executor waker directly; application notifications invalidate certificates.
use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not};
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};
use std::task::{Wake, Waker};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(super) enum Lane {
    Foreign,
    Accept,
    Calls,
    Worker,
    Maintenance,
    Commands,
}
impl Lane {
    const ALL: [Self; 6] = [
        Self::Foreign,
        Self::Accept,
        Self::Calls,
        Self::Worker,
        Self::Maintenance,
        Self::Commands,
    ];
    pub(super) const fn work(self) -> WorkSet {
        WorkSet(1 << self as u8)
    }
    pub(super) const fn publishes(self) -> bool {
        matches!(
            self,
            Self::Calls | Self::Worker | Self::Maintenance | Self::Commands
        )
    }
}

/// Closed set: callers cannot create out-of-range scheduler bits.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct WorkSet(u8);
impl WorkSet {
    pub(super) const CONTROL: Self = Self(1 << 6);
    pub(super) const NONE: Self = Self(0);
    pub(super) const ALL: Self = Self(0x3f);
    pub(super) const FOREIGN: Self = Lane::Foreign.work();
    pub(super) const ACCEPT: Self = Lane::Accept.work();
    pub(super) const CALLS: Self = Lane::Calls.work();
    pub(super) const WORKER: Self = Lane::Worker.work();
    pub(super) const MAINTENANCE: Self = Lane::Maintenance.work();
    pub(super) const COMMANDS: Self = Lane::Commands.work();
    pub(super) fn is_empty(self) -> bool {
        self == Self::NONE
    }
}
impl BitOr for WorkSet {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}
impl BitAnd for WorkSet {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}
impl Not for WorkSet {
    type Output = Self;
    fn not(self) -> Self {
        Self(!self.0 & Self::ALL.0)
    }
}
impl BitOrAssign for WorkSet {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = *self | rhs;
    }
}
impl BitAndAssign for WorkSet {
    fn bitand_assign(&mut self, rhs: Self) {
        *self = *self & rhs;
    }
}

pub(super) struct Schedule {
    pub(super) active: WorkSet,
    pub(super) cursor: Lane,
}
impl Default for Schedule {
    fn default() -> Self {
        Self {
            active: WorkSet::ALL,
            cursor: Lane::Foreign,
        }
    }
}
impl Schedule {
    /// Prefer a new source only after a complete idle scan. Notifications during
    /// a partial scan must not let a hot source continually postpone cleanup.
    pub(super) fn resume_at(&mut self, lane: Lane) {
        if self.active.is_empty() {
            self.cursor = lane;
        }
    }
    pub(super) fn next(&mut self) -> Option<Lane> {
        if self.active.is_empty() {
            return None;
        }
        let after_cursor = self.active.0 & (u8::MAX << self.cursor as u8);
        let candidates = if after_cursor == 0 {
            self.active.0
        } else {
            after_cursor
        };
        let index = candidates.trailing_zeros() as usize;
        self.cursor = Lane::ALL[if index == 5 { 0 } else { index + 1 }];
        Some(Lane::ALL[index])
    }
}

#[derive(Default)]
struct WorkWake {
    changed: AtomicU8,
    parent: futures::task::AtomicWaker,
}
impl WorkWake {
    fn notify(&self, work: WorkSet) {
        if self.changed.fetch_or(work.0, Ordering::AcqRel) == 0 {
            self.parent.wake();
        }
    }
}
impl Wake for WorkWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.notify(WorkSet::ALL);
    }
}

// Local enqueue shares the notifier without a task-waker clone per request.
pub(in crate::scoped) struct EnqueueWake(Arc<WorkWake>);
struct ControlWake(Arc<WorkWake>);
impl Wake for ControlWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.notify(WorkSet::CONTROL);
    }
}
impl EnqueueWake {
    pub(in crate::scoped) fn notify(&self) {
        self.0.notify(WorkSet::CALLS);
    }
}

#[derive(Default)]
pub(super) struct Notifications {
    wake: Arc<WorkWake>,
    parent: Option<Waker>,
}
impl Notifications {
    pub(super) fn control_waker(&self) -> Waker {
        Waker::from(Arc::new(ControlWake(self.wake.clone())))
    }
    pub(super) fn waker(&self) -> Waker {
        Waker::from(self.wake.clone())
    }
    pub(super) fn enqueue(&self) -> EnqueueWake {
        EnqueueWake(self.wake.clone())
    }
    pub(super) fn pending(&self) -> WorkSet {
        WorkSet(self.wake.changed.load(Ordering::Acquire))
    }
    pub(super) fn same_parent(&self, parent: &Waker) -> bool {
        self.parent
            .as_ref()
            .is_some_and(|old| old.will_wake(parent))
    }
    /// Register before consuming the mask: notification consumes AtomicWaker's
    /// owner. Replacing the executor task also requires a new registration even
    /// if the application has not notified us. Spurious same-task polls do not.
    pub(super) fn take(&mut self, parent: &Waker) -> WorkSet {
        let same = self.same_parent(parent);
        if !self.pending().is_empty() || !same {
            self.wake.parent.register(parent);
            if !same {
                self.parent = Some(parent.clone());
            }
            WorkSet(self.wake.changed.swap(0, Ordering::AcqRel))
        } else {
            WorkSet::NONE
        }
    }
    pub(super) fn detach(&mut self) {
        self.wake.parent.take();
        self.parent = None;
    }
}

#[cfg(test)]
mod tests;
