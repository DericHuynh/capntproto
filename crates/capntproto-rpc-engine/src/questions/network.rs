//! Outgoing result destinations are fixed at reservation, including after Return.
#[cfg(any(test, kani))]
fn check(cancel_first: bool) {
    use super::*;
    let mut slots = [Slot::EMPTY];
    with_questions(&mut slots, |questions| {
        let uncommitted = questions.reserve_third_party().unwrap();
        #[cfg(kani)]
        kani::assert(
            uncommitted.destination() == ResultsTo::ThirdParty,
            "third-party question is not a local tail source",
        );
        assert_eq!(uncommitted.destination(), ResultsTo::ThirdParty);
        drop(uncommitted);
        assert!(questions.is_empty());
        let question = questions.reserve_third_party().unwrap().accept();
        let key = question.key();
        #[cfg(kani)]
        kani::assert(
            questions.results_to(key.id()) == Ok(ResultsTo::ThirdParty),
            "third-party question is not a local tail source",
        );
        assert_eq!(questions.results_to(key.id()), Ok(ResultsTo::ThirdParty));
        assert_eq!(questions.is_redirected(key.id()), Ok(true));
        if cancel_first {
            questions.release(question);
            questions.pending_finish().unwrap().accept();
            assert_eq!(questions.results_to(key.id()), Ok(ResultsTo::ThirdParty));
            assert!(questions.reserve().is_err());
            questions.returned(key.id(), false).unwrap();
        } else {
            questions.returned(key.id(), false).unwrap();
            assert_eq!(questions.results_to(key.id()), Ok(ResultsTo::ThirdParty));
            questions.release(question);
            questions.pending_finish().unwrap().accept();
        }
        let reused = questions.reserve_redirected().unwrap().accept();
        assert_eq!(reused.key().id(), key.id());
        assert_ne!(reused.key(), key);
        assert_eq!(
            questions.results_to(reused.key().id()),
            Ok(ResultsTo::Yourself)
        );
        questions.returned(reused.key().id(), true).unwrap();
        questions.release(reused);
        let ordinary = questions.reserve().unwrap();
        assert_eq!(ordinary.destination(), ResultsTo::Caller);
        drop(ordinary);
        assert!(questions.is_empty());
    })
    .unwrap();
}
#[cfg(test)]
mod tests {
    #[test]
    fn destination_stays_bound_through_return_and_generation_reuse() {
        let measured = allocation_counter::measure(|| {
            super::check(false);
            super::check(true);
        });
        assert_eq!(measured.count_total, 0);
    }
}
#[cfg(kani)]
mod proofs {
    #[kani::proof]
    #[kani::unwind(4)]
    fn question_destination_cannot_change_on_return_or_reuse() {
        super::check(kani::any());
    }
}
