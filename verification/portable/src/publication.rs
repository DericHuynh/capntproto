use super::*;

/// A real Return capability table binds export grants to a resumable send.
/// The receiving bounded import table releases exactly the decoded grants.
pub fn bounded_rpc_publication() -> Result<()> {
    use capntproto_rpc_engine::{
        exports,
        ids::{ImportId, WireId},
        imports, messages,
    };
    use capntproto_rpc_wire::{
        publication,
        rpc_capnp::{
            self,
            api::{CapDescriptor, Message, Payload, Return},
        },
    };
    let mut exports = [const { exports::Slot::EMPTY }; 1];
    let mut imports = [const { imports::Slot::EMPTY }; 1];
    let mut source = [ZERO; 32];
    let mut output = [ZERO; 33];
    let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut output))];
    let mut sends = [messages::SendSlot::EMPTY];
    exports::with_exports(&mut exports, |exports| {
        imports::with_imports(&mut imports, |imports| {
            messages::with_messages(&mut buffers, &mut sends, |messages| -> Result<()> {
                let mut batch = exports.batch();
                let grant = batch.export(31).unwrap();
                let _ = batch.grant(grant.key()).unwrap();
                let mut builder = message::FixedBuilder::new(&mut source)?;
                let mut root = builder.try_init_root::<Message>()?;
                let mut result = root.field(Message::RETURN).init()?;
                result.scalar(Return::ANSWER_ID).set(17);
                let mut payload = result.field(Return::RESULTS).init()?;
                let mut table = payload.field(Payload::CAP_TABLE).init(2)?;
                for index in 0..2 {
                    table
                        .get_mut(index)
                        .unwrap()
                        .scalar(CapDescriptor::SENDER_HOSTED)
                        .set(grant.id().to_wire());
                }
                let writer = messages.reserve(33 * 8).unwrap();
                let mut view = messages.write(&writer);
                let mut bytes = &mut *view;
                capnp::serialize::write_message_segments(
                    &mut bytes,
                    &builder.get_segments_for_output(),
                )?;
                let used = 33 * 8 - bytes.len();
                drop(view);
                let lease = messages.seal(writer, used).unwrap();
                let prepared =
                    publication::prepare(batch, messages, lease, &mut [0], Default::default())
                        .unwrap();
                let send = prepared.accept();
                let mut input = [ZERO; 33];
                let mut receiver = capnp::serialize::FrameReceiver::new(
                    Word::words_to_bytes_mut(&mut input),
                    Default::default(),
                )?;
                for position in 0..used {
                    let window = messages.send_window(send).unwrap();
                    let mut receive = receiver.input()?.unwrap();
                    receive.bytes()[0] = window.bytes()[0];
                    receive.commit(1)?;
                    assert_eq!(window.advance(1), Ok(position + 1 == used));
                }
                let decoded = receiver.message()?.unwrap();
                let rpc_capnp::message::Return(result) = decoded
                    .get_root::<rpc_capnp::message::Reader<'_>>()?
                    .which()
                    .unwrap()
                else {
                    panic!("expected Return")
                };
                let result = result?;
                assert_eq!(result.get_answer_id(), 17);
                let rpc_capnp::return_::Results(payload) = result.which().unwrap() else {
                    panic!("expected results")
                };
                let table = payload?.get_cap_table()?;
                assert_eq!(table.len(), 2);
                let mut locals = [None, None];
                for (index, descriptor) in table.into_iter().enumerate() {
                    let rpc_capnp::cap_descriptor::SenderHosted(id) = descriptor.which().unwrap()
                    else {
                        panic!("expected senderHosted")
                    };
                    let id = ImportId::from_wire(id);
                    locals[index] = Some(if index == 0 {
                        imports.admit(id, 41).unwrap()
                    } else {
                        imports.receive(id).unwrap()
                    });
                }
                let call = exports.lease(grant.id()).unwrap();
                for local in locals {
                    imports.release(local.unwrap());
                }
                let release = imports.pending_release(0).unwrap();
                assert_eq!(release.count(), 2);
                assert_eq!(exports.release(grant.id(), release.count()), Ok(None));
                assert_eq!(release.commit(), 41);
                assert_eq!(exports.get(&call), &31);
                assert_eq!(exports.release_local(call), Some(31));
                let writer = messages.reserve(33 * 8).unwrap();
                messages.discard(writer);
                Ok(())
            })
            .unwrap()
        })
        .unwrap()
    })
    .unwrap()
}
