use super::*;
use capntproto_rpc_engine::{
    answers, exports,
    ids::{ExportId, WireId},
    imports, questions,
};
use capntproto_rpc_wire::{
    inbound::full,
    reception::full::{Capability, Slots},
    rpc_capnp::{
        self,
        api::{Call, CapDescriptor, Message, Payload, Return},
    },
};

/// A returned receiverHosted capability must survive the same Return releasing
/// its last parameter grant. Rejected messages cannot consume either obligation.
pub fn bounded_reflected_return() -> Result<()> {
    for variant in 0..6 {
        let mut is = [imports::Slot::<i32>::EMPTY];
        let mut es = [exports::Slot::EMPTY];
        let mut as_ = [answers::Slot::<i32>::EMPTY];
        let mut qs = [questions::Slot::EMPTY];
        imports::with_imports(&mut is, |imports| {
            exports::with_exports(&mut es, |exports| {
                answers::with_answers(&mut as_, |answers| {
                    questions::with_questions(&mut qs, |questions| -> Result<()> {
                        let question = questions.reserve().unwrap().accept();
                        let id = question.key().id();
                        let mut batch = exports.batch();
                        let grant = batch.export(41).unwrap();
                        batch.commit();
                        let mut source = [ZERO; 40];
                        let mut original = message::FixedBuilder::new(&mut source)?;
                        let mut root = original.try_init_root::<Message>()?;
                        let mut call = root.field(Message::CALL).init()?;
                        call.scalar(Call::QUESTION_ID).set(id.to_wire());
                        let mut params = call.field(Call::PARAMS).init()?;
                        params.field(Payload::CONTENT).set_capability_index(0)?;
                        let count = if variant == 4 { 2 } else { 1 };
                        let mut table = params.field(Payload::CAP_TABLE).init(count)?;
                        for index in 0..count {
                            table
                                .get_mut(index)
                                .unwrap()
                                .scalar(CapDescriptor::SENDER_HOSTED)
                                .set(grant.id().to_wire());
                        }
                        let original = message::Reader::new(
                            original.get_segments_for_output(),
                            Default::default(),
                        );
                        let mut source = [ZERO; 40];
                        let mut result = message::FixedBuilder::new(&mut source)?;
                        let mut root = result.try_init_root::<Return>()?;
                        root.scalar(Return::ANSWER_ID).set(id.to_wire());
                        let mut payload = root.field(Return::RESULTS).init()?;
                        payload.field(Payload::CONTENT).set_capability_index(0)?;
                        let local = if variant == 1 {
                            ExportId::from_wire(99)
                        } else {
                            grant.id()
                        };
                        payload
                            .field(Payload::CAP_TABLE)
                            .init(1)?
                            .get_mut(0)
                            .unwrap()
                            .scalar(CapDescriptor::RECEIVER_HOSTED)
                            .set(local.to_wire());
                        if variant == 3 {
                            root.scalar(Return::NO_FINISH_NEEDED).set(true);
                        }
                        let result = message::Reader::new(
                            result.get_segments_for_output(),
                            Default::default(),
                        );
                        let question = if variant == 2 {
                            questions.release(question);
                            None
                        } else {
                            Some(question)
                        };
                        let mut ir = [imports::ReceiveSlot::none()];
                        let mut er = [exports::LeaseSlot::new(local)];
                        let mut ar = [answers::PipelineSlot::none()];
                        let mut counts = [0];
                        let pending = full::returned(
                            result.get_root::<rpc_capnp::return_::Reader<'_>>()?,
                            original.get_root()?,
                            questions,
                            imports,
                            exports,
                            answers,
                            Slots {
                                imports: &mut ir,
                                exports: &mut er,
                                answers: &mut ar,
                            },
                            &mut counts,
                        );
                        if matches!(variant, 1 | 3 | 4 | 5) {
                            if variant == 5 {
                                drop(pending.unwrap());
                            } else {
                                assert!(pending.is_err());
                                drop(pending);
                            }
                            assert_eq!(exports.release(grant.id(), 1), Ok(Some(41)));
                            drop(questions.prepare_receive(id, false).unwrap());
                        } else {
                            let (_, reply) = pending.unwrap().accept();
                            if variant == 2 {
                                assert!(matches!(reply, full::Reply::Discarded));
                                assert_eq!(exports.take_retired(0), Some(41));
                            } else {
                                let full::Reply::Results(received) = reply else {
                                    unreachable!()
                                };
                                assert!(exports.lookup(grant.id()).is_none());
                                assert_eq!(exports.take_retired(0), None);
                                let Capability::Local(lease) = received
                                    .capability(
                                        received
                                            .content()
                                            .get_as::<capnp::wire_capability::Reader<'_, ()>>()?,
                                    )
                                    .unwrap()
                                else {
                                    unreachable!()
                                };
                                assert_eq!(exports.get(lease), &41);
                                received.release(imports, exports, answers);
                                assert_eq!(exports.take_retired(0), Some(41));
                            }
                        }
                        if let Some(question) = question {
                            questions.release(question);
                        }
                        assert!(exports.is_empty());
                        Ok(())
                    })
                    .unwrap()
                })
                .unwrap()
            })
            .unwrap()
        })
        .unwrap()?;
    }
    Ok(())
}
