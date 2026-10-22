//! Compile external consumers of public APIs and the exact private RPC table
//! modules. Check exact Rust diagnostics after a positive control; all
//! orchestration is an ordinary Cargo integration test.
use capntproto_test_support::verification::{self as v, command, root, run};
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs};

#[path = "api_contracts/rpc_reply.rs"]
mod rpc_reply;

#[test]
fn bounded_graph_domain_compile_contracts() {
    let prelude = "use capntproto_rpc_engine::{connection, clients, imports, exports, questions, answers, ids::{ImportId, WireId}};";
    let storage = "let mut is = [imports::Slot::<()>::EMPTY]; let mut es = [exports::Slot::EMPTY]; let mut qs = [questions::Slot::EMPTY]; let mut ans = [answers::Slot::<clients::Client<'_>>::EMPTY]; let mut nodes = [clients::Slot::<imports::Local<'_>, u8>::EMPTY];";
    let bind = "domain.bind(connection::GraphStorage {
                notifications: capntproto_rpc_engine::clients::NotificationStorage::empty(), imports: &mut is, exports: &mut es, questions: &mut qs, answers: &mut ans, clients: &mut nodes }).unwrap()";
    let body = "let local = connection.ledgers.imports.admit(ImportId::from_wire(7), ()).unwrap(); let client = connection.clients.endpoint(local, clients::Location::Remote(1)).unwrap(); let mut batch = connection.ledgers.exports.batch(); let grant = batch.export(client).unwrap(); batch.commit(); let client = connection.ledgers.exports.release(grant.id(), 1).unwrap().unwrap(); connection.clients.release(client); let local = connection.clients.take_retired().unwrap(); connection.ledgers.imports.release(local);";
    let mut cases = vec![("positive".into(), format!("{prelude} fn main() {{ connection::with_domain(|domain| {{ {storage} let mut connection = {bind}; {body} }}); }}"), None)];
    cases.push(("bind-domain-only-once".into(), format!("{prelude} fn main() {{ connection::with_domain(|domain| {{ {storage} let connection = {bind}; drop(connection); let _again = {bind}; }}); }}"), Some("E0382")));
    cases.push(("domain-not-copy".into(), format!("{prelude} fn require<T: Copy>() {{}} fn main() {{ require::<connection::Domain<'_>>(); }}"), Some("E0277")));
    cases.push(("domain-not-forgeable".into(), format!("{prelude} fn main() {{ let _ = connection::Domain {{ brand: core::marker::PhantomData }}; }}"), Some("E0451")));
    cases.push((
        "domain-cannot-escape".into(),
        format!("{prelude} fn main() {{ let _ = connection::with_domain(|domain| domain); }}"),
        Some("lifetime may not live long enough"),
    ));
    check_contracts("bounded-graph-domain-contracts", &format!("capntproto-rpc-engine = {{ path = {:?} }}\n", root().join("crates/capntproto-rpc-engine")), cases,
        "joint connection domains permit graph endpoints to own imports while exports and answers own graph clients; the generative domain cannot be forged, duplicated, reused or escaped",
        "terminal scope teardown and explicit resource draining remain runtime responsibilities");
}

#[test]
fn bounded_owned_rpc_compile_contracts() {
    let prelude = "use capntproto_rpc_engine::{messages, imports, exports, answers}; use capntproto_rpc_wire::{calls, owned};";
    let signature = "fn usage<'p, 'i, 'e, 'a>(request: owned::Call<'p, 'i, 'e, 'a, (), 1, 0>, messages: &mut messages::Messages<'p, '_, '_>, imports: &mut imports::Imports<'i, '_, ()>, exports: &mut exports::Exports<'e, '_, ()>, answers: &mut answers::Answers<'a, '_, ()>)";
    let mut cases = vec![("positive".into(), format!("{prelude} {signature} {{ let future = request.with_async(messages, async |_| ()); let writer = messages.reserve(4).unwrap(); messages.discard(writer); drop(future); request.release(messages, imports, exports, answers); }} fn main() {{}}"), None)];
    cases.push(("release-suspended-request".into(), format!("{prelude} {signature} {{ let future = request.with_async(messages, async |_| ()); request.release(messages, imports, exports, answers); drop(future); }} fn main() {{}}"), Some("E0505")));
    cases.push(("immutable-receipt-origin".into(), format!("{prelude} fn wrong(request: owned::Request<'_, '_, '_, '_, '_, (), 1>) {{ let _ = request.incoming; }} fn main() {{}}"), Some("E0616")));
    cases.push((
        "frame-lease-private".into(),
        format!(
            "{prelude} fn wrong(frame: owned::Frame<'_>) {{ let _ = frame.lease; }} fn main() {{}}"
        ),
        Some("E0616"),
    ));
    cases.push(("frame-release-once".into(), format!("{prelude} fn wrong<'p>(frame: owned::Frame<'p>, messages: &mut messages::Messages<'p, '_, '_>) {{ frame.release(messages); frame.release(messages); }} fn main() {{}}"), Some("E0382")));
    cases.push((
        "no-copy-frame".into(),
        format!(
            "{prelude} fn require<T: Copy>() {{}} fn main() {{ require::<owned::Frame<'_>>(); }}"
        ),
        Some("E0277"),
    ));
    let setup = "let mut bytes = [0u8; 8]; let mut buffers = [messages::Buffer::new(&mut bytes)]; messages::with_messages(&mut buffers, &mut [], |messages| { let writer = messages.reserve(8).unwrap(); let mut writing = messages.write_owned(writer).unwrap();";
    cases.push(("writer-moved-into-task".into(), format!("{prelude} fn main() {{ {setup} messages.discard(writer); drop(writing); }}).unwrap(); }}"), Some("E0382")));
    cases.push(("recover-view-while-borrowed".into(), format!("{prelude} fn main() {{ {setup} let bytes = writing.bytes(); let writer = writing.into_writer(); bytes[0] = 1; messages.discard(writer); }}).unwrap(); }}"), Some("E0505")));
    check_contracts("bounded-owned-rpc-contracts", &format!("capntproto-rpc-engine = {{ path = {:?} }}\ncapntproto-rpc-wire = {{ path = {:?} }}\n", root().join("crates/capntproto-rpc-engine"), root().join("crates/capntproto-rpc-wire")), cases,
        "owning requests keep their immutable receipt origin and frame across suspension; affine frames and writable task reservations cannot be released or recovered twice; independent pool operations remain available",
        "terminally forgotten tasks retain reservations until scope teardown; protocol admission, cancellation policy and wire cleanup require runtime checks");
}

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
fn bounded_connection_and_adoption_compile_contracts() {
    let prelude = "use capntproto_rpc_engine::{connection::{with_connection, Storage}, imports, exports, questions, answers, ids::{ImportId, AnswerId, WireId}};";
    let storage = "let mut is = [imports::Slot::<i32>::EMPTY]; let mut es = [exports::Slot::<i32>::EMPTY]; let mut qs = [questions::Slot::EMPTY]; let mut as_ = [answers::Slot::<i32>::EMPTY];";
    let scope = "with_connection(Storage { imports:&mut is, exports:&mut es, questions:&mut qs, answers:&mut as_ }, |connection|";
    let setup = "let id = AnswerId::from_wire(17); connection.answers.prepare_admission(id, None).unwrap().redirected().accept(41); let pending = connection.answers.prepare_adoption(id).unwrap();";
    let mut cases = vec![("positive".into(), format!("{prelude} fn main() {{ {storage} {scope} {{ {setup} let adopted = pending.accept(); assert_eq!(connection.answers.adopted(&adopted), &41); connection.answers.release_adopted(adopted); }}).unwrap(); }}"), None)];
    for (name, code, error) in [
        ("adopt-once", "pending.accept(); pending.accept();", "E0382"),
        (
            "finish-during-adoption",
            "connection.answers.finish(id, false); pending.accept();",
            "E0499",
        ),
        (
            "adopted-not-copy",
            "drop(pending); fn require<T: Copy>() {} require::<answers::Adopted<'_>>();",
            "E0277",
        ),
    ] {
        cases.push((
            name.into(),
            format!("{prelude} fn main() {{ {storage} {scope} {{ {setup} {code} }}).unwrap(); }}"),
            Some(error),
        ));
    }
    cases.push(("cross-connection-import".into(), format!("{prelude} fn main() {{ {storage} {scope} {{ let local = connection.imports.admit(ImportId::from_wire(3), 41).unwrap(); {storage} {scope} {{ connection.imports.release(local); }}).unwrap(); }}).unwrap(); }}"), Some("E0521")));
    cases.push(("connection-handle-escape".into(), format!("{prelude} fn main() {{ {storage} let _outside = {scope} {{ connection.questions.reserve().unwrap().accept() }}).unwrap(); }}"), Some("lifetime may not live long enough")));
    check_contracts("bounded-connection-adoption-contracts", &format!("capntproto-rpc-engine = {{ path = {:?} }}\n", root().join("crates/capntproto-rpc-engine")), cases,
        "one connection scope keeps role-specific handles local; adoption is affine and excludes concurrent Finish mutation",
        "runtime transitions validate wire IDs, once-only adoption and producer retention; the scope alone is not a complete dispatcher");
}

#[test]
fn bounded_reflected_publication_compile_contracts() {
    let prelude = "use capntproto_rpc_engine::messages; use capntproto_rpc_wire::publication::reflected::{Publication, Reference}; type P<'a> = Publication<'a,'a,'a,'a,'a,'a,'a,'a,'a,'a,'a,(),()>;";
    let mut cases = vec![("positive".into(), format!("{prelude} fn accept<'a>(p: P<'a>, slot: messages::QueueSlot<'a,'a,'a>) {{ let _ = p.accept_into(slot); }} fn main() {{}}"), None)];
    for (name, code, error) in [
        ("publication-once", "fn wrong<'a>(p: P<'a>, first: messages::QueueSlot<'a,'a,'a>, second: messages::QueueSlot<'a,'a,'a>) { p.accept_into(first); p.accept_into(second); }", "E0382"),
        ("no-unordered-accept", "fn wrong(p: P<'_>) { p.accept(); }", "E0599"),
        ("references-private", "fn wrong(p: P<'_>) { let _ = p.references; }", "E0616"),
        ("reference-not-copy", "fn require<T: Copy>() {} fn wrong() { require::<Reference<'_, '_, '_>>(); }", "E0277"),
        ("reference-move-once", "fn wrong<'a>(local: capntproto_rpc_engine::imports::Local<'a>) { let _first = Reference::Hosted(local); let _second = Reference::Hosted(local); }", "E0382"),
    ] { cases.push((name.into(), format!("{prelude} {code} fn main() {{}}"), Some(error))); }
    check_contracts("bounded-reflected-publication-contracts", &format!("capntproto-rpc-engine = {{ path = {:?} }}\ncapntproto-rpc-wire = {{ path = {:?} }}\n", root().join("crates/capntproto-rpc-engine"), root().join("crates/capntproto-rpc-wire")), cases,
        "reflected outgoing references are affine and publication only permits one ordered acceptance",
        "wire identity, transform matching and cleanup FIFO order require runtime validation and fault tests");
}

#[test]
fn bounded_reflected_capability_compile_contracts() {
    let prelude = "use capntproto_rpc_engine::exports::{with_exports, Slot, LeaseSlot};";
    let setup = "let mut slots = [Slot::EMPTY]; with_exports(&mut slots, |exports| { let mut batch = exports.batch(); let grant = batch.export(41).unwrap(); batch.commit(); let mut receipts = [LeaseSlot::new(grant.id())]; let pending = exports.prepare_leases(&mut receipts).unwrap();";
    let mut cases = vec![("positive".into(), format!("{prelude} fn main() {{ {setup} let receipts = pending.accept(); let lease = receipts[0].take().unwrap(); assert_eq!(exports.get(&lease), &41); exports.release_local(lease); }}).unwrap(); }}"), None)];
    for (name, code, error) in [
        ("double-reflected-admission", "pending.accept(); pending.accept();", "E0382"),
        ("release-during-reflected-admission", "exports.release(grant.id(), 1); pending.accept();", "E0499"),
        ("replace-reflected-receipt", "receipts[0] = LeaseSlot::none(); pending.accept();", "E0506"),
        ("cross-reflected-domain", "let receipts = pending.accept(); let lease = receipts[0].take().unwrap(); let mut other = [Slot::<i32>::EMPTY]; with_exports(&mut other, |other| { other.release_local(lease); }).unwrap();", "E0521"),
    ] { cases.push((name.into(), format!("{prelude} fn main() {{ {setup} {code} }}).unwrap(); }}"), Some(error))); }
    check_contracts("bounded-reflected-capability-contracts", &format!("capntproto-rpc-engine = {{ path = {:?} }}\n", root().join("crates/capntproto-rpc-engine")), cases,
        "reflected local descriptors have exclusive preflight, immutable receipt plans and scoped noncopy ownership",
        "full wire admission separately matches every descriptor and target before accepting the batch");
}

#[test]
fn bounded_promise_and_embargo_compile_contracts() {
    let prelude = "use capntproto_rpc_engine::{imports::{with_imports, Slot, ReceiveSlot}, ids::{ImportId, WireId}};";
    let setup = "let mut slots = [Slot::EMPTY]; with_imports(&mut slots, |imports| { let id = ImportId::from_wire(17); let mut first = [ReceiveSlot::new_promise(id, 41)]; imports.prepare_receive(&mut first).unwrap().commit(); let mut empty = []; let pending = imports.prepare_resolution(id, &mut empty).unwrap();";
    let mut cases = vec![("positive".into(), format!("{prelude} fn main() {{ {setup} let (target, _) = pending.commit(); assert!(target.is_some()); }}).unwrap(); }}"), None)];
    for (name, code, error) in [
        ("resolve-twice", "pending.commit(); pending.commit();", "E0382"),
        ("release-during-resolve", "imports.release(first[0].take_local().unwrap()); pending.commit();", "E0499"),
        ("cross-resolution-domain", "let (target, _) = pending.commit(); let mut other = [Slot::<i32>::EMPTY]; with_imports(&mut other, |other| { let _ = other.resolve(target.unwrap()); }).unwrap();", "E0521"),
    ] { cases.push((name.into(), format!("{prelude} fn main() {{ {setup} {code} }}).unwrap(); }}"), Some(error))); }
    let prelude =
        "use capntproto_rpc_engine::{embargoes::Embargoes, peer_table::{BorrowedSparse, Slot}};";
    let setup = "let mut slots = [Slot::EMPTY]; let mut table = Embargoes::new(BorrowedSparse::new(&mut slots)).ok().unwrap(); let pending = table.reserve(41).unwrap();";
    for (name, code, error) in [
        (
            "accept-embargo-twice",
            "pending.accept(); pending.accept();",
            "E0382",
        ),
        (
            "echo-during-reservation",
            "let id = pending.id(); let _ = table.receive(id); pending.accept();",
            "E0499",
        ),
    ] {
        cases.push((
            name.into(),
            format!("{prelude} fn main() {{ {setup} {code} }}"),
            Some(error),
        ));
    }
    check_contracts("bounded-promise-embargo-contracts", &format!("capntproto-rpc-engine = {{ path = {:?} }}\n", root().join("crates/capntproto-rpc-engine")), cases,
        "single Resolve/embargo acceptance, exclusive reference preflight and connection-local resolution targets",
        "wire admission binds output to the exact FIFO frame; dispatch and full connection composition remain separate");
}

#[test]
fn bounded_input_admission_compile_contracts() {
    let prelude =
        "use capntproto_rpc_engine::{answers::{with_answers, Slot}, ids::{AnswerId, WireId}};";
    let setup = "let mut slots = [const { Slot::EMPTY }; 2]; with_answers(&mut slots, |answers| { let parent = answers.admit(AnswerId::from_wire(1), 17).unwrap(); let pending = answers.prepare_admission(AnswerId::from_wire(2), Some(parent.id())).unwrap();";
    let mut cases = vec![("positive".into(), format!("{prelude} fn main() {{ {setup} let (key, parent) = pending.accept(23); assert_eq!(answers.get(key), Ok(&23)); assert_eq!(answers.pipelined(&parent.unwrap()), &17); }}).unwrap(); }}"), None)];
    for (name, code, error) in [
        ("double-input-acceptance", "let _ = pending.accept(23); let _ = pending.accept(29);", "E0382"),
        ("finish-during-input-preflight", "answers.finish(parent.id(), false); let _ = pending.accept(23);", "E0499"),
        ("cross-input-pipeline", "let (_, parent) = pending.accept(23); let mut other = [Slot::<i32>::EMPTY]; with_answers(&mut other, |other| { let _ = other.release_pipeline(parent.unwrap()); }).unwrap();", "E0521"),
    ] { cases.push((name.into(), format!("{prelude} fn main() {{ {setup} {code} }}).unwrap(); }}"), Some(error))); }
    let prelude = "use capntproto_rpc_engine::exports::{with_exports, Slot};";
    let setup = "let mut slots = [Slot::EMPTY]; with_exports(&mut slots, |exports| { let mut batch = exports.batch(); let grant = batch.export(17).unwrap(); batch.commit(); let mut scratch = [0]; let mut pending = exports.prepare_releases(&mut scratch).unwrap(); pending.record(grant.id(), 1).unwrap(); let ready = pending.finish().unwrap();";
    for (name, code, error) in [
        (
            "double-release-batch-acceptance",
            "ready.accept(); ready.accept();",
            "E0382",
        ),
        (
            "change-prepared-release-counts",
            "scratch[0] = 2; ready.accept();",
            "E0506",
        ),
        (
            "release-during-preflight",
            "let _ = exports.release(grant.id(), 1); ready.accept();",
            "E0499",
        ),
    ] {
        cases.push((
            name.into(),
            format!("{prelude} fn main() {{ {setup} {code} }}).unwrap(); }}"),
            Some(error),
        ));
    }
    check_contracts("bounded-input-admission-contracts", &format!("capntproto-rpc-engine = {{ path = {:?} }}\n", root().join("crates/capntproto-rpc-engine")), cases,
        "exclusive input preflight, immutable release counts, single acceptance and connection-local pipeline ownership",
        "the wire layer must bind these tickets to the exact received and retained sent frames");
}

#[test]
fn bounded_async_scope_compile_contracts() {
    let prelude = "use capntproto_rpc_engine::{messages::{with_messages_async, Buffer, SendSlot}, questions::{with_questions_async, Slot}};";
    let setup = "let mut bytes = [0; 8]; let mut buffers = [Buffer::new(&mut bytes)]; let mut sends = [SendSlot::EMPTY]; with_messages_async(&mut buffers, &mut sends, async |messages| { let writer = messages.reserve(8).unwrap(); let lease = messages.seal(writer, 8).unwrap(); let view = messages.read(&lease); core::future::ready(()).await;";
    let mut cases = vec![("positive".into(), format!("{prelude} async fn check() {{ {setup} let offer = messages.offer(&lease).unwrap(); let key = messages.accept(offer); messages.send_window(key).unwrap().advance(8).unwrap(); assert_eq!(view.len(), 8); drop(view); messages.release(lease); }}).await.unwrap(); }} fn main() {{ let _ = check(); }}"), None)];
    for (name, code, error) in [
        ("release-suspended-view", "messages.release(lease); assert_eq!(view.len(), 8);", "E0505"),
        ("cross-async-message-domain", "drop(view); let mut other_bytes = [0; 8]; let mut other = [Buffer::new(&mut other_bytes)]; with_messages_async(&mut other, &mut [], async |other| { other.release(lease); }).await.unwrap();", "lifetime may not live long enough"),
    ] {
        cases.push((name.into(), format!("{prelude} async fn check() {{ {setup} {code} }}).await.unwrap(); }} fn main() {{ let _ = check(); }}"), Some(error)));
    }
    cases.push(("cross-async-question-domain".into(), format!("{prelude} async fn check() {{ let mut slots = [Slot::EMPTY]; with_questions_async(&mut slots, async |questions| {{ let call = questions.reserve().unwrap().accept(); let key = call.key(); let mut other = [Slot::EMPTY]; with_questions_async(&mut other, async |other| {{ let _ = other.is_live(key); }}).await.unwrap(); }}).await.unwrap(); }} fn main() {{ let _ = check(); }}"), Some("lifetime may not live long enough")));
    check_contracts(
        "bounded-async-scope-contracts",
        &format!(
            "capntproto-rpc-engine = {{ path = {:?} }}\n",
            root().join("crates/capntproto-rpc-engine")
        ),
        cases,
        "suspended views retain their lease and async message/question scopes remain distinct",
        "the caller supplies pinning, polling and quiesces IO before terminal cancellation",
    );
}

#[test]
fn bounded_pipeline_and_streaming_compile_contracts() {
    let prelude = "use capntproto_portable_check::portable_capnp::{echo, api::{Record, echo::{CapabilityResults, EchoResults, ListResults}}};";
    let setup = "let mut storage = [0u16; 4];";
    let path = "echo::Owned::CAPABILITY.pipeline(&mut storage).capability(CapabilityResults::VALUE).unwrap()";
    let mut cases = vec![(
        "positive".into(),
        format!(
            "{prelude} fn main() {{ {setup} let path = {path}; assert_eq!(path.fields(), &[0]); }}"
        ),
        None,
    )];
    for (name, code, diagnostic) in [
        ("wrong-path-schema", "let _ = echo::Owned::CAPABILITY.pipeline(&mut storage).field(Record::NESTED);".into(), "E0308"),
        ("scalar-is-not-capability", "let _ = echo::Owned::ECHO.pipeline(&mut storage).capability(EchoResults::VALUE);".into(), "E0308"),
        ("list-is-not-capability", "let _ = echo::Owned::LIST.pipeline(&mut storage).capability(ListResults::VALUES);".into(), "E0308"),
        ("stream-has-no-pipeline", "let _ = echo::Owned::WRITE.pipeline(&mut storage);".into(), "E0599"),
        ("stream-has-no-results", "fn check(content: capnp::any_pointer::Reader<'_>) { let _ = echo::Owned::WRITE.read_results(content); }".into(), "E0599"),
        ("path-keeps-result-schema", format!("let _: capnp::wire_capability::CapabilityPath<'_, EchoResults, echo::Owned> = {path};"), "E0308"),
        ("path-keeps-storage-borrow", format!("let path = {path}; storage[0] = 7; assert_eq!(path.fields(), &[0]);"), "E0506"),
    ] {
        cases.push((name.into(), format!("{prelude} fn main() {{ {setup} {code} }}"), Some(diagnostic)));
    }
    check_contracts("bounded-pipeline-streaming-contracts", &format!("capnp = {{ package = \"capntproto-core\", path = {:?}, default-features = false }}\ncapntproto-portable-check = {{ path = {:?} }}\n", root().join("crates/capntproto-core"), root().join("verification/portable")), cases,
        "schema-typed result paths, capability-only endpoints, caller-owned path storage and streaming method roles",
        "wire union selections and capacity are checked at runtime; question ownership and dispatch belong to the connection");
}

#[test]
fn bounded_import_batch_compile_contracts() {
    let prelude = "use capntproto_rpc_engine::{imports::{with_imports, Slot, ReceiveSlot}, ids::{ImportId, WireId}};";
    let setup = "let mut slots = [Slot::EMPTY]; with_imports(&mut slots, |imports| { let mut receipts = [ReceiveSlot::new(ImportId::from_wire(9), 17)]; let prepared = imports.prepare_receive(&mut receipts).unwrap();";
    let mut cases = vec![("positive".into(), format!("{prelude} fn main() {{ {setup} prepared.commit(); let local = receipts[0].take_local().unwrap(); assert_eq!(imports.get(&local), &17); imports.release(local); }}).unwrap(); }}"), None)];
    for (name, code, diagnostic) in [
        ("double-receive-commit", "prepared.commit(); prepared.commit();", "E0382"),
        ("mutate-receipt-during-preflight", "let _ = receipts[0].take_owner(); prepared.commit();", "E0499"),
        ("receive-during-preflight", "let _ = imports.receive(ImportId::from_wire(9)); prepared.commit();", "E0499"),
        ("cross-import-batch", "prepared.commit(); let local = receipts[0].take_local().unwrap(); let mut other = [Slot::<i32>::EMPTY]; with_imports(&mut other, |other| { other.release(local); }).unwrap();", "E0521"),
    ] {
        cases.push((name.into(), format!("{prelude} fn main() {{ {setup} {code} }}).unwrap(); }}"), Some(diagnostic)));
    }
    check_contracts(
        "bounded-import-batch-contracts",
        &format!(
            "capntproto-rpc-engine = {{ path = {:?} }}\n",
            root().join("crates/capntproto-rpc-engine")
        ),
        cases,
        "exclusive import/receipt preflight, single acceptance and invariant connection ownership",
        "the receiver must bind receipt IDs to validated wire descriptors before acceptance",
    );
}

#[test]
fn bounded_call_and_fifo_compile_contracts() {
    let prelude = "use capntproto_rpc_engine::{questions::{with_questions, Slot}, messages::{with_messages, Buffer, SendSlot, Queue}};";
    let setup = "let mut slots = [Slot::EMPTY]; with_questions(&mut slots, |questions| { let local = questions.reserve().unwrap().accept(); let key = local.key();";
    let mut cases = vec![("positive".into(), format!("{prelude} fn main() {{ {setup} questions.returned(key.id(), false).unwrap(); questions.release(local); questions.pending_finish().unwrap().accept(); assert!(questions.is_empty()); }}).unwrap(); }}"), None)];
    for (name, code, error) in [
        ("double-local-release", "questions.release(local); questions.release(local);", "E0382"),
        ("unchecked-local-clone", "let _ = local.clone();", "E0599"),
        ("cross-question-domain", "let mut other = [Slot::EMPTY]; with_questions(&mut other, |other| { let _ = other.is_live(key); }).unwrap();", "E0521"),
        ("double-finish-accept", "questions.release(local); let finish = questions.pending_finish().unwrap(); finish.accept(); finish.accept();", "E0382"),
    ] {
        cases.push((name.into(), format!("{prelude} fn main() {{ {setup} {code} }}).unwrap(); }}"), Some(error)));
    }
    let queue_setup = "let mut bytes = [0; 8]; let mut buffers = [Buffer::new(&mut bytes)]; let mut sends = [SendSlot::EMPTY]; with_messages(&mut buffers, &mut sends, |messages| { let writer = messages.reserve(8).unwrap(); let lease = messages.seal(writer, 8).unwrap(); let offer = messages.offer(&lease).unwrap(); let mut slots = [None]; let mut queue = Queue::new(&mut slots).unwrap();";
    for (name, code, error) in [
        ("copied-send-key-is-not-queue-admission", "let key = messages.accept(offer); queue.reserve().unwrap().accept(messages, key);", "E0308"),
        ("duplicate-offer", "queue.reserve().unwrap().accept(messages, offer); queue.reserve().unwrap().accept(messages, offer);", "E0382"),
        ("duplicate-queue-reservation", "let slot = queue.reserve().unwrap(); slot.accept(messages, offer); drop(slot);", "E0382"),
    ] {
        cases.push((name.into(), format!("{prelude} fn main() {{ {queue_setup} {code} }}).unwrap(); }}"), Some(error)));
    }
    check_contracts("bounded-call-fifo-contracts", &format!("capntproto-rpc-engine = {{ path = {:?} }}\n", root().join("crates/capntproto-rpc-engine")), cases,
        "connection-scoped question identity, affine local/cleanup ownership and single-use FIFO admission",
        "protocol decoding, descriptor authority and shutdown remain connection responsibilities");
}

#[test]
fn bounded_rpc_publication_compile_contracts() {
    let prelude = "use capntproto_rpc_engine::{exports::Batch, messages::{Messages, Lease, SendKey}}; use capntproto_rpc_wire::publication::prepare;";
    let setup = "fn publish<'a, 'e, 't, 'p, 's, 'b>(batch: Batch<'a, 'e, 't, u32>, messages: &'a mut Messages<'p, 's, 'b>, lease: Lease<'p>) { let prepared = prepare(batch, messages, lease, &mut [0; 4], Default::default()).unwrap();";
    let mut cases = vec![("positive".into(), format!("{prelude} {setup} let size = prepared.bytes().len(); let key = prepared.accept(); assert_eq!(messages.send_window(key).unwrap().advance(size), Ok(true)); }} fn main() {{}}"), None)];
    for (name, body, diagnostic) in [
        (
            "double-accept",
            "let _ = prepared.accept(); let _ = prepared.accept();",
            "E0382",
        ),
        ("double-commit", "batch.commit(); drop(prepared);", "E0382"),
        (
            "lease-reuse",
            "drop(prepared); messages.release(lease);",
            "E0382",
        ),
        (
            "mutate-validated-frame",
            "prepared.bytes()[0] = 0;",
            "E0594",
        ),
        (
            "touch-pool-before-accept",
            "let _ = messages.reserve(0); drop(prepared);",
            "E0499",
        ),
        (
            "read-while-accepting",
            "let bytes = prepared.bytes(); let _ = prepared.accept(); assert_eq!(bytes.len(), 0);",
            "E0505",
        ),
        ("private-offer", "let _ = prepared.offer;", "E0616"),
        ("private-batch", "let _ = prepared.batch;", "E0616"),
        (
            "prepared-is-not-send",
            "let _: SendKey<'_> = prepared;",
            "E0308",
        ),
    ] {
        cases.push((
            name.into(),
            format!("{prelude} {setup} {body} }} fn main() {{}}"),
            Some(diagnostic),
        ));
    }
    check_contracts("bounded-publication-contracts", &format!("capntproto-rpc-engine = {{ path = {:?} }}\ncapntproto-rpc-wire = {{ path = {:?} }}\n", root().join("crates/capntproto-rpc-engine"), root().join("crates/capntproto-rpc-wire")), cases,
        "validated frame immutability, exclusive table/pool ownership, no send identity before acceptance and single-use publication",
        "connection identity, queue reservation/order, content-capability indices and transport delivery remain driver responsibilities");
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
fn bounded_export_ownership_compile_contracts() {
    let prelude = "use capntproto_rpc_engine::{exports::{with_exports, Slot, Lease, Key}, ids::{ExportId, WireId}};\n";
    let setup = "let mut slots = [const { Slot::EMPTY }; 1]; with_exports(&mut slots, |exports| { let mut batch = exports.batch(); let grant = batch.export(17).unwrap(); let key = grant.key(); let id = grant.id(); batch.commit(); let lease = exports.lease(id).unwrap();";
    let end = "}).unwrap();";
    let mut cases = vec![("positive".into(), format!("{prelude} fn main() {{ {setup} assert_eq!(exports.release(id, 1), Ok(None)); assert!(exports.resolve(key).is_none()); assert_eq!(exports.get(&lease), &17); assert_eq!(exports.release_local(lease), Some(17)); {end} }}"), None)];
    for (name, code, diagnostic) in [
        ("double-release", "exports.release_local(lease); exports.release_local(lease);", "E0382"),
        ("unchecked-clone", "let _ = lease.clone();", "E0599"),
        ("forge-lease", "let _ = Lease { key };", "E0451"),
        ("forge-key", "let _ = Key { index: 0, generation: 1, brand: core::marker::PhantomData };", "E0451"),
        ("live-owner-view", "let view = exports.get(&lease); exports.release_local(lease); assert_eq!(*view, 17);", "E0502"),
        ("duplicate-batch", "let batch = exports.batch(); let other = exports.batch(); batch.commit(); other.commit();", "E0499"),
        ("double-commit", "let batch = exports.batch(); batch.commit(); batch.commit();", "E0382"),
        ("peer-release-during-batch", "let batch = exports.batch(); exports.release(id, 1).unwrap(); batch.commit();", "E0499"),
        ("local-release-during-batch", "let batch = exports.batch(); exports.release_local(lease); batch.commit();", "E0499"),
        ("ignored-capacity", "exports.batch().export(19);", "unused_must_use"),
        ("cross-domain-lease", "let mut other = [const { Slot::EMPTY }; 1]; with_exports(&mut other, |other| { let _: Option<i32> = other.release_local(lease); }).unwrap();", "E0521"),
        ("cross-domain-key", "let mut other = [const { Slot::EMPTY }; 1]; with_exports(&mut other, |other| { let _: Option<&i32> = other.resolve(key); }).unwrap();", "E0521"),
        ("import-key-role", "fn import_key<'a>(key: capntproto_rpc_engine::imports::Key<'a>) { let _: Key<'a> = key; }", "E0308"),
    ] {
        cases.push((name.into(), format!("#![deny(unused_must_use)]\n{prelude} fn main() {{ {setup} {code} {end} }}"), Some(diagnostic)));
    }
    for (name, token) in [("escape-key", "key"), ("escape-lease", "lease")] {
        cases.push((name.into(), format!("{prelude} fn main() {{ let mut escaped = None; {setup} escaped = Some({token}); {end} drop(escaped); }}"), Some("E0521")));
    }
    check_contracts("bounded-export-contracts", &format!("capntproto-rpc-engine = {{ path = {:?} }}\n", root().join("crates/capntproto-rpc-engine")), cases,
        "invariant export domains, distinct import keys, linear call leases and exclusive publication batches",
        "the adapter must serialize every grant and bind commit to acceptance of that message; wire delivery and complete RPC dispatch are outside this contract");
}

#[test]
fn bounded_import_ownership_compile_contracts() {
    let prelude = "use capntproto_rpc_engine::{imports::{with_imports, Slot, Local, Key}, ids::{ImportId, WireId}};\n";
    let setup = "let mut slots = [const { Slot::EMPTY }; 1]; with_imports(&mut slots, |imports| { let local = imports.admit(ImportId::from_wire(0), 17).unwrap();";
    let end = "}).unwrap();";
    let mut cases = vec![("positive".into(), format!("{prelude} fn main() {{ {setup} let key = local.key(); let alias = imports.retain(&local).unwrap(); imports.release(local); assert_eq!(imports.resolve(key), Some(&17)); imports.release(alias); assert_eq!(imports.resolve(key), None); let release = imports.pending_release(0).unwrap(); assert_eq!(release.commit(), 17); {end} }}"), None)];
    for (name, code, diagnostic) in [
        ("double-release", "imports.release(local); imports.release(local);", "E0382"),
        ("unchecked-clone", "let _ = local.clone();", "E0599"),
        ("forged-local", "let _ = Local { key: local.key() };", "E0451"),
        ("forged-key", "let _ = Key { index: 0, generation: 1, brand: core::marker::PhantomData };", "E0451"),
        ("view-during-release", "let view = imports.get(&local); imports.release(local); assert_eq!(*view, 17);", "E0502"),
        ("duplicate-cleanup", "imports.release(local); let release = imports.pending_release(0).unwrap(); let other = imports.pending_release(0); release.commit(); drop(other);", "E0499"),
        ("double-cleanup-commit", "imports.release(local); let release = imports.pending_release(0).unwrap(); release.commit(); release.commit();", "E0382"),
        ("receive-during-cleanup", "imports.release(local); let release = imports.pending_release(0).unwrap(); let _ = imports.receive(ImportId::from_wire(0)); release.commit();", "E0499"),
        ("unchecked-retain", "imports.retain(&local);", "unused_must_use"),
        ("cross-domain-local", "let mut other = [const { Slot::EMPTY }; 1]; with_imports(&mut other, |other| { let _ = other.admit(ImportId::from_wire(0), 17).unwrap(); other.release(local); }).unwrap();", "E0521"),
        ("cross-domain-key", "let key = local.key(); let mut other = [const { Slot::EMPTY }; 1]; with_imports(&mut other, |other| { let _ = other.admit(ImportId::from_wire(0), 17).unwrap(); let _ = other.resolve(key); }).unwrap();", "E0521"),
    ] {
        cases.push((name.into(), format!("#![deny(unused_must_use)]\n{prelude} fn main() {{ {setup} {code} {end} }}"), Some(diagnostic)));
    }
    // Returning either kind of token would let a callback outlive its connection.
    for (name, token) in [("escape-local", "local"), ("escape-key", "local.key()")] {
        cases.push((name.into(), format!("{prelude} fn main() {{ let mut slots = [const {{ Slot::EMPTY }}; 1]; let mut escaped = None; with_imports(&mut slots, |imports| {{ let local = imports.admit(ImportId::from_wire(0), 17).unwrap(); escaped = Some({token}); }}).unwrap(); drop(escaped); }}"), Some("E0521")));
    }
    check_contracts("bounded-import-contracts", &format!("capntproto-rpc-engine = {{ path = {:?} }}\n", root().join("crates/capntproto-rpc-engine")), cases,
        "invariant connection scopes, unforgeable noncopy local tokens, exclusive views and single-use cleanup acceptance",
        "explicitly forgotten handles retain ownership until terminal teardown; does not prove remote delivery or complete bounded RPC dispatch");
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
use capntproto_portable_check::portable_capnp::api::{BoundedRecord, Child, Collections, Entry};
"#;
    let setup = "let mut words = [word(0,0,0,0,0,0,0,0); 32]; let mut message = FixedBuilder::new(&mut words).unwrap();";
    let mut cases = vec![("positive".into(), format!("{prelude} fn main() {{ {setup} let mut root = message.try_init_root::<BoundedRecord>().unwrap(); root.scalar(BoundedRecord::ID).set(8); root.field(BoundedRecord::TITLE).copy_from(\"bounded\").unwrap(); root.field(BoundedRecord::CHILD).init().unwrap().scalar(Child::ID).set(3); assert_eq!(root.read().id(), 8); }}"), None)];
    for (name, code, diagnostic) in [
        ("generic-legacy-editor", "type B = capntproto_portable_check::portable_capnp::api::Box<Entry>; let mut root = message.try_init_root::<B>().unwrap(); let _ = root.field(B::VALUE).typed().init().unwrap().into_raw();", "E0599"),
        ("generic-wrong-kind", "type B = capntproto_portable_check::portable_capnp::api::Box<capnp::text::Owned>; let mut root = message.try_init_root::<B>().unwrap(); let _ = root.field(B::VALUE).typed().cast::<capnp::field_api::Data>();", "E0599"),
        ("any-live-view", "type B = capntproto_portable_check::portable_capnp::api::Box<capnp::any_pointer::Owned>; let mut root = message.try_init_root::<B>().unwrap(); let value = root.field(B::VALUE).typed().read().unwrap(); root.field(B::VALUE).clear(); assert!(value.is_null());", "E0499"),
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
        "capnp = {{ package = \"capntproto-core\", path = {:?}, default-features = false }}\ncapntproto-portable-check = {{ path = {:?} }}\n",
        root().join("crates/capntproto-core"), root().join("verification/portable")
    ), cases, "typed scalar/pointer descriptors, no legacy growing editor escape, exclusive views and checked growth results", "not a proof of arbitrary user Schema/Scalar implementations or complete bounded RPC");
}

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
#[test]
fn bounded_client_dispatch_compile_contracts() {
    let prelude = "use capntproto_rpc_engine::clients::{self, Client, Location, dispatch::{with_runtime, Storage, TaskSlot}};";
    let storage = "let mut clients = [const { clients::Slot::<u32,u8>::EMPTY }; 3]; let mut tasks = [TaskSlot::<u32>::EMPTY];";
    let scope = "with_runtime(Storage { notifications: capntproto_rpc_engine::clients::NotificationStorage::empty(),
                clients: &mut clients, tasks: &mut tasks }, |runtime|";
    let setup = "let client = runtime.endpoint(7, Location::Local).unwrap(); let mut owner = Some(42); let offered = runtime.prepare_call(&client, &mut owner).unwrap();";
    let mut cases = vec![("positive".into(), format!("{prelude} fn main() {{ {storage} {scope} {{ {setup} offered.accept(); let running = runtime.next_ready().unwrap().unwrap().accept(); assert_eq!(runtime.complete(running), 42); runtime.release(client); assert_eq!(runtime.take_retired(), Some(7)); assert!(runtime.is_empty()); }}).unwrap(); }}"), None)];
    for (name, code, error) in [
        ("enqueue-once", "offered.accept(); offered.accept();", "E0382"),
        ("exclusive-output-admission", "runtime.release(client); offered.accept();", "E0499"),
        ("borrowed-call-owner", "owner.take(); offered.accept();", "E0499"),
        ("complete-once", "offered.accept(); let running = runtime.next_ready().unwrap().unwrap().accept(); runtime.complete(running); runtime.complete(running);", "E0382"),
        ("retained-route-not-copy", "drop(offered); fn require<T: Copy>() {} require::<Client<'_>>();", "E0277"),
    ] { cases.push((name.into(), format!("{prelude} fn main() {{ {storage} {scope} {{ {setup} {code} }}).unwrap(); }}"), Some(error))); }
    cases.push(("cross-runtime-client".into(), format!("{prelude} fn main() {{ {storage} {scope} {{ let client = runtime.endpoint(1, Location::Local).unwrap(); {storage} {scope} {{ runtime.release(client); }}).unwrap(); }}).unwrap(); }}"), Some("E0521")));
    cases.push(("runtime-client-escape".into(), format!("{prelude} fn main() {{ {storage} let _escaped = {scope} {{ runtime.endpoint(1, Location::Local).unwrap() }}).unwrap(); }}"), Some("lifetime may not live long enough")));
    check_contracts("bounded-client-dispatch-contracts", &format!("capntproto-rpc-engine = {{ path = {:?} }}\n", root().join("crates/capntproto-rpc-engine")), cases,
        "scoped clients, exclusive queue admission and affine running tasks prevent cross-runtime ownership, double admission and duplicate completion",
        "protocol publication, descriptor provenance, IO ordering and service execution require the wire/runtime adapters");
}
#[test]
fn bounded_generated_server_compile_contracts() {
    let prelude = "use capnp::{message::{self, FixedBuilder}, field_api::FixedStruct}; use capntproto_portable_check::portable_capnp::{echo, api::echo::{EchoParamsRef,EchoResults}}; use capntproto_rpc_wire::{calls, rpc_capnp}; use capntproto_rpc_engine::ids::{QuestionId,ImportId,WireId}; struct Service; impl echo::Server for Service { async fn echo<'call>(&'call mut self, params: EchoParamsRef<'call>, mut results: FixedStruct<'call,EchoResults>) -> capnp::Result<()> { results.scalar(EchoResults::VALUE).set(params.value()); Ok(()) } }";
    let setup = "let mut input = [capnp::word(0,0,0,0,0,0,0,0); 40]; let mut output = [capnp::word(0,0,0,0,0,0,0,0); 40]; let mut request = FixedBuilder::new(&mut input).unwrap(); let mut response = FixedBuilder::new(&mut output).unwrap(); calls::call(&mut request, QuestionId::from_wire(0), ImportId::from_wire(1), echo::Owned::ECHO, |_| Ok(())).unwrap(); let reader = message::Reader::new(request.get_segments_for_output(), Default::default()); let rpc_capnp::message::Call(call) = reader.get_root::<rpc_capnp::message::Reader<'_>>().unwrap().which().unwrap() else { unreachable!() }; let incoming = calls::Incoming::new(call.unwrap()); let mut service = echo::ServerDispatch::new(Service); let future = incoming.serve(&mut service, &mut response);";
    let mut cases = vec![(
        "positive".into(),
        format!("{prelude} fn main() {{ {setup} drop(future); response.reset(); }}"),
        None,
    )];
    for (name, code, error) in [
        (
            "output-borrowed-during-service",
            "response.reset(); drop(future);",
            "E0499",
        ),
        (
            "request-borrowed-during-service",
            "request.reset(); drop(future);",
            "E0502",
        ),
        (
            "service-owned-by-future",
            "drop(service); drop(future);",
            "E0505",
        ),
    ] {
        cases.push((
            name.into(),
            format!("{prelude} fn main() {{ {setup} {code} }}"),
            Some(error),
        ));
    }
    check_contracts("bounded-generated-server-contracts", &format!("capnp = {{ package = \"capntproto-core\", path = {:?}, default-features = false }}\ncapntproto-rpc-engine = {{ path = {:?} }}\ncapntproto-rpc-wire = {{ path = {:?} }}\ncapntproto-portable-check = {{ path = {:?} }}\n", root().join("crates/capntproto-core"), root().join("crates/capntproto-rpc-engine"), root().join("crates/capntproto-rpc-wire"), root().join("verification/portable")), cases,
        "generated async server futures retain their service, borrowed parameters and fixed result buffer until completion or cancellation",
        "the connection separately owns capability receipts, publication, tail adoption and scheduling");
}

#[test]
fn portable_driver_mutation_is_confined_to_notifying_transitions() {
    let prelude = "use capntproto_rpc_wire::owned::connection::{Driver, Runtime}; use capntproto_rpc_engine::clients::{Capabilities, Client}; type D<'p,'id,'s> = Driver<'p,'id,'s,(),(),u8,u32,4,4>;";
    let mut cases = vec![("positive".into(), format!("{prelude} fn use_scope<'p,'id,'s>(driver: &mut D<'p,'id,'s>, client: Client<'id>) {{ let retained = driver.capabilities().retain_key(client.key()).unwrap(); driver.capabilities().release(retained); driver.capabilities().release(client); let _ = driver.ledgers().questions.is_empty(); }} fn main() {{}}"), None)];
    for field in ["runtime", "ledgers", "imports", "pipelines", "exported"] {
        cases.push((format!("{field}-private"), format!("{prelude} fn wrong(driver: &mut D<'_,'_,'_>) {{ let _ = &mut driver.{field}; }} fn main() {{}}"), Some("E0616")));
    }
    cases.push(("scope-cannot-replace-runtime".into(), format!("{prelude} fn wrong<'p,'id,'s>(driver: &mut D<'p,'id,'s>) {{ let _: &mut Runtime<'p,'id,'s,(),u8,u32,4,4> = &mut driver.capabilities(); }} fn main() {{}}"), Some("E0308")));
    cases.push(("inspection-is-read-only".into(), format!("{prelude} fn wrong<'p,'id,'s>(driver: &mut D<'p,'id,'s>, client: Client<'id>) {{ driver.runtime().release(client); }} fn main() {{}}"), Some("E0596")));
    check_contracts("portable-driver-mutation-contracts", &format!("capntproto-rpc-engine = {{ path = {:?} }}\ncapntproto-rpc-wire = {{ path = {:?} }}\n", root().join("crates/capntproto-rpc-engine"), root().join("crates/capntproto-rpc-wire")), cases,
        "driver-owned runtime and ledgers cannot be replaced; capability transitions and affine publication guards preserve readiness effects",
        "callbacks must still complete or release the affine owners they receive");
}
