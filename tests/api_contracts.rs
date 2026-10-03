//! Compile external consumers of public APIs and the exact private RPC table
//! modules. Check exact Rust diagnostics after a positive control; all
//! orchestration is an ordinary Cargo integration test.
use capntproto_test_support::verification::{self as v, command, root, run};
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs};

#[path = "api_contracts/rpc_reply.rs"]
mod rpc_reply;

#[test]
fn authority_bindings_and_route_generation_compile_contracts() {
    let mut cases = vec![(
        "positive".to_owned(),
        r#"
use capntproto::authority::{Grant, ObjectGeneration, ObjectId, Rights};
use capntproto::native_rpc::{RouteGeneration, RouteObserver};
fn observe(route: &RouteObserver) -> RouteGeneration {
    let id = route.generation();
    let _: u64 = id.get();
    let _ = id.to_string();
    id
}
fn main() {
    let grant = Grant::root(capntproto::authority::ObjectId::new(7).unwrap(), capntproto::authority::ObjectGeneration::new(11).unwrap(), [1; 32], Rights::ALL);
    let child = grant.delegate([2; 32], Rights::VIEW).unwrap();
    let _: ObjectId = child.object();
    let _: ObjectGeneration = child.generation();
    let _: [u8; 32] = child.holder();
    let _: Rights = child.rights();
    let _ = child.clone();
    child.revoke();
}
"#
        .to_owned(),
        None,
    )];
    for (field, value) in [
        ("object", "99"),
        ("generation", "99"),
        ("holder", "[9; 32]"),
    ] {
        cases.push((format!("grant-{field}-private"), format!(
            "fn retarget(grant: &mut capntproto::authority::Grant) {{ grant.{field} = {value}; }}\nfn main() {{}}"
        ), Some("E0616")));
    }
    for (name, source, diagnostic) in [
        ("generation-from-count", "fn main() { let _: capntproto::native_rpc::RouteGeneration = 1u64; }", "E0308"),
        ("generation-as-count", "fn wrong(g: capntproto::native_rpc::RouteGeneration) { let _: u64 = g; } fn main() {}", "E0308"),
        ("generation-private-constructor", "use capntproto::native_rpc::RouteGeneration; fn main() { let _ = RouteGeneration(std::num::NonZeroU64::new(1).unwrap()); }", "E0423"),
        ("generation-from-conversion", "fn main() { let _: capntproto::native_rpc::RouteGeneration = 1u64.into(); }", "E0277"),
        ("generation-default", "fn main() { let _: capntproto::native_rpc::RouteGeneration = Default::default(); }", "E0277"),
        ("generation-arithmetic", "fn wrong(g: capntproto::native_rpc::RouteGeneration) { let _ = g + 1; } fn main() {}", "E0369"),
        ("generation-deserialize", "fn main() { let _ = serde_json::from_str::<capntproto::native_rpc::RouteGeneration>(\"1\"); }", "E0277"),
        ("grant-deserialize", "fn main() { let _ = serde_json::from_str::<capntproto::authority::Grant>(\"{}\"); }", "E0277"),
        ("grant-send", "fn require<T: Send>() {} fn main() { require::<capntproto::authority::Grant>(); }", "E0277"),
        ("grant-sync", "fn require<T: Sync>() {} fn main() { require::<capntproto::authority::Grant>(); }", "E0277"),
        ("observer-send", "fn require<T: Send>() {} fn main() { require::<capntproto::native_rpc::RouteObserver>(); }", "E0277"),
        ("grant-must-use", "#![deny(unused_must_use)]\nfn main() { capntproto::authority::Grant::root(capntproto::authority::ObjectId::new(7).unwrap(), capntproto::authority::ObjectGeneration::new(11).unwrap(), [1;32], capntproto::authority::Rights::ALL); }", "unused_must_use"),
    ] {
        cases.push((name.to_owned(), source.to_owned(), Some(diagnostic)));
    }
    check_contracts("api-contracts", &format!(
        "capntproto = {{ path = {:?}, default-features = false, features = [\"native\"] }}\nserde_json = \"1\"\n", root()
    ), cases,
        "external Rust API construction, domain separation, immutable grant fields, local thread affinity and must-use checks",
        "not a proof of runtime authority, authentication or cross-network generation uniqueness");
}

#[test]
fn rpc_identifier_and_table_compile_contracts() {
    // Compile the actual private modules in a consumer whose parent has the
    // same visibility as rpc.rs. This tests internal callers' contracts without
    // promoting RPC implementation types into the public API.
    let prelude = format!(
        "#![allow(dead_code)]\n#[path = {:?}] mod rpc_ids;\n#[path = {:?}] mod rpc_tables;\nuse rpc_ids::*;\nuse rpc_tables::*;\n",
        root().join("vendor/capnp-rpc/src/rpc_ids.rs"),
        root().join("vendor/capnp-rpc/src/rpc_tables.rs")
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
            "let table = LocalTable::<QuestionId, ()>::new(); let _ = table.slots;",
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
    ] {
        cases.push((
            name.to_owned(),
            format!("{prelude}fn main() {{ {code} }}"),
            Some(diagnostic),
        ));
    }
    check_contracts("rpc-id-contracts", "", cases,
        "exact private RPC identifier/table source: domain and allocation-role separation, explicit wire conversion, private allocator storage",
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
        "capntproto = {{ path = {:?}, default-features = false, features = [\"services\"] }}\ncapnp = {{ path = {:?} }}\nserde_json = \"1\"\n", root(), root().join("vendor/capnp")
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
        ("route-as-object-generation", "fn wrong(g: capntproto::native_rpc::RouteGeneration) { let _: ObjectGeneration = g; } fn main() {}", "E0308"),
        ("object-as-route-generation", "fn wrong(g: ObjectGeneration) { let _: capntproto::native_rpc::RouteGeneration = g; } fn main() {}", "E0308"),
    ] {
        cases.push((name.to_owned(), format!("{prelude}{code}"), Some(diagnostic)));
    }
    cases.push(("descriptor-must-use".to_owned(), format!("#![deny(unused_must_use)]\n{prelude}fn main() {{ Descriptor::new(ObjectKind::new(1).unwrap(), ObjectId::new(1).unwrap(), ObjectGeneration::new(1).unwrap(), Rights::ALL); }}"), Some("unused_must_use")));
    check_contracts("persistent-descriptor-contracts", &format!(
        "capntproto = {{ path = {:?}, default-features = false, features = [\"storage\", \"native\"] }}\nserde_json = \"1\"\n", root()
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
fn discovery_generation_and_resolved_compile_contracts() {
    let prelude = "use capntproto::native_discovery::{Binding, Directory, DiscoveryGeneration, Resolved, Publication, AdvertisementOptions};\n";
    let mut cases = vec![(
        "positive".to_owned(),
        format!(
            "{prelude}{}",
            r#"
fn thread_safe<T: Send + Sync + Copy>() {}
fn inspect(r: &Resolved, p: &Publication) {
    let _: &Binding = r.binding();
    let _: DiscoveryGeneration = r.generation();
    let _: tokio::time::Instant = r.expires();
    let _: DiscoveryGeneration = p.generation();
}
fn publish(d: &Directory, b: Binding) {
    let g = d.publish("service", b.clone(), None, std::time::Duration::from_secs(1)).unwrap();
    let _ = d.publish("service", b, Some(g), std::time::Duration::from_secs(1));
    let _ = d.revoke("service", [1;32], g);
    let _ = AdvertisementOptions { expected_generation:Some(g), ..Default::default() };
}
fn main() {
    thread_safe::<DiscoveryGeneration>();
    let max = DiscoveryGeneration::new(u64::MAX).unwrap();
    assert_eq!(max.get(), u64::MAX);
    assert!(DiscoveryGeneration::new(0).is_none());
}
"#
        ),
        None,
    )];
    for (name, code, diagnostic) in [
        ("generation-constructor", "fn wrong() { let _ = DiscoveryGeneration(std::num::NonZeroU64::new(1).unwrap()); }", "E0423"),
        ("generation-field", "fn wrong(g: &mut DiscoveryGeneration) { g.0 = std::num::NonZeroU64::new(1).unwrap(); }", "E0616"),
        ("raw-generation", "fn wrong() { let _: DiscoveryGeneration = 1u64; }", "E0308"),
        ("implicit-generation", "fn wrong() { let _: DiscoveryGeneration = 1u64.into(); }", "E0277"),
        ("generation-decay", "fn wrong(g: DiscoveryGeneration) { let _: u64 = g; }", "E0308"),
        ("generation-default", "fn wrong() { let _: DiscoveryGeneration = Default::default(); }", "E0277"),
        ("generation-arithmetic", "fn wrong(g: DiscoveryGeneration) { let _ = g + 1; }", "E0369"),
        ("generation-deserialize", "fn wrong() { let _ = serde_json::from_str::<DiscoveryGeneration>(\"1\"); }", "E0277"),
        ("object-generation", "fn wrong(g: capntproto::authority::ObjectGeneration) { let _: DiscoveryGeneration = g; }", "E0308"),
        ("route-generation", "fn wrong(g: capntproto::native_rpc::RouteGeneration) { let _: DiscoveryGeneration = g; }", "E0308"),
        ("revision-generation", "fn wrong(g: capntproto::semantics::Revision) { let _: DiscoveryGeneration = g; }", "E0308"),
        ("raw-create", "fn wrong(d: &Directory, b: Binding) { let _ = d.publish(\"service\", b, 0, std::time::Duration::from_secs(1)); }", "E0308"),
        ("raw-renew", "fn wrong(d: &Directory, b: Binding) { let _ = d.maintain(\"service\", b, Some(1u64), std::time::Duration::from_secs(1)); }", "E0308"),
        ("raw-revoke", "fn wrong(d: &Directory) { let _ = d.revoke(\"service\", [1;32], 1u64); }", "E0308"),
        ("absence-revoke", "fn wrong(d: &Directory) { let _ = d.revoke(\"service\", [1;32], None); }", "E0308"),
        ("advertisement-generation", "fn wrong() { let _ = AdvertisementOptions { expected_generation:Some(1u64), ..Default::default() }; }", "E0308"),
        ("resolved-constructor", "fn wrong(b: Binding, g: DiscoveryGeneration, t: tokio::time::Instant) { let _ = Resolved { binding:b, generation:g, expires:t }; }", "E0451"),
        ("resolved-binding", "fn wrong(r: &mut Resolved, b: Binding) { r.binding = b; }", "E0616"),
        ("resolved-generation", "fn wrong(r: &mut Resolved, g: DiscoveryGeneration) { r.generation = g; }", "E0616"),
        ("resolved-expiry", "fn wrong(r: &mut Resolved, t: tokio::time::Instant) { r.expires = t; }", "E0616"),
        ("resolved-readonly-host", "fn wrong(r: &mut Resolved) { r.binding().host = [0;32]; }", "E0594"),
        ("resolved-readonly-context", "fn wrong(r: &mut Resolved) { r.binding().context.clear(); }", "E0596"),
        ("resolved-default", "fn wrong() { let _: Resolved = Default::default(); }", "E0277"),
        ("resolved-deserialize", "fn wrong() { let _ = serde_json::from_str::<Resolved>(\"{}\"); }", "E0277"),
        ("resolved-clone", "fn wrong(r: Resolved) { let _ = r.clone(); }", "E0599"),
        ("resolved-send", "fn send<T: Send>() {} fn wrong() { send::<Resolved>(); }", "E0277"),
        ("resolved-sync", "fn sync<T: Sync>() {} fn wrong() { sync::<Resolved>(); }", "E0277"),
        ("generation-must-use", "fn wrong() { DiscoveryGeneration::new(1).unwrap(); }", "unused_must_use"),
        ("resolved-must-use", "async fn wrong(d: &capntproto::native_discovery::Discovery) { d.resolve([1;32], \"service\").await.unwrap(); }", "unused_must_use"),
    ] {
        cases.push((name.to_owned(), format!("#![deny(unused_must_use)]\n{prelude}{code}\nfn main() {{}}"), Some(diagnostic)));
    }
    check_contracts("discovery-type-contracts", &format!(
        "capntproto = {{ path = {:?}, default-features = false, features = [\"native\"] }}\nserde_json = \"1\"\ntokio = {{ version = \"1\", features = [\"time\"] }}\n", root()
    ), cases,
        "nonzero discovery generation domain across administrative APIs and immutable recipient/host/provider/context/expiry lookup bindings; explicit numeric imports and local capability ownership",
        "generations are directory-local metadata, not branded ownership or authentication; expiry, CAS, provider authority and Native authentication remain runtime checks");
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

#[test]
fn datagram_batch_compile_contracts() {
    // Compile the exact private production module, without exporting a testing
    // API. The surrounding constants reproduce the production queue bounds.
    let prelude = format!(
        "const DATAGRAM_QUEUE: usize = 64;\nconst MAX_DATAGRAM_BYTES: usize = 1024;\n#[path = {:?}] mod batch;\nuse batch::Batch;\nuse tokio::sync::mpsc;\n",
        root().join("src/transport/datagram_batch.rs")
    );
    let mut cases = vec![(
        "positive".to_owned(),
        format!(
            "{prelude}{}",
            r#"
fn main() {
    let (sender, mut receiver) = mpsc::channel(4);
    let packets = vec![vec![1], vec![2]];
    let reservation = batch::reserve(&sender, &packets).unwrap();
    reservation.send();
    assert_eq!(receiver.try_recv().unwrap(), packets[0]);
    drop(batch::reserve(&sender, &packets).unwrap());
}
"#
        ),
        None,
    )];
    for (name, code, diagnostic) in [
        ("private-constructor", "fn wrong<'a>(p: mpsc::PermitIterator<'a, Vec<u8>>, b: &'a [Vec<u8>]) { let _ = Batch { permits:p, packets:b }; }", "E0451"),
        ("packets-private", "fn wrong(b: &Batch<'_>) { let _ = b.packets; }", "E0616"),
        ("permits-private", "fn wrong(b: &Batch<'_>) { let _ = &b.permits; }", "E0616"),
        ("packet-mutation", "fn wrong() { let (s, _r) = mpsc::channel(4); let mut p = vec![vec![1]]; let b = batch::reserve(&s, &p).unwrap(); p[0][0] = 2; b.send(); }", "E0502"),
        ("packet-drop", "fn wrong() { let (s, _r) = mpsc::channel(4); let p = vec![vec![1]]; let b = batch::reserve(&s, &p).unwrap(); drop(p); b.send(); }", "E0505"),
        ("sender-drop", "fn wrong() { let (s, _r) = mpsc::channel(4); let p = vec![vec![1]]; let b = batch::reserve(&s, &p).unwrap(); drop(s); b.send(); }", "E0505"),
        ("send-twice", "fn wrong(b: Batch<'_>) { b.send(); b.send(); }", "E0382"),
        ("no-clone", "fn wrong(b: Batch<'_>) { let _ = b.clone(); }", "E0599"),
        ("no-copy", "fn require<T: Copy>() {} fn wrong() { require::<Batch<'_>>(); }", "E0277"),
        ("no-default", "fn wrong() { let _: Batch<'_> = Default::default(); }", "E0277"),
        ("local-packets-escape", "fn wrong(s: &mpsc::Sender<Vec<u8>>) -> Batch<'_> { let p = vec![vec![1]]; batch::reserve(s, &p).unwrap() }", "E0515"),
        ("must-use", "fn wrong(s: &mpsc::Sender<Vec<u8>>, p: &[Vec<u8>]) { batch::reserve(s, p).unwrap(); }", "unused_must_use"),
    ] {
        cases.push((name.to_owned(), format!("#![deny(unused_must_use)]\n{prelude}{code}\nfn main() {{}}"), Some(diagnostic)));
    }
    check_contracts("datagram-batch-contracts", "tokio = { version = \"1\", features = [\"sync\"] }\n", cases,
        "exact private datagram batch module: validated queue/packet binding, immutable borrowed payloads, one-shot sending, lifetimes and must-use",
        "the compiler does not enforce that the caller commits its protocol state before send; native reentrant-waker regressions and TLC traces check that ordering; no remote delivery guarantee");
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
