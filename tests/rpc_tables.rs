//! Replay the production tables and shared identifiers directly, without a
//! duplicate implementation or a test-only runtime API. Wire behavior is exercised
//! separately by the ordinary RPC, Join and third-party integration tests.
use capntproto_rpc_engine::ids as rpc_ids;
use std::collections;
#[allow(dead_code)]
#[path = "../crates/capntproto-rpc/src/rpc_tables.rs"]
mod rpc_tables;

use rpc_ids::{AnswerId, ImportId, WireId};
use rpc_tables::PeerTable;

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
