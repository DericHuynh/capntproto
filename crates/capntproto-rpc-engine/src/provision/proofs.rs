use super::*;

#[kani::proof]
#[kani::unwind(7)]
fn accept_embargo_replay_and_revoke_preserve_authority() {
    let mut state = EmbargoState::new();
    let mut accepts = 0u8;
    let mut releases = 0u8;
    let mut ended = false;
    for _ in 0..6 {
        let action: u8 = kani::any();
        kani::assume(action < 3);
        match action {
            0 => {
                let accepted = state.claim().is_ok();
                assert!(!accepted || !ended, "ended provision refuses new Accept");
                accepts += u8::from(accepted);
                kani::assert(accepts <= 1, "one Accept per embargo identifier");
            }
            1 => {
                let released = state.release().is_ok();
                assert!(!released || !ended, "ended provision refuses disembargo");
                releases += u8::from(released);
                assert!(releases <= 1, "one release per embargo identifier");
            }
            _ => {
                state.close();
                ended = true;
            }
        }
        let ready = state.readiness();
        if accepts == 1 && releases == 1 {
            assert!(ready.is_ok(), "released acceptance survives provision end");
        } else if accepts == 1 && ended {
            assert_eq!(
                ready,
                Err(Error::Closed),
                "pending acceptance is canceled by provision end"
            );
        } else {
            assert_eq!(
                ready,
                Err(Error::Pending),
                "pending acceptance cannot pass embargo"
            );
        }
    }
}

#[kani::proof]
#[kani::unwind(8)]
fn scoped_embargo_claims_survive_capacity_and_revoke() {
    let mut slots = [const { Slot::<1>::EMPTY }; 2];
    with_embargoes(&mut slots, |table| {
        let first_release: bool = kani::any();
        if first_release {
            table.release(b"a").unwrap();
        }
        let first = table.claim(b"a").unwrap();
        let second = table.claim(b"b").unwrap();
        assert!(
            matches!(table.claim(b"c"), Err(Error::Full)),
            "full table refuses another claim"
        );
        assert_eq!(
            table.release(b"ab"),
            Err(Error::IdentifierTooLong),
            "long identifier does not enter storage"
        );
        assert_eq!(
            table.readiness(&second),
            Err(Error::Pending),
            "neighboring claim retains its own embargo"
        );
        if !first_release {
            table.release(b"a").unwrap();
        }
        table.close();
        assert!(
            table.complete(first).is_ok(),
            "released claim transfers after registry close"
        );
        let rejected = table.complete(second).unwrap_err();
        kani::assert(
            rejected.error == Error::Closed,
            "unreleased claim cannot complete at registry close",
        );
        table.cancel(rejected.claim);
    })
    .unwrap();
}
