//! Bind a pending loopback barrier to its exact FIFO message and old target.
use super::*;
use capntproto_rpc_engine::{embargoes, ids::EmbargoId, peer_table::SparseStorage};
#[must_use]
pub struct Embargo<
    'a,
    'pool,
    'storage,
    'bytes,
    T,
    S: SparseStorage<EmbargoId, embargoes::Record<T>>,
> {
    pending: embargoes::Offer<'a, T, S>,
    frame: Frame<'a, 'pool, 'storage, 'bytes>,
}
pub fn embargo<
    'a,
    'pool,
    'storage,
    'bytes,
    T,
    S: SparseStorage<EmbargoId, embargoes::Record<T>>,
>(
    pending: embargoes::Offer<'a, T, S>,
    target: crate::calls::Target<'_>,
    messages: &'a mut messages::Messages<'pool, 'storage, 'bytes>,
    lease: messages::Lease<'pool>,
    options: capnp::message::ReaderOptions,
) -> Result<Embargo<'a, 'pool, 'storage, 'bytes, T, S>, Error> {
    let mut frame = Frame::new(messages, lease);
    frame.validate(options, |root| {
        let rpc_capnp::message::Disembargo(barrier) = root.which().map_err(|_| Error::Unsupported)? else { return Err(Error::Unsupported); };
        let barrier = barrier?;
        if !matches!(barrier.get_context().which(), Ok(rpc_capnp::disembargo::context::SenderLoopback(id)) if id == pending.id().to_wire()) { return Err(Error::Mismatch); }
        match (barrier.get_target()?.which().map_err(|_| Error::Unsupported)?, target) {
            (rpc_capnp::message_target::ImportedCap(actual), crate::calls::Target::Imported(expected)) if actual == expected.to_wire() => (),
            (rpc_capnp::message_target::PromisedAnswer(actual), crate::calls::Target::Promised { question, path }) => {
                let actual = actual?;
                if actual.get_question_id() != question.to_wire() { return Err(Error::Mismatch); }
                let mut expected = path.fields().iter();
                for op in actual.get_transform()? {
                    match op.which().map_err(|_| Error::Unsupported)? {
                        rpc_capnp::promised_answer::op::Noop(()) => (),
                        rpc_capnp::promised_answer::op::GetPointerField(field) if expected.next() == Some(&field) => (),
                        _ => return Err(Error::Mismatch),
                    }
                }
                if expected.next().is_some() { return Err(Error::Mismatch); }
            }
            _ => return Err(Error::Mismatch),
        }
        Ok(())
    })?;
    frame.offer()?;
    Ok(Embargo { pending, frame })
}
impl<'pool, T, S: SparseStorage<EmbargoId, embargoes::Record<T>>> Embargo<'_, 'pool, '_, '_, T, S> {
    pub fn accept(self, queue: messages::QueueSlot<'_, 'pool, '_, impl Sized>) -> EmbargoId {
        let id = self.pending.accept();
        self.frame.accept(queue);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use capnp::{message::FixedBuilder, word};
    use capntproto_rpc_engine::{
        ids::{ImportId, QuestionId},
        peer_table::{BorrowedSparse, Slot},
    };
    #[test]
    fn rejected_or_mismatched_loopback_frames_cannot_open_an_embargo() {
        let measured = allocation_counter::measure(|| {
            for variant in 0..8 {
                let mut records = [Slot::EMPTY];
                let mut barriers = embargoes::Embargoes::new(BorrowedSparse::new(&mut records))
                    .ok()
                    .unwrap();
                let mut source = [word(0, 0, 0, 0, 0, 0, 0, 0); 40];
                let mut output = FixedBuilder::new(&mut source).unwrap();
                let target = if variant >= 6 {
                    crate::calls::Target::Promised {
                        question: QuestionId::from_wire(17),
                        path: crate::pipeline::Path::new(&[1, 2]),
                    }
                } else {
                    crate::calls::Target::Imported(ImportId::from_wire(7))
                };
                let expected = if variant == 2 {
                    crate::calls::Target::Imported(ImportId::from_wire(8))
                } else if variant == 7 {
                    crate::calls::Target::Promised {
                        question: QuestionId::from_wire(17),
                        path: crate::pipeline::Path::new(&[1, 3]),
                    }
                } else {
                    target
                };
                let mut wire = [word(0, 0, 0, 0, 0, 0, 0, 0); 41];
                let mut buffers = [messages::Buffer::new(capnp::Word::words_to_bytes_mut(
                    &mut wire,
                ))];
                let mut sends = [messages::SendSlot::EMPTY];
                messages::with_messages(
                    &mut buffers,
                    &mut sends[..usize::from(variant != 3)],
                    |messages| {
                        let pending = barriers.reserve(41).unwrap();
                        let id = pending.id();
                        crate::calls::sender_loopback(
                            &mut output,
                            if variant == 1 {
                                EmbargoId::from_wire(99)
                            } else {
                                id
                            },
                            target,
                        )
                        .unwrap();
                        let writer = messages.reserve(41 * 8).unwrap();
                        let length = output.size_in_words() * 8 + 8;
                        capnp::serialize::write_message_segments(
                            &mut &mut messages.write(&writer)[..length],
                            &output.get_segments_for_output(),
                        )
                        .unwrap();
                        let lease = messages
                            .seal(writer, if variant == 5 { length - 1 } else { length })
                            .unwrap();
                        let offered =
                            embargo(pending, expected, messages, lease, Default::default());
                        if matches!(variant, 0 | 6) {
                            let mut slots = [None];
                            let mut queue = messages::Queue::new(&mut slots).unwrap();
                            let accepted = offered.unwrap().accept(queue.reserve().unwrap());
                            assert_eq!(accepted, id);
                            let window = queue.window(messages).unwrap().unwrap();
                            let len = window.bytes().len();
                            assert!(window.advance(len).unwrap());
                            assert_eq!(barriers.receive(id), Ok(Some(41)));
                        } else {
                            if variant == 4 {
                                drop(offered.unwrap());
                            } else {
                                assert!(offered.is_err());
                                drop(offered);
                            }
                            assert_eq!(barriers.receive(id), Err(embargoes::Error::NotAccepted));
                            assert_eq!(barriers.take_rejected(), Some(41));
                        }
                        let writer = messages.reserve(41 * 8).unwrap();
                        messages.discard(writer);
                    },
                )
                .unwrap();
                assert!(barriers.is_empty());
            }
        });
        assert_eq!(measured.count_total, 0);
    }
}
