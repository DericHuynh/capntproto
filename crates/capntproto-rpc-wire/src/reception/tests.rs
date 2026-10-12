use super::*;
use capnp::{message::FixedBuilder, word};
use rpc_capnp::api::{CapDescriptor, Payload};

fn payload(builder: &mut FixedBuilder<'_>, id: u32, index: u32, variant: u8) {
    let mut root = builder.try_init_root::<Payload>().unwrap();
    root.field(Payload::CONTENT)
        .set_capability_index(index)
        .unwrap();
    let mut table = root.field(Payload::CAP_TABLE).init(1).unwrap();
    let mut descriptor = table.get_mut(0).unwrap();
    match variant {
        1 => descriptor.scalar(CapDescriptor::SENDER_PROMISE).set(id),
        2 => descriptor.void(CapDescriptor::NONE).set(),
        _ => descriptor.scalar(CapDescriptor::SENDER_HOSTED).set(id),
    }
    if variant == 3 {
        descriptor.scalar(CapDescriptor::ATTACHED_FD).set(0);
    }
}

#[test]
fn typed_capability_resolution_rejects_same_index_from_another_message() {
    let measured = allocation_counter::measure(|| {
        let mut slots = [imports::Slot::EMPTY];
        let mut first_words = [word(0, 0, 0, 0, 0, 0, 0, 0); 24];
        let mut second_words = [word(0, 0, 0, 0, 0, 0, 0, 0); 24];
        let mut first = FixedBuilder::new(&mut first_words).unwrap();
        let mut second = FixedBuilder::new(&mut second_words).unwrap();
        payload(&mut first, 7, 0, 3);
        payload(&mut second, 9, 0, 0);
        let first =
            capnp::message::Reader::new(first.get_segments_for_output(), Default::default());
        let second =
            capnp::message::Reader::new(second.get_segments_for_output(), Default::default());
        let payload = first.get_root::<rpc_capnp::payload::Reader<'_>>().unwrap();
        let foreign = second
            .get_root::<rpc_capnp::payload::Reader<'_>>()
            .unwrap()
            .get_content()
            .get_as::<capnp::wire_capability::Reader<'_, ()>>()
            .unwrap();
        imports::with_imports(&mut slots, |imports| {
            let mut receipts = [imports::ReceiveSlot::new(ImportId::from_wire(7), 42)];
            let received = prepare(payload, imports, &mut receipts).unwrap().accept();
            let own = received
                .content()
                .get_as::<capnp::wire_capability::Reader<'_, ()>>()
                .unwrap();
            assert!(matches!(
                received.capability(foreign),
                Err(Error::ForeignMessage)
            ));
            let cap = received.capability(own).unwrap();
            assert_eq!(imports.get(cap), &42);
            let retained = imports.retain(cap).unwrap();
            assert_eq!(
                received
                    .pipeline(crate::pipeline::Path::new(&[]))
                    .unwrap()
                    .key(),
                retained.key()
            );
            received.release(imports);
            assert!(imports.pending_release(0).is_none());
            imports.release(retained);
            let cleanup = imports.pending_release(0).unwrap();
            assert_eq!(cleanup.count(), 1);
            assert_eq!(cleanup.commit(), 42);
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn malformed_or_mismatched_descriptor_tables_never_import_a_prefix() {
    for variant in [0, 1, 2, 4] {
        let mut slots = [imports::Slot::EMPTY];
        let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 24];
        let mut builder = FixedBuilder::new(&mut words).unwrap();
        payload(&mut builder, 7, u32::from(variant == 4), variant);
        let reader =
            capnp::message::Reader::new(builder.get_segments_for_output(), Default::default());
        imports::with_imports(&mut slots, |imports| {
            let id = ImportId::from_wire(if variant == 0 { 9 } else { 7 });
            let mut receipts = [imports::ReceiveSlot::new(id, 42)];
            assert!(prepare(reader.get_root().unwrap(), imports, &mut receipts).is_err());
            assert!(imports.is_empty());
            assert_eq!(receipts[0].owner(), Some(&42));
            assert!(receipts[0].local().is_none());
        })
        .unwrap();
    }
}

#[test]
fn tombstone_is_not_callable_and_dropped_preflight_keeps_input_owners() {
    let mut slots = [imports::Slot::EMPTY];
    let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 24];
    let mut builder = FixedBuilder::new(&mut words).unwrap();
    payload(&mut builder, 7, 0, 0);
    imports::with_imports(&mut slots, |imports| {
        let reader =
            capnp::message::Reader::new(builder.get_segments_for_output(), Default::default());
        let mut receipts = [imports::ReceiveSlot::new(ImportId::from_wire(7), 42)];
        drop(prepare(reader.get_root().unwrap(), imports, &mut receipts).unwrap());
        assert!(imports.is_empty());
        assert_eq!(receipts[0].owner(), Some(&42));
        assert!(matches!(
            prepare(reader.get_root().unwrap(), imports, &mut []),
            Err(Error::DescriptorCount)
        ));
        builder.reset();
        payload(&mut builder, 7, 0, 2);
        let reader =
            capnp::message::Reader::new(builder.get_segments_for_output(), Default::default());
        let mut receipts = [imports::ReceiveSlot::none()];
        let received = prepare(reader.get_root().unwrap(), imports, &mut receipts)
            .unwrap()
            .accept();
        let pointer = received
            .content()
            .get_as::<capnp::wire_capability::Reader<'_, ()>>()
            .unwrap();
        assert!(matches!(
            received.capability(pointer),
            Err(Error::MissingCapability)
        ));
        received.release(imports);
        assert!(imports.is_empty());
    })
    .unwrap();
}
