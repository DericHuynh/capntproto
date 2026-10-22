use capntproto_rpc_engine::{
    clients::{self, Capabilities, Location, Source},
    exports, messages,
};
use capntproto_rpc_wire::{
    clients::{exporting, Endpoint},
    rpc_capnp,
};

/// Exception Resolve needs no replacement export slot, and output exhaustion
/// must leave the same frozen failure available for retry.
pub fn exported_promise_failure() {
    let mut words = [super::ZERO; 97];
    let mut buffers = [messages::Buffer::new(capnp::Word::words_to_bytes_mut(
        &mut words,
    ))];
    messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        clients::with_graph(
            &mut [const { clients::Slot::EMPTY }; 3],
            clients::NotificationStorage {
                listeners: &mut [const { clients::ListenerSlot::EMPTY }; 4],
                subscriptions: &mut [const { clients::SubscriptionSlot::EMPTY }; 12],
            },
            |graph: &mut clients::Graph<'_, '_, Endpoint<'_, u8, u32>, u8>| {
                exports::with_exports(&mut [exports::Slot::EMPTY], |exports| {
                    let source = graph.promise(&mut None).unwrap();
                    let mut observers = [exporting::Slot::EMPTY];
                    let mut exporter = exporting::Bindings::new(&mut observers).unwrap();
                    // A missing export slot rolls back both graph references and
                    // the independent promise, without consuming the source.
                    let mut batch = exports.batch();
                    let occupied = batch.export(graph.retain(&source).unwrap()).unwrap();
                    assert!(matches!(
                        exporter.stage(graph, &mut batch, &source),
                        Err(exporting::Error::Export(exports::Error::Full))
                    ));
                    drop(batch);
                    graph.release(exports.take_retired(0).unwrap());
                    let _ = occupied;
                    let mut batch = exports.batch();
                    let grant = exporter.stage(graph, &mut batch, &source).unwrap();
                    batch.commit();
                    let failure = graph
                        .endpoint(
                            Endpoint::Broken(capnp::ErrorKind::Overloaded),
                            Location::Broken,
                        )
                        .unwrap();
                    assert!(graph
                        .resolve(source.key(), &mut Some(failure), Source::Direct)
                        .unwrap()
                        .is_none());
                    let mut queue_slots = [None];
                    let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
                    let busy = messages.reserve(776).unwrap();
                    assert!(matches!(
                        exporter.poll_one(
                            0,
                            graph,
                            exports,
                            messages,
                            &mut queue,
                            776,
                            Default::default(),
                            &mut [0]
                        ),
                        Err(exporting::Error::Output(_))
                    ));
                    assert!(queue.is_empty());
                    messages.discard(busy);
                    assert_eq!(
                        exporter.poll_one(
                            0,
                            graph,
                            exports,
                            messages,
                            &mut queue,
                            776,
                            Default::default(),
                            &mut [0]
                        ),
                        Ok(exporting::Progress::Published)
                    );
                    let window = queue.window(messages).unwrap().unwrap();
                    let length = window.bytes().len();
                    let mut bytes = window.bytes();
                    let reader = capnp::serialize::read_message_from_flat_slice_no_alloc(
                        &mut bytes,
                        Default::default(),
                    )
                    .unwrap();
                    let rpc_capnp::message::Resolve(resolve) = reader
                        .get_root::<rpc_capnp::message::Reader<'_>>()
                        .unwrap()
                        .which()
                        .unwrap()
                    else {
                        panic!("Resolve")
                    };
                    let rpc_capnp::resolve::Exception(exception) =
                        resolve.unwrap().which().unwrap()
                    else {
                        panic!("exception")
                    };
                    assert_eq!(
                        exception.unwrap().get_type().unwrap(),
                        rpc_capnp::exception::Type::Overloaded
                    );
                    window.advance(length).unwrap();
                    graph.release(exports.release(grant.id(), 1).unwrap().unwrap());
                    graph.release(source);
                    assert!(matches!(
                        graph.take_retired(),
                        Some(Endpoint::Broken(capnp::ErrorKind::Overloaded))
                    ));
                    assert!(graph.is_empty() && exports.is_empty());
                    assert!(!exporter.drain_one(graph));
                    let writer = messages.reserve(776).unwrap();
                    messages.discard(writer);
                })
                .unwrap();
            },
        )
        .unwrap();
    })
    .unwrap();
}
