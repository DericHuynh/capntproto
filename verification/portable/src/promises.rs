use super::*;
use capntproto_rpc_engine::{
    exports,
    ids::{ImportId, WireId},
    imports, messages,
};
use capntproto_rpc_wire::{
    inbound, publication, reception,
    rpc_capnp::{
        self,
        api::{CapDescriptor, Message, Payload, Resolve, Return},
    },
};

/// Exchange senderPromise and Resolve over caller-polled, fault-injected IO.
/// Rejected output retains the original obligation; an abandoned receiver must
/// release the replacement even when its original promise is already absent.
pub fn bounded_promise_resolution(chunk: usize, abandoned: bool) -> Result<()> {
    let mut export_slots = [const { exports::Slot::EMPTY }; 2];
    let mut import_slots = [const { imports::Slot::EMPTY }; 2];
    let mut output = [ZERO; 65];
    let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut output))];
    let mut sends = [messages::SendSlot::EMPTY];
    exports::with_exports(&mut export_slots, |exports| {
        imports::with_imports(&mut import_slots, |imports| {
            messages::with_messages(&mut buffers, &mut sends, |messages| -> Result<()> {
                let mut queue_slots = [None];
                let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
                let mut source = [ZERO; 64];
                let mut builder = message::FixedBuilder::new(&mut source)?;
                let mut batch = exports.batch();
                let promise = batch.export_promise(41).unwrap();
                assert!(promise.is_promise());
                let promise_key = promise.key();
                let promise_id = promise.id();
                let import_id = ImportId::from_wire(promise_id.to_wire());
                let mut root = builder.try_init_root::<Message>()?;
                let mut result = root.field(Message::RETURN).init()?;
                result.scalar(Return::ANSWER_ID).set(17);
                let mut payload = result.field(Return::RESULTS).init()?;
                payload.field(Payload::CONTENT).set_capability_index(0)?;
                payload
                    .field(Payload::CAP_TABLE)
                    .init(1)?
                    .get_mut(0)
                    .unwrap()
                    .scalar(CapDescriptor::SENDER_PROMISE)
                    .set(promise_id.to_wire());
                let lease = super::rpc::seal(messages, &builder);
                publication::prepare(batch, messages, lease, &mut [0; 2], Default::default())
                    .unwrap()
                    .accept_into(queue.reserve().unwrap());
                let mut input = [ZERO; 65];
                let receiver = super::rpc::receive(&mut queue, messages, &mut input, chunk);
                let frame = receiver.message()?.unwrap();
                let rpc_capnp::message::Return(result) = frame
                    .get_root::<rpc_capnp::message::Reader<'_>>()?
                    .which()
                    .unwrap()
                else {
                    unreachable!()
                };
                let rpc_capnp::return_::Results(payload) = result?.which().unwrap() else {
                    unreachable!()
                };
                let mut initial = [imports::ReceiveSlot::new_promise(import_id, 47)];
                let received = reception::prepare(payload?, imports, &mut initial)
                    .unwrap()
                    .accept();
                let pointer = received
                    .content()
                    .get_as::<capnp::wire_capability::Reader<'_, ()>>()?;
                let observer = imports
                    .retain(received.capability(pointer).unwrap())
                    .unwrap();
                let original_key = observer.key();
                received.release(imports);
                let observer = if abandoned {
                    imports.release(observer);
                    assert_eq!(imports.pending_release(0).unwrap().commit(), 47);
                    None
                } else {
                    Some(observer)
                };

                // First reject the complete valid Resolve. The original promise
                // must remain pending and its replacement must remain unpublished.
                let mut replacement_id = None;
                for accepted in [false, true] {
                    builder.reset();
                    let mut batch = exports.batch();
                    batch.resolve_promise(promise_key).unwrap();
                    let replacement = batch.export(53).unwrap();
                    replacement_id = Some(replacement.id());
                    let mut root = builder.try_init_root::<Message>()?;
                    let mut resolve = root.field(Message::RESOLVE).init()?;
                    resolve
                        .scalar(Resolve::PROMISE_ID)
                        .set(promise_id.to_wire());
                    resolve
                        .field(Resolve::CAP)
                        .init()?
                        .scalar(CapDescriptor::SENDER_HOSTED)
                        .set(replacement.id().to_wire());
                    let lease = super::rpc::seal(messages, &builder);
                    let offered = publication::prepare(
                        batch,
                        messages,
                        lease,
                        &mut [0; 2],
                        Default::default(),
                    )
                    .unwrap();
                    if accepted {
                        offered.accept_into(queue.reserve().unwrap());
                    } else {
                        drop(offered);
                        assert_eq!(exports.take_retired(1), Some(53));
                    }
                }
                let replacement_id = replacement_id.unwrap();
                let mut input = [ZERO; 65];
                let receiver = super::rpc::receive(&mut queue, messages, &mut input, chunk);
                let frame = receiver.message()?.unwrap();
                let rpc_capnp::message::Resolve(resolve) = frame
                    .get_root::<rpc_capnp::message::Reader<'_>>()?
                    .which()
                    .unwrap()
                else {
                    unreachable!()
                };
                let mut receipts = [imports::ReceiveSlot::new(
                    ImportId::from_wire(replacement_id.to_wire()),
                    61,
                )];
                let received = inbound::resolve(resolve?, imports, &mut receipts)
                    .unwrap()
                    .accept();
                assert_eq!(
                    received.target(),
                    if abandoned { None } else { Some(original_key) }
                );
                assert_eq!(imports.get(received.replacement().unwrap()), &61);
                assert!(received.exception().is_none());
                received.release(imports);
                if let Some(observer) = observer {
                    assert!(imports.prepare_resolution(import_id, &mut []).is_err());
                    imports.release(observer);
                    let cleanup = imports.pending_release(0).unwrap();
                    assert_eq!(cleanup.count(), 1);
                    assert_eq!(cleanup.commit(), 47);
                }
                let cleanup = imports.pending_release(usize::from(!abandoned)).unwrap();
                assert_eq!(cleanup.count(), 1);
                assert_eq!(cleanup.commit(), 61);
                assert!(imports.is_empty());
                assert_eq!(exports.release(promise_id, 1), Ok(Some(41)));
                assert_eq!(exports.release(replacement_id, 1), Ok(Some(53)));
                assert!(exports.is_empty());
                Ok(())
            })
            .unwrap()
        })
        .unwrap()
    })
    .unwrap()
}
