use super::*;
use capntproto_rpc_engine::{
    answers, exports,
    ids::{AnswerId, WireId},
    imports,
};
use capntproto_rpc_wire::{
    inbound, pipeline,
    reception::full::Slots,
    rpc_capnp::{
        self,
        api::{Call, CapDescriptor, MessageTarget, Payload},
    },
};

/// A reflected local capability and promised-answer descriptor retain their
/// owners together with the call target, including early Finish and Release.
pub fn bounded_reflected_capabilities() -> Result<()> {
    let mut is = [imports::Slot::<i32>::EMPTY];
    let mut es = [exports::Slot::EMPTY];
    let mut as_ = [const { answers::Slot::EMPTY }; 2];
    imports::with_imports(&mut is, |imports| {
        exports::with_exports(&mut es, |exports| {
            answers::with_answers(&mut as_, |answers| -> Result<()> {
                let mut batch = exports.batch();
                let grant = batch.export(41).unwrap();
                batch.commit();
                let parent = answers.admit(AnswerId::from_wire(3), 43).unwrap();
                let mut source = [ZERO; 48];
                let mut builder = message::FixedBuilder::new(&mut source)?;
                let mut root = builder.try_init_root::<Call>()?;
                root.scalar(Call::QUESTION_ID).set(5);
                root.field(Call::TARGET)
                    .init()?
                    .scalar(MessageTarget::IMPORTED_CAP)
                    .set(grant.id().to_wire());
                let mut payload = root.field(Call::PARAMS).init()?;
                payload.field(Payload::CONTENT).set_capability_index(0)?;
                let mut table = payload.field(Payload::CAP_TABLE).init(2)?;
                table
                    .get_mut(0)
                    .unwrap()
                    .scalar(CapDescriptor::RECEIVER_HOSTED)
                    .set(grant.id().to_wire());
                table
                    .get_mut(1)
                    .unwrap()
                    .field(CapDescriptor::RECEIVER_ANSWER)
                    .init()?
                    .scalar(rpc_capnp::api::PromisedAnswer::QUESTION_ID)
                    .set(3);
                let reader =
                    message::Reader::new(builder.get_segments_for_output(), Default::default());
                let mut ir = [const { imports::ReceiveSlot::none() }; 2];
                let mut er = [
                    exports::LeaseSlot::new(grant.id()),
                    exports::LeaseSlot::none(),
                    exports::LeaseSlot::new(grant.id()),
                ];
                let mut ar = [
                    answers::PipelineSlot::none(),
                    answers::PipelineSlot::new(parent.id()),
                    answers::PipelineSlot::none(),
                ];
                let mut path = [0; 2];
                let admitted = inbound::full::call(
                    reader.get_root()?,
                    imports,
                    exports,
                    answers,
                    Slots {
                        imports: &mut ir,
                        exports: &mut er,
                        answers: &mut ar,
                    },
                    &mut path,
                )
                .unwrap()
                .accept(47);
                assert_eq!(exports.release(grant.id(), 1), Ok(None));
                answers.finish(parent.id(), false);
                assert!(answers.dispatch(parent, true).unwrap());
                assert_eq!(answers.prepare_return(parent).unwrap().accept(), None);
                admitted.payload.release(imports, exports, answers);
                assert_eq!(answers.take_finished(0), Some(43));
                let pipeline::Target::Hosted(target) = admitted.target else {
                    unreachable!()
                };
                assert_eq!(exports.take_retired(0), None);
                assert_eq!(exports.get(&target), &41);
                assert_eq!(exports.release_local(target), Some(41));
                answers.finish(admitted.answer.id(), false);
                assert_eq!(
                    answers.prepare_return(admitted.answer).unwrap().accept(),
                    Some(47)
                );
                assert!(answers.is_empty() && exports.is_empty() && imports.is_empty());
                Ok(())
            })
            .unwrap()
        })
        .unwrap()
    })
    .unwrap()
}
