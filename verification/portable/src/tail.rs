use super::*;
use capntproto_rpc_engine::{
    answers,
    connection::{with_connection, Storage},
    exports,
    ids::{AnswerId, WireId},
    imports, messages, questions,
};
use capntproto_rpc_wire::{
    admission,
    inbound::full,
    pipeline,
    reception::full::Slots,
    rpc_capnp::{
        self,
        api::{call::SendResultsTo, Call, Message, MessageTarget, Return},
    },
};

/// Same-connection tail adoption over caller IO, including canceled observers,
/// dropped output admission and Finish before the local producer completes.
pub fn bounded_tail_adoption(chunk: usize, canceled: bool) -> Result<()> {
    let mut ai = [imports::Slot::<i32>::EMPTY];
    let mut ae = [exports::Slot::EMPTY];
    let mut aq = [questions::Slot::EMPTY];
    let mut aa = [answers::Slot::EMPTY];
    let mut bi = [imports::Slot::<i32>::EMPTY];
    let mut be = [exports::Slot::<i32>::EMPTY];
    let mut bq = [questions::Slot::EMPTY];
    let mut ba = [answers::Slot::EMPTY];
    let mut output1 = [ZERO; 65];
    let mut output2 = [ZERO; 65];
    let mut buffers = [
        messages::Buffer::new(Word::words_to_bytes_mut(&mut output1)),
        messages::Buffer::new(Word::words_to_bytes_mut(&mut output2)),
    ];
    let mut sends = [const { messages::SendSlot::EMPTY }; 2];
    with_connection(
        Storage {
            imports: &mut ai,
            exports: &mut ae,
            questions: &mut aq,
            answers: &mut aa,
        },
        |a| {
            with_connection(
                Storage {
                    imports: &mut bi,
                    exports: &mut be,
                    questions: &mut bq,
                    answers: &mut ba,
                },
                |b| {
                    messages::with_messages(&mut buffers, &mut sends, |messages| -> Result<()> {
                        let mut queue_slots = [None];
                        let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
                        let original_question = a.questions.reserve().unwrap().accept();
                        let original_id = original_question.key().id();
                        let original_answer = b
                            .answers
                            .admit(AnswerId::from_wire(original_id.to_wire()), 41)
                            .unwrap();
                        let mut original_words = [ZERO; 40];
                        let mut original = message::FixedBuilder::new(&mut original_words)?;
                        original
                            .try_init_root::<Message>()?
                            .field(Message::CALL)
                            .init()?
                            .scalar(Call::QUESTION_ID)
                            .set(original_id.to_wire());
                        let original = message::Reader::new(
                            original.get_segments_for_output(),
                            Default::default(),
                        );
                        let mut batch = a.exports.batch();
                        let server = batch.export(43).unwrap();
                        batch.commit();
                        let reservation = b.questions.reserve_redirected().unwrap();
                        let child_id = reservation.id();
                        let mut words = [ZERO; 64];
                        let mut builder = message::FixedBuilder::new(&mut words)?;
                        let mut root = builder.try_init_root::<Message>()?;
                        let mut call = root.field(Message::CALL).init()?;
                        call.scalar(Call::QUESTION_ID).set(child_id.to_wire());
                        call.group(Call::SEND_RESULTS_TO)
                            .select()
                            .void(SendResultsTo::YOURSELF)
                            .set();
                        call.field(Call::TARGET)
                            .init()?
                            .scalar(MessageTarget::IMPORTED_CAP)
                            .set(server.id().to_wire());
                        let lease = super::rpc::seal(messages, &builder);
                        let retained_call = messages.retain(&lease).unwrap();
                        let child_question =
                            admission::question(reservation, messages, lease, Default::default())
                                .unwrap()
                                .accept(queue.reserve().unwrap());
                        let mut input = [ZERO; 65];
                        let receiver = super::rpc::receive(&mut queue, messages, &mut input, chunk);
                        let frame = receiver.message()?.unwrap();
                        let rpc_capnp::message::Call(call) = frame
                            .get_root::<rpc_capnp::message::Reader<'_>>()?
                            .which()
                            .unwrap()
                        else {
                            unreachable!()
                        };
                        let mut ir = [];
                        let mut er = [exports::LeaseSlot::new(server.id())];
                        let mut ar = [answers::PipelineSlot::none()];
                        let admitted = full::call(
                            call?,
                            &mut a.imports,
                            &mut a.exports,
                            &mut a.answers,
                            Slots {
                                imports: &mut ir,
                                exports: &mut er,
                                answers: &mut ar,
                            },
                            &mut [],
                        )
                        .unwrap()
                        .accept(47);
                        let child_answer = admitted.answer;
                        assert!(a.answers.dispatch(child_answer, true).unwrap());
                        admitted
                            .payload
                            .release(&mut a.imports, &mut a.exports, &mut a.answers);
                        let pipeline::Target::Hosted(target) = admitted.target else {
                            unreachable!()
                        };
                        assert_eq!(a.exports.release_local(target), None);
                        assert_eq!(a.exports.release(server.id(), 1), Ok(Some(43)));

                        builder.reset();
                        let mut root = builder.try_init_root::<Message>()?;
                        let mut result = root.field(Message::RETURN).init()?;
                        result.scalar(Return::ANSWER_ID).set(original_id.to_wire());
                        result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
                        result
                            .scalar(Return::TAKE_FROM_OTHER_QUESTION)
                            .set(child_id.to_wire());
                        for accepted in [false, true] {
                            let lease = super::rpc::seal(messages, &builder);
                            let prepared = admission::tail_answer(
                                b.answers.prepare_return(original_answer).unwrap(),
                                &child_question,
                                &b.questions,
                                messages,
                                lease,
                                Default::default(),
                            )
                            .unwrap();
                            if accepted {
                                assert_eq!(prepared.accept(queue.reserve().unwrap()), None);
                            } else {
                                drop(prepared);
                            }
                        }
                        let original_question = if canceled {
                            a.questions.release(original_question);
                            None
                        } else {
                            Some(original_question)
                        };
                        let mut input = [ZERO; 65];
                        let receiver = super::rpc::receive(&mut queue, messages, &mut input, chunk);
                        let frame = receiver.message()?.unwrap();
                        let rpc_capnp::message::Return(result) = frame
                            .get_root::<rpc_capnp::message::Reader<'_>>()?
                            .which()
                            .unwrap()
                        else {
                            unreachable!()
                        };
                        let mut scratch = [0];
                        let (_, reply) = full::returned(
                            result?,
                            original.get_root()?,
                            &mut a.questions,
                            &mut a.imports,
                            &mut a.exports,
                            &mut a.answers,
                            Slots {
                                imports: &mut [],
                                exports: &mut [],
                                answers: &mut [],
                            },
                            &mut scratch,
                        )
                        .unwrap()
                        .accept();
                        let adopted = match reply {
                            full::Reply::Adopted(owner) => Some(owner),
                            full::Reply::Discarded => None,
                            _ => unreachable!(),
                        };
                        assert_eq!(adopted.is_none(), canceled);
                        assert!(a.answers.prepare_adoption(child_answer.id()).is_err());

                        // Acknowledge the redirected question without serializing its
                        // local results. The adopted owner still carries the producer.
                        builder.reset();
                        let mut root = builder.try_init_root::<Message>()?;
                        let mut result = root.field(Message::RETURN).init()?;
                        result.scalar(Return::ANSWER_ID).set(child_id.to_wire());
                        result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
                        result.void(Return::RESULTS_SENT_ELSEWHERE).set();
                        let lease = super::rpc::seal(messages, &builder);
                        assert_eq!(
                            admission::answer(
                                a.answers.prepare_return(child_answer).unwrap(),
                                messages,
                                lease,
                                Default::default()
                            )
                            .unwrap()
                            .accept(queue.reserve().unwrap()),
                            None
                        );
                        let mut input = [ZERO; 65];
                        let receiver = super::rpc::receive(&mut queue, messages, &mut input, chunk);
                        let frame = receiver.message()?.unwrap();
                        let rpc_capnp::message::Return(result) = frame
                            .get_root::<rpc_capnp::message::Reader<'_>>()?
                            .which()
                            .unwrap()
                        else {
                            unreachable!()
                        };
                        {
                            let view = messages.read(&retained_call);
                            let mut bytes = &*view;
                            let original = capnp::serialize::read_message_from_flat_slice_no_alloc(
                                &mut bytes,
                                Default::default(),
                            )?;
                            let mut scratch = [0];
                            let (_, reply) = full::returned(
                                result?,
                                original.get_root()?,
                                &mut b.questions,
                                &mut b.imports,
                                &mut b.exports,
                                &mut b.answers,
                                Slots {
                                    imports: &mut [],
                                    exports: &mut [],
                                    answers: &mut [],
                                },
                                &mut scratch,
                            )
                            .unwrap()
                            .accept();
                            assert!(matches!(reply, full::Reply::ResultsSentElsewhere));
                        }
                        messages.release(retained_call);
                        if let Some(question) = original_question {
                            a.questions.release(question);
                        }
                        let finish = a.questions.pending_finish().unwrap();
                        assert_eq!(
                            b.answers
                                .finish(
                                    AnswerId::from_wire(finish.id().to_wire()),
                                    finish.release_result_caps()
                                )
                                .retired,
                            Some(41)
                        );
                        finish.accept();
                        b.questions.release(child_question);
                        let finish = b.questions.pending_finish().unwrap();
                        assert_eq!(
                            a.answers
                                .finish(
                                    AnswerId::from_wire(finish.id().to_wire()),
                                    finish.release_result_caps()
                                )
                                .retired,
                            if canceled { Some(47) } else { None }
                        );
                        finish.accept();
                        if let Some(adopted) = adopted {
                            // Application completion happens after the old wire answer
                            // is finished. The adopting local owner remains usable.
                            assert_eq!(a.answers.adopted(&adopted), &47);
                            *a.answers.get_mut(adopted.key()).unwrap() = 53;
                            assert_eq!(a.answers.release_adopted(adopted), Some(53));
                        }
                        assert!(
                            a.questions.is_empty()
                                && a.answers.is_empty()
                                && a.imports.is_empty()
                                && a.exports.is_empty()
                        );
                        assert!(
                            b.questions.is_empty()
                                && b.answers.is_empty()
                                && b.imports.is_empty()
                                && b.exports.is_empty()
                        );
                        Ok(())
                    })
                    .unwrap()
                },
            )
            .unwrap()
        },
    )
    .unwrap()
}
