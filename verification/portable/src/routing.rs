//! Actual framed calls and loopback control through the bounded dispatcher.
use super::*;
use capntproto_rpc_engine::{
    clients::{self, dispatch, Destination, Location, Source},
    embargoes,
    ids::{ImportId, WireId},
    messages,
    peer_table::{BorrowedSparse, Slot},
    questions,
};
use capntproto_rpc_wire::{admission, calls, inbound, rpc_capnp};
use portable_capnp::{api::echo::EchoParams, echo};

#[derive(Clone, Copy)]
enum Work {
    Call(u32),
    Echo,
}
/// Old pipelined calls, a reflected same-peer resolution, and its loopback all
/// share dispatch/output ordering. A late echo cannot open a newer barrier.
pub fn bounded_dispatch_ordering(chunk: usize) -> Result<()> {
    let mut clients = [const { clients::Slot::EMPTY }; 3];
    let mut tasks = [const { dispatch::TaskSlot::EMPTY }; 4];
    dispatch::with_runtime(dispatch::Storage { clients: &mut clients, tasks: &mut tasks }, |runtime| -> Result<()> {
        let mut embargo_slots = [Slot::EMPTY];
        let mut embargoes = embargoes::Embargoes::new(BorrowedSparse::new(&mut embargo_slots)).ok().unwrap();
        let mut question_slots = [const { questions::Slot::EMPTY }; 3];
        questions::with_questions(&mut question_slots, |questions| -> Result<()> {
            let mut output = [ZERO; 80];
            let mut frame_storage = [ZERO; 81];
            let mut input = [ZERO; 81];
            let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut frame_storage))];
            let mut sends = [messages::SendSlot::EMPTY];
            messages::with_messages(&mut buffers, &mut sends, |messages| -> Result<()> {
                let mut queue_slots = [None];
                let mut queue = messages::Queue::new(&mut queue_slots).unwrap();
                let mut builder = message::FixedBuilder::new(&mut output)?;
                let initial = runtime.endpoint(7u32, Location::Remote(1u8)).unwrap();
                let client = runtime.promise(&mut Some(initial)).unwrap();
                runtime.prepare_call(&client, &mut Some(Work::Call(1))).unwrap().accept();
                let replacement = runtime.endpoint(9, Location::Remote(1)).unwrap();
                let barrier = runtime.prepare_resolution(client.key(), &mut Some(replacement), Source::Reflected).unwrap().accept().unwrap();
                runtime.prepare_call(&client, &mut Some(Work::Call(2))).unwrap().accept();
                let target = runtime.retain_barrier_route(&barrier).unwrap();
                let mut barrier = Some(barrier);
                let mut echo_id = None;
                runtime.prepare_call(&target, &mut Some(Work::Echo)).unwrap().accept();
                runtime.release(target);
                let mut received_calls = [0u64; 3];
                let mut calls_count = 0;
                let mut echo_count = 0;
                while let Some(ready) = runtime.next_ready().unwrap() {
                    let work = *ready.owner();
                    drop(ready); // rejection is retryable, including after earlier partial writes
                    let ready = runtime.next_ready().unwrap().unwrap();
                    let Destination::Endpoint { owner, .. } = ready.destination() else { unreachable!() };
                    let target_id = *owner;
                    builder.reset();
                    let mut question = None;
                    match work {
                        Work::Call(value) => {
                            assert_eq!(target_id, if value == 1 { 7 } else { 9 });
                            let reservation = questions.reserve().unwrap();
                            calls::call(&mut builder, reservation.id(), ImportId::from_wire(target_id), echo::Owned::ECHO, |mut params| {
                                params.scalar(EchoParams::VALUE).set(u64::from(value)); Ok(())
                            })?;
                            let lease = super::rpc::seal(messages, &builder);
                            question = Some(admission::question(reservation, messages, lease, Default::default()).unwrap().accept(queue.reserve().unwrap()));
                        }
                        Work::Echo => {
                            let offer = embargoes.reserve(barrier.take().unwrap()).unwrap();
                            let id = offer.id();
                            echo_id = Some(id);
                            assert_eq!(target_id, 7);
                            let target = calls::Target::Imported(ImportId::from_wire(target_id));
                            calls::sender_loopback(&mut builder, id, target)?;
                            let lease = super::rpc::seal(messages, &builder);
                            admission::embargo(offer, target, messages, lease, Default::default()).unwrap().accept(queue.reserve().unwrap());
                        }
                    }
                    let running = ready.accept();
                    let receiver = super::rpc::receive(&mut queue, messages, &mut input, chunk);
                    let decoded = receiver.message()?.unwrap();
                    match decoded.get_root::<rpc_capnp::message::Reader<'_>>()?.which().unwrap() {
                        rpc_capnp::message::Call(call) => {
                            let call = calls::Incoming::new(call?);
                            let dispatch = call.dispatch(echo::Owned::ECHO)?;
                            received_calls[calls_count] = dispatch.params.value();
                            calls_count += 1;
                            let local = question.take().unwrap();
                            // A synthetic empty completion releases the test's question.
                            // Full generated Return reception is tested by rpc.rs.
                            questions.prepare_receive(questions.pipeline_id(&local).unwrap(), true).unwrap().accept();
                            questions.release(local);
                        }
                        rpc_capnp::message::Disembargo(message) => {
                            let id = echo_id.unwrap();
                            assert_eq!(calls_count, 1);
                            assert!(matches!(message?.get_context().which().unwrap(), rpc_capnp::disembargo::context::SenderLoopback(e) if e == id.to_wire()));
                            builder.reset();
                            calls::receiver_loopback(&mut builder, id.to_wire(), calls::Target::Imported(ImportId::from_wire(target_id)))?;
                            let echo_reader = message::Reader::new(builder.get_segments_for_output(), Default::default());
                            let rpc_capnp::message::Disembargo(echo) = echo_reader.get_root::<rpc_capnp::message::Reader<'_>>()?.which().unwrap() else { unreachable!() };
                            let echo = echo?;
                            let barrier = inbound::receiver_loopback(echo, &mut embargoes).unwrap().unwrap();
                            runtime.open(barrier);
                            runtime.prepare_call(&client, &mut Some(Work::Call(3))).unwrap().accept();
                            assert!(inbound::receiver_loopback(echo, &mut embargoes).is_err());
                            echo_count += 1;
                        }
                        _ => unreachable!(),
                    }
                    runtime.complete(running);
                }
                assert_eq!(received_calls, [1, 2, 3]);
                assert_eq!(calls_count, 3);
                assert_eq!(echo_count, 1);
                assert!(questions.is_empty());
                assert!(embargoes.is_empty());
                runtime.release(client);
                assert_eq!(runtime.take_retired(), Some(7));
                assert_eq!(runtime.take_retired(), Some(9));
                assert!(runtime.is_empty());
                Ok(())
            }).unwrap()
        }).unwrap()
    }).unwrap()
}
