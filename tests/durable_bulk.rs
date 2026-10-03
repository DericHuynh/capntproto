use capnp::traits::HasTypeId;
use capntproto::durable_bulk::JournalId;
use capntproto::storage::ObjectKey;
use capntproto::storage::Revision;
use capntproto::{
    authority::{Grant, ObjectGeneration, ObjectId, Rights},
    bulk::{Config, Status},
    bulk_capnp::durable_transfer,
    durable_bulk::{self, Receiver},
    orm::{ObjectServer, ObjectState},
    storage::{Retention, Store, Update},
    store_capnp::{document, object},
};
use futures::FutureExt;
use ring::digest::{digest, SHA256};
use std::{cell::RefCell, rc::Rc, time::Duration};

struct Gate {
    receiver: Receiver<document::Owned>,
    final_reply: bool,
    reached: Rc<std::cell::Cell<bool>>,
}

struct MalformedReply {
    bytes: Vec<u8>,
    checkpoint: bool,
    acknowledgment: bool,
}
impl durable_transfer::Server for MalformedReply {
    async fn describe(
        self: Rc<Self>,
        _: durable_transfer::DescribeParams,
        mut out: durable_transfer::DescribeResults,
    ) -> capnp::Result<()> {
        let c = config(&self.bytes);
        let mut out = out.get();
        let mut config = out.reborrow().init_config();
        config.set_length(c.length());
        config.set_max_chunk_bytes(c.max_chunk_bytes());
        config.set_window_bytes(c.window_bytes());
        config.set_max_chunks(c.max_chunks());
        out.reborrow()
            .init_progress()
            .set_bytes(if self.checkpoint { c.length() + 1 } else { 0 });
        out.set_status(Status::Receiving);
        out.set_sha256(&hash(&self.bytes));
        Ok(())
    }
    async fn write(
        self: Rc<Self>,
        p: durable_transfer::WriteParams,
        mut out: durable_transfer::WriteResults,
    ) -> capnp::Result<()> {
        out.get().set_sequence(if self.acknowledgment {
            0
        } else {
            p.get()?.get_sequence()
        });
        Ok(())
    }
    async fn done(
        self: Rc<Self>,
        _: durable_transfer::DoneParams,
        mut out: durable_transfer::DoneResults,
    ) -> capnp::Result<()> {
        out.get().init_summary().set_bytes(1); // A partial receipt cannot imply success.
        Ok(())
    }
}

#[tokio::test(flavor = "current_thread")]
async fn file_sender_rejects_wrong_source_and_malformed_checkpoints_acknowledgments_and_receipts() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("input");
            let bytes = message("sender validation");
            std::fs::write(&path, &bytes).unwrap();
            for (checkpoint, acknowledgment) in [(true, false), (false, true), (false, false)] {
                let wire = Wire::new(capnp_rpc::new_client(MalformedReply {
                    bytes: bytes.clone(),
                    checkpoint,
                    acknowledgment,
                }));
                assert!(durable_bulk::upload_file(
                    wire.client.clone(),
                    &mut std::fs::File::open(&path).unwrap()
                )
                .await
                .is_err());
                wire.stop().await;
            }
            let db = Rc::new(RefCell::new(Store::open(dir.path().join("db")).unwrap()));
            let r = Receiver::<document::Owned>::create(
                ObjectState::new(db.clone(), ObjectId::new(7).unwrap()),
                JournalId::new(99),
                grant(),
                config(&bytes),
                hash(&bytes),
            )
            .unwrap();
            let wire = Wire::new(r.capability());
            let mut wrong = bytes.clone();
            wrong[bytes.len() - 1] ^= 1;
            std::fs::write(&path, wrong).unwrap();
            assert!(durable_bulk::upload_file(
                wire.client.clone(),
                &mut std::fs::File::open(&path).unwrap()
            )
            .await
            .is_err());
            std::fs::write(&path, b"short").unwrap();
            assert!(durable_bulk::upload_file(
                wire.client.clone(),
                &mut std::fs::File::open(&path).unwrap()
            )
            .await
            .is_err());
            assert_eq!(db.borrow().head(ObjectKey::new(99)), Revision::new(1));
            assert_eq!(db.borrow().head(ObjectKey::new(7)), Revision::INITIAL);
            wire.stop().await;
        })
        .await;
}
impl durable_transfer::Server for Gate {
    async fn write(
        self: Rc<Self>,
        p: durable_transfer::WriteParams,
        mut out: durable_transfer::WriteResults,
    ) -> capnp::Result<()> {
        let p = p.get()?;
        let sequence = p.get_sequence();
        self.receiver.write(sequence, p.get_data()?)?;
        if !self.final_reply {
            self.reached.set(true);
            std::future::pending::<()>().await;
        }
        out.get().set_sequence(sequence);
        Ok(())
    }
    async fn done(
        self: Rc<Self>,
        _: durable_transfer::DoneParams,
        mut out: durable_transfer::DoneResults,
    ) -> capnp::Result<()> {
        let checkpoint = self.receiver.done()?;
        if self.final_reply {
            self.reached.set(true);
            std::future::pending::<()>().await;
        }
        let mut result = out.get();
        result
            .reborrow()
            .init_summary()
            .set_bytes(checkpoint.progress.bytes);
        result
            .reborrow()
            .get_summary()?
            .set_chunks(checkpoint.progress.chunks);
        result.set_revision(checkpoint.revision.get());
        Ok(())
    }
}

#[tokio::test(flavor = "current_thread")]
async fn lost_chunk_and_completion_replies_resume_without_duplicate_commits() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for final_reply in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("db");
                let bytes = message("lost reply");
                let input = dir.path().join("input");
                std::fs::write(&input, &bytes).unwrap();
                {
                    let db = Rc::new(RefCell::new(Store::open(&path).unwrap()));
                    let state = ObjectState::new(db.clone(), ObjectId::new(7).unwrap());
                    let receiver = Receiver::<document::Owned>::create(
                        state,
                        JournalId::new(99),
                        grant(),
                        config(&bytes),
                        hash(&bytes),
                    )
                    .unwrap();
                    let reached = Rc::new(std::cell::Cell::new(false));
                    let wire = Wire::new(capnp_rpc::new_client(Gate {
                        receiver: receiver.clone(),
                        final_reply,
                        reached: reached.clone(),
                    }));
                    let pending = if final_reply {
                        write(&wire.client, 1, &bytes[..bytes.len() / 2])
                            .await
                            .unwrap();
                        write(&wire.client, 2, &bytes[bytes.len() / 2..])
                            .await
                            .unwrap();
                        wire.client
                            .done_request()
                            .send()
                            .promise
                            .map(|r| r.map(|_| ()))
                            .boxed_local()
                    } else {
                        let mut r = wire.client.write_request();
                        r.get().set_sequence(1);
                        r.get().set_data(&bytes[..bytes.len() / 2]);
                        r.send().promise.map(|r| r.map(|_| ())).boxed_local()
                    };
                    let mut pending = pending;
                    tokio::time::timeout(Duration::from_secs(2), async {
                        while !reached.get() {
                            assert!((&mut pending).now_or_never().is_none());
                            tokio::task::yield_now().await;
                        }
                    })
                    .await
                    .unwrap();
                    drop(pending);
                    wire.stop().await;
                    assert_eq!(
                        db.borrow().published(ObjectKey::new(7)),
                        Revision::new(u64::from(final_reply))
                    );
                }
                let db = Rc::new(RefCell::new(Store::open(&path).unwrap()));
                let receiver = Receiver::<document::Owned>::resume(
                    ObjectState::new(db.clone(), ObjectId::new(7).unwrap()),
                    JournalId::new(99),
                    grant(),
                )
                .unwrap();
                let wire = Wire::new(receiver.capability());
                let result = durable_bulk::upload_file(
                    wire.client.clone(),
                    &mut std::fs::File::open(&input).unwrap(),
                )
                .await
                .unwrap();
                assert_eq!(result.revision, Revision::new(1));
                assert_eq!(db.borrow().head(ObjectKey::new(7)), Revision::new(1));
                assert_eq!(db.borrow().head(ObjectKey::new(99)), Revision::new(4)); // Initial + two chunks + receipt, exactly once.
                wire.stop().await;
            }
        })
        .await;
}

#[test]
fn live_capability_payloads_and_final_commit_quota_failure_do_not_publish() {
    use capntproto_test_support::dynamic_test_capnp::external_case;
    let mut payload = capnp::message::Builder::new_default();
    struct Empty;
    impl capntproto_test_support::runtime_test_capnp::harness::Server for Empty {}
    let broken: capntproto_test_support::runtime_test_capnp::harness::Client =
        capnp_rpc::new_client(Empty);
    let mut caps = Vec::new();
    let mut root = payload.init_root::<external_case::Builder>();
    capnp::traits::ImbueMut::imbue_mut(&mut root, &mut caps);
    root.set_token(broken);
    let bytes = capnp::serialize::write_message_to_words(&payload);
    let dir = tempfile::tempdir().unwrap();
    let db = Rc::new(RefCell::new(Store::open(dir.path().join("db")).unwrap()));
    let r = Receiver::<external_case::Owned>::create(
        ObjectState::new(db.clone(), ObjectId::new(7).unwrap()),
        JournalId::new(99),
        grant(),
        config(&bytes),
        hash(&bytes),
    )
    .unwrap();
    r.write(1, &bytes[..bytes.len() / 2]).unwrap();
    r.write(2, &bytes[bytes.len() / 2..]).unwrap();
    assert!(r.done().is_err());
    assert_eq!(db.borrow().head(ObjectKey::new(7)), Revision::INITIAL);
    let dir = tempfile::tempdir().unwrap();
    let db = Rc::new(RefCell::new(Store::open(dir.path().join("db")).unwrap()));
    let bytes = message("quota can grow");
    let r = Receiver::<document::Owned>::create(
        ObjectState::new(db.clone(), ObjectId::new(7).unwrap()),
        JournalId::new(99),
        grant(),
        config(&bytes),
        hash(&bytes),
    )
    .unwrap();
    r.write(1, &bytes[..bytes.len() / 2]).unwrap();
    r.write(2, &bytes[bytes.len() / 2..]).unwrap();
    let old_head = db.borrow().head(ObjectKey::new(99));
    let limits = db.borrow().limits();
    let size = db.borrow().file_bytes();
    db.borrow_mut()
        .set_limits(capntproto::storage::Limits {
            max_file_bytes: size,
            ..limits
        })
        .unwrap();
    assert!(r.done().is_err());
    assert_eq!(db.borrow().head(ObjectKey::new(7)), Revision::INITIAL);
    assert_eq!(db.borrow().head(ObjectKey::new(99)), old_head);
    assert_eq!(r.checkpoint().unwrap().status, Status::Receiving);
    db.borrow_mut().set_limits(limits).unwrap();
    assert_eq!(r.done().unwrap().revision, Revision::new(1));
}

fn message(text: &str) -> Vec<u8> {
    let mut message = capnp::message::Builder::new_default();
    message.init_root::<document::Builder>().set_text(text);
    capnp::serialize::write_message_to_words(&message)
}
fn hash(bytes: &[u8]) -> [u8; 32] {
    digest(&SHA256, bytes).as_ref().try_into().unwrap()
}
fn config(bytes: &[u8]) -> Config {
    Config::new(
        bytes.len() as u64,
        (bytes.len() / 2) as u32,
        bytes.len() as u32,
        2,
    )
    .unwrap()
}
fn grant() -> Grant {
    Grant::root(
        ObjectId::new(7).unwrap(),
        ObjectGeneration::new(1).unwrap(),
        [1; 32],
        Rights::ALL,
    )
}
fn text(store: &Store) -> Option<String> {
    let snapshot = store.get(ObjectKey::new(7)).ok()?;
    assert_eq!(
        &snapshot.bytes()[..8],
        &document::Reader::TYPE_ID.to_le_bytes()
    );
    let mut bytes = &snapshot.bytes()[8..];
    let message = capnp::serialize::read_message_from_flat_slice(
        &mut bytes,
        capnp::message::ReaderOptions::new(),
    )
    .unwrap();
    Some(
        message
            .get_root::<document::Reader>()
            .unwrap()
            .get_text()
            .unwrap()
            .to_str()
            .unwrap()
            .into(),
    )
}
struct Wire {
    client: durable_transfer::Client,
    tasks: Vec<tokio::task::JoinHandle<capnp::Result<()>>>,
}
impl Wire {
    fn new(client: durable_transfer::Client) -> Self {
        let (a, b) = tokio::io::duplex(4096);
        let server = capntproto::rpc::serve(b, client.client);
        let (client, driver) = capntproto::rpc::client(a);
        Self {
            client,
            tasks: vec![server, driver],
        }
    }
    async fn stop(self) {
        drop(self.client);
        for t in self.tasks {
            t.abort();
            let _ = t.await;
        }
    }
}
async fn write(
    client: &durable_transfer::Client,
    sequence: u64,
    bytes: &[u8],
) -> capnp::Result<()> {
    let mut r = client.write_request();
    r.get().set_sequence(sequence);
    r.get().set_data(bytes);
    assert_eq!(r.send().promise.await?.get()?.get_sequence(), sequence);
    Ok(())
}

#[test]
fn atomic_store_batches_recover_all_members_or_none_at_every_torn_byte() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    let mut db = Store::open(&path).unwrap();
    db.put(ObjectKey::new(7), Revision::INITIAL, b"old")
        .unwrap();
    db.publish(ObjectKey::new(7), Revision::new(1), Revision::INITIAL)
        .unwrap();
    let old = std::fs::read(&path).unwrap();
    let held = db.get(ObjectKey::new(7)).unwrap();
    let values = db
        .commit(&[
            Update {
                object: ObjectKey::new(7),
                expected_head: Revision::new(1),
                expected_published: Some(Revision::new(1)),
                value: b"new",
            },
            Update {
                object: ObjectKey::new(99),
                expected_head: Revision::INITIAL,
                expected_published: Some(Revision::INITIAL),
                value: b"receipt",
            },
            Update {
                object: ObjectKey::new(42),
                expected_head: Revision::INITIAL,
                expected_published: None,
                value: b"draft",
            },
        ])
        .unwrap();
    assert_eq!(values, [2, 1, 1].map(Revision::new));
    assert_eq!(held.bytes(), b"old");
    let complete = std::fs::read(&path).unwrap();
    for cut in old.len()..complete.len() {
        let p = dir.path().join("cut");
        std::fs::write(&p, &complete[..cut]).unwrap();
        let recovered = Store::open(p).unwrap();
        assert_eq!(
            recovered.published(ObjectKey::new(7)),
            Revision::new(1),
            "cut {cut}"
        );
        assert_eq!(recovered.head(ObjectKey::new(99)), Revision::INITIAL);
        assert_eq!(recovered.head(ObjectKey::new(42)), Revision::INITIAL);
    }
    for retention in [
        Retention::History,
        Retention::Publishable,
        Retention::Latest,
    ] {
        db.compact(retention).unwrap();
        assert_eq!(db.get(ObjectKey::new(7)).unwrap().bytes(), b"new");
        assert_eq!(db.get(ObjectKey::new(99)).unwrap().bytes(), b"receipt");
        assert_eq!(db.published(ObjectKey::new(42)), Revision::INITIAL);
    }
    drop(held);
    drop(db);
    let recovered = Store::open(path).unwrap();
    assert_eq!(recovered.published(ObjectKey::new(7)), Revision::new(2));
    assert_eq!(recovered.published(ObjectKey::new(99)), Revision::new(1));
    assert_eq!(recovered.head(ObjectKey::new(42)), Revision::new(1));
    assert_eq!(recovered.published(ObjectKey::new(42)), Revision::INITIAL);
}

#[test]
fn batch_conflicts_limits_and_semantic_corruption_never_partially_update_indexes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("objects");
    let mut db = Store::open(&path).unwrap();
    db.commit(&[Update {
        object: ObjectKey::new(7),
        expected_head: Revision::INITIAL,
        expected_published: Some(Revision::INITIAL),
        value: b"one",
    }])
    .unwrap();
    let old = std::fs::read(&path).unwrap();
    for updates in [
        vec![
            Update {
                object: ObjectKey::new(8),
                expected_head: Revision::INITIAL,
                expected_published: None,
                value: b"first",
            },
            Update {
                object: ObjectKey::new(7),
                expected_head: Revision::INITIAL,
                expected_published: None,
                value: b"bad",
            },
        ],
        vec![
            Update {
                object: ObjectKey::new(8),
                expected_head: Revision::INITIAL,
                expected_published: None,
                value: b"first",
            },
            Update {
                object: ObjectKey::new(8),
                expected_head: Revision::INITIAL,
                expected_published: None,
                value: b"duplicate",
            },
        ],
        vec![Update {
            object: ObjectKey::new(7),
            expected_head: Revision::new(1),
            expected_published: Some(Revision::INITIAL),
            value: b"wrong pub",
        }],
    ] {
        assert!(db.commit(&updates).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), old);
        assert_eq!(db.head(ObjectKey::new(8)), Revision::INITIAL);
    }
    db.commit(&[
        Update {
            object: ObjectKey::new(7),
            expected_head: Revision::new(1),
            expected_published: Some(Revision::new(1)),
            value: b"two",
        },
        Update {
            object: ObjectKey::new(8),
            expected_head: Revision::INITIAL,
            expected_published: Some(Revision::INITIAL),
            value: b"receipt",
        },
    ])
    .unwrap();
    let good = std::fs::read(&path).unwrap();
    let start = old.len();
    let payload = start + 80;
    // Recompute the checksum: the parser must reject invalid batch semantics.
    for (offset, value) in [
        (payload, 0),
        (payload, 17),
        (payload + 8 + 16, 2),
        (payload + 8 + 8, 9),
        (payload + 8 + 40, 7),
        (payload + 8 + 40 + 8, 0),
        (start + 16, 1),
    ] {
        let mut bytes = good.clone();
        bytes[offset..offset + 8].copy_from_slice(&(value as u64).to_le_bytes());
        let len = u64::from_le_bytes(bytes[start + 32..start + 40].try_into().unwrap()) as usize;
        let mut hashed = bytes[start..start + 40].to_vec();
        hashed.extend_from_slice(&bytes[payload..payload + len]);
        bytes[start + 40..start + 72].copy_from_slice(&hash(&hashed));
        let header = hash(&bytes[start..start + 72]);
        bytes[start + 72..start + 80].copy_from_slice(&header[..8]);
        let p = dir.path().join("bad");
        std::fs::write(&p, bytes).unwrap();
        assert!(Store::open(&p).is_err(), "offset {offset}");
    }
    db.set_limits(capntproto::storage::Limits {
        max_file_bytes: db.file_bytes(),
        ..db.limits()
    })
    .unwrap();
    assert!(db
        .commit(&[Update {
            object: ObjectKey::new(7),
            expected_head: Revision::new(2),
            expected_published: Some(Revision::new(2)),
            value: b"quota"
        }])
        .is_err());
    assert_eq!(db.published(ObjectKey::new(7)), Revision::new(2));
    assert_eq!(std::fs::read(path).unwrap(), good);
}

#[tokio::test(flavor = "current_thread")]
async fn file_upload_resumes_after_full_server_restart_and_completion_receipt_survives_compaction()
{
    tokio::task::LocalSet::new()
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("db");
            let input = dir.path().join("input");
            let bytes = message("resumed file");
            std::fs::write(&input, &bytes).unwrap();
            for round in 0..3 {
                let db = Rc::new(RefCell::new(Store::open(&path).unwrap()));
                let state = ObjectState::new(db.clone(), ObjectId::new(7).unwrap());
                let receiver = if round == 0 {
                    Receiver::<document::Owned>::create(
                        state.clone(),
                        JournalId::new(99),
                        grant(),
                        config(&bytes),
                        hash(&bytes),
                    )
                    .unwrap()
                } else {
                    Receiver::resume(state.clone(), JournalId::new(99), grant()).unwrap()
                };
                let wire = Wire::new(receiver.capability());
                if round == 0 {
                    write(&wire.client, 1, &bytes[..bytes.len() / 2])
                        .await
                        .unwrap();
                    assert_eq!(db.borrow().published(ObjectKey::new(7)), Revision::INITIAL);
                    db.borrow_mut().compact(Retention::History).unwrap();
                } else {
                    let checkpoint = durable_bulk::upload_file(
                        wire.client.clone(),
                        &mut std::fs::File::open(&input).unwrap(),
                    )
                    .await
                    .unwrap();
                    assert_eq!(checkpoint.revision, Revision::new(1));
                    assert_eq!(checkpoint.progress.chunks, 2);
                    assert_eq!(text(&db.borrow()), Some("resumed file".into()));
                    assert_eq!(db.borrow().head(ObjectKey::new(7)), Revision::new(1));
                    // Completed receipts need only the latest journal entry.
                    db.borrow_mut().compact(Retention::Latest).unwrap();
                }
                wire.stop().await;
                drop(receiver);
                drop(state);
                drop(db);
            }
        })
        .await;
}

#[test]
fn journal_rejects_invalid_or_substituted_metadata_without_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let original = Rc::new(RefCell::new(
        Store::open(dir.path().join("original")).unwrap(),
    ));
    let bytes = message("journal binding");
    let receiver = Receiver::<document::Owned>::create(
        ObjectState::new(original.clone(), ObjectId::new(7).unwrap()),
        JournalId::new(99),
        grant(),
        config(&bytes),
        hash(&bytes),
    )
    .unwrap();
    let snapshot = original.borrow().get(ObjectKey::new(99)).unwrap();
    let header_len = u64::from_le_bytes(snapshot.bytes()[8..16].try_into().unwrap()) as usize;
    let metadata: serde_json::Value =
        serde_json::from_slice(&snapshot.bytes()[16..16 + header_len]).unwrap();
    assert_eq!(metadata["object"], 7);
    assert_eq!(metadata["generation"], 1);
    let mut cases = Vec::new();
    for (field, value, accepted) in [
        ("object", 7, true),
        ("object", 0, false),
        ("object", 8, false),
        ("generation", 0, false),
        ("generation", 2, false),
    ] {
        let mut changed = metadata.clone();
        changed[field] = serde_json::json!(value);
        cases.push((format!("{field}={value}"), changed, accepted));
    }
    for (field, value) in [
        ("length", u64::MAX),
        ("max_chunk_bytes", 0),
        ("window_bytes", 1),
        ("max_chunks", 0),
        ("max_chunks", 65537),
        ("unexpected", 1),
    ] {
        let mut changed = metadata.clone();
        changed["config"][field] = serde_json::json!(value);
        cases.push((format!("config.{field}={value}"), changed, false));
    }
    for (index, (label, changed, accepted)) in cases.into_iter().enumerate() {
        let json = serde_json::to_vec(&changed).unwrap();
        let mut journal = b"RPBULK01".to_vec();
        journal.extend_from_slice(&(json.len() as u64).to_le_bytes());
        journal.extend(json);
        let mut recovered = Store::open(dir.path().join(format!("recovered-{index}"))).unwrap();
        recovered
            .commit(&[Update {
                object: ObjectKey::new(99),
                expected_head: Revision::INITIAL,
                expected_published: Some(Revision::INITIAL),
                value: &journal,
            }])
            .unwrap();
        let recovered = Rc::new(RefCell::new(recovered));
        let before = recovered.borrow().file_bytes();
        let resumed = Receiver::<document::Owned>::resume(
            ObjectState::new(recovered.clone(), ObjectId::new(7).unwrap()),
            JournalId::new(99),
            grant(),
        );
        assert_eq!(resumed.is_ok(), accepted, "{label}");
        assert_eq!(recovered.borrow().file_bytes(), before);
        assert_eq!(
            recovered.borrow().head(ObjectKey::new(7)),
            Revision::INITIAL
        );
        assert_eq!(
            recovered.borrow().head(ObjectKey::new(99)),
            Revision::new(1)
        );
    }
    assert_eq!(receiver.checkpoint().unwrap().status, Status::Receiving);
}

#[test]
fn retries_conflicts_authority_and_cancel_preserve_durable_prefix_and_target() {
    let dir = tempfile::tempdir().unwrap();
    let db = Rc::new(RefCell::new(Store::open(dir.path().join("db")).unwrap()));
    let state = ObjectState::new(db.clone(), ObjectId::new(7).unwrap());
    let bytes = message("bulk");
    let parent = grant();
    let r = Receiver::<document::Owned>::create(
        state.clone(),
        JournalId::new(99),
        parent.clone(),
        config(&bytes),
        hash(&bytes),
    )
    .unwrap();
    assert!(Receiver::<document::Owned>::create(
        state.clone(),
        JournalId::new(99),
        parent.clone(),
        config(&bytes),
        hash(&bytes)
    )
    .is_err());
    assert!(r.done().is_err());
    r.write(1, &bytes[..bytes.len() / 2]).unwrap();
    let size = db.borrow().file_bytes();
    r.write(1, &bytes[..bytes.len() / 2]).unwrap();
    assert_eq!(db.borrow().file_bytes(), size);
    assert!(r.write(1, b"conflict").is_err());
    assert_eq!(db.borrow().file_bytes(), size);
    let again =
        Receiver::<document::Owned>::resume(state.clone(), JournalId::new(99), parent.clone())
            .unwrap();
    again.write(2, &bytes[bytes.len() / 2..]).unwrap();
    // A concurrent editor changes the pinned expected head/publication.
    let mut foreign = document::Reader::TYPE_ID.to_le_bytes().to_vec();
    foreign.extend(message("editor"));
    db.borrow_mut()
        .commit(&[Update {
            object: ObjectKey::new(7),
            expected_head: Revision::INITIAL,
            expected_published: Some(Revision::INITIAL),
            value: &foreign,
        }])
        .unwrap();
    assert!(r.done().is_err());
    assert_eq!(text(&db.borrow()), Some("editor".into()));
    assert_eq!(r.cancel().unwrap(), Status::Canceled);
    assert_eq!(r.cancel().unwrap(), Status::Canceled);
    assert!(again.write(1, &bytes[..bytes.len() / 2]).is_err());
    assert!(again.done().is_err());
    parent.revoke();
    assert!(r.checkpoint().is_err());
    assert!(r.cancel().is_err());
    assert!(
        Receiver::<document::Owned>::resume(state.clone(), JournalId::new(99), parent).is_err()
    );
    assert!(Receiver::<document::Owned>::resume(
        state.clone(),
        JournalId::new(99),
        Grant::root(
            ObjectId::new(7).unwrap(),
            ObjectGeneration::new(2).unwrap(),
            [1; 32],
            Rights::ALL
        )
    )
    .is_err());
    assert!(Receiver::<document::Owned>::resume(
        state.clone(),
        JournalId::new(99),
        Grant::root(
            ObjectId::new(7).unwrap(),
            ObjectGeneration::new(1).unwrap(),
            [1; 32],
            Rights::VIEW
        )
    )
    .is_err());
    assert!(
        Receiver::<capntproto::store_capnp::introduction_ticket::Owned>::resume(
            state,
            JournalId::new(99),
            grant()
        )
        .is_err()
    );
}

#[test]
fn invalid_values_digest_and_retired_staging_cannot_publish() {
    for invalid in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let db = Rc::new(RefCell::new(Store::open(dir.path().join("db")).unwrap()));
        let state = ObjectState::new(db.clone(), ObjectId::new(7).unwrap());
        let bytes = if invalid {
            vec![0xff; 32]
        } else {
            message("wrong digest")
        };
        let r = Receiver::<document::Owned>::create(
            state.clone(),
            JournalId::new(99),
            grant(),
            config(&bytes),
            if invalid { hash(&bytes) } else { [0; 32] },
        )
        .unwrap();
        r.write(1, &bytes[..bytes.len() / 2]).unwrap();
        r.write(2, &bytes[bytes.len() / 2..]).unwrap();
        assert!(r.done().is_err());
        assert_eq!(r.checkpoint().unwrap().status, Status::Failed);
        assert_eq!(db.borrow().head(ObjectKey::new(7)), Revision::INITIAL);
        assert!(r.done().is_err());
        assert_eq!(
            Receiver::<document::Owned>::resume(state, JournalId::new(99), grant())
                .unwrap()
                .checkpoint()
                .unwrap()
                .status,
            Status::Failed
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let db = Rc::new(RefCell::new(Store::open(dir.path().join("db")).unwrap()));
    let state = ObjectState::new(db.clone(), ObjectId::new(7).unwrap());
    let bytes = message("retained journal");
    let r = Receiver::<document::Owned>::create(
        state.clone(),
        JournalId::new(99),
        grant(),
        config(&bytes),
        hash(&bytes),
    )
    .unwrap();
    r.write(1, &bytes[..bytes.len() / 2]).unwrap();
    r.write(2, &bytes[bytes.len() / 2..]).unwrap();
    db.borrow_mut().compact(Retention::Latest).unwrap();
    assert!(r.done().is_err());
    assert!(Receiver::<document::Owned>::resume(state, JournalId::new(99), grant()).is_err());
    assert_eq!(db.borrow().published(ObjectKey::new(7)), Revision::INITIAL);
    assert_eq!(r.cancel().unwrap(), Status::Canceled);
}

#[tokio::test(flavor = "current_thread")]
async fn completion_notifies_orm_history_and_push_subscribers_over_native() {
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(Duration::from_secs(10), async {
                let dir = tempfile::tempdir().unwrap();
                let db = Rc::new(RefCell::new(Store::open(dir.path().join("db")).unwrap()));
                let state = ObjectState::new(db.clone(), ObjectId::new(7).unwrap());
                let bytes = message("Native file");
                let receiver = Receiver::<document::Owned>::create(
                    state.clone(),
                    JournalId::new(99),
                    grant(),
                    config(&bytes),
                    hash(&bytes),
                )
                .unwrap();
                let object: object::Client<document::Owned> =
                    ObjectServer::client(state, grant()).unwrap();
                let history = object
                    .history_request()
                    .send()
                    .promise
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_history()
                    .unwrap();
                let mut waiting = history.next_request().send().promise;
                assert!((&mut waiting).now_or_never().is_none());
                struct Observer(Rc<RefCell<Vec<u64>>>);
                impl capntproto::store_capnp::observer::Server<document::Owned> for Observer {
                    async fn changed(
                        self: Rc<Self>,
                        p: capntproto::store_capnp::observer::ChangedParams<document::Owned>,
                        _: capntproto::store_capnp::observer::ChangedResults<document::Owned>,
                    ) -> capnp::Result<()> {
                        self.0.borrow_mut().push(p.get()?.get_revision());
                        Ok(())
                    }
                }
                let observations = Rc::new(RefCell::new(vec![]));
                let mut subscribe = object.subscribe_request();
                subscribe
                    .get()
                    .set_observer(capnp_rpc::new_client(Observer(observations.clone())));
                let subscription = subscribe
                    .send()
                    .promise
                    .await
                    .unwrap()
                    .get()
                    .unwrap()
                    .get_subscription()
                    .unwrap();
                let a = capntproto::transport::Identity::generate();
                let b = capntproto::transport::Identity::generate();
                let left = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let right = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let address = right.local_addr().unwrap();
                let mut ac =
                    capntproto::transport::config(&a, b.public_key(), None, b"durable-bulk")
                        .unwrap();
                let mut bc =
                    capntproto::transport::config(&b, a.public_key(), None, b"durable-bulk")
                        .unwrap();
                let (left, at) = capntproto::transport::connect(left, address, &mut ac)
                    .await
                    .unwrap();
                let (right, bt) = capntproto::transport::accept(right, &mut bc).await.unwrap();
                let server = capntproto::rpc::serve(right, receiver.capability().client);
                let (client, driver) = capntproto::rpc::client::<durable_transfer::Client>(left);
                let input = dir.path().join("input");
                std::fs::write(&input, &bytes).unwrap();
                assert_eq!(
                    durable_bulk::upload_file(client, &mut std::fs::File::open(input).unwrap())
                        .await
                        .unwrap()
                        .revision,
                    Revision::new(1)
                );
                let event = waiting.await.unwrap();
                match event.get().unwrap().get_result().unwrap().which().unwrap() {
                    capntproto::store_capnp::history_result::Event(e) => {
                        assert_eq!(e.get_revision(), 1)
                    }
                    _ => panic!("completion did not publish an ORM event"),
                }
                while observations.borrow().is_empty() {
                    tokio::task::yield_now().await;
                }
                assert_eq!(&*observations.borrow(), &[1]);
                assert_eq!(text(&db.borrow()), Some("Native file".into()));
                subscription.cancel_request().send().promise.await.unwrap();
                server.abort();
                driver.abort();
                at.abort();
                bt.abort();
            })
            .await
            .unwrap();
        })
        .await;
}

#[derive(serde::Deserialize)]
struct Trace {
    extra: u8,
    valid_digest: bool,
    steps: Vec<Step>,
}
#[derive(serde::Deserialize)]
struct Step {
    action: String,
    state: Vec<u64>,
    result: u64,
}
struct Replay {
    dir: tempfile::TempDir,
    db: Option<Rc<RefCell<Store>>>,
    state: Option<Rc<ObjectState>>,
    receiver: Option<Receiver<document::Owned>>,
    grant: Grant,
    bytes: Vec<u8>,
}
impl Replay {
    fn new(valid: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = Rc::new(RefCell::new(Store::open(dir.path().join("db")).unwrap()));
        let state = ObjectState::new(db.clone(), ObjectId::new(7).unwrap());
        let grant = grant();
        let bytes = message("model bulk");
        let receiver = Receiver::<document::Owned>::create(
            state.clone(),
            JournalId::new(99),
            grant.clone(),
            config(&bytes),
            if valid { hash(&bytes) } else { [0; 32] },
        )
        .unwrap();
        Self {
            dir,
            db: Some(db),
            state: Some(state),
            receiver: Some(receiver),
            grant,
            bytes,
        }
    }
    fn restart(&mut self) {
        drop(self.receiver.take());
        drop(self.state.take());
        drop(self.db.take());
        let db = Rc::new(RefCell::new(
            Store::open(self.dir.path().join("db")).unwrap(),
        ));
        let state = ObjectState::new(db.clone(), ObjectId::new(7).unwrap());
        self.receiver =
            Some(Receiver::resume(state.clone(), JournalId::new(99), self.grant.clone()).unwrap());
        self.state = Some(state);
        self.db = Some(db);
    }
    fn checkpoint(&self) -> durable_bulk::Checkpoint {
        // A separate trusted administrator observes the durable journal even
        // after the consumer's branch has been revoked.
        Receiver::<document::Owned>::resume(
            self.state.as_ref().unwrap().clone(),
            JournalId::new(99),
            grant(),
        )
        .unwrap()
        .checkpoint()
        .unwrap()
    }
    fn edit(&self) {
        let mut value = document::Reader::TYPE_ID.to_le_bytes().to_vec();
        value.extend(message("editor"));
        let mut db = self.db.as_ref().unwrap().borrow_mut();
        let head = db.head(ObjectKey::new(7));
        let published = db.published(ObjectKey::new(7));
        db.commit(&[Update {
            object: ObjectKey::new(7),
            expected_head: head,
            expected_published: Some(published),
            value: &value,
        }])
        .unwrap();
    }
}
fn cancel_result(status: Status) -> u64 {
    match status {
        Status::Complete => 3,
        Status::Canceled => 4,
        Status::Failed => 5,
        Status::Receiving => panic!("cancel returned receiving"),
    }
}
async fn replay_operation(r: &Replay, action: &str, wire: bool) -> u64 {
    let receiver = r.receiver.as_ref().unwrap();
    let sequence = if action == "retry" || action == "conflict" {
        1
    } else {
        r.checkpoint().progress.chunks + 1
    };
    let data = if action == "conflict" {
        b"conflicting retry".as_slice()
    } else if sequence == 1 {
        &r.bytes[..r.bytes.len() / 2]
    } else {
        &r.bytes[r.bytes.len() / 2..]
    };
    if !wire {
        return match action {
            "write" | "retry" | "denied" | "conflict" => {
                if receiver.write(sequence, data).is_ok() {
                    1
                } else {
                    2
                }
            }
            "done" => match receiver.done() {
                Ok(c) => {
                    assert_eq!(c.revision, Revision::new(1));
                    3
                }
                Err(_) => 2,
            },
            "cancel" => receiver.cancel().map(cancel_result).unwrap_or(2),
            _ => panic!("unknown operation"),
        };
    }
    let wire = Wire::new(receiver.capability());
    let result = tokio::time::timeout(Duration::from_secs(2), async {
        match action {
            "write" | "retry" | "denied" | "conflict" => {
                if write(&wire.client, sequence, data).await.is_ok() {
                    1
                } else {
                    2
                }
            }
            "done" => match wire.client.done_request().send().promise.await {
                Ok(response) => {
                    let response = response.get().unwrap();
                    assert_eq!(response.get_revision(), 1);
                    assert_eq!(
                        response.get_summary().unwrap().get_bytes(),
                        r.bytes.len() as u64
                    );
                    3
                }
                Err(_) => 2,
            },
            "cancel" => match wire.client.cancel_request().send().promise.await {
                Ok(response) => cancel_result(response.get().unwrap().get_status().unwrap()),
                Err(_) => 2,
            },
            _ => panic!("unknown wire operation"),
        }
    })
    .await
    .unwrap();
    wire.stop().await;
    result
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_durable_bulk_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_DURABLE_BULK_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!traces.is_empty());
    tokio::task::LocalSet::new()
        .run_until(async {
            for (index, trace) in traces.iter().enumerate() {
                for wire in [false, true] {
                    let mut r = Replay::new(trace.valid_digest);
                    for step in &trace.steps {
                        let result = if step.action == "extra" {
                            match trace.extra {
                                1 => r.restart(),
                                2 => {
                                    r.db.as_ref()
                                        .unwrap()
                                        .borrow_mut()
                                        .compact(Retention::History)
                                        .unwrap();
                                }
                                3 => r.grant.revoke(),
                                4 => r.edit(),
                                5 => (),
                                _ => panic!("unknown extra action"),
                            }
                            if trace.extra == 5 {
                                replay_operation(&r, "conflict", wire).await
                            } else {
                                1
                            }
                        } else {
                            replay_operation(&r, &step.action, wire).await
                        };
                        assert_eq!(
                            result, step.result,
                            "trace {index} {} wire={wire}",
                            step.action
                        );
                        let checkpoint = r.checkpoint();
                        let s = &step.state;
                        assert_eq!(checkpoint.progress.chunks, s[0]);
                        assert_eq!(checkpoint.progress.bytes, s[0] * (r.bytes.len() / 2) as u64);
                        let phase = match checkpoint.status {
                            Status::Receiving => 0,
                            Status::Complete => 1,
                            Status::Canceled => 2,
                            Status::Failed => 3,
                        };
                        assert_eq!(phase, s[2]);
                        assert_eq!(checkpoint.revision, Revision::new(s[3]));
                        let db = r.db.as_ref().unwrap().borrow();
                        assert_eq!(db.head(ObjectKey::new(7)), Revision::new(s[4]));
                        assert_eq!(db.published(ObjectKey::new(7)), Revision::new(s[4]));
                        assert_eq!(db.head(ObjectKey::new(99)), Revision::new(s[6]));
                        assert_eq!(db.published(ObjectKey::new(99)), Revision::new(s[6]));
                        assert_eq!(
                            text(&db),
                            match s[5] {
                                0 => None,
                                1 => Some("model bulk".into()),
                                2 => Some("editor".into()),
                                _ => panic!(),
                            }
                        );
                        assert_eq!(u64::from(r.grant.is_live()), s[8]);
                    }
                }
            }
        })
        .await;
}
