//! Two connection drivers exchange loopbacks with bounded dispatch and IO.
use super::{driver::with_peer, portable_capnp::echo, ZERO};
use capntproto_rpc_engine::{
    clients::{self, Capabilities},
    ids::{ImportId, QuestionId, WireId},
    imports, messages,
};
use capntproto_rpc_wire::{
    calls, clients as binding,
    owned::{
        self,
        connection::{Driver, Progress, Received},
        Frame,
    },
    rpc_capnp::{
        self,
        api::{CapDescriptor, Message, Resolve},
    },
};
type Peer<'p, 'id, 's> = Driver<'p, 'id, 's, (), u32, u8, u32, 4, 4>;
fn tick<'p>(
    peer: &mut Peer<'p, '_, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_>,
) {
    match peer.poll_maintenance(
        messages,
        queue,
        776,
        Default::default(),
        &mut [0; 4],
        core::num::NonZeroUsize::new(1).unwrap(),
    ) {
        Ok(Progress::Idle | Progress::Yield | Progress::Advanced) => (),
        Err(
            owned::connection::Error::Dispatch(
                capntproto_rpc_engine::clients::dispatch::DispatchError::Full,
            )
            | owned::connection::Error::Enqueue(owned::EnqueueError::Dispatch(
                capntproto_rpc_engine::clients::dispatch::DispatchError::Full,
            )),
        ) => (),
        Err(error) => panic!("maintenance: {error}"),
        Ok(_) => panic!("unexpected detached owner"),
    }
}
fn input_call<'p>(
    peer: &mut Peer<'p, '_, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    target: u32,
    question: u32,
) {
    let frame = Frame::build(messages, 776, Default::default(), |builder| {
        calls::call(
            builder,
            QuestionId::from_wire(question),
            ImportId::from_wire(target),
            echo::Owned::ECHO,
            |_| Ok(()),
        )
    })
    .unwrap();
    assert!(matches!(
        peer.receive(
            frame,
            messages,
            &mut [0; 4],
            |_| panic!("empty caps"),
            || 43
        )
        .unwrap_or_else(|rejected| panic!("Call: {}", rejected.error)),
        Received::Queued(_) | Received::DeferredCall(_)
    ));
}
fn complete_relay<'p>(
    peer: &mut Peer<'p, '_, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    local: bool,
) -> u32 {
    let mut dispatch = peer.capabilities();
    let ready = dispatch.next_ready().unwrap().unwrap();
    assert!(
        matches!(
            ready.destination(),
            clients::Destination::Endpoint {
                location: clients::Location::Local,
                ..
            }
        ) == local
    );
    let running = ready.accept();
    let call = peer
        .capabilities()
        .complete(running)
        .into_call()
        .unwrap_or_else(|_| panic!("older relay call must precede loopback"));
    let answer = call.answer();
    peer.release_call(call, messages);
    // The fixture models the relay's already accepted Return/Finish here; the
    // loopback frames themselves travel through real framing and faulted IO.
    assert!(peer
        .prepare_answer_return(answer)
        .unwrap()
        .accept()
        .is_none());
    let state = peer.finish_answer(answer.id(), false).retired.unwrap();
    assert_eq!(peer.release_answer(state, messages), 43);
    answer.id().to_wire()
}

pub fn owning_loopback_driver(chunk: usize, reject_output: bool) {
    let mut words = [[ZERO; 97]; 12];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut slots = [None, None];
        let mut queue = super::driver::message_queue(&mut slots).unwrap();
        with_peer(|alice| {
            with_peer(|bob| {
                let service = alice
                    .capabilities()
                    .endpoint(binding::Route::local(41), clients::Location::Local)
                    .unwrap();
                let (mut graph, mut batch) = alice.export_batch();
                let local_export = binding::stage(&mut graph, &mut batch, &service)
                    .unwrap()
                    .id();
                batch.commit();
                let mut incoming = [imports::ReceiveSlot::new(
                    ImportId::from_wire(local_export.to_wire()),
                    (),
                )];
                bob.prepare_imports(&mut incoming).unwrap().commit();
                let reflected = incoming[0].take_local().unwrap();
                let frozen = bob.freeze_import(&reflected).unwrap();
                let (_graph, mut batch) = bob.export_batch();
                let original = batch.export_promise(frozen).unwrap().id();
                batch.commit();
                let mut incoming = [imports::ReceiveSlot::new_promise(
                    ImportId::from_wire(original.to_wire()),
                    (),
                )];
                alice.prepare_imports(&mut incoming).unwrap().commit();
                let receipt = incoming[0].take_local().unwrap();
                let promise = alice.bind_import(&receipt).unwrap();
                // A relay's local routing entry points at this observed promise.
                let retained = alice.capabilities().retain(&promise).unwrap();
                let (_graph, mut batch) = alice.export_batch();
                let relay = batch.export(retained).unwrap().id();
                batch.commit();
                input_call(alice, messages, relay.to_wire(), 71);
                let frame = Frame::build(messages, 776, Default::default(), |builder| {
                    let mut root = builder.try_init_root::<Message>()?;
                    let mut resolve = root.field(Message::RESOLVE).init()?;
                    resolve.scalar(Resolve::PROMISE_ID).set(original.to_wire());
                    resolve
                        .field(Resolve::CAP)
                        .init()?
                        .scalar(CapDescriptor::RECEIVER_HOSTED)
                        .set(local_export.to_wire());
                    Ok(())
                })
                .unwrap();
                assert!(matches!(
                    alice
                        .receive(
                            frame,
                            messages,
                            &mut [0; 4],
                            |_| panic!("reflection"),
                            || panic!("control")
                        )
                        .unwrap_or_else(|rejected| panic!("Resolve: {}", rejected.error)),
                    Received::Resolved
                ));
                assert!(matches!(
                    alice.runtime().inspect(&promise).unwrap(),
                    clients::Destination::Waiting(_)
                ));
                // The ordinary dispatch slot still owns an older call. A maintenance
                // pass cannot send a loopback ahead of it.
                for _ in 0..if reject_output { 0 } else { 100 } {
                    tick(alice, messages, &mut queue);
                }
                assert!(queue.is_empty());
                complete_relay(alice, messages, false);
                // A new call may arrive before the sender loopback is emitted.
                // It cannot consume the control capacity needed to unblock it.
                input_call(alice, messages, relay.to_wire(), 72);
                input_call(alice, messages, relay.to_wire(), 73);
                if reject_output {
                    // Leave queue capacity while exhausting send ownership. The
                    // loopback must retain its task and affine barrier on rejection.
                    let writer = messages.reserve(8).unwrap();
                    messages.write(&writer)[..8].fill(0);
                    let lease = messages.seal(writer, 8).unwrap();
                    let offer = messages.offer(&lease).unwrap();
                    queue.reserve().unwrap().accept(messages, offer);
                    messages.release(lease);
                    // Exhausted send ownership parks the exact loopback; no
                    // repeated errors or retry scan are needed before release.
                    let mut slept = false;
                    for _ in 0..150 {
                        tick(alice, messages, &mut queue);
                        slept |= alice.maintenance_is_idle(messages, &queue);
                    }
                    assert!(slept, "blocked loopback must eventually sleep");
                    assert!(matches!(
                        alice.runtime().inspect(&promise).unwrap(),
                        clients::Destination::Waiting(_)
                    ));
                    let window = queue.window(messages).unwrap().unwrap();
                    let length = window.bytes().len();
                    assert!(window.advance(length).unwrap());
                }
                for _ in 0..150 {
                    if !queue.is_empty() {
                        break;
                    }
                    tick(alice, messages, &mut queue);
                }
                assert!(!queue.is_empty());
                let frame = super::owning::exchange(messages, &mut queue, chunk);
                let id = frame
                    .with(messages, |root| {
                        let rpc_capnp::message::Disembargo(control) = root.which().unwrap() else {
                            panic!("loopback")
                        };
                        let rpc_capnp::disembargo::context::SenderLoopback(id) =
                            control.unwrap().get_context().which().unwrap()
                        else {
                            panic!("sender")
                        };
                        id
                    })
                    .unwrap();
                if reject_output {
                    assert!(id > 0, "rejected output IDs are never reused");
                }
                // Bob also has an older relay call occupying its only task slot.
                input_call(bob, messages, original.to_wire(), 81);
                assert!(matches!(
                    bob.receive(
                        frame,
                        messages,
                        &mut [0; 4],
                        |_| panic!("control"),
                        || panic!("control")
                    )
                    .unwrap_or_else(|rejected| panic!("sender loopback: {}", rejected.error)),
                    Received::Loopback
                ));
                let newer = Frame::build(messages, 776, Default::default(), |builder| {
                    calls::call(
                        builder,
                        QuestionId::from_wire(82),
                        ImportId::from_wire(original.to_wire()),
                        echo::Owned::ECHO,
                        |_| Ok(()),
                    )
                })
                .unwrap();
                let rejected = bob
                    .receive(
                        newer,
                        messages,
                        &mut [0; 4],
                        |_| panic!("control"),
                        || panic!("cannot overtake echo"),
                    )
                    .err()
                    .unwrap();
                assert_eq!(rejected.error, owned::connection::Error::Busy);
                rejected.frame.release(messages);
                for _ in 0..100 {
                    tick(bob, messages, &mut queue);
                }
                assert!(queue.is_empty());
                complete_relay(bob, messages, false);
                // A later Alice call remains blocked on the resolved local route.
                assert!(alice.capabilities().next_ready().unwrap().is_none());
                for _ in 0..150 {
                    if !queue.is_empty() {
                        break;
                    }
                    tick(bob, messages, &mut queue);
                }
                assert!(!queue.is_empty());
                let frame = super::owning::exchange(messages, &mut queue, chunk);
                assert!(matches!(
                    alice
                        .receive(
                            frame,
                            messages,
                            &mut [0; 4],
                            |_| panic!("control"),
                            || panic!("control")
                        )
                        .unwrap_or_else(|rejected| panic!("echo: {}", rejected.error)),
                    Received::Loopback
                ));
                for _ in 0..150 {
                    if alice.capabilities().next_ready().unwrap().is_some() {
                        break;
                    }
                    tick(alice, messages, &mut queue);
                }
                assert_eq!(complete_relay(alice, messages, true), 72);
                for _ in 0..150 {
                    if alice.capabilities().next_ready().unwrap().is_some() {
                        break;
                    }
                    tick(alice, messages, &mut queue);
                }
                assert_eq!(complete_relay(alice, messages, true), 73);
                let duplicate = Frame::build(messages, 776, Default::default(), |builder| {
                    calls::receiver_loopback(
                        builder,
                        id,
                        calls::Target::Imported(ImportId::from_wire(local_export.to_wire())),
                    )
                })
                .unwrap();
                let rejected = alice
                    .receive(
                        duplicate,
                        messages,
                        &mut [0; 4],
                        |_| panic!("control"),
                        || panic!("control"),
                    )
                    .err()
                    .unwrap();
                assert!(matches!(
                    rejected.error,
                    owned::connection::Error::Loopback(_)
                ));
                rejected.frame.release(messages);
                alice.capabilities().release(promise);
                alice.capabilities().release(service);
                alice.release_import(receipt);
                bob.release_import(reflected);
                for export in [relay, local_export] {
                    let peer = &mut *alice;
                    if let Some(client) = peer.release_export(export, 1).unwrap() {
                        peer.capabilities().release(client);
                    }
                }
                if let Some(client) = bob.release_export(original, 1).unwrap() {
                    bob.capabilities().release(client);
                }
                while alice.drain_endpoint().is_some() {}
                while bob.drain_endpoint().is_some() {}
                assert!(alice.runtime().is_empty() && bob.runtime().is_empty());
                assert!(alice.ledgers().exports.is_empty() && bob.ledgers().exports.is_empty());
                assert!(alice.ledgers().answers.is_empty() && bob.ledgers().answers.is_empty());
                // All frame ownership is reclaimed independently of pending peer
                // Release controls, which the fixture's terminal teardown discards.
                let writers = core::array::from_fn::<_, 12, _>(|_| messages.reserve(776).unwrap());
                for writer in writers {
                    messages.discard(writer);
                }
            })
        });
    })
    .unwrap();
}

/// A loopback targeting an unresolved answer must keep that exact answer alive
/// across Finish, and wait for its published reflected route before echoing.
pub fn owning_loopback_unresolved(chunk: usize) {
    use capntproto_rpc_wire::{
        pipeline,
        publication::reflected::Reference,
        rpc_capnp::api::{Payload, Return},
    };
    let mut words = [[ZERO; 97]; 6];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut queue_slots = [None];
        let mut queue = super::driver::message_queue(&mut queue_slots).unwrap();
        with_peer(|peer| {
            let frame = Frame::build(messages, 776, Default::default(), |builder| {
                calls::bootstrap(builder, QuestionId::from_wire(7))
            })
            .unwrap();
            let Received::Bootstrap(key) = peer
                .receive(frame, messages, &mut [0; 4], |_| (), || 43)
                .unwrap_or_else(|_| panic!("bootstrap"))
            else {
                panic!("bootstrap")
            };
            let key = key.answer();
            let mut incoming = [imports::ReceiveSlot::new(ImportId::from_wire(17), ())];
            peer.prepare_imports(&mut incoming).unwrap().commit();
            let remote = incoming[0].take_local().unwrap();
            let frame = Frame::build(messages, 776, Default::default(), |builder| {
                calls::sender_loopback(
                    builder,
                    capntproto_rpc_engine::ids::EmbargoId::from_wire(99),
                    calls::Target::Promised {
                        question: QuestionId::from_wire(7),
                        path: pipeline::Path::new(&[]),
                    },
                )
            })
            .unwrap();
            assert!(matches!(
                peer.receive(
                    frame,
                    messages,
                    &mut [0; 4],
                    |_| panic!("control"),
                    || panic!("control")
                )
                .unwrap_or_else(|_| panic!("loopback")),
                Received::Loopback
            ));
            for _ in 0..100 {
                tick(peer, messages, &mut queue);
            }
            assert!(queue.is_empty());
            assert!(peer.capabilities().next_ready().unwrap().is_none());
            let frame = Frame::build(messages, 776, Default::default(), |builder| {
                calls::finish(builder, QuestionId::from_wire(7), false)
            })
            .unwrap();
            assert!(matches!(
                peer.receive(
                    frame,
                    messages,
                    &mut [0; 4],
                    |_| panic!("control"),
                    || panic!("control")
                )
                .unwrap_or_else(|_| panic!("Finish")),
                Received::Finished(None)
            ));
            assert!(
                peer.answers().dispatch(key, true).unwrap(),
                "loopback retains producer through Finish"
            );
            let frame = Frame::build(messages, 776, Default::default(), |builder| {
                let mut root = builder.try_init_root::<Message>()?;
                let mut returned = root.field(Message::RETURN).init()?;
                returned.scalar(Return::ANSWER_ID).set(7);
                returned.scalar(Return::RELEASE_PARAM_CAPS).set(false);
                let mut payload = returned.field(Return::RESULTS).init()?;
                payload.field(Payload::CONTENT).set_capability_index(0)?;
                payload
                    .field(Payload::CAP_TABLE)
                    .init(1)?
                    .get_mut(0)
                    .unwrap()
                    .scalar(CapDescriptor::RECEIVER_HOSTED)
                    .set(17);
                Ok(())
            })
            .unwrap();
            let mut refs = [Reference::Hosted(peer.retain_import(&remote).unwrap())];
            let published = peer
                .snapshot_reflected_results(&frame, messages, &mut refs, &mut [0; 4])
                .unwrap();
            // Finish cancels the peer's result, but not its already admitted
            // loopback dependency. Retain results locally and send Canceled.
            frame.release(messages);
            let Reference::Hosted(retained) = core::mem::replace(&mut refs[0], Reference::None)
            else {
                unreachable!()
            };
            peer.release_import(retained);
            let canceled = Frame::build(messages, 776, Default::default(), |builder| {
                calls::cancelled(builder, key.id())
            })
            .unwrap();
            assert!(peer
                .publish_answer_frame(key, messages, canceled, queue.reserve().unwrap())
                .unwrap()
                .is_none());
            peer.publish_answer_pipeline(key, Ok(published))
                .unwrap_or_else(|_| panic!("publish"));
            let returned = super::owning::exchange(messages, &mut queue, chunk);
            returned.release(messages);
            for _ in 0..150 {
                if !queue.is_empty() {
                    break;
                }
                tick(peer, messages, &mut queue);
            }
            assert!(!queue.is_empty());
            let frame = super::owning::exchange(messages, &mut queue, chunk);
            frame
                .with(messages, |root| {
                    let rpc_capnp::message::Disembargo(control) = root.which().unwrap() else {
                        panic!("echo")
                    };
                    let control = control.unwrap();
                    assert!(matches!(
                        control.get_context().which().unwrap(),
                        rpc_capnp::disembargo::context::ReceiverLoopback(99)
                    ));
                    assert!(matches!(
                        control.get_target().unwrap().which().unwrap(),
                        rpc_capnp::message_target::ImportedCap(17)
                    ));
                })
                .unwrap();
            frame.release(messages);
            assert_eq!(peer.drain_answer(0, messages), Some(43));
            peer.release_import(remote);
            while peer.drain_endpoint().is_some() {}
            assert!(peer.runtime().is_empty() && peer.ledgers().answers.is_empty());
            let writers = core::array::from_fn::<_, 6, _>(|_| messages.reserve(776).unwrap());
            for writer in writers {
                messages.discard(writer);
            }
        });
    })
    .unwrap();
}
