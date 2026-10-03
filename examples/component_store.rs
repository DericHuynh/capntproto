//! Compare logical file bytes for the same typed value and durability boundary.
//! Run with --release --no-default-features --features storage.
use capnp::traits::HasTypeId;
use capntproto::{
    authority::ObjectId,
    orm::components::{ComponentState, Edit},
    storage::{ComponentId, ObjectKey, Retention, Revision, Store, Update},
    store_capnp::document,
};
use std::{cell::RefCell, rc::Rc};

fn message(text: &str) -> capnp::message::Builder<capnp::message::HeapAllocator> {
    let mut message = capnp::message::Builder::new_default();
    message.init_root::<document::Builder<'_>>().set_text(text);
    message
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let mut whole = Store::open(directory.path().join("whole"))?;
    let components = Rc::new(RefCell::new(Store::open_components(
        directory.path().join("components"),
    )?));
    let object = ObjectId::new(1).unwrap();
    let state = ComponentState::new(components.clone(), object)?;
    let hot = ComponentId::new(1);
    let cold = ComponentId::new(2);
    let payload = "x".repeat(64 * 1024);
    let mut whole_seed = 0;
    let mut component_seed = 0;
    let edits = 200;
    for i in 0..=edits {
        let value = format!("{i:08x}");
        // The whole-entry reference keeps both fields in one typed message.
        let entire = message(&format!("{value}{payload}"));
        let mut bytes = <document::Reader<'_> as HasTypeId>::TYPE_ID
            .to_le_bytes()
            .to_vec();
        bytes.extend(capnp::serialize::write_message_to_words(&entire));
        whole.commit(&[Update {
            object: ObjectKey::from(object),
            expected_head: Revision::new(i),
            expected_published: Some(Revision::new(i)),
            value: &bytes,
        }])?;
        // The component ORM serializes only the changed typed component.
        let hot_message = message(&value);
        let mut updates = vec![Edit::set::<document::Owned>(
            hot,
            hot_message.get_root_as_reader()?,
        )?];
        if i == 0 {
            let cold_message = message(&payload);
            updates.push(Edit::set::<document::Owned>(
                cold,
                cold_message.get_root_as_reader()?,
            )?);
        }
        state.edit(Revision::new(i), Some(Revision::new(i)), &updates)?;
        if i == 0 {
            whole_seed = whole.file_bytes();
            component_seed = components.borrow().file_bytes();
        }
    }
    let whole_updates = whole.file_bytes() - whole_seed;
    let component_updates = components.borrow().file_bytes() - component_seed;
    println!("200 durable typed updates: 8-byte hot text, 64 KiB unchanged cold text");
    println!("One stage-and-publish record and file sync per update on both paths.");
    println!("Seed bytes (including file header): whole={whole_seed}, components={component_seed}");
    println!("Update bytes: whole={whole_updates}, components={component_updates}");
    println!(
        "Bytes/update: whole={}, components={}",
        whole_updates / edits as usize,
        component_updates / edits as usize
    );
    println!(
        "Logical update-byte reduction: {:.2}x",
        whole_updates as f64 / component_updates as f64
    );
    let a = whole.compact(Retention::History)?;
    let b = components.borrow_mut().compact(Retention::History)?;
    println!(
        "History checkpoint bytes: whole={}, components={}",
        a.after_bytes, b.after_bytes
    );
    let value = state.get::<document::Owned>(hot)?;
    value.with_reader(|r| {
        assert_eq!(r.get_text()?.to_str()?, format!("{edits:08x}"));
        Ok(())
    })?;
    assert_eq!(
        state
            .get::<document::Owned>(cold)?
            .with_reader(|r| Ok(r.get_text()?.len()))?,
        payload.len()
    );
    // These are logical bytes, not device writes or a latency/throughput result.
    Ok(())
}
