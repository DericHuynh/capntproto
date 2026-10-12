use super::Publication;
use crate::admission;
use capntproto_rpc_engine::{answers, messages};

/// A prepared Return owns both the answer lifecycle obligation and publication.
/// Its fields are private; only a validated binding can construct it.
#[must_use]
pub struct Answer<'a, 'id, 'storage, A, P> {
    pending: answers::Return<'a, 'id, 'storage, A>,
    publication: P,
}

/// Bind reflected result publication to the exact incoming question's Return.
#[allow(clippy::type_complexity)]
pub fn answer<
    'a,
    'id,
    'answers,
    'exports,
    'table,
    'pool,
    'storage,
    'bytes,
    'imports,
    'import_storage,
    'questions,
    'question_storage,
    'path,
    A,
    E,
    I,
>(
    mut pending: answers::Return<'a, 'id, 'answers, A>,
    publication: Publication<
        'a,
        'exports,
        'table,
        'pool,
        'storage,
        'bytes,
        'imports,
        'import_storage,
        'questions,
        'question_storage,
        'path,
        E,
        I,
    >,
    options: capnp::message::ReaderOptions,
) -> Result<
    Answer<
        'a,
        'id,
        'answers,
        A,
        Publication<
            'a,
            'exports,
            'table,
            'pool,
            'storage,
            'bytes,
            'imports,
            'import_storage,
            'questions,
            'question_storage,
            'path,
            E,
            I,
        >,
    >,
    admission::Error,
> {
    {
        let view = publication.bytes();
        let mut bytes = &*view;
        let message = capnp::serialize::read_message_from_flat_slice_no_alloc(&mut bytes, options)?;
        admission::check_return(&mut pending, message.get_root()?, true)?;
    }
    Ok(Answer {
        pending,
        publication,
    })
}

impl<'pool, A, E, I>
    Answer<'_, '_, '_, A, Publication<'_, '_, '_, 'pool, '_, '_, '_, '_, '_, '_, '_, E, I>>
{
    /// Accept once without callbacks. An owner detached by an earlier Finish is
    /// returned to the caller for disposal outside all protocol table borrows.
    pub fn accept_into(self, slot: messages::QueueSlot<'_, 'pool, '_, impl Sized>) -> Option<A> {
        self.publication.accept_into(slot);
        self.pending.accept()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        publication::reflected::{self, Reference, References},
        rpc_capnp::api::{CapDescriptor, Message, Payload, Return},
    };
    use capnp::{message::FixedBuilder, word, Word};
    use capntproto_rpc_engine::{
        exports,
        ids::{AnswerId, ImportId, WireId},
        imports, questions,
    };

    #[test]
    fn reflected_return_binding_preserves_lifecycle_on_rejection() {
        let measured = allocation_counter::measure(|| {
            for variant in 0..4 {
                let mut es = [exports::Slot::<i32>::EMPTY];
                let mut is = [imports::Slot::EMPTY];
                let mut qs = [questions::Slot::EMPTY];
                let mut as_ = [answers::Slot::EMPTY];
                let mut source = [word(0, 0, 0, 0, 0, 0, 0, 0); 32];
                let mut wire = [word(0, 0, 0, 0, 0, 0, 0, 0); 33];
                let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut wire))];
                let mut sends = [messages::SendSlot::EMPTY];
                exports::with_exports(&mut es, |exports| {
                    imports::with_imports(&mut is, |imports| {
                        questions::with_questions(&mut qs, |questions| {
                            answers::with_answers(&mut as_, |answers| {
                                let key = answers.admit(AnswerId::from_wire(7), 111).unwrap();
                                if variant == 2 {
                                    answers.finish(key.id(), true);
                                }
                                let mut refs = [Reference::Hosted(
                                    imports.admit(ImportId::from_wire(42), 55).unwrap(),
                                )];
                                messages::with_messages(&mut buffers, &mut sends, |messages| {
                                    let mut builder = FixedBuilder::new(&mut source).unwrap();
                                    let mut root = builder.try_init_root::<Message>().unwrap();
                                    let mut result = root.field(Message::RETURN).init().unwrap();
                                    result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
                                    result.scalar(Return::ANSWER_ID).set(if variant == 1 {
                                        9
                                    } else {
                                        7
                                    });
                                    result
                                        .field(Return::RESULTS)
                                        .init()
                                        .unwrap()
                                        .field(Payload::CAP_TABLE)
                                        .init(1)
                                        .unwrap()
                                        .get_mut(0)
                                        .unwrap()
                                        .scalar(CapDescriptor::RECEIVER_HOSTED)
                                        .set(42);
                                    let writer = messages.reserve(33 * 8).unwrap();
                                    let length = builder.size_in_words() * 8 + 8;
                                    capnp::serialize::write_message_segments(
                                        &mut &mut messages.write(&writer)[..length],
                                        &builder.get_segments_for_output(),
                                    )
                                    .unwrap();
                                    let lease = messages.seal(writer, length).unwrap();
                                    let publication = reflected::prepare(
                                        exports.batch(),
                                        messages,
                                        lease,
                                        &mut [0],
                                        Default::default(),
                                        References {
                                            imports,
                                            questions,
                                            slots: &mut refs,
                                        },
                                    )
                                    .unwrap();
                                    let bound = answer(
                                        answers.prepare_return(key).unwrap(),
                                        publication,
                                        Default::default(),
                                    );
                                    if variant == 0 {
                                        let mut queue_slots = [None];
                                        let mut queue =
                                            messages::Queue::new(&mut queue_slots).unwrap();
                                        assert_eq!(
                                            bound.unwrap().accept_into(queue.reserve().unwrap()),
                                            None
                                        );
                                        assert!(answers.prepare_return(key).is_err());
                                        assert!(matches!(refs[0], Reference::None));
                                        assert_eq!(
                                            imports.pending_release(0).unwrap().commit(),
                                            55
                                        );
                                        assert_eq!(
                                            answers.finish(key.id(), false).retired,
                                            Some(111)
                                        );
                                        let window = queue.window(messages).unwrap().unwrap();
                                        let length = window.bytes().len();
                                        window.advance(length).unwrap();
                                    } else {
                                        if variant == 3 {
                                            drop(bound.unwrap());
                                        } else {
                                            assert!(bound.is_err());
                                            drop(bound);
                                        }
                                        assert!(matches!(refs[0], Reference::Hosted(_)));
                                        super::super::release_references(
                                            &mut refs, imports, questions,
                                        );
                                        assert_eq!(
                                            imports.pending_release(0).unwrap().commit(),
                                            55
                                        );
                                        answers.finish(key.id(), false);
                                        assert_eq!(
                                            answers.prepare_return(key).unwrap().accept(),
                                            Some(111)
                                        );
                                    }
                                    assert!(
                                        answers.is_empty()
                                            && imports.is_empty()
                                            && exports.is_empty()
                                    );
                                })
                                .unwrap();
                            })
                            .unwrap();
                        })
                        .unwrap();
                    })
                    .unwrap();
                })
                .unwrap();
            }
        });
        assert_eq!(measured.count_total, 0);
    }
}
