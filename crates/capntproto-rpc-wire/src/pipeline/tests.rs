use super::*;
use capnp::{message::FixedBuilder, word};
use rpc_capnp::api::{promised_answer::Op, MessageTarget, Payload, PromisedAnswer};

#[test]
fn prepared_transform_preserves_scratch_on_capacity_and_late_invalid_operations() {
    use capnp::traits::IntoInternalStructReader;
    let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 32];
    let base = words.as_ptr() as usize;
    let mut builder = FixedBuilder::new(&mut words).unwrap();
    let mut promised = builder.try_init_root::<PromisedAnswer>().unwrap();
    let mut operations = promised.field(PromisedAnswer::TRANSFORM).init(3).unwrap();
    operations
        .get_mut(0)
        .unwrap()
        .scalar(Op::GET_POINTER_FIELD)
        .set(5);
    operations.get_mut(1).unwrap().void(Op::NOOP).set();
    operations
        .get_mut(2)
        .unwrap()
        .scalar(Op::GET_POINTER_FIELD)
        .set(9);
    let (length, last) = {
        let segments = builder.get_segments_for_output();
        let reader = capnp::message::Reader::new(segments, Default::default());
        let promised = reader
            .get_root::<rpc_capnp::promised_answer::Reader<'_>>()
            .unwrap();
        let prepared = PreparedPath::new(promised).unwrap();
        assert_eq!(prepared.len(), 2);
        let mut small = [77];
        assert_eq!(prepared.decode(&mut small).unwrap_err(), Error::Capacity);
        assert_eq!(small, [77]);
        let mut scratch = [77, 88, 99];
        assert_eq!(
            PreparedPath::new(promised)
                .unwrap()
                .decode(&mut scratch)
                .unwrap()
                .fields(),
            &[5, 9]
        );
        assert_eq!(scratch, [5, 9, 99]);
        let last = promised
            .get_transform()
            .unwrap()
            .get(2)
            .into_internal_struct_reader()
            .get_data_section_as_blob()
            .as_ptr() as usize
            - base;
        (reader.into_segments()[0].len(), last)
    };
    let _ = builder.into_segment();
    // Op's first UInt16 is the union discriminant. Corrupt the final operation
    // after a valid prefix to ensure validation precedes every scratch write.
    capnp::Word::words_to_bytes_mut(&mut words)[last..last + 2].fill(0xff);
    let segments = [&capnp::Word::words_to_bytes(&words)[..length]];
    let reader = capnp::message::Reader::new(segments.as_slice(), Default::default());
    let mut scratch = [77, 88];
    assert_eq!(
        Path::decode(reader.get_root().unwrap(), &mut scratch).unwrap_err(),
        Error::UnknownTransform
    );
    assert_eq!(scratch, [77, 88]);
}

#[test]
fn normalized_transform_uses_caller_storage_and_reader_limits() {
    let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 64];
    let mut builder = FixedBuilder::new(&mut words).unwrap();
    let mut promised = builder.try_init_root::<PromisedAnswer>().unwrap();
    let mut operations = promised.field(PromisedAnswer::TRANSFORM).init(3).unwrap();
    operations.get_mut(0).unwrap().void(Op::NOOP).set();
    operations
        .get_mut(1)
        .unwrap()
        .scalar(Op::GET_POINTER_FIELD)
        .set(0);
    operations.get_mut(2).unwrap().void(Op::NOOP).set();
    let reader = capnp::message::Reader::new(builder.get_segments_for_output(), Default::default());
    let promised = reader
        .get_root::<rpc_capnp::promised_answer::Reader<'_>>()
        .unwrap();
    assert_eq!(
        Path::decode(promised, &mut []).unwrap_err(),
        Error::Capacity
    );
    let mut scratch = [99, 101];
    let path = Path::decode(promised, &mut scratch).unwrap();
    assert_eq!(path.fields(), &[0]);

    let mut result_words = [word(0, 0, 0, 0, 0, 0, 0, 0); 8];
    let mut result = FixedBuilder::new(&mut result_words).unwrap();
    result
        .try_init_root::<Payload>()
        .unwrap()
        .field(Payload::CONTENT)
        .set_capability_index(7)
        .unwrap();
    let message = capnp::message::Reader::new(result.get_segments_for_output(), Default::default());
    assert_eq!(path.resolve(message.get_root().unwrap()).unwrap(), 7);
    assert!(Path::new(&[1])
        .resolve(message.get_root().unwrap())
        .is_err());
    assert!(Path::new(&[0, 0])
        .resolve(message.get_root().unwrap())
        .is_err());
    let limited = capnp::message::Reader::new(
        result.get_segments_for_output(),
        capnp::message::ReaderOptions {
            nesting_limit: 0,
            ..Default::default()
        },
    );
    assert!(path.resolve(limited.get_root().unwrap()).is_err());
    assert_eq!(scratch, [0, 101]);
}

#[test]
fn admitted_promised_target_survives_finish_and_wire_identity_reuse_without_allocation() {
    let measured = allocation_counter::measure(|| {
        let mut answer_slots = [answers::Slot::EMPTY, answers::Slot::EMPTY];
        let mut export_slots = [exports::Slot::<u8>::EMPTY];
        let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 16];
        let mut builder = FixedBuilder::new(&mut words).unwrap();
        Path::new(&[])
            .write(
                builder
                    .try_init_root::<MessageTarget>()
                    .unwrap()
                    .field(MessageTarget::PROMISED_ANSWER)
                    .init()
                    .unwrap(),
                QuestionId::from_wire(9),
            )
            .unwrap();
        let reader =
            capnp::message::Reader::new(builder.get_segments_for_output(), Default::default());
        answers::with_answers(&mut answer_slots, |answers| {
            exports::with_exports(&mut export_slots, |exports| {
                let key = answers.admit(AnswerId::from_wire(9), 42).unwrap();
                let target = admit(reader.get_root().unwrap(), exports, answers, &mut []).unwrap();
                let Target::Promised { answer, path } = target else {
                    panic!("wrong target");
                };
                assert!(path.fields().is_empty());
                assert!(!answers.finish(key.id(), false).cancel);
                assert!(answers.dispatch(key, true).unwrap());
                assert_eq!(answers.prepare_return(key).unwrap().accept(), None);
                let new = answers.admit(key.id(), 17).unwrap();
                assert_eq!(answers.pipelined(&answer), &42);
                assert_eq!(answers.release_pipeline(answer), Some(42));
                assert_eq!(answers.get(new), Ok(&17));
            })
            .unwrap();
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn rejected_transform_does_not_pin_answer_and_hosted_target_retains_released_export() {
    let mut answer_slots = [answers::Slot::EMPTY];
    let mut export_slots = [exports::Slot::EMPTY];
    let mut words = [word(0, 0, 0, 0, 0, 0, 0, 0); 24];
    let mut builder = FixedBuilder::new(&mut words).unwrap();
    Path::new(&[0])
        .write(
            builder
                .try_init_root::<MessageTarget>()
                .unwrap()
                .field(MessageTarget::PROMISED_ANSWER)
                .init()
                .unwrap(),
            QuestionId::from_wire(9),
        )
        .unwrap();
    answers::with_answers(&mut answer_slots, |answers| {
        exports::with_exports(&mut export_slots, |exports| {
            let key = answers.admit(AnswerId::from_wire(9), 42).unwrap();
            let reader =
                capnp::message::Reader::new(builder.get_segments_for_output(), Default::default());
            assert!(matches!(
                admit(reader.get_root().unwrap(), exports, answers, &mut []),
                Err(Error::Capacity)
            ));
            assert!(answers.finish(key.id(), false).cancel);
            assert_eq!(answers.prepare_return(key).unwrap().accept(), Some(42));

            let mut batch = exports.batch();
            let export = batch.export(17).unwrap();
            batch.commit();
            builder.reset();
            builder
                .try_init_root::<MessageTarget>()
                .unwrap()
                .scalar(MessageTarget::IMPORTED_CAP)
                .set(export.id().to_wire());
            let reader =
                capnp::message::Reader::new(builder.get_segments_for_output(), Default::default());
            let Target::Hosted(lease) =
                admit(reader.get_root().unwrap(), exports, answers, &mut []).unwrap()
            else {
                panic!("wrong target");
            };
            assert_eq!(exports.release(export.id(), 1), Ok(None));
            assert_eq!(exports.get(&lease), &17);
            assert_eq!(exports.release_local(lease), Some(17));
        })
        .unwrap();
    })
    .unwrap();
}
