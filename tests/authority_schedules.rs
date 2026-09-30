use reproto::authority::{Grant, ObjectGeneration, ObjectId, Rights};
use reproto_test_support::schedules::{self, block_on, spawn_local, yield_now, Trace};
use std::{cell::Cell, rc::Rc};

fn notifications(trace: Trace) {
    block_on(async {
        let root = Grant::root(
            ObjectId::new(7).unwrap(),
            ObjectGeneration::new(11).unwrap(),
            [1; 32],
            Rights::ALL,
        );
        let branch = root.delegate([2; 32], Rights::ALL).unwrap();
        let leaf = branch.delegate([3; 32], Rights::VIEW).unwrap();
        let sibling = root.delegate([4; 32], Rights::VIEW).unwrap();
        let revoked = Rc::new(Cell::new(0u8));
        let (abort, registration) = futures::future::AbortHandle::new_pair();
        let mut registration = Some(registration);
        let mut waits = Vec::new();
        for (id, grant, mask) in [(0, leaf.clone(), 3), (1, leaf.clone(), 3), (2, sibling, 1)] {
            let registration = registration.take();
            let revoked = revoked.clone();
            let trace = trace.clone();
            waits.push(spawn_local(async move {
                let mut waiting = Box::pin(grant.when_revoked());
                let future = futures::future::poll_fn(|cx| {
                    let result = std::future::Future::poll(waiting.as_mut(), cx);
                    trace.event(&format!("poll-{id}"), u64::from(result.is_ready()));
                    if result.is_ready() {
                        assert_ne!(revoked.get() & mask, 0, "premature branch notification");
                    }
                    result
                });
                if let Some(registration) = registration {
                    let result = futures::future::Abortable::new(future, registration).await;
                    trace.event("cancel-result", u64::from(result.is_ok()));
                    drop(waiting);
                    grant.when_revoked().await;
                    trace.event("replacement", 1);
                } else {
                    future.await;
                }
                assert!(!grant.is_live());
                assert!(!grant.allows(Rights::GET));
                assert!(grant.delegate([5; 32], Rights::VIEW).is_none());
                trace.event("done", id);
            }));
        }
        let cancellation = {
            let trace = trace.clone();
            spawn_local(async move {
                yield_now().await;
                trace.event("cancel", 0);
                abort.abort();
            })
        };
        let revocation = {
            let revoked = revoked.clone();
            let trace = trace.clone();
            spawn_local(async move {
                yield_now().await;
                revoked.set(2);
                trace.event("revoke-branch", 2);
                branch.revoke();
                yield_now().await;
                revoked.set(3);
                trace.event("revoke-root", 3);
                root.revoke();
            })
        };
        cancellation.await.unwrap();
        revocation.await.unwrap();
        for wait in waits {
            wait.await.unwrap();
        }
        assert!(!leaf.is_live());
        trace.event("complete", 1);
    });
}

#[test]
fn shuttle_grant_waiters_cancel_replace_and_preserve_sibling_isolation() {
    let batch = schedules::check("grant-waiters", notifications);
    if !batch.sampled {
        return;
    }
    let runs = batch.runs;
    let outcomes: std::collections::BTreeSet<_> = runs
        .iter()
        .flat_map(|run| &run.events)
        .filter_map(|(name, value)| (name == "cancel-result").then_some(*value))
        .collect();
    assert_eq!(outcomes, [0, 1].into_iter().collect());
    assert!(runs.iter().any(|run| {
        let cancel = run
            .events
            .iter()
            .position(|(name, _)| name == "cancel")
            .unwrap();
        run.events[..cancel].contains(&("poll-0".into(), 0))
            && run.events.contains(&("cancel-result".into(), 0))
    }));
    assert!(runs.iter().any(|run| {
        let cancel = run
            .events
            .iter()
            .position(|(event, _)| event == "cancel")
            .unwrap();
        !run.events[..cancel]
            .iter()
            .any(|(event, _)| event == "poll-0")
            && run.events.contains(&("cancel-result".into(), 0))
    }));
}
