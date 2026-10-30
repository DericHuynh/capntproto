use super::*;

#[test]
fn observer_slots_reuse_bitmap_and_heap_without_retargeting_late_events() {
    const COUNT: usize = if cfg!(miri) { 3 } else { 137 };
    let measured = allocation_counter::measure(|| {
        let mut words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 64];
        let mut buffers = [messages::Buffer::new(capnp::Word::words_to_bytes_mut(
            &mut words,
        ))];
        messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
            capntproto_rpc_engine::connection::with_domain(|domain| {
                let mut question_slots = [questions::Slot::EMPTY];
                let mut answers: [answers::Slot<()>; 0] = [];
                let mut nodes =
                    [const { clients::Slot::<Route<'_, u8, u32, 1>, u8>::EMPTY }; COUNT * 3 + 5];
                let mut listeners = [const { clients::ListenerSlot::EMPTY }; 1];
                let mut subscriptions = [const { clients::SubscriptionSlot::EMPTY }; COUNT];
                let mut connection = domain
                    .bind(capntproto_rpc_engine::connection::GraphStorage {
                        notifications: clients::NotificationStorage {
                            listeners: &mut listeners,
                            subscriptions: &mut subscriptions,
                        },
                        imports: &mut [],
                        exports: &mut [],
                        questions: &mut question_slots,
                        pipeline_questions: &mut [],
                        answers: &mut answers,
                        clients: &mut nodes,
                    })
                    .unwrap();
                let questions = &mut connection.ledgers.questions;
                let graph = &mut connection.clients;
                let pending = questions.reserve().unwrap();
                let frame = owned::Frame::build(messages, 512, Default::default(), |builder| {
                    calls::bootstrap(builder, pending.id())
                })
                .unwrap();
                let mut send_slots = [None];
                let mut queue = messages::Queue::new(&mut send_slots).unwrap();
                let mut question =
                    owned::Question::<0>::send(pending, messages, frame, queue.reserve().unwrap())
                        .unwrap();
                let window = queue.window(messages).unwrap().unwrap();
                let length = window.bytes().len();
                window.advance(length).unwrap();
                let mut slots = [const { Slot::<1>::EMPTY }; COUNT];
                let mut bindings = Bindings::new(&mut slots, 1).unwrap();
                let mut clients = core::array::from_fn::<_, COUNT, _>(|index| {
                    let client = bindings
                        .bind(
                            &question,
                            questions,
                            graph,
                            pipeline::Path::new(&[index as u16]),
                        )
                        .unwrap();
                    assert_eq!(
                        bindings.slots[index].entry.as_ref().unwrap().client,
                        client.key()
                    );
                    Some(client)
                });
                assert!(matches!(
                    bindings.bind(&question, questions, graph, pipeline::Path::new(&[1000])),
                    Err(Error::ObserversFull)
                ));
                let alias = bindings
                    .bind(&question, questions, graph, pipeline::Path::new(&[0]))
                    .unwrap();
                assert_eq!(alias.key(), clients[0].as_ref().unwrap().key());
                graph.release(alias);
                for index in [COUNT - 1, COUNT / 2, 0] {
                    graph.release(clients[index].take().unwrap());
                }
                let events =
                    core::array::from_fn::<_, 3, _>(|_| bindings.next_event(graph).unwrap());
                for event in &events {
                    assert_eq!(
                        bindings.abandon(
                            clients::Event {
                                subscription: event.subscription,
                                client: event.client,
                                owner: event.owner,
                                kind: event.kind,
                            },
                            questions,
                            graph
                        ),
                        Some(question.key())
                    );
                }
                for (path, index) in [0, COUNT / 2, COUNT - 1].into_iter().enumerate() {
                    let client = bindings
                        .bind(
                            &question,
                            questions,
                            graph,
                            pipeline::Path::new(&[1000 + path as u16]),
                        )
                        .unwrap();
                    assert_eq!(
                        bindings.slots[index].entry.as_ref().unwrap().client,
                        client.key()
                    );
                    clients[index] = Some(client);
                }
                for event in events {
                    assert_eq!(
                        bindings.abandon(event, questions, graph),
                        None,
                        "an old subscription cannot retire a replacement"
                    );
                }
                question.cancel(questions);
                for client in clients.into_iter().flatten() {
                    graph.release(client);
                }
                while let Some(event) = bindings.next_event(graph) {
                    bindings.abandon(event, questions, graph).unwrap();
                }
                assert!(bindings.is_empty());
                while let Some(route) = graph.take_retired() {
                    assert!(route
                        .release::<()>(&mut connection.ledgers.imports, questions)
                        .is_none());
                }
                questions.pending_finish().unwrap().accept();
                assert!(
                    !questions.is_empty(),
                    "cancellation retains the late-Return tombstone"
                );
                questions.returned(question.key().id(), false).unwrap();
                assert!(questions.is_empty());
                assert!(graph.is_empty());
            });
        })
        .unwrap();
    });
    assert_eq!(measured.count_total, 0);
}
