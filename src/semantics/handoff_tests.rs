use super::*;

#[test]
fn enqueue_reserves_drain_capacity_before_reporting_success() {
    let mut state = HandoffState {
        drained: u64::MAX - 1,
        ..Default::default()
    };
    assert!(state.enqueue());
    let full = state;
    assert!(!state.enqueue());
    assert_eq!(state, full);
    assert!(state.drain());
    assert_eq!(state.drained(), u64::MAX);
    let exhausted = state;
    assert!(!state.enqueue());
    assert_eq!(state, exhausted);
    assert!(state.provide());
    assert!(state.accept(true));
    assert!(state.lift());
    assert!(state.direct_ready());

    // Pending-count overflow and direct-count overflow are separate limits.
    let mut pending = HandoffState {
        pending: u64::MAX,
        ..Default::default()
    };
    let before = pending;
    assert!(!pending.enqueue());
    assert_eq!(pending, before);
    assert!(pending.drain());
    assert!(!pending.enqueue());
    let mut direct = HandoffState {
        phase: HandoffPhase::Direct,
        direct: u64::MAX,
        ..Default::default()
    };
    let before = direct;
    assert!(!direct.invoke_direct());
    assert_eq!(direct, before);
    assert!(direct.accept(true)); // An idempotent accept does not allocate a new counter slot.
}

#[test]
fn replay_tlc_handoff_counter_boundary() {
    use reproto_test_support::verification::exploration;
    const MODEL: &str = "verification/HandoffCounterBoundary.tla";
    const CONFIG: &str = include_str!("../../verification/HandoffCounterBoundary.cfg");
    // These private test fixtures represent reachable histories with MAX-2
    // completed operations. No restoration/deserialization API is exposed.
    const BASE: u64 = u64::MAX - 2;
    for kind in ["proxy", "direct"] {
        let config = CONFIG.replace("Kind = \"proxy\"", &format!("Kind = \"{kind}\""));
        let report = format!("handoff-counter-{kind}");
        let paths = exploration::traces(MODEL, &report, &config).unwrap();
        let direct = kind == "direct";
        for path in &paths {
            let mut state = HandoffState {
                phase: if direct {
                    HandoffPhase::Direct
                } else {
                    HandoffPhase::Proxying
                },
                drained: if direct { 0 } else { BASE },
                direct: if direct { BASE } else { 0 },
                ..Default::default()
            };
            for step in path {
                let before = state;
                let result = match step["event"] {
                    1 => state.enqueue(),
                    2 => state.drain(),
                    3 => state.provide(),
                    4 => state.accept(true),
                    5 => state.accept(false),
                    6 => state.lift(),
                    7 => state.invoke_direct(),
                    8 => state.revoke(),
                    _ => panic!("unknown counter action"),
                };
                assert_eq!(u64::from(result), step["result"], "{kind}: {path:?}");
                if !result {
                    assert_eq!(state, before, "rejection changed state: {path:?}");
                }
                let phase = match state.phase() {
                    HandoffPhase::Proxying => 0,
                    HandoffPhase::Offered => 1,
                    HandoffPhase::Embargoed => 2,
                    HandoffPhase::Direct => 3,
                };
                assert_eq!(phase, step["phase"]);
                assert_eq!(state.pending(), step["pending"]);
                assert_eq!(
                    state.drained() - if direct { 0 } else { BASE },
                    step["drained"]
                );
                assert_eq!(
                    state.direct_calls() - if direct { BASE } else { 0 },
                    step["direct"]
                );
                assert_eq!(u64::from(state.is_revoked()), step["revoked"]);
                assert_eq!(state.accepted(), phase >= 2);
                assert!(state.pending().checked_add(state.drained()).is_some());
            }
        }
        let faults: &[(&str, &str)] = if direct {
            &[("wrappingDirect", "ExpectedOutcome")]
        } else {
            &[
                ("unreserved", "DrainCapacity"),
                ("lostDrain", "Conservation"),
                ("earlyLift", "Embargo"),
                ("revokedAccept", "ExpectedOutcome"),
            ]
        };
        exploration::controls(MODEL, &report, &config, faults, None).unwrap();
        eprintln!(
            "{kind}: {} native u64-boundary edge-prefix replays",
            paths.len()
        );
    }
}
