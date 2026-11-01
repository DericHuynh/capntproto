//! Compile external consumers of public APIs and the exact private RPC table
//! modules. Check exact Rust diagnostics after a positive control; all
//! orchestration is an ordinary Cargo integration test.
use capntproto_test_support::verification::{self as v, command, root, run};
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs};

#[path = "api_contracts/rpc_reply.rs"]
mod rpc_reply;

#[test]
fn bounded_result_publication_compile_contracts() {
    let prelude = "use capnp::{message::FixedBuilder, word, Word};";
    let setup = "let mut words = [word(0,0,0,0,0,0,0,0); 8]; let mut builder = FixedBuilder::new(&mut words).unwrap(); builder.try_set_root_text(\"live\").unwrap();";
    let mut cases = vec![("positive".into(), format!("{prelude} fn main() {{ {setup} let segment = builder.into_segment(); let segments = [segment]; let reader = capnp::message::Reader::new(&segments[..], Default::default()); assert_eq!(reader.get_root::<capnp::text::Reader<'_>>().unwrap(), \"live\"); }}"), None)];
    for (name, code, error) in [
        ("publish-builder-once", "builder.into_segment(); builder.into_segment();", "E0382"),
        ("publish-borrowed-builder", "let text = builder.get_root_as_reader::<capnp::text::Reader<'_>>().unwrap(); builder.into_segment(); assert!(!text.is_empty());", "E0505"),
        ("reuse-published-buffer", "let segment = builder.into_segment(); let _ = Word::words_to_bytes_mut(&mut words); assert!(!segment.is_empty());", "E0499"),
    ] { cases.push((name.into(), format!("{prelude} fn main() {{ {setup} {code} }}"), Some(error))); }
    let prelude = "use capntproto_rpc_engine::messages::{self, Buffer, SendSlot};";
    let setup = "let mut a = [0u8; 4]; let mut b = [0u8; 4]; let mut buffers = [Buffer::new(&mut a), Buffer::new(&mut b)]; let mut sends = [SendSlot::EMPTY]; messages::with_messages(&mut buffers, &mut sends, |messages| { let mut writer = messages.reserve(4).unwrap(); let mut view = messages.write_borrowed(&mut writer).unwrap();";
    for (name, code, error) in [
        (
            "seal-borrowed-result",
            "let _ = messages.seal(writer, 4); view[0] = 1;",
            "E0505",
        ),
        (
            "discard-borrowed-result",
            "messages.discard(writer); view[0] = 1;",
            "E0505",
        ),
        (
            "second-mutable-result-view",
            "let _second = messages.write_borrowed(&mut writer); view[0] = 1;",
            "E0499",
        ),
    ] {
        cases.push((
            name.into(),
            format!("{prelude} fn main() {{ {setup} {code} }}).unwrap(); }}"),
            Some(error),
        ));
    }
    check_contracts("bounded-result-publication-contracts", &format!("capnp = {{ package = \"capntproto-core\", path = {:?}, default-features = false }}\ncapntproto-rpc-engine = {{ path = {:?} }}\n", root().join("crates/capntproto-core"), root().join("crates/capntproto-rpc-engine")), cases,
        "consuming fixed-segment publication and writer-bound mutable views prevent reuse, mutation and publication while borrowed",
        "wire validity, framing, capability ownership and transport admission are separate checks");
}

#[test]
fn bounded_message_lease_compile_contracts() {
    let prelude =
        "use capntproto_rpc_engine::messages::{with_messages, Buffer, SendSlot, Lease, SendKey};\n";
    let setup = "let mut bytes = [0; 8]; let mut buffers = [Buffer::new(&mut bytes)]; let mut sends = [SendSlot::EMPTY]; with_messages(&mut buffers, &mut sends, |messages| { let writer = messages.reserve(8).unwrap(); messages.write(&writer).copy_from_slice(b\"retained\"); let lease = messages.seal(writer, 8).unwrap(); let offer = messages.offer(&lease).unwrap(); let key = messages.accept(offer);";
    let end = "}).unwrap();";
    let mut cases = vec![("positive".into(), format!("{prelude} fn main() {{ {setup} let copy = messages.retain(&lease).unwrap(); messages.release(lease); assert_eq!(messages.send_window(key).unwrap().advance(8), Ok(true)); assert_eq!(&*messages.read(&copy), b\"retained\"); messages.release(copy); let writer = messages.reserve(8).unwrap(); messages.discard(writer); {end} }}"), None)];
    for (name, code, diagnostic) in [
        ("double-release", "messages.release(lease); messages.release(lease);", "E0382"),
        ("unchecked-clone", "let _ = lease.clone();", "E0599"),
        ("sealed-write", "let _ = messages.write(&lease);", "E0308"),
        ("private-buffer-key", "let _ = lease.key;", "E0616"),
        ("live-view-release", "let bytes = messages.read(&lease); messages.release(lease); assert_eq!(bytes.len(), 8);", "E0505"),
        ("writer-after-seal", "messages.discard(writer);", "E0382"),
        ("live-write-seal", "let writer = messages.reserve(0).unwrap(); let mut bytes = messages.write(&writer); let _ = messages.seal(writer, 0); bytes[0] = 1;", "E0499"),
        ("double-accept", "let _ = messages.accept(offer);", "E0382"),
        ("reader-is-not-send-key", "let _: SendKey<'_> = lease;", "E0308"),
        ("double-window-completion", "let window = messages.send_window(key).unwrap(); window.advance(1).unwrap(); window.advance(1).unwrap();", "E0382"),
        ("live-send-bytes", "let window = messages.send_window(key).unwrap(); let bytes = window.bytes(); window.advance(8).unwrap(); assert_eq!(bytes.len(), 8);", "E0505"),
        ("failure-during-window", "let window = messages.send_window(key).unwrap(); messages.fail_send(key).unwrap(); window.advance(8).unwrap();", "E0499"),
        ("unchecked-capacity", "messages.reserve(8);", "unused_must_use"),
        ("cross-pool-reader", "let mut bytes = [0; 8]; let mut other = [Buffer::new(&mut bytes)]; with_messages(&mut other, &mut [], |other| { other.release(lease); }).unwrap();", "E0521"),
        ("cross-pool-send", "let mut bytes = [0; 8]; let mut other = [Buffer::new(&mut bytes)]; with_messages(&mut other, &mut [], |other| { let _ = other.fail_send(key); }).unwrap();", "E0521"),
    ] {
        cases.push((name.into(), format!("#![deny(unused_must_use)]\n{prelude} fn main() {{ {setup} {code} {end} }}"), Some(diagnostic)));
    }
    for (name, token) in [
        ("escape-reader", "lease"),
        ("escape-send", "key"),
        ("escape-writer", "messages.reserve(0).unwrap()"),
    ] {
        cases.push((name.into(), format!("{prelude} fn main() {{ let mut escaped = None; {setup} escaped = Some({token}); {end} drop(escaped); }}"), Some("E0521")));
    }
    check_contracts("bounded-message-contracts", &format!("capntproto-rpc-engine = {{ path = {:?} }}\n", root().join("crates/capntproto-rpc-engine")), cases,
        "invariant pool scopes, exclusive writers, immutable retained views, distinct reader/send roles and consumed completion windows",
        "drivers still validate framing and IO results, bind keys to the correct stream and quiesce IO before terminal teardown");
}

#[test]
fn bounded_reference_release_compile_contracts() {
    let prelude =
        "use capntproto_rpc_engine::references::{ExportRefs, ImportRefs, ReleaseTicket};\n";
    let setup = "let mut refs = ImportRefs::new(); refs.receive(3).unwrap();";
    let mut cases = vec![("positive".into(), format!("{prelude} fn main() {{ {setup} {{ let ticket = refs.pending_release().unwrap(); assert_eq!(ticket.count(), 3); }} assert_eq!(refs.count(), 3); refs.pending_release().unwrap().commit(); assert_eq!(refs.count(), 0); }}"), None)];
    for (name, code, diagnostic) in [
        ("duplicate-commit", "let ticket = refs.pending_release().unwrap(); ticket.commit(); ticket.commit();", "E0382"),
        ("receive-while-releasing", "let ticket = refs.pending_release().unwrap(); refs.receive(1).unwrap(); ticket.commit();", "E0499"),
        ("duplicate-ticket", "let first = refs.pending_release().unwrap(); let second = refs.pending_release().unwrap(); first.commit(); second.commit();", "E0499"),
        ("private-count", "refs.count = 0;", "E0616"),
        ("forge-ticket", "let _ticket = ReleaseTicket { refs: &mut refs };", "E0451"),
        ("reference-domain", "let _: ExportRefs = refs;", "E0308"),
        ("ignored-overflow", "refs.receive(u32::MAX);", "unused_must_use"),
    ] {
        cases.push((name.into(), format!("#![deny(unused_must_use)]\n{prelude} fn main() {{ {setup} {code} }}"), Some(diagnostic)));
    }
    check_contracts("bounded-reference-contracts", &format!("capntproto-rpc-engine = {{ path = {:?} }}\n", root().join("crates/capntproto-rpc-engine")), cases,
        "private wire counts, distinct import/export roles, checked overflow and exclusive single-use cleanup acceptance",
        "does not prove local capability ownership, wire IDs/generations, output queue admission or delivery");
}

#[test]
fn fixed_buffer_builder_compile_contracts() {
    let prelude = r#"
use capnp::{message::FixedBuilder, word};
use capntproto_test_support::allocation_capnp::api::{BoundedRecord, Child, Collections, Entry};
"#;
    let setup = "let mut words = [word(0,0,0,0,0,0,0,0); 32]; let mut message = FixedBuilder::new(&mut words).unwrap();";
    let mut cases = vec![("positive".into(), format!("{prelude} fn main() {{ {setup} let mut root = message.try_init_root::<BoundedRecord>().unwrap(); root.scalar(BoundedRecord::ID).set(8); root.field(BoundedRecord::TITLE).copy_from(\"bounded\").unwrap(); root.field(BoundedRecord::CHILD).init().unwrap().scalar(Child::ID).set(3); assert_eq!(root.read().id(), 8); }}"), None)];
    for (name, code, diagnostic) in [
        ("generic-legacy-editor", "type B = capntproto_test_support::allocation_capnp::api::Box<Entry>; let mut root = message.try_init_root::<B>().unwrap(); let _ = root.field(B::VALUE).typed().init().unwrap().into_raw();", "E0599"),
        ("generic-wrong-kind", "type B = capntproto_test_support::allocation_capnp::api::Box<capnp::text::Owned>; let mut root = message.try_init_root::<B>().unwrap(); let _ = root.field(B::VALUE).typed().cast::<capnp::field_api::Data>();", "E0599"),
        ("any-live-view", "type B = capntproto_test_support::allocation_capnp::api::Box<capnp::any_pointer::Owned>; let mut root = message.try_init_root::<B>().unwrap(); let value = root.field(B::VALUE).typed().read().unwrap(); root.field(B::VALUE).clear(); assert!(value.is_null());", "E0499"),
        ("group-schema", "let mut root = message.try_init_root::<BoundedRecord>().unwrap(); let _ = root.group(Collections::DETAILS);", "E0308"),
        ("void-schema", "let mut root = message.try_init_root::<Child>().unwrap(); let _ = root.void(Collections::IDLE);", "E0308"),
        ("list-legacy-editor", "let mut list = message.try_init_root_list::<Entry>(1).unwrap(); let _ = list.get_mut(0).unwrap().into_raw();", "E0599"),
        ("nested-list-legacy-editor", "let mut list = message.try_init_root_list::<capnp::field_api::List<u32>>(1).unwrap(); let _ = list.get_mut(0).unwrap().ensure();", "E0599"),
        ("list-live-element", "let mut list = message.try_init_root_list::<Entry>(1).unwrap(); let mut item = list.get_mut(0).unwrap(); let _ = list.get_mut(0); item.scalar(Entry::ID).set(4);", "E0499"),
        ("scalar-schema", "let mut root = message.try_init_root::<BoundedRecord>().unwrap(); root.scalar(Child::ID).set(1);", "E0308"),
        ("pointer-schema", "let mut root = message.try_init_root::<Child>().unwrap(); root.field(BoundedRecord::TITLE);", "E0308"),
        ("legacy-editor", "let root = message.try_init_root::<BoundedRecord>().unwrap(); let _ = root.into_raw();", "E0599"),
        ("private-editor", "let root = message.try_init_root::<BoundedRecord>().unwrap(); let _ = root.raw;", "E0616"),
        ("implicit-upgrade", "let mut root = message.try_init_root::<BoundedRecord>().unwrap(); let _ = root.field(BoundedRecord::CHILD).ensure();", "E0599"),
        ("live-view-reset", "message.try_set_root_text(\"live\").unwrap(); let view = message.get_root_as_reader::<capnp::text::Reader<'_>>().unwrap(); message.reset(); assert_eq!(view, \"live\");", "E0502"),
        ("live-payload-edit", "let mut root = message.try_init_root::<BoundedRecord>().unwrap(); let bytes = root.field(BoundedRecord::PAYLOAD).init(8).unwrap(); root.scalar(BoundedRecord::ID).set(1); bytes[0] = 7;", "E0499"),
        ("ignored-exhaustion", "message.try_set_root_text(\"check capacity\");", "unused_must_use"),
    ] {
        cases.push((name.into(), format!("#![deny(unused_must_use)]\n{prelude} fn main() {{ {setup} {code} }}"), Some(diagnostic)));
    }
    check_contracts("fixed-builder-contracts", &format!(
        "capnp = {{ package = \"capntproto-core\", path = {:?}, default-features = false }}\ncapntproto-test-support = {{ path = {:?} }}\n",
        root().join("crates/capntproto-core"), root().join("test-support")
    ), cases, "typed scalar/pointer descriptors, no legacy growing editor escape, exclusive views and checked growth results", "not a proof of arbitrary user Schema/Scalar implementations or complete bounded RPC");
}

#[test]
fn rpc_identifier_and_table_compile_contracts() {
    // Compile hosted tables with the shared engine's identifiers. The parent
    // has the same visibility as rpc.rs; no test-only runtime API is needed.
    let prelude = format!(
        "#![allow(dead_code)]\nuse capntproto_rpc_engine::ids as rpc_ids;\nuse capntproto_rpc_engine::peer_table as bounded;\n#[path = {:?}] mod rpc_tables;\nuse rpc_ids::*;\nuse rpc_tables::*;\n",
        root().join("crates/capntproto-rpc/src/rpc_tables.rs")
    );
    let mut cases = vec![(
        "positive".to_owned(),
        format!(
            "{prelude}{}",
            r#"
fn main() {
    let mut q = LocalTable::<QuestionId, ()>::new();
    let id = q.push(());
    let _: u32 = id.to_wire();
    let _ = q.find(id);
    let _ = q.get(id);
    let _ = q.remove(id);
    q.erase(id);
    let _ = q.push_high(());
    let _ = q.insert_adopted(QuestionId::from_wire(1 << 30), ());
    let mut e = LocalTable::<ExportId, ()>::new();
    let _ = e.push(());
    let mut b = LocalTable::<EmbargoId, ()>::new();
    let _ = b.push(());
    let mut a = PeerTable::<AnswerId, ()>::new();
    a.slots.insert(AnswerId::from_wire(0), ());
    let mut i = PeerTable::<ImportId, ()>::new();
    i.slots.insert(ImportId::from_wire(u32::MAX), ());
    let mut slots = [const { bounded::Slot::EMPTY }; 2];
    let mut bounded = bounded::PeerTable::<AnswerId, (), _, 0>::try_new(
        bounded::BorrowedSparse::new(&mut slots)
    ).expect("fresh storage");
    bounded.try_insert_unique(AnswerId::from_wire(u32::MAX), ()).unwrap();
}
"#
        ),
        None,
    )];
    let domains = [
        "QuestionId",
        "AnswerId",
        "ExportId",
        "ImportId",
        "EmbargoId",
    ];
    for from in domains {
        for to in domains {
            if from != to {
                cases.push((
                    format!("{from}-as-{to}"),
                    format!("{prelude}fn main() {{ let _: {to} = {from}::from_wire(0); }}"),
                    Some("E0308"),
                ));
            }
        }
    }
    for (domain, wrong) in [
        ("QuestionId", "AnswerId"),
        ("ExportId", "ImportId"),
        ("EmbargoId", "QuestionId"),
    ] {
        for method in ["find", "get", "remove", "erase"] {
            cases.push((format!("{domain}-{method}-wrong-key"), format!("{prelude}fn main() {{ let mut table = LocalTable::<{domain}, ()>::new(); let _ = table.{method}({wrong}::from_wire(0)); }}"), Some("E0308")));
        }
    }
    for (domain, wrong) in [("AnswerId", "QuestionId"), ("ImportId", "ExportId")] {
        cases.push((format!("{domain}-peer-wrong-key"), format!("{prelude}fn main() {{ let mut table = PeerTable::<{domain}, ()>::new(); table.slots.insert({wrong}::from_wire(0), ()); }}"), Some("E0308")));
    }
    for (table, domains) in [
        ("LocalTable", &domains[1..2]),
        ("LocalTable", &domains[3..4]),
        ("PeerTable", &domains[0..1]),
        ("PeerTable", &domains[2..3]),
        ("PeerTable", &domains[4..5]),
    ] {
        for domain in domains {
            cases.push((
                format!("{table}-{domain}-wrong-allocation-role"),
                format!("{prelude}fn wrong(_: {table}<{domain}, ()>) {{}} fn main() {{}}"),
                Some("E0277"),
            ));
        }
    }
    for domain in ["ExportId", "EmbargoId"] {
        for method in [
            "push_high(())",
            "insert_adopted(QuestionId::from_wire(1 << 30), ())",
        ] {
            cases.push((format!("{domain}-{}", method.split('(').next().unwrap()), format!("{prelude}fn main() {{ let mut table = LocalTable::<{domain}, ()>::new(); let _ = table.{method}; }}"), Some("E0599")));
        }
    }
    for (name, code, diagnostic) in [
        (
            "raw-key",
            "let mut table = LocalTable::<QuestionId, ()>::new(); let _ = table.find(0u32);",
            "E0308",
        ),
        (
            "private-storage",
            "let table = LocalTable::<QuestionId, ()>::new(); let _ = table.table;",
            "E0616",
        ),
        ("private-constructor", "let _ = QuestionId(0);", "E0423"),
        (
            "implicit-conversion",
            "let _: QuestionId = 0u32.into();",
            "E0277",
        ),
        (
            "default-key",
            "let _: QuestionId = Default::default();",
            "E0277",
        ),
        (
            "arithmetic",
            "let _ = QuestionId::from_wire(0) + 1;",
            "E0369",
        ),
        (
            "bounded-slot-private",
            "let mut slot = bounded::Slot::<AnswerId, ()>::EMPTY; slot.entry = None;",
            "E0616",
        ),
        (
            "borrowed-storage-exclusive",
            "let mut slots = [bounded::Slot::<AnswerId, ()>::EMPTY]; let first = bounded::BorrowedSparse::new(&mut slots); let second = bounded::BorrowedSparse::new(&mut slots); drop((first, second));",
            "E0499",
        ),
        (
            "local-slot-private",
            "let mut slot = capntproto_rpc_engine::local_table::Slot::<()>::EMPTY; slot.free_id = 1;",
            "E0616",
        ),
        (
            "local-storage-exclusive",
            "use capntproto_rpc_engine::local_table::{Slot, BorrowedSlots}; let mut slots = [Slot::<()>::EMPTY]; let first = BorrowedSlots::new(&mut slots); let second = BorrowedSlots::new(&mut slots); drop((first, second));",
            "E0499",
        ),
    ] {
        cases.push((
            name.to_owned(),
            format!("{prelude}fn main() {{ {code} }}"),
            Some(diagnostic),
        ));
    }
    let local_setup = "use capntproto_rpc_engine::local_table::{LocalTable as BoundedLocal, BorrowedSlots, Slot}; let mut slots = [Slot::<()>::EMPTY]; let mut h = []; let mut a = [];";
    for domain in ["ExportId", "EmbargoId"] {
        for method in [
            "try_push_high(())",
            "try_insert_adopted(QuestionId::from_wire(1 << 30), ())",
        ] {
            cases.push((format!("bounded-{domain}-{}", method.split('(').next().unwrap()), format!("{prelude}fn main() {{ {local_setup} let mut table = BoundedLocal::<{domain}, _, _, _, _>::try_new(BorrowedSlots::new(&mut slots), bounded::BorrowedSparse::new(&mut h), bounded::BorrowedSparse::new(&mut a)).unwrap(); table.{method}; }}"), Some("E0599")));
        }
    }
    for domain in ["AnswerId", "ImportId"] {
        cases.push((format!("bounded-local-{domain}-role"), format!("{prelude}fn wrong<D: capntproto_rpc_engine::local_table::Storage<()>, S: bounded::SparseStorage<{domain}, ()>>(_: capntproto_rpc_engine::local_table::LocalTable<{domain}, (), D, S, S>) {{}} fn main() {{}}"), Some("E0277")));
    }
    check_contracts("rpc-id-contracts", &format!("capntproto-rpc-engine = {{ path = {:?}, features = [\"alloc\"] }}\n", root().join("crates/capntproto-rpc-engine")), cases,
        "shared RPC identifiers and exact hosted table source: domain and allocation-role separation, explicit wire conversion, private slots and exclusive borrowed storage",
        "does not establish connection branding, stale-ID prevention, or correctness of a handler's choice of wire domain");
}

#[test]
fn bulk_reservation_compile_contracts() {
    let mut cases = vec![(
        "positive".to_owned(),
        r#"
use capntproto::bulk::{CreditWindow, Reservation, Settlement};
fn thread_safe<T: Send + Sync>() {}
fn main() {
    thread_safe::<CreditWindow>();
    thread_safe::<Reservation>();
    let mut window = CreditWindow::new(3, 2).unwrap();
    let reservation = window.reserve(2).unwrap().unwrap();
    let _: u64 = reservation.sequence();
    let _: u32 = reservation.bytes();
    let mut moved = Box::new(window);
    assert_eq!(moved.settle(&reservation).unwrap(), Settlement::Released);
    assert_eq!(moved.settle(&reservation).unwrap(), Settlement::AlreadySettled);
}
"#
        .to_owned(),
        None,
    )];
    for (name, source, diagnostic) in [
        ("raw-number", "fn main() { let mut w = capntproto::bulk::CreditWindow::new(3, 2).unwrap(); let _ = w.settle(&1u64); }", "E0308"),
        ("private-fields", "fn main() { let _ = capntproto::bulk::Reservation { owner: std::sync::Arc::new(()), sequence: std::num::NonZeroU64::new(1).unwrap(), bytes: std::num::NonZeroU32::new(1).unwrap() }; }", "E0451"),
        ("sequence-mutation", "fn wrong(r: &mut capntproto::bulk::Reservation) { r.sequence = std::num::NonZeroU64::new(1).unwrap(); } fn main() {}", "E0616"),
        ("owner-mutation", "fn wrong(r: &mut capntproto::bulk::Reservation) { r.owner = std::sync::Arc::new(()); } fn main() {}", "E0616"),
        ("bytes-mutation", "fn wrong(r: &mut capntproto::bulk::Reservation) { r.bytes = std::num::NonZeroU32::new(1).unwrap(); } fn main() {}", "E0616"),
        ("no-clone", "fn wrong(r: capntproto::bulk::Reservation) { let _ = r.clone(); } fn main() {}", "E0599"),
        ("no-copy", "fn require<T: Copy>() {} fn main() { require::<capntproto::bulk::Reservation>(); }", "E0277"),
        ("no-default", "fn main() { let _: capntproto::bulk::Reservation = Default::default(); }", "E0277"),
        ("no-conversion", "fn main() { let _: capntproto::bulk::Reservation = 1u64.into(); }", "E0277"),
        ("no-deserialization", "fn main() { let _ = serde_json::from_str::<capntproto::bulk::Reservation>(\"{}\"); }", "E0277"),
        ("no-implicit-decay", "fn wrong(r: capntproto::bulk::Reservation) { let _: u64 = r; } fn main() {}", "E0308"),
        ("use-after-move", "fn wrong(r: capntproto::bulk::Reservation) { drop(r); let _ = r.sequence(); } fn main() {}", "E0382"),
        ("reservation-must-use", "#![deny(unused_must_use)]\nfn main() { let mut w = capntproto::bulk::CreditWindow::new(3, 2).unwrap(); w.reserve(1).unwrap().unwrap(); }", "unused_must_use"),
        ("settlement-must-use", "#![deny(unused_must_use)]\nfn main() { let mut w = capntproto::bulk::CreditWindow::new(3, 2).unwrap(); let r = w.reserve(1).unwrap().unwrap(); w.settle(&r).unwrap(); }", "unused_must_use"),
        ("no-raw-acknowledge", "fn main() { let mut w = capntproto::bulk::CreditWindow::new(3, 2).unwrap(); let _ = w.acknowledge(1); }", "E0599"),
    ] {
        cases.push((name.to_owned(), source.to_owned(), Some(diagnostic)));
    }
    check_contracts("bulk-reservation-contracts", &format!(
        "capntproto = {{ path = {:?}, default-features = false, features = [\"services\"] }}\nserde_json = \"1\"\n", root()
    ), cases,
        "public bulk reservations: private immutable ownership, explicit sequence projection, move-only handles, must-use results, Send/Sync and removal of raw-number settlement",
        "foreign-window rejection is dynamic; a retained reservation is not evidence of remote execution or permission to settle before a terminal reply");
}

#[test]
fn bulk_config_compile_contracts() {
    let prelude = "use capntproto::bulk::{Config, Receiver, Sender};\n";
    let mut cases = vec![(
        "positive".to_owned(),
        format!(
            "{prelude}{}",
            r#"
fn thread_safe<T: Send + Sync>() {}
fn inspect(s: &Sender) {
    let c: &Config = s.config();
    let _: u64 = c.length();
    let _: u32 = c.max_chunk_bytes();
    let _: u32 = c.window_bytes();
    let _: u32 = c.max_chunks();
}
fn main() {
    thread_safe::<Config>();
    let checked: capnp::Result<Config> = Config::new(2, 1, 1, 2);
    let c = checked.unwrap();
    let imported: Config = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
    assert_eq!(c, imported);
    let _: (Receiver, capntproto::bulk_capnp::transfer::Client) = Receiver::new(c.clone());
}
"#
        ),
        None,
    )];
    for (name, code, diagnostic) in [
        ("private-constructor", "fn wrong() { let _ = Config { length:0, max_chunk_bytes:0, window_bytes:0, max_chunks:0 }; }", "E0451"),
        ("length-mutation", "fn wrong(c: &mut Config) { c.length = 0; }", "E0616"),
        ("chunk-mutation", "fn wrong(c: &mut Config) { c.max_chunk_bytes = 0; }", "E0616"),
        ("window-mutation", "fn wrong(c: &mut Config) { c.window_bytes = 0; }", "E0616"),
        ("count-mutation", "fn wrong(c: &mut Config) { c.max_chunks = 0; }", "E0616"),
        ("sender-mutation", "fn wrong(s: &Sender) { s.config().length = 0; }", "E0616"),
        ("no-default", "fn wrong() { let _: Config = Default::default(); }", "E0277"),
        ("no-tuple-conversion", "fn wrong() { let _: Config = (0u64,0u32,0u32,0u32).into(); }", "E0277"),
        ("checked-result", "fn wrong() { let _: Config = Config::new(0, 0, 0, 0); }", "E0308"),
        ("receiver-requires-checked", "fn wrong() { let _ = Receiver::new(Config::new(0, 0, 0, 0)); }", "E0308"),
        ("config-must-use", "fn wrong() { Config::new(1, 1, 1, 1).unwrap(); }", "unused_must_use"),
        ("receiver-must-use", "fn wrong(c: Config) { Receiver::new(c); }", "unused_must_use"),
    ] {
        cases.push((name.to_owned(), format!("#![deny(unused_must_use)]\n{prelude}{code}\nfn main() {{}}"), Some(diagnostic)));
    }
    check_contracts("bulk-config-contracts", &format!(
        "capntproto = {{ path = {:?}, default-features = false, features = [\"services\"] }}\ncapnp = {{ package = \"capntproto-core\", path = {:?} }}\nserde_json = \"1\"\n", root(), root().join("crates/capntproto-core")
    ), cases,
        "checked immutable bulk limits across receiver/sender APIs, private fields, explicit imports, Send/Sync, clone and must-use construction",
        "runtime validation of wire/JSON inputs is tested separately; limits are not authority or evidence of remote execution");
}

#[test]
fn persistent_descriptor_compile_contracts() {
    let prelude = "use capntproto::persistence::{Descriptor, ObjectKind};\nuse capntproto::authority::{ObjectGeneration, ObjectId, Rights};\n";
    let mut cases = vec![(
        "positive".to_owned(),
        format!(
            "{prelude}{}",
            r#"
fn metadata_is_thread_safe<T: Send + Sync>() {}
fn main() {
    metadata_is_thread_safe::<Descriptor>();
    let kind = ObjectKind::new(7).unwrap();
    let object = ObjectId::try_from(19u64).unwrap();
    let generation = ObjectGeneration::new(23).unwrap();
    let descriptor = Descriptor::new(kind, object, generation, Rights::VIEW);
    let _: ObjectKind = descriptor.kind();
    let _: ObjectId = descriptor.object();
    let _: ObjectGeneration = descriptor.generation();
    let _: Rights = descriptor.rights();
    let _: u64 = descriptor.generation().get();
    let _ = descriptor.clone();
    let json = serde_json::to_string(&descriptor).unwrap();
    let _: Descriptor = serde_json::from_str(&json).unwrap();
}
"#
        ),
        None,
    )];
    let domains = ["ObjectKind", "ObjectId", "ObjectGeneration"];
    for from in domains {
        for to in domains {
            if from != to {
                cases.push((
                    format!("{from}-as-{to}"),
                    format!("{prelude}fn main() {{ let _: {to} = {from}::new(1).unwrap(); }}"),
                    Some("E0308"),
                ));
            }
        }
        for (name, code, diagnostic) in [
            ("raw-number", format!("let _: {from} = 1u64;"), "E0308"),
            (
                "private-constructor",
                format!("let _ = {from}(std::num::NonZeroU64::new(1).unwrap());"),
                "E0423",
            ),
            (
                "implicit-conversion",
                format!("let _: {from} = 1u64.into();"),
                "E0277",
            ),
            (
                "default",
                format!("let _: {from} = Default::default();"),
                "E0277",
            ),
            (
                "arithmetic",
                format!("let _ = {from}::new(1).unwrap() + 1;"),
                "E0369",
            ),
            (
                "implicit-decay",
                format!("let _: u64 = {from}::new(1).unwrap();"),
                "E0308",
            ),
        ] {
            cases.push((
                format!("{from}-{name}"),
                format!("{prelude}fn main() {{ {code} }}"),
                Some(diagnostic),
            ));
        }
    }
    for (field, value) in [
        ("kind", "ObjectKind::new(2).unwrap()"),
        ("object", "ObjectId::new(2).unwrap()"),
        ("generation", "ObjectGeneration::new(2).unwrap()"),
        ("rights", "Rights::ALL"),
    ] {
        cases.push((
            format!("descriptor-{field}-private"),
            format!(
                "{prelude}fn wrong(d: &mut Descriptor) {{ d.{field} = {value}; }} fn main() {{}}"
            ),
            Some("E0616"),
        ));
    }
    for (name, code, diagnostic) in [
        ("private-literal", "fn main() { let _ = Descriptor { kind: ObjectKind::new(1).unwrap(), object: ObjectId::new(1).unwrap(), generation: ObjectGeneration::new(1).unwrap(), rights: Rights::ALL }; }", "E0451"),
        ("constructor-raw-kind", "fn wrong(o: ObjectId, g: ObjectGeneration) { let _ = Descriptor::new(1u64, o, g, Rights::ALL); } fn main() {}", "E0308"),
        ("constructor-raw-object", "fn wrong(k: ObjectKind, g: ObjectGeneration) { let _ = Descriptor::new(k, 1u64, g, Rights::ALL); } fn main() {}", "E0308"),
        ("constructor-raw-generation", "fn wrong(k: ObjectKind, o: ObjectId) { let _ = Descriptor::new(k, o, 1u64, Rights::ALL); } fn main() {}", "E0308"),
        ("constructor-swapped", "fn wrong(k: ObjectKind, o: ObjectId, g: ObjectGeneration) { let _ = Descriptor::new(k, g, o, Rights::ALL); } fn main() {}", "E0308"),
        ("registry-wrong-kind", "fn wrong(r: &capntproto::persistence::Realm, f: std::rc::Rc<dyn capntproto::persistence::Factory>, o: ObjectId) { let _ = r.register_factory(o, f); } fn main() {}", "E0308"),
        ("registry-raw-kind", "fn wrong(r: &capntproto::persistence::Realm, f: std::rc::Rc<dyn capntproto::persistence::Factory>) { let _ = r.register_factory(1u64, f); } fn main() {}", "E0308"),
        ("factory-wrong-generation", "fn wrong(s: std::rc::Rc<std::cell::RefCell<capntproto::storage::Store>>, k: ObjectKind) { let _ = capntproto::persistence::ObjectFactory::<capntproto::store_capnp::document::Owned>::new(s, k); } fn main() {}", "E0308"),
        ("factory-wrong-object", "fn wrong(f: &capntproto::persistence::ObjectFactory<capntproto::store_capnp::document::Owned>, g: ObjectGeneration) { let _ = f.state(g); } fn main() {}", "E0308"),
        ("orm-binding-wrong-kind", "fn wrong(r: &capntproto::persistence::Realm, s: std::rc::Rc<capntproto::orm::ObjectState>, g: capntproto::authority::Grant, o: ObjectId) { let _ = r.persistent_object::<capntproto::store_capnp::document::Owned>(s, g, o); } fn main() {}", "E0308"),
    ] {
        cases.push((name.to_owned(), format!("{prelude}{code}"), Some(diagnostic)));
    }
    cases.push(("descriptor-must-use".to_owned(), format!("#![deny(unused_must_use)]\n{prelude}fn main() {{ Descriptor::new(ObjectKind::new(1).unwrap(), ObjectId::new(1).unwrap(), ObjectGeneration::new(1).unwrap(), Rights::ALL); }}"), Some("unused_must_use")));
    check_contracts("persistent-descriptor-contracts", &format!(
        "capntproto = {{ path = {:?}, default-features = false, features = [\"storage\"] }}\nserde_json = \"1\"\n", root()
    ), cases,
        "external persistent metadata consumers: distinct nonzero identifier domains, immutable descriptor bindings, typed factory registry/cache/generation and ORM binding, checked serialization, must-use descriptors",
        "metadata does not confer authority or brand IDs to a realm; hosts and factories still enforce object meaning, authorization, generation and exact restored rights");
}

#[test]
fn object_authority_compile_contracts() {
    let prelude = "use capntproto::{authority::{Grant, ObjectId, ObjectGeneration, Rights}, orm::ObjectState};\n";
    let mut cases = vec![(
        "positive".to_owned(),
        format!(
            "{prelude}{}",
            r#"
fn bind(store: std::rc::Rc<std::cell::RefCell<capntproto::storage::Store>>) {
    let object = ObjectId::new(7).unwrap();
    let generation = ObjectGeneration::new(11).unwrap();
    let root = Grant::root(object, generation, [1;32], Rights::ALL);
    let child = root.delegate([2;32], Rights::VIEW).unwrap();
    let _: ObjectId = child.object();
    let _: ObjectGeneration = child.generation();
    let state = ObjectState::new(store.clone(), child.object());
    let _: ObjectId = state.object();
    let _: &std::rc::Rc<std::cell::RefCell<capntproto::storage::Store>> = state.store();
    let _ = capntproto::orm::ObjectServer::<capntproto::store_capnp::document::Owned>::client(state, child).unwrap();
}
fn main() {}
"#
        ),
        None,
    )];
    for (name, code, diagnostic) in [
        ("root-raw-object", "fn wrong(g: ObjectGeneration) { let _ = Grant::root(0u64, g, [1;32], Rights::ALL); }", "E0308"),
        ("root-raw-generation", "fn wrong(o: ObjectId) { let _ = Grant::root(o, 0u64, [1;32], Rights::ALL); }", "E0308"),
        ("root-swapped", "fn wrong(o: ObjectId, g: ObjectGeneration) { let _ = Grant::root(g, o, [1;32], Rights::ALL); }", "E0308"),
        ("root-factory-kind", "fn wrong(k: capntproto::persistence::ObjectKind, g: ObjectGeneration) { let _ = Grant::root(k, g, [1;32], Rights::ALL); }", "E0308"),
        ("object-numeric-decay", "fn wrong(g: &Grant) { let _: u64 = g.object(); }", "E0308"),
        ("generation-numeric-decay", "fn wrong(g: &Grant) { let _: u64 = g.generation(); }", "E0308"),
        ("object-as-generation", "fn wrong(g: &Grant) { let _: ObjectGeneration = g.object(); }", "E0308"),
        ("generation-as-object", "fn wrong(g: &Grant) { let _: ObjectId = g.generation(); }", "E0308"),
        ("state-raw-object", "fn wrong(s: std::rc::Rc<std::cell::RefCell<capntproto::storage::Store>>) { let _ = ObjectState::new(s, 0u64); }", "E0308"),
        ("state-generation", "fn wrong(s: std::rc::Rc<std::cell::RefCell<capntproto::storage::Store>>, g: ObjectGeneration) { let _ = ObjectState::new(s, g); }", "E0308"),
        ("state-object-mutation", "fn wrong(s: &mut ObjectState, o: ObjectId) { s.object = o; }", "E0616"),
        ("state-store-mutation", "fn wrong(s: &mut ObjectState, store: std::rc::Rc<std::cell::RefCell<capntproto::storage::Store>>) { s.store = store; }", "E0616"),
        ("state-store-accessor-rebind", "fn wrong(s: &ObjectState, store: std::rc::Rc<std::cell::RefCell<capntproto::storage::Store>>) { *s.store() = store; }", "E0594"),
        ("state-object-decay", "fn wrong(s: &ObjectState) { let _: u64 = s.object(); }", "E0308"),
        ("state-send", "fn require<T: Send>() {} fn wrong() { require::<ObjectState>(); }", "E0277"),
        ("state-sync", "fn require<T: Sync>() {} fn wrong() { require::<ObjectState>(); }", "E0277"),
    ] {
        cases.push((name.to_owned(), format!("{prelude}{code}\nfn main() {{}}"), Some(diagnostic)));
    }
    check_contracts("object-authority-contracts", &format!(
        "capntproto = {{ path = {:?}, default-features = false, features = [\"storage\"] }}\n", root()
    ), cases,
        "shared object authority domains in root grants, delegation, ORM binding and immutable state accessors",
        "IDs are metadata, not store brands; trusted hosts still issue root grants and choose generations and stores");
}

#[test]
fn storage_key_and_snapshot_compile_contracts() {
    let prelude =
        "use capntproto::{storage::{ObjectKey, Revision, Snapshot, Store, Update}, durable_bulk::JournalId};\n";
    let mut cases = vec![(
        "positive".to_owned(),
        format!(
            "{prelude}{}",
            r#"
fn thread_safe<T: Send + Sync>() {}
fn inspect(store: &mut Store, snapshot: &Snapshot) {
    let key = ObjectKey::new(0);
    let journal = JournalId::new(u64::MAX);
    let _: ObjectKey = journal.key();
    let _: u64 = journal.get();
    let _: u64 = key.get();
    let _: ObjectKey = snapshot.object();
    let _: Revision = snapshot.revision();
    let _ = snapshot.clone();
    let _ = store.head(key);
    let _ = store.put(key, Revision::INITIAL, b"value");
    let _ = store.publish(key, Revision::new(1), Revision::INITIAL);
    let _ = store.commit(&[Update {object:key, expected_head:Revision::new(1), expected_published:None, value:b"next"}]);
    let _: Vec<ObjectKey> = store.objects().collect();
    let _ = ObjectKey::from(capntproto::authority::ObjectId::new(7).unwrap());
}
fn main() { thread_safe::<ObjectKey>(); thread_safe::<JournalId>(); thread_safe::<Snapshot>(); }
"#
        ),
        None,
    )];
    for method in [
        "head(1u64)",
        "published(1u64)",
        "get(1u64)",
        "revision(1u64, Revision::new(1))",
        "put(1u64, Revision::INITIAL, b\"value\")",
        "publish(1u64, Revision::new(1), Revision::INITIAL)",
        "history_bounds(1u64)",
        "publication_cursor(1u64, 0)",
    ] {
        cases.push((
            format!("raw-{}", method.split('(').next().unwrap()),
            format!("{prelude}fn wrong(s: &mut Store) {{ let _ = s.{method}; }} fn main() {{}}"),
            Some("E0308"),
        ));
    }
    for domain in ["ObjectKey", "JournalId"] {
        for (name, code, diagnostic) in [
            ("number", format!("let _: {domain} = 1u64;"), "E0308"),
            (
                "conversion",
                format!("let _: {domain} = 1u64.into();"),
                "E0277",
            ),
            (
                "default",
                format!("let _: {domain} = Default::default();"),
                "E0277",
            ),
            (
                "arithmetic",
                format!("let _ = {domain}::new(1) + 1;"),
                "E0369",
            ),
            (
                "numeric-decay",
                format!("let _: u64 = {domain}::new(1);"),
                "E0308",
            ),
        ] {
            cases.push((
                format!("{domain}-{name}"),
                format!("{prelude}fn main() {{ {code} }}"),
                Some(diagnostic),
            ));
        }
    }
    for (name, code, diagnostic) in [
        ("key-private-constructor", "fn wrong() { let _ = ObjectKey(1); }", "E0423"),
        ("journal-private-constructor", "fn wrong() { let _ = JournalId(ObjectKey::new(1)); }", "E0423"),
        ("snapshot-revision-mutation", "fn wrong(s: &mut Snapshot) { s.revision = Revision::new(99); }", "E0616"),
        ("snapshot-object-mutation", "fn wrong(s: &mut Snapshot) { s.object = ObjectKey::new(99); }", "E0616"),
        ("snapshot-object-as-revision", "fn wrong(s: &Snapshot) { let _: u64 = s.object(); }", "E0308"),
        ("snapshot-revision-as-object", "fn wrong(s: &Snapshot) { let _: ObjectKey = s.revision(); }", "E0308"),
        ("batch-raw-object", "fn wrong() { let _ = Update {object:0u64, expected_head:Revision::INITIAL, expected_published:None, value:b\"\"}; }", "E0308"),
        ("batch-key-as-revision", "fn wrong(k: ObjectKey) { let _ = Update {object:k, expected_head:k, expected_published:None, value:b\"\"}; }", "E0308"),
        ("journal-as-object", "fn wrong(s: &Store, j: JournalId) { let _ = s.get(j); }", "E0308"),
        ("authority-as-store-key", "fn wrong(s: &Store, o: capntproto::authority::ObjectId) { let _ = s.get(o); }", "E0308"),
        ("key-as-generation", "fn wrong(k: ObjectKey) { let _: capntproto::authority::ObjectGeneration = k; }", "E0308"),
        ("object-as-journal", "fn wrong(o: capntproto::authority::ObjectId) { let _: JournalId = o; }", "E0308"),
        ("key-as-journal", "fn wrong(k: ObjectKey) { let _: JournalId = k; }", "E0308"),
        ("resume-raw-journal", "fn wrong(s: std::rc::Rc<capntproto::orm::ObjectState>, g: capntproto::authority::Grant) { let _ = capntproto::durable_bulk::Receiver::<capntproto::store_capnp::document::Owned>::resume(s, 1u64, g); }", "E0308"),
        ("create-object-as-journal", "fn wrong(s: std::rc::Rc<capntproto::orm::ObjectState>, g: capntproto::authority::Grant, c: capntproto::bulk::Config, k: ObjectKey) { let _ = capntproto::durable_bulk::Receiver::<capntproto::store_capnp::document::Owned>::create(s, k, g, c, [0;32]); }", "E0308"),
    ] {cases.push((name.to_owned(), format!("{prelude}{code} fn main() {{}}"),Some(diagnostic)));}
    check_contracts("storage-key-contracts", &format!(
        "capntproto = {{ path = {:?}, default-features = false, features = [\"storage\", \"services\"] }}\n", root()
    ), cases,
        "typed Store and upload-journal domains, immutable snapshot coordinates, explicit conversions, and Send/Sync preservation",
        "keys are store-local metadata, not instance brands or authority; revision counters have a distinct metadata type and external mmap mutation remains prohibited");
}

#[test]
fn publication_cursor_compile_contracts() {
    let prelude =
        "use capntproto::storage::{ObjectKey, Publication, PublicationCursor, Revision, Snapshot, Store};\n";
    let mut cases = vec![(
        "positive".to_owned(),
        format!(
            "{prelude}{}",
            r#"
fn thread_safe<T: Send + Sync>() {}
fn read(store: &Store) -> capntproto::storage::Result<()> {
    let cursor = store.publication_cursor(ObjectKey::new(0), 0)?;
    let _: ObjectKey = cursor.object();
    let _: Revision = cursor.after();
    let _ = cursor.clone();
    if let Some(publication) = store.publication_after(&cursor)? {
        let _: &Snapshot = publication.snapshot();
        let _: &PublicationCursor = publication.cursor();
        let _: (Snapshot, PublicationCursor) = publication.into_parts();
    }
    Ok(())
}
fn main() { thread_safe::<PublicationCursor>(); thread_safe::<Publication>(); }
"#
        ),
        None,
    )];
    for (name, code, diagnostic) in [
        ("raw-read", "fn wrong(s: &Store) { let _ = s.publication_after(&0u64); }", "E0308"),
        ("key-as-cursor", "fn wrong(s: &Store, k: ObjectKey) { let _ = s.publication_after(&k); }", "E0308"),
        ("snapshot-as-cursor", "fn wrong(s: &Store, v: &Snapshot) { let _ = s.publication_after(v); }", "E0308"),
        ("cursor-as-object", "fn wrong(s: &Store, c: PublicationCursor) { let _ = s.get(c); }", "E0308"),
        ("cursor-as-revision", "fn wrong(s: &mut Store, k: ObjectKey, c: PublicationCursor) { let _ = s.put(k, c, b\"value\"); }", "E0308"),
        ("cursor-as-checkpoint", "fn wrong(c: PublicationCursor) { let _: u64 = c; }", "E0308"),
        ("cursor-convert", "fn wrong() { let _: PublicationCursor = 0u64.into(); }", "E0277"),
        ("cursor-default", "fn wrong() { let _: PublicationCursor = Default::default(); }", "E0277"),
        ("cursor-copy", "fn require<T: Copy>() {} fn wrong() { require::<PublicationCursor>(); }", "E0277"),
        ("cursor-arithmetic", "fn wrong(c: PublicationCursor) { let _ = c + 1; }", "E0369"),
        ("cursor-owner-private", "fn wrong(c: &mut PublicationCursor) { c.owner = std::sync::Arc::new(()); }", "E0616"),
        ("cursor-object-private", "fn wrong(c: &mut PublicationCursor) { c.object = ObjectKey::new(7); }", "E0616"),
        ("cursor-position-private", "fn wrong(c: &mut PublicationCursor) { c.after = Revision::new(7); }", "E0616"),
        ("cursor-constructor", "fn wrong() { let _ = PublicationCursor { owner: std::sync::Arc::new(()), object: ObjectKey::new(0), after: Revision::INITIAL }; }", "E0451"),
        ("cursor-deserialize", "fn wrong() { let _ = serde_json::from_str::<PublicationCursor>(\"{}\"); }", "E0277"),
        ("cursor-serialize", "fn wrong(c: &PublicationCursor) { let _ = serde_json::to_string(c); }", "E0277"),
        ("publication-snapshot-private", "fn wrong(p: &mut Publication, s: Snapshot) { p.snapshot = s; }", "E0616"),
        ("publication-cursor-private", "fn wrong(p: &mut Publication, c: PublicationCursor) { p.cursor = c; }", "E0616"),
        ("publication-constructor", "fn wrong(s: Snapshot, c: PublicationCursor) { let _ = Publication { snapshot:s, cursor:c }; }", "E0451"),
        ("publication-cursor-rebind", "fn wrong(p: &Publication, c: PublicationCursor) { *p.cursor() = c; }", "E0594"),
        ("publication-default", "fn wrong() { let _: Publication = Default::default(); }", "E0277"),
        ("publication-double-consume", "fn wrong(p: Publication) { let _ = p.into_parts(); let _ = p.into_parts(); }", "E0382"),
        ("cursor-must-use", "fn wrong(s: &Store) { s.publication_cursor(ObjectKey::new(0), 0).unwrap(); }", "unused_must_use"),
        ("publication-must-use", "fn wrong(s: &Store, c: &PublicationCursor) { s.publication_after(c).unwrap().unwrap(); }", "unused_must_use"),
    ] {
        cases.push((name.to_owned(), format!("#![deny(unused_must_use)]\n{prelude}{code}\nfn main() {{}}"), Some(diagnostic)));
    }
    check_contracts("publication-cursor-contracts", &format!(
        "capntproto = {{ path = {:?}, default-features = false, features = [\"storage\"] }}\nserde_json = \"1\"\n", root()
    ), cases,
        "checked publication cursors, immutable Store/object/position binding, paired snapshots and next positions, explicit wire checkpoints and retained Send/Sync",
        "Store ownership and retention require runtime checks; numeric imports are explicit trusted-host operations and cursors do not confer capability authority");
}

#[test]
fn revision_domain_compile_contracts() {
    let prelude = "use capntproto::{semantics::Revisions, storage::{ObjectKey, Revision, Store, Update, Snapshot, PublicationCursor}};\n";
    let mut cases = vec![(
        "positive".to_owned(),
        format!(
            "{prelude}{}",
            r#"
fn thread_safe<T: Send + Sync>() {}
fn inspect(s: &mut Store, view: &Snapshot, cursor: &PublicationCursor) {
    let zero = Revision::INITIAL;
    let max = Revision::MAX;
    let _: u64 = max.get();
    let _: Option<Revision> = zero.checked_next();
    let _: Revision = view.revision();
    let _: Revision = cursor.after();
    let state = Revisions::new(max, zero).unwrap();
    let _ = state.stage(max);
    let _ = state.publish(max, zero);
    let _: Revision = state.head();
    let _: Revision = state.published();
    let key = ObjectKey::new(0);
    let _: Revision = s.head(key);
    let _: Revision = s.published(key);
    let _: (Revision, Revision) = s.history_bounds(key).unwrap();
    let _ = s.put(key, zero, b"x");
    let _ = s.publish(key, Revision::new(1), zero);
    let _: Vec<Revision> = s.commit(&[Update { object:key, expected_head:Revision::new(1), expected_published:Some(Revision::new(1)), value:b"y" }]).unwrap();
    let encoded = serde_json::to_string(&max).unwrap();
    let _: Revision = serde_json::from_str(&encoded).unwrap();
}
fn main() { thread_safe::<Revision>(); thread_safe::<Revisions>(); }
"#
        ),
        None,
    )];
    for (name, code, diagnostic) in [
        ("raw-counter", "fn wrong() { let _: Revision = 1u64; }", "E0308"),
        ("implicit-counter", "fn wrong() { let _: Revision = 1u64.into(); }", "E0277"),
        ("counter-decay", "fn wrong(r: Revision) { let _: u64 = r; }", "E0308"),
        ("counter-default", "fn wrong() { let _: Revision = Default::default(); }", "E0277"),
        ("counter-constructor", "fn wrong() { let _ = Revision(0); }", "E0423"),
        ("counter-field", "fn wrong(r: &mut Revision) { r.0 = 0; }", "E0616"),
        ("unchecked-add", "fn wrong(r: Revision) { let _ = r + 1; }", "E0369"),
        ("raw-comparison", "fn wrong(r: Revision) { let _ = r == 0; }", "E0308"),
        ("object-as-counter", "fn wrong(k: ObjectKey) { let _: Revision = k; }", "E0308"),
        ("counter-as-object", "fn wrong(r: Revision) { let _: ObjectKey = r; }", "E0308"),
        ("generation-as-counter", "fn wrong(g: capntproto::authority::ObjectGeneration) { let _: Revision = g; }", "E0308"),
        ("counter-as-generation", "fn wrong(r: Revision) { let _: capntproto::authority::ObjectGeneration = r; }", "E0308"),
        ("counter-as-cursor", "fn wrong(r: Revision) { let _: PublicationCursor = r; }", "E0308"),
        ("snapshot-decay", "fn wrong(s: &Snapshot) { let _: u64 = s.revision(); }", "E0308"),
        ("cursor-decay", "fn wrong(c: &PublicationCursor) { let _: u64 = c.after(); }", "E0308"),
        ("raw-read", "fn wrong(s: &Store, k: ObjectKey) { let _ = s.revision(k, 1u64); }", "E0308"),
        ("raw-stage", "fn wrong(s: &mut Store, k: ObjectKey) { let _ = s.put(k, 0u64, b\"x\"); }", "E0308"),
        ("raw-publish", "fn wrong(s: &mut Store, k: ObjectKey) { let _ = s.publish(k, 1u64, Revision::INITIAL); }", "E0308"),
        ("raw-publish-compare", "fn wrong(s: &mut Store, k: ObjectKey) { let _ = s.publish(k, Revision::new(1), 0u64); }", "E0308"),
        ("raw-batch-head", "fn wrong(k: ObjectKey) { let _ = Update { object:k, expected_head:0u64, expected_published:None, value:b\"x\" }; }", "E0308"),
        ("raw-batch-publication", "fn wrong(k: ObjectKey) { let _ = Update { object:k, expected_head:Revision::INITIAL, expected_published:Some(0u64), value:b\"x\" }; }", "E0308"),
        ("state-constructor", "fn wrong() { let _ = Revisions { head:Revision::INITIAL, published:Revision::MAX }; }", "E0451"),
        ("state-head-private", "fn wrong(s: &mut Revisions) { s.head = Revision::MAX; }", "E0616"),
        ("state-publication-private", "fn wrong(s: &mut Revisions) { s.published = Revision::MAX; }", "E0616"),
        ("state-raw-stage", "fn wrong(s: Revisions) { let _ = s.stage(0u64); }", "E0308"),
        ("state-deserialize", "fn wrong() { let _ = serde_json::from_str::<Revisions>(\"{}\"); }", "E0277"),
        ("state-must-use", "fn wrong() { Revisions::default(); }", "unused_must_use"),
        ("stage-must-use", "fn wrong(s: Revisions) { s.stage(s.head()); }", "unused_must_use"),
        ("publish-must-use", "fn wrong(s: Revisions) { s.publish(s.head(), s.published()); }", "unused_must_use"),
        ("next-must-use", "fn wrong(r: Revision) { r.checked_next(); }", "unused_must_use"),
    ] {
        cases.push((name.to_owned(), format!("#![deny(unused_must_use)]\n{prelude}{code}\nfn main() {{}}"), Some(diagnostic)));
    }
    check_contracts("revision-domain-contracts", &format!(
        "capntproto = {{ path = {:?}, default-features = false, features = [\"storage\"] }}\nserde_json = \"1\"\n", root()
    ), cases,
        "distinct revision counters throughout Store and snapshot/cursor APIs; private checked revision states, explicit numeric boundaries and checked successor operations",
        "zero denotes the initial counter; numbers remain object-local metadata, not existence/publication evidence or Store ownership; no unbounded implementation proof");
}

#[test]
fn realtime_snapshot_compile_contracts() {
    let prelude = "use capntproto::realtime::{Receiver, Snapshot};\n";
    let mut cases = vec![(
        "positive".to_owned(),
        format!(
            "{prelude}{}",
            r#"
fn inspect(receiver: &Receiver) {
    let snapshot: std::rc::Rc<Snapshot> = receiver.get(0).unwrap();
    let _: u64 = snapshot.sequence();
    let _: u32 = snapshot.key();
    let _: u64 = snapshot.not_after();
    let _: &[u8] = snapshot.bytes();
    let owned: Snapshot = snapshot.as_ref().clone();
    let _ = format!("{:?}", owned);
}
fn main() {}
"#
        ),
        None,
    )];
    for (name, code, diagnostic) in [
        ("private-constructor", "fn wrong() { let _ = Snapshot { sequence:1, key:0, not_after:1, bytes:std::rc::Rc::from(b\"a\".as_slice()) }; }", "E0451"),
        ("sequence-mutation", "fn wrong(s: &mut Snapshot) { s.sequence = 2; }", "E0616"),
        ("key-mutation", "fn wrong(s: &mut Snapshot) { s.key = 1; }", "E0616"),
        ("deadline-mutation", "fn wrong(s: &mut Snapshot) { s.not_after = 0; }", "E0616"),
        ("payload-replacement", "fn wrong(s: &mut Snapshot) { s.bytes = std::rc::Rc::from(b\"b\".as_slice()); }", "E0616"),
        ("payload-mutation", "fn wrong(s: &mut Snapshot) { s.bytes()[0] = 0; }", "E0594"),
        ("mutable-slice", "fn wrong(s: &mut Snapshot) { let _: &mut [u8] = s.bytes(); }", "E0308"),
        ("default", "fn wrong() { let _: Snapshot = Default::default(); }", "E0277"),
        ("deserialize", "fn wrong() { let _ = serde_json::from_str::<Snapshot>(\"{}\"); }", "E0277"),
        ("copy", "fn require<T: Copy>() {} fn wrong() { require::<Snapshot>(); }", "E0277"),
        ("send", "fn require<T: Send>() {} fn wrong() { require::<Snapshot>(); }", "E0277"),
        ("sync", "fn require<T: Sync>() {} fn wrong() { require::<Snapshot>(); }", "E0277"),
        ("must-use", "fn wrong(s: &Snapshot) { s.clone(); }", "unused_must_use"),
    ] {
        cases.push((name.to_owned(), format!("#![deny(unused_must_use)]\n{prelude}{code}\nfn main() {{}}"), Some(diagnostic)));
    }
    check_contracts("realtime-snapshot-contracts", &format!(
        "capntproto = {{ path = {:?}, default-features = false, features = [\"services\"] }}\nserde_json = \"1\"\n", root()
    ), cases,
        "immutable realtime snapshot coordinates/data, receiver-issued construction, retained clone/Debug and local thread traits, and required handling of returned values",
        "snapshot values are not receiver-branded authority or evidence that the deadline remains in the future; runtime publication/deadline/receipt invariants have separate trace tests");
}

#[test]
fn realtime_config_compile_contracts() {
    let prelude = "use capntproto::realtime::{Clock, Config, Receiver, Sender};\n";
    let mut cases = vec![(
        "positive".to_owned(),
        format!(
            "{prelude}{}",
            r#"
fn thread_safe<T: Send + Sync + Clone + std::fmt::Debug + Eq>() {}
fn inspect(sender: &Sender) {
    let c: &Config = sender.config();
    let _: &str = c.clock_domain();
    let _: u64 = c.clock_skew();
    let _: u32 = c.keys();
    let _: u32 = c.capacity();
    let _: u64 = c.max_sequence();
    let _: u32 = c.max_payload_bytes();
    let _: u32 = c.max_waiters();
}
fn create(clock: std::rc::Rc<dyn Clock>) {
    let config = Config::new("ticks", 0, 1, 1, 1, 1, 1).unwrap();
    let _: (Receiver, capntproto::realtime_capnp::snapshots::Client) = Receiver::new(config, clock);
}
fn main() { thread_safe::<Config>(); }
"#
        ),
        None,
    )];
    for (name, code, diagnostic) in [
        ("private-constructor", "fn wrong() { let _ = Config { clock_domain:String::new(), clock_skew:0, keys:0, capacity:0, max_sequence:0, max_payload_bytes:0, max_waiters:0 }; }", "E0451"),
        ("domain-mutation", "fn wrong(c: &mut Config) { c.clock_domain = String::new(); }", "E0616"),
        ("skew-mutation", "fn wrong(c: &mut Config) { c.clock_skew = 1; }", "E0616"),
        ("keys-mutation", "fn wrong(c: &mut Config) { c.keys = 0; }", "E0616"),
        ("capacity-mutation", "fn wrong(c: &mut Config) { c.capacity = 0; }", "E0616"),
        ("sequence-mutation", "fn wrong(c: &mut Config) { c.max_sequence = 0; }", "E0616"),
        ("payload-mutation", "fn wrong(c: &mut Config) { c.max_payload_bytes = 0; }", "E0616"),
        ("waiters-mutation", "fn wrong(c: &mut Config) { c.max_waiters = 0; }", "E0616"),
        ("domain-readonly", "fn wrong(c: &mut Config) { let _: &mut str = c.clock_domain(); }", "E0308"),
        ("sender-readonly", "fn wrong(s: &mut Sender) { let _: &mut Config = s.config(); }", "E0308"),
        ("default", "fn wrong() { let _: Config = Default::default(); }", "E0277"),
        ("deserialize", "fn wrong() { let _ = serde_json::from_str::<Config>(\"{}\"); }", "E0277"),
        ("unchecked-result", "fn wrong() { let _: Config = Config::new(\"ticks\", 0, 1, 1, 1, 1, 1); }", "E0308"),
        ("config-must-use", "fn wrong() { Config::new(\"ticks\", 0, 1, 1, 1, 1, 1).unwrap(); }", "unused_must_use"),
        ("receiver-must-use", "fn wrong(c: Config, clock: std::rc::Rc<dyn Clock>) { Receiver::new(c, clock); }", "unused_must_use"),
    ] {
        cases.push((name.to_owned(), format!("#![deny(unused_must_use)]\n{prelude}{code}\nfn main() {{}}"), Some(diagnostic)));
    }
    check_contracts("realtime-config-contracts", &format!(
        "capntproto = {{ path = {:?}, default-features = false, features = [\"services\"] }}\nserde_json = \"1\"\n", root()
    ), cases,
        "checked immutable clock/resource configuration, no construction/mutation/deserialization bypass, infallible receiver creation and required result handling",
        "limits are stream metadata, not capability authority, clock synchronization evidence or a global process memory quota; datagram limits remain an additional adapter check");
}

fn check_contracts(
    name: &str,
    dependencies: &str,
    cases: Vec<(String, String, Option<&str>)>,
    scope: &str,
    limits: &str,
) {
    let base = root().join("target/verification").join(name);
    fs::create_dir_all(&base).unwrap();
    let reports = tempfile::tempdir_in(&base).unwrap().keep();
    let inputs = v::distribution::verification_hashes(&root()).unwrap();
    let compiler = run(
        command("rustc").args(["--version", "--verbose"]),
        &reports.join("compiler.log"),
        0,
    )
    .unwrap();
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    fs::write(project.path().join("Cargo.toml"), format!(
        "[package]\nname = \"capntproto-{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n[dependencies]\n{dependencies}"
    )).unwrap();
    // Seed from the workspace; the positive check adjusts the consumer entry.
    // The subsequent checks require exactly that resulting dependency lock.
    fs::copy(root().join("Cargo.lock"), project.path().join("Cargo.lock")).unwrap();
    let mut evidence = Vec::new();
    for (index, (name, source, diagnostic)) in cases.iter().enumerate() {
        fs::write(project.path().join("src/main.rs"), source).unwrap();
        let mut check = command("cargo");
        check
            .args([
                "check",
                "--offline",
                "--message-format=json",
                "--manifest-path",
            ])
            .arg(project.path().join("Cargo.toml"))
            .arg("--target-dir")
            .arg(root().join("target/api-contracts"));
        if index > 0 {
            check.arg("--locked");
        }
        let output = run(
            &mut check,
            &reports.join(format!("{name}.log")),
            if diagnostic.is_some() { 101 } else { 0 },
        )
        .unwrap();
        let errors: BTreeSet<String> = output
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|message| {
                message["reason"] == "compiler-message" && message["message"]["level"] == "error"
            })
            .map(|message| {
                message["message"]["code"]["code"]
                    .as_str()
                    .or_else(|| {
                        (message["message"]["message"] == "lifetime may not live long enough")
                            .then_some("lifetime may not live long enough")
                    })
                    .expect("unclassified compiler failure")
                    .to_owned()
            })
            .collect();
        assert_eq!(
            errors,
            diagnostic.iter().map(|code| (*code).to_owned()).collect(),
            "{name}: {output}"
        );
        evidence.push(json!({"case": name, "source": source, "diagnostic": diagnostic}));
    }
    assert_eq!(
        inputs,
        v::distribution::verification_hashes(&root()).unwrap(),
        "sources changed during API qualification"
    );
    let evidence = json!({"sources":inputs, "compiler":compiler, "cases":evidence,
        "consumer_lock_sha256": v::sha256(fs::read(project.path().join("Cargo.lock")).unwrap()),
        "scope":scope, "limits":limits});
    fs::write(
        reports.join("checked.json"),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
    eprintln!(
        "{} compiler contracts; artifacts: {}",
        cases.len(),
        reports.display()
    );
}
