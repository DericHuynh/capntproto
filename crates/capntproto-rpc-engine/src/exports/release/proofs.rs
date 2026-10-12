use super::*;
#[kani::proof]
#[kani::unwind(4)]
fn implicit_releases_are_atomic_for_full_reference_counts() {
    let count: u32 = kani::any();
    kani::assume(count > 0);
    let first: u32 = kani::any();
    let second: u32 = kani::any();
    let mut slots = [Slot::EMPTY];
    with_exports(&mut slots, |exports| {
        let mut batch = exports.batch();
        let grant = batch.export(17u8).unwrap();
        batch.commit();
        let entry = exports.slots[0].entry.as_mut().unwrap();
        let State::Live(refs) = &mut entry.state else {
            unreachable!()
        };
        refs.retain(count - 1).unwrap();
        let mut scratch = [0];
        let mut pending = exports.prepare_releases(&mut scratch).unwrap();
        let a = pending.record(grant.id(), first);
        let b = pending.record(grant.id(), second);
        let sum = first.checked_add(second);
        let valid = sum.is_some_and(|sum| sum <= count);
        let ready = pending.finish();
        kani::assert(
            ready.is_ok() == valid,
            "implicit release requires the complete valid count",
        );
        if valid {
            assert!(a.is_ok() && b.is_ok());
            ready.unwrap().accept();
            let entry = exports.slots[0].entry.as_ref().unwrap();
            match &entry.state {
                State::Live(refs) => assert_eq!(refs.count(), count - sum.unwrap()),
                State::Retired => assert_eq!(count, sum.unwrap()),
                _ => unreachable!(),
            }
        } else {
            drop(ready);
            let State::Live(refs) = &exports.slots[0].entry.as_ref().unwrap().state else {
                unreachable!()
            };
            kani::assert(
                refs.count() == count,
                "rejected implicit releases preserve published ownership",
            );
        }
    })
    .unwrap();
}
