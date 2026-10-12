//! The hosted result bridge ends in this same allocation-free answer path.
use capnp::{word, Word};
use capntproto_rpc_engine::{
    clients::Location,
    connection,
    ids::{ImportId, QuestionId, WireId},
    messages,
};
use capntproto_rpc_wire::{
    calls::{self, DynamicCall, Target},
    clients::{self, Route},
    owned::{
        connection::{Answer, FixedStorage, Request, Termination},
        pipelines, Frame,
    },
    pipeline,
};
use core::num::NonZeroUsize;

pub fn bounded_server_output() {
    let mut words = [[word(0, 0, 0, 0, 0, 0, 0, 0); 64]; 4];
    let mut buffers = words
        .each_mut()
        .map(|w| messages::Buffer::new(Word::words_to_bytes_mut(w)));
    messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        connection::with_domain(|domain| {
            let mut storage = FixedStorage::<(), (), u8, u32, 2, 1, 4, 12>::new();
            let mut driver = storage.bind(domain, 1).unwrap();
            let target = driver
                .runtime
                .endpoint(Route::local(73), Location::Local)
                .unwrap();
            let id = {
                let mut batch = driver.ledgers.exports.batch();
                let id = clients::stage(&mut driver.runtime, &mut batch, &target)
                    .unwrap()
                    .id();
                batch.commit();
                id
            };
            let frame = Frame::build(messages, 512, Default::default(), |b| {
                calls::dynamic_call(
                    b,
                    QuestionId::from_wire(3),
                    Target::Imported(ImportId::from_wire(id.to_wire())),
                    DynamicCall::default(),
                    |_| Ok(()),
                )
            })
            .unwrap();
            let call = Request::admit(
                frame,
                messages,
                &mut driver.ledgers.imports,
                &mut driver.ledgers.exports,
                &mut driver.ledgers.answers,
                Answer::new(pipelines::Results::new(())),
                |_| (),
            )
            .unwrap_or_else(|_| panic!("request"));
            let key = call.answer();
            assert!(driver.ledgers.answers.dispatch(key, true).unwrap());
            let parent = driver.ledgers.answers.pipeline(key.id()).unwrap();
            let observer = driver
                .pipelines
                .bind(
                    &mut driver.runtime,
                    &mut driver.ledgers.answers,
                    &parent,
                    pipeline::Path::new(&[]),
                )
                .unwrap();
            driver.ledgers.answers.release_pipeline_deferred(parent);
            let result = driver
                .local_result_dynamic(
                    0,
                    0,
                    &[None, Some(&target)],
                    messages,
                    512,
                    Default::default(),
                    |p| p.set_capability_index(1),
                )
                .unwrap();
            driver
                .complete_answer(&call, Ok(result), messages)
                .unwrap_or_else(|_| panic!("complete"));
            let mut entries = [None];
            let mut queue = messages::Queue::new(&mut entries).unwrap();
            driver
                .send_answer(
                    key,
                    messages,
                    &mut queue,
                    512,
                    Default::default(),
                    &mut [0; 4],
                    |_, _| Ok(()),
                )
                .unwrap();
            assert_eq!(
                driver.poll_pipeline(0, messages).unwrap(),
                capntproto_rpc_wire::owned::connection::Propagation::Resolved
            );
            let output = queue.window(messages).unwrap().unwrap();
            let bytes = output.bytes().len();
            output.advance(bytes).unwrap();
            driver.runtime.release(observer);
            driver.release_call(call, messages);
            driver.runtime.release(target);
            let mut closing = driver.terminate(&mut queue);
            for _ in 0..256 {
                if matches!(
                    closing
                        .poll(messages, NonZeroUsize::new(16).unwrap())
                        .unwrap(),
                    Termination::Done
                ) {
                    return;
                }
            }
            panic!("cleanup");
        });
    })
    .unwrap();
}
