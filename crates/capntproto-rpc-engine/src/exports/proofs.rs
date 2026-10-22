use super::*;

/// Publication of a Resolve changes the promise obligation, not its arbitrary
/// positive wire count. Aborted/forgotten output remains retryable.
#[kani::proof]
#[kani::unwind(3)]
fn exported_resolution_requires_output_acceptance() {
    let count: u32 = kani::any();
    kani::assume(count != 0);
    let mut refs = ExportRefs::new();
    refs.retain(count - 1).unwrap();
    let mut slots = [Slot {
        retirement: crate::work_queue::Link::EMPTY,
        publication: crate::work_queue::Link::EMPTY,
        generation: 1,
        entry: Some(Entry {
            #[cfg(feature = "alloc")]
            presence: Default::default(),
            value: 41u8,
            state: State::Live(refs),
            local: 0,
            pending: 0,
            promise: promise::ImportState::Pending,
        }),
    }];
    let mut exports = Exports {
        unavailable: slots
            .iter()
            .filter(|s| s.entry.is_some() || s.generation == u64::MAX)
            .count(),
        retired: crate::work_queue::Queue::EMPTY,
        staged: crate::work_queue::Queue::EMPTY,
        #[cfg(feature = "alloc")]
        observation: Default::default(),
        slots: crate::storage::Storage::borrowed(&mut slots),
        open_batch: false,
        pending_resolution: None,
        brand: PhantomData,
    };
    let key = exports.key(0);
    let mut batch = exports.batch();
    batch.resolve_promise(key).unwrap();
    assert_eq!(batch.resolution_id(), Some(ExportId::from_wire(0)));
    assert!(batch.grant(key).is_err());
    let accepted: bool = kani::any();
    if accepted {
        batch.commit();
    } else if kani::any() {
        drop(batch);
    } else {
        core::mem::forget(batch);
    }
    let mut retry = exports.batch();
    kani::assert(
        retry.resolve_promise(key).is_ok() != accepted,
        "exported promise resolves only on output acceptance",
    );
    let State::Live(refs) = &retry.exports.slots[0].entry.as_ref().unwrap().state else {
        unreachable!()
    };
    assert_eq!(refs.count(), count);
}

/// Exact descriptor counts for two exports and up to three full-width wire IDs.
/// Wrong/extra/missing IDs, even ignored record errors, cannot validate a batch.
#[kani::proof]
#[kani::unwind(4)]
fn descriptor_multiset_matches_reservations() {
    let a: u32 = kani::any();
    let b: u32 = kani::any();
    kani::assume(a <= 2 && b <= 2);
    let count: usize = kani::any();
    kani::assume(count <= 3);
    let ids: [u32; 3] = kani::any();
    let mut slots = [const { Slot::EMPTY }; 2];
    for (index, pending) in [a, b].into_iter().enumerate() {
        if pending > 0 {
            slots[index].entry = Some(Entry {
                #[cfg(feature = "alloc")]
                presence: Default::default(),
                value: (),
                state: State::Staged,
                pending,
                local: 0,
                promise: promise::ImportState::Hosted,
            });
        }
    }
    let mut exports = Exports {
        unavailable: slots
            .iter()
            .filter(|s| s.entry.is_some() || s.generation == u64::MAX)
            .count(),
        retired: crate::work_queue::Queue::EMPTY,
        staged: crate::work_queue::Queue::EMPTY,
        #[cfg(feature = "alloc")]
        observation: Default::default(),
        slots: crate::storage::Storage::borrowed(&mut slots),
        open_batch: true,
        pending_resolution: None,
        brand: PhantomData,
    };
    for index in 0..2 {
        if exports.slots[index].entry.is_some() {
            exports
                .staged
                .push(&mut exports.slots, index, |slot| &mut slot.publication);
        }
    }
    let batch = Batch {
        exports: &mut exports,
    };
    let mut scratch = [u32::MAX; 2];
    let mut check = batch.descriptors(&mut scratch).unwrap();
    let mut expected_a = 0;
    let mut expected_b = 0;
    let mut unknown = false;
    for id in &ids[..count] {
        match *id {
            0 => expected_a += 1,
            1 => expected_b += 1,
            _ => unknown = true,
        }
        let _ = check.record(ExportId::from_wire(*id));
    }
    kani::assert(
        check.finish().is_ok() == (!unknown && expected_a == a && expected_b == b),
        "descriptor multiset equals reserved grants",
    );
    assert_eq!(
        batch.exports.slots[0]
            .entry
            .as_ref()
            .map_or(0, |e| e.pending),
        a
    );
    assert_eq!(
        batch.exports.slots[1]
            .entry
            .as_ref()
            .map_or(0, |e| e.pending),
        b
    );
}

/// Full-width count admission cannot overflow and errors remain poisoned.
#[kani::proof]
#[kani::unwind(2)]
fn descriptor_count_admission_never_wraps() {
    let reserved: u32 = kani::any();
    let count: u32 = kani::any();
    kani::assume(count <= reserved);
    let rejected: bool = kani::any();
    let slots = [Slot {
        retirement: crate::work_queue::Link::EMPTY,
        publication: crate::work_queue::Link::EMPTY,
        generation: 1,
        entry: Some(Entry {
            #[cfg(feature = "alloc")]
            presence: Default::default(),
            value: (),
            state: State::Staged,
            pending: reserved,
            local: 0,
            promise: promise::ImportState::Hosted,
        }),
    }];
    let mut counts = [count];
    let mut check = DescriptorCheck {
        slots: &slots,
        counts: &mut counts,
        rejected,
    };
    let result = check.record(ExportId::from_wire(0));
    if rejected || count == reserved {
        assert_eq!(result, Err(DescriptorError::Unreserved));
        assert_eq!(check.counts[0], count);
        assert_eq!(check.finish(), Err(DescriptorError::Unreserved));
    } else {
        assert_eq!(result, Ok(()));
        assert_eq!(check.counts[0], count + 1);
        assert_eq!(check.finish().is_ok(), count + 1 == reserved);
    }
}

/// One slot, arbitrary starting generation and descriptor acceptance. Counts,
/// reuse, rollback and retained-call ownership are checked on production code.
#[kani::proof]
#[kani::unwind(3)]
fn export_publication_and_lease_lifecycle() {
    let generation: u64 = kani::any();
    let accept: bool = kani::any();
    let mut slots = [Slot {
        retirement: crate::work_queue::Link::EMPTY,
        publication: crate::work_queue::Link::EMPTY,
        generation,
        entry: None,
    }];
    with_exports(&mut slots, |exports| {
        let mut batch = exports.batch();
        let result = batch.export(17u8);
        if generation == u64::MAX {
            let rejected = result.unwrap_err();
            assert_eq!(rejected.error, Error::GenerationExhausted);
            assert_eq!(rejected.value, 17);
            return;
        }
        let grant = result.unwrap();
        let key = grant.key();
        let id = grant.id();
        let _ = batch.grant(key).unwrap();
        if accept {
            batch.commit();
            let lease = exports.lease(id).unwrap();
            assert_eq!(exports.release(id, 3), Err(Error::OverRelease));
            let released = exports.release(id, 2).unwrap();
            kani::assert(released.is_none(), "export lease retains released owner");
            kani::assert(
                exports.resolve(key).is_none(),
                "released export cannot authorize a vine callback",
            );
            kani::assert(
                exports.take_retired(0).is_none(),
                "export lease retains released owner",
            );
            assert_eq!(exports.get(&lease), &17);
            assert_eq!(exports.release_local(lease), Some(17));
        } else {
            drop(batch);
            kani::assert(
                exports.lookup(id).is_none(),
                "unaccepted export is not published",
            );
            assert!(exports.resolve(key).is_none());
            assert_eq!(exports.release(id, 1), Err(Error::Missing));
            assert_eq!(exports.take_retired(0), Some(17));
        }
        assert!(exports.is_empty());
        let mut batch = exports.batch();
        let replacement = batch.export(29u8);
        if generation == u64::MAX - 1 {
            assert_eq!(replacement.unwrap_err().error, Error::GenerationExhausted);
        } else {
            let replacement = replacement.unwrap();
            assert_eq!(batch.grant(key).unwrap_err(), Error::StaleKey);
            batch.commit();
            assert_eq!(exports.resolve(key), None);
            assert_eq!(exports.release(replacement.id(), 1), Ok(Some(29)));
        }
    })
    .unwrap();
    assert!(slots[0].entry.is_none());
    assert!(slots[0].generation >= generation);
}

/// Inductive reservation transition over all valid full-width published and
/// staged counts; rejection is atomic and abort preserves published ownership.
#[kani::proof]
#[kani::unwind(3)]
fn export_reservations_preserve_published_counts() {
    let published: u32 = kani::any();
    let pending: u32 = kani::any();
    kani::assume(published.checked_add(pending).is_some());
    kani::assume(published != 0 || pending != 0);
    let state = if published == 0 {
        State::Staged
    } else {
        let mut refs = ExportRefs::new();
        refs.retain(published - 1).unwrap();
        State::Live(refs)
    };
    let mut slots = [Slot {
        retirement: crate::work_queue::Link::EMPTY,
        publication: crate::work_queue::Link::EMPTY,
        generation: 1,
        entry: Some(Entry {
            #[cfg(feature = "alloc")]
            presence: Default::default(),
            value: 17u8,
            state,
            local: 0,
            promise: promise::ImportState::Hosted,
            pending,
        }),
    }];
    let mut exports = Exports {
        unavailable: slots
            .iter()
            .filter(|s| s.entry.is_some() || s.generation == u64::MAX)
            .count(),
        retired: crate::work_queue::Queue::EMPTY,
        staged: crate::work_queue::Queue::EMPTY,
        #[cfg(feature = "alloc")]
        observation: Default::default(),
        slots: crate::storage::Storage::borrowed(&mut slots),
        open_batch: true,
        pending_resolution: None,
        brand: PhantomData,
    };
    if pending != 0 {
        exports
            .staged
            .push(&mut exports.slots, 0, |slot| &mut slot.publication);
    }
    let key = exports.key(0);
    let mut batch = Batch {
        exports: &mut exports,
    };
    let result = batch.grant(key);
    let total = u64::from(published) + u64::from(pending);
    if total == u64::from(u32::MAX) {
        assert_eq!(result.unwrap_err(), Error::RemoteReferencesExhausted);
        assert_eq!(
            batch.exports.slots[0].entry.as_ref().unwrap().pending,
            pending
        );
    } else {
        let _ = result.unwrap();
        assert_eq!(
            batch.exports.slots[0].entry.as_ref().unwrap().pending,
            pending + 1
        );
    }
    if kani::any() {
        batch.commit();
        let expected = if total == u64::from(u32::MAX) {
            total as u32
        } else {
            total as u32 + 1
        };
        assert_eq!(
            exports.release(ExportId::from_wire(0), expected),
            Ok(Some(17))
        );
    } else {
        drop(batch);
        if published == 0 {
            assert!(exports.lookup(ExportId::from_wire(0)).is_none());
            assert_eq!(exports.take_retired(0), Some(17));
            return;
        }
        let entry = exports.slots[0].entry.as_ref().unwrap();
        let State::Live(refs) = &entry.state else {
            unreachable!()
        };
        kani::assert(
            refs.count() == published && entry.pending == 0,
            "aborted export grants preserve published count",
        );
        assert_eq!(
            exports.release(ExportId::from_wire(0), published),
            Ok(Some(17))
        );
    }
}
