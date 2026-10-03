use capntproto::authority::{Grant, ObjectGeneration, ObjectId, Rights};

#[test]
fn exhaustive_rights_decoding_and_attenuation() {
    for bits in 0..=u8::MAX {
        let rights = Rights::from_bits(bits);
        assert_eq!(rights.is_some(), bits < 32, "{bits}");
        if let Some(rights) = rights {
            assert_eq!(rights.bits(), bits);
        }
    }
    let named = [
        Rights::GET,
        Rights::PUT,
        Rights::PUBLISH,
        Rights::SUBSCRIBE,
        Rights::DELEGATE,
    ];
    for (bit, right) in named.into_iter().enumerate() {
        assert_eq!(right.bits(), 1 << bit);
    }
    assert_eq!(Rights::ALL.bits(), 31);
    assert_eq!(Rights::VIEW.bits(), 9);
    // An independent set oracle exhausts all valid parent/child masks.
    let members = |mask: u8| {
        (0..5)
            .filter(move |bit| (mask / (1 << bit)) % 2 == 1)
            .collect::<std::collections::BTreeSet<_>>()
    };
    for parent in 0..32 {
        for child in 0..32 {
            let parent_rights = Rights::from_bits(parent).unwrap();
            let child_rights = Rights::from_bits(child).unwrap();
            let subset = members(child).is_subset(&members(parent));
            assert_eq!(parent_rights.contains(child_rights), subset);
            let root = Grant::root(
                ObjectId::new(19).unwrap(),
                ObjectGeneration::new(23).unwrap(),
                [11; 32],
                parent_rights,
            );
            assert_eq!(
                (root.object().get(), root.generation().get(), root.holder()),
                (19, 23, [11; 32])
            );
            assert_eq!(root.rights(), parent_rights);
            assert!(root.is_live());
            assert_eq!(root.allows(child_rights), subset);
            let grant = root.delegate([42; 32], child_rights);
            assert_eq!(grant.is_some(), subset && members(parent).contains(&4));
            if let Some(grant) = grant {
                assert_eq!(
                    (
                        grant.object().get(),
                        grant.generation().get(),
                        grant.holder()
                    ),
                    (19, 23, [42; 32])
                );
                assert_eq!(grant.rights(), child_rights);
                assert!(grant.is_live());
                root.revoke();
                assert!(!grant.is_live());
                assert!(!grant.allows(child_rights));
                assert!(grant
                    .delegate([7; 32], Rights::from_bits(0).unwrap())
                    .is_none());
            } else {
                root.revoke();
            }
            assert!(!root.allows(child_rights));
            assert!(root.delegate([42; 32], child_rights).is_none());
        }
    }
}

fn grant_tree() -> [Grant; 4] {
    let root = Grant::root(
        ObjectId::new(19).unwrap(),
        ObjectGeneration::new(23).unwrap(),
        [1; 32],
        Rights::ALL,
    );
    let branch = root.delegate([2; 32], Rights::ALL).unwrap();
    let leaf = branch.delegate([3; 32], Rights::VIEW).unwrap();
    let sibling = root.delegate([4; 32], Rights::VIEW).unwrap();
    [root, branch, leaf, sibling]
}

#[test]
fn replay_tlc_grant_revocation_waits() {
    use capntproto_test_support::verification::exploration;
    use std::{future::Future, pin::Pin, task::Context};
    const MODEL: &str = "verification/GrantRevocation.tla";
    const CONFIG: &str = include_str!("../verification/GrantRevocation.cfg");
    for watched in 0..4 {
        let config = CONFIG.replace("Watched = 2", &format!("Watched = {watched}"));
        let report = format!("grant-revocation-{watched}");
        let paths = exploration::traces(MODEL, &report, &config).unwrap();
        for path in &paths {
            let grants = grant_tree();
            let mut waiter: Option<Pin<Box<dyn Future<Output = ()> + '_>>> = None;
            let mut cx = Context::from_waker(std::task::Waker::noop());
            for step in path {
                match step["event"] {
                    1 => {
                        waiter = Some(Box::pin(grants[watched].when_revoked()));
                    }
                    2 => {
                        let ready = waiter.as_mut().unwrap().as_mut().poll(&mut cx).is_ready();
                        assert_eq!(u64::from(ready), step["result"], "{watched}: {path:?}");
                    }
                    3 => {
                        waiter = None;
                    }
                    4 => grants[step["target"] as usize].revoke(),
                    _ => panic!("unexpected revocation event"),
                }
                let mask = step["revoked"];
                for (index, ancestors) in
                    [&[0][..], &[0, 1], &[0, 1, 2], &[0, 3]].iter().enumerate()
                {
                    assert_eq!(
                        (
                            grants[index].object().get(),
                            grants[index].generation().get()
                        ),
                        (19, 23)
                    );
                    assert_eq!(grants[index].holder(), [index as u8 + 1; 32]);
                    let live = ancestors.iter().all(|bit| mask & (1 << bit) == 0);
                    assert_eq!(grants[index].is_live(), live);
                    assert_eq!(grants[index].allows(Rights::GET), live);
                }
            }
        }
        let live = config.replace("SPECIFICATION Spec", "SPECIFICATION FairSpec")
            + "\nPROPERTY EventuallyObserved\n";
        let faults = if watched == 2 {
            &[
                ("ownOnly", "Notification"),
                ("crossBranch", "Notification"),
                ("lostEarly", "Notification"),
                ("alwaysReady", "Notification"),
            ][..]
        } else {
            &[]
        };
        exploration::controls(MODEL, &report, &config, faults, Some(&live)).unwrap();
        eprintln!(
            "grant {watched}: {} revocation wait edge-prefix replays",
            paths.len()
        );
    }
}

#[derive(Default)]
struct Wakes(std::sync::atomic::AtomicUsize);
impl std::task::Wake for Wakes {
    fn wake(self: std::sync::Arc<Self>) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[test]
fn revocation_waits_observe_ancestors_before_and_after_registration() {
    use std::{
        future::Future,
        sync::{atomic::Ordering, Arc},
        task::{Context, Waker},
    };
    let ancestors: &[&[usize]] = &[&[0], &[0, 1], &[0, 1, 2], &[0, 3]];
    for (watched, lineage) in ancestors.iter().enumerate() {
        for revoked in 0..4 {
            for timing in 0..3 {
                let grants = grant_tree();
                let wakes = Arc::new(Wakes::default());
                let waker = Waker::from(wakes.clone());
                let mut cx = Context::from_waker(&waker);
                if timing == 0 {
                    grants[revoked].revoke();
                }
                let mut wait = Box::pin(grants[watched].when_revoked());
                if timing == 1 {
                    grants[revoked].revoke();
                }
                if timing == 2 {
                    assert!(wait.as_mut().poll(&mut cx).is_pending());
                    grants[revoked].clone().revoke();
                    assert_eq!(
                        wakes.0.load(Ordering::Relaxed) > 0,
                        lineage.contains(&revoked)
                    );
                }
                assert_eq!(
                    wait.as_mut().poll(&mut cx).is_ready(),
                    lineage.contains(&revoked)
                );
                assert_eq!(grants[watched].is_live(), !lineage.contains(&revoked));
                // Cancelling a waiter has no authority effect. A replacement
                // observes persisted revocation without needing another signal.
                drop(wait);
                let mut replacement = Box::pin(grants[watched].when_revoked());
                assert_eq!(
                    replacement.as_mut().poll(&mut cx).is_ready(),
                    lineage.contains(&revoked)
                );
                drop(replacement);
                let mut replacement = Box::pin(grants[watched].when_revoked());
                grants[watched].revoke();
                assert!(replacement.as_mut().poll(&mut cx).is_ready());
                grants[watched].revoke(); // Revocation is idempotent.
                assert!(!grants[watched].is_live());
            }
        }
    }
}
#[test]
fn attenuation_and_branch_revocation() {
    let root = Grant::root(
        ObjectId::new(1).unwrap(),
        ObjectGeneration::new(1).unwrap(),
        [1; 32],
        Rights::ALL,
    );
    let first = root.delegate([2; 32], Rights::VIEW).unwrap();
    let second = root.delegate([3; 32], Rights::VIEW).unwrap();
    assert!(first.allows(Rights::GET));
    assert!(!first.allows(Rights::PUT));
    assert!(first.delegate([4; 32], Rights::ALL).is_none());
    first.revoke();
    assert!(!first.allows(Rights::GET));
    assert!(second.allows(Rights::GET));
    root.revoke();
    assert!(!second.allows(Rights::GET));
}
#[test]
fn embargo_and_replay() {
    let mut h = capntproto::semantics::HandoffState::default();
    assert_eq!(h.pending(), 0);
    assert_eq!(h.direct_calls(), 0);
    assert!(!h.accepted());
    assert!(!h.is_revoked());
    assert!(h.enqueue());
    assert_eq!(h.pending(), 1);
    assert!(h.enqueue());
    assert_eq!(h.pending(), 2);
    assert!(h.provide());
    let old = h;
    assert!(!h.accept(false));
    assert_eq!(h, old);
    assert!(h.accept(true));
    assert!(!h.lift());
    assert!(!h.invoke_direct());
    assert!(h.drain());
    assert!(!h.lift());
    assert!(h.drain());
    assert!(h.lift());
    assert!(h.invoke_direct());
    assert_eq!(h.direct_calls(), 1);
    assert!(h.invoke_direct());
    assert_eq!(h.direct_calls(), 2);
    assert!(h.accept(true));
    assert!(h.accepted());
    assert!(h.revoke());
    assert!(!h.invoke_direct());
    assert!(!h.accept(true));
}

#[test]
fn revocation_preserves_phase_and_allows_only_pending_cleanup() {
    use capntproto::semantics::{HandoffPhase, HandoffState};
    for phase in [
        HandoffPhase::Proxying,
        HandoffPhase::Offered,
        HandoffPhase::Embargoed,
        HandoffPhase::Direct,
    ] {
        let mut state = HandoffState::default();
        assert!(state.enqueue());
        if phase != HandoffPhase::Proxying {
            assert!(state.provide());
        }
        if matches!(phase, HandoffPhase::Embargoed | HandoffPhase::Direct) {
            assert!(state.accept(true));
        }
        if phase == HandoffPhase::Direct {
            assert!(state.drain());
            assert!(state.lift());
        }
        assert_eq!(state.phase(), phase);
        let accepted = state.accepted();
        assert!(state.revoke());
        let revoked = state;
        assert!(!state.enqueue());
        assert!(!state.provide());
        assert!(!state.accept(true));
        assert!(!state.accept(false));
        assert!(!state.lift());
        assert!(!state.invoke_direct());
        assert!(!state.revoke());
        assert_eq!(state, revoked);
        assert!(!state.direct_ready());
        assert_eq!(state.drain(), phase != HandoffPhase::Direct);
        assert_eq!(state.pending(), 0);
        assert_eq!(state.drained(), 1);
        assert_eq!(state.phase(), phase);
        assert_eq!(state.accepted(), accepted);
        assert!(state.is_revoked());
    }
}

#[test]
fn native_stream_gate_enforces_roles_and_terminal_preface_failure() {
    use capntproto::semantics::{NativeStreamGate, NativeStreamState as State, StreamRole};
    for role in [StreamRole::Initiator, StreamRole::Responder] {
        let mut gate = NativeStreamGate::new(role);
        let initial = gate;
        assert!(!gate.ready());
        assert!(!gate.sent());
        assert!(!gate.receive(b'R'));
        assert!(!gate.receive(b'!'));
        assert_eq!(gate, initial);
        gate.authenticate();
        if role == StreamRole::Initiator {
            assert_eq!(gate.state(), State::SendPreface);
            assert!(!gate.receive(b'R'));
            assert!(!gate.receive(b'!'));
            assert!(gate.sent());
        } else {
            assert_eq!(gate.state(), State::ReceivePreface);
            assert!(!gate.sent());
            assert!(gate.receive(b'R'));
        }
        assert_eq!(gate.state(), State::Ready);
        gate.authenticate();
        assert!(!gate.sent());
        assert!(!gate.receive(b'R'));
        assert!(!gate.receive(b'!'));
        assert!(gate.ready());
        assert!(!gate.needs_send() && !gate.needs_receive());
    }
    let mut gate = NativeStreamGate::new(StreamRole::Responder);
    gate.authenticate();
    assert!(!gate.receive(b'!'));
    let failed = gate;
    assert_eq!(gate.state(), State::Failed);
    gate.authenticate();
    assert!(!gate.sent());
    assert!(!gate.receive(b'R'));
    assert!(!gate.receive(b'!'));
    assert_eq!(gate, failed);
    assert!(!gate.ready() && !gate.needs_send() && !gate.needs_receive());
}
