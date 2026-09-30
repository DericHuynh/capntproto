//! Feasibility probe, not a production ORM backend. The builder owns scratch
//! memory; only validated byte differences enter an EAE transaction.
use capnp::field_api::{Message, MessageView};
use capnp::message::ReaderOptions;
use eae_pages::durable::{Arena, Durability, Limits, Outcome};
use std::io;

pub mod entry_capnp {
    include!(concat!(env!("OUT_DIR"), "/entry_capnp.rs"));
}
use capnp::traits::HasTypeId;
use entry_capnp::api::Value;
const SCHEMA: u64 = <entry_capnp::value::Reader<'static> as HasTypeId>::TYPE_ID;
const CAPACITY: usize = 65536;
const META: usize = 32;

fn invalid(e: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}
fn word(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
fn payload(bytes: &[u8]) -> io::Result<&[u8]> {
    let len = usize::try_from(word(bytes, 24)).map_err(invalid)?;
    if word(bytes, 0) != SCHEMA || len > CAPACITY - META {
        return Err(invalid("schema or capacity mismatch"));
    }
    Ok(&bytes[META..META + len])
}
fn validate(segments: &[&[u8]], budget: usize) -> io::Result<()> {
    let bytes = segments[0];
    if word(bytes, 16) != word(bytes, 8) {
        return Err(invalid("probe requires atomic latest publication"));
    }
    let mut data = payload(bytes)?;
    let mut options = ReaderOptions::new();
    options.traversal_limit_in_words(Some(budget));
    options.nesting_limit(16);
    let reader =
        capnp::serialize::read_message_from_flat_slice(&mut data, options).map_err(invalid)?;
    if !data.is_empty() {
        return Err(invalid("trailing serialized bytes"));
    }
    let root = reader
        .get_root::<capnp::any_pointer::Reader<'_>>()
        .map_err(invalid)?;
    if root.target_size().map_err(invalid)?.cap_count != 0 {
        return Err(invalid("live capabilities cannot be persisted"));
    }
    let value = reader
        .get_root::<entry_capnp::value::Reader<'_>>()
        .map_err(invalid)?;
    value.get_payload().map_err(invalid)?;
    Ok(())
}
fn image(value: &[u8], revision: u64) -> io::Result<Vec<u8>> {
    if value.len() > CAPACITY - META {
        return Err(invalid("slot exhausted"));
    }
    let mut bytes = vec![0; CAPACITY];
    for (i, n) in [SCHEMA, revision, revision, value.len() as u64]
        .into_iter()
        .enumerate()
    {
        bytes[i * 8..i * 8 + 8].copy_from_slice(&n.to_le_bytes());
    }
    bytes[META..META + value.len()].copy_from_slice(value);
    Ok(bytes)
}
fn publish(arena: &mut Arena, expected: u64, value: &[u8], budget: usize) -> io::Result<u64> {
    let before = arena.segments()?[0].to_vec();
    if word(&before, 8) != expected {
        return Err(invalid("object CAS conflict"));
    }
    let next = image(
        value,
        expected
            .checked_add(1)
            .ok_or_else(|| invalid("revision exhausted"))?,
    )?;
    let generation = arena.generation();
    let mut tx = arena.begin_at(generation)?;
    // Difference detection is outside the builder. All changed words, pointer
    // words and cleared tail bytes participate in the same undo/redo commit.
    for offset in (0..CAPACITY).step_by(8) {
        if before[offset..offset + 8] != next[offset..offset + 8] {
            tx.write(0, offset, &next[offset..offset + 8])?;
        }
    }
    tx.commit_checked(Durability::Durable, |s| validate(s, budget))
        .map_err(invalid)?;
    Ok(expected + 1)
}
fn document(counter: u64, len: usize) -> Vec<u8> {
    let mut value = Message::<Value>::new().unwrap();
    value.edit().counter().set(counter);
    value.edit().payload().copy_from(&vec![7; len]).unwrap();
    value.to_vec()
}
fn counter(bytes: &[u8]) -> u64 {
    MessageView::<Value>::from_unpacked(payload(bytes).unwrap(), Default::default())
        .unwrap()
        .read()
        .counter()
}

#[test]
fn generated_fields_delta_commit_cas_snapshot_checkpoint_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("arena");
    let mut arena = Arena::create(&path, &[CAPACITY], SCHEMA, Limits::default()).unwrap();
    publish(&mut arena, 0, &document(1, 32000), 65536).unwrap();
    let held = arena.snapshot().unwrap();
    let before = arena.log_bytes();
    publish(&mut arena, 1, &document(2, 32000), 65536).unwrap();
    assert!(
        arena.log_bytes() - before < 512,
        "small edit must yield a small redo frame"
    );
    assert!(publish(&mut arena, 1, &document(3, 32000), 65536).is_err());
    assert_eq!(counter(arena.segments().unwrap()[0]), 2);
    arena.checkpoint().unwrap();
    drop(arena);
    let arena = Arena::open(&path, SCHEMA, Limits::default()).unwrap();
    validate(&arena.segments().unwrap(), 65536).unwrap();
    assert_eq!(counter(arena.segments().unwrap()[0]), 2);
    assert_eq!(counter(held.segments()[0]), 1);
    assert!(Arena::open(&path, SCHEMA, Limits::default()).is_err());
    drop(arena);
    assert!(Arena::open(&path, SCHEMA ^ 1, Limits::default()).is_err());
}

#[test]
fn validation_rejects_capabilities_malformed_graphs_limits_and_trailing_bytes_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let mut arena = Arena::create(
        dir.path().join("arena"),
        &[CAPACITY],
        SCHEMA,
        Limits::default(),
    )
    .unwrap();
    let good = document(1, 1024);
    publish(&mut arena, 0, &good, 65536).unwrap();
    let before = arena.segments().unwrap()[0].to_vec();
    let log = arena.log_bytes();
    let mut trailing = good.clone();
    trailing.extend_from_slice(&[0; 8]);
    // One pointer slot containing a live capability pointer; even without a
    // hook table the wire pointer must never become persistent authority.
    let cap = [
        0u32.to_le_bytes().as_slice(),
        1u32.to_le_bytes().as_slice(),
        3u64.to_le_bytes().as_slice(),
    ]
    .concat();
    let nested_cap = [
        0u32.to_le_bytes().as_slice(),
        2u32.to_le_bytes().as_slice(),
        (1u64 << 48).to_le_bytes().as_slice(),
        3u64.to_le_bytes().as_slice(),
    ]
    .concat();
    for (value, budget) in [
        (vec![0xff; 16], 65536),
        (trailing, 65536),
        (cap, 65536),
        (nested_cap, 65536),
        (good, 0),
        (vec![0; CAPACITY], 65536),
    ] {
        assert!(publish(&mut arena, 1, &value, budget).is_err());
        assert_eq!(arena.segments().unwrap()[0], before);
        assert_eq!(arena.log_bytes(), log);
        assert_eq!(arena.generation(), 1);
    }
    let mut tx = arena.begin().unwrap();
    tx.put_word(0, 0, SCHEMA ^ 1).unwrap();
    let error = tx
        .commit_checked(Durability::Durable, |s| validate(s, 65536))
        .unwrap_err();
    assert_eq!(error.outcome, Outcome::Rejected);
    assert_eq!(arena.segments().unwrap()[0], before);
}

#[test]
fn abandoned_edits_undo_metadata_payload_and_pointer_changes() {
    let dir = tempfile::tempdir().unwrap();
    let mut arena = Arena::create(
        dir.path().join("arena"),
        &[CAPACITY],
        SCHEMA,
        Limits::default(),
    )
    .unwrap();
    publish(&mut arena, 0, &document(1, 1024), 65536).unwrap();
    let before = arena.segments().unwrap()[0].to_vec();
    {
        let mut tx = arena.begin().unwrap();
        tx.write(0, 0, &vec![0xff; 4096]).unwrap();
    }
    assert_eq!(arena.segments().unwrap()[0], before);
    publish(&mut arena, 1, &document(2, 8), 65536).unwrap();
    let bytes = arena.segments().unwrap();
    assert!(bytes[0][META + word(bytes[0], 24) as usize..]
        .iter()
        .all(|b| *b == 0));
}
