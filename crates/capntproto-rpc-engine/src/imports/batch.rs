//! Atomic receive-side reference admission. Scratch and new owners belong to
//! the caller; preflight changes no owners, references or generations and invokes
//! no application code. A growing provider may retain reserved empty capacity
//! after rejection or rollback.
use super::*;

#[derive(Clone, Copy)]
struct Plan {
    index: usize,
    generation: u64,
}

/// One decoded descriptor's input and accepted local reference. `none` reserves
/// a tombstone in a message's capability table. New owners remain in the slot on
/// every rejection; successful admission moves them into the import table.
pub struct ReceiveSlot<'id, T> {
    wire: Option<ImportId>,
    promise: bool,
    owner: Option<T>,
    local: Option<Local<'id>>,
    plan: Option<Plan>,
}
impl<'id, T> ReceiveSlot<'id, T> {
    pub const fn none() -> Self {
        Self {
            wire: None,
            promise: false,
            owner: None,
            local: None,
            plan: None,
        }
    }
    pub fn new(id: ImportId, owner: T) -> Self {
        Self {
            wire: Some(id),
            promise: false,
            owner: Some(owner),
            local: None,
            plan: None,
        }
    }
    pub fn alias(id: ImportId) -> Self {
        Self {
            wire: Some(id),
            promise: false,
            owner: None,
            local: None,
            plan: None,
        }
    }
    pub fn id(&self) -> Option<ImportId> {
        self.wire
    }
    pub fn new_promise(id: ImportId, owner: T) -> Self {
        Self {
            promise: true,
            ..Self::new(id, owner)
        }
    }
    pub fn alias_promise(id: ImportId) -> Self {
        Self {
            promise: true,
            ..Self::alias(id)
        }
    }
    pub fn is_promise(&self) -> bool {
        self.promise
    }
    pub fn owner(&self) -> Option<&T> {
        self.owner.as_ref()
    }
    pub fn take_owner(&mut self) -> Option<T> {
        self.owner.take()
    }
    pub fn local(&self) -> Option<&Local<'id>> {
        self.local.as_ref()
    }
    pub fn take_local(&mut self) -> Option<Local<'id>> {
        self.local.take()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiveError {
    Import(Error),
    OccupiedReceipt,
    MissingOwner,
    UnexpectedOwner,
    Promise(promise::Error),
}
impl core::fmt::Display for ReceiveError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "import batch: {self:?}")
    }
}
impl core::error::Error for ReceiveError {}
impl From<Error> for ReceiveError {
    fn from(value: Error) -> Self {
        Self::Import(value)
    }
}

impl<'id, 'storage, T> Imports<'id, 'storage, T> {
    /// Reserve every descriptor before applying any reference count, generation
    /// or new-owner transition. Exclusive borrows keep the plan valid until commit.
    /// Repeated IDs require one `new` (only if absent) followed by `alias` slots.
    /// Cost is bounded by O(receipts * (imports + receipts)), independent of IDs.
    pub fn prepare_receive<'table, 'receipts>(
        &'table mut self,
        receipts: &'receipts mut [ReceiveSlot<'id, T>],
    ) -> Result<Receive<'table, 'receipts, 'id, 'storage, T>, ReceiveError> {
        let mut receive = Receive {
            imports: self,
            receipts,
        };
        receive.plan()?;
        Ok(receive)
    }
}

#[must_use = "commit only after the complete incoming message is validated"]
pub struct Receive<'table, 'receipts, 'id, 'storage, T> {
    imports: &'table mut Imports<'id, 'storage, T>,
    receipts: &'receipts mut [ReceiveSlot<'id, T>],
}
impl<'receipts, 'id, T> Receive<'_, 'receipts, 'id, '_, T> {
    fn plan(&mut self) -> Result<(), ReceiveError> {
        // Also discard plans left by a deliberately forgotten previous ticket.
        for receipt in self.receipts.iter_mut() {
            receipt.plan = None;
        }
        for position in 0..self.receipts.len() {
            let (before, current) = self.receipts.split_at_mut(position);
            let current = &mut current[0];
            if current.local.is_some() {
                return Err(ReceiveError::OccupiedReceipt);
            }
            let Some(wire) = current.wire else {
                if current.owner.is_some() {
                    return Err(ReceiveError::UnexpectedOwner);
                }
                continue;
            };
            let repeated = before
                .iter()
                .find(|r| r.wire == Some(wire))
                .and_then(|r| r.plan);
            let existing = self.imports.index(wire);
            existing
                .and_then(|index| self.imports.slots[index].entry.as_ref())
                .map_or(promise::ImportState::Hosted, |e| e.promise)
                .descriptor(current.promise)
                .map_err(ReceiveError::Promise)?;
            let plan = if let Some(plan) = repeated {
                plan
            } else if let Some(index) = existing {
                let slot = &self.imports.slots[index];
                let generation = if slot.entry.as_ref().unwrap().local == 0 {
                    slot.generation
                        .checked_add(1)
                        .ok_or(Error::GenerationExhausted)?
                } else {
                    slot.generation
                };
                Plan { index, generation }
            } else {
                let index = self
                    .imports
                    .slots
                    .vacant(
                        |index, slot| {
                            slot.entry.is_none()
                                && slot.generation != u64::MAX
                                && !before
                                    .iter()
                                    .any(|r| r.plan.is_some_and(|p| p.index == index))
                        },
                        || Slot::EMPTY,
                    )
                    .ok_or(Error::Full)?;
                Plan {
                    index,
                    generation: self.imports.slots[index].generation + 1,
                }
            };
            if existing.is_some() || repeated.is_some() {
                if current.owner.is_some() {
                    return Err(ReceiveError::UnexpectedOwner);
                }
            } else if current.owner.is_none() {
                return Err(ReceiveError::MissingOwner);
            }
            let count = before
                .iter()
                .filter(|r| r.plan.is_some_and(|p| p.index == plan.index))
                .count()
                .checked_add(1)
                .and_then(|n| u32::try_from(n).ok())
                .ok_or(Error::RemoteReferencesExhausted)?;
            let entry = self.imports.slots[plan.index].entry.as_ref();
            entry
                .map_or(0, |e| e.local)
                .checked_add(count)
                .ok_or(Error::LocalReferencesExhausted)?;
            entry
                .map_or(0, |e| e.remote.count())
                .checked_add(count)
                .ok_or(Error::RemoteReferencesExhausted)?;
            current.plan = Some(plan);
        }
        Ok(())
    }
    /// Infallible after preflight. Every descriptor gets one noncopy local token;
    /// no callbacks, destructors, IO or allocation occur inside the transition.
    pub fn commit(mut self) -> &'receipts mut [ReceiveSlot<'id, T>] {
        for receipt in self.receipts.iter_mut() {
            let Some(plan) = receipt.plan.take() else {
                continue;
            };
            if self.imports.slots[plan.index]
                .entry
                .as_ref()
                .is_some_and(|entry| entry.local == 0)
            {
                self.imports.binding_revision = self.imports.binding_revision.saturating_add(1);
                self.imports.unlink_release(plan.index);
            }
            if self.imports.slots[plan.index].entry.is_none() {
                loop {
                    let index = self
                        .imports
                        .free
                        .take(&mut self.imports.slots, |slot| &mut slot.free);
                    if self.imports.slots[index].generation != u64::MAX {
                        assert_eq!(
                            index, plan.index,
                            "preflighted lowest available import slot"
                        );
                        break;
                    }
                }
            }
            let slot = &mut self.imports.slots[plan.index];
            let wire = receipt.wire.take().expect("planned wire identity");
            if let Some(entry) = slot.entry.as_mut() {
                entry.promise = entry
                    .promise
                    .descriptor(receipt.promise)
                    .expect("preflighted promise obligation");
                entry.local += 1;
                entry
                    .remote
                    .receive(1)
                    .expect("preflighted remote references");
            } else {
                self.imports.displaced +=
                    usize::from(usize::try_from(wire.to_wire()).ok() != Some(plan.index));
                let mut remote = ImportRefs::new();
                remote.receive(1).unwrap();
                slot.entry = Some(Entry {
                    #[cfg(feature = "alloc")]
                    presence: self.imports.observation.enter(),
                    wire,
                    value: receipt.owner.take().expect("preflighted owner"),
                    local: 1,
                    remote,
                    promise: promise::ImportState::Hosted
                        .descriptor(receipt.promise)
                        .unwrap(),
                });
            }
            slot.generation = plan.generation;
            receipt.local = Some(Local {
                key: Key {
                    index: plan.index,
                    generation: plan.generation,
                    brand: PhantomData,
                },
            });
        }
        core::mem::take(&mut self.receipts)
    }
}
impl<T> Drop for Receive<'_, '_, '_, '_, T> {
    fn drop(&mut self) {
        for receipt in self.receipts.iter_mut() {
            receipt.plan = None;
        }
    }
}

/// One Resolve and its replacement descriptor are committed together. The
/// returned key identifies an existing observer; absent/abandoned promises
/// require the caller to release the replacement instead of dispatching it.
#[must_use]
pub struct Resolution<'table, 'receipts, 'id, 'storage, T> {
    receive: Receive<'table, 'receipts, 'id, 'storage, T>,
    promise: Option<usize>,
}
impl<'id, 'storage, T> Imports<'id, 'storage, T> {
    pub fn prepare_resolution<'table, 'receipts>(
        &'table mut self,
        id: ImportId,
        receipts: &'receipts mut [ReceiveSlot<'id, T>],
    ) -> Result<Resolution<'table, 'receipts, 'id, 'storage, T>, ReceiveError> {
        let index = self.index(id);
        if let Some(index) = index {
            self.slots[index]
                .entry
                .as_ref()
                .unwrap()
                .promise
                .resolution()
                .map_err(ReceiveError::Promise)?;
            // Receiving this descriptor after committing Resolve would be a
            // forbidden re-introduction of the just-resolved promise ID.
            if receipts.iter().any(|r| r.wire == Some(id) && r.promise) {
                return Err(ReceiveError::Promise(promise::Error::AlreadyResolved));
            }
        }
        Ok(Resolution {
            receive: self.prepare_receive(receipts)?,
            promise: index,
        })
    }
}
impl<'receipts, 'id, T> Resolution<'_, 'receipts, 'id, '_, T> {
    pub fn is_observed(&self) -> bool {
        self.promise.is_some_and(|index| {
            self.receive.imports.slots[index]
                .entry
                .as_ref()
                .unwrap()
                .local
                != 0
        })
    }
    pub fn commit(self) -> (Option<Key<'id>>, &'receipts mut [ReceiveSlot<'id, T>]) {
        let key = self.promise.and_then(|index| {
            let entry = self.receive.imports.slots[index].entry.as_mut().unwrap();
            entry.promise = entry.promise.resolution().expect("preflighted Resolve");
            (entry.local != 0).then(|| self.receive.imports.local(index).key())
        });
        (key, self.receive.commit())
    }
}

#[cfg(kani)]
mod proofs;
#[cfg(test)]
mod tests;
