//! Two independent connections rendezvous through caller-authenticated metadata.
use super::{owning::exchange, ZERO};
use capntproto_rpc_engine::{
    answers, connection, exports,
    ids::{ImportId, WireId},
    imports, messages, provision, questions,
};
use capntproto_rpc_wire::{
    calls, network,
    owned::{
        self, answer, controls,
        provision::{Accept, Provide},
        Frame, TargetRef,
    },
    pipeline::Path,
    reception,
};

type Host<'p, 'id, 's> = connection::Connection<'id, 's, (), u8, answer::State<'p, u8>>;

pub fn bounded_provision_exchange(release_first: bool) {
    let mut words = [[ZERO; 97]; 8];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut queue_slots = [None];
        let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
        let mut pq = [questions::Slot::EMPTY];
        questions::with_questions(&mut pq, |provider| {
            let mut he = [exports::Slot::EMPTY];
            let mut ha = [answers::Slot::EMPTY];
            connection::with_connection(connection::Storage {imports:&mut [], exports:&mut he, answers:&mut ha, questions:&mut []}, |host: &mut Host<'_, '_, '_>| {
                let mut batch = host.exports.batch();
                let grant = batch.export(41).unwrap();
                batch.commit();
                let pending = provider.reserve_provision().unwrap();
                let frame = Frame::build(messages, 776, Default::default(), |builder| network::provide(builder, pending.id(), calls::Target::Imported(ImportId::from_wire(grant.id().to_wire())), |recipient| recipient.cast::<capnp::field_api::Data>().copy_from(b"authorized-peer"))).unwrap();
                let vine = owned::provision::send(pending, messages, frame, queue.reserve().unwrap()).unwrap();
                let incoming = exchange(messages, &mut queue, 1);
                let provide = Provide::<1>::admit(incoming, messages, &mut host.exports, &mut host.answers, answer::State::new(47)).unwrap_or_else(|e|panic!("{}", e.error));
                assert_eq!(host.exports.release(grant.id(), 1), Ok(None));
                let TargetRef::Hosted(target) = provide.target() else {panic!("hosted")};
                let authority = *host.exports.get(target);
                let mut ci = [imports::Slot::EMPTY];
                let mut cq = [questions::Slot::EMPTY];
                connection::with_connection(connection::Storage {imports:&mut ci, exports:&mut [], answers:&mut [], questions:&mut cq}, |recipient: &mut connection::Connection<'_, '_, u8, (), ()>| {
                    let mut accepted_exports = [exports::Slot::EMPTY];
                    let mut accepted_answers = [answers::Slot::EMPTY];
                    connection::with_connection(connection::Storage {imports:&mut [], exports:&mut accepted_exports, answers:&mut accepted_answers, questions:&mut []}, |accepting: &mut Host<'_, '_, '_>| {
                        let mut embargo_slots = [const {provision::Slot::<0>::EMPTY}; 1];
                        provision::with_embargoes(&mut embargo_slots, |embargoes| {
                            if release_first {embargoes.release(b"").unwrap();}
                            let pending = recipient.questions.reserve().unwrap();
                            let frame = Frame::build(messages, 776, Default::default(), |builder| network::accept(builder, pending.id(), Some(b""), |completion| completion.cast::<capnp::field_api::Data>().copy_from(b"authorized-peer"))).unwrap();
                            let question = owned::Question::send(pending, messages, frame, queue.reserve().unwrap()).unwrap();
                            let incoming = exchange(messages, &mut queue, 1);
                            let accept = Accept::admit(incoming, messages, &mut accepting.answers, answer::State::new(53)).unwrap_or_else(|e|panic!("{}", e.error));
                            let claim = accept.with(messages, |request| {
                                // Fixture authentication stands in for the network's verified
                                // peer/completion credentials; matching an arbitrary ID alone
                                // is deliberately not implemented by the protocol engine.
                                provide.with_recipient(messages, |recipient| assert_eq!(recipient.get_as::<capnp::data::Reader<'_>>().unwrap(), request.completion.get_as::<capnp::data::Reader<'_>>().unwrap())).unwrap();
                                embargoes.claim(request.embargo.expect("empty embargo is present")).unwrap()
                            }).unwrap();
                            let pipeline = accepting.answers.pipeline(accept.key().id()).unwrap();
                            if !release_first {
                                let rejected = embargoes.complete(claim).unwrap_err();
                                assert_eq!(rejected.error, provision::Error::Pending);
                                embargoes.release(b"").unwrap();
                                embargoes.complete(rejected.claim).unwrap();
                            } else {embargoes.complete(claim).unwrap();}
                            assert_eq!(embargoes.claim(b"").unwrap_err(), provision::Error::DuplicateAccept);
                            let key = accept.key();
                            accept.release(messages);
                            let mut batch = accepting.exports.batch();
                            let exported = batch.export(authority).unwrap();
                            let frame = Frame::build(messages, 776, Default::default(), |builder| network::accepted(builder, key.id(), &exported)).unwrap();
                            assert!(answer::try_send_granting(key, &mut accepting.answers, batch, messages, frame, &mut [0], queue.reserve().unwrap()).unwrap_or_else(|e|panic!("{}", e.error)).is_none());
                            let incoming = exchange(messages, &mut queue, 1);
                            let response: owned::Response<'_, '_, '_, '_, '_, u8, 1> = question.returned(incoming, messages, &mut recipient.questions, &mut recipient.imports, &mut recipient.exports, &mut recipient.answers, &mut [], |_|71).unwrap_or_else(|e|panic!("{}", e.error));
                            response.with(messages, |reply| {
                                let owned::Reply::Results(payload) = reply else {panic!("results")};
                                let reception::full::Capability::Remote(local) = payload.pipeline(Path::new(&[])).unwrap() else {panic!("remote")};
                                assert_eq!(recipient.imports.wire_id(local).to_wire(), exported.id().to_wire());
                            }).unwrap();
                            response.release(messages, &mut recipient.questions, &mut recipient.imports, &mut recipient.exports, &mut recipient.answers);
                            assert!(controls::finish_one(&mut recipient.questions, messages, &mut queue, 776, Default::default()).unwrap());
                            let finished = exchange(messages, &mut queue, 1);
                            assert!(answer::finished(&finished, messages, &mut accepting.answers, &mut accepting.exports, &mut [0]).unwrap().unwrap().retired.is_none());
                            finished.release(messages);
                            assert_eq!(accepting.answers.release_pipeline(pipeline).unwrap().release(messages), 53);
                            assert!(controls::release_one(&mut recipient.imports, messages, &mut queue, 776, Default::default()).unwrap().is_some());
                            let release = exchange(messages, &mut queue, 1);
                            release.with(messages, |root| {
                                let capntproto_rpc_wire::rpc_capnp::message::Release(release) = root.which().unwrap() else {panic!("Release")};
                                let release = release.unwrap();
                                assert_eq!(accepting.exports.release(capntproto_rpc_engine::ids::ExportId::from_wire(release.get_id()), release.get_reference_count()), Ok(Some(41)));
                            }).unwrap();
                            release.release(messages);
                            assert!(recipient.questions.is_empty() && recipient.imports.is_empty());
                            assert!(accepting.answers.is_empty() && accepting.exports.is_empty());
                            embargoes.close();
                        }).unwrap();
                    }).unwrap();
                }).unwrap();
                provider.release_provision(vine);
                assert!(controls::finish_one(provider, messages, &mut queue, 776, Default::default()).unwrap());
                let finish = exchange(messages, &mut queue, 1);
                assert_eq!(answer::finished(&finish, messages, &mut host.answers, &mut host.exports, &mut [0]).unwrap().unwrap().retired.unwrap().release(messages), 47);
                finish.release(messages);
                provide.release(messages, &mut host.exports, &mut host.answers);
                assert_eq!(host.exports.take_retired(0), Some(41));
                assert!(provider.is_empty() && host.exports.is_empty() && host.answers.is_empty());
            }).unwrap();
        }).unwrap();
        let free: [_; 8] = core::array::from_fn(|_|messages.reserve(776).unwrap());
        for writer in free {messages.discard(writer);}
    }).unwrap();
}
