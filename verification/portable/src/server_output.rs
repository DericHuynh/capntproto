//! The hosted result bridge ends in this same allocation-free answer path.
use capnp::{word, Word};
use capntproto_rpc_engine::clients::Capabilities as _;
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
        connection::{Answer, FixedStorage, Termination},
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
                .capabilities()
                .endpoint(Route::local(73), Location::Local)
                .unwrap();
            let id = {
                let (mut graph, mut batch) = driver.export_batch();
                let id = clients::stage(&mut graph, &mut batch, &target)
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
            let call = driver
                .admit_request(
                    frame,
                    messages,
                    Answer::new(pipelines::Results::new(())),
                    |_| (),
                )
                .unwrap_or_else(|_| panic!("request"));
            let key = call.answer();
            assert!(driver.answers().dispatch(key, true).unwrap());
            let parent = driver.answer_pipeline(key.id()).unwrap();
            let observer = driver
                .project_answer(&parent, pipeline::Path::new(&[]))
                .unwrap();
            driver.release_answer_pipeline_deferred(parent);
            let result = driver
                .local_result_dynamic(
                    0,
                    0,
                    [None, Some(&target)].into_iter(),
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
            driver.capabilities().release(observer);
            driver.release_call(call, messages);
            driver.capabilities().release(target);
            queue.fail();
            while queue.discard_one(messages).unwrap() {}
            let mut closing = driver.terminate();
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
