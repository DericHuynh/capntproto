//! Three vats, three physical links, six independent branded connection ledgers.
//! The caller-owned network matches immutable metadata before bridging results.
use super::{
    driver::{with_owned_peer, with_owned_peer_storage},
    driver_invocation::close,
    owning::exchange,
    portable_capnp::{api::echo::EchoResults, echo},
    ZERO,
};
use capntproto_rpc_engine::clients::Capabilities as _;
use capntproto_rpc_engine::{
    clients::{self},
    ids::{ImportId, WireId},
    messages,
};
use capntproto_rpc_wire::{
    calls, clients as binding, network,
    owned::{
        connection::{CallOutput, Driver, NetworkRequest, Received, Request},
        Frame, Reply,
    },
};
type Peer<'p, 'id, 's> = Driver<'p, 'id, 's, (), u32, u8, u32, 4, 4>;
fn service<'p, 'id>(peer: &mut Peer<'p, 'id, '_>, value: u32) -> ImportId {
    let client = peer
        .capabilities()
        .endpoint(binding::Route::local(value), clients::Location::Local)
        .unwrap();
    let (mut graph, mut batch) = peer.export_batch();
    let grant = binding::stage(&mut graph, &mut batch, &client).unwrap();
    let id = ImportId::from_wire(grant.id().to_wire());
    batch.commit();
    peer.capabilities().release(client);
    id
}
fn receive_call<'p, 'id>(
    peer: &mut Peer<'p, 'id, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_>,
) -> Request<'p, 'id, (), 4, 4> {
    let frame = exchange(messages, queue, 1);
    let Received::Queued(key) = peer
        .receive(frame, messages, &mut [0; 4], |_| (), || 43)
        .unwrap_or_else(|r| panic!("{}", r.error))
    else {
        panic!("call")
    };
    let mut dispatch = peer.capabilities();
    let running = dispatch.next_ready().unwrap().unwrap().accept();
    peer.answers().dispatch(key, true).unwrap();
    peer.capabilities()
        .complete(running)
        .into_call()
        .unwrap_or_else(|_| panic!("call owner"))
}
fn finish<'p>(
    sender: &mut Peer<'p, '_, '_>,
    receiver: &mut Peer<'p, '_, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_>,
) {
    assert!(sender
        .send_pending_finish(messages, queue, 776, Default::default())
        .unwrap());
    let frame = exchange(messages, queue, 1);
    assert!(matches!(
        receiver
            .receive(frame, messages, &mut [0; 4], |_| (), || panic!("Finish"))
            .unwrap_or_else(|r| panic!("{}", r.error)),
        Received::Finished(Some(43))
    ));
}
fn send_await<'p, 'id>(
    sender: &mut Peer<'p, 'id, '_>,
    answer: capntproto_rpc_engine::answers::Key<'id>,
    receiver: &mut Peer<'p, '_, '_>,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_>,
) {
    let frame = Frame::build(messages, 776, Default::default(), |b| {
        network::await_from_third_party(b, answer.id(), |token| {
            token
                .cast::<capnp::field_api::Data>()
                .copy_from(b"authenticated three-vat call")
        })
    })
    .unwrap();
    assert!(sender
        .try_publish_answer_frame(answer, messages, frame, queue.reserve().unwrap())
        .unwrap_or_else(|r| panic!("{}", r.error))
        .is_none());
    let frame = exchange(messages, queue, 1);
    assert!(matches!(
        receiver
            .receive(frame, messages, &mut [0; 4], |_| (), || panic!("Await"))
            .unwrap_or_else(|r| panic!("{}", r.error)),
        Received::Network
    ));
}
/// Complete metadata/Return may arrive on the direct link before the original
/// introducer's Await. Neither ordering changes ownership or authenticates early.
pub fn owning_network_rendezvous(direct_first: bool) {
    let mut words = [[ZERO; 97]; 24];
    let mut buffers = words
        .each_mut()
        .map(|w| messages::Buffer::new(capnp::Word::words_to_bytes_mut(w)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut output = [None]; let mut queue = super::driver::message_queue(&mut output).unwrap();
        with_owned_peer(|mut alice_bob| with_owned_peer(|mut bob_alice| with_owned_peer(|mut bob_carol| with_owned_peer(|mut carol_bob| with_owned_peer_storage::<2, 1, _>(|mut alice_carol| with_owned_peer(|mut carol_alice| {
            let bob = service(&mut bob_alice, 41);
            let carol = service(&mut carol_bob, 47);
            let original = alice_bob.send(messages, &mut queue, 776, Default::default(), |b, id| {
                calls::call(b, id, bob, echo::Owned::ECHO, |_| Ok(()))?;
                network::allow_third_party_tail_call(b)
            }).unwrap();
            let mut contact_words = [ZERO; 12];
            let mut contact = capnp::message::FixedBuilder::new(&mut contact_words).unwrap();
            contact.try_set_root_data(b"authenticated three-vat call").unwrap();
            let target = super::driver_invocation::remote(&mut bob_carol, carol);
            let task = bob_carol.enqueue_dynamic_to(&target, calls::DynamicCall {
                interface: echo::Owned::ECHO.interface_id(), method: echo::Owned::ECHO.ordinal(),
                no_promise_pipelining: false, only_promise_pipeline: false,
            }, &[], messages, 776, Default::default(), Some(contact.get_root_as_reader().unwrap()), |_| Ok(())).unwrap();
            bob_carol.capabilities().release(target);
            assert!(bob_carol.send_invocation(messages, &mut queue, 776, Default::default(), &mut [0;4]).is_err(), "full output retains redirected invocation");
            assert_eq!(bob_carol.capabilities().next_ready().unwrap().unwrap().owner().invocation_key().unwrap(), task);
            assert!(bob_carol.ledgers().questions.is_empty());
            let original_call = receive_call(&mut bob_alice, messages, &mut queue);
            let rejected = bob_carol.send_redirected(messages, &mut queue, 776, Default::default(), |b, id| {
                calls::call(b, id, carol, echo::Owned::ECHO, |_| Ok(()))?;
                network::send_results_to_third_party(b, |_| Ok(()))
            });
            assert!(rejected.is_err(), "third-party question cannot be published as a local tail source");
            assert!(queue.is_empty());
            assert!(bob_carol.ledgers().questions.is_empty());
            let sent = bob_carol.send_invocation(messages, &mut queue, 776, Default::default(), &mut [0;4]).unwrap().unwrap();
            assert_eq!(sent.task, task);
            let forwarded = sent.question;
            let forwarded_call = receive_call(&mut carol_bob, messages, &mut queue);
            forwarded_call.with(messages, |request| assert_eq!(request.incoming().third_party_destination().unwrap().unwrap().get_as::<capnp::data::Reader<'_>>().unwrap(), b"authenticated three-vat call")).unwrap();
            assert!(carol_bob.prepare_answer_adoption(forwarded_call.answer().id()).is_err(), "third-party results cannot be stolen by local adoption");
            // Old-link pipeline state is completed only after its own Return commits.
            let premature = carol_bob.complete_network_answer(&forwarded_call, Err(capnp::ErrorKind::Failed), messages).err().unwrap();
            assert_eq!(premature.error, capntproto_rpc_wire::owned::connection::Error::InvalidNetworkTarget);
            if !direct_first { send_await(&mut bob_alice, original_call.answer(), &mut alice_bob, messages, &mut queue); }
            let adopted = carol_alice.send_adopted_answer(messages, &mut queue, 776, Default::default(), 43, |token| token.cast::<capnp::field_api::Data>().copy_from(b"authenticated three-vat call")).unwrap_or_else(|r| panic!("adopted answer: {}", r.error));
            let frame = exchange(messages, &mut queue, 1);
            alice_carol.receive(frame, messages, &mut [0; 4], |_| (), || panic!("Adopt")).unwrap_or_else(|r| panic!("{}",r.error));
            let NetworkRequest::Adopt(adoption) = alice_carol.take_network().unwrap() else { panic!("adoption") };
            let direct_question = adoption.key();
            let frame = Frame::build(messages, 776, Default::default(), |b| calls::results(b, adopted.id(), echo::Owned::ECHO, |mut r| { r.scalar(EchoResults::VALUE).set(53); Ok(()) })).unwrap();
            assert!(carol_alice.try_publish_answer_frame(adopted, messages, frame, queue.reserve().unwrap()).unwrap_or_else(|r| panic!("{}",r.error)).is_none());
            let frame = exchange(messages, &mut queue, 1);
            alice_carol.receive(frame, messages, &mut [0; 4], |_| (), || panic!("Return")).unwrap_or_else(|r| panic!("{}",r.error));
            assert_eq!(alice_carol.take_response(direct_question).err(), Some(capntproto_rpc_wire::owned::connection::Error::PendingAuthorization), "direct result cannot authenticate itself");
            if direct_first { send_await(&mut bob_alice, original_call.answer(), &mut alice_bob, messages, &mut queue); }
            let NetworkRequest::Await(awaiting) = alice_bob.take_network().unwrap() else { panic!("Await") };
            // The network compares both immutable envelopes while each still owns
            // its independently branded original generation.
            let matched = awaiting.with_metadata(messages, |expected| adoption.with_completion(messages, |actual| expected.get_as::<capnp::data::Reader<'_>>().unwrap() == actual.get_as::<capnp::data::Reader<'_>>().unwrap())).unwrap().unwrap();
            assert!(matched);
            alice_carol.authorize_adoption(adoption, messages, |_| if matched { Ok(()) } else { Err(()) }).unwrap_or_else(|_| panic!("authorize"));
            let response = alice_carol.take_response(direct_question).unwrap();
            let value = response.with(messages, |reply| {
                let Reply::Results(payload) = reply else { panic!("direct results") };
                echo::Owned::ECHO.read_results(payload.content()).unwrap().value()
            }).unwrap();
            let bridge = alice_bob.local_result(echo::Owned::ECHO, &[], messages, 776, Default::default(), |mut r| { r.scalar(EchoResults::VALUE).set(value); Ok(()) }).unwrap();
            alice_bob.complete_await(awaiting, Ok(bridge), messages, |_| if matched { Ok(()) } else { Err(()) }).unwrap_or_else(|_| panic!("complete original"));
            alice_carol.release_response(response, messages);
            finish(&mut alice_carol, &mut carol_alice, messages, &mut queue);
            // Both old links preserve their own protocol lifetime and pipelines.
            let frame = Frame::build(messages, 776, Default::default(), |b| calls::results_sent_elsewhere(b, forwarded_call.answer().id())).unwrap();
            assert!(carol_bob.try_publish_answer_frame(forwarded_call.answer(), messages, frame, queue.reserve().unwrap()).unwrap_or_else(|r| panic!("{}",r.error)).is_none());
            let frame = exchange(messages, &mut queue, 1);
            bob_carol.receive(frame, messages, &mut [0; 4], |_| (), || panic!("Elsewhere")).unwrap_or_else(|r| panic!("{}",r.error));
            let response = bob_carol.take_response(forwarded).unwrap();
            response.with(messages, |reply| assert!(matches!(reply, Reply::ResultsSentElsewhere))).unwrap();
            bob_carol.release_response(response, messages);
            { let (peer, call) = (&mut carol_bob, &forwarded_call);
                let value = peer.local_result(echo::Owned::ECHO, &[], messages, 776, Default::default(), |mut r| { r.scalar(EchoResults::VALUE).set(53); Ok(()) }).unwrap();
                peer.complete_network_answer(call, Ok(value), messages).unwrap_or_else(|r| panic!("{}", r.error));
            }
            let value = bob_alice.local_result(echo::Owned::ECHO, &[], messages, 776, Default::default(), |mut r| { r.scalar(EchoResults::VALUE).set(53); Ok(()) }).unwrap();
            bob_alice.complete_network_answer(&original_call, Ok(value), messages).unwrap_or_else(|r| panic!("{}",r.error));
            let duplicate = bob_alice.complete_network_answer(&original_call, Err(capnp::ErrorKind::Failed), messages).err().unwrap();
            assert!(matches!(duplicate.error, capntproto_rpc_wire::owned::connection::Error::Pipeline(_)));
            carol_bob.release_call(forwarded_call, messages);
            bob_alice.release_call(original_call, messages);
            finish(&mut bob_carol, &mut carol_bob, messages, &mut queue);
            let output = alice_bob.take_output(original, messages).unwrap().unwrap();
            let CallOutput::Local(result) = &output else { panic!("bridged original response") };
            result.with(messages, |payload| assert_eq!(payload.results(echo::Owned::ECHO).unwrap().value(), 53)).unwrap();
            alice_bob.release_output(output, messages);
            finish(&mut alice_bob, &mut bob_alice, messages, &mut queue);
            assert_eq!(close(alice_bob, messages, &mut queue), 0);
            assert_eq!(close(bob_alice, messages, &mut queue), 41);
            assert_eq!(close(bob_carol, messages, &mut queue), 0);
            assert_eq!(close(carol_bob, messages, &mut queue), 47);
            assert_eq!(close(alice_carol, messages, &mut queue), 0);
            assert_eq!(close(carol_alice, messages, &mut queue), 0);
        }))))));
        let free: [_; 24] = core::array::from_fn(|_| messages.reserve(776).unwrap());
        for w in free { messages.discard(w); }
    }).unwrap();
}
