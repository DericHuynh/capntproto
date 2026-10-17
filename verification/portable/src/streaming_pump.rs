//! Draining includes later acknowledgments after the first deferred error.
use super::{
    driver_invocation::{close, remote},
    portable_capnp::echo,
    ZERO,
};
use capnp_rpc::bounded::{
    calls, clients as binding,
    owned::{
        connection::{
            CallError, CallProgress, Driver, FixedStorage, InvocationSlot, Session, StreamingSlot,
        },
        Frame,
    },
    rpc_capnp::api::{CapDescriptor, Message, Payload, Return},
};
use capntproto_rpc_engine::{
    clients, completion, connection,
    ids::{AnswerId, ImportId, WireId},
    messages,
};
use core::{
    num::NonZeroUsize,
    task::{Context, Poll, Waker},
};

fn with_fixed_peer<'pool, const SLOTS: usize, R>(
    growing: u8,
    f: impl for<'id, 's> FnOnce(Driver<'pool, 'id, 's, (), u32, u8, u32, 4, 4>) -> R,
) -> R {
    connection::with_domain(|domain| {
        #[cfg(feature = "alloc")]
        if growing == 1 {
            let mut storage = FixedStorage::<(), u32, u8, u32, 4, 4, SLOTS, 0>::new();
            return f(storage.bind_growing_clients(domain, 1, 0, 12).unwrap());
        }
        #[cfg(feature = "alloc")]
        if growing == 2 {
            let mut storage = FixedStorage::<(), u32, u8, u32, 4, 4, SLOTS, 0, 0, 0>::new();
            let bound = connection::Growth {
                initial: 0,
                maximum: 4,
            };
            let bounds = connection::GrowingStorage {
                imports: bound,
                exports: bound,
                questions: bound,
                answers: bound,
                clients: connection::Growth {
                    initial: 0,
                    maximum: 12,
                },
            };
            return f(storage.bind_growing(domain, 1, bounds).unwrap());
        }
        assert_eq!(growing, 0);
        let mut storage = FixedStorage::<(), u32, u8, u32, 4, 4, SLOTS, 12>::new();
        f(storage.bind(domain, 1).unwrap())
    })
}

pub fn streaming_acknowledgment_pump(mode: u8) {
    run(mode, 0)
}

#[cfg(feature = "alloc")]
pub fn growing_streaming_acknowledgment_pump(mode: u8) {
    run(mode, 1)
}

#[cfg(feature = "alloc")]
pub fn growing_ledger_streaming_acknowledgment_pump(mode: u8) {
    run(mode, 2)
}

fn run(mode: u8, growing: u8) {
    let mut words = [[ZERO; 97]; 8];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut sends = [None];
        let mut queue = super::driver::message_queue(&mut sends).unwrap();
        with_fixed_peer::<4, _>(growing, |mut peer| {
            let remote_call = matches!(mode, 2 | 3 | 5);
            let target = if remote_call {
                remote(&mut peer, ImportId::from_wire(91))
            } else {
                peer.runtime
                    .endpoint(binding::Route::local(43), clients::Location::Local)
                    .unwrap()
            };
            let completions = [const { completion::Slot::new() }; 3];
            let mut bindings = [const { InvocationSlot::EMPTY }; 2];
            let calls = super::driver::invocations(&mut bindings).unwrap();
            let session = Session::new(peer, messages, calls, 776, Default::default());
            let mut cx = Context::from_waker(Waker::noop());
            let budget = NonZeroUsize::new(1).unwrap();
            let mut empty = [];
            let mut zero = session.streaming(&mut empty).unwrap();
            assert_eq!(
                zero.poll_drained(&mut cx, budget).unwrap(),
                Poll::Ready(Ok(()))
            );
            assert_eq!(
                zero.call(
                    &completions[0],
                    &target,
                    echo::Owned::WRITE,
                    &[],
                    |_| panic!("zero capacity filler")
                ),
                Err(CallError::Busy)
            );
            drop(zero);
            let mut slots = [const { StreamingSlot::EMPTY }; 2];
            let mut stream = session.streaming(&mut slots).unwrap();
            let first = stream
                .call(
                    &completions[0],
                    &target,
                    echo::Owned::WRITE,
                    &[],
                    |_| Ok(()),
                )
                .unwrap();
            let second = stream
                .call(
                    &completions[1],
                    &target,
                    echo::Owned::WRITE,
                    &[],
                    |_| Ok(()),
                )
                .unwrap();
            assert_eq!(stream.pending(), 2);
            assert_eq!(stream.capacity(), 2);
            assert_eq!(
                stream.call(
                    &completions[2],
                    &target,
                    echo::Owned::WRITE,
                    &[],
                    |_| panic!("full capacity filler")
                ),
                Err(CallError::Busy)
            );
            assert!(stream.poll_drained(&mut cx, budget).unwrap().is_pending());
            assert!(stream.poll_drained(&mut cx, budget).unwrap().is_pending());
            let failed = matches!(mode, 1 | 3);
            if mode == 4 {
                drop(stream);
                session
                    .access(|driver, messages, calls| {
                        for index in 0..2 {
                            assert_eq!(
                                calls.poll_one(index, driver, messages, &mut cx).unwrap(),
                                CallProgress::Canceled
                            );
                        }
                    })
                    .unwrap();
            } else {
                for (index, task) in [first, second].into_iter().enumerate() {
                    session
                        .access(|driver, messages, calls| {
                            if remote_call {
                                let sent = calls
                                    .send_one(
                                        driver,
                                        messages,
                                        &mut queue,
                                        776,
                                        Default::default(),
                                        &mut [0; 4],
                                    )
                                    .unwrap()
                                    .unwrap();
                                assert_eq!(sent.task, task);
                                super::owning::exchange(messages, &mut queue, 1).release(messages);
                                let frame =
                                    Frame::build(messages, 776, Default::default(), |builder| {
                                        let id = AnswerId::from_wire(sent.question.id().to_wire());
                                        if failed && index == 0 {
                                            calls::exception(
                                                builder,
                                                id,
                                                capnp::ErrorKind::Overloaded,
                                                "stream rejected",
                                            )
                                        } else {
                                            calls::acknowledge(builder, id)?;
                                            // Even an unexpected result capability must be reclaimed.
                                            let root = builder.try_get_root::<Message>()?;
                                            let result = root.into_field(Message::RETURN).edit()?;
                                            let mut caps = result
                                                .into_field(Return::RESULTS)
                                                .edit()?
                                                .into_field(Payload::CAP_TABLE)
                                                .init(1)?;
                                            caps.get_mut(0)
                                                .unwrap()
                                                .scalar(CapDescriptor::SENDER_HOSTED)
                                                .set(117);
                                            Ok(())
                                        }
                                    })
                                    .unwrap();
                                driver
                                    .receive(
                                        frame,
                                        messages,
                                        &mut [0; 4],
                                        |_| (),
                                        || panic!("Return producer"),
                                    )
                                    .unwrap_or_else(|_| panic!("ack admission"));
                            } else {
                                let call = calls.accept_local(driver).unwrap().unwrap();
                                assert_eq!(call.task(), task);
                                let result = if failed && index == 0 {
                                    Err(capnp::ErrorKind::Overloaded)
                                } else {
                                    Ok(driver
                                        .local_ack(
                                            echo::Owned::WRITE,
                                            messages,
                                            776,
                                            Default::default(),
                                        )
                                        .unwrap())
                                };
                                calls
                                    .complete_local(driver, messages, call, result)
                                    .unwrap_or_else(|_| panic!("complete ack"));
                            }
                            assert_eq!(
                                calls.poll_one(index, driver, messages, &mut cx).unwrap(),
                                CallProgress::Delivered
                            );
                        })
                        .unwrap();
                    if mode == 5 {
                        continue;
                    }
                    if mode == 6 {
                        session
                            .access(|_, _, _| {
                                assert_eq!(
                                    stream.poll_drained(&mut cx, budget),
                                    Err(CallError::Busy)
                                )
                            })
                            .unwrap();
                        assert_eq!(
                            stream.pending(),
                            2 - index,
                            "busy polling cannot consume a reply"
                        );
                    }
                    let mut drained = Poll::Pending;
                    for _ in 0..2 {
                        drained = stream.poll_drained(&mut cx, budget).unwrap();
                    }
                    assert_eq!(stream.pending(), 1 - index);
                    if index == 0 {
                        assert!(
                            drained.is_pending(),
                            "a failed ack cannot complete drain before later work"
                        );
                        if failed {
                            assert_eq!(
                                stream.failure(),
                                Some(CallError::Failed(capnp::ErrorKind::Overloaded))
                            );
                            assert_eq!(
                                stream.call(
                                    &completions[2],
                                    &target,
                                    echo::Owned::WRITE,
                                    &[],
                                    |_| panic!("failed stream filler")
                                ),
                                Err(CallError::Failed(capnp::ErrorKind::Overloaded))
                            );
                        }
                    } else {
                        assert_eq!(
                            drained,
                            Poll::Ready(if failed {
                                Err(CallError::Failed(capnp::ErrorKind::Overloaded))
                            } else {
                                Ok(())
                            })
                        );
                    }
                    session
                        .access(|driver, messages, calls| {
                            assert_eq!(
                                calls.poll_one(index, driver, messages, &mut cx).unwrap(),
                                CallProgress::Consumed
                            );
                        })
                        .unwrap();
                }
                drop(stream);
                if mode == 5 {
                    session
                        .access(|driver, messages, calls| {
                            for index in 0..2 {
                                assert_eq!(
                                    calls.poll_one(index, driver, messages, &mut cx).unwrap(),
                                    CallProgress::Reclaimed
                                );
                            }
                        })
                        .unwrap();
                }
            }
            // Ordinary Drop clears the borrowed registry and permits reuse.
            let mut stream = session.streaming(&mut slots).unwrap();
            assert_eq!(stream.pending(), 0);
            assert_eq!(
                stream.poll_drained(&mut cx, budget).unwrap(),
                Poll::Ready(Ok(()))
            );
            drop(stream);
            for slot in &completions {
                drop(slot.try_pair().unwrap());
            }
            let (mut peer, messages, calls) = session.into_parts();
            assert!(calls.is_empty());
            assert_eq!(calls.terminate(&mut peer, messages), 0);
            peer.runtime.release(target);
            assert_eq!(
                close(peer, messages, &mut queue),
                if remote_call { 0 } else { 43 }
            );
            let writers = core::array::from_fn::<_, 8, _>(|_| messages.reserve(776).unwrap());
            for writer in writers {
                messages.discard(writer);
            }
        });
    })
    .unwrap();
}

/// Growth after the driver caches an idle scan must expose every new cleanup
/// slot. The IO queue stays bounded to one frame throughout this test.
#[cfg(feature = "alloc")]
pub fn growing_ledger_cleanup() {
    use capntproto_rpc_wire::owned::{answer, connection::Progress, pipelines};
    let mut words = [[ZERO; 97]; 8];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut sends = [None];
        let mut queue = super::driver::message_queue(&mut sends).unwrap();
        with_fixed_peer::<9, _>(2, |mut peer| {
            // Exceed the eight-slot vacancy certificate to leave the general
            // maintenance cursor part way through an empty scan. Ledger growth
            // and the one-frame output bound remain the fault under test.
            assert!(matches!(
                peer.poll_maintenance(
                    messages,
                    &mut queue,
                    776,
                    Default::default(),
                    &mut [0; 4],
                    NonZeroUsize::new(7).unwrap()
                )
                .unwrap(),
                Progress::Yield
            ));
            for n in 0..4 {
                let imported = peer
                    .ledgers
                    .imports
                    .admit(ImportId::from_wire(n), ())
                    .unwrap();
                peer.ledgers.imports.release(imported);
                let server = peer
                    .runtime
                    .endpoint(binding::Route::local(n + 1), clients::Location::Local)
                    .unwrap();
                // An uncommitted export retains its owner for explicit draining.
                let _ = peer.ledgers.exports.batch().export(server).ok().unwrap();
                let id = AnswerId::from_wire(n);
                let key = peer
                    .ledgers
                    .answers
                    .admit(id, answer::State::new(pipelines::Results::new(n + 1)))
                    .ok()
                    .unwrap();
                let retained = peer.ledgers.answers.pipeline(id).unwrap();
                assert!(peer
                    .ledgers
                    .answers
                    .prepare_return(key)
                    .unwrap()
                    .accept()
                    .is_none());
                assert!(peer.ledgers.answers.finish(id, false).retired.is_none());
                peer.ledgers.answers.release_pipeline_deferred(retained);
            }
            let (mut imports, mut producers, mut servers, mut releases) = (0, 0, 0, 0);
            let mut idle = false;
            for _ in 0..400 {
                match peer
                    .poll_maintenance(
                        messages,
                        &mut queue,
                        776,
                        Default::default(),
                        &mut [0; 4],
                        NonZeroUsize::new(1).unwrap(),
                    )
                    .unwrap()
                {
                    Progress::Import(()) => imports += 1,
                    Progress::Producer(n) => producers += n,
                    Progress::Server(n) => servers += n,
                    Progress::Advanced | Progress::Yield => (),
                    Progress::Idle => {
                        idle = true;
                        break;
                    }
                }
                if !queue.is_empty() {
                    let frame = super::owning::exchange(messages, &mut queue, 7);
                    frame
                        .with(messages, |root| {
                            let capntproto_rpc_wire::rpc_capnp::message::Release(release) =
                                root.which().unwrap()
                            else {
                                panic!("expected Release")
                            };
                            let release = release.unwrap();
                            releases |= 1 << release.get_id();
                            assert_eq!(release.get_reference_count(), 1);
                        })
                        .unwrap();
                    frame.release(messages);
                }
            }
            assert!(idle, "growing ledgers eventually become idle");
            assert_eq!(imports, 4, "maintenance visits grown import slots");
            assert_eq!(producers, 10, "maintenance visits grown answer slots");
            assert_eq!(servers, 10, "maintenance visits grown export slots");
            assert_eq!(releases, 15);
            assert!(peer.ledgers.imports.is_empty());
            assert!(peer.ledgers.exports.is_empty());
            assert!(peer.ledgers.answers.is_empty());
            assert_eq!(close(peer, messages, &mut queue), 0);
        });
        let writers = core::array::from_fn::<_, 8, _>(|_| messages.reserve(776).unwrap());
        for writer in writers {
            messages.discard(writer);
        }
    })
    .unwrap();
}

/// A maintenance scan started before bindings grew must visit every new slot.
/// Dropping the observer does not remove its retained answer until maintenance.
#[cfg(feature = "growing")]
pub fn growing_binding_cleanup() {
    use capntproto_rpc_wire::owned::{answer, connection::Progress, pipelines};
    let mut words = [[ZERO; 97]; 2];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut sends = [None];
        let mut queue = super::driver::message_queue(&mut sends).unwrap();
        super::driver::with_owned_peer(|mut peer| {
            assert!(matches!(
                peer.poll_maintenance(
                    messages,
                    &mut queue,
                    776,
                    Default::default(),
                    &mut [0; 4],
                    NonZeroUsize::new(3).unwrap()
                )
                .unwrap(),
                Progress::Idle
            ));
            for n in 0..4 {
                let id = AnswerId::from_wire(n);
                let key = peer
                    .ledgers
                    .answers
                    .admit(id, answer::State::new(pipelines::Results::new(n + 1)))
                    .ok()
                    .unwrap();
                let answer = peer.ledgers.answers.pipeline(id).unwrap();
                let observer = peer
                    .pipelines
                    .bind(
                        &mut peer.runtime,
                        &mut peer.ledgers.answers,
                        &answer,
                        capntproto_rpc_wire::pipeline::Path::new(&[]),
                    )
                    .unwrap();
                peer.ledgers.answers.release_pipeline_deferred(answer);
                peer.runtime.release(observer);
                assert!(peer
                    .ledgers
                    .answers
                    .prepare_return(key)
                    .unwrap()
                    .accept()
                    .is_none());
                assert!(peer.ledgers.answers.finish(id, false).retired.is_none());
                if n == 0 {
                    // Growth invalidates the empty-ledger idle certificate.
                    // Suspend the new scan before the other bindings grow, so
                    // both idle-to-active and mid-scan growth are exercised.
                    assert!(matches!(
                        peer.poll_maintenance(
                            messages,
                            &mut queue,
                            776,
                            Default::default(),
                            &mut [0; 4],
                            NonZeroUsize::new(1).unwrap()
                        )
                        .unwrap(),
                        Progress::Yield
                    ));
                }
            }
            assert_eq!(peer.pipelines.capacity(), 4);
            let mut producers = 0;
            let mut idle = false;
            for _ in 0..400 {
                match peer
                    .poll_maintenance(
                        messages,
                        &mut queue,
                        776,
                        Default::default(),
                        &mut [0; 4],
                        NonZeroUsize::new(1).unwrap(),
                    )
                    .unwrap()
                {
                    Progress::Producer(n) => producers += n,
                    Progress::Advanced | Progress::Yield => (),
                    Progress::Idle => {
                        idle = true;
                        break;
                    }
                    _ => panic!("unexpected cleanup owner"),
                }
            }
            assert!(idle);
            assert_eq!(
                producers, 10,
                "maintenance visits every grown pipeline observer"
            );
            assert!(peer.ledgers.answers.is_empty());
            assert_eq!(close(peer, messages, &mut queue), 0);
        });
    })
    .unwrap();
}
