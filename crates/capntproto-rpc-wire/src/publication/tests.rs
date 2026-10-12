use super::*;
use capnp::{message::FixedBuilder, word, Word};
use exports::{with_exports, Slot};
use messages::{with_messages, Buffer, SendSlot};
use rpc_capnp::api::{Call, CapDescriptor, Message, Payload, Return};
extern crate std;

fn frame(output: &mut [u8], ids: &[u32], call: bool, variant: u8) -> usize {
    let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 96];
    let mut builder = FixedBuilder::new(&mut words).unwrap();
    let mut root = builder.try_init_root::<Message>().unwrap();
    if call {
        let mut call = root.field(Message::CALL).init().unwrap();
        call.scalar(Call::QUESTION_ID).set(17);
        fill(call.field(Call::PARAMS).init().unwrap(), ids, variant);
    } else {
        let mut result = root.field(Message::RETURN).init().unwrap();
        result.scalar(Return::ANSWER_ID).set(17);
        result.scalar(Return::NO_FINISH_NEEDED).set(variant == 5);
        fill(result.field(Return::RESULTS).init().unwrap(), ids, variant);
    }
    let capacity = output.len();
    let mut cursor = output;
    capnp::serialize::write_message_segments(&mut cursor, &builder.get_segments_for_output())
        .unwrap();
    capacity - cursor.len()
}

fn fill(mut payload: capnp::field_api::FixedStruct<'_, Payload>, ids: &[u32], variant: u8) {
    let mut table = payload.field(Payload::CAP_TABLE).init(ids.len()).unwrap();
    for (index, id) in ids.iter().enumerate() {
        let mut descriptor = table.get_mut(index).unwrap();
        match variant {
            1 => descriptor.scalar(CapDescriptor::SENDER_PROMISE).set(*id),
            2 => descriptor.scalar(CapDescriptor::RECEIVER_HOSTED).set(*id),
            3 => descriptor.void(CapDescriptor::NONE).set(),
            _ => descriptor.scalar(CapDescriptor::SENDER_HOSTED).set(*id),
        }
        if variant == 4 {
            descriptor.scalar(CapDescriptor::ATTACHED_FD).set(0);
        }
    }
}

#[test]
fn call_and_return_publish_exact_new_and_repeated_grants_at_acceptance() {
    for call in [false, true] {
        let mut slots = [const { Slot::EMPTY }; 2];
        let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 96];
        let mut buffers = [Buffer::new(Word::words_to_bytes_mut(&mut words))];
        let mut sends = [SendSlot::EMPTY];
        with_exports(&mut slots, |exports| {
            with_messages(&mut buffers, &mut sends, |messages| {
                let mut batch = exports.batch();
                let first = batch.export(11).unwrap();
                let second = batch.export(13).unwrap();
                let _ = batch.grant(first.key()).unwrap();
                let writer = messages.reserve(1).unwrap();
                let length = frame(
                    &mut messages.write(&writer),
                    &[
                        first.id().to_wire(),
                        second.id().to_wire(),
                        first.id().to_wire(),
                    ],
                    call,
                    4,
                );
                let lease = messages.seal(writer, length).unwrap();
                let mut scratch = [u32::MAX; 3];
                let prepared =
                    prepare(batch, messages, lease, &mut scratch, Default::default()).unwrap();
                assert_eq!(prepared.bytes().len(), length);
                assert_eq!(scratch, [2, 1, u32::MAX]);
                let send = prepared.accept();
                let live = exports.lease(first.id()).unwrap();
                assert_eq!(exports.resolve(first.key()), Some(&11));
                // Transmission completion never releases remote grants.
                assert_eq!(
                    messages.send_window(send).unwrap().advance(length),
                    Ok(true)
                );
                assert_eq!(
                    exports.release(first.id(), 3),
                    Err(exports::Error::OverRelease)
                );
                assert_eq!(exports.release(first.id(), 2), Ok(None));
                assert_eq!(exports.get(&live), &11);
                assert_eq!(exports.release_local(live), Some(11));
                assert_eq!(exports.release(second.id(), 1), Ok(Some(13)));
                let writer = messages.reserve(1).unwrap();
                messages.discard(writer);
            })
            .unwrap();
        })
        .unwrap();
    }
}

#[test]
fn mismatches_abort_new_grants_and_preserve_published_counts() {
    for ids in [
        &[0][..],
        &[1, 0][..],
        &[0, 0, 0][..],
        &[0, u32::MAX][..],
        &[][..],
    ] {
        let mut slots = [const { Slot::EMPTY }; 2];
        let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 96];
        let mut buffers = [Buffer::new(Word::words_to_bytes_mut(&mut words))];
        let mut sends = [SendSlot::EMPTY];
        with_exports(&mut slots, |exports| {
            let mut initial = exports.batch();
            let existing = initial.export(7).unwrap();
            initial.commit();
            with_messages(&mut buffers, &mut sends, |messages| {
                let mut batch = exports.batch();
                let _ = batch.grant(existing.key()).unwrap();
                let _ = batch.grant(existing.key()).unwrap();
                let new = batch.export(9).unwrap();
                let writer = messages.reserve(1).unwrap();
                let length = frame(&mut messages.write(&writer), ids, false, 0);
                let lease = messages.seal(writer, length).unwrap();
                assert!(matches!(
                    prepare(batch, messages, lease, &mut [0; 2], Default::default()),
                    Err(Error::Descriptors(_))
                ));
                assert_eq!(exports.resolve(new.key()), None);
                assert_eq!(exports.take_retired(1), Some(9));
                assert_eq!(
                    exports.release(existing.id(), 2),
                    Err(exports::Error::OverRelease)
                );
                assert_eq!(exports.release(existing.id(), 1), Ok(Some(7)));
                let writer = messages.reserve(1).unwrap();
                messages.discard(writer);
            })
            .unwrap();
        })
        .unwrap();
    }
}

#[test]
fn unsupported_descriptor_semantics_and_invalid_return_fail_closed() {
    for variant in [1, 2, 3, 5] {
        let mut slots = [const { Slot::EMPTY }; 1];
        let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 96];
        let mut buffers = [Buffer::new(Word::words_to_bytes_mut(&mut words))];
        let mut sends = [SendSlot::EMPTY];
        with_exports(&mut slots, |exports| {
            with_messages(&mut buffers, &mut sends, |messages| {
                let mut batch = exports.batch();
                let grant = batch.export(7).unwrap();
                let writer = messages.reserve(1).unwrap();
                let length = frame(&mut messages.write(&writer), &[grant.id().to_wire()], false, variant);
                let lease = messages.seal(writer, length).unwrap();
                let expected = match variant {
                    1 => Error::Descriptors(exports::DescriptorError::PromiseKind),
                    2 => Error::UnsupportedDescriptor,
                    3 => Error::Descriptors(exports::DescriptorError::Missing),
                    5 => Error::InvalidReturn,
                    _ => unreachable!(),
                };
                assert!(matches!(prepare(batch, messages, lease, &mut [0], Default::default()), Err(e) if e == expected));
                assert_eq!(exports.lookup(grant.id()), None);
                assert_eq!(exports.take_retired(0), Some(7));
            }).unwrap();
        }).unwrap();
    }
}

#[test]
fn every_truncated_frame_and_trailing_data_abort_publication() {
    let mut source = [word(0, 0, 0, 0, 0, 0, 0, 0); 96];
    let source = Word::words_to_bytes_mut(&mut source);
    let length = frame(source, &[0], false, 0);
    for keep in (0..length).chain(core::iter::once(length + 1)) {
        let mut slots = [const { Slot::EMPTY }; 1];
        let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 96];
        let mut buffers = [Buffer::new(Word::words_to_bytes_mut(&mut words))];
        let mut sends = [SendSlot::EMPTY];
        with_exports(&mut slots, |exports| {
            with_messages(&mut buffers, &mut sends, |messages| {
                let mut batch = exports.batch();
                let _ = batch.export(7).unwrap();
                let writer = messages.reserve(keep).unwrap();
                messages.write(&writer)[..keep].copy_from_slice(&source[..keep]);
                let lease = messages.seal(writer, keep).unwrap();
                assert!(prepare(batch, messages, lease, &mut [0], Default::default()).is_err());
                assert_eq!(exports.take_retired(0), Some(7));
                let writer = messages.reserve(keep).unwrap();
                messages.discard(writer);
            })
            .unwrap();
        })
        .unwrap();
    }
}

#[test]
fn rejected_output_scratch_and_send_capacity_release_only_the_transaction() {
    for failure in 0..4 {
        let mut slots = [const { Slot::EMPTY }; 1];
        let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 96];
        let mut buffers = [Buffer::new(Word::words_to_bytes_mut(&mut words))];
        let mut sends = [SendSlot::EMPTY];
        let sends = if failure == 0 {
            &mut sends[..0]
        } else {
            &mut sends[..]
        };
        with_exports(&mut slots, |exports| {
            with_messages(&mut buffers, sends, |messages| {
                let mut batch = exports.batch();
                let grant = batch.export(7).unwrap();
                let writer = messages.reserve(1).unwrap();
                let length = frame(&mut messages.write(&writer), &[0], false, 0);
                let lease = messages.seal(writer, length).unwrap();
                let retained = messages.retain(&lease).unwrap();
                let mut scratch = [0];
                let scratch = if failure == 1 {
                    &mut scratch[..0]
                } else {
                    &mut scratch[..]
                };
                let options = capnp::message::ReaderOptions {
                    traversal_limit_in_words: if failure == 3 { Some(0) } else { Some(128) },
                    ..Default::default()
                };
                match prepare(batch, messages, lease, scratch, options) {
                    Ok(prepared) => {
                        assert_eq!(failure, 2);
                        drop(prepared);
                    }
                    Err(Error::Storage(messages::Error::NoSendSlot)) => assert_eq!(failure, 0),
                    Err(Error::Descriptors(exports::DescriptorError::ScratchTooSmall)) => {
                        assert_eq!(failure, 1)
                    }
                    Err(Error::Wire(_)) => assert_eq!(failure, 3),
                    _ => panic!("unexpected rejection"),
                }
                assert_eq!(exports.lookup(grant.id()), None);
                assert_eq!(exports.take_retired(0), Some(7));
                assert_eq!(messages.read(&retained).len(), length);
                assert_eq!(messages.reserve(1).unwrap_err(), messages::Error::NoBuffer);
                messages.release(retained);
                let writer = messages.reserve(1).unwrap();
                messages.discard(writer);
            })
            .unwrap();
        })
        .unwrap();
    }
}

#[test]
fn publication_reuse_and_rejection_do_not_allocate() {
    let info = allocation_counter::measure(|| {
        let mut slots = [const { Slot::EMPTY }; 1];
        let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 96];
        let mut buffers = [Buffer::new(Word::words_to_bytes_mut(&mut words))];
        let mut sends = [SendSlot::EMPTY];
        with_exports(&mut slots, |exports| {
            with_messages(&mut buffers, &mut sends, |messages| {
                for accept in [false, true, true, false] {
                    let mut batch = exports.batch();
                    let grant = batch.export(7).unwrap();
                    let writer = messages.reserve(1).unwrap();
                    let length = frame(&mut messages.write(&writer), &[0], false, 0);
                    let lease = messages.seal(writer, length).unwrap();
                    let prepared =
                        prepare(batch, messages, lease, &mut [0], Default::default()).unwrap();
                    if accept {
                        let send = prepared.accept();
                        assert_eq!(
                            messages.send_window(send).unwrap().advance(length),
                            Ok(true)
                        );
                        assert_eq!(exports.release(grant.id(), 1), Ok(Some(7)));
                    } else {
                        drop(prepared);
                        assert_eq!(exports.take_retired(0), Some(7));
                    }
                }
            })
            .unwrap();
        })
        .unwrap();
    });
    assert_eq!(info.count_total, 0);
}

#[test]
fn unwind_aborts_and_forgotten_publications_cannot_become_visible() {
    for forget in [false, true] {
        let mut slots = [const { Slot::EMPTY }; 1];
        let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 96];
        let mut buffers = [Buffer::new(Word::words_to_bytes_mut(&mut words))];
        let mut sends = [SendSlot::EMPTY];
        with_exports(&mut slots, |exports| {
            with_messages(&mut buffers, &mut sends, |messages| {
                let mut batch = exports.batch();
                let grant = batch.export(7).unwrap();
                let writer = messages.reserve(1).unwrap();
                let length = frame(&mut messages.write(&writer), &[0], false, 0);
                let lease = messages.seal(writer, length).unwrap();
                let prepared =
                    prepare(batch, messages, lease, &mut [0], Default::default()).unwrap();
                if forget {
                    core::mem::forget(prepared);
                    assert_eq!(messages.reserve(1).unwrap_err(), messages::Error::NoBuffer);
                } else {
                    assert!(
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                            let _prepared = prepared;
                            panic!("driver failed before queue acceptance");
                        }))
                        .is_err()
                    );
                    let writer = messages.reserve(1).unwrap();
                    messages.discard(writer);
                }
                assert_eq!(exports.lookup(grant.id()), None);
                assert_eq!(exports.take_retired(0), Some(7));
            })
            .unwrap();
        })
        .unwrap();
        // Terminal teardown recovers forgotten offers but performs no IO.
        with_messages(&mut buffers, &mut sends, |messages| {
            let writer = messages.reserve(1).unwrap();
            messages.discard(writer);
        })
        .unwrap();
    }
}

#[test]
fn existing_grants_remain_published_after_send_failure_and_new_batch_rejection() {
    let mut slots = [const { Slot::EMPTY }; 1];
    let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 96];
    let mut buffers = [Buffer::new(Word::words_to_bytes_mut(&mut words))];
    let mut sends = [SendSlot::EMPTY];
    with_exports(&mut slots, |exports| {
        let mut initial = exports.batch();
        let grant = initial.export(7).unwrap();
        initial.commit();
        with_messages(&mut buffers, &mut sends, |messages| {
            for accept in [false, true] {
                let mut batch = exports.batch();
                let _ = batch.grant(grant.key()).unwrap();
                let writer = messages.reserve(1).unwrap();
                let length = frame(&mut messages.write(&writer), &[0], false, 0);
                let lease = messages.seal(writer, length).unwrap();
                let prepared =
                    prepare(batch, messages, lease, &mut [0], Default::default()).unwrap();
                if accept {
                    let send = prepared.accept();
                    assert_eq!(messages.send_window(send).unwrap().advance(1), Ok(false));
                    messages.fail_send(send).unwrap();
                    // Failing IO cannot roll back a possibly observed grant.
                    assert_eq!(exports.release(grant.id(), 1), Ok(None));
                } else {
                    drop(prepared);
                }
                assert_eq!(exports.resolve(grant.key()), Some(&7));
                assert_eq!(
                    exports.release(grant.id(), 2),
                    Err(exports::Error::OverRelease)
                );
            }
            assert_eq!(exports.release(grant.id(), 1), Ok(Some(7)));
        })
        .unwrap();
    })
    .unwrap();
}

#[test]
fn tombstones_without_grants_are_allowed_and_bad_roots_are_rejected() {
    for mode in 0..3 {
        let mut slots: [Slot<()>; 0] = [];
        let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 96];
        let mut buffers = [Buffer::new(Word::words_to_bytes_mut(&mut words))];
        let mut sends = [SendSlot::EMPTY];
        with_exports(&mut slots, |exports| {
            with_messages(&mut buffers, &mut sends, |messages| {
                let batch = exports.batch();
                let writer = messages.reserve(1).unwrap();
                let length = frame(&mut messages.write(&writer), &[0, 0], false, 3);
                if mode == 1 {
                    // An invalid far pointer at the root of the single-segment frame.
                    messages.write(&writer)[8..16]
                        .copy_from_slice(&[2, 0, 0, 0, 255, 255, 255, 255]);
                } else if mode == 2 {
                    // Null root reads as default Message.unimplemented, outside this path.
                    messages.write(&writer)[8..16].fill(0);
                }
                let lease = messages.seal(writer, length).unwrap();
                let send = match prepare(batch, messages, lease, &mut [], Default::default()) {
                    Ok(prepared) => {
                        assert_eq!(mode, 0);
                        Some(prepared.accept())
                    }
                    Err(Error::Wire(_)) => {
                        assert_eq!(mode, 1);
                        None
                    }
                    Err(Error::UnsupportedMessage) => {
                        assert_eq!(mode, 2);
                        None
                    }
                    _ => panic!("unexpected validation result"),
                };
                if let Some(send) = send {
                    messages.fail_send(send).unwrap();
                }
                let writer = messages.reserve(1).unwrap();
                messages.discard(writer);
            })
            .unwrap();
        })
        .unwrap();
    }
}

#[test]
fn attachment_bundle_commits_with_call_or_return_and_aborts_with_unpublished_grants() {
    use core::cell::Cell;
    struct Resource<'a>(&'a Cell<usize>);
    impl Drop for Resource<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    for call in [false, true] {
        for accept in [false, true] {
            let usage = allocation_counter::measure(|| {
                let drops = Cell::new(0);
                let mut slots = [const { Slot::EMPTY }; 1];
                let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 96];
                let mut buffers = [Buffer::new(Word::words_to_bytes_mut(&mut words))];
                with_exports(&mut slots, |exports| {
                    with_messages(&mut buffers, &mut [SendSlot::EMPTY], |messages| {
                        let mut entries = [None];
                        let mut queue = messages::Queue::with_resources(&mut entries).unwrap();
                        let mut attachments = crate::attachments::Sender::<_, 1>::default();
                        assert_eq!(attachments.push(Some(Resource(&drops))), 0);
                        let reserved = queue.reserve_with(attachments).unwrap();
                        let mut batch = exports.batch();
                        let grant = batch.export(17).unwrap();
                        let writer = messages.reserve(1).unwrap();
                        let length = frame(
                            &mut messages.write(&writer),
                            &[grant.id().to_wire()],
                            call,
                            4,
                        );
                        let lease = messages.seal(writer, length).unwrap();
                        let prepared =
                            prepare(batch, messages, lease, &mut [0], Default::default()).unwrap();
                        if accept {
                            prepared.accept_into(reserved);
                            let window = queue.window(messages).unwrap().unwrap();
                            assert_eq!(window.resources().unwrap().len(), 1);
                            window.advance(1).unwrap();
                            assert_eq!(drops.get(), 0);
                            let window = queue.window(messages).unwrap().unwrap();
                            assert!(window.resources().is_none());
                            window.advance(length - 1).unwrap();
                            assert_eq!(exports.release(grant.id(), 1), Ok(Some(17)));
                        } else {
                            drop(prepared);
                            drop(reserved);
                            assert!(queue.is_empty());
                            assert_eq!(exports.lookup(grant.id()), None);
                            assert_eq!(exports.take_retired(0), Some(17));
                        }
                        assert_eq!(drops.get(), 1);
                        let writer = messages.reserve(1).unwrap();
                        messages.discard(writer);
                    })
                    .unwrap();
                })
                .unwrap();
            });
            assert_eq!(usage.count_total, 0);
        }
    }
}
