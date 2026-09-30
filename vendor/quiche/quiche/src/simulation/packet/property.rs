// Copyright (C) 2026, ReProto contributors.
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are
// met:
//
//     * Redistributions of source code must retain the above copyright notice,
//       this list of conditions and the following disclaimer.
//
//     * Redistributions in binary form must reproduce the above copyright
//       notice, this list of conditions and the following disclaimer in the
//       documentation and/or other materials provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS
// IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO,
// THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
// PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR
// CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
// EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
// PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
// PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
// LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
// NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
// SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

//! Shrinkable delivery decisions use queue-relative choices, never packet IDs
//! from an earlier execution. Removing a decision therefore cannot introduce
//! a nonexistent-packet failure. Fresh connections are built for every trial.

use super::*;
use proptest::prelude::*;
use proptest::test_runner::Config;
use proptest::test_runner::RngAlgorithm;
use proptest::test_runner::RngSeed;
use proptest::test_runner::TestError;
use proptest::test_runner::TestRunner;
use std::cell::RefCell;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
enum Treatment {
    Deliver,
    Drop,
    Corrupt,
    Duplicate,
    CorruptCopy,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct Decision {
    pick: u8,
    treatment: Treatment,
    delay_ms: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Case {
    format: u32,
    seed: u64,
    cc: String,
    psk: bool,
    after_handshake: bool,
    decisions: Vec<Decision>,
}

fn losses(decisions: &[Decision]) -> usize {
    decisions
        .iter()
        .filter(|d| matches!(d.treatment, Treatment::Drop | Treatment::Corrupt))
        .count()
}

impl Case {
    fn validate(&self) {
        assert_eq!(self.format, 1, "unsupported packet property format");
        assert!(matches!(self.cc.as_str(), "cubic" | "bbr2_gcongestion"));
        assert!(self.decisions.len() <= 12);
        assert!(self.decisions.iter().all(|d| d.delay_ms <= 3));
        assert!(losses(&self.decisions) <= 2);
    }
}

fn decisions() -> impl Strategy<Value = Vec<Decision>> {
    let treatment = prop_oneof![
        4 => Just(Treatment::Deliver),
        1 => Just(Treatment::Drop),
        1 => Just(Treatment::Corrupt),
        2 => Just(Treatment::Duplicate),
        2 => Just(Treatment::CorruptCopy),
    ];
    proptest::collection::vec(
        (any::<u8>(), treatment, 0u8..4).prop_map(
            |(pick, treatment, delay_ms)| Decision {
                pick,
                treatment,
                delay_ms,
            },
        ),
        0..13,
    )
    .prop_filter("at most two destructive packet losses", |v| losses(v) <= 2)
}

fn strategy(
    cc: &str, psk: bool, after_handshake: bool,
) -> impl Strategy<Value = Case> + use<> {
    let cc = cc.to_owned();
    (any::<u64>(), decisions()).prop_map(move |(seed, decisions)| Case {
        format: 1,
        seed,
        cc: cc.clone(),
        psk,
        after_handshake,
        decisions,
    })
}

fn runner(cases: u32) -> TestRunner {
    TestRunner::new(Config {
        cases,
        rng_algorithm: RngAlgorithm::ChaCha,
        rng_seed: RngSeed::Fixed(0x726570726f746f),
        max_shrink_iters: 1024,
        // Save the actual minimized input, not a generator-version-dependent
        // seed. The Cargo gate retains all successful inputs and build identity.
        failure_persistence: None,
        ..Config::default()
    })
}

#[derive(Serialize)]
struct Evidence {
    case: Case,
    events: usize,
    applied: usize,
    treatments: [usize; 5],
    report_sha256: String,
}

fn execute(case: &Case, persist: bool) -> (Report, usize, [usize; 5]) {
    case.validate();
    let mut machine = Machine::new(case.seed, &case.cc, case.psk);
    machine.persist_report = persist;
    machine.report.scenario = "property".into();
    machine.report.completion_generation = Some(0);
    if case.after_handshake {
        // Reach authenticated, confirmed peers before this fault window.
        // Recovery's confirm action drives the real handshake/timers.
        machine.recovery_step(7);
        assert!(machine.peers.iter().all(|p| p.is_established()));
    }
    let mut written = [false; 2];
    let mut applied = 0;
    let mut treatments = [0; 5];
    for _ in 0..400 {
        if machine.peers.iter().all(|p| p.is_established()) &&
            machine.observation().verified &&
            !written[0]
        {
            machine.step(Action::Write { server: false });
            written[0] = true;
        }
        if machine.finished[1] && !written[1] {
            machine.step(Action::Write { server: true });
            written[1] = true;
        }
        for server in [false, true] {
            machine.step(Action::Emit { server });
        }
        if machine.pending.is_empty() {
            if machine.finished == [true; 2] {
                break;
            }
            machine.timer();
            continue;
        }
        let decision = case.decisions.get(applied);
        let index =
            decision.map_or(0, |d| usize::from(d.pick) % machine.pending.len());
        let packet = machine.pending[index].clone();
        machine.advance(
            packet.at + u64::from(decision.map_or(0, |d| d.delay_ms)) * 1_000_000,
        );
        let treatment = decision.map_or(Treatment::Deliver, |d| d.treatment);
        if decision.is_some() {
            applied += 1;
            treatments[treatment as usize] += 1;
        }
        let id = packet.id;
        match treatment {
            Treatment::Drop => machine.step(Action::Drop { id }),
            Treatment::Corrupt => machine.step(Action::Corrupt { id }),
            Treatment::Deliver => machine.step(Action::Deliver { id }),
            Treatment::Duplicate | Treatment::CorruptCopy => {
                machine.step(Action::Duplicate { id });
                if treatment == Treatment::CorruptCopy {
                    machine.step(Action::Corrupt {
                        id: machine.next_id - 1,
                    });
                }
                machine.step(Action::Deliver { id });
            },
        }
    }
    machine.verify_complete();
    assert!(machine
        .peers
        .iter()
        .all(|p| p.is_established() && !p.is_closed()));
    (machine.report.clone(), applied, treatments)
}

fn check(case: Case) -> Evidence {
    let (report, applied, treatments) = execute(&case, false);
    replay_with_persistence(&report, false);
    let digest = ring::digest::digest(
        &ring::digest::SHA256,
        &serde_json::to_vec(&report).unwrap(),
    );
    Evidence {
        case,
        events: report.records.len(),
        applied,
        treatments,
        report_sha256: digest
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    }
}

fn save_failure(case: &Case) -> std::path::PathBuf {
    let directory = std::env::var_os("REPROTO_NOISE_PROPERTY_FAILURE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("reproto-noise-properties"));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!(
        "{}-{}-{}-{}.json",
        case.cc, case.psk, case.after_handshake, case.seed
    ));
    std::fs::write(&path, serde_json::to_vec_pretty(case).unwrap()).unwrap();
    path
}

#[test]
fn generated_packet_properties() {
    if let Some(path) = std::env::var_os("REPROTO_NOISE_PROPERTY_REPLAY") {
        let case: Case =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        check(case);
        return;
    }
    let cases = std::env::var("REPROTO_NOISE_PROPERTY_CASES")
        .map(|v| v.parse::<u32>().unwrap())
        .unwrap_or(16);
    assert!((1..=4096).contains(&cases));
    let evidence = RefCell::new(Vec::new());
    for cc in ["cubic", "bbr2_gcongestion"] {
        for psk in [false, true] {
            for after_handshake in [false, true] {
                let result = runner(cases).run(
                    &strategy(cc, psk, after_handshake),
                    |case| {
                        evidence.borrow_mut().push(check(case));
                        Ok(())
                    },
                );
                if let Err(error) = result {
                    if let TestError::Fail(_, case) = &error {
                        let path = save_failure(case);
                        // Persist the final reduced packet log as well. Failed
                        // intermediate shrink trials do not overwrite artifacts.
                        let _ = std::panic::catch_unwind(|| execute(case, true));
                        eprintln!(
                            "replay with REPROTO_NOISE_PROPERTY_REPLAY={}",
                            path.display()
                        );
                    }
                    panic!("Noise packet property failed: {error}");
                }
            }
        }
    }
    let evidence = evidence.into_inner();
    assert_eq!(evidence.len(), cases as usize * 8);
    if let Some(path) = std::env::var_os("REPROTO_NOISE_PROPERTY_REPORT") {
        std::fs::write(path, serde_json::to_vec(&evidence).unwrap()).unwrap();
    }
    println!(
        "Noise packet properties: {} generated cases replayed",
        evidence.len()
    );
}

#[test]
fn shrinking_preserves_valid_decisions_and_failure() {
    // A synthetic failure checks the shrink pipeline, not protocol correctness
    // or implementation mutation coverage. Queue-relative choices remain valid
    // even when earlier decisions disappear.
    let result = runner(256).run(&decisions(), |v| {
        if v.iter().any(|d| d.treatment == Treatment::Duplicate) &&
            v.iter().any(|d| d.treatment == Treatment::CorruptCopy)
        {
            Err(TestCaseError::fail("synthetic pair"))
        } else {
            Ok(())
        }
    });
    let TestError::Fail(reason, minimal) = result.unwrap_err() else {
        panic!("no reduced failure")
    };
    assert_eq!(reason.to_string(), "synthetic pair");
    assert_eq!(minimal.len(), 2);
    assert!(minimal.iter().all(|d| d.pick == 0 && d.delay_ms == 0));
    assert!(minimal.iter().any(|d| d.treatment == Treatment::Duplicate));
    assert!(minimal
        .iter()
        .any(|d| d.treatment == Treatment::CorruptCopy));
    for cc in ["cubic", "bbr2_gcongestion"] {
        for psk in [false, true] {
            for after_handshake in [false, true] {
                let case = Case {
                    format: 1,
                    seed: 0,
                    cc: cc.into(),
                    psk,
                    after_handshake,
                    decisions: minimal.clone(),
                };
                // These decisions cause no real protocol failure. This positive
                // control detects invalid shrink inputs masquerading as bugs.
                let case =
                    serde_json::from_slice(&serde_json::to_vec(&case).unwrap())
                        .unwrap();
                check(case);
            }
        }
    }
}

#[test]
fn replay_rejects_incomplete_generation() {
    let mut report = run(42, "cubic", false);
    // Both FINs from generation zero cannot satisfy a lifecycle report that
    // promises completion after replacing both connections.
    report.completion_generation = Some(1);
    let failure =
        std::panic::catch_unwind(|| replay_with_persistence(&report, false))
            .unwrap_err();
    let message = failure.downcast_ref::<String>().unwrap();
    assert!(message.contains("wrong completion generation"));
}

#[test]
fn packet_property_corpus() {
    // Versioned seed cases remain runnable if generation or shrinking changes.
    // Add minimized real failures here after fixing their underlying defects.
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("property-corpus.json")).unwrap();
    assert!(!cases.is_empty());
    for case in cases {
        check(case);
    }
}
