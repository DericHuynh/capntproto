//! Network redirection is not authority to adopt a locally retained result.
#[cfg(any(test, kani))]
fn check(batched: bool, allowed: bool, finish_first: bool) {
    use super::*;
    use crate::ids::WireId;
    let mut slots = [Slot::EMPTY];
    with_answers(&mut slots, |answers| {
        let id = AnswerId::from_wire(7);
        let key = if batched {
            answers
                .prepare_admission_with_pipelines(id, &mut [])
                .unwrap()
                .third_party()
                .allow_handoff(allowed)
                .accept(41u8)
                .0
        } else {
            answers
                .prepare_admission(id, None)
                .unwrap()
                .third_party()
                .allow_handoff(allowed)
                .accept(41u8)
                .0
        };
        #[cfg(kani)]
        kani::assert(
            answers.prepare_adoption(id).is_err(),
            "third-party answer cannot be adopted locally",
        );
        assert!(matches!(
            answers.prepare_adoption(id),
            Err(Error::NotRedirected)
        ));
        assert!(answers.is_third_party(key).unwrap());
        let pipeline = answers.pipeline(id).unwrap();
        if finish_first {
            assert!(answers.finish(id, false).retired.is_none());
        }
        let pending = answers.prepare_return(key).unwrap();
        #[cfg(kani)]
        kani::assert(
            pending.allows_handoff() == allowed,
            "network Return preserves caller handoff opt-in",
        );
        assert_eq!(pending.allows_handoff(), allowed);
        assert!(pending.is_redirected());
        assert_eq!(pending.is_cancelled(), finish_first);
        assert_eq!(pending.accept(), None);
        if !finish_first {
            assert!(answers.finish(id, false).retired.is_none());
        }
        assert_eq!(*answers.pipelined(&pipeline), 41);
        assert_eq!(answers.release_pipeline(pipeline), Some(41));
        let reused = answers.prepare_admission(id, None).unwrap().accept(43).0;
        assert_ne!(reused, key);
        let pending = answers.prepare_return(reused).unwrap();
        assert!(
            !pending.allows_handoff(),
            "new generation does not inherit handoff permission"
        );
        assert!(!pending.is_redirected());
        pending.accept();
        assert_eq!(answers.finish(id, false).retired, Some(43));
        assert!(answers.is_empty());
    })
    .unwrap();
}
#[cfg(test)]
mod tests {
    #[test]
    fn third_party_admission_cannot_be_locally_adopted() {
        let measured = allocation_counter::measure(|| {
            for batched in [false, true] {
                for allowed in [false, true] {
                    for finish_first in [false, true] {
                        super::check(batched, allowed, finish_first);
                    }
                }
            }
        });
        assert_eq!(measured.count_total, 0);
    }
}
#[cfg(kani)]
mod proofs {
    #[kani::proof]
    #[kani::unwind(4)]
    fn third_party_destination_and_handoff_permission_stay_distinct() {
        super::check(kani::any(), kani::any(), kani::any());
    }
}
