//! Queued work alone owns a payload and receipt waiters. Finished records retain
//! only immutable duplicate-detection metadata and the first terminal outcome.
#![forbid(unsafe_code)]

use super::{Outcome, Snapshot};
use futures::channel::oneshot;
use std::{collections::BTreeMap, rc::Rc};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) struct Metadata {
    pub key: u32,
    pub deadline: u64,
    pub digest: [u8; 32],
}

pub(super) type Waiters = BTreeMap<u64, oneshot::Sender<Outcome>>;

pub(super) struct Queued {
    metadata: Metadata,
    pub snapshot: Rc<Snapshot>,
    pub waiters: Waiters,
}

/// Cancellation before an offer does not bind any payload metadata. Later
/// valid offers cannot revive it, regardless of their key, deadline or data.
#[derive(Clone, Copy)]
pub(super) enum Tombstone {
    Canceled,
    Closed,
}
impl Tombstone {
    pub fn outcome(self) -> Outcome {
        match self {
            Self::Canceled => Outcome::Canceled,
            Self::Closed => Outcome::Closed,
        }
    }
}

pub(super) enum Record {
    Queued(Queued),
    Finished {
        metadata: Metadata,
        outcome: Outcome,
    },
    Tombstone(Tombstone),
}
impl Record {
    pub fn queued(sequence: u64, metadata: Metadata, bytes: &[u8]) -> Self {
        Self::Queued(Queued {
            snapshot: Rc::new(Snapshot::new(
                sequence,
                metadata.key,
                metadata.deadline,
                bytes,
            )),
            metadata,
            waiters: BTreeMap::new(),
        })
    }
    pub fn metadata(&self) -> Option<Metadata> {
        match self {
            Self::Queued(pending) => Some(pending.metadata),
            Self::Finished { metadata, .. } => Some(*metadata),
            Self::Tombstone(_) => None,
        }
    }
    pub fn outcome(&self) -> Option<Outcome> {
        match self {
            Self::Queued(_) => None,
            Self::Finished { outcome, .. } => Some(*outcome),
            Self::Tombstone(tombstone) => Some(tombstone.outcome()),
        }
    }
    pub fn pending(&self) -> Option<&Queued> {
        match self {
            Self::Queued(pending) => Some(pending),
            _ => None,
        }
    }
    pub fn pending_mut(&mut self) -> Option<&mut Queued> {
        match self {
            Self::Queued(pending) => Some(pending),
            _ => None,
        }
    }
    /// Take waiters for notification after the receiver's state borrow ends.
    /// Repeated completion preserves the first outcome and returns no waiters.
    pub fn finish(&mut self, outcome: Outcome) -> Option<(u32, Waiters)> {
        let Self::Queued(pending) = self else {
            return None;
        };
        let metadata = pending.metadata;
        let Self::Queued(pending) = std::mem::replace(self, Self::Finished { metadata, outcome })
        else {
            unreachable!("record was queued")
        };
        Some((metadata.key, pending.waiters))
    }
}
