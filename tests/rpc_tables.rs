//! Replay the private production table modules directly, without a duplicate
//! implementation or a public test-only runtime API. Wire behavior is exercised
//! separately by the ordinary RPC, Join and third-party integration tests.
#[allow(dead_code)]
#[path = "../crates/capntproto-rpc/src/rpc_ids.rs"]
mod rpc_ids;
#[allow(dead_code)]
#[path = "../crates/capntproto-rpc/src/rpc_tables.rs"]
mod rpc_tables;

use rpc_ids::{AnswerId, ExportId, ImportId, QuestionId, WireId};
use rpc_tables::{LocalTable, PeerTable};

#[test]
fn peer_tables_preserve_zero_and_full_wire_range_without_cross_domain_aliasing() {
    let mut answers = PeerTable::<AnswerId, _>::new();
    let mut imports = PeerTable::<ImportId, _>::new();
    for raw in [0, (1 << 30) - 1, 1 << 30, (1 << 31) - 1, 1 << 31, u32::MAX] {
        let answer = AnswerId::from_wire(raw);
        let import = ImportId::from_wire(raw);
        assert_eq!(answer.to_wire(), raw);
        assert_eq!(import.to_wire(), raw);
        answers.slots.insert(answer, "answer");
        imports.slots.insert(import, "import");
        assert_eq!(answers.slots.remove(&answer), Some("answer"));
        assert_eq!(imports.slots.get(&import), Some(&"import"));
    }
}

#[test]
fn replay_tlc_rpc_id_table_operations() {
    use capntproto_test_support::verification::exploration;
    const MODEL: &str = "verification/RpcIdTables.tla";
    const CONFIG: &str = include_str!("../verification/RpcIdTables.cfg");
    let paths = exploration::traces(MODEL, "rpc-id-tables", CONFIG).unwrap();
    for path in &paths {
        let mut questions = LocalTable::<QuestionId, u8>::new();
        let mut exports = LocalTable::<ExportId, u8>::new();
        let adopted_id = QuestionId::from_wire(1 << 30);
        let high_id = QuestionId::from_wire(1 << 31);
        let mut issued = 0;
        for state in path {
            let q_before = low_mask(&questions);
            let e_before = low_mask(&exports);
            let op = state["event"];
            let result = match op {
                1 => u64::from(questions.push(10).to_wire()) + 1,
                2 => u64::from(exports.push(20).to_wire()) + 1,
                3 | 4 => u64::from(
                    questions
                        .remove(QuestionId::from_wire((op - 3) as u32))
                        .is_some(),
                ),
                5 | 6 => u64::from(
                    exports
                        .remove(ExportId::from_wire((op - 5) as u32))
                        .is_some(),
                ),
                7 => {
                    assert_eq!(questions.push_high(30), high_id);
                    issued += 1;
                    4
                }
                8 => u64::from(questions.remove(high_id).is_some()),
                9 => {
                    // A duplicate attempts to replace the owner with another
                    // value. Rejection must preserve both the ID and its value.
                    let value = if questions.get(adopted_id).is_some() {
                        2
                    } else {
                        1
                    };
                    u64::from(questions.insert_adopted(adopted_id, value).is_ok())
                }
                10 => u64::from(questions.remove(adopted_id).is_some()),
                11 => u64::from(
                    questions
                        .insert_adopted(QuestionId::from_wire(0), 99)
                        .is_ok(),
                ),
                12 => u64::from(questions.insert_adopted(high_id, 99).is_ok()),
                _ => panic!("unknown event: {state:?}"),
            };
            let (q, e) = (low_mask(&questions), low_mask(&exports));
            let adopted = u64::from(questions.get(adopted_id).copied().unwrap_or(0));
            let h = u64::from(questions.get(high_id).is_some());
            for (name, actual) in [
                ("q", q),
                ("e", e),
                ("h", h),
                ("issued", issued),
                ("adopted", adopted),
                ("result", result),
                (
                    "beforeOther",
                    if matches!(op, 2 | 5 | 6) {
                        q_before
                    } else {
                        e_before
                    },
                ),
                ("beforeLow", q_before),
            ] {
                assert_eq!(actual, state[name], "{name}: {path:?}");
            }
            assert_eq!(questions.adopted_len(), usize::from(adopted != 0));
            assert_eq!(
                questions.iter().count(),
                q.count_ones() as usize + h as usize + adopted as usize
            );
            assert_eq!(exports.iter().count(), e.count_ones() as usize);
            assert_eq!(questions.is_empty(), q == 0 && h == 0 && adopted == 0);
            assert_eq!(exports.is_empty(), e == 0);
            for raw in 0..2 {
                if let Some(value) = questions.get(QuestionId::from_wire(raw)) {
                    assert_eq!(*value, 10);
                }
                if let Some(value) = exports.get(ExportId::from_wire(raw)) {
                    assert_eq!(*value, 20);
                }
            }
            if let Some(value) = questions.get(high_id) {
                assert_eq!(*value, 30);
            }
        }
    }
    exploration::controls(
        MODEL,
        "rpc-id-tables",
        CONFIG,
        &[
            ("reuse-live", "UniqueAllocation"),
            ("domain", "DomainIsolation"),
            ("sparse-low", "SparseIsolation"),
            ("replace-adopted", "AdoptedOwner"),
        ],
        None,
    )
    .unwrap();
    eprintln!("{} production RPC table edge-prefix replays", paths.len());
}

fn low_mask<I: rpc_ids::LocalId>(table: &LocalTable<I, u8>) -> u64 {
    (0..2)
        .filter(|raw| table.get(I::from_wire(*raw)).is_some())
        .map(|raw| 1 << raw)
        .sum()
}
