//! Protocol Abort and optional-Resolve fallback use the same owning driver.
use super::{driver::with_owned_peer, portable_capnp::echo, ZERO};
use capntproto_rpc_engine::clients::Capabilities as _;
use capntproto_rpc_engine::{
    clients,
    ids::{ImportId, QuestionId, WireId},
    messages,
};
use capntproto_rpc_wire::{
    calls, clients as binding,
    owned::{
        self,
        connection::{Received, Termination},
        Frame,
    },
    rpc_capnp::{
        self,
        api::{CapDescriptor, Message, Resolve},
    },
};

pub fn owning_protocol_controls(required_unimplemented: bool) {
    let mut words = [[ZERO; 97]; 5];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut queue_slots = [None];
        let mut queue = super::driver::message_queue(&mut queue_slots).unwrap();
        with_owned_peer(|mut peer| {
            let service = peer.capabilities().endpoint(binding::Route::local(41), clients::Location::Local).unwrap();
            let (mut graph, mut batch) = peer.export_batch();
            let first = binding::stage(&mut graph, &mut batch, &service).unwrap();
            let export = first.id();
            let _second_grant = batch.grant(first.key()).unwrap();
            batch.commit();
            let frame = Frame::build(messages, 776, Default::default(), |builder| calls::call(builder, QuestionId::from_wire(71), ImportId::from_wire(export.to_wire()), echo::Owned::ECHO, |_| Ok(()))).unwrap();
            assert!(matches!(peer.receive(frame, messages, &mut [0; 4], |_| panic!("params"), || 43).unwrap_or_else(|_| panic!("Call")), Received::Queued(_)));
            for _ in 0..2 {
                let mut original_words = [ZERO; 32];
                let mut original = capnp::message::FixedBuilder::new(&mut original_words).unwrap();
                let mut root = original.try_init_root::<Message>().unwrap();
                let mut resolve = root.field(Message::RESOLVE).init().unwrap();
                resolve.scalar(Resolve::PROMISE_ID).set(17);
                resolve.field(Resolve::CAP).init().unwrap().scalar(CapDescriptor::SENDER_HOSTED).set(export.to_wire());
                let segments = [original.into_segment()];
                let reader = capnp::message::Reader::new(capnp::message::SegmentArray::new(&segments), Default::default());
                let source = reader.get_root::<rpc_capnp::message::Reader<'_>>().unwrap();
                let frame = Frame::build(messages, 776, Default::default(), |builder| calls::unimplemented(builder, source)).unwrap();
                assert!(matches!(peer.receive(frame, messages, &mut [0; 4], |_| panic!("control"), || panic!("control")).unwrap_or_else(|rejected| panic!("Unimplemented: {}", rejected.error)), Received::Unimplemented));
                assert!(!peer.drain_export(0), "admitted call keeps its export lease");
            }
            assert!(peer.ledgers().exports.lookup(export).is_none());
            // Abort is admissible with an existing queued call and must not
            // invoke a producer, emit cleanup, or admit a later request.
            let frame = Frame::build(messages, 776, Default::default(), |builder| {
                if required_unimplemented {
                    let mut root = builder.try_init_root::<Message>()?;
                    let mut unsupported = root.field(Message::UNIMPLEMENTED).init()?;
                    unsupported.field(Message::BOOTSTRAP).init()?;
                    Ok(())
                } else { calls::abort(builder, capnp::ErrorKind::Overloaded, "peer overloaded") }
            }).unwrap();
            let reason = if required_unimplemented { capnp::ErrorKind::Unimplemented } else { capnp::ErrorKind::Overloaded };
            assert!(matches!(peer.receive(frame, messages, &mut [0; 4], |_| panic!("control"), || panic!("control")).unwrap_or_else(|_| panic!("Abort")), Received::Aborted(kind) if kind == reason));
            assert_eq!(peer.disconnected(), Some(reason));
            peer.disconnect(capnp::ErrorKind::Disconnected);
            assert_eq!(peer.disconnected(), Some(reason), "preserve first failure");
            assert!(matches!(peer.send(messages, &mut queue, 776, Default::default(), |_, _| panic!("cannot build after abort")), Err(owned::connection::Error::Disconnected(kind)) if kind == reason));
            assert!(matches!(peer.poll_maintenance(messages, &mut queue, 776, Default::default(), &mut [0; 4], core::num::NonZeroUsize::new(1).unwrap()), Err(owned::connection::Error::Disconnected(kind)) if kind == reason));
            let frame = Frame::build(messages, 776, Default::default(), |builder| calls::bootstrap(builder, QuestionId::from_wire(72))).unwrap();
            let rejected = peer.receive(frame, messages, &mut [0; 4], |_| panic!("closed"), || panic!("closed")).err().unwrap();
            assert_eq!(rejected.error, owned::connection::Error::Disconnected(reason));
            assert!(rejected.producer.is_none());
            rejected.frame.release(messages);
            peer.capabilities().release(service);
            queue.fail();
            while queue.discard_one(messages).unwrap() {}
            let mut closing = peer.terminate();
            let mut owners = [false; 2];
            let mut done = false;
            for _ in 0..200 {
                match closing.poll(messages, core::num::NonZeroUsize::new(1).unwrap()).unwrap() {
                    Termination::Pending => (),
                    Termination::Done => { done = true; break; }
                    Termination::Producer(43) => assert!(!core::mem::replace(&mut owners[0], true)),
                    Termination::Server(41) => assert!(!core::mem::replace(&mut owners[1], true)),
                    _ => panic!("unexpected owner"),
                }
            }
            assert!(done && owners == [true, true]);
            let writers = core::array::from_fn::<_, 5, _>(|_| messages.reserve(776).unwrap());
            for writer in writers { messages.discard(writer); }
        });
    }).unwrap();
}
