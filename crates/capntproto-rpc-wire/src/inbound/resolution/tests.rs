use super::*;
use capnp::{
    message::{FixedBuilder, Reader},
    word,
};
use rpc_capnp::api::{CapDescriptor, Resolve};

#[test]
fn rejected_resolution_never_consumes_original_or_replacement_ownership() {
    let measured = allocation_counter::measure(|| {
        for variant in 0..7 {
            let mut slots = [const { imports::Slot::EMPTY }; 2];
            let capacity = if variant == 3 { 1 } else { 2 };
            imports::with_imports(&mut slots[..capacity], |imports| {
                let id = ImportId::from_wire(17);
                let mut initial = [imports::ReceiveSlot::new_promise(id, 41)];
                imports.prepare_receive(&mut initial).unwrap().commit();
                let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 24];
                let mut builder = FixedBuilder::new(&mut words).unwrap();
                let mut root = builder.try_init_root::<Resolve>().unwrap();
                root.scalar(Resolve::PROMISE_ID).set(17);
                let mut cap = root.field(Resolve::CAP).init().unwrap();
                if variant == 2 {
                    cap.void(CapDescriptor::NONE).set();
                } else if variant == 4 {
                    cap.scalar(CapDescriptor::SENDER_PROMISE).set(23);
                } else {
                    cap.scalar(CapDescriptor::SENDER_HOSTED).set(23);
                }
                if variant == 5 {
                    cap.scalar(CapDescriptor::ATTACHED_FD).set(0);
                }
                let reader = Reader::new(builder.get_segments_for_output(), Default::default());
                let mut replacement = [imports::ReceiveSlot::new(
                    ImportId::from_wire(if variant == 1 { 29 } else { 23 }),
                    47,
                )];
                let offered = resolve(reader.get_root().unwrap(), imports, &mut replacement);
                if matches!(variant, 0 | 5) {
                    let accepted = offered.unwrap().accept();
                    assert_eq!(accepted.target(), Some(initial[0].local().unwrap().key()));
                    accepted.release(imports);
                    assert!(imports.prepare_resolution(id, &mut []).is_err());
                } else {
                    if variant == 6 {
                        drop(offered.unwrap());
                    } else {
                        assert!(offered.is_err());
                        drop(offered);
                    }
                    assert_eq!(replacement[0].owner(), Some(&47));
                    assert_eq!(imports.len(), 1);
                    imports.prepare_resolution(id, &mut []).unwrap().commit();
                }
                imports.release(initial[0].take_local().unwrap());
                let cleanup = imports.pending_release(0).unwrap();
                assert_eq!(cleanup.count(), 1);
                assert_eq!(cleanup.commit(), 41);
                if matches!(variant, 0 | 5) {
                    assert_eq!(imports.pending_release(1).unwrap().commit(), 47);
                }
                assert!(imports.is_empty());
            })
            .unwrap();
        }
    });
    assert_eq!(measured.count_total, 0);
}
