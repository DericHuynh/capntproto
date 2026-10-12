use super::*;
use capnp::{message::FixedBuilder, word, Word};
use rpc_capnp::api::{CapDescriptor, Message, Resolve};

#[test]
fn resolution_publication_binds_the_obligation_frame_and_replacement_grant() {
    let measured = allocation_counter::measure(|| {
        for variant in 0..8 {
            let mut exports = [const { exports::Slot::EMPTY }; 2];
            let mut source = [word(0, 0, 0, 0, 0, 0, 0, 0); 40];
            let mut wire = [word(0, 0, 0, 0, 0, 0, 0, 0); 41];
            let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut wire))];
            let mut sends = [messages::SendSlot::EMPTY];
            exports::with_exports(&mut exports, |exports| {
                let mut batch = exports.batch();
                let promise = batch.export_promise(41).unwrap();
                batch.commit();
                messages::with_messages(
                    &mut buffers,
                    &mut sends[..usize::from(variant != 3)],
                    |messages| {
                        let mut batch = exports.batch();
                        if variant != 2 {
                            batch.resolve_promise(promise.key()).unwrap();
                        }
                        let replacement = batch.export(47).unwrap();
                        let mut builder = FixedBuilder::new(&mut source).unwrap();
                        let mut root = builder.try_init_root::<Message>().unwrap();
                        if variant == 7 {
                            root.field(Message::CALL).init().unwrap();
                        } else {
                            let mut resolve = root.field(Message::RESOLVE).init().unwrap();
                            resolve.scalar(Resolve::PROMISE_ID).set(if variant == 1 {
                                99
                            } else {
                                promise.id().to_wire()
                            });
                            let mut cap = resolve.field(Resolve::CAP).init().unwrap();
                            match variant {
                                5 => cap
                                    .scalar(CapDescriptor::SENDER_PROMISE)
                                    .set(replacement.id().to_wire()),
                                6 => cap.void(CapDescriptor::NONE).set(),
                                _ => cap
                                    .scalar(CapDescriptor::SENDER_HOSTED)
                                    .set(replacement.id().to_wire()),
                            }
                        }
                        let writer = messages.reserve(41 * 8).unwrap();
                        let length = builder.size_in_words() * 8 + 8;
                        capnp::serialize::write_message_segments(
                            &mut &mut messages.write(&writer)[..length],
                            &builder.get_segments_for_output(),
                        )
                        .unwrap();
                        let lease = messages
                            .seal(writer, if variant == 4 { length - 1 } else { length })
                            .unwrap();
                        let offered =
                            prepare(batch, messages, lease, &mut [0; 2], Default::default());
                        if variant == 0 {
                            let mut queue_slots = [None];
                            let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
                            offered.unwrap().accept_into(queue.reserve().unwrap());
                            let window = queue.window(messages).unwrap().unwrap();
                            let length = window.bytes().len();
                            assert!(window.advance(length).unwrap());
                            assert!(exports.batch().resolve_promise(promise.key()).is_err());
                            assert_eq!(exports.release(replacement.id(), 1), Ok(Some(47)));
                        } else {
                            assert!(offered.is_err());
                            drop(offered);
                            assert_eq!(exports.take_retired(1), Some(47));
                            let mut retry = exports.batch();
                            retry.resolve_promise(promise.key()).unwrap();
                            drop(retry);
                        }
                        assert_eq!(exports.release(promise.id(), 1), Ok(Some(41)));
                        assert!(exports.is_empty());
                        let writer = messages.reserve(41 * 8).unwrap();
                        messages.discard(writer);
                    },
                )
                .unwrap();
            })
            .unwrap();
        }
    });
    assert_eq!(measured.count_total, 0);
}
