//! Outgoing question routes survive cancellation and returned capability routing.
use capntproto_rpc_engine::{
    answers,
    clients::{self, dispatch, Destination, Location},
    connection, exports,
    ids::WireId,
    imports, messages, questions,
};
use capntproto_rpc_wire::{
    calls,
    clients::{self as bindings, outgoing, Endpoint, Route},
    owned::{self, answer, pipelines, Frame, Question},
    pipeline::Path,
    rpc_capnp::api::{CapDescriptor, Message, Payload, Return},
};

fn drain<'p>(messages: &mut messages::Messages<'p, '_, '_>, queue: &mut messages::Queue<'p, '_>) {
    while let Some(window) = queue.window(messages).unwrap() {
        let length = window.bytes().len();
        window.advance(length).unwrap();
    }
}

pub fn outgoing_question_pipeline(reflected: bool, cancel: bool) {
    let mut a = [super::ZERO; 64];
    let mut b = [super::ZERO; 64];
    let mut buffers = [
        messages::Buffer::new(capnp::Word::words_to_bytes_mut(&mut a)),
        messages::Buffer::new(capnp::Word::words_to_bytes_mut(&mut b)),
    ];
    messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        connection::with_domain(|domain| {
            let mut is = [imports::Slot::EMPTY];
            let mut es = [exports::Slot::EMPTY];
            let mut qs = [questions::Slot::EMPTY];
            let mut ans =
                [answers::Slot::<answer::State<'_, pipelines::Results<'_, '_, (), 0>>>::EMPTY];
            let mut nodes = [const { clients::Slot::<Route<'_, u8, u32, 2>, u8>::EMPTY }; 5];
            let mut listeners = [const { clients::ListenerSlot::EMPTY }; 4];
            let mut subscriptions = [const { clients::SubscriptionSlot::EMPTY }; 20];
            let mut connection = domain
                .bind(connection::GraphStorage {
                    notifications: clients::NotificationStorage {
                        listeners: &mut listeners,
                        subscriptions: &mut subscriptions,
                    },
                    imports: &mut is,
                    exports: &mut es,
                    questions: &mut qs,
                    pipeline_questions: &mut [],
                    answers: &mut ans,
                    clients: &mut nodes,
                })
                .unwrap();
            let imports = &mut connection.ledgers.imports;
            let exports = &mut connection.ledgers.exports;
            let questions = &mut connection.ledgers.questions;
            let answers = &mut connection.ledgers.answers;
            let graph = &mut connection.clients;
            let mut queue_slots = [None];
            let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
            let pending = questions.reserve().unwrap();
            let id = pending.id();
            let frame = Frame::build(messages, 512, Default::default(), |b| {
                calls::bootstrap(b, id)
            })
            .unwrap();
            let mut question =
                Question::send(pending, messages, frame, queue.reserve().unwrap()).unwrap();
            drain(messages, &mut queue);
            let mut outgoing_slots = [outgoing::Slot::<2>::EMPTY];
            let mut outgoing = outgoing::Bindings::new(&mut outgoing_slots, 1).unwrap();
            let client = outgoing
                .bind(&question, questions, graph, Path::new(&[]))
                .unwrap();
            let alias = outgoing
                .bind(&question, questions, graph, Path::new(&[]))
                .unwrap();
            assert_eq!(client.key(), alias.key());
            let mut other_slots = [outgoing::Slot::<2>::EMPTY];
            let mut other = outgoing::Bindings::new(&mut other_slots, 1).unwrap();
            assert!(matches!(
                other.bind(&question, questions, graph, Path::new(&[])),
                Err(outgoing::Error::Question(questions::Error::AlreadyObserved))
            ));
            graph.release(alias);
            // This independently exported original route must continue retaining
            // the peer answer even after the observed promise resolves elsewhere.
            let original_route = graph.snapshot(&client).unwrap();
            if cancel {
                question.cancel(questions);
            }
            assert!(questions.pending_finish().is_none());
            let mut task_slots = [dispatch::TaskSlot::EMPTY];
            let mut runtime = dispatch::borrow_runtime(graph, &mut task_slots).unwrap();
            runtime
                .prepare_call(&client, &mut Some(41))
                .unwrap()
                .accept();
            let ready = runtime.next_ready().unwrap().unwrap();
            let Destination::Endpoint {
                owner: Route::Question(route),
                ..
            } = ready.destination()
            else {
                panic!("question route")
            };
            assert_eq!(route.peer(), 1);
            let (question_id, path) = route.target(questions).unwrap();
            assert_eq!(question_id, id);
            assert!(path.fields().is_empty());
            let running = ready.accept();
            assert_eq!(runtime.complete(running), 41);

            let export = if reflected {
                let client = runtime.endpoint(Route::local(42), Location::Local).unwrap();
                let mut batch = exports.batch();
                let grant = batch.export(client).unwrap();
                batch.commit();
                Some(grant.id())
            } else {
                None
            };
            let frame = Frame::build(messages, 512, Default::default(), |builder| {
                let mut root = builder.try_init_root::<Message>()?;
                let mut result = root.field(Message::RETURN).init()?;
                result.scalar(Return::ANSWER_ID).set(id.to_wire());
                result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
                let mut payload = result.field(Return::RESULTS).init()?;
                payload.field(Payload::CONTENT).set_capability_index(0)?;
                let mut descriptors = payload.field(Payload::CAP_TABLE).init(1)?;
                let mut descriptor = descriptors.get_mut(0).unwrap();
                if let Some(id) = export {
                    descriptor
                        .scalar(CapDescriptor::RECEIVER_HOSTED)
                        .set(id.to_wire());
                } else {
                    descriptor.scalar(CapDescriptor::SENDER_HOSTED).set(7);
                }
                Ok(())
            })
            .unwrap();
            let response: owned::Response<'_, '_, '_, '_, '_, (), 1> = question
                .returned(
                    frame,
                    messages,
                    questions,
                    imports,
                    exports,
                    answers,
                    &mut [0],
                    |_| (),
                )
                .unwrap_or_else(|_| panic!("Return"));
            if let Some(export) = export {
                assert!(exports.release(export, 1).unwrap().is_none());
            }
            let mut import_slots = [bindings::ImportSlot::EMPTY];
            let mut imports_binding = bindings::Imports::new(&mut import_slots, 1).unwrap();
            let mut answer_slots = [pipelines::Slot::<0>::EMPTY];
            let mut answer_binding = pipelines::Bindings::new(&mut answer_slots).unwrap();
            assert!(outgoing.pending_for(response.key(), questions));
            let progress = outgoing
                .poll_one(
                    0,
                    &response,
                    questions,
                    messages,
                    &mut runtime,
                    &mut imports_binding,
                    imports,
                    exports,
                    answers,
                    &mut answer_binding,
                )
                .unwrap();
            let outgoing::Progress::Resolved(barrier) = progress else {
                panic!("pipeline result")
            };
            assert_eq!(barrier.is_some(), reflected);
            assert!(!outgoing.pending_for(response.key(), questions));
            assert!(matches!(
                outgoing
                    .poll_one(
                        0,
                        &response,
                        questions,
                        messages,
                        &mut runtime,
                        &mut imports_binding,
                        imports,
                        exports,
                        answers,
                        &mut answer_binding
                    )
                    .unwrap(),
                outgoing::Progress::Unrelated
            ));
            runtime
                .prepare_call(&client, &mut Some(43))
                .unwrap()
                .accept();
            if let Some(barrier) = barrier {
                assert!(runtime.next_ready().unwrap().is_none());
                assert!(matches!(
                    runtime.barrier_route(&barrier),
                    Route::Question(_)
                ));
                runtime.open(barrier);
            }
            let ready = runtime.next_ready().unwrap().unwrap();
            match ready.destination() {
                Destination::Endpoint {
                    owner: Route::Capability(Endpoint::Local(42)),
                    ..
                } if reflected => (),
                Destination::Endpoint {
                    owner: Route::Capability(Endpoint::Imported(remote)),
                    ..
                } if !reflected => assert_eq!(remote.id().to_wire(), 7),
                _ => panic!("returned capability"),
            }
            let running = ready.accept();
            assert_eq!(runtime.complete(running), 43);
            response.release(messages, questions, imports, exports, answers);
            if let Some(export) = export {
                runtime.release(exports.take_retired(export.to_wire() as usize).unwrap());
            }
            // Even though the response and application question are gone, an
            // exported original route still has authority to call this answer.
            assert!(questions.pending_finish().is_none());
            runtime.release(original_route);
            while let Some(route) = runtime.take_retired() {
                assert!(route.release(imports, questions).is_none());
            }
            assert!(owned::controls::finish_one(
                questions,
                messages,
                &mut queue,
                512,
                Default::default()
            )
            .unwrap());
            drain(messages, &mut queue);
            assert!(questions.is_empty());
            runtime.release(client);
            let mut servers = 0;
            while let Some(route) = runtime.take_retired() {
                if let Some(server) = route.release(imports, questions) {
                    assert_eq!(server, 42);
                    servers += 1;
                }
            }
            assert_eq!(servers, usize::from(reflected));
            if !reflected {
                assert!(owned::controls::release_one(
                    imports,
                    messages,
                    &mut queue,
                    512,
                    Default::default()
                )
                .unwrap()
                .is_some());
                drain(messages, &mut queue);
            }
            assert!(imports.is_empty());
            assert!(exports.is_empty());
            assert!(runtime.is_empty());
            let a = messages.reserve(512).unwrap();
            let b = messages.reserve(512).unwrap();
            messages.discard(a);
            messages.discard(b);
        });
    })
    .unwrap();
}
