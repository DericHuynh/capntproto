//! Reflected owning results keep exact advertised routes and retry intact.
use capntproto_rpc_engine::{
    answers,
    clients::{self, Capabilities},
    connection,
    ids::{AnswerId, ImportId, WireId},
    imports, messages, questions,
};
use capntproto_rpc_wire::{
    clients::{self as binding, Endpoint, Route},
    owned::{answer, pipelines, Frame},
    pipeline,
    publication::reflected::{Reference, References},
    rpc_capnp::{
        self,
        api::{CapDescriptor, Message, Payload, PromisedAnswer, Return},
    },
};

pub fn owning_reflected_output(reflect_question: bool) {
    let mut words = [[super::ZERO; 97]; 3];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        connection::with_domain(|domain| {
            let mut is = [imports::Slot::EMPTY];
            let mut es = [];
            let mut qs = [questions::Slot::EMPTY];
            let mut ans = [answers::Slot::EMPTY];
            let mut nodes = [const { clients::Slot::<Route<'_, u8, u32, 2>, u8>::EMPTY }; 4];
            let mut connection = domain
                .bind(connection::GraphStorage {
                    notifications: capntproto_rpc_engine::clients::NotificationStorage::empty(),
                    imports: &mut is,
                    exports: &mut es,
                    questions: &mut qs,
                    answers: &mut ans,
                    clients: &mut nodes,
                })
                .unwrap();
            let imports = &mut connection.ledgers.imports;
            let questions = &mut connection.ledgers.questions;
            let exports = &mut connection.ledgers.exports;
            let answers = &mut connection.ledgers.answers;
            let graph = &mut connection.clients;
            let key = answers
                .admit(AnswerId::from_wire(7), answer::State::new(41))
                .unwrap_or_else(|_| panic!("answer"));
            let mut map_slots = [binding::ImportSlot::EMPTY];
            let mut bindings = binding::Imports::new(&mut map_slots, 1).unwrap();
            let question = questions.reserve().unwrap().accept();
            let qid = question.key().id();
            let mut slots = [imports::ReceiveSlot::new_promise(
                ImportId::from_wire(19),
                (),
            )];
            imports.prepare_receive(&mut slots).unwrap().commit();
            let local = slots[0].take_local().unwrap();
            let observed = bindings.bind(graph, imports, &local).unwrap();
            let replacement = graph
                .endpoint(Route::local(99), clients::Location::Local)
                .unwrap();
            graph
                .resolve(
                    observed.key(),
                    &mut Some(replacement),
                    clients::Source::Direct,
                )
                .unwrap();
            // The ordinary client now routes locally, but receiverHosted still
            // advertises import 19 and must retain that precise remote endpoint.
            assert!(matches!(
                graph.inspect(&observed).unwrap(),
                clients::Destination::Endpoint {
                    owner: Route::Capability(Endpoint::Local(99)),
                    ..
                }
            ));
            let mut refs = [if reflect_question {
                Reference::Answer {
                    question: questions.retain(&question).unwrap(),
                    path: pipeline::Path::new(&[]),
                }
            } else {
                Reference::Hosted(imports.retain(&local).unwrap())
            }];
            let mut references = References {
                imports,
                questions,
                slots: &mut refs,
            };
            let frame = Frame::build(messages, 776, Default::default(), |builder| {
                let mut root = builder.try_init_root::<Message>()?;
                let mut result = root.field(Message::RETURN).init()?;
                result.scalar(Return::ANSWER_ID).set(key.id().to_wire());
                result.scalar(Return::RELEASE_PARAM_CAPS).set(false);
                let mut payload = result.field(Return::RESULTS).init()?;
                payload.field(Payload::CONTENT).set_capability_index(0)?;
                let mut descriptors = payload.field(Payload::CAP_TABLE).init(1)?;
                let mut descriptor = descriptors.get_mut(0).unwrap();
                if reflect_question {
                    descriptor
                        .field(CapDescriptor::RECEIVER_ANSWER)
                        .init()?
                        .scalar(PromisedAnswer::QUESTION_ID)
                        .set(qid.to_wire());
                } else {
                    descriptor.scalar(CapDescriptor::RECEIVER_HOSTED).set(19);
                }
                Ok(())
            })
            .unwrap();
            let batch = exports.batch();
            // Force graph rejection first. It must not consume reflection refs.
            let occupying = graph
                .endpoint(Route::local(100), clients::Location::Local)
                .unwrap();
            assert!(matches!(
                pipelines::Published::<1>::from_reflected(
                    &frame,
                    messages,
                    &batch,
                    graph,
                    &bindings,
                    &mut references,
                    &mut []
                ),
                Err(pipelines::Error::Graph(clients::Error::Full))
            ));
            graph.release(occupying);
            let mut retired_servers = 0;
            while let Some(endpoint) = graph.take_retired() {
                if let Some(server) = endpoint.release(references.imports, references.questions) {
                    retired_servers += server;
                }
            }
            assert_eq!(retired_servers, 100);
            let published = pipelines::Published::<1>::from_reflected(
                &frame,
                messages,
                &batch,
                graph,
                &bindings,
                &mut references,
                &mut [],
            )
            .unwrap();
            let client = published
                .target(pipeline::Path::new(&[]), messages, graph)
                .unwrap();
            let clients::Destination::Endpoint {
                owner,
                location: clients::Location::Remote(1),
                ..
            } = graph.inspect(&client).unwrap()
            else {
                panic!("frozen original endpoint")
            };
            match owner {
                Route::Question(route) if reflect_question => {
                    assert_eq!(route.target(references.questions).unwrap().0, qid)
                }
                Route::Capability(Endpoint::Imported(remote)) if !reflect_question => {
                    assert_eq!(remote.id().to_wire(), 19)
                }
                _ => panic!("wrong frozen route"),
            }
            graph.release(client);
            // Occupy the only send slot, but leave FIFO space for an attempted
            // reply. Retry must preserve the same frame and all reflection refs.
            let mut queue_slots = [None, None];
            let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
            // Hold the send slot without committing the reflected answer.
            let writer = messages.reserve(8).unwrap();
            messages.write(&writer)[..8].fill(0);
            let lease = messages.seal(writer, 8).unwrap();
            let offer = messages.offer(&lease).unwrap();
            queue.reserve().unwrap().accept(messages, offer);
            messages.release(lease);
            let rejected = answer::try_send_reflected(
                key,
                answers,
                batch,
                messages,
                frame,
                &mut [],
                &mut references,
                queue.reserve().unwrap(),
            )
            .err()
            .unwrap();
            assert!(
                matches!(
                    rejected.error,
                    capntproto_rpc_wire::owned::Error::Storage(messages::Error::NoSendSlot)
                ),
                "{}",
                rejected.error
            );
            assert!(!matches!(references.slots[0], Reference::None));
            let window = queue.window(messages).unwrap().unwrap();
            assert_eq!(window.bytes().len(), 8);
            window.advance(8).unwrap();
            assert!(answer::try_send_reflected(
                key,
                answers,
                rejected.grants,
                messages,
                rejected.frame,
                &mut [],
                &mut references,
                queue.reserve().unwrap()
            )
            .unwrap_or_else(|_| panic!("retry reflection"))
            .is_none());
            assert!(matches!(references.slots[0], Reference::None));
            let window = queue.window(messages).unwrap().unwrap();
            let length = window.bytes().len();
            let mut bytes = window.bytes();
            let reader = capnp::serialize::read_message_from_flat_slice_no_alloc(
                &mut bytes,
                Default::default(),
            )
            .unwrap();
            assert!(matches!(
                reader
                    .get_root::<rpc_capnp::message::Reader<'_>>()
                    .unwrap()
                    .which()
                    .unwrap(),
                rpc_capnp::message::Return(_)
            ));
            window.advance(length).unwrap();
            assert_eq!(
                answers
                    .finish(key.id(), false)
                    .retired
                    .unwrap()
                    .release(messages),
                41
            );
            published.release(messages, graph);
            references.imports.release(local);
            references
                .questions
                .prepare_receive(qid, false)
                .unwrap()
                .accept();
            references.questions.release(question);
            graph.release(observed);
            let mut servers = 0;
            while let Some(endpoint) = graph.take_retired() {
                if let Some(server) = endpoint.release(references.imports, references.questions) {
                    servers += server;
                }
            }
            assert_eq!(servers, 99);
            assert!(graph.is_empty());
            let finished = references.questions.pending_finish().unwrap();
            finished.accept();
            references.imports.pending_release(0).unwrap().commit();
            assert!(references.imports.is_empty() && references.questions.is_empty());
            let writers = core::array::from_fn::<_, 3, _>(|_| messages.reserve(776).unwrap());
            for writer in writers {
                messages.discard(writer);
            }
        });
    })
    .unwrap();
}
