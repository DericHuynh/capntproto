//! Atomic local-export leases for receiverHosted descriptors and call targets.
use super::*;

pub struct LeaseSlot<'id> {
    wire: Option<ExportId>,
    lease: Option<Lease<'id>>,
    plan: Option<Key<'id>>,
}
impl<'id> LeaseSlot<'id> {
    pub const fn none() -> Self {
        Self {
            wire: None,
            lease: None,
            plan: None,
        }
    }
    pub fn new(id: ExportId) -> Self {
        Self {
            wire: Some(id),
            ..Self::none()
        }
    }
    pub fn id(&self) -> Option<ExportId> {
        self.wire
    }
    pub fn lease(&self) -> Option<&Lease<'id>> {
        self.lease.as_ref()
    }
    pub fn take(&mut self) -> Option<Lease<'id>> {
        self.lease.take()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseError {
    Export(Error),
    Occupied,
}
impl From<Error> for LeaseError {
    fn from(e: Error) -> Self {
        Self::Export(e)
    }
}
impl core::fmt::Display for LeaseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "export lease batch: {self:?}")
    }
}
impl core::error::Error for LeaseError {}

#[must_use]
pub struct Leases<'a, 'receipts, 'id, 'storage, T> {
    pub(super) exports: &'a mut Exports<'id, 'storage, T>,
    pub(super) receipts: &'receipts mut [LeaseSlot<'id>],
}
impl<'id, 'storage, T> Exports<'id, 'storage, T> {
    pub fn prepare_leases<'a, 'receipts>(
        &'a mut self,
        receipts: &'receipts mut [LeaseSlot<'id>],
    ) -> Result<Leases<'a, 'receipts, 'id, 'storage, T>, LeaseError> {
        self.abort_pending();
        let mut prepared = Leases {
            exports: self,
            receipts,
        };
        prepared.plan()?;
        Ok(prepared)
    }
}
impl<'receipts, 'id, T> Leases<'_, 'receipts, 'id, '_, T> {
    fn plan(&mut self) -> Result<(), LeaseError> {
        for receipt in self.receipts.iter_mut() {
            receipt.plan = None;
        }
        for position in 0..self.receipts.len() {
            let (before, current) = self.receipts.split_at_mut(position);
            let current = &mut current[0];
            if current.lease.is_some() {
                return Err(LeaseError::Occupied);
            }
            let Some(id) = current.wire else {
                continue;
            };
            let index = self.exports.live_index(id).ok_or(Error::Missing)?;
            let count = before
                .iter()
                .filter(|r| r.plan.is_some_and(|k| k.index == index))
                .count()
                .checked_add(1)
                .and_then(|n| u32::try_from(n).ok())
                .ok_or(Error::LocalReferencesExhausted)?;
            self.exports.slots[index]
                .entry
                .as_ref()
                .unwrap()
                .local
                .checked_add(count)
                .ok_or(Error::LocalReferencesExhausted)?;
            current.plan = Some(self.exports.key(index));
        }
        Ok(())
    }
    pub fn accept(mut self) -> &'receipts mut [LeaseSlot<'id>] {
        self.commit();
        core::mem::take(&mut self.receipts)
    }
    pub(super) fn commit(&mut self) {
        for receipt in self.receipts.iter_mut() {
            if let Some(key) = receipt.plan.take() {
                self.exports.slots[key.index].entry.as_mut().unwrap().local += 1;
                receipt.lease = Some(Lease { key });
                receipt.wire = None;
            }
        }
    }
}
impl<T> Drop for Leases<'_, '_, '_, '_, T> {
    fn drop(&mut self) {
        for receipt in self.receipts.iter_mut() {
            receipt.plan = None;
        }
    }
}

#[cfg(kani)]
mod proofs {
    use super::*;
    #[kani::proof]
    #[kani::unwind(4)]
    fn receiver_hosted_batch_retains_every_local_reference() {
        let local: u32 = kani::any();
        let mut slots = [Slot {
            generation: 1,
            entry: Some(Entry {
                #[cfg(feature = "alloc")]
                presence: Default::default(),
                value: 41u8,
                state: State::Live(ExportRefs::new()),
                local,
                pending: 0,
                promise: promise::ImportState::Hosted,
            }),
        }];
        let mut exports = Exports {
            #[cfg(feature = "alloc")]
            observation: Default::default(),
            slots: crate::storage::Storage::borrowed(&mut slots),
            open_batch: false,
            pending_resolution: None,
            brand: PhantomData,
        };
        let mut receipts = [
            LeaseSlot::new(ExportId::from_wire(0)),
            LeaseSlot::new(ExportId::from_wire(0)),
        ];
        let accepted = match exports.prepare_leases(&mut receipts) {
            Ok(pending) => {
                assert!(local <= u32::MAX - 2);
                if kani::any() {
                    pending.accept();
                    true
                } else {
                    drop(pending);
                    false
                }
            }
            Err(error) => {
                assert!(local > u32::MAX - 2);
                assert_eq!(error, LeaseError::Export(Error::LocalReferencesExhausted));
                false
            }
        };
        kani::assert(
            exports.slots[0].entry.as_ref().unwrap().local == local + if accepted { 2 } else { 0 },
            "all receiver-hosted leases are retained atomically",
        );
        assert_eq!(receipts[0].lease().is_some(), accepted);
        assert_eq!(receipts[1].lease().is_some(), accepted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_local_descriptors_keep_export_alive_after_remote_release() {
        let measured = allocation_counter::measure(|| {
            let mut slots = [Slot::EMPTY];
            with_exports(&mut slots, |exports| {
                let mut batch = exports.batch();
                let grant = batch.export(41).unwrap();
                batch.commit();
                let mut leases = [
                    LeaseSlot::new(grant.id()),
                    LeaseSlot::none(),
                    LeaseSlot::new(grant.id()),
                ];
                drop(exports.prepare_leases(&mut leases).unwrap());
                assert_eq!(exports.slots[0].entry.as_ref().unwrap().local, 0);
                exports.prepare_leases(&mut leases).unwrap().accept();
                assert_eq!(exports.release(grant.id(), 1), Ok(None));
                assert_eq!(exports.get(leases[2].lease().unwrap()), &41);
                assert_eq!(exports.release_local(leases[0].take().unwrap()), None);
                assert_eq!(exports.release_local(leases[2].take().unwrap()), Some(41));
                assert!(exports.is_empty());
            })
            .unwrap();
        });
        assert_eq!(measured.count_total, 0);
    }
    #[test]
    fn invalid_or_overflowing_local_batch_changes_no_prefix() {
        for variant in 0..3 {
            let mut slots = [Slot::EMPTY];
            with_exports(&mut slots, |exports| {
                let mut batch = exports.batch();
                let grant = batch.export(41).unwrap();
                batch.commit();
                let before = if variant == 1 { u32::MAX - 1 } else { 0 };
                exports.slots[0].entry.as_mut().unwrap().local = before;
                let mut leases = [
                    LeaseSlot::new(grant.id()),
                    LeaseSlot::new(if variant == 0 {
                        ExportId::from_wire(99)
                    } else {
                        grant.id()
                    }),
                ];
                if variant == 2 {
                    leases[1].lease = Some(exports.lease(grant.id()).unwrap());
                }
                assert!(exports.prepare_leases(&mut leases).is_err());
                assert_eq!(
                    exports.slots[0].entry.as_ref().unwrap().local,
                    before + u32::from(variant == 2)
                );
                assert!(leases[0].lease().is_none());
            })
            .unwrap();
        }
    }
}
