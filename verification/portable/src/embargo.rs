use super::*;
use capntproto_rpc_engine::{
    embargoes,
    ids::{ImportId, WireId},
    messages,
    peer_table::{BorrowedSparse, Slot},
};
use capntproto_rpc_wire::{admission, calls, inbound, rpc_capnp};

/// An accepted senderLoopback survives partial writes; only its own decoded
/// echo detaches the waiter. A delayed duplicate cannot open the next barrier.
pub fn bounded_loopback_ordering() -> Result<()> {
    let mut slots = [Slot::EMPTY];
    let mut ledger = embargoes::Embargoes::new(BorrowedSparse::new(&mut slots))
        .ok()
        .unwrap();
    let mut source = [ZERO; 40];
    let mut output = [ZERO; 41];
    let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut output))];
    let mut sends = [messages::SendSlot::EMPTY];
    messages::with_messages(&mut buffers, &mut sends, |messages| -> Result<()> {
        let mut builder = message::FixedBuilder::new(&mut source)?;
        let target = calls::Target::Imported(ImportId::from_wire(7));
        let mut queue_slots = [None];
        let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
        let mut previous = None;
        for owner in [41, 43] {
            builder.reset();
            let pending = ledger.reserve(owner).unwrap();
            let id = pending.id();
            calls::sender_loopback(&mut builder, id, target)?;
            let lease = super::rpc::seal(messages, &builder);
            admission::embargo(pending, target, messages, lease, Default::default())
                .unwrap()
                .accept(queue.reserve().unwrap());
            let mut input = [ZERO; 41];
            let receiver = super::rpc::receive(&mut queue, messages, &mut input, 1);
            let frame = receiver.message()?.unwrap();
            let rpc_capnp::message::Disembargo(barrier) = frame
                .get_root::<rpc_capnp::message::Reader<'_>>()?
                .which()
                .unwrap()
            else {
                unreachable!()
            };
            let rpc_capnp::disembargo::context::SenderLoopback(echo) =
                barrier?.get_context().which().unwrap()
            else {
                unreachable!()
            };
            assert_eq!(echo, id.to_wire());
            for echoed in previous.into_iter().chain(core::iter::once(echo)) {
                builder.reset();
                calls::receiver_loopback(&mut builder, echoed, target)?;
                let reader =
                    message::Reader::new(builder.get_segments_for_output(), Default::default());
                let rpc_capnp::message::Disembargo(barrier) = reader
                    .get_root::<rpc_capnp::message::Reader<'_>>()?
                    .which()
                    .unwrap()
                else {
                    unreachable!()
                };
                let result = inbound::receiver_loopback(barrier?, &mut ledger);
                if echoed == echo {
                    assert_eq!(result, Ok(Some(owner)));
                } else {
                    assert!(result.is_err());
                    assert_eq!(ledger.len(), 1);
                }
            }
            previous = Some(echo);
            assert!(ledger.is_empty());
        }
        Ok(())
    })
    .unwrap()
}
